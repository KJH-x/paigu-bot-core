use super::*;
use crate::parser::parsed_event::ParsedClaimItem;

fn claim_item(name: &str, quantity: u32) -> ParsedClaimItem {
    ParsedClaimItem {
        name: name.to_string(),
        category_hint: None,
        quantity,
        claim_type: Some("Split".to_string()),
        is_proxy_card: Some(false),
        slot_policy: Some("Normal".to_string()),
        notes: None,
        resolved_item_id: Some("item_a".to_string()),
        resolved_subitem_id: None,
        resolved_round_id: Some("round_a".to_string()),
    }
}

fn parsed(intent: ParsedIntent, items: Vec<ParsedClaimItem>) -> ParsedMessage {
    ParsedMessage {
        intent,
        round_hint: None,
        items,
        cancel_target_hint: None,
        admin_command: None,
        confidence: 0.95,
        ambiguous_parts: vec![],
    }
}

async fn validate(parsed: ParsedMessage) -> ValidationOutcome {
    let validator = EventValidator::new(0.65);
    let user_id = UserId("u1".to_string());
    let box_map = std::collections::HashMap::new();
    validator
        .validate(
            parsed,
            ValidateContext {
                user_id: &user_id,
                group_id: "g1",
                raw_message_id: Some("m1".to_string()),
                active_rounds: &[],
                box_map: &box_map,
                now: chrono::Utc::now(),
                sequence: 1,
            },
        )
        .await
        .expect("validate")
}

#[tokio::test]
async fn zero_quantity_is_rejected() {
    let outcome = validate(parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 0)])).await;
    assert!(matches!(outcome, ValidationOutcome::Reject(_)));
}

#[tokio::test]
async fn quantity_over_limit_is_rejected() {
    let outcome = validate(parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 100)])).await;
    assert!(matches!(outcome, ValidationOutcome::Reject(_)));
}

#[tokio::test]
async fn ambiguous_parts_need_confirm() {
    let mut message = parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 1)]);
    message.ambiguous_parts = vec!["商品名不确定".to_string()];
    let outcome = validate(message).await;
    assert!(matches!(outcome, ValidationOutcome::NeedConfirm(_)));
}

#[tokio::test]
async fn unknown_intent_is_ignored() {
    let outcome = validate(parsed(ParsedIntent::Unknown, vec![])).await;
    assert!(matches!(outcome, ValidationOutcome::Ignore));
}

#[tokio::test]
async fn bare_group_claim_redirects_to_box_item() {
    use crate::domain::claim::ClaimType;
    let validator = EventValidator::new(0.65);
    let user_id = UserId("u1".to_string());
    let mut box_map = std::collections::HashMap::new();
    box_map.insert(ItemId("item_a".to_string()), ItemId("box_a".to_string()));
    let outcome = validator
        .validate(
            parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 1)]),
            ValidateContext {
                user_id: &user_id,
                group_id: "g1",
                raw_message_id: Some("m1".to_string()),
                active_rounds: &[],
                box_map: &box_map,
                now: chrono::Utc::now(),
                sequence: 1,
            },
        )
        .await
        .expect("validate");
    match outcome {
        ValidationOutcome::Ok(event) => match &event.payload {
            DomainEvent::ClaimCreated(created) => {
                assert_eq!(created.items[0].item_id.0, "box_a");
                assert!(matches!(created.items[0].claim_type, ClaimType::Single));
            }
            other => panic!("expected ClaimCreated, got {other:?}"),
        },
        _ => panic!("expected Ok"),
    }
}

#[tokio::test]
async fn group_claim_with_subitem_is_not_redirected() {
    let validator = EventValidator::new(0.65);
    let user_id = UserId("u1".to_string());
    let mut box_map = std::collections::HashMap::new();
    box_map.insert(ItemId("item_a".to_string()), ItemId("box_a".to_string()));
    let mut item = claim_item("商品甲", 1);
    item.resolved_subitem_id = Some("v1".to_string());
    let outcome = validator
        .validate(
            parsed(ParsedIntent::Claim, vec![item]),
            ValidateContext {
                user_id: &user_id,
                group_id: "g1",
                raw_message_id: Some("m1".to_string()),
                active_rounds: &[],
                box_map: &box_map,
                now: chrono::Utc::now(),
                sequence: 1,
            },
        )
        .await
        .expect("validate");
    match outcome {
        ValidationOutcome::Ok(event) => match &event.payload {
            DomainEvent::ClaimCreated(created) => {
                assert_eq!(created.items[0].item_id.0, "item_a");
            }
            other => panic!("expected ClaimCreated, got {other:?}"),
        },
        _ => panic!("expected Ok"),
    }
}
