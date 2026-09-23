use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::warn;

use crate::bus::{EventSink, IncomingEvent, PipelineOutcome};
use crate::domain::event::DomainEvent;
use crate::domain::ids::{RoundId, UserId};
use crate::domain::item::RoundContext;
use crate::engine::replay::{describe_event, rebuild_allocation_snapshot};
use crate::messages::{MessageLog, MessageRecord as LogMessageRecord};
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::policy;
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidateContext, ValidationOutcome};
use crate::settings::ConfigStore;

use super::client::{LlmClient, OpenAiClient};

mod admin;
mod export;
mod llm_parse;
mod state;

#[cfg(test)]
mod tests;

use admin::{cancel_for_modify, priority_eligibility};
use state::{MessageRecord, State};

const RULE_CONFIDENCE: f32 = 0.9;
const CONFIDENCE_THRESHOLD: f32 = 0.65;

pub struct Pipeline {
    cfg: Arc<ConfigStore>,
    llm: Arc<dyn LlmClient>,
    state: Mutex<State>,
    messages: Arc<MessageLog>,
}

impl Pipeline {
    #[allow(dead_code)]
    pub fn new(cfg: Arc<ConfigStore>) -> Arc<Self> {
        Self::new_with_client(cfg, Arc::new(OpenAiClient::new()))
    }

    #[allow(dead_code)]
    pub fn new_with_client(cfg: Arc<ConfigStore>, llm: Arc<dyn LlmClient>) -> Arc<Self> {
        Self::build(cfg, llm, None)
    }

    /// 指定消息日志目录（测试隔离用）；生产默认 `PAIGU_MESSAGES_DIR` / `data/messages`。
    #[cfg(test)]
    pub fn new_with_client_dir(
        cfg: Arc<ConfigStore>,
        llm: Arc<dyn LlmClient>,
        messages_dir: impl Into<std::path::PathBuf>,
    ) -> Arc<Self> {
        let dir: std::path::PathBuf = messages_dir.into();
        let log = MessageLog::new(dir.clone(), dir.join("events"));
        Self::build(cfg, llm, Some(log))
    }

    /// 注入共享消息日志（T-05）。
    pub fn new_with_messages(
        cfg: Arc<ConfigStore>,
        llm: Arc<dyn LlmClient>,
        messages: Arc<MessageLog>,
    ) -> Arc<Self> {
        Self::build(cfg, llm, Some(messages))
    }

