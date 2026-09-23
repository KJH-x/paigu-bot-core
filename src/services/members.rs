use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tracing::warn;

use crate::api::ApiState;

const SEED_PATH: &str = "data/members.seed.json";
const EXAMPLE_PATH: &str = "data/members.example.json";

fn seed_path() -> PathBuf {
    std::env::var("PAIGU_MEMBERS_SEED_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(SEED_PATH))
}

fn example_path() -> PathBuf {
    std::env::var("PAIGU_MEMBERS_EXAMPLE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(EXAMPLE_PATH))
}

fn parse_members(raw: &str) -> Option<Vec<Value>> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let members = value.get("members").cloned().unwrap_or(value);
    members.as_array().cloned()
}

fn load_members(path: &Path) -> Option<Vec<Value>> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| parse_members(&raw))
}

/// 读取顺序：**刷新缓存（真实名单）** → seed（真实名单，用户手工放置）→ example（占位）。
fn members_from_disk(cache_path: &Path) -> (Vec<Value>, &'static str) {
    if let Some(members) = load_members(cache_path) {
        return (members, "cache");
    }
    if let Some(members) = load_members(&seed_path()) {
        return (members, "seed");
    }
    if let Some(members) = load_members(&example_path()) {
        return (members, "example");
    }
    (Vec::new(), "empty")
}

pub async fn get_members(state: &ApiState) -> Value {
    let cfg = state.cfg.get().await;
    let (members, source) = members_from_disk(Path::new(&cfg.members.cache_path));
    json!({ "members": members, "source": source })
}

/// 拉取群成员并写入缓存（供路由与每日 19:00 调度复用）。只读动作，绝不发消息。
pub async fn refresh_members_from_gateway(state: &ApiState) -> anyhow::Result<Value> {
    let cfg = state.cfg.get().await;
    let group_id = cfg.members.group_id.clone();

    let response = state
        .gateway
        .send_action("get_group_member_list", json!({ "group_id": group_id }))
        .await?;

    let status_ok = response
        .get("status")
        .and_then(Value::as_str)
        .map(|s| s == "ok")
        .unwrap_or(false);
    let data = response.get("data").cloned().unwrap_or(Value::Null);
    if !status_ok || data.is_null() {
        anyhow::bail!("gateway_action_failed: {response}");
    }

    let path = Path::new(&cfg.members.cache_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!("成员缓存目录创建失败 {}: {e}", parent.display());
            }
        }
    }
    let serialized = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(path, serialized)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_members_accepts_array_and_object() {
        assert_eq!(parse_members(r#"[{"nickname":"a"}]"#).unwrap().len(), 1);
        assert_eq!(
            parse_members(r#"{"members":[{"nickname":"a"},{"nickname":"b"}]}"#)
                .unwrap()
                .len(),
            2
        );
        assert!(parse_members("not json").is_none());
        assert!(parse_members(r#"{"other":1}"#).is_none());
    }

    #[test]
    fn cache_takes_priority_over_seed_and_example() {
        let dir = std::env::temp_dir().join(format!("paigu-members-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("members.json");

        // 无缓存 → 回退 seed/example（此时不应是 cache）
        assert_ne!(members_from_disk(&cache).1, "cache");

        std::fs::write(&cache, r#"{"members":[{"user_id":"1","nickname":"n1"}]}"#).unwrap();
        let (members, source) = members_from_disk(&cache);
        assert_eq!(source, "cache");
        assert_eq!(members.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn example_members_are_placeholders() {
        let members =
            load_members(Path::new(EXAMPLE_PATH)).expect("data/members.example.json 应存在");
        assert!(!members.is_empty());
        for m in &members {
            let nickname = m["nickname"].as_str().unwrap_or("");
            assert!(nickname.starts_with("成员"), "占位昵称异常: {nickname}");
        }
    }
}
