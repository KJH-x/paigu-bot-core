use super::*;

fn record() -> MessageRecord {
    MessageRecord {
        record_id: "record-a".to_string(),
        record_revision: 1,
        seq: 7,
        group_id: "123456789".to_string(),
        user_id: "u1".to_string(),
        nickname: "成员01".to_string(),
        message_id: "m7".to_string(),
        text: "排 徽章 甲 1".to_string(),
        timestamp_ms: 1000,
        is_admin: false,
        routed: "message".to_string(),
        status: "Applied".to_string(),
        detail: String::new(),
        sender_user_id: None,
        ingress: None,
        sender_nickname: None,
        processing: None,
    }
}

#[test]
fn input_fingerprint_is_independent_of_execution_projection() {
    let original = record();
    let mut changed = original.clone();
    changed.status = "Rejected".to_string();
    changed.detail = "当前权限拒绝".to_string();
    changed.routed = "drop:not_whitelisted_member".to_string();
    changed.record_revision = 3;
    assert_eq!(
        ProcessingMember::from_record(&original),
        ProcessingMember::from_record(&changed)
    );
    changed.text = "排 徽章 甲 2".to_string();
    assert_ne!(
        ProcessingMember::from_record(&original),
        ProcessingMember::from_record(&changed)
    );
}

#[test]
fn history_keeps_original_result_and_matches_only_exact_members() {
    let original = record();
    let mut second = record();
    second.record_id = "record-b".to_string();
    second.seq = 8;
    second.timestamp_ms = 2000;
    let first_result = AutomaticResult {
        format_version: 1,
        created_at_ms: 1000,
        result_id: "analysis-1".to_string(),
        configuration_revision: 1,
        members: processing_members(&[&original]),
        understanding: Understanding::terminal(
            UnderstandingSource::Rules,
            "Ignored",
            "原自动结果",
            None,
        ),
        original_outcome: ProcessingOutcome::new("Ignored", "原自动结果", None),
        manual_phase_at_ingest: None,
    };
    let mut next_result = first_result.clone();
    next_result.result_id = "analysis-2".to_string();
    next_result.members = processing_members(&[&original, &second]);
    let history = ProcessingHistory {
        results: vec![first_result, next_result],
        unreadable: None,
    };
    assert_eq!(history.results[0].result_id, "analysis-1");
    assert_eq!(
        history.latest_for(&[&second, &original]).unwrap().result_id,
        "analysis-2"
    );
    assert!(history.latest_for(&[&second]).is_none());
    let mut edited = original.clone();
    edited.text.push('改');
    assert!(history.latest_for(&[&edited]).is_none());
}

#[test]
fn malformed_processing_does_not_erase_original_member_message() {
    let mut encoded = serde_json::to_value(record()).unwrap();
    let malformed =
        serde_json::json!({"results":[{"format_version":42,"unknown":"original payload"}]});
    encoded["processing"] = malformed.clone();
    let decoded: MessageRecord = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.text, "排 徽章 甲 1");
    assert_eq!(
        decoded.processing.as_ref().unwrap().unreadable,
        Some(malformed)
    );
    assert!(decoded
        .processing
        .as_ref()
        .unwrap()
        .latest_for(&[&decoded])
        .is_none());
}
