use super::tests::{fixture, record, FixtureDir};
use super::*;
use crate::parser::parsed_event::ParseOverrides;
use crate::replay::session;
use crate::settings::CnOverride;

fn canonical(mut value: serde_json::Value) -> serde_json::Value {
    fn clear(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                for key in ["generated_at", "claim_id", "segment_id"] {
                    object.remove(key);
                }
                for child in object.values_mut() {
                    clear(child);
                }
            }
            serde_json::Value::Array(array) => {
                for child in array {
                    clear(child);
                }
            }
            _ => {}
        }
    }
    clear(&mut value);
    value
}

#[tokio::test]
async fn explicit_cn_priority_every_prefix_and_report_are_deterministic() {
    let dir = FixtureDir::new();
    let mut fixture = fixture();
    fixture.config.members.cn_overrides = vec![CnOverride {
        user_id: "u1".into(),
        cn: "成员01".into(),
        aliases: vec!["优先占位".into()],
    }];
    fixture.config.round.priority_users = vec!["优先占位".into()];
    let queue = vec![
        record(1, "u2", "排 通行证 1"),
        record(2, "u1", "排 通行证 2"),
        record(3, "u1", "撤 通行证 1"),
    ];
    let path = dir.queue(&queue);
    let (records, eligibility) = prepare::prepare(&queue, &fixture).await.unwrap();
    let full = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default(),
    )
    .await
    .unwrap();
    let report = replay_report::build(&fixture, &records, &full);
    for length in 1..=records.len() {
        let prefix = session::replay_prepared(
            &fixture.config,
            &records[..length],
            &fixture.items,
            &eligibility,
            &ParseOverrides::default(),
        )
        .await
        .unwrap();
        let step = &report.steps[length - 1];
        assert_eq!(
            canonical(serde_json::to_value(&prefix.board).unwrap()),
            canonical(serde_json::to_value(&step.allocation_snapshot).unwrap())
        );
        assert_eq!(step.before_version, length as i64 - 1);
        assert_eq!(step.after_version, length as i64);
    }
    assert_eq!(full.messages[1].nickname, "成员01");
    assert_eq!(
        full.board.item_allocations[0].boxes[0].slots[0].user_id_str(),
        Some("u1")
    );
    assert_eq!(
        full.board
            .user_summaries
            .iter()
            .find(|u| u.user_id.0 == "u1")
            .unwrap()
            .display_name,
        "成员01"
    );
    assert_eq!(
        report
            .final_settlement
            .as_ref()
            .unwrap()
            .user_bills
            .iter()
            .find(|u| u.user_id.0 == "u1")
            .unwrap()
            .display_name,
        "成员01"
    );
    assert_eq!(report.input_message_count, 3);
    let one = verify(&path, fixture).await.unwrap();
    let mut again = super::tests::fixture();
    again.config.members.cn_overrides = vec![CnOverride {
        user_id: "u1".into(),
        cn: "成员01".into(),
        aliases: vec!["优先占位".into()],
    }];
    again.config.round.priority_users = vec!["优先占位".into()];
    let two = verify(&path, again).await.unwrap();
    assert_eq!(
        serde_json::to_value(one).unwrap(),
        serde_json::to_value(two).unwrap()
    );
}

#[tokio::test]
async fn frozen_actions_survive_alias_retarget_and_bad_history_does_not_reparse() {
    let mut fixture = fixture();
    let (mut records, eligibility) = prepare::prepare(&[record(1, "u1", "排 通行证 1")], &fixture)
        .await
        .unwrap();
    let before = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default(),
    )
    .await
    .unwrap();
    let mut other = fixture.items[0].clone();
    other.item_id.0 = "other".into();
    other.name = "另一个商品".into();
    fixture.items[0].aliases.clear();
    fixture.items[0].name = "原商品改名".into();
    fixture.items.push(other);
    fixture.config.round.items = fixture::catalog_settings(&fixture.items);
    let after = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&before.events).unwrap(),
        serde_json::to_value(&after.events).unwrap()
    );
    assert!(after
        .board
        .item_allocations
        .iter()
        .find(|a| a.item_id.0 == "other")
        .unwrap()
        .boxes
        .is_empty());
    records[0].processing.as_mut().unwrap().results[0].format_version = 2;
    let bad = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default(),
    )
    .await
    .unwrap();
    assert_eq!(bad.outcomes[0].status, "NeedConfirm");
    assert!(bad.events.is_empty());
    records[0].processing = None;
    assert!(session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default()
    )
    .await
    .is_err());
}

