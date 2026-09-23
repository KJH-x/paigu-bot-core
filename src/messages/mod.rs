use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
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
}

pub const DEFAULT_MESSAGES_DIR: &str = "data/messages";
pub const DEFAULT_EVENTS_DIR: &str = "data/events";

static NEXT_SEQ: AtomicI64 = AtomicI64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 进程内单调递增序号，用于持久化日志（Drop 与已处理消息共用，避免 seq 冲突）。
pub fn next_seq() -> i64 {
    NEXT_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

/// 复位进程级序号（`Pipeline::reset` 时调用，使内存状态与全局序号一道回到初始）。
pub fn reset_seq() {
    NEXT_SEQ.store(0, Ordering::SeqCst);
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
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path)?;
        let mut out = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push(serde_json::from_str(trimmed)?);
        }
        Ok(out)
    }

    pub(crate) async fn replace_all(&self, recs: &[MessageRecord]) -> anyhow::Result<()> {
        let mut buf = String::new();
        for rec in recs {
            buf.push_str(&serde_json::to_string(rec)?);
            buf.push('\n');
        }
        let _guard = Self::lock();
        Self::ensure_parent(&self.path)?;
        std::fs::write(&self.path, buf)?;
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

    /// 就地修改单条（按 `seq`），返回是否命中。
    async fn update<F>(&self, seq: i64, mut f: F) -> anyhow::Result<bool>
    where
        F: FnMut(&mut MessageRecord) + Send,
    {
        let mut recs = self.read_all().await?;
        let mut hit = false;
        for rec in recs.iter_mut() {
            if rec.seq == seq {
                f(rec);
                hit = true;
            }
        }
        if hit {
            self.replace_all(&recs).await?;
        }
        Ok(hit)
    }

    /// 删除单条（按 `seq`），返回是否命中。
    async fn delete(&self, seq: i64) -> anyhow::Result<bool> {
        let recs = self.read_all().await?;
        let before = recs.len();
        let kept: Vec<MessageRecord> = recs.into_iter().filter(|r| r.seq != seq).collect();
        let hit = kept.len() != before;
        if hit {
            self.replace_all(&kept).await?;
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
}

impl MessageLog {
    pub fn new(dir: impl Into<PathBuf>, events_dir: impl Into<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            dir: dir.into(),
            events_dir: events_dir.into(),
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
        self.store_for(round_id).append(rec).await
    }

    pub async fn read_all(&self, round_id: &str) -> anyhow::Result<Vec<MessageRecord>> {
        self.store_for(round_id).read_all().await
    }

    pub async fn replace_all(&self, round_id: &str, recs: &[MessageRecord]) -> anyhow::Result<()> {
        self.store_for(round_id).replace_all(recs).await
    }

    #[allow(dead_code)] // C-5：供 API/运维按条件检索，当前无调用方
    pub async fn query<F>(&self, round_id: &str, pred: F) -> anyhow::Result<Vec<MessageRecord>>
    where
        F: Fn(&MessageRecord) -> bool + Send,
    {
        self.store_for(round_id).query(pred).await
    }

    pub async fn update<F>(&self, round_id: &str, seq: i64, f: F) -> anyhow::Result<bool>
    where
        F: FnMut(&mut MessageRecord) + Send,
    {
        self.store_for(round_id).update(seq, f).await
    }

    pub async fn delete(&self, round_id: &str, seq: i64) -> anyhow::Result<bool> {
        self.store_for(round_id).delete(seq).await
    }

    /// 追加一条**原始事件**（C-3：保留所有成员原始事件，供事件溯源/重放）。
    pub async fn append_raw_event(
        &self,
        round_id: &str,
        ev: &serde_json::Value,
    ) -> anyhow::Result<()> {
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

    pub async fn read_raw_events(&self, round_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let path = self.event_path_for(round_id);
        let _guard = JsonlMessageStore::lock();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&path)?;
        let mut out = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push(serde_json::from_str(trimmed)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
