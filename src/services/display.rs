use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::api::ApiState;

const CACHE_LIMIT: usize = 64;

fn board_cache() -> &'static Mutex<HashMap<i64, Value>> {
    static CACHE: OnceLock<Mutex<HashMap<i64, Value>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn clear_cache() {
    if let Ok(mut cache) = board_cache().lock() {
        cache.clear();
    }
}

fn remember(version: i64, board: &Value) {
    if let Ok(mut cache) = board_cache().lock() {
        if cache.len() >= CACHE_LIMIT {
            if let Some(&oldest) = cache.keys().min() {
                cache.remove(&oldest);
            }
        }
        cache.insert(version, board.clone());
    }
}

pub async fn display(state: &ApiState, since: i64) -> Value {
    let (version, board) = state.pipeline.board().await;
    let messages = state.pipeline.messages_since(since).await;
    let who_whats = resolve_who_whats(state, state.pipeline.who_whats().await).await;
    let status = state.gateway.status().await;

    let changed = {
        let previous = board_cache().lock().ok().and_then(|cache| {
            cache
                .iter()
                .filter(|(cached, _)| **cached < version)
                .max_by_key(|(cached, _)| **cached)
                .map(|(_, value)| value.clone())
        });
        match previous {
            Some(previous) => diff_board(&previous, &board),
            None => Vec::new(),
        }
    };
    remember(version, &board);

    json!({
        "version": version,
        "board": board,
        "messages": messages,
        "who_whats": who_whats,
        "status": status,
        "changed": changed,
    })
}

/// 用 CN 覆盖 `who_whats` 的人名（U4）：按成员缓存 `nickname → user_id` 反查。
async fn resolve_who_whats(state: &ApiState, who_whats: Vec<Value>) -> Vec<Value> {
    if who_whats.is_empty() {
        return who_whats;
    }
    let cfg = state.cfg.get().await;
    if cfg.members.cn_overrides.is_empty() {
        return who_whats;
    }
    let members = crate::services::members::cached_members(&cfg);
    let map = crate::services::members::cn_display_map(&cfg, &members);
    if map.is_empty() {
        return who_whats;
    }
    who_whats
        .into_iter()
        .map(|mut entry| {
            let display = entry
                .get("display")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(cn) = map.get(&display) {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("cn".to_string(), json!(cn));
                    obj.insert("display".to_string(), json!(cn));
                }
            }
            entry
        })
        .collect()
}

fn board_cells(board: &Value) -> BTreeMap<String, String> {
    let mut cells = BTreeMap::new();
    let Some(items) = board.get("item_allocations").and_then(Value::as_array) else {
        return cells;
    };

    for item in items {
        let item_id = item.get("item_id").and_then(Value::as_str).unwrap_or("");
        let variant = item.get("variant_id").and_then(Value::as_str).unwrap_or("");

        if let Some(boxes) = item.get("boxes").and_then(Value::as_array) {
            for b in boxes {
                let box_index = b.get("box_index").and_then(Value::as_u64).unwrap_or(0);
                let Some(slots) = b.get("slots").and_then(Value::as_array) else {
                    continue;
                };
                for slot in slots {
                    let slot_index = slot.get("slot_index").and_then(Value::as_u64).unwrap_or(0);
                    let user_id = slot.get("user_id").and_then(Value::as_str).unwrap_or("");
                    let status = slot.get("status").and_then(Value::as_str).unwrap_or("");
                    let key = format!("slot|{item_id}|{variant}|{box_index}|{slot_index}");
                    cells.insert(key, format!("{user_id}|{status}"));
                }
            }
        }

        for kind in ["singles", "waiting"] {
            let Some(list) = item.get(kind).and_then(Value::as_array) else {
                continue;
            };
            for entry in list {
                let user_id = entry.get("user_id").and_then(Value::as_str).unwrap_or("");
                let quantity = entry.get("quantity").and_then(Value::as_u64).unwrap_or(0);
                let key = format!("{kind}|{item_id}|{variant}|{user_id}");
                cells.insert(key, quantity.to_string());
            }
        }
    }

    cells
}

fn diff_board(previous: &Value, current: &Value) -> Vec<Value> {
    let prev = board_cells(previous);
    let next = board_cells(current);
    let mut changed = Vec::new();

    for (key, value) in &next {
        if prev.get(key) == Some(value) {
            continue;
        }
        changed.push(cell_change(key, Some(value)));
    }
    for key in prev.keys() {
        if !next.contains_key(key) {
            changed.push(cell_change(key, None));
        }
    }

    changed
}

fn cell_change(key: &str, value: Option<&String>) -> Value {
    let parts: Vec<&str> = key.split('|').collect();
    match parts.first().copied() {
        Some("slot") => json!({
            "kind": "slot",
            "item_id": parts.get(1).copied().unwrap_or(""),
            "variant_id": parts.get(2).copied().unwrap_or(""),
            "box_index": parts.get(3).and_then(|v| v.parse::<u64>().ok()),
            "slot_index": parts.get(4).and_then(|v| v.parse::<u64>().ok()),
            "value": value.map(String::as_str),
        }),
        Some(kind) => json!({
            "kind": kind,
            "item_id": parts.get(1).copied().unwrap_or(""),
            "variant_id": parts.get(2).copied().unwrap_or(""),
            "user_id": parts.get(3).copied().unwrap_or(""),
            "value": value.map(String::as_str),
        }),
        None => json!({ "kind": "unknown", "key": key }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn board_with_slot(user: Option<&str>) -> Value {
        json!({
            "item_allocations": [{
                "item_id": "pass_sp",
                "variant_id": "v_jcl",
                "boxes": [{
                    "box_index": 0,
                    "slots": [{
                        "slot_index": 0,
                        "user_id": user,
                        "status": if user.is_some() { "filled" } else { "empty" }
                    }]
                }],
                "singles": [],
                "waiting": []
            }]
        })
    }

    #[test]
    fn diff_reports_newly_filled_slot() {
        let changed = diff_board(&board_with_slot(None), &board_with_slot(Some("u1")));
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0]["kind"], "slot");
        assert_eq!(changed[0]["box_index"], 0);
        assert_eq!(changed[0]["value"], "u1|filled");
    }

    #[test]
    fn diff_is_empty_when_unchanged() {
        let board = board_with_slot(Some("u1"));
        assert!(diff_board(&board, &board).is_empty());
    }

    #[test]
    fn diff_reports_cleared_slot() {
        let changed = diff_board(&board_with_slot(Some("u1")), &board_with_slot(None));
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0]["value"], "|empty");
    }

    #[test]
    fn diff_reports_removed_cell() {
        let with_single = json!({
            "item_allocations": [{
                "item_id": "gift_card",
                "variant_id": "v_zt",
                "boxes": [],
                "singles": [{ "user_id": "u1", "quantity": 2 }],
                "waiting": []
            }]
        });
        let without_single = json!({
            "item_allocations": [{
                "item_id": "gift_card",
                "variant_id": "v_zt",
                "boxes": [],
                "singles": [],
                "waiting": []
            }]
        });
        let changed = diff_board(&with_single, &without_single);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0]["kind"], "singles");
        assert_eq!(changed[0]["value"], Value::Null);
    }
}