#[tokio::test]
async fn empty_bad_inputs_and_private_cache_are_never_silently_accepted() {
    let dir = FixtureDir::new();
    let mut fixture = fixture();
    fixture.items.clear();
    fixture.config.round.items.clear();
    let empty = verify(&dir.queue(&[]), fixture).await.unwrap();
    assert_eq!(empty.final_snapshot.round_id.0, "round_x");
    assert!(empty.replay.steps.is_empty());
    assert_eq!(empty.final_snapshot.version, 0);
    let mut fixture = super::tests::fixture();
    let (records, eligibility) = prepare::prepare(&[record(1, "u1", "排 通行证 1")], &fixture)
        .await
        .unwrap();
    fixture.config.members.cache_path = "not-an-offline-source".into();
    assert!(session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default()
    )
    .await
    .is_err());
    let mut broken = record(1, "u1", "排 通行证 1");
    broken.timestamp_ms = i64::MAX;
    assert!(verify(&dir.queue(&[broken]), super::tests::fixture())
        .await
        .is_err());
}

struct NoModel;
#[async_trait::async_trait]
impl crate::llm::client::LlmClient for NoModel {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        _: &str,
        _: &str,
    ) -> anyhow::Result<String> {
        panic!("offline parity must not call a model");
    }
}

#[tokio::test]
async fn canonical_offline_matches_actual_live_pipeline_at_every_prefix_with_merge_and_modify() {
    for merge in [false, true] {
        let dir = FixtureDir::new();
        let mut fixture = fixture();
        fixture.config.round.round_id = format!("parity-{}", uuid::Uuid::new_v4());
        fixture.config.gateway.admin_commands_enabled = false;
        fixture.config.merge.enabled = merge;
        fixture.items[0].round_id.0 = fixture.config.round.round_id.clone();
        fixture.items[0].box_size = None;
        fixture.items[0].subitems = vec![crate::domain::item::ItemSubitem {
            subitem_id: "a".into(),
            name: "甲".into(),
            unit_price: crate::domain::money::MoneyCents(1000),
            capacity: Some(2),
            aliases: vec![],
        }];
        fixture.config.round.items = fixture::catalog_settings(&fixture.items);
        fixture.config.round.items[0].class = Some("B".into());
        fixture.config.members.cn_overrides = vec![CnOverride {
            user_id: "u1".into(),
            cn: "成员01".into(),
            aliases: vec![],
        }];
        let config_path = dir.0.join("config.json");
        std::fs::write(&config_path, serde_json::to_vec(&fixture.config).unwrap()).unwrap();
        let store = std::sync::Arc::new(crate::settings::ConfigStore::load(&config_path).unwrap());
        let pipeline = crate::llm::Pipeline::new_with_client_dir(
            store,
            std::sync::Arc::new(NoModel),
            dir.0.join("messages"),
        );
        let mut source = vec![];
        for (index, (user, text)) in [
            ("u1", "排 通行证 甲 1"),
            ("u1", "排 通行证 甲 1"),
            ("u2", "排 通行证 甲 1"),
            ("u1", "撤 通行证 1"),
            ("u2", "改 通行证 甲 2"),
        ]
        .into_iter()
        .enumerate()
        {
            let input = record(index as u64 + 1, user, text);
            pipeline
                .ingest(crate::bus::IncomingEvent {
                    group_id: input.group_id.clone(),
                    user_id: input.user_id.clone(),
                    nickname: input.nickname.clone(),
                    message_id: input.message_id.clone(),
                    text: input.text.clone(),
                    timestamp_ms: input.timestamp_ms,
                    is_admin: false,
                    raw: None,
                    sender_user_id: None,
                    ingress: None,
                    sender_nickname: None,
                })
                .await;
            source.push(input);
            let (records, eligibility) = prepare::prepare(&source, &fixture).await.unwrap();
            let offline = session::replay_prepared(
                &fixture.config,
                &records,
                &fixture.items,
                &eligibility,
                &ParseOverrides::default(),
            )
            .await
            .unwrap();
            let (version, board) = pipeline.board().await;
            assert_eq!(
                version,
                offline.version,
                "merge={merge} prefix={}",
                index + 1
            );
            assert_eq!(
                canonical(board),
                canonical(serde_json::to_value(&offline.board).unwrap()),
                "merge={merge} prefix={}",
                index + 1
            );
        }
    }
}

