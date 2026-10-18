use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::parser::action_override::ActionSelection;
use crate::services::message_actions::{self, SaveActions};
use serde_json::json;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn mp_t11_t29_late_multiple_supplements_are_one_undoable_transaction_without_model() {
    let (state, model) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut other = incoming("排 徽章 甲 1", 2000);
    other.user_id = "u2".into();
    other.nickname = "成员02".into();
    state.pipeline.process(other).await;
    state
        .pipeline
        .process(incoming("这是对之前意思的占位补充", 45 * 86_400_000))
        .await;
    state
        .pipeline
        .process(incoming("排 徽章 甲 2", 45 * 86_400_000 + 1000))
        .await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.llm.enabled = true;
    state.cfg.put(cfg, revision).await.unwrap();
    let (id, mut body) = opened(&state).await;
    let raw = state.messages.read_all(&body.round_id).await.unwrap();
    let round_id = body.round_id.clone();
    let journal_before = state
        .messages
        .read_events_strict(&round_id)
        .await
        .unwrap()
        .len();
    body.supplements = raw[2..]
        .iter()
        .map(|record| message_actions::RecordVersion {
            record_id: record.record_id.clone(),
            record_revision: 1,
        })
        .collect();
    if let ActionSelection::Replace { actions, note } = &mut body.selection {
        actions[0].items[0].quantity = 3;
        *note = "四十五天后的两条补充属于原意说明".into();
    }
    let saved = message_actions::save(&state, &id, body).await.unwrap();
    assert_eq!(saved["undo"]["records"], 3);
    for supplement in &raw[2..] {
        let current = message_actions::read(&state, &supplement.record_id, &round_id)
            .await
            .unwrap();
        assert_eq!(current["absorbed_by"], id);
        assert_eq!(current["current_outcome"]["status"], "Rejected");
    }
    let cfg = state.cfg.get().await;
    let undo = message_actions::undo_status(&state, &cfg.round.round_id)
        .await
        .unwrap();
    message_actions::undo(
        &state,
        message_actions::UndoInput {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            round_id: cfg.round.round_id.clone(),
            config_revision: cfg.revision,
            undo_token: undo["undo_token"].as_str().unwrap().into(),
        },
    )
    .await
    .unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 4);
    for supplement in &raw[2..] {
        let current = message_actions::read(&state, &supplement.record_id, &cfg.round.round_id)
            .await
            .unwrap();
        assert_eq!(current["absorbed_by"], serde_json::Value::Null);
        assert_eq!(current["current_outcome"]["status"], supplement.status);
        assert!(current["message"]["record_revision"].as_u64().unwrap() >= 3);
    }
    assert_eq!(
        state
            .messages
            .read_events_strict(&round_id)
            .await
            .unwrap()
            .len(),
        journal_before + 2,
        "save and undo append two audit transactions"
    );
    assert_eq!(
        serde_json::to_value(&raw).unwrap(),
        serde_json::to_value(state.messages.read_all(&cfg.round.round_id).await.unwrap()).unwrap(),
        "undo appends audit, never rewrites original member messages"
    );
    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        0,
        "even enabled, save/group/restore/recompute cannot call model"
    );
}

