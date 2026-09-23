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

#[async_trait::async_trait]
pub trait MessageStore: Send + Sync {
    async fn append(&self, rec: &MessageRecord) -> anyhow::Result<()>;
    #[allow(dead_code)]
    async fn read_all(&self) -> anyhow::Result<Vec<MessageRecord>>;
    #[allow(dead_code)]
    async fn replace_all(&self, recs: &[MessageRecord]) -> anyhow::Result<()>;
}

pub const DEFAULT_MESSAGES_DIR: &str = "data/messages";
pub const DEFAULT_EVENTS_DIR: &str = "data/events";

static NEXT_SEQ: AtomicI64 = AtomicI64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 进程内单调递增序号，用于持久化日志（Drop 与已处理消息共用，避免 seq 冲突）。
pub fn next_seq() -> i64 {
    NEXT_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn messages_dir_from(env_value: Option<String>) -> PathBuf {
    env_value
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MESSAGES_DIR))
}

/// 每条消息一行 JSON 的追加式日志：`<dir>/<round_id>.jsonl`。
pub struct JsonlMessageStore {
    path: PathBuf,
}

impl JsonlMessageStore {
    #[allow(dead_code)]
    pub fn new(dir: impl AsRef<Path>, round_id: &str) -> Self {
        Self {
            path: dir.as_ref().join(format!("{round_id}.jsonl")),
        }
    }

    /// 目录可由 `PAIGU_MESSAGES_DIR` 覆盖，默认 `data/messages`。
    #[allow(dead_code)]
    pub fn from_env(round_id: &str) -> Self {
        let dir = messages_dir_from(std::env::var("PAIGU_MESSAGES_DIR").ok());
        Self::new(dir, round_id)
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
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
}

/// C-5：本地数据的细粒度读写（`query` / `update` / `delete`）。
impl JsonlMessageStore {
    #[allow(dead_code)]
    pub async fn query<F>(&self, pred: F) -> anyhow::Result<Vec<MessageRecord>>
    where
        F: Fn(&MessageRecord) -> bool + Send,
    {
        let all = MessageStore::read_all(self).await?;
        Ok(all.into_iter().filter(|r| pred(r)).collect())
    }

    /// 就地修改单条（按 `seq`），返回是否命中。
    #[allow(dead_code)]
    pub async fn update<F>(&self, seq: i64, mut f: F) -> anyhow::Result<bool>
    where
        F: FnMut(&mut MessageRecord) + Send,
    {
        let mut recs = MessageStore::read_all(self).await?;
        let mut hit = false;
        for rec in recs.iter_mut() {
            if rec.seq == seq {
                f(rec);
                hit = true;
            }
        }
        if hit {
            MessageStore::replace_all(self, &recs).await?;
        }
        Ok(hit)
    }

    /// 删除单条（按 `seq`），返回是否命中。
    pub async fn delete(&self, seq: i64) -> anyhow::Result<bool> {
        let recs = MessageStore::read_all(self).await?;
        let before = recs.len();
        let kept: Vec<MessageRecord> = recs.into_iter().filter(|r| r.seq != seq).collect();
        let hit = kept.len() != before;
        if hit {
            MessageStore::replace_all(self, &kept).await?;
        }
        Ok(hit)
    }
}

#[async_trait::async_trait]
impl MessageStore for JsonlMessageStore {
    async fn append(&self, rec: &MessageRecord) -> anyhow::Result<()> {
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

    async fn read_all(&self) -> anyhow::Result<Vec<MessageRecord>> {
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

    async fn replace_all(&self, recs: &[MessageRecord]) -> anyhow::Result<()> {
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
}

/// 进程级**共享**消息日志（T-05）：按 `round_id` 在调用时解析文件路径，
/// 因此同一实例可服务热载后的不同轮次；Gateway / Pipeline / API 共用一份。
pub struct MessageLog {
    dir: PathBuf,
    events_dir: PathBuf,
}

#[allow(dead_code)]
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

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn events_dir(&self) -> &Path {
        &self.events_dir
    }

    pub fn path_for(&self, round_id: &str) -> PathBuf {
        self.dir.join(format!("{round_id}.jsonl"))
    }

    pub fn event_path_for(&self, round_id: &str) -> PathBuf {
        self.events_dir.join(format!("{round_id}.jsonl"))
    }

    /// 便捷：按轮次构造一个路径型 store（与共享实例写同一文件）。
    pub fn store_for(&self, round_id: &str) -> JsonlMessageStore {
        JsonlMessageStore {
            path: self.path_for(round_id),
        }
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

    pub async fn query<F>(&self, round_id: &str, pred: F) -> anyhow::Result<Vec<MessageRecord>>
    where
        F: Fn(&MessageRecord) -> bool + Send,
    {
        self.store_for(round_id).query(pred).await
    }

    #[allow(dead_code)]
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
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("paigu-msgs-test-{}", uuid::Uuid::new_v4()))
    }

    fn record(seq: i64, text: &str) -> MessageRecord {
        MessageRecord {
            seq,
            group_id: "123456789".to_string(),
            user_id: "10001".to_string(),
            nickname: "成员01".to_string(),
            message_id: format!("m{seq}"),
            text: text.to_string(),
            timestamp_ms: 1_788_782_400_000 + seq,
            is_admin: false,
            routed: "message".to_string(),
            status: "Applied".to_string(),
            detail: "ok".to_string(),
        }
    }

    #[test]
    fn messages_dir_from_defaults_and_overrides() {
        assert_eq!(messages_dir_from(None), PathBuf::from(DEFAULT_MESSAGES_DIR));
        assert_eq!(
            messages_dir_from(Some("  ".to_string())),
            PathBuf::from(DEFAULT_MESSAGES_DIR)
        );
        assert_eq!(
            messages_dir_from(Some("tmp/msgs".to_string())),
            PathBuf::from("tmp/msgs")
        );
    }

    #[test]
    fn new_uses_round_id_filename() {
        let store = JsonlMessageStore::new("data/messages", "月行水上");
        assert_eq!(
            store.path(),
            Path::new("data/messages").join("月行水上.jsonl")
        );
    }

    #[tokio::test]
    async fn append_then_read_round_trips_in_order() {
        let store = JsonlMessageStore::new(temp_dir(), "r1");
        store.append(&record(1, "第一条")).await.unwrap();
        store.append(&record(2, "第二条")).await.unwrap();

        let all = store.read_all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].seq, 1);
        assert_eq!(all[0].text, "第一条");
        assert_eq!(all[1].seq, 2);
        assert_eq!(all[1].nickname, "成员01");
    }

    #[tokio::test]
    async fn read_all_missing_file_is_empty() {
        let store = JsonlMessageStore::new(temp_dir(), "missing");
        assert!(store.read_all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn replace_all_overwrites_existing() {
        let store = JsonlMessageStore::new(temp_dir(), "r2");
        store.append(&record(1, "旧一")).await.unwrap();
        store.append(&record(2, "旧二")).await.unwrap();

        store.replace_all(&[record(9, "新唯一")]).await.unwrap();
        let all = store.read_all().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].seq, 9);
        assert_eq!(all[0].text, "新唯一");
    }

    #[tokio::test]
    async fn replace_all_with_empty_clears_file() {
        let store = JsonlMessageStore::new(temp_dir(), "r3");
        store.append(&record(1, "x")).await.unwrap();
        store.replace_all(&[]).await.unwrap();
        assert!(store.read_all().await.unwrap().is_empty());
    }

    #[test]
    fn next_seq_is_monotonic() {
        let a = next_seq();
        let b = next_seq();
        assert!(b > a);
    }
}
