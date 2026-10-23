use super::tests::{record, test_config};
use super::*;
use serde_json::{json, Value};

fn cfg() -> AppConfig {
    let mut cfg = test_config();
    cfg.members.cache_path.clear();
    cfg.merge.enabled = false;
    cfg.gateway.whitelist_groups = vec!["123456789".into()];
    cfg.gateway.whitelist_members.clear();
    cfg
}

async fn equivalent(
    cfg: &AppConfig,
    records: &[MessageRecord],
    journal: &[Value],
    before: &Memo,
) -> (ReplayResult, Memo) {
    let (actual, memo) = replay_incremental(cfg, records, journal, before)
        .await
        .unwrap();
    let expected = replay_from_journal(cfg, records, journal, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let eligibility = |result: &ReplayResult| {
        result
            .eligibilities
            .iter()
            .map(|e| {
                let mut value = serde_json::to_value(e).unwrap();
                value.as_object_mut().unwrap().remove("eligibility_id");
                value
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        eligibility(&actual),
        eligibility(&expected),
        "仅随机资格ID不参与跨独立重算比较，其余内部字段完整比较"
    );
    assert_eq!(actual.display_names, expected.display_names);
    (actual, memo)
}

fn override_event(
    rec: &MessageRecord,
    parsed: crate::parser::parsed_event::ParsedMessage,
    by_record: bool,
) -> Value {
    let id = EventId(format!("override-{}", rec.seq));
    let now = DateTime::<Utc>::from_timestamp_millis(10_000 + rec.seq).unwrap();
    serde_json::to_value(EventEnvelope {
        event_id: id.clone(),
        round_id: RoundId("test_round".into()),
        group_id: rec.group_id.clone(),
        user_id: UserId("admin".into()),
        raw_message_id: Some(rec.message_id.clone()),
        event_type: "parse_override".into(),
        effective_at: now,
        sequence: 10_000 + rec.seq,
        status: EventStatus::Active,
        payload: DomainEvent::ParseOverride(crate::domain::event::ParseOverrideEvent {
            event_id: id,
            round_id: RoundId("test_round".into()),
            target_raw_message_id: rec.message_id.clone(),
            target_record_id: by_record.then(|| rec.record_id.clone()),
            target_record_revision: Some(2),
            corrected_parsed_message: serde_json::to_value(parsed).unwrap(),
            admin_user_id: UserId("admin".into()),
            reason: "占位人工解释".into(),
            occurred_at: now,
        }),
    })
    .unwrap()
}

#[tokio::test]
async fn every_prefix_appends_one_actual_evaluation_and_identical_input_reuses_all_groups() {
    for merge in [false, true] {
        let mut cfg = cfg();
        cfg.merge.enabled = merge;
        cfg.merge.same_sender = true;
        let mut records = vec![];
        let mut memo = Memo::default();
        for index in 0..24 {
            records.push(record(
                index + 1,
                &format!("member{}", index % 2),
                "排 徽章 甲 1",
                1000 + index,
            ));
            let (_, next) = equivalent(&cfg, &records, &[], &memo).await;
            assert_eq!(next.work.evaluated_groups, 1);
            assert_eq!(next.work.reused_groups, index as usize);
            memo = next;
        }
        let (_, memo) = equivalent(&cfg, &records, &[], &memo).await;
        assert_eq!(memo.work.evaluated_groups, 0);
        assert_eq!(memo.work.reused_groups, records.len());
    }
}

#[tokio::test]
async fn tail_group_growth_does_not_reevaluate_the_prefix_or_merge_a_later_cancel() {
    let mut cfg = cfg();
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    let mut records: Vec<_> = (0..5)
        .map(|i| record(i + 1, &format!("member{i}"), "排 徽章 甲 1", 1000 + i))
        .collect();
    let (_, memo) = equivalent(&cfg, &records, &[], &Memo::default()).await;
    records.push(record(6, "member4", "排 徽章 甲 2", 2000));
    let (result, memo) = equivalent(&cfg, &records, &[], &memo).await;
    assert_eq!(memo.work.reused_groups, 4);
    assert_eq!(memo.work.evaluated_groups, 1);
    assert_eq!(result.messages.len(), 6);
    assert_eq!(result.messages[4].status, "Merged");
    records.push(record(7, "member4", "撤 徽章 甲 1", 3000));
    let (result, memo) = equivalent(&cfg, &records, &[], &memo).await;
    assert_eq!(memo.work.reused_groups, 5);
    assert_eq!(memo.work.evaluated_groups, 1);
    assert_eq!(result.messages[6].status, "Applied");
}

#[tokio::test]
async fn an_older_input_change_deletion_or_backdating_recomputes_the_entire_affected_suffix() {
    let cfg = cfg();
    let initial: Vec<_> = (0..10)
        .map(|i| record(i + 1, "member01", "排 徽章 甲 1", 1000 + i))
        .collect();
    let (_, before) = equivalent(&cfg, &initial, &[], &Memo::default()).await;
    for which in 0..4 {
        let mut changed = initial.clone();
        match which {
            0 => changed[4].text = "排 徽章 甲 2".into(), // same seq AND revision, not a cursor check
            1 => {
                changed.remove(4);
            }
            2 => changed[4].timestamp_ms = 999,
            _ => changed[4].routed = "drop:test".into(),
        }
        let (_, after) = equivalent(&cfg, &changed, &[], &before).await;
        let prefix = if which == 2 { 0 } else { 4 };
        assert_eq!(after.work.reused_groups, prefix);
        assert_eq!(
            after.work.evaluated_groups,
            changed.len() - prefix,
            "cannot resume reuse after the first change"
        );
    }
}

#[tokio::test]
async fn record_and_legacy_message_overrides_invalidate_the_correct_prefix_and_raw_audit_does_not()
{
    let cfg = cfg();
    let records: Vec<_> = (0..8)
        .map(|i| record(i + 1, "member01", "排 徽章 甲 1", 1000 + i))
        .collect();
    let (_, before) = equivalent(&cfg, &records, &[], &Memo::default()).await;
    let (_, same) = equivalent(
        &cfg,
        &records,
        &[json!({"opaque_id":9007199254740993_u64})],
        &before,
    )
    .await;
    assert_eq!(same.work.evaluated_groups, 0);
    assert_eq!(same.work.reused_groups, 8);
    for by_record in [false, true] {
        let parsed = RuleParser::parse("撤 徽章 甲 1", &cfg.round.to_items(), false);
        let journal = vec![override_event(&records[3], parsed, by_record)];
        let (result, changed) = equivalent(&cfg, &records, &journal, &before).await;
        assert_eq!(changed.work.reused_groups, 3);
        assert_eq!(changed.work.evaluated_groups, 5);
        assert_eq!(result.messages[3].status, "Applied");
        assert!(result.events.iter().any(|event| event.sequence == records[3].seq
            && matches!(&event.payload, DomainEvent::ClaimCancelled(cancel) if cancel.quantity == Some(1))));
    }
}

#[tokio::test]
async fn global_policy_changes_force_full_work_but_revision_metadata_and_current_names_are_not_stale(
) {
    let cfg = cfg();
    let mut records = vec![
        record(1, "member01", "排 徽章 甲 1", 1000),
        record(2, "member02", "排 徽章 甲 1", 2000),
    ];
    records[0].nickname = "初始称呼".into();
    let (_, before) = equivalent(&cfg, &records, &[], &Memo::default()).await;
    for which in 0..5 {
        let mut next = cfg.clone();
        match which {
            0 => next.round.priority_users.push("member01".into()),
            1 => next.gateway.whitelist_members = vec!["other".into()],
            2 => next.round.items[0].subitems[0].capacity = Some(3),
            3 => next.round.items[0].subitems[0].unit_price_cents += 10,
            _ => next.members.cn_overrides.push(crate::settings::CnOverride {
                user_id: "member01".into(),
                cn: "覆盖称呼".into(),
                aliases: vec![],
            }),
        }
        let (_, after) = equivalent(&next, &records, &[], &before).await;
        assert_eq!(after.work.evaluated_groups, 2);
        assert_eq!(after.work.reused_groups, 0);
    }
    let mut next = cfg.clone();
    next.revision += 5;
    next.active_round_id = None;
    let (result, same) = equivalent(&next, &records, &[], &before).await;
    assert_eq!(same.work.reused_groups, 2);
    assert_eq!(result.revision, next.revision);
    let mut later = record(3, "member01", "占位闲聊", 3000);
    later.nickname = "最新称呼".into();
    records.push(later);
    let (result, changed) = equivalent(&cfg, &records, &[], &before).await;
    assert_eq!(changed.work.reused_groups, 2);
    assert_eq!(changed.work.evaluated_groups, 1);
    assert_eq!(result.messages[0].nickname, "最新称呼");
    records.pop();
    let (result, removed) = equivalent(&cfg, &records, &[], &changed).await;
    assert_eq!(removed.work.reused_groups, 2);
    assert_eq!(result.messages[0].nickname, "初始称呼");
}

#[tokio::test]
async fn persisted_noncontiguous_units_and_unknown_artifacts_keep_full_replay_semantics() {
    use crate::messages::processing::{
        processing_members, AutomaticResult, ProcessingHistory, ProcessingOutcome,
        UnderstandingSource,
    };
    let cfg = cfg();
    let mut records = vec![
        record(1, "member01", "排 徽章 甲 1", 1000),
        record(2, "member02", "排 徽章 甲 1", 2000),
        record(3, "member01", "排 徽章 甲 2", 3000),
    ];
    let understanding = prepare_understanding(
        &cfg,
        &records[2],
        RuleParser::parse("排 徽章 甲 3", &cfg.round.to_items(), false),
        UnderstandingSource::Rules,
    )
    .await;
    let auto = AutomaticResult {
        format_version: 1,
        created_at_ms: 4000,
        result_id: "frozen-noncontiguous".into(),
        configuration_revision: cfg.revision,
        members: processing_members(&[&records[0], &records[2]]),
        understanding,
        original_outcome: ProcessingOutcome::new("Applied", "占位冻结组", None),
        manual_phase_at_ingest: None,
    };
    records[2].processing = Some(ProcessingHistory {
        results: vec![auto],
        ..Default::default()
    });
    let (original, before) = equivalent(&cfg, &records, &[], &Memo::default()).await;
    assert_eq!(before.work.evaluated_groups, 2);
    assert!(original
        .messages
        .iter()
        .any(|m| m.record_id == records[0].record_id && m.status == "Merged"));
    records.push(record(4, "member03", "排 徽章 甲 1", 4000));
    let (_, appended) = equivalent(&cfg, &records, &[], &before).await;
    assert_eq!(appended.work.reused_groups, 2);
    assert_eq!(appended.work.evaluated_groups, 1);
    records[2].processing.as_mut().unwrap().results[0].format_version = 2;
    let (unknown, changed) = equivalent(&cfg, &records, &[], &appended).await;
    assert!(unknown
        .messages
        .iter()
        .any(|m| m.record_id == records[2].record_id && m.status == "NeedConfirm"));
    assert_eq!(
        changed.work.reused_groups, 0,
        "分组首界变化不可复用旧的非连续单元"
    );
}

#[tokio::test]
async fn actual_tail_work_is_one_group_while_reference_still_evaluates_the_complete_source() {
    let cfg = cfg();
    let mut records: Vec<_> = (0..128)
        .map(|i| record(i + 1, &format!("member{}", i % 4), "排 徽章 甲 1", 1000 + i))
        .collect();
    let (_, before) = equivalent(&cfg, &records, &[], &Memo::default()).await;
    records.push(record(129, "member01", "撤 徽章 甲 1", 2000));
    let (_, tail) = equivalent(&cfg, &records, &[], &before).await;
    let (_, full) = compute_with_memo(
        &cfg,
        &records,
        &ParseOverrides::default(),
        None,
        &cfg.round.to_items(),
        &[],
        None,
    )
    .await;
    assert_eq!(tail.work.evaluated_groups, 1);
    assert_eq!(tail.work.reused_groups, 128);
    assert_eq!(full.work.evaluated_groups, 129);
    assert_eq!(full.work.reused_groups, 0);
}
