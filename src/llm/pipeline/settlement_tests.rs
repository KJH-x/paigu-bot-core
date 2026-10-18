use super::automatic_tests::{fixture, incoming};
use crate::services::{settlement::*, ServiceError};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

fn table() -> Value {
    json!({"packages":[
        {"package_id":"空包","lines":[]},
        {"package_id":"包甲","lines":[{"item_id":"badge","subitem_id":"a","qty":1,"unit_price_cents":1000,"is_gift":false}]},
        {"package_id":"包乙","lines":[{"item_id":"badge","subitem_id":"a","qty":1,"unit_price_cents":1000,"is_gift":false}]}
    ]})
}
fn save(view: &Value, table: Value, ack: bool) -> DraftBody {
    serde_json::from_value(json!({"round_id":view["round_id"],"revision":view["revision"],"source_token":view["source_token"],"ack_review":ack,"order_table":table,"order_numbers":{"包甲":"ORDER-01","包乙":"ORDER-02"}})).unwrap()
}
fn lock(view: &Value) -> DraftRevisionBody {
    serde_json::from_value(json!({"round_id":view["round_id"],"revision":view["revision"],"source_token":view["source_token"]})).unwrap()
}

#[tokio::test]
async fn settlement_source_is_stable_on_reads_and_independent_of_connection_preferences() {
    let (state, model) = fixture(false, "").await;
    let first = get_draft(&state).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.display.refresh_ms += 1;
    state.cfg.put(cfg, revision).await.unwrap();
    let second = get_status(&state).await.unwrap();
    assert_eq!(first["source_token"], second["source_token"]);
    assert_eq!(second["needs_review"], false);
    assert!(second.get("draft").is_none());
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn upstream_changes_require_explicit_review_without_erasing_orders_or_calling_model() {
    let (state, model) = fixture(true, r#"{"intent":"claim","items":[{"item":"徽章","subitem":"甲","quantity":2,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#).await;
    state
        .pipeline
        .process(incoming("排 徽章 甲 2", 1_700_000_000_000))
        .await;
    let first = get_draft(&state).await.unwrap();
    let calls_before_review = model.calls.load(Ordering::SeqCst);
    let saved = put_draft(&state, save(&first, table(), false))
        .await
        .unwrap();
    assert_eq!(saved["needs_review"], false);
    assert_eq!(saved["issues"], json!([]));
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.round.items[0].subitems[0].adjust_cents = 200;
    cfg.round.priority_users.push("成员02".into());
    state.cfg.put(cfg, revision).await.unwrap();
    let changed = get_draft(&state).await.unwrap();
    assert_eq!(changed["needs_review"], true);
    assert_eq!(changed["result_stale"], true);
    assert_eq!(
        changed["draft"]["order_table"],
        saved["draft"]["order_table"]
    );
    assert_eq!(
        changed["draft"]["order_numbers"],
        saved["draft"]["order_numbers"]
    );
    assert!(matches!(
        put_draft(&state, save(&saved, table(), false)).await,
        Err(ServiceError::Conflict(_))
    ));
    assert!(matches!(
        lock_draft(&state, lock(&changed)).await,
        Err(ServiceError::Conflict(_))
    ));
    let ordinary = put_draft(&state, save(&changed, table(), false))
        .await
        .unwrap();
    assert_eq!(
        ordinary["needs_review"], true,
        "autosave is not review consent"
    );
    assert_eq!(
        ordinary["result_stale"], false,
        "stored trial result has a truthful source stamp"
    );
    let reviewed = put_draft(&state, save(&ordinary, table(), true))
        .await
        .unwrap();
    assert_eq!(reviewed["needs_review"], false);
    assert_eq!(
        reviewed["draft"]["order_numbers"],
        saved["draft"]["order_numbers"]
    );
    let locked = lock_draft(&state, lock(&reviewed)).await.unwrap();
    assert_eq!(locked["draft"]["locked"], true);
    let unlocked = unlock_draft(&state, lock(&locked)).await.unwrap();
    assert_eq!(unlocked["draft"]["locked"], false);
    assert_eq!(model.calls.load(Ordering::SeqCst), calls_before_review);
}

#[tokio::test]
async fn settlement_review_detects_new_messages_even_with_same_saved_order_and_stale_ack_fails() {
    let (state, model) = fixture(false, "").await;
    state
        .pipeline
        .process(incoming("排 徽章 甲 2", 1_700_000_000_000))
        .await;
    let saved = put_draft(
        &state,
        save(&get_draft(&state).await.unwrap(), table(), false),
    )
    .await
    .unwrap();
    state
        .pipeline
        .process(incoming("撤 徽章 甲 1", 1_700_000_001_000))
        .await;
    let view = get_draft(&state).await.unwrap();
    assert_eq!(view["needs_review"], true);
    assert_ne!(saved["source_token"], view["source_token"]);
    assert!(view["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["code"] == "quantity_gap"));
    assert!(matches!(
        put_draft(&state, save(&saved, table(), true)).await,
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(get_draft(&state).await.unwrap()["draft"], saved["draft"]);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn settlement_collects_all_locatable_issues_and_does_not_lock_invalid_order() {
    let (state, model) = fixture(false, "").await;
    state
        .pipeline
        .process(incoming("排 徽章 甲 2", 1_700_000_000_000))
        .await;
    let mut body = save(&get_draft(&state).await.unwrap(), table(), false);
    body.order_numbers.clear();
    body.order_table.packages[1].lines[0].unit_price_cents = 0;
    body.order_table.packages[2].lines[0].qty = 3;
    let view = put_draft(&state, body).await.unwrap();
    let issues = view["issues"].as_array().unwrap();
    for code in [
        "order_missing",
        "price_missing",
        "price_conflict",
        "quantity_gap",
    ] {
        assert!(issues.iter().any(|i| i["code"] == code), "{code}");
    }
    assert_eq!(
        issues[0]["package_index"], 1,
        "empty packages cannot shift order number targets"
    );
    assert!(matches!(
        lock_draft(&state, lock(&view)).await,
        Err(ServiceError::Validation(_))
    ));
    assert!(!state.pipeline.is_locked().await);
    assert_eq!(get_draft(&state).await.unwrap()["draft"], view["draft"]);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
