use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::bus::{IncomingEvent, PipelineOutcome};
use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::ids::{EligibilityId, RoundId, UserId};

use super::state::MessageRecord;
use super::Pipeline;

impl Pipeline {
    /// 管理员斜杠命令**落地执行**（D-1，仅在 `gateway.admin_commands_enabled` 开启时调用）。
    pub(super) async fn run_admin_command(
        &self,
        ev: &IncomingEvent,
        display: &str,
        seq: i64,
        cmd: &str,
    ) -> PipelineOutcome {
        let head = cmd.split_whitespace().next().unwrap_or("").to_string();
        let (status, detail, reply): (&str, String, String) = match head.as_str() {
            "开团" | "解锁" | "open" | "unlock" => {
                let mut st = self.state.lock().await;
                st.locked = false;
                let v = st.version;
                (
                    "Applied",
                    "已开团（解锁）".to_string(),
                    format!("已开团（解锁），当前版本 #{v}"),
                )
            }
            "锁位" | "锁定" | "结团" | "结束" | "lock" | "close" => {
                let mut st = self.state.lock().await;
                st.locked = true;
                let v = st.version;
                (
                    "Applied",
                    "已锁定".to_string(),
                    format!("已锁定，当前版本 #{v}"),
                )
            }
            "状态" | "status" => {
                let st = self.state.lock().await;
                (
                    "Applied",
                    "状态".to_string(),
                    format!("版本 #{}，锁定={}", st.version, st.locked),
                )
            }
            "导出" | "export" => self.export_snapshot().await,
            other => (
                "Rejected",
                format!("未支持的管理员命令：{other}"),
                format!("未支持的管理员命令：{other}"),
            ),
        };

        self.persist(ev, status, &detail, "admin").await;
        let (version, snapshot_value) = {
            let mut st = self.state.lock().await;
            st.messages.push(MessageRecord {
                seq,
                display: display.to_string(),
                text: ev.text.clone(),
                status: status.to_string(),
                detail: detail.clone(),
            });
            (
                st.version,
                st.snapshot
                    .as_ref()
                    .and_then(|s| serde_json::to_value(s).ok())
                    .unwrap_or(Value::Null),
            )
        };

        PipelineOutcome {
            status: status.to_string(),
            detail,
            reply: Some(reply),
            version,
            snapshot: Some(snapshot_value),
        }
    }
}

pub(super) fn cancel_for_modify(
    round_id: &RoundId,
    ev: &IncomingEvent,
    seq: i64,
    now: DateTime<Utc>,
    item_id: crate::domain::ids::ItemId,
) -> EventEnvelope {
    EventEnvelope {
        event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        group_id: ev.group_id.clone(),
        user_id: UserId(ev.user_id.clone()),
        raw_message_id: Some(ev.message_id.clone()),
        event_type: "claim_cancelled".to_string(),
        effective_at: now,
        sequence: seq,
        payload: DomainEvent::ClaimCancelled(crate::domain::event::ClaimCancelled {
            target_claim_id: None,
            target_item_id: Some(item_id),
            quantity: None,
            reason: Some("改单".to_string()),
            parse_trace: None,
            validation_trace: vec![],
        }),
        status: crate::domain::event::EventStatus::Active,
    }
}

pub(super) fn priority_eligibility(round_id: &RoundId, user_id: &str) -> Eligibility {
    Eligibility {
        eligibility_id: EligibilityId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        user_id: UserId(user_id.to_string()),
        priority_type: "shopping_fund".to_string(),
        priority_level: 10,
        scope: EligibilityScope {
            item_ids: None,
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: Some(DateTime::<Utc>::from_timestamp_millis(0).unwrap_or_else(Utc::now)),
        valid_until: None,
        note: Some("预存(购物金)用户".to_string()),
    }
}
