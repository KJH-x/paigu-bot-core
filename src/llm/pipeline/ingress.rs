use super::Pipeline;
use crate::{
    bus::{IncomingEvent, PipelineOutcome},
    messages::MessageRecord,
    settings::AppConfig,
};

impl Pipeline {
    pub(super) async fn mark_ingress(&self, record: &MessageRecord) {
        if let Some(source) = &record.ingress {
            let mut state = self.state.lock().await;
            state.ingress_pending.remove(source);
            state.ingress_seen.insert(source.clone());
        }
    }

    pub(super) async fn ingress_replay(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
    ) -> Option<PipelineOutcome> {
        let source = ev.ingress.as_ref()?;
        let (loaded, pending, dirty) = {
            let state = self.state.lock().await;
            (
                state.ingress_loaded,
                state.ingress_pending.contains_key(source),
                state.ingress_dirty,
            )
        };
        if !loaded || pending || dirty {
            let records = match self
                .messages
                .read_ingress_records(&cfg.round.round_id)
                .await
            {
                Ok(records) => records,
                Err(error) => return Some(self.persistence_failure(ev, false, error).await),
            };
            if let Err(error) = self.recover_ingress_members(cfg, &records).await {
                return Some(self.persistence_failure(ev, false, error).await);
            }
            if dirty || records.iter().any(|r| r.ingress.as_ref() == Some(source)) {
                if let Err(error) = self.recompute_locked(cfg).await {
                    return Some(self.persistence_failure(ev, false, error).await);
                }
            } else {
                // Loading receipt metadata for a genuinely new frame must not
                // turn an explicit transient test reset/fresh activation into replay.
                let mut state = self.state.lock().await;
                state.ingress_seen = records
                    .iter()
                    .filter(|r| r.status != "Pending")
                    .filter_map(|r| r.ingress.clone())
                    .collect();
                state.ingress_pending = records
                    .into_iter()
                    .filter(|r| r.status == "Pending")
                    .filter_map(|r| r.ingress.clone().map(|id| (id, r)))
                    .collect();
                state.ingress_loaded = true;
            }
        }
        let delivered = self.state.lock().await.ingress_seen.contains(source);
        if !delivered {
            return None;
        }
        if let Err(error) = self
            .messages
            .read_ingress_records(&cfg.round.round_id)
            .await
        {
            return Some(self.persistence_failure(ev, false, error).await);
        }
        if let Err(error) = self.recompute_locked(cfg).await {
            return Some(self.persistence_failure(ev, false, error).await);
        }
        let state = self.state.lock().await;
        Some(PipelineOutcome {
            status: "AlreadyDelivered".into(),
            detail: "同一入站记录已持久化，未重复执行".into(),
            reply: None,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        })
    }

    /// A group representative can be committed just before its member receipt.
    /// Reuse only an intact, exact-input artifact, never ask the model again.
    async fn recover_ingress_members(
        &self,
        cfg: &AppConfig,
        records: &[MessageRecord],
    ) -> anyhow::Result<()> {
        for pending in records
            .iter()
            .filter(|r| r.ingress.is_some() && r.status == "Pending")
        {
            let saved = records
                .iter()
                .filter_map(|owner| {
                    let history = owner.processing.as_ref()?;
                    history.results.iter().rev().find_map(|result| {
                        let members: Option<Vec<_>> = result
                            .members
                            .iter()
                            .map(|member| records.iter().find(|r| r.record_id == member.record_id))
                            .collect();
                        let members = members?;
                        if owner.status == "Pending"
                            || members
                                .iter()
                                .max_by_key(|r| (r.timestamp_ms, r.seq))
                                .map(|r| &r.record_id)
                                != Some(&owner.record_id)
                            || !members.iter().any(|r| r.record_id == pending.record_id)
                            || history.latest_for(&members).map(|r| &r.result_id)
                                != Some(&result.result_id)
                        {
                            return None;
                        }
                        Some(result.clone())
                    })
                })
                .next();
            if let Some(mut result) = saved {
                result.original_outcome = crate::messages::processing::ProcessingOutcome::new(
                    "Merged",
                    "已恢复同组持久化理解结果",
                    None,
                );
                self.messages
                    .update(&cfg.round.round_id, &pending.record_id, |record| {
                        record.status = result.original_outcome.status.clone();
                        record.detail = result.original_outcome.detail.clone();
                        record
                            .processing
                            .get_or_insert_with(Default::default)
                            .results
                            .push(result.clone());
                    })
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("待恢复成员消息缺失"))?;
            }
        }
        Ok(())
    }
}