#[tokio::test]
async fn legacy_lock_duplicate_and_claim_cancel_are_group_boundaries() {
    let dir = FixtureDir::new();
    let mut fixture = fixture();
    fixture.config.merge.enabled = true;
    let mut close = record(2, "admin", "/结团");
    close.is_admin = true;
    let result = verify(
        &dir.queue(&[
            record(1, "u1", "排 通行证 1"),
            close,
            record(3, "u1", "排 通行证 1"),
            record(4, "u1", "撤 通行证 1"),
        ]),
        fixture,
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .outcomes
            .iter()
            .map(|o| o.status.as_str())
            .collect::<Vec<_>>(),
        ["Applied", "Applied", "Rejected", "Rejected"]
    );
    let mut fixture = super::tests::fixture();
    fixture.config.merge.enabled = true;
    let mut duplicate = record(2, "u1", "排 通行证 9");
    duplicate.message_id = "m1".into();
    let result = verify(
        &dir.queue(&[
            record(1, "u1", "排 通行证 1"),
            duplicate,
            record(3, "u1", "撤 通行证 1"),
        ]),
        fixture,
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .outcomes
            .iter()
            .map(|o| o.status.as_str())
            .collect::<Vec<_>>(),
        ["Applied", "Duplicate", "Applied"]
    );
    assert_eq!(
        result
            .final_snapshot
            .user_summaries
            .iter()
            .flat_map(|u| &u.items)
            .map(|i| i.quantity)
            .sum::<u32>(),
        0
    );
}

#[tokio::test]
async fn phase_foreign_group_and_whitelist_are_shared_without_admin_bypass() {
    let dir = FixtureDir::new();
    let mut fixture = fixture();
    fixture.config.round.items[0].class = Some("A".into());
    fixture.config.round.phases = vec![crate::round::PhaseWindow {
        phase: crate::round::RoundPhase::PhaseII,
        start_ms: 1_700_000_000_000,
        end_ms: i64::MAX,
    }];
    let mut foreign = record(1, "u1", "排 通行证 1");
    foreign.group_id = "foreign".into();
    let mut admin = record(2, "u2", "排 通行证 1");
    admin.is_admin = true;
    let result = verify(&dir.queue(&[foreign, admin]), fixture)
        .await
        .unwrap();
    assert_eq!(result.outcomes[0].status, "Dropped");
    let mut guarded = super::tests::fixture();
    guarded.config.gateway.whitelist_members = vec!["u1".into()];
    let mut denied_admin = record(1, "u2", "/加优先 用户=u1 等级=20 范围=全部");
    denied_admin.is_admin = true;
    let denied_result = verify(
        &dir.queue(&[denied_admin, record(2, "u1", "排 通行证 1")]),
        guarded,
    )
    .await
    .unwrap();
    assert_eq!(denied_result.outcomes[0].status, "Dropped");
    assert!(denied_result.eligibility.is_empty());
    assert_eq!(result.outcomes[1].status, "Rejected");
    assert_eq!(result.dropped_count, 1);
    assert_eq!(
        result.dropped_count + result.rejected_count,
        result.message_count
    );
    let mut fixture = super::tests::fixture();
    fixture.config.gateway.whitelist_members = vec!["u1".into()];
    let result = verify(&dir.queue(&[record(1, "u2", "排 通行证 1")]), fixture)
        .await
        .unwrap();
    assert_eq!(result.outcomes[0].status, "Dropped");
}

