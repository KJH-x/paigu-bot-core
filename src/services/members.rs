use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tracing::warn;

use crate::api::ApiState;
use crate::settings::{resolve_cn, AppConfig};

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
    let cfg = state.cfg.read().await;
    let (members, source) = members_from_disk(Path::new(&cfg.members.cache_path));
    let decorated: Vec<Value> = members.iter().map(|m| decorate_member(&cfg, m)).collect();
    let cache = (source == "cache").then(|| Path::new(&cfg.members.cache_path));
    let cached_group_id = cache
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v.get("group_id").and_then(scalar_string));
    let updated = cache
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);
    json!({ "members": decorated, "source": source, "round_id":cfg.round.round_id,
        "config_revision":cfg.revision,"cached_group_id":cached_group_id,"cache_updated_at_ms":updated })
}

fn scalar_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// 群名片 LLM 清理：假设每张群名片至少含 CN，去除空格/括号等注释，产出 `user_id -> CN` 建议。
pub async fn suggest_cn(state: &ApiState) -> Result<Value, super::ServiceError> {
    let cfg = state.cfg.get().await;
    let (members, source) = members_from_disk(Path::new(&cfg.members.cache_path));
    let pairs: Vec<Value> = members
        .iter()
        .filter_map(|m| {
            let uid = m.get("user_id").and_then(scalar_string)?;
            let card = member_name(m);
            if card.trim().is_empty() {
                return None;
            }
            Some(json!({ "user_id": uid, "card": card }))
        })
        .collect();
    if pairs.is_empty() {
        return Ok(json!({ "suggestions": [], "source": source }));
    }
    let system = "你是群名片清洗助手。每张群名片至少包含一个 CN（人名/称呼），后面可能跟空格、括号等注释（状态/后缀/备注）。\
请为每条提取最核心的 CN（去掉注释，仅保留人名本身），user_id 原样返回。只输出 JSON：{\"items\":[{\"user_id\":\"...\",\"cn\":\"...\"}]}，不要解释。";
    let user = format!("群名片列表（JSON 数组）：\n{}", Value::Array(pairs));
    let client = state.pipeline.llm_client();
    let raw = client
        .complete(&cfg.llm, system, &user)
        .await
        .map_err(super::ServiceError::Internal)?;
    let parsed: Value = serde_json::from_str(&raw)
        .map_err(|e| super::ServiceError::Internal(anyhow::anyhow!("LLM 响应非 JSON: {e}")))?;
    let items = parsed
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let suggestions: Vec<Value> = items
        .into_iter()
        .filter_map(|it| {
            let uid = it.get("user_id").and_then(scalar_string)?;
            let cn = it
                .get("cn")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            (!cn.is_empty()).then(|| json!({ "user_id": uid, "cn": cn }))
        })
        .collect();
    Ok(json!({ "suggestions": suggestions, "source": source }))
}

/// 群名片优先、回退昵称（与网关 `parse_identity` 一致：统一昵称来源 = 群名片）。
pub fn member_name(m: &Value) -> String {
    let card = m
        .get("card")
        .and_then(value_as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !card.is_empty() {
        return card;
    }
    m.get("nickname")
        .and_then(value_as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 兼容字符串/数字 user_id 等标量取值。
fn value_as_str(v: &Value) -> Option<&str> {
    v.as_str()
}

/// 成员项附加 `cn`（CN 覆盖，未配置为 null）与 `resolved`（U4 展示名，CN→归一化昵称→user_id）。
fn decorate_member(cfg: &AppConfig, member: &Value) -> Value {
    let user_id = member.get("user_id").and_then(Value::as_str).unwrap_or("");
    let name = member_name(member);
    let cn = cfg
        .members
        .cn_overrides
        .iter()
        .find(|o| o.user_id == user_id)
        .map(|o| o.cn.clone());
    let resolved = resolve_cn(cfg, user_id, &name).unwrap_or_default();
    let mut out = member.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("cn".to_string(), json!(cn));
        obj.insert("resolved".to_string(), json!(resolved));
    }
    out
}

/// 当前配置下缓存的成员名单（供展示层做 `nickname → CN` 映射）。
pub fn cached_members(cfg: &AppConfig) -> Vec<Value> {
    members_from_disk(Path::new(&cfg.members.cache_path)).0
}

/// 展示层人名映射：`nickname / CN / 别名 → CN`（U4，best-effort）。
///
/// `who_whats` 由 Pipeline 按 display 分组、不含 user_id，故此处以成员缓存
/// 的 `nickname ↔ user_id` 反查；精确绑定仍需 Pipeline 携带 user_id（见汇报）。
pub fn cn_display_map(cfg: &AppConfig, members: &[Value]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for m in members {
        let user_id = m.get("user_id").and_then(Value::as_str).unwrap_or("");
        if user_id.is_empty() {
            continue;
        }
        let Some(over) = cfg
            .members
            .cn_overrides
            .iter()
            .find(|o| o.user_id == user_id)
        else {
            continue;
        };
        let cn = over.cn.trim();
        if cn.is_empty() {
            continue;
        }
        let name = member_name(m);
        if !name.trim().is_empty() {
            map.insert(name.clone(), cn.to_string());
        }
        map.insert(over.cn.clone(), cn.to_string());
        for alias in &over.aliases {
            if !alias.trim().is_empty() {
                map.insert(alias.clone(), cn.to_string());
            }
        }
    }
    map
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
    fn decorate_member_adds_cn_and_resolved() {
        use crate::settings::{default_config, CnOverride};
        let mut cfg = default_config();
        cfg.members.cn_overrides = vec![CnOverride {
            user_id: "u1".to_string(),
            cn: "甲子".to_string(),
            aliases: vec!["小甲".to_string()],
        }];

        let with_cn = decorate_member(&cfg, &json!({ "user_id": "u1", "nickname": "老甲" }));
        assert_eq!(with_cn["cn"], "甲子");
        assert_eq!(with_cn["resolved"], "甲子");

        let no_cn = decorate_member(&cfg, &json!({ "user_id": null, "nickname": "成员01" }));
        assert_eq!(no_cn["cn"], Value::Null);
        assert_eq!(no_cn["resolved"], "成员01");

        let cn_map = cn_display_map(&cfg, &[json!({ "user_id": "u1", "nickname": "老甲" })]);
        assert_eq!(cn_map.get("老甲").map(String::as_str), Some("甲子"));
        assert_eq!(cn_map.get("小甲").map(String::as_str), Some("甲子"));
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