    fn build(
        cfg: Arc<ConfigStore>,
        llm: Arc<dyn LlmClient>,
        messages: Option<Arc<MessageLog>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            llm,
            state: Mutex::new(State::default()),
            messages: messages.unwrap_or_else(MessageLog::from_env),
        })
    }

    /// 共享消息日志句柄（Gateway / API 复用同一实例）。
    #[allow(dead_code)]
    pub fn messages(&self) -> Arc<MessageLog> {
        self.messages.clone()
    }

    pub async fn process(&self, ev: IncomingEvent) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        // C-3：保留成员原始事件（WS 路径由 Gateway 落盘；此处覆盖 API/脚本注入路径）
        if let Some(raw) = &ev.raw {
            let round_id = cfg.round.round_id.clone();
            if let Err(e) = self.messages.append_raw_event(&round_id, raw).await {
                warn!("原始事件写入失败: {e}");
            }
        }
        let (identity, display_raw) = crate::parser::normalize::clean_nickname(&ev.nickname);
        let display = if display_raw.trim().is_empty() {
            ev.user_id.clone()
        } else {
            display_raw
        };

        let seq = {
            let mut state = self.state.lock().await;
            let key = format!("{}::{}", ev.group_id, ev.message_id);
            if !state.seen.insert(key) {
                let detail = "重复 message_id，已按幂等忽略";
                let seq = state.seq;
                state.messages.push(MessageRecord {
                    seq,
                    display: display.clone(),
                    text: ev.text.clone(),
                    status: "Duplicate".to_string(),
                    detail: detail.to_string(),
                });
                let version = state.version;
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .and_then(|s| serde_json::to_value(s).ok());
                drop(state);
                self.persist(&ev, "Duplicate", detail, "message").await;
                return PipelineOutcome {
                    status: "Duplicate".to_string(),
                    detail: detail.to_string(),
                    reply: None,
                    version,
                    snapshot,
                };
            }
            state.seq += 1;
            state.display.insert(ev.user_id.clone(), display.clone());
            state.identity.insert(ev.user_id.clone(), identity.clone());
            state.seq
        };

        let text = ev.text.trim().to_string();

        if text.is_empty() {
            return self
                .finish(&ev, &display, seq, "Ignored", "空消息", None)
                .await;
        }

        if text.starts_with('/') {
            if !ev.is_admin {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Rejected",
                        "非管理员斜杠命令",
                        Some("此命令仅管理员可用。".to_string()),
                    )
                    .await;
            }
            let cmd = text.trim_start_matches('/').trim().to_string();
            if !cfg.gateway.admin_commands_enabled {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Applied",
                        "管理员命令已记录（执行开关关闭）",
                        Some("管理员命令已记录（执行开关关闭）".to_string()),
                    )
                    .await;
            }
            return self.run_admin_command(&ev, &display, seq, &cmd).await;
        }

        let items = cfg.round.to_items();
        let rule = RuleParser::parse(&text, &items, ev.is_admin);

        let parsed = if rule.confidence >= RULE_CONFIDENCE && rule.intent != ParsedIntent::Unknown {
            rule
        } else if cfg.llm.enabled {
            match self.llm_parse(&cfg, &ev).await {
                Ok(parsed) => parsed,
                Err(e) => {
                    if cfg.llm.fallback_to_rules {
                        warn!("LLM 解析失败，回退规则解析器: {e}");
                        rule
                    } else {
                        return self
                            .finish(
                                &ev,
                                &display,
                                seq,
                                "Rejected",
                                "没识别成功",
                                Some("没识别成功".to_string()),
                            )
                            .await;
                    }
                }
            }
        } else if cfg.llm.fallback_to_rules {
            rule
        } else {
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Rejected",
                    "LLM 未启用",
                    Some("没识别成功".to_string()),
                )
                .await;
        };

        let is_modify = parsed.intent == ParsedIntent::Modify;
        let mut parsed = parsed;
        if is_modify {
            // 改单（D-2）：按新数量重排；应用时先撤销本人该商品的既有认购
            parsed.intent = ParsedIntent::Claim;
        }

        if parsed.intent == ParsedIntent::Claim
            && parsed.items.is_empty()
            && parsed.ambiguous_parts.is_empty()
        {
            return self
                .finish(&ev, &display, seq, "Ignored", "未解析出商品", None)
                .await;
        }

        let round_id = RoundId(cfg.round.round_id.clone());
        let round_contexts = vec![RoundContext {
            round_id: round_id.clone(),
            title: cfg.round.title.clone(),
            items: items.clone(),
        }];
        let now = DateTime::<Utc>::from_timestamp_millis(ev.timestamp_ms).unwrap_or_else(Utc::now);
        let user_id = UserId(ev.user_id.clone());
        let validator = EventValidator::new(CONFIDENCE_THRESHOLD);

        let event = match validator
            .validate(
                parsed,
                ValidateContext {
                    user_id: &user_id,
                    group_id: &ev.group_id,
                    raw_message_id: Some(ev.message_id.clone()),
                    active_rounds: &round_contexts,
                    now,
                    sequence: seq,
                },
            )
            .await
        {
            Ok(ValidationOutcome::Ok(event)) => *event,
            Ok(ValidationOutcome::NeedConfirm(reply)) => {
                let reply = reply.text_content().map(str::to_string);
                return self
                    .finish(&ev, &display, seq, "NeedConfirm", "需要确认", reply)
                    .await;
            }
            Ok(ValidationOutcome::Reject(reply)) => {
                let reply = reply.text_content().map(str::to_string);
                return self
                    .finish(&ev, &display, seq, "Rejected", "校验拒绝", reply)
                    .await;
            }
            Ok(ValidationOutcome::Ignore) => {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Ignored",
                        "无法识别为排谷/撤销意图",
                        None,
                    )
                    .await;
            }
            Err(e) => {
                return self
                    .finish(&ev, &display, seq, "Error", &format!("处理失败: {e}"), None)
                    .await;
            }
        };

        let is_priority = policy::is_priority(
            &cfg,
            &[
                ev.user_id.as_str(),
                ev.nickname.as_str(),
                identity.as_str(),
                display.as_str(),
            ],
        );
        if policy::in_priority_at(&cfg, ev.timestamp_ms) && !is_priority {
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Rejected",
                    "优先时段仅限预存(购物金)用户",
                    Some("优先时段仅限预存(购物金)用户，请求已拒绝".to_string()),
                )
                .await;
        }

        if !cfg.round.phases.is_empty() {
            if let Some(phase) = policy::phase_at(&cfg.round.phases, ev.timestamp_ms) {
                if let Some(detail) = policy::phase_rejection(&cfg, &event, phase, is_priority) {
                    let reply = detail.clone();
                    return self
                        .finish(&ev, &display, seq, "Rejected", &detail, Some(reply))
                        .await;
                }
            }
        }

        {
            let st = self.state.lock().await;
            if st.locked {
                drop(st);
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Rejected",
                        "已锁定，不再接受排/撤/改",
                        Some("已锁定，不再接受排/撤/改".to_string()),
                    )
                    .await;
            }
        }

        let mut detail = describe_event(&event);
        if is_modify {
            detail = format!("改单：{detail}");
        }
        let (version, snapshot_value, reply) = {
            let mut state = self.state.lock().await;
            if is_priority
                && !state
                    .eligibilities
                    .iter()
                    .any(|e| e.user_id.0 == ev.user_id)
            {
                state
                    .eligibilities
                    .push(priority_eligibility(&round_id, &ev.user_id));
            }
            if is_modify {
                if let DomainEvent::ClaimCreated(created) = &event.payload {
                    let targets: Vec<_> = created.items.iter().map(|l| l.item_id.clone()).collect();
                    for item_id in targets {
                        state
                            .events
                            .push(cancel_for_modify(&round_id, &ev, seq, now, item_id));
                    }
                }
            }
            state.events.push(event);
            let snapshot = rebuild_allocation_snapshot(&items, &state.events, &state.eligibilities);
            let version = snapshot.version;
            let snapshot_value = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
            state.version = version;
            state.snapshot = Some(snapshot);
            state.messages.push(MessageRecord {
                seq,
                display: display.clone(),
                text: ev.text.clone(),
                status: "Applied".to_string(),
                detail: detail.clone(),
            });
            (
                version,
                snapshot_value,
                format!("已记录，当前版本 #{}", version),
            )
        };

        self.persist(&ev, "Applied", &detail, "message").await;

        PipelineOutcome {
            status: "Applied".to_string(),
            detail,
            reply: Some(reply),
            version,
            snapshot: Some(snapshot_value),
        }
    }

    /// 持久化一条入站消息日志（`routed=message`，`status` 记处理结果）。
    async fn persist(&self, ev: &IncomingEvent, status: &str, detail: &str, routed: &str) {
        let round_id = self.cfg.get().await.round.round_id.clone();
        let rec = LogMessageRecord {
            seq: crate::messages::next_seq(),
            group_id: ev.group_id.clone(),
            user_id: ev.user_id.clone(),
            nickname: ev.nickname.clone(),
            message_id: ev.message_id.clone(),
            text: ev.text.clone(),
            timestamp_ms: ev.timestamp_ms,
            is_admin: ev.is_admin,
            routed: routed.to_string(),
            status: status.to_string(),
            detail: detail.to_string(),
        };
        if let Err(e) = self.messages.append(&round_id, &rec).await {
            warn!("消息日志写入失败: {e}");
        }
    }

    async fn finish(
        &self,
        ev: &IncomingEvent,
        display: &str,
        seq: i64,
        status: &str,
        detail: &str,
        reply: Option<String>,
    ) -> PipelineOutcome {
        self.persist(ev, status, detail, "message").await;
        let mut state = self.state.lock().await;
        state.messages.push(MessageRecord {
            seq,
            display: display.to_string(),
            text: ev.text.clone(),
            status: status.to_string(),
            detail: detail.to_string(),
        });
        PipelineOutcome {
            status: status.to_string(),
            detail: detail.to_string(),
            reply,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        }
    }
}

#[async_trait]
impl EventSink for Pipeline {
    async fn handle(&self, ev: IncomingEvent) {
        self.process(ev).await;
    }

    fn messages(&self) -> Arc<MessageLog> {
        self.messages.clone()
    }
}


