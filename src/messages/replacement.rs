//! Private full-log replacement: one durable intent, two recoverable files, a
//! retained before/after backup. No original NC payload is re-injected.
use super::{JsonlMessageStore, MessageLog, MessageRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct Replacement {
    version: u8,
    round_id: String,
    transaction_id: String,
    checksum: String,
    before_records: Vec<MessageRecord>,
    before_events: Vec<Value>,
    records: Vec<MessageRecord>,
    events: Vec<Value>,
}

fn lines<T: Serialize>(values: &[T]) -> anyhow::Result<Vec<u8>> {
    let mut raw = vec![];
    for value in values {
        serde_json::to_writer(&mut raw, value)?;
        raw.push(b'\n');
    }
    Ok(raw)
}

impl MessageLog {
    fn replacement_path(&self, round: &str) -> PathBuf {
        self.dir.join(format!("{round}.replacement.json"))
    }
    pub(super) fn ensure_no_replacement(&self, round: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.replacement_path(round).exists(),
            "快照事务待恢复，暂不读写该轮消息；请重启隔离服务或重试导入"
        );
        Ok(())
    }
    fn install_replacement(&self, r: &Replacement) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(
            r.version == 1 && crate::settings::rounds::is_valid_round_id(&r.round_id),
            "快照恢复格式或轮次无效"
        );
        let hash = super::transactions::hash(&(
            &r.round_id,
            &r.transaction_id,
            &r.before_records,
            &r.before_events,
            &r.records,
            &r.events,
        ))?;
        anyhow::ensure!(r.checksum == hash, "快照恢复校验失败，未覆盖原文件");
        #[cfg(test)]
        let fault = self
            .durable_fault
            .swap(0, std::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        if fault == 4 {
            anyhow::bail!("injected import crash after prepare");
        }
        crate::persistence::atomic_write(&self.store_for(&r.round_id).path, &lines(&r.records)?)?;
        #[cfg(test)]
        if fault == 5 {
            anyhow::bail!("injected import crash after message install");
        }
        crate::persistence::atomic_write(&self.event_path_for(&r.round_id), &lines(&r.events)?)?;
        #[cfg(test)]
        if fault == 6 {
            anyhow::bail!("injected import crash after event install");
        }
        for rec in &r.records {
            super::advance_seq_floor(rec.seq);
        }
        for ev in &r.events {
            if let Some(seq) = ev["sequence"].as_i64() {
                super::advance_seq_floor(seq);
            }
        }
        let backup = self
            .dir
            .join(format!("{}.applied-{}.json", r.round_id, r.transaction_id));
        // Retain both the exact old private logs and imported logs for recovery.
        std::fs::rename(self.replacement_path(&r.round_id), &backup)?;
        Ok(backup)
    }
    pub(crate) async fn recover_replacement(&self, round: &str) -> anyhow::Result<Option<PathBuf>> {
        let _serial = JsonlMessageStore::lock();
        let path = self.replacement_path(round);
        if !path.exists() {
            return Ok(None);
        }
        let r: Replacement = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(r.round_id == round, "快照恢复轮次不匹配");
        self.install_replacement(&r).map(Some)
    }
    pub(crate) async fn replace_round_durable(
        &self,
        round: &str,
        records: &[MessageRecord],
        events: &[Value],
    ) -> anyhow::Result<PathBuf> {
        self.ensure_no_replacement(round)?;
        let before_records = self.read_only(round).await?;
        let before_events = self.read_events_strict(round).await?;
        let transaction_id = uuid::Uuid::new_v4().to_string();
        let checksum = super::transactions::hash(&(
            round,
            &transaction_id,
            &before_records,
            &before_events,
            records,
            events,
        ))?;
        let r = Replacement {
            version: 1,
            round_id: round.into(),
            transaction_id,
            checksum,
            before_records,
            before_events,
            records: records.to_vec(),
            events: events.to_vec(),
        };
        let _serial = JsonlMessageStore::lock();
        crate::persistence::atomic_write(&self.replacement_path(round), &serde_json::to_vec(&r)?)?;
        self.install_replacement(&r)
    }
}
