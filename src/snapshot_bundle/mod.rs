use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// 结构化快照包（C4）：manifest + 配置 + 消息日志 + 事件 + 排位 + 结算。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotBundle {
    pub manifest: Value,
    pub config: Value,
    pub messages: Value,
    pub events: Value,
    pub snapshot: Value,
    pub settlement: Value,
}

pub const MANIFEST_FILE: &str = "manifest.json";
pub const CONFIG_FILE: &str = "config.json";
pub const MESSAGES_FILE: &str = "messages.jsonl";
pub const EVENTS_FILE: &str = "events.json";
pub const SNAPSHOT_FILE: &str = "snapshot.json";
pub const SETTLEMENT_FILE: &str = "settlement.json";

/// 单一 JSON 快照的格式标识。
pub const FILE_FORMAT: &str = "paigu-snapshot/1";

/// 单一 JSON 快照（C-2）：原始消息数据 + 计算结果缓存 + 计算版本与时间。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub format: String,
    /// 计算时间（RFC3339）。
    pub computed_at: String,
    /// 计算版本（程序/版本标识）。
    pub computed_by: String,
    /// 计算版本号（= 排位快照 version）。
    #[serde(default)]
    pub version: i64,
    pub manifest: Value,
    pub config: Value,
    pub messages: Value,
    pub events: Value,
    pub snapshot: Value,
    pub settlement: Value,
}

/// `manifest.json` 的强类型视图；`hash` 覆盖包内除 manifest 外的全部内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub round_id: String,
    pub revision: u64,
    pub created_at: String,
    pub hash: String,
    #[serde(default)]
    pub message_count: usize,
    #[serde(default)]
    pub event_count: usize,
    #[serde(default)]
    pub version: i64,
}

impl SnapshotBundle {
    /// 用各部分内容封装成包：计算消息/事件计数、版本与内容哈希并写入 manifest。
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        round_id: impl Into<String>,
        revision: u64,
        created_at: impl Into<String>,
        config: Value,
        messages: Value,
        events: Value,
        snapshot: Value,
        settlement: Value,
    ) -> anyhow::Result<Self> {
        let message_count = messages.as_array().map(Vec::len).unwrap_or(0);
        let event_count = events.as_array().map(Vec::len).unwrap_or(0);
        let version = snapshot.get("version").and_then(Value::as_i64).unwrap_or(0);

        let mut bundle = Self {
            manifest: Value::Null,
            config,
            messages,
            events,
            snapshot,
            settlement,
        };
        let hash = bundle.recompute_hash()?;
        bundle.manifest = serde_json::to_value(SnapshotManifest {
            round_id: round_id.into(),
            revision,
            created_at: created_at.into(),
            hash,
            message_count,
            event_count,
            version,
        })?;
        Ok(bundle)
    }

    pub fn manifest_typed(&self) -> anyhow::Result<SnapshotManifest> {
        Ok(serde_json::from_value(self.manifest.clone())?)
    }

    /// 内容哈希：对 config/messages/events/snapshot/settlement 的规范化 JSON 依次摘要。
    pub fn recompute_hash(&self) -> anyhow::Result<String> {
        let mut hasher = Sha256::new();
        for part in [
            &self.config,
            &self.messages,
            &self.events,
            &self.snapshot,
            &self.settlement,
        ] {
            hasher.update(serde_json::to_string(part)?.as_bytes());
            hasher.update(b"\n");
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// 校验结构（manifest 可解析）与内容哈希一致。
    pub fn verify(&self) -> anyhow::Result<SnapshotManifest> {
        let manifest = self.manifest_typed()?;
        let actual = self.recompute_hash()?;
        if actual != manifest.hash {
            anyhow::bail!(
                "snapshot hash mismatch: manifest={} actual={actual}",
                manifest.hash
            );
        }
        Ok(manifest)
    }

    /// 导出为目录：`manifest.json` / `config.json` / `messages.jsonl` / `events.json` / `snapshot.json` / `settlement.json`。
    pub fn export_dir(&self, dir: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        write_json(&dir.join(MANIFEST_FILE), &self.manifest)?;
        write_json(&dir.join(CONFIG_FILE), &self.config)?;
        write_json(&dir.join(EVENTS_FILE), &self.events)?;
        write_json(&dir.join(SNAPSHOT_FILE), &self.snapshot)?;
        write_json(&dir.join(SETTLEMENT_FILE), &self.settlement)?;

        let mut lines = String::new();
        for msg in self.messages.as_array().cloned().unwrap_or_default() {
            lines.push_str(&serde_json::to_string(&msg)?);
            lines.push('\n');
        }
        std::fs::write(dir.join(MESSAGES_FILE), lines)?;
        Ok(dir)
    }

    /// 从目录导入并校验哈希；不一致则报错。
    pub fn import_dir(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        let bundle = Self {
            manifest: read_json(&dir.join(MANIFEST_FILE))?,
            config: read_json(&dir.join(CONFIG_FILE))?,
            messages: read_jsonl(&dir.join(MESSAGES_FILE))?,
            events: read_json(&dir.join(EVENTS_FILE))?,
            snapshot: read_json(&dir.join(SNAPSHOT_FILE))?,
            settlement: read_json(&dir.join(SETTLEMENT_FILE))?,
        };
        bundle.verify()?;
        Ok(bundle)
    }

    /// 导出为**单一 JSON 文件**（C-2）：原始消息 + 计算结果缓存 + 计算版本与时间。
    pub fn export_file(&self, path: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let manifest = self.manifest_typed()?;
        let file = SnapshotFile {
            format: FILE_FORMAT.to_string(),
            computed_at: chrono::Utc::now().to_rfc3339(),
            computed_by: format!("paigu-bot-core/{}", env!("CARGO_PKG_VERSION")),
            version: manifest.version,
            manifest: self.manifest.clone(),
            config: self.config.clone(),
            messages: self.messages.clone(),
            events: self.events.clone(),
            snapshot: self.snapshot.clone(),
            settlement: self.settlement.clone(),
        };
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&path, serde_json::to_string_pretty(&file)?)?;
        Ok(path)
    }

    /// 从单一 JSON 文件导入并校验哈希；格式或哈希不符则报错。
    pub fn import_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        let file: SnapshotFile = serde_json::from_str(&raw)?;
        if file.format != FILE_FORMAT {
            anyhow::bail!("unsupported snapshot format: {}", file.format);
        }
        let bundle = Self {
            manifest: file.manifest,
            config: file.config,
            messages: file.messages,
            events: file.events,
            snapshot: file.snapshot,
            settlement: file.settlement,
        };
        bundle.verify()?;
        Ok(bundle)
    }
}

fn write_json(path: &Path, value: &Value) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn read_json(path: &Path) -> anyhow::Result<Value> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn read_jsonl(path: &Path) -> anyhow::Result<Value> {
    let raw = std::fs::read_to_string(path)?;
    let mut arr = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        arr.push(serde_json::from_str::<Value>(trimmed)?);
    }
    Ok(Value::Array(arr))
}

#[cfg(test)]
mod tests;
