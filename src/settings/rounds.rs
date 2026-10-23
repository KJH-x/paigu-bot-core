//! 轮次库（U3）：`data/rounds/<round_id>.json` 的读写与激活解析。
//!
//! - 每个轮次一个文件，内容 = `RoundSettings`；
//! - `AppConfig.round` 仍是**运行时激活轮次**；
//! - `AppConfig.active_round_id` 指向轮次文件；坏指针/文件拒绝启动或热载；
//!   只有未设置指针时才以当前合法 `round` 初始化轮次库。

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
pub fn round_path(round_id: &str) -> std::io::Result<PathBuf> {
    validate_round_id(round_id)?;
    Ok(rounds_dir().join(format!("{round_id}.json")))
}

/// 轮次 id 合法性：非空、≤64、无路径分隔符/点/冒号，仅字母/数字/中文/`_`/`-`。
pub fn is_valid_round_id(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    !id.is_empty()
        && !reserved
        && id.chars().count() <= 64
        && !id.contains(['/', '\\', '.', ':'])
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn validate_round_id(id: &str) -> std::io::Result<()> {
    if is_valid_round_id(id) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "非法轮次标识，未访问轮次文件",
        ))
    }
}

/// 读取单个轮次文件；非法 id、缺失或解析失败均返回 `None`。
pub fn read_round(round_id: &str) -> Option<RoundSettings> {
    if !is_valid_round_id(round_id) {
        return None;
    }
    let raw = std::fs::read_to_string(round_path(round_id).ok()?).ok()?;
    let round: RoundSettings = serde_json::from_str(&raw).ok()?;
    (round.round_id == round_id).then_some(round)
}

/// 落盘单个轮次（pretty JSON）；非法 id 拒绝。
pub fn write_round(round: &RoundSettings) -> std::io::Result<PathBuf> {
    if !is_valid_round_id(&round.round_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid round_id: {}", round.round_id),
        ));
    }
    let path = round_path(&round.round_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(round)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::persistence::atomic_write(&path, raw.as_bytes())?;
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
/// - 尚未设置指针时初始化当前合法轮次；已有指针无效/文件缺失损坏则失败。
///
/// 返回 `true` 表示 `AppConfig` 被修改（调用方应回写 config）。
pub fn resolve_active(cfg: &mut AppConfig) -> anyhow::Result<bool> {
    if let Some(active) = cfg.active_round_id.as_deref() {
        validate_round_id(active)?;
        cfg.round = read_round(active).ok_or_else(|| {
            anyhow::anyhow!("激活轮次文件缺失、损坏或标识不匹配，未回退为其他轮次")
        })?;
        return Ok(false);
    }
    validate_round_id(&cfg.round.round_id)?;
    write_round(&cfg.round)?;
    cfg.active_round_id = Some(cfg.round.round_id.clone());
    Ok(true)
}
