use chrono::Utc;

use crate::bus::IncomingEvent;
use crate::messages::processing::{
    processing_members, AutomaticResult, ProcessingHistory, ProcessingOutcome, Understanding,
    UnderstandingSource,
};
use crate::messages::MessageRecord;
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::rule_parser::RuleParser;
use crate::settings::AppConfig;

use super::{llm_parse::BareSubitemOutcome, looks_like_order, Pipeline, RULE_CONFIDENCE};

pub(super) struct ProcessingContext {
    pub record: MessageRecord,
    pub understanding: Understanding,
    pub configuration_revision: u64,
    pub manual_phase: Option<crate::round::RoundPhase>,
    pub members: Vec<crate::messages::processing::ProcessingMember>,
}

impl ProcessingContext {
    pub fn completed_record(&self, outcome: ProcessingOutcome) -> MessageRecord {
        let mut record = self.record.clone();
        record.status = outcome.status.clone();
        record.detail = outcome.detail.clone();
        record
            .processing
            .get_or_insert_with(ProcessingHistory::default)
            .results
            .push(AutomaticResult {
                format_version: 1,
                created_at_ms: Utc::now().timestamp_millis(),
                result_id: uuid::Uuid::new_v4().to_string(),
                configuration_revision: self.configuration_revision,
                members: self.members.clone(),
                understanding: self.understanding.clone(),
                original_outcome: outcome,
                manual_phase_at_ingest: self.manual_phase,
            });
        record
    }
}

impl Pipeline {
    pub(super) async fn processing_context(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
    ) -> ProcessingContext {
        let pending = if let Some(source) = &ev.ingress {
            self.state.lock().await.ingress_pending.get(source).cloned()
        } else {
            None
        };
        let record = pending.unwrap_or_else(|| MessageRecord {
            record_id: crate::messages::new_record_id(),
            record_revision: 1,
            seq: crate::messages::next_seq(),
            group_id: ev.group_id.clone(),
            user_id: ev.user_id.clone(),
            nickname: ev.nickname.clone(),
            message_id: ev.message_id.clone(),
            text: ev.text.clone(),
            timestamp_ms: ev.timestamp_ms,
            is_admin: ev.is_admin,
            routed: "message".to_string(),
            status: "Pending".to_string(),
            detail: String::new(),
            sender_user_id: ev.sender_user_id.clone(),
            ingress: ev.ingress.clone(),
            sender_nickname: ev.sender_nickname.clone(),
            processing: Some(ProcessingHistory::default()),
        });
        let manual_phase =
            crate::replay::session::load_manual_phase(&self.messages, &cfg.round.round_id)
                .await
                .or(if self.is_locked().await {
                    Some(crate::round::RoundPhase::Settling)
                } else {
                    None
                });
        ProcessingContext {
            members: processing_members(&[&record]),
            record,
            understanding: Understanding::terminal(
                UnderstandingSource::Rules,
                "Pending",
                "处理中",
                None,
            ),
            configuration_revision: cfg.revision,
            manual_phase,
        }
    }

    pub(super) async fn understand(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
        context: &ProcessingContext,
        group: bool,
    ) -> Understanding {
        let items = cfg.round.to_items();
        let rule = RuleParser::parse(&ev.text, &items, ev.is_admin);
        let force_llm = (group || cfg.llm.segment_claims) && looks_like_order(&ev.text, &rule);
        let (mut parsed, mut source) = if cfg.llm.enabled
            && (force_llm
                || rule.confidence < RULE_CONFIDENCE
                || rule.intent == ParsedIntent::Unknown)
        {
            match self.llm_parse(cfg, ev).await {
                Ok(parsed) => (parsed, UnderstandingSource::Llm),
                Err(error)
                    if cfg.llm.fallback_to_rules || (cfg.llm.segment_claims && force_llm) =>
                {
                    tracing::warn!("LLM 解析失败，回退规则：{error}");
                    (rule, UnderstandingSource::RulesFallback)
                }
                Err(_) => {
                    return Understanding::terminal(
                        UnderstandingSource::Llm,
                        "Rejected",
                        "没识别成功",
                        Some("没识别成功".to_string()),
                    )
                }
            }
        } else if rule.confidence >= RULE_CONFIDENCE && rule.intent != ParsedIntent::Unknown
            || cfg.llm.fallback_to_rules
        {
            (rule, UnderstandingSource::Rules)
        } else {
            return Understanding::terminal(
                UnderstandingSource::Control,
                "Rejected",
                "LLM 未启用",
                Some("没识别成功".to_string()),
            );
        };

        if matches!(parsed.intent, ParsedIntent::Claim | ParsedIntent::Modify) {
            let original_intent = parsed.intent.clone();
            parsed.intent = ParsedIntent::Claim;
            let original_parsed = parsed.clone();
            let rounds = vec![crate::domain::item::RoundContext {
                round_id: crate::domain::ids::RoundId(cfg.round.round_id.clone()),
                title: cfg.round.title.clone(),
                items,
            }];
            let now = chrono::DateTime::<Utc>::from_timestamp_millis(ev.timestamp_ms)
                .unwrap_or_else(Utc::now);
            match self
                .resolve_bare_subitem_claims(cfg, ev, parsed, &rounds, now, context.record.seq)
                .await
            {
                BareSubitemOutcome::Resolved(p, used_llm) => {
                    parsed = *p;
                    parsed.intent = original_intent;
                    if used_llm {
                        source = UnderstandingSource::FirstMatchLlm;
                    }
                }
                failure => {
                    if cfg.llm.enabled {
                        source = UnderstandingSource::FirstMatchLlm;
                    }
                    let (status, detail) = match failure {
                        BareSubitemOutcome::FallbackIgnore => {
                            ("Ignored", "商品未能 first-match，未记录")
                        }
                        _ => ("Rejected", "没识别成功"),
                    };
                    let mut result =
                        Understanding::terminal(source, status, detail, Some(detail.to_string()));
                    let mut original_parsed = original_parsed;
                    original_parsed.intent = original_intent;
                    result.parsed = Some(original_parsed);
                    return result;
                }
            }
        }
        crate::replay::session::prepare_understanding(cfg, &context.record, parsed, source).await
    }

    pub(super) async fn persist_understanding(
        &self,
        round_id: &str,
        context: &ProcessingContext,
        outcome: ProcessingOutcome,
    ) -> anyhow::Result<MessageRecord> {
        let record = context.completed_record(outcome);
        let resumed = if let Some(source) = &record.ingress {
            self.state.lock().await.ingress_pending.contains_key(source)
        } else {
            false
        };
        if resumed {
            self.messages
                .update(round_id, &record.record_id, |pending| {
                    *pending = record.clone()
                })
                .await?
                .ok_or_else(|| anyhow::anyhow!("待恢复消息缺失"))?;
        } else {
            self.messages.append(round_id, &record).await?;
        }
        self.mark_ingress(&record).await;
        Ok(record)
    }
}