#[tokio::test]
async fn mp_t19_new_inbound_and_two_editors_are_serial_without_lost_messages() {
    let (state, model) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut first) = opened(&state).await;
    let mut second = first.clone();
    second.transaction_id = uuid::Uuid::new_v4().to_string();
    if let ActionSelection::Replace { actions, .. } = &mut first.selection {
        actions[0].items[0].quantity = 2;
    }
    if let ActionSelection::Replace { actions, .. } = &mut second.selection {
        actions[0].items[0].quantity = 3;
    }
    let mut other = incoming("排 徽章 甲 1", 2000);
    other.user_id = "u2".into();
    other.nickname = "成员02".into();
    let (left, right, inbound) = tokio::join!(
        message_actions::save(&state, &id, first),
        message_actions::save(&state, &id, second),
        state.pipeline.ingest(other)
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert_eq!(inbound.status, "Applied");
    let result = state.pipeline.recompute().await.unwrap();
    let total = quantity(&result.board);
    assert!(total == 3 || total == 4);
    assert_eq!(result.messages.len(), 2);
    let current = message_actions::read(&state, &id, &state.cfg.get().await.round.round_id)
        .await
        .unwrap();
    assert_eq!(current["message"]["record_revision"], 2);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mp_t32_multi_cancel_validates_every_target_and_quantity_without_recent_fallback() {
    use crate::parser::parsed_event::{ParsedClaimItem, ParsedIntent, ParsedMessage};
    let (state, model) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let record = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    let valid = ParsedClaimItem {
        name: "甲".into(),
        category_hint: None,
        quantity: 1,
        claim_type: None,
        is_proxy_card: None,
        slot_policy: None,
        notes: None,
        resolved_item_id: Some("badge".into()),
        resolved_subitem_id: Some("a".into()),
        resolved_round_id: Some(cfg.round.round_id.clone()),
    };
    for failure in ["missing", "invalid_hint", "zero", "over_limit"] {
        let mut bad = valid.clone();
        match failure {
            "missing" => {
                bad.name = "不存在商品".into();
                bad.resolved_item_id = None;
                bad.resolved_subitem_id = None;
            }
            "invalid_hint" => {
                bad.name = "徽章".into();
                bad.resolved_item_id = Some("removed".into());
            }
            "zero" => bad.quantity = 0,
            _ => bad.quantity = 100,
        }
        let parsed = ParsedMessage {
            intent: ParsedIntent::Cancel,
            round_hint: None,
            items: vec![valid.clone(), bad],
            cancel_target_hint: Some("逐项撤销占位".into()),
            admin_command: None,
            confidence: 0.95,
            ambiguous_parts: vec![],
        };
        let understood = crate::replay::session::prepare_understanding(
            &cfg,
            &record,
            parsed,
            crate::messages::processing::UnderstandingSource::Rules,
        )
        .await;
        assert!(understood.events.is_empty(),"{failure}: invalid second target must not partially cancel the first or retarget latest");
        assert!(
            matches!(
                understood.validation.status.as_str(),
                "NeedConfirm" | "Rejected"
            ),
            "{failure}"
        );
    }
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mp_t41_explicit_product_scope_wins_over_shared_subitem_first_match() {
    let (state, model) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    let mut item = cfg.round.items[0].clone();
    item.item_id = "stand".into();
    item.name = "立牌".into();
    cfg.round.items.push(item);
    state.cfg.put(cfg, revision).await.unwrap();
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.process(incoming("排 立牌 甲 1", 2000)).await;
    state.pipeline.process(incoming("撤 立牌 甲 1", 3000)).await;
    let result = state.pipeline.recompute().await.unwrap();
    let lines =
        crate::engine::replay::collect_effective_claims(&result.events, &result.eligibilities);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0].item_id.0, "badge",
        "known product context must not be discarded by matching the shared subitem again"
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mp_t39_claim_then_bare_cancel_stays_two_units_with_merge_off_and_on() {
    for merge in [false, true] {
        let (state, model) = fixture(false, "").await;
        let mut cfg = state.cfg.get().await;
        let revision = cfg.revision;
        cfg.merge.enabled = merge;
        state.cfg.put(cfg, revision).await.unwrap();
        assert_eq!(
            state
                .pipeline
                .ingest(incoming("排 徽章 甲 1", 1000))
                .await
                .status,
            "Applied"
        );
        assert_eq!(
            state
                .pipeline
                .ingest(incoming("撤 徽章", 2000))
                .await
                .status,
            "Applied"
        );
        let replay = state.pipeline.recompute().await.unwrap();
        assert_eq!(quantity(&replay.board), 0);
        assert_eq!(replay.outcomes.len(), 2);
        assert!(replay
            .outcomes
            .iter()
            .all(|outcome| outcome.status != "Merged"));
        let records = state
            .messages
            .read_all(&state.cfg.get().await.round.round_id)
            .await
            .unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record
            .processing
            .as_ref()
            .unwrap()
            .results
            .last()
            .unwrap()
            .members
            .len()
            == 1));
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn mp_t40_t41_intervening_own_item_chatter_and_other_member_do_not_retarget_cancellation() {
    for merge in [false, true] {
        let (state, model) = fixture(false, "").await;
        let mut cfg = state.cfg.get().await;
        let revision = cfg.revision;
        cfg.merge.enabled = merge;
        let mut other = cfg.round.items[0].clone();
        other.item_id = "stand".into();
        other.name = "立牌".into();
        other.subitems[0].subitem_id = "b".into();
        other.subitems[0].name = "乙".into();
        cfg.round.items.push(other);
        state.cfg.put(cfg, revision).await.unwrap();
        state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
        state.pipeline.ingest(incoming("排 立牌 乙 1", 2000)).await;
        state
            .pipeline
            .ingest(incoming("这是一条占位闲聊", 3000))
            .await;
        let mut other = incoming("排 徽章 甲 1", 4000);
        other.user_id = "u2".into();
        other.nickname = "成员02".into();
        state.pipeline.ingest(other).await;
        state.pipeline.ingest(incoming("撤 徽章", 5000)).await;
        let first = state.pipeline.recompute().await.unwrap();
        let lines =
            crate::engine::replay::collect_effective_claims(&first.events, &first.eligibilities);
        assert!(lines
            .iter()
            .any(|line| line.user_id.0 == "u1" && line.item_id.0 == "stand" && line.quantity == 1));
        assert!(lines
            .iter()
            .any(|line| line.user_id.0 == "u2" && line.item_id.0 == "badge" && line.quantity == 1));
        assert!(!lines
            .iter()
            .any(|line| line.user_id.0 == "u1" && line.item_id.0 == "badge"));
        let (id, mut body) = opened(&state).await;
        if merge {
            let records = state.messages.read_all(&body.round_id).await.unwrap();
            body.supplements.push(message_actions::RecordVersion {
                record_id: records[1].record_id.clone(),
                record_revision: 1,
            });
        }
        if let ActionSelection::Replace { actions, .. } = &mut body.selection {
            for line in actions.iter_mut().flat_map(|action| &mut action.items) {
                if line.item_id == "badge" {
                    line.quantity = 3;
                }
            }
        }
        message_actions::save(&state, &id, body).await.unwrap();
        state.pipeline.ingest(incoming("撤 立牌", 6000)).await;
        let after = state.pipeline.recompute().await.unwrap();
        let lines =
            crate::engine::replay::collect_effective_claims(&after.events, &after.eligibilities);
        assert!(lines.iter().any(|line| line.user_id.0=="u1" && line.item_id.0=="badge" && line.quantity==2), "the preserved later cancellation still subtracts its original normalized quantity of one");
        assert!(lines
            .iter()
            .any(|line| line.user_id.0 == "u2" && line.item_id.0 == "badge" && line.quantity == 1));
        assert!(
            !lines.iter().any(|line| line.item_id.0 == "stand"),
            "explicit other-product cancellation must not be retargeted"
        );
        let after_detail =
            message_actions::read(&state, &id, &state.cfg.get().await.round.round_id)
                .await
                .unwrap();
        assert_eq!(after_detail["message"]["timestamp_ms"], 1000);
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn mp_t32_expired_exact_cancel_explains_no_match_and_never_cancels_another_claim() {
    let (state, model) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.process(incoming("排 徽章 甲 1", 2000)).await;
    state.pipeline.process(incoming("撤 徽章 1", 3000)).await;
    let cfg = state.cfg.get().await;
    let records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    let first_claim = crate::parser::action_override::primary_claim_id(&records[0]);
    let cancel_id = &records[2].record_id;
    let detail = message_actions::read(&state, cancel_id, &cfg.round.round_id)
        .await
        .unwrap();
    let mut selection: ActionSelection =
        serde_json::from_value(detail["editable_selection"].clone()).unwrap();
    if let ActionSelection::Replace { actions, .. } = &mut selection {
        actions[0].target_claim_id = Some(first_claim.clone());
        actions[0].target_item_id = None;
        actions[0].quantity = None;
    }
    message_actions::save(
        &state,
        cancel_id,
        SaveActions {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            round_id: cfg.round.round_id.clone(),
            config_revision: cfg.revision,
            record_revision: detail["message"]["record_revision"].as_u64().unwrap(),
            selection,
            supplements: vec![],
            release_supplements: vec![],
        },
    )
    .await
    .unwrap();
    let before = message_actions::read(&state, cancel_id, &cfg.round.round_id)
        .await
        .unwrap();
    let (id, mut body) = opened(&state).await;
    body.selection = ActionSelection::Ignore {
        note: "管理员明确撤掉第一条执行结果".into(),
    };
    message_actions::save(&state, &id, body).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 1);
    let detail = message_actions::read(&state, cancel_id, &cfg.round.round_id)
        .await
        .unwrap();
    assert_eq!(
        detail["editable_selection"]["actions"][0]["target_claim_id"],
        first_claim
    );
    assert_eq!(detail["original_automatic"], before["original_automatic"]);
    assert!(
        detail["current_outcome"]["detail"]
            .as_str()
            .unwrap()
            .contains("未命中"),
        "expired exact reference must be explained instead of pretending success: {}",
        detail["current_outcome"]
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mp_t05_manual_replacement_overrides_ignored_needconfirm_and_error_without_losing_original()
{
    for status in ["Ignored", "NeedConfirm", "Error"] {
        let (state, model) = fixture(false, "").await;
        let text = if status == "NeedConfirm" {
            "排 徽章 甲 1 撤"
        } else {
            "占位闲聊"
        };
        state.pipeline.process(incoming(text, 1000)).await;
        let cfg = state.cfg.get().await;
        if status == "Error" {
            let mut records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
            records[0].status = "Error".into();
            records[0].detail = "占位历史处理错误".into();
            let result = &mut records[0].processing.as_mut().unwrap().results[0];
            result.original_outcome.status = "Error".into();
            result.original_outcome.detail = "占位历史处理错误".into();
            result.understanding.validation = result.original_outcome.clone();
            result.understanding.events.clear();
            state
                .messages
                .replace_all(&cfg.round.round_id, &records)
                .await
                .unwrap();
        }
        let (id, mut body) = opened(&state).await;
        let original = message_actions::read(&state, &id, &cfg.round.round_id)
            .await
            .unwrap();
        assert_eq!(original["current_outcome"]["status"], status);
        body.selection=serde_json::from_value(json!({"kind":"replace","note":"当时实际要求两份","actions":[{"action_id":"primary","operation":"claim","items":[{"line_id":"first","item_id":"badge","subitem_id":"a","quantity":2,"claim_type":"Split","slot_policy":"Normal","is_proxy_card":false,"notes":null}],"target_claim_id":null,"target_item_id":null,"quantity":null}]})).unwrap();
        message_actions::save(&state, &id, body).await.unwrap();
        let after = message_actions::read(&state, &id, &cfg.round.round_id)
            .await
            .unwrap();
        assert_eq!(after["current_outcome"]["status"], "Applied");
        assert_eq!(after["original_automatic"], original["original_automatic"]);
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            2
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}