#[tokio::test]
async fn different_capacities_tail_fullbox_single_and_step_diff_keep_variant_identity() {
    let dir = FixtureDir::new();
    let mut fixture = fixture();
    fixture.items[0].box_size = None;
    fixture.items[0].subitems = [("a", "甲", 2), ("b", "乙", 3)]
        .into_iter()
        .map(|(id, name, capacity)| crate::domain::item::ItemSubitem {
            subitem_id: id.into(),
            name: name.into(),
            unit_price: crate::domain::money::MoneyCents(1000),
            capacity: Some(capacity),
            aliases: vec![],
        })
        .collect();
    let mut single = fixture.items[0].clone();
    single.item_id.0 = "single".into();
    single.name = "单品".into();
    single.aliases.clear();
    single.kind = crate::domain::item::ItemKind::Single;
    single.subitems.clear();
    single.max_quantity = Some(1);
    fixture.items.push(single);
    fixture.config.round.items = fixture::catalog_settings(&fixture.items);
    let source = vec![
        record(1, "u1", "排 通行证 甲 1"),
        record(2, "u2", "排 通行证 乙 1"),
        record(3, "u3", "包尾 通行证 甲 1"),
        record(4, "u4", "包盒 通行证 乙 1"),
        record(5, "u5", "排 单品 2"),
        record(6, "u1", "撤 通行证 1"),
    ];
    let result = verify(&dir.queue(&source), fixture).await.unwrap();
    assert!(
        result.outcomes.iter().all(|o| o.status == "Applied"),
        "{:?}",
        result.outcomes
    );
    let a = result
        .final_snapshot
        .item_allocations
        .iter()
        .find(|a| a.subitem_id.as_deref() == Some("a"))
        .unwrap();
    let b = result
        .final_snapshot
        .item_allocations
        .iter()
        .find(|a| a.subitem_id.as_deref() == Some("b"))
        .unwrap();
    let initial = &result.replay.steps[1].allocation_snapshot;
    assert!(initial
        .item_allocations
        .iter()
        .find(|row| row.subitem_id.as_deref() == Some("a"))
        .unwrap()
        .boxes
        .iter()
        .all(|column| column.slots.len() == 2));
    assert!(initial
        .item_allocations
        .iter()
        .find(|row| row.subitem_id.as_deref() == Some("b"))
        .unwrap()
        .boxes
        .iter()
        .all(|column| column.slots.len() == 3));
    assert!(a
        .boxes
        .iter()
        .flat_map(|c| &c.slots)
        .any(|s| s.slot_policy == crate::domain::claim::SlotPolicy::TailLocked));
    assert!(b
        .boxes
        .iter()
        .flat_map(|c| &c.slots)
        .any(|s| s.slot_policy == crate::domain::claim::SlotPolicy::FullBox));
    let single = result
        .final_snapshot
        .item_allocations
        .iter()
        .find(|a| a.item_id.0 == "single")
        .unwrap();
    assert_eq!(single.singles.iter().map(|s| s.quantity).sum::<u32>(), 1);
    assert_eq!(single.waiting.iter().map(|s| s.quantity).sum::<u32>(), 1);
    let changes = &result.replay.steps[1].state_diff.slot_changes;
    assert!(!changes.is_empty());
    assert!(
        changes.iter().all(|c| c.subitem_id.as_deref() == Some("b")),
        "{:?}",
        changes
    );
    let single_trace = &result.replay.steps[4].decision_trace.allocation_trace[0];
    assert_eq!(single_trace.quantity_requested, 2);
    assert_eq!(single_trace.quantity_allocated, 1);
}

