use std::path::Path;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use super::ApiState;

const BUILTIN_MEMBERS: &[&str] = &[
    "成员105", "雨落", "霜星厨", "KJH", "SIM", "空格", "Dele.", "HOA", "梓寒", "成员161/阿布",
    "晏", "以后当屯屯鼠", "Xnze", "成员115", "成员150", "cz", "KitaKita", "羽翼青冥", "Yomi",
    "双双", "成员157", "成员57", "二黑", "成员147", "3206", "鱼见见", "成员151", "終夏", "雪雉厨",
    "umbb", "林苏", "不长談", "成员118", "幽烛黎夜", "成员37", "星砾", "林恩克里斯蒂安",
    "成员130", "成员16", "code:015", "wuchang", "成员77", "LORD", "祁无争", "成员133",
    "嘟嘟", "枯枯", "成员131", "Kang", "芜笙", "韩江", "尤娜", "成员163", "楚狂", "稀饭", "Nian",
    "成员90", "成员168", "天江衣", "成员156", "成员155", "成员142", "静边辰", "豆腐脑", "约德莱卡",
    "成员164", "建安文容", "goya", "成员88",
];

fn builtin_members() -> Vec<Value> {
    BUILTIN_MEMBERS
        .iter()
        .map(|name| json!({ "user_id": Value::Null, "nickname": name }))
        .collect()
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/members", get(get_members))
        .route("/api/members/refresh", post(refresh_members))
}

async fn get_members(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let cfg = state.cfg.get().await;
    match std::fs::read_to_string(Path::new(&cfg.members.cache_path)) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(value) => {
                let members = value.get("members").cloned().unwrap_or(value);
                Json(json!({ "members": members, "source": "cache" }))
            }
            Err(_) => Json(json!({ "members": builtin_members(), "source": "builtin" })),
        },
        Err(_) => Json(json!({ "members": builtin_members(), "source": "builtin" })),
    }
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
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let serialized = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(path, serialized)?;
    Ok(data)
}

async fn refresh_members(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match refresh_members_from_gateway(&state).await {
        Ok(data) => Ok(Json(json!({ "members": data, "source": "gateway" }))),
        Err(error) => Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": error.to_string() })),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_subset_is_non_empty_and_shaped() {
        let members = builtin_members();
        assert_eq!(members.len(), BUILTIN_MEMBERS.len());
        assert!(members.iter().any(|m| m["nickname"] == "SIM"));
        assert!(members.iter().any(|m| m["nickname"] == "code:015"));
    }
}
