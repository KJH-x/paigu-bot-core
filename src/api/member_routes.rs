use std::path::Path;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use super::ApiState;

const BUILTIN_MEMBERS: &[&str] = &[
    "澄猫三崎", "雨落", "霜星厨", "KJH", "SIM", "空格", "Dele.", "HOA", "梓寒", "齐布/阿布",
    "晏", "以后当屯屯鼠", "Xnze", "聆听风声", "琉羽", "cz", "KitaKita", "羽翼青冥", "Yomi",
    "双双", "竹璃", "kosame", "二黑", "万事", "3206", "鱼见见", "少年", "終夏", "雪雉厨",
    "umbb", "林苏", "不长談", "特别周", "幽烛黎夜", "Malanda", "星砾", "林恩克里斯蒂安",
    "荷兰豆", "阿文AkameAya", "code:015", "wuchang", "karie", "LORD", "祁无争", "？？？",
    "嘟嘟", "枯枯", "可怜酱", "Kang", "芜笙", "韩江", "尤娜", "雾日", "楚狂", "稀饭", "Nian",
    "稗子酒商", "千阳", "天江衣", "雀雀", "南极", "谌晨", "静边辰", "豆腐脑", "约德莱卡",
    "朔夜", "建安文容", "goya", "ソクサル",
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

async fn refresh_members(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = state.cfg.get().await;
    let group_id = cfg.members.group_id.clone();

    let response = state
        .gateway
        .send_action("get_group_member_list", json!({ "group_id": group_id }))
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": error.to_string() })),
            )
        })?;

    let status_ok = response
        .get("status")
        .and_then(Value::as_str)
        .map(|s| s == "ok")
        .unwrap_or(false);
    let data = response.get("data").cloned().unwrap_or(Value::Null);
    if !status_ok || data.is_null() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "gateway_action_failed", "response": response })),
        ));
    }

    let path = Path::new(&cfg.members.cache_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let serialized = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "[]".to_string());
    std::fs::write(path, serialized).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
    })?;

    Ok(Json(json!({ "members": data, "source": "gateway" })))
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