#[tokio::test]
async fn removed_columns_and_same_user_new_claim_are_reported_and_names_follow_each_prefix() {
    let dir = FixtureDir::new();
    let fixture = fixture();
    let mut second = record(2, "u1", "改 通行证 1");
    second.nickname = "成员02".into();
    let result = verify(
        &dir.queue(&[
            record(1, "u1", "排 通行证 3"),
            second,
            record(3, "u1", "撤 通行证 1"),
        ]),
        fixture,
    )
    .await
    .unwrap();
    let modification_diff = crate::replay::state_diff::StateDiff::from_snapshots(
        &result.replay.steps[0].allocation_snapshot,
        &result.replay.steps[2].allocation_snapshot,
    );
    let change = &modification_diff.slot_changes;
    assert!(change.iter().any(|c| c.before.is_some()
        && c.after.is_some()
        && c.before.as_ref().unwrap().claim_id != c.after.as_ref().unwrap().claim_id));
    assert!(
        change
            .iter()
            .any(|c| c.before.is_some() && c.after.is_none()),
        "{:?}",
        change
    );
    assert_eq!(
        result.replay.steps[0].allocation_snapshot.user_summaries[0].display_name,
        "成员1"
    );
    assert_eq!(
        result.replay.steps[2].allocation_snapshot.user_summaries[0].display_name,
        "成员02"
    );
    assert!(result
        .replay
        .steps
        .last()
        .unwrap()
        .state_diff
        .slot_changes
        .iter()
        .any(|c| c.after.is_none()));
}

#[tokio::test]
async fn legacy_fixture_modify_is_not_reinterpreted_but_current_fixture_supports_it() {
    let dir = FixtureDir::new();
    let source = dir.queue(&[
        record(1, "u1", "排 通行证 1"),
        record(2, "u1", "改 通行证 2"),
    ]);
    let mut old = fixture();
    old.fixture_version = 1;
    let old = verify(&source, old).await.unwrap();
    let current = verify(&source, fixture()).await.unwrap();
    assert_eq!(old.outcomes[1].status, "Unsupported");
    assert_eq!(current.outcomes[1].status, "Applied");
    assert_eq!(old.final_snapshot.user_summaries[0].items[0].quantity, 1);
    assert_eq!(
        current.final_snapshot.user_summaries[0].items[0].quantity,
        2
    );
}

#[tokio::test]
async fn record_override_multi_action_uses_shared_recompute_and_keeps_later_cancel_and_original() {
    let mut fixture = fixture();
    let mut other = fixture.items[0].clone();
    other.item_id.0 = "other".into();
    other.name = "另商品".into();
    other.aliases = vec![];
    fixture.items.push(other);
    fixture.config.round.items = fixture::catalog_settings(&fixture.items);
    let (records, eligibility) = prepare::prepare(
        &[
            record(1, "u1", "排 通行证 1"),
            record(2, "u1", "撤 通行证 1"),
        ],
        &fixture,
    )
    .await
    .unwrap();
    let original = serde_json::to_value(&records).unwrap();
    let selection:crate::parser::action_override::ActionSelection=serde_json::from_value(serde_json::json!({"kind":"replace","note":"占位管理员解释","actions":[
        {"action_id":"a","operation":"claim","items":[{"line_id":"l1","item_id":"pass_sp","subitem_id":null,"quantity":2,"claim_type":"Split","slot_policy":"Normal","is_proxy_card":false,"notes":null}]},
        {"action_id":"b","operation":"claim","items":[{"line_id":"l2","item_id":"other","subitem_id":null,"quantity":1,"claim_type":"Split","slot_policy":"Normal","is_proxy_card":false,"notes":null}]}
    ]})).unwrap();
    let compiled =
        crate::parser::action_override::compile(&fixture.config, &records[0], &selection).unwrap();
    let mut choices = ParseOverrides::default();
    let saved = crate::parser::action_override::SavedSelection {
        action_selection_version: 1,
        transaction_id: "offline-manual".into(),
        request_hash: "test".into(),
        selection,
        events: compiled.events,
    };
    choices.apply_choice(
        &records[0].record_id,
        2,
        Some(crate::messages::transactions::Choice {
            value: serde_json::to_value(saved).unwrap(),
            reason: "占位管理员解释".into(),
            absorbed_by: None,
        }),
    );
    let result = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &choices,
    )
    .await
    .unwrap();
    assert_eq!(result.messages[0].record_revision, 2);
    assert_eq!(
        result.messages[0].manual_note.as_deref(),
        Some("占位管理员解释")
    );
    let quantities: std::collections::BTreeMap<_, _> = result.board.user_summaries[0]
        .items
        .iter()
        .map(|i| (i.item_id.0.as_str(), i.quantity))
        .collect();
    assert_eq!(quantities.get("pass_sp"), Some(&1));
    assert_eq!(quantities.get("other"), Some(&1));
    assert_eq!(serde_json::to_value(&records).unwrap(), original);
}

