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
mod tests {
    use super::*;
    use crate::messages::MessageRecord;
    use crate::replay::session::{replay_messages, ReplayOverrides};
    use crate::settings::{default_config, ItemConfig, VariantConfig};
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("paigu-bundle-{tag}-{}", uuid::Uuid::new_v4()))
    }

    fn sample_bundle() -> SnapshotBundle {
        SnapshotBundle::seal(
            "test_round",
            3,
            "2026-09-14T00:00:00Z",
            json!({ "revision": 3 }),
            json!([{ "seq": 1, "text": "排 徽章 甲 1" }]),
            json!([{ "event_type": "claim_created" }]),
            json!({ "version": 1 }),
            Value::Null,
        )
        .unwrap()
    }

    #[test]
    fn seal_computes_hash_and_counts() {
        let bundle = sample_bundle();
        let manifest = bundle.manifest_typed().unwrap();
        assert_eq!(manifest.round_id, "test_round");
        assert_eq!(manifest.revision, 3);
        assert_eq!(manifest.message_count, 1);
        assert_eq!(manifest.event_count, 1);
        assert_eq!(manifest.version, 1);
        assert_eq!(bundle.verify().unwrap(), manifest);
    }

    #[test]
    fn export_import_round_trips_and_detects_tamper() {
        let bundle = sample_bundle();
        let dir = temp_dir("roundtrip");
        bundle.export_dir(&dir).unwrap();

        let imported = SnapshotBundle::import_dir(&dir).unwrap();
        assert_eq!(
            serde_json::to_value(&imported).unwrap(),
            serde_json::to_value(&bundle).unwrap()
        );

        std::fs::write(dir.join(MESSAGES_FILE), "{\"seq\":9}\n").unwrap();
        assert!(SnapshotBundle::import_dir(&dir).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn imported_bundle_replays_to_same_board() {
        let mut cfg = default_config();
        cfg.llm.enabled = false;
        cfg.round.round_id = "test_round".to_string();
        cfg.round.priority_users = vec![];
        cfg.round.priority_window = None;
        cfg.round.phases = vec![];
        cfg.round.items = vec![ItemConfig {
            item_id: "badge".to_string(),
            name: "徽章".to_string(),
            kind: "split".to_string(),
            class: Some("B".to_string()),
            unit_price_cents: 0,
            box_size: None,
            max_quantity: None,
            aliases: vec!["徽章".to_string()],
            variants: vec![VariantConfig {
                variant_id: "v_a".to_string(),
                name: "甲".to_string(),
                unit_price_cents: 0,
                pieces: 0,
                capacity: Some(1),
                aliases: vec![],
            }],
        }];

        let record = MessageRecord {
            seq: 1,
            group_id: "123456789".to_string(),
            user_id: "u1".to_string(),
            nickname: "u1".to_string(),
            message_id: "m1".to_string(),
            text: "排 徽章 甲 1".to_string(),
            timestamp_ms: 1_000,
            is_admin: false,
            routed: "message".to_string(),
            status: "Applied".to_string(),
            detail: String::new(),
        };

        let result = replay_messages(
            &cfg,
            std::slice::from_ref(&record),
            ReplayOverrides::default(),
        )
        .await
        .unwrap();

        let bundle = SnapshotBundle::seal(
            cfg.round.round_id.clone(),
            cfg.revision,
            "2026-09-14T00:00:00Z",
            serde_json::to_value(&cfg).unwrap(),
            serde_json::to_value(&[record]).unwrap(),
            serde_json::to_value(&result.events).unwrap(),
            serde_json::to_value(&result.board).unwrap(),
            Value::Null,
        )
        .unwrap();

        let export_dir = temp_dir("export");
        bundle.export_dir(&export_dir).unwrap();
        let imported = SnapshotBundle::import_dir(&export_dir).unwrap();

        let records: Vec<MessageRecord> =
            serde_json::from_value(imported.messages.clone()).unwrap();
        let imported_cfg: crate::settings::AppConfig =
            serde_json::from_value(imported.config.clone()).unwrap();
        let recomputed = replay_messages(&imported_cfg, &records, ReplayOverrides::default())
            .await
            .unwrap();

        assert_eq!(
            serde_json::to_value(&recomputed.board).unwrap(),
            serde_json::to_value(&result.board).unwrap()
        );

        let _ = std::fs::remove_dir_all(&export_dir);
    }

    #[test]
    fn single_json_snapshot_round_trips() {
        let bundle = SnapshotBundle::seal(
            "r1",
            3,
            "2026-09-17T00:00:00Z",
            serde_json::json!({ "a": 1 }),
            serde_json::json!([{ "seq": 1, "text": "x" }]),
            serde_json::json!([{ "raw": true }]),
            serde_json::json!({ "version": 7 }),
            serde_json::json!({ "grand_total": 100 }),
        )
        .unwrap();

        let dir = temp_dir("snapfile");
        let path = dir.join("s.json");
        bundle.export_file(&path).unwrap();

        let back = SnapshotBundle::import_file(&path).unwrap();
        let manifest = back.manifest_typed().unwrap();
        assert_eq!(manifest.version, 7);
        assert_eq!(manifest.message_count, 1);
        assert_eq!(manifest.event_count, 1);
        assert_eq!(back.config, bundle.config);
        assert_eq!(back.settlement, bundle.settlement);
        back.verify().unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_file_rejects_unknown_format() {
        let dir = temp_dir("snapfile-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, r#"{"format":"nope"}"#).unwrap();
        assert!(SnapshotBundle::import_file(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
