use super::Pipeline;
use crate::{
    bus::PipelineOutcome,
    messages::MessageRecord,
    replay::session::{self, ReplayResult},
    settings::AppConfig,
};

impl Pipeline {
    pub(super) async fn publication_failure(&self, error: anyhow::Error) -> PipelineOutcome {
        tracing::warn!("消息已保存但派生发布失败：{error}");
        let mut state = self.state.lock().await;
        state.ingress_dirty = true;
        state.ingress_loaded = false;
        PipelineOutcome {
            status: "Error".into(),
            detail: "消息已保存，但当前排表尚未发布；请检查本地存储并恢复处理，未重复执行原动作"
                .into(),
            reply: None,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        }
    }

    pub(super) async fn publish_completed(
        &self,
        cfg: &AppConfig,
        record: &MessageRecord,
        reply: Option<String>,
        acknowledge_action: bool,
    ) -> PipelineOutcome {
        let result = match self.advance_locked(cfg).await {
            Ok(result) => result,
            Err(error) => return self.publication_failure(error).await,
        };
        let Some(message) = result
            .messages
            .iter()
            .find(|message| message.record_id == record.record_id)
        else {
            return self
                .publication_failure(anyhow::anyhow!("已提交消息不在当前发布切面，需核对"))
                .await;
        };
        let reply = if acknowledge_action {
            Some(if message.status == "Applied" {
                format!("已记录，当前版本 #{}", result.version)
            } else {
                message.detail.clone()
            })
        } else {
            reply
        };
        PipelineOutcome {
            status: message.status.clone(),
            detail: message.detail.clone(),
            reply,
            version: result.version,
            snapshot: serde_json::to_value(result.board).ok(),
        }
    }

    pub(super) async fn select_live_records(
        &self,
        records: Vec<MessageRecord>,
    ) -> Vec<MessageRecord> {
        let state = self.state.lock().await;
        records
            .into_iter()
            .filter(|r| {
                state
                    .source_cut
                    .as_ref()
                    .is_none_or(|cut| !cut.contains(&r.record_id))
            })
            .collect()
    }

    /// Caller owns the writer. The temporary fresh cut is not a durable source truncation.
    pub(super) async fn advance_locked(&self, cfg: &AppConfig) -> anyhow::Result<ReplayResult> {
        let all = self
            .messages
            .read_ingress_records(&cfg.round.round_id)
            .await?;
        let journal = self
            .messages
            .read_events_strict(&cfg.round.round_id)
            .await?;
        let (cut, memo) = {
            let state = self.state.lock().await;
            (state.source_cut.clone(), state.memo.clone())
        };
        let records: Vec<_> = all
            .iter()
            .filter(|r| cut.as_ref().is_none_or(|cut| !cut.contains(&r.record_id)))
            .cloned()
            .collect();
        let (result, memo) = session::replay_incremental(cfg, &records, &journal, &memo).await?;
        let persistent_lock = crate::services::settlement::round_locked(&cfg.round.round_id)
            .map_err(|_| anyhow::anyhow!("持久结算锁不可校验，未发布派生结果"))?;
        self.apply_replay(cfg, &result, &records).await;
        self.restore_settlement_lock(persistent_lock).await;
        if cut.is_none() {
            crate::replay::computed::checkpoint(&self.messages, cfg, &result, &all).await;
        }
        let mut state = self.state.lock().await;
        state.source_cut = cut;
        state.memo = memo;
        // Receipts include the complete audit source, even when the live board is fresh.
        state.ingress_seen = all
            .iter()
            .filter(|r| r.status != "Pending")
            .filter_map(|r| r.ingress.clone())
            .collect();
        state.ingress_pending = all
            .into_iter()
            .filter(|r| r.status == "Pending")
            .filter_map(|r| r.ingress.clone().map(|id| (id, r)))
            .collect();
        Ok(result)
    }
}