#[tokio::test]
async fn scoped_later_grant_preserves_global_priority_and_cannot_retroactively_prioritize() {
    let mut fixture = fixture();
    let mut other = fixture.items[0].clone();
    other.item_id.0 = "other".into();
    other.name = "另商品".into();
    other.aliases = vec![];
    fixture.items.push(other);
    fixture.config.round.items = fixture::catalog_settings(&fixture.items);
    fixture.config.round.priority_users = vec!["u1".into()];
    let source = [
        record(1, "u1", "排 通行证 1"),
        record(2, "u1", "排 另商品 1"),
        record(3, "u1", "排 另商品 1"),
    ];
    let (records, _) = prepare::prepare(&source, &fixture).await.unwrap();
    let grant = controls::make_eligibility(
        &crate::domain::ids::RoundId(fixture.config.round.round_id.clone()),
        "u1",
        20,
        Some(vec![crate::domain::ids::ItemId("other".into())]),
        "占位限定授权",
        chrono::DateTime::from_timestamp_millis(source[2].timestamp_ms).unwrap(),
    );
    let result = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &[grant],
        &ParseOverrides::default(),
    )
    .await
    .unwrap();
    let report = replay_report::build(&fixture, &records, &result);
    assert_eq!(
        report
            .steps
            .iter()
            .map(|s| s
                .decision_trace
                .priority_trace
                .as_ref()
                .unwrap()
                .priority_level)
            .collect::<Vec<_>>(),
        [10, 10, 20]
    );
}

#[tokio::test]
async fn saved_ambiguous_understanding_stays_need_confirm_after_alias_changes() {
    let mut fixture = fixture();
    let (mut records, eligibility) = prepare::prepare(&[record(1, "u1", "排 通行证 1")], &fixture)
        .await
        .unwrap();
    let mut parsed =
        crate::parser::rule_parser::RuleParser::parse(&records[0].text, &fixture.items, false);
    parsed.ambiguous_parts = vec!["占位同名候选待确认".into()];
    let understood = session::prepare_with_catalog(
        &fixture.config,
        &records[0],
        parsed,
        crate::messages::processing::UnderstandingSource::Rules,
        &fixture.items,
    )
    .await;
    assert_eq!(understood.validation.status, "NeedConfirm");
    let auto = &mut records[0].processing.as_mut().unwrap().results[0];
    auto.original_outcome = understood.validation.clone();
    auto.understanding = understood;
    fixture.items[0].aliases.clear();
    fixture.config.round.items = fixture::catalog_settings(&fixture.items);
    let result = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default(),
    )
    .await
    .unwrap();
    assert_eq!(result.outcomes[0].status, "NeedConfirm");
    assert!(result.events.is_empty());
    assert_eq!(
        result.messages[0]
            .original_automatic
            .as_ref()
            .unwrap()
            .outcome
            .status,
        "NeedConfirm"
    );
}
