use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod processing;
mod replacement;
pub mod transactions;

fn default_record_revision() -> u64 {
    1
}

pub fn new_record_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<crate::bus::Delegation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<crate::bus::IngressSource>,
    #[serde(default)]
    pub record_id: String,
    #[serde(default = "default_record_revision")]
    pub record_revision: u64,
    pub seq: i64,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub text: String,
    pub timestamp_ms: i64,
    pub is_admin: bool,
    pub routed: String,
    pub status: String,
    pub detail: String,
    /// 匿名代排（`X(代)`）的**真实发送者**身份（权限按发送者判定）；普通消息为 `None`。
    #[serde(default)]
    pub sender_user_id: Option<String>,
    #[serde(default)]
    pub sender_nickname: Option<String>,
    #[serde(
        default,
        deserialize_with = "processing::deserialize_history",
        skip_serializing_if = "Option::is_none"
    )]
    pub processing: Option<processing::ProcessingHistory>,
}

pub const DEFAULT_MESSAGES_DIR: &str = "data/messages";
pub const DEFAULT_EVENTS_DIR: &str = "data/events";

static NEXT_SEQ: AtomicI64 = AtomicI64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 进程内单调递增序号，用于持久化日志（Drop 与已处理消息共用，避免 seq 冲突）。
pub fn next_seq() -> i64 {
    NEXT_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

fn advance_seq_floor(value: i64) {
    let mut current = NEXT_SEQ.load(Ordering::SeqCst);
    while value > current {
        match NEXT_SEQ.compare_exchange(current, value, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub fn messages_dir_from(env_value: Option<String>) -> PathBuf {
    env_value
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MESSAGES_DIR))
}

/// 单个轮次的消息文件句柄（`MessageLog` 内部按 `round_id` 构造，不对外暴露）。
pub(crate) struct JsonlMessageStore {
    path: PathBuf,
}

impl JsonlMessageStore {
    pub(crate) fn new(dir: impl AsRef<Path>, round_id: &str) -> Self {
        Self {
            path: dir.as_ref().join(format!("{round_id}.jsonl")),
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ensure_parent(path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn append(&self, rec: &MessageRecord) -> anyhow::Result<()> {
        let line = serde_json::to_string(rec)?;
        let _guard = Self::lock();
        Self::ensure_parent(&self.path)?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    pub(crate) async fn read_all(&self) -> anyhow::Result<Vec<MessageRecord>> {
        let _guard = Self::lock();
        self.read_all_locked(true)
    }

    pub(crate) async fn read_only(&self) -> anyhow::Result<Vec<MessageRecord>> {
        let _guard = Self::lock();
        self.read_all_locked(false)
    }

    fn read_all_locked(&self, persist_migration: bool) -> anyhow::Result<Vec<MessageRecord>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path)?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        let mut migrated = false;
        for (line_index, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // 容错：损坏/截断行跳过而非整体失败（长期运行下单个坏行不应致命）
            match serde_json::from_str::<MessageRecord>(trimmed) {
                Ok(mut rec) => {
                    if rec.record_id.trim().is_empty() {
                        let mut hash = Sha256::new();
                        hash.update(
                            self.path
                                .file_stem()
                                .and_then(|value| value.to_str())
                                .unwrap_or("unknown-round")
                                .as_bytes(),
                        );
                        hash.update(b"\0");
                        hash.update(line_index.to_le_bytes());
                        hash.update(b"\0");
                        hash.update(trimmed.as_bytes());
                        rec.record_id = format!("legacy-{:x}", hash.finalize());
                        migrated = true;
                    }
                    if rec.record_revision == 0 {
                        rec.record_revision = 1;
                        migrated = true;
                    }
                    advance_seq_floor(rec.seq);
                    out.push(rec)
                }
                Err(_) => skipped += 1,
            }
        }
        if skipped > 0 {
            tracing::warn!(path = %self.path.display(), skipped, "消息日志存在无法解析的行，已跳过");
        }
        if migrated && persist_migration {
            let backup = self.path.with_extension("jsonl.pre-record-id.bak");
            if !backup.exists() {
                std::fs::copy(&self.path, &backup)?;
            }
            Self::replace_locked(&self.path, &out)?;
        }
        Ok(out)
    }

    #[cfg(test)]
    pub(crate) async fn replace_all(&self, recs: &[MessageRecord]) -> anyhow::Result<()> {
        let _guard = Self::lock();
        Self::replace_locked(&self.path, recs)
    }

    fn replace_locked(path: &Path, recs: &[MessageRecord]) -> anyhow::Result<()> {
        let mut buf = String::new();
        for rec in recs {
            buf.push_str(&serde_json::to_string(rec)?);
            buf.push('\n');
        }
        crate::persistence::atomic_write(path, buf.as_bytes())?;
        Ok(())
    }

    /// C-5：本地数据的细粒度读写（`query` / `update` / `delete`）。
    async fn query<F>(&self, pred: F) -> anyhow::Result<Vec<MessageRecord>>
    where
        F: Fn(&MessageRecord) -> bool + Send,
    {
        let all = self.read_all().await?;
        Ok(all.into_iter().filter(|r| pred(r)).collect())
    }

    /// 就地修改单条（按不可变 `record_id`），返回修改后的记录。
    async fn update<F>(&self, record_id: &str, mut f: F) -> anyhow::Result<Option<MessageRecord>>
    where
        F: FnMut(&mut MessageRecord) + Send,
    {
        let _guard = Self::lock();
        let mut recs = self.read_all_locked(true)?;
        let mut updated = None;
        for rec in recs.iter_mut() {
            if rec.record_id == record_id {
                f(rec);
                updated = Some(rec.clone());
                break;
            }
        }
        if updated.is_some() {
            Self::replace_locked(&self.path, &recs)?;
        }
        Ok(updated)
    }

    /// 删除单条（按不可变 `record_id`），返回是否命中。
    async fn delete(&self, record_id: &str) -> anyhow::Result<bool> {
        let _guard = Self::lock();
        let recs = self.read_all_locked(true)?;
        let before = recs.len();
        let kept: Vec<MessageRecord> = recs
            .into_iter()
            .filter(|r| r.record_id != record_id)
            .collect();
        let hit = kept.len() != before;
        if hit {
            Self::replace_locked(&self.path, &kept)?;
        }
        Ok(hit)
    }
}

/// 进程级**共享**消息日志（T-05）：按 `round_id` 在调用时解析文件路径，
/// 因此同一实例可服务热载后的不同轮次；Gateway / Pipeline / API 共用一份。
/// 这是对外的**唯一存储 API**；`JsonlMessageStore` 仅为其内部文件句柄。
pub struct MessageLog {
    dir: PathBuf,
    events_dir: PathBuf,
    writer: tokio::sync::Mutex<()>,
    ingress_fence: tokio::sync::RwLock<()>,
    #[cfg(test)]
    durable_fault: std::sync::atomic::AtomicU8,
}

impl MessageLog {
    pub(crate) fn ingress_dir(&self) -> PathBuf {
        self.events_dir.join("_ingress")
    }

    pub(crate) async fn ingress_accept_guard(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.ingress_fence.read().await
    }
    pub(crate) async fn ingress_commit_guard(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.ingress_fence.write().await
    }
    /// Caller holds the common writer. Flush both durable destinations before
    /// removing the independent intake copy, including a previous partial commit.
    pub(crate) fn sync_ingress_commit(&self, round: &str) -> anyhow::Result<()> {
        for path in [self.store_for(round).path, self.event_path_for(round)] {
            match std::fs::OpenOptions::new().write(true).open(path) {
                Ok(file) => file.sync_all()?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    pub(crate) async fn read_ingress_records(
        &self,
        round: &str,
    ) -> anyhow::Result<Vec<MessageRecord>> {
        let path = self.store_for(round).path;
        if path.exists() {
            for line in std::fs::read_to_string(path)?
                .lines()
                .filter(|line| !line.trim().is_empty())
            {
                anyhow::ensure!(
                    serde_json::from_str::<MessageRecord>(line).is_ok(),
                    "消息日志有损坏行；未跳过后重试入站消息"
                );
            }
        }
        self.read_events_strict(round).await?;
        let records = self.read_only(round).await?;
        let mut ids = std::collections::HashSet::new();
        anyhow::ensure!(
            records.iter().all(|record| ids.insert(&record.record_id)),
            "消息日志身份重复；未重试入站消息"
        );
        Ok(records)
    }
    pub(crate) fn has_pending_ingress(&self, round: &str) -> anyhow::Result<bool> {
        anyhow::ensure!(
            crate::settings::rounds::is_valid_round_id(round),
            "待处理轮次标识无效"
        );
        let directory = self.ingress_dir().join(round);
        if !directory.exists() {
            return Ok(false);
        }
        for entry in std::fs::read_dir(directory)? {
            if entry?.path().extension().and_then(|e| e.to_str()) != Some("tmp") {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(crate) async fn write_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.writer.lock().await
    }
    pub fn new(dir: impl Into<PathBuf>, events_dir: impl Into<PathBuf>) -> Arc<Self> {
        let dir = dir.into();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|v| v.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                    for line in raw.lines() {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                            if let Some(seq) = value.get("seq").and_then(|v| v.as_i64()) {
                                advance_seq_floor(seq);
                            }
                        }
                    }
                }
            }
        }
        Arc::new(Self {
            dir,
            events_dir: events_dir.into(),
            writer: tokio::sync::Mutex::new(()),
            ingress_fence: tokio::sync::RwLock::new(()),
            #[cfg(test)]
            durable_fault: std::sync::atomic::AtomicU8::new(0),
        })
    }

    /// 目录可由 `PAIGU_MESSAGES_DIR` / `PAIGU_EVENTS_DIR` 覆盖。
    pub fn from_env() -> Arc<Self> {
        let dir = messages_dir_from(std::env::var("PAIGU_MESSAGES_DIR").ok());
        let events_dir = std::env::var("PAIGU_EVENTS_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_EVENTS_DIR));
        Self::new(dir, events_dir)
    }

    fn store_for(&self, round_id: &str) -> JsonlMessageStore {
        JsonlMessageStore::new(&self.dir, round_id)
    }

    fn event_path_for(&self, round_id: &str) -> PathBuf {
        self.events_dir.join(format!("{round_id}.jsonl"))
    }

    pub async fn append(&self, round_id: &str, rec: &MessageRecord) -> anyhow::Result<()> {
        self.ensure_no_replacement(round_id)?;
        self.store_for(round_id).append(rec).await
    }

    pub async fn read_all(&self, round_id: &str) -> anyhow::Result<Vec<MessageRecord>> {
        self.ensure_no_replacement(round_id)?;
        self.store_for(round_id).read_all().await
    }

    pub async fn read_only(&self, round_id: &str) -> anyhow::Result<Vec<MessageRecord>> {
        self.ensure_no_replacement(round_id)?;
        self.store_for(round_id).read_only().await
    }

    #[cfg(test)]
    pub async fn replace_all(&self, round_id: &str, recs: &[MessageRecord]) -> anyhow::Result<()> {
        self.ensure_no_replacement(round_id)?;
        self.store_for(round_id).replace_all(recs).await
    }

    #[allow(dead_code)] // C-5：供 API/运维按条件检索，当前无调用方
    pub async fn query<F>(&self, round_id: &str, pred: F) -> anyhow::Result<Vec<MessageRecord>>
    where
        F: Fn(&MessageRecord) -> bool + Send,
    {
        self.store_for(round_id).query(pred).await
    }

    pub async fn update<F>(
        &self,
        round_id: &str,
        record_id: &str,
        f: F,
    ) -> anyhow::Result<Option<MessageRecord>>
    where
        F: FnMut(&mut MessageRecord) + Send,
    {
        self.ensure_no_replacement(round_id)?;
        self.store_for(round_id).update(record_id, f).await
    }

    pub async fn delete(&self, round_id: &str, record_id: &str) -> anyhow::Result<bool> {
        self.ensure_no_replacement(round_id)?;
        self.store_for(round_id).delete(record_id).await
    }

    #[cfg(test)]
    pub async fn resolve_record_id(
        &self,
        round_id: &str,
        key: &str,
    ) -> anyhow::Result<Result<String, usize>> {
        let records = self.read_all(round_id).await?;
        if records.iter().any(|record| record.record_id == key) {
            return Ok(Ok(key.to_string()));
        }
        let Ok(seq) = key.parse::<i64>() else {
            return Ok(Err(0));
        };
        let matches: Vec<&MessageRecord> =
            records.iter().filter(|record| record.seq == seq).collect();
        match matches.as_slice() {
            [record] => Ok(Ok(record.record_id.clone())),
            many => Ok(Err(many.len())),
        }
    }

    /// 追加一条**原始事件**（C-3：保留所有成员原始事件，供事件溯源/重放）。
    pub async fn append_raw_event(
        &self,
        round_id: &str,
        ev: &serde_json::Value,
    ) -> anyhow::Result<()> {
        self.ensure_no_replacement(round_id)?;
        let path = self.event_path_for(round_id);
        let line = serde_json::to_string(ev)?;
        let _guard = JsonlMessageStore::lock();
        JsonlMessageStore::ensure_parent(&path)?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// 管理选择在单条同步日志提交后发布；写失败恢复原长度，截断尾禁止追加。
    pub async fn append_durable_event(
        &self,
        round_id: &str,
        ev: &serde_json::Value,
    ) -> anyhow::Result<()> {
        self.ensure_no_replacement(round_id)?;
        use std::io::{Read, Seek, SeekFrom, Write};
        let path = self.event_path_for(round_id);
        let _guard = JsonlMessageStore::lock();
        JsonlMessageStore::ensure_parent(&path)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        let length = file.metadata()?.len();
        if length > 0 {
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            anyhow::ensure!(
                last[0] == b'\n',
                "事件日志末尾不完整，未提交修订；请先检查本地存储"
            );
        }
        file.seek(SeekFrom::End(0))?;
        let mut line = serde_json::to_vec(ev)?;
        line.push(b'\n');
        #[cfg(test)]
        let fault = self.durable_fault.swap(0, Ordering::SeqCst);
        #[cfg(test)]
        if fault == 1 {
            anyhow::bail!("injected failure before journal write");
        }
        #[cfg(test)]
        let write = if fault == 2 {
            file.write_all(&line[..line.len() / 2])
                .and_then(|_| Err(std::io::Error::other("injected interrupted journal write")))
        } else {
            file.write_all(&line).and_then(|_| file.sync_data())
        };
        #[cfg(not(test))]
        let write = file.write_all(&line).and_then(|_| file.sync_data());
        if let Err(error) = write {
            file.set_len(length)
                .and_then(|_| file.sync_data())
                .map_err(|rollback| {
                    anyhow::anyhow!("提交失败且日志尾恢复失败：{error}；{rollback}")
                })?;
            return Err(error.into());
        }
        #[cfg(test)]
        if fault == 3 {
            anyhow::bail!("injected loss after durable commit, before publish/response");
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_durable_fault(&self, fault: u8) {
        self.durable_fault.store(fault, Ordering::SeqCst);
    }

    /// 读取原始事件（C-3）。
    pub async fn read_events_strict(
        &self,
        round_id: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        self.ensure_no_replacement(round_id)?;
        let path = self.event_path_for(round_id);
        let _guard = JsonlMessageStore::lock();
        if !path.exists() {
            return Ok(vec![]);
        }
        let text = std::fs::read_to_string(path)?;
        anyhow::ensure!(
            text.is_empty() || text.ends_with('\n'),
            "事件日志末尾不完整，未提交；请检查本地存储"
        );
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
    }

    /// NC events stay private raw data; malformed domain/audit rows fail closed.
    pub async fn domain_events(
        &self,
        round_id: &str,
    ) -> anyhow::Result<Vec<crate::domain::event::EventEnvelope>> {
        self.read_events_strict(round_id)
            .await?
            .into_iter()
            .filter(|value| value.get("event_type").is_some())
            .map(|value| serde_json::from_value(value).map_err(Into::into))
            .collect()
    }

    /// 读取原始事件（C-3）。
    ///
    /// - **容错**：损坏/截断行跳过并 `warn!`，不使整体失败；
    /// - `limit == 0` 表示全部；否则只返回**最后 `limit` 条**（避免 40MB+ 日志撑爆响应）。
    pub async fn read_raw_events(
        &self,
        round_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let path = self.event_path_for(round_id);
        let _guard = JsonlMessageStore::lock();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&path)?;
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        let start = if limit == 0 || lines.len() <= limit {
            0
        } else {
            lines.len() - limit
        };
        let mut out = Vec::with_capacity(lines.len() - start);
        let mut skipped = 0usize;
        for line in &lines[start..] {
            match serde_json::from_str::<serde_json::Value>(line.trim()) {
                Ok(v) => out.push(v),
                Err(_) => skipped += 1,
            }
        }
        if skipped > 0 {
            tracing::warn!(round_id, skipped, "原始事件日志存在无法解析的行，已跳过");
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
