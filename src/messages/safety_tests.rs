use super::*;

#[tokio::test]
async fn safety_invalid_rounds_fail_at_storage_boundaries_without_touching_files() {
    let root = std::env::temp_dir().join(format!("paigu-path-safety-{}", uuid::Uuid::new_v4()));
    let messages = MessageLog::new(root.join("messages"), root.join("events"));
    let rec: MessageRecord = serde_json::from_value(serde_json::json!({
        "seq":1,"group_id":"123456789","user_id":"member-01","nickname":"成员01",
        "message_id":"one","text":"占位","timestamp_ms":1000,"is_admin":false,
        "routed":"message","status":"Ignored","detail":"占位"
    }))
    .unwrap();
    for invalid in [
        "",
        "../escape",
        "..\\escape",
        "/escape",
        "C:escape",
        "a/b",
        "a.b",
        "round ",
        " round",
        "CON",
        "nul",
        "COM1",
        "LPT9",
    ] {
        assert!(
            messages.read_only(invalid).await.is_err(),
            "read {invalid:?}"
        );
        assert!(
            messages.read_events_strict(invalid).await.is_err(),
            "journal {invalid:?}"
        );
        assert!(
            messages.query(invalid, |_| true).await.is_err(),
            "query {invalid:?}"
        );
        assert!(
            messages.read_ingress_records(invalid).await.is_err(),
            "ingress read {invalid:?}"
        );
        assert!(
            messages.recover_replacement(invalid).await.is_err(),
            "recovery {invalid:?}"
        );
        assert!(
            messages.sync_ingress_commit(invalid).is_err(),
            "sync {invalid:?}"
        );
        assert!(
            messages.computed_path(invalid).is_err(),
            "computed {invalid:?}"
        );
        assert!(
            messages.has_pending_ingress(invalid).is_err(),
            "pending {invalid:?}"
        );
        assert!(
            messages.append(invalid, &rec).await.is_err(),
            "append {invalid:?}"
        );
        assert!(
            messages.update(invalid, "one", |_| {}).await.is_err(),
            "update {invalid:?}"
        );
        assert!(
            messages.delete(invalid, "one").await.is_err(),
            "delete {invalid:?}"
        );
        assert!(
            messages
                .append_raw_event(invalid, &serde_json::json!({}))
                .await
                .is_err(),
            "raw {invalid:?}"
        );
        assert!(
            messages
                .append_durable_event(invalid, &serde_json::json!({}))
                .await
                .is_err(),
            "durable {invalid:?}"
        );
        assert!(
            messages
                .replace_round_durable(invalid, &[], &[])
                .await
                .is_err(),
            "replace {invalid:?}"
        );
    }
    assert!(
        !root.exists(),
        "reject before even creating the private root"
    );
}

#[test]
fn safety_only_typed_same_round_internal_rows_can_be_control_or_override_events() {
    use crate::domain::event::{
        DomainEvent, EventEnvelope, EventStatus, ParseOverrideEvent, RoundPhaseSet,
    };
    use crate::domain::ids::{EventId, RoundId, UserId};
    let round = "safety_round";
    for payload in [
        DomainEvent::RoundPhaseSet(RoundPhaseSet {
            phase: Some(crate::round::RoundPhase::Locked),
            set_by: UserId("u1".into()),
        }),
        DomainEvent::ParseOverride(ParseOverrideEvent {
            event_id: EventId("override-01".into()),
            round_id: RoundId(round.into()),
            target_raw_message_id: "message-01".into(),
            target_record_id: None,
            target_record_revision: None,
            corrected_parsed_message: serde_json::json!({"intent":"cancel"}),
            admin_user_id: UserId("u1".into()),
            reason: "占位".into(),
            occurred_at: chrono::Utc::now(),
        }),
    ] {
        let event = EventEnvelope {
            event_id: EventId("event-01".into()),
            round_id: RoundId(round.into()),
            group_id: "123456789".into(),
            user_id: UserId("u1".into()),
            raw_message_id: None,
            event_type: payload.event_type_str().into(),
            effective_at: chrono::Utc::now(),
            sequence: 1,
            payload,
            status: EventStatus::Active,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(
            domain_journal_events(std::slice::from_ref(&value), round)
                .unwrap()
                .len(),
            1
        );
        let wrapped = serde_json::json!({"paigu_inbound":value});
        assert!(domain_journal_events(&[wrapped], round).unwrap().is_empty());
        let mut legacy_raw = value.clone();
        legacy_raw["post_type"] = serde_json::json!("message");
        assert!(domain_journal_events(&[legacy_raw], round)
            .unwrap()
            .is_empty());
        let mut foreign = value.clone();
        foreign["round_id"] = serde_json::json!("other_round");
        assert!(domain_journal_events(&[foreign], round).is_err());
        let mut mismatched = value;
        mismatched["event_type"] = serde_json::json!("wrong_type");
        assert!(domain_journal_events(&[mismatched], round).is_err());
    }
    assert!(domain_journal_events(
        &[serde_json::json!({"event_type":"round_phase_set"})],
        round
    )
    .is_err());
}
