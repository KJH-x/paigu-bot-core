use super::{controls, RoundFixture};
use crate::domain::claim::Eligibility;
use crate::domain::ids::{RoundId, UserId};
use crate::messages::{
    processing::{
        processing_members, AutomaticResult, ProcessingHistory, Understanding, UnderstandingSource,
    },
    MessageRecord,
};
use crate::parser::{parsed_event::ParseOverrides, policy, rule_parser::RuleParser};
use crate::replay::session;
use crate::simulation::queue_file::QueueMessageRecord;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

pub(super) async fn prepare(
    source: &[QueueMessageRecord],
    fixture: &RoundFixture,
) -> anyhow::Result<(Vec<MessageRecord>, Vec<Eligibility>)> {
    let cfg = &fixture.config;
    let mut names: HashMap<String, HashSet<String>> = HashMap::new();
    for record in source {
        names
            .entry(record.nickname.clone())
            .or_default()
            .insert(record.user_id.clone());
    }
    let names = names
        .into_iter()
        .filter_map(|(name, users)| {
            (users.len() == 1).then(|| (name, users.into_iter().next().unwrap_or_default()))
        })
        .collect();
    let mut records = Vec::with_capacity(source.len());
    let mut eligibility = vec![];
    let mut seen = HashSet::new();
    let mut closed = false;
    let mut fund_users: HashSet<String> = cfg.round.priority_users.iter().cloned().collect();
    for (index, input) in source.iter().enumerate() {
        let at = DateTime::<Utc>::from_timestamp_millis(input.timestamp_ms)
            .ok_or_else(|| anyhow::anyhow!("第{}条夹具时间超出范围", index + 1))?;
        anyhow::ensure!(
            !input.user_id.trim().is_empty() && !input.message_id.trim().is_empty(),
            "第{}条夹具缺少身份/消息标识",
            index + 1
        );
        let mut record = MessageRecord {
            record_id: format!("fixture-{}-{}", cfg.round.round_id, index + 1),
            record_revision: 1,
            seq: index as i64 + 1,
            group_id: input.group_id.clone(),
            user_id: input.user_id.clone(),
            nickname: input.nickname.clone(),
            message_id: input.message_id.clone(),
            text: input.text.clone(),
            timestamp_ms: input.timestamp_ms,
            is_admin: input.is_admin,
            routed: "message".into(),
            status: String::new(),
            detail: String::new(),
            sender_user_id: None,
            delegation: None,
            ingress: None,
            sender_nickname: None,
            processing: Some(ProcessingHistory::default()),
        };
        let text = input.text.trim();
        let candidates = crate::settings::priority_candidates(cfg, &input.user_id, &input.nickname);
        let understood = if !policy::group_allowed(&cfg.gateway.whitelist_groups, &input.group_id) {
            Understanding::terminal(
                UnderstandingSource::Control,
                "Dropped",
                "非白名单/Drop",
                None,
            )
        } else if !policy::member_allowed(
            &cfg.gateway.whitelist_members,
            &candidates.iter().map(String::as_str).collect::<Vec<_>>(),
        ) {
            Understanding::terminal(
                UnderstandingSource::Control,
                "Dropped",
                "成员不在白名单",
                None,
            )
        } else if !seen.insert((input.group_id.clone(), input.message_id.clone())) {
            Understanding::terminal(
                UnderstandingSource::Duplicate,
                "Duplicate",
                "重复 message_id，已按幂等忽略",
                None,
            )
        } else if text.starts_with('/') {
            controls::command(
                input,
                &names,
                &fixture.items,
                &RoundId(cfg.round.round_id.clone()),
                &mut closed,
                &mut eligibility,
            )
        } else if closed {
            Understanding::terminal(
                UnderstandingSource::Control,
                "Rejected",
                "团已结团，拒绝排谷",
                None,
            )
        } else {
            if text.contains("购物金")
                && !text.contains("非购物金")
                && fund_users.insert(input.user_id.clone())
            {
                eligibility.push(controls::make_eligibility(
                    &RoundId(cfg.round.round_id.clone()),
                    &input.user_id,
                    10,
                    None,
                    "购物金(夹具自述)",
                    at,
                ));
            }
            let parsed = RuleParser::parse(text, &fixture.items, input.is_admin);
            if fixture.fixture_version == 1
                && parsed.intent == crate::parser::parsed_event::ParsedIntent::Modify
            {
                Understanding::terminal(
                    UnderstandingSource::Control,
                    "Unsupported",
                    "改单意图未在校验层实现",
                    None,
                )
            } else {
                session::prepare_with_catalog(
                    cfg,
                    &record,
                    parsed,
                    UnderstandingSource::Rules,
                    &fixture.items,
                )
                .await
            }
        };
        if understood.validation.status == "Dropped" {
            record.routed = "drop".into();
        } else if matches!(
            understood.source,
            UnderstandingSource::Control | UnderstandingSource::Duplicate
        ) {
            record.routed = "control".into();
        }
        records.push(record);
        save(&mut records, &[index], cfg, understood);
        if cfg.merge.enabled && cfg.merge.same_sender {
            let refs: Vec<_> = records.iter().collect();
            let groups = session::prospective_groups(cfg, &refs, &ParseOverrides::default());
            if let Some(group) = groups.last().filter(|g| g.len() > 1) {
                let mut representative = records[index].clone();
                representative.text = group
                    .iter()
                    .map(|&i| records[i].text.trim())
                    .collect::<Vec<_>>()
                    .join("\n");
                let parsed = RuleParser::parse(
                    &representative.text,
                    &fixture.items,
                    representative.is_admin,
                );
                let understood = session::prepare_with_catalog(
                    cfg,
                    &representative,
                    parsed,
                    UnderstandingSource::Rules,
                    &fixture.items,
                )
                .await;
                save(&mut records, group, cfg, understood);
            }
        }
    }
    // Configured priority is handled by the shared policy, not by names treated as user IDs.
    for record in &records {
        if policy::is_priority(cfg, &record.user_id, &record.nickname)
            && !eligibility.iter().any(|e| {
                e.user_id == UserId(record.user_id.clone())
                    && e.valid_from.is_some_and(|at| at.timestamp_millis() == 0)
            })
        {
            eligibility.push(controls::make_eligibility(
                &RoundId(cfg.round.round_id.clone()),
                &record.user_id,
                10,
                None,
                "预存(夹具配置)",
                DateTime::from_timestamp_millis(0)
                    .ok_or_else(|| anyhow::anyhow!("invalid epoch"))?,
            ));
        }
    }
    Ok((records, eligibility))
}

fn save(
    records: &mut [MessageRecord],
    group: &[usize],
    cfg: &crate::settings::AppConfig,
    understanding: Understanding,
) {
    let Some(&owner) = group.last() else {
        return;
    };
    let members = processing_members(&group.iter().map(|&i| &records[i]).collect::<Vec<_>>());
    let original_outcome =
        session::evaluate_understanding(cfg, &records[owner], &understanding, None).outcome;
    let result = AutomaticResult {
        format_version: 1,
        result_id: format!("auto-{}-{}", records[owner].record_id, group.len()),
        created_at_ms: records[owner].timestamp_ms,
        configuration_revision: cfg.revision,
        members,
        understanding,
        original_outcome: original_outcome.clone(),
        manual_phase_at_ingest: None,
    };
    records[owner].status = original_outcome.status;
    records[owner].detail = original_outcome.detail;
    records[owner]
        .processing
        .get_or_insert_with(ProcessingHistory::default)
        .results
        .push(result);
}
