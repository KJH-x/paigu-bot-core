//! 轮次库（U3）：`data/rounds/<round_id>.json` 的读写与激活解析。
//!
//! - 每个轮次一个文件，内容 = `RoundSettings`；
//! - `AppConfig.round` 仍是**运行时激活轮次**；
//! - `AppConfig.active_round_id` 指向轮次文件；启动/热载据此覆盖 `round`，
//!   缺失时把当前 `round` 落盘并把 `active_round_id` 设为它。

use std::path::PathBuf;
use std::time::SystemTime;

use crate::settings::{AppConfig, RoundSettings};

/// 轮次库默认目录（可由 `PAIGU_ROUNDS_DIR` 覆盖）。
pub const DEFAULT_ROUNDS_DIR: &str = "data/rounds";

pub fn rounds_dir() -> PathBuf {
    std::env::var("PAIGU_ROUNDS_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ROUNDS_DIR))
}

/// 轮次文件路径（`data/rounds/<round_id>.json`）。
pub fn round_path(round_id: &str) -> PathBuf {
    rounds_dir().join(format!("{}.json", round_id.trim()))
}

/// 轮次 id 合法性：非空、≤64、无路径分隔符/点/冒号，仅字母/数字/中文/`_`/`-`。
pub fn is_valid_round_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id.chars().count() <= 64
        && !id.contains(['/', '\\', '.', ':'])
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// 读取单个轮次文件；非法 id、缺失或解析失败均返回 `None`。
pub fn read_round(round_id: &str) -> Option<RoundSettings> {
    if !is_valid_round_id(round_id) {
        return None;
    }
    let raw = std::fs::read_to_string(round_path(round_id)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 落盘单个轮次（pretty JSON）；非法 id 拒绝。
pub fn write_round(round: &RoundSettings) -> std::io::Result<PathBuf> {
    if !is_valid_round_id(&round.round_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid round_id: {}", round.round_id),
        ));
    }
    let path = round_path(&round.round_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(round)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, raw)?;
    Ok(path)
}

/// 轮次库中的一个文件条目（按 mtime 降序排列）。
pub struct RoundFileInfo {
    pub round_id: String,
    pub modified: Option<SystemTime>,
}

/// 列出轮次库中的合法 `*.json` 文件（按 mtime 降序，同刻按 id 升序）。
pub fn list_round_files() -> Vec<RoundFileInfo> {
    let Ok(entries) = std::fs::read_dir(rounds_dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_valid_round_id(stem) {
            continue;
        }
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        out.push(RoundFileInfo {
            round_id: stem.to_string(),
            modified,
        });
    }
    out.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.round_id.cmp(&b.round_id))
    });
    out
}

/// 启动/热载解析（U3）：
/// - `active_round_id` 指向的文件存在 → 用它覆盖 `round`；
/// - 否则把当前 `round` 落盘为 `data/rounds/<round.round_id>.json` 并把 `active_round_id` 设为它。
///
/// 返回 `true` 表示 `AppConfig` 被修改（调用方应回写 config）。
pub fn resolve_active(cfg: &mut AppConfig) -> bool {
    let current_id = cfg.round.round_id.trim().to_string();

    if let Some(active) = cfg
        .active_round_id
        .clone()
        .filter(|s| !s.trim().is_empty())
    {
        if let Some(mut loaded) = read_round(active.trim()) {
            if loaded.round_id.trim().is_empty() {
                loaded.round_id = active.trim().to_string();
            }
            cfg.round = loaded;
            return false;
        }
        // active 指向缺失文件：回退为「以当前 round 为准」。
    }

    if current_id.is_empty() {
        return false;
    }
    let newly_set = cfg.active_round_id.as_deref() != Some(current_id.as_str());
    cfg.active_round_id = Some(current_id);
    dump_current(cfg);
    newly_set
}

/// 把当前 `round` 落盘（内容一致时跳过，避免无谓写盘）。
pub fn dump_current(cfg: &AppConfig) {
    if !is_valid_round_id(&cfg.round.round_id) {
        return;
    }
    let path = round_path(&cfg.round.round_id);
    let Ok(raw) = serde_json::to_string_pretty(&cfg.round) else {
        return;
    };
    if std::fs::read_to_string(&path).ok().as_deref() == Some(raw.as_str()) {
        return;
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("轮次目录创建失败 {}: {e}", parent.display());
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, raw) {
        tracing::warn!("轮次落盘失败 {}: {e}", path.display());
    }
}
