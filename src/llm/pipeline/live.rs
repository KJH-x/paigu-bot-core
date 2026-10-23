use super::Pipeline;
use crate::{
    messages::MessageRecord,
    replay::session::{self, ReplayResult},
    settings::AppConfig,
};

impl Pipeline {
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
        self.apply_replay(&result, &records).await;
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
