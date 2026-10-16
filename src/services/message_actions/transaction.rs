use super::*;
use crate::messages::processing::ProcessingMember;
use crate::messages::transactions::{self as tx, Change, Checkpoint, Choice, Transaction};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoInput {
    pub transaction_id: String,
    pub round_id: String,
    pub config_revision: u64,
    pub undo_token: String,
}

fn last_manual(events: &[EventEnvelope]) -> Option<&EventEnvelope> {
    events.iter().rev().find(
        |e| matches!(&e.payload,DomainEvent::ParseOverride(o) if o.target_record_id.is_some()),
    )
}

pub(super) fn undo_view(events: &[EventEnvelope]) -> Value {
    let Some(t) = last_manual(events)
        .and_then(tx::decode)
        .filter(|t| t.kind == "save")
    else {
        return json!({"available":false,"reason":"没有可撤回的最近人工事务，或旧保存缺少检查点"});
    };
    json!({"available":true,"undo_token":t.transaction_id,"main_record_id":t.main_record_id,"records":t.changes.len()})
}

pub async fn undo_status(state: &ApiState, round_id: &str) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if cfg.round.round_id != round_id {
        return Err(ServiceError::Conflict(json!({"error":"round_changed"})));
    }
    let events = journal(&state.messages, round_id).await?;
    let mut view = undo_view(&events);
    view["round_id"] = json!(round_id);
    view["config_revision"] = json!(cfg.revision);
    if let Some(t) = last_manual(&events).and_then(tx::decode) {
        if check_lock(state, &cfg).await.is_err()
            || tx::hash(&tx::policies(&cfg))? != t.checkpoint.policy_hash
            || tx::phase_hash(&events)? != t.checkpoint.phase_hash
        {
            view["available"] = json!(false);
            view["reason"] = json!("轮次已锁定或业务配置/阶段变化，旧检查点不可用");
        }
    }
    Ok(view)
}

fn duplicate(events: &[EventEnvelope], id: &str, hash: &str) -> Result<bool, ServiceError> {
    for event in events {
        if let Some(t) = tx::decode(event) {
            if t.transaction_id == id {
                if t.request_hash != hash {
                    return Err(ServiceError::Conflict(
                        json!({"error":"transaction_id_conflict","message":"同一提交标识不能用于不同内容"}),
                    ));
                }
                return Ok(true);
            }
        }
        if let DomainEvent::ParseOverride(o) = &event.payload {
            if let Ok(s) =
                serde_json::from_value::<SavedSelection>(o.corrected_parsed_message.clone())
            {
                if s.transaction_id == id {
                    return Err(ServiceError::Conflict(
                        json!({"error":"transaction_id_conflict"}),
                    ));
                }
            }
        }
    }
    Ok(false)
}

fn check_versions(
    cfg: &AppConfig,
    round: &str,
    revision: u64,
    id: &str,
) -> Result<(), ServiceError> {
    if cfg.round.round_id != round {
        return Err(ServiceError::Conflict(
            json!({"error":"round_changed","round_id":cfg.round.round_id}),
        ));
    }
    if cfg.revision != revision {
        return Err(ServiceError::StaleRevision(cfg.revision));
    }
    if !action_override::valid_id(id) {
        return Err(ServiceError::BadRequest("提交标识无效".into()));
    }
    Ok(())
}

async fn check_lock(state: &ApiState, cfg: &AppConfig) -> Result<(), ServiceError> {
    if state.pipeline.is_locked().await
        || crate::services::settlement::round_locked(&cfg.round.round_id)?
    {
        return Err(ServiceError::Locked("轮次已锁定，不能修订/撤回动作".into()));
    }
    Ok(())
}

fn checkpoint(
    cfg: &AppConfig,
    records: &[MessageRecord],
    events: &[EventEnvelope],
    overrides: &ParseOverrides,
    computed: &ReplayResult,
) -> Result<Checkpoint, ServiceError> {
    let policies = tx::policies(cfg);
    Ok(Checkpoint {
        policy_hash: tx::hash(&policies)?,
        policies,
        message_refs: records.iter().map(ProcessingMember::from_record).collect(),
        automatic_refs: records
            .iter()
            .filter_map(|r| {
                r.processing.as_ref().map(|p| {
                    (
                        r.record_id.clone(),
                        p.results.iter().map(|a| a.result_id.clone()).collect(),
                    )
                })
            })
            .collect(),
        event_cursor: events.len(),
        event_prefix_hash: tx::hash(&events)?,
        phase_hash: tx::phase_hash(events)?,
        selections: overrides.choices().clone(),
        events: computed.events.clone(),
        board: computed.board.clone(),
        settlement: crate::services::settlement::checkpoint(&cfg.round.round_id)?,
    })
}

async fn publish(
    state: &ApiState,
    cfg: &AppConfig,
    records: &[MessageRecord],
    events: &[EventEnvelope],
    t: Transaction,
    mut overrides: ParseOverrides,
) -> Result<Value, ServiceError> {
    for c in &t.changes {
        overrides.apply_choice(&c.record_id, c.revision, c.after.clone());
    }
    let computed = compute(&state.messages, cfg, records, &overrides).await?;
    let now = Utc::now();
    let id = EventId(uuid::Uuid::new_v4().to_string());
    let main = records
        .iter()
        .find(|r| r.record_id == t.main_record_id)
        .ok_or_else(|| ServiceError::NotFound("主消息不存在".into()))?;
    let event = EventEnvelope {
        event_id: id.clone(),
        round_id: RoundId(cfg.round.round_id.clone()),
        group_id: main.group_id.clone(),
        user_id: UserId("admin".into()),
        raw_message_id: Some(main.message_id.clone()),
        event_type: "parse_override".into(),
        effective_at: now,
        sequence: crate::messages::next_seq(),
        status: EventStatus::Active,
        payload: DomainEvent::ParseOverride(ParseOverrideEvent {
            event_id: id,
            round_id: RoundId(cfg.round.round_id.clone()),
            target_raw_message_id: main.message_id.clone(),
            target_record_id: Some(main.record_id.clone()),
            target_record_revision: t
                .changes
                .iter()
                .find(|c| c.record_id == main.record_id)
                .map(|c| c.revision),
            corrected_parsed_message: serde_json::to_value(&t).map_err(anyhow::Error::from)?,
            admin_user_id: UserId("admin".into()),
            reason: if t.kind == "undo" {
                "撤回最近人工事务".into()
            } else {
                "完整动作事务".into()
            },
            occurred_at: now,
        }),
    };
    state
        .messages
        .append_durable_event(
            &cfg.round.round_id,
            &serde_json::to_value(&event).map_err(anyhow::Error::from)?,
        )
        .await?;
    // Everything after the durable record is a rebuildable projection, not another filesystem commit.
    state.pipeline.apply_replay(&computed, records).await;
    replay::remember_replay(&computed);
    let mut next = events.to_vec();
    next.push(event);
    Ok(
        json!({"ok":true,"transaction_id":t.transaction_id,"message":projected(main,&computed),"result":replay::replay_result_json(&computed),"undo":undo_view(&next)}),
    )
}

pub(super) async fn save(
    state: &ApiState,
    key: &str,
    body: SaveActions,
) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if cfg.round.round_id != body.round_id {
        return Err(ServiceError::Conflict(json!({"error":"round_changed"})));
    }
    let records = state.messages.read_only(&body.round_id).await?;
    let main = records
        .iter()
        .find(|r| r.record_id == key)
        .ok_or_else(|| ServiceError::NotFound("消息不存在".into()))?;
    let events = journal(&state.messages, &body.round_id).await?;
    let hash = tx::hash(&(key, &body))?;
    let overrides = crate::parser::parsed_event::collect_parse_overrides(&events);
    if duplicate(&events, &body.transaction_id, &hash)? {
        let computed = compute(&state.messages, &cfg, &records, &overrides).await?;
        state.pipeline.apply_replay(&computed, &records).await;
        return Ok(
            json!({"ok":true,"deduplicated":true,"transaction_id":body.transaction_id,"message":projected(main,&computed),"result":replay::replay_result_json(&computed),"undo":undo_view(&events)}),
        );
    }
    check_versions(
        &cfg,
        &body.round_id,
        body.config_revision,
        &body.transaction_id,
    )?;
    check_lock(state, &cfg).await?;
    let targets: Vec<_> = std::iter::once(RecordVersion {
        record_id: key.into(),
        record_revision: body.record_revision,
    })
    .chain(body.supplements.iter().cloned())
    .chain(body.release_supplements.iter().cloned())
    .collect();
    let ids: HashSet<_> = targets.iter().map(|r| r.record_id.as_str()).collect();
    if ids.len() != targets.len() || targets.len() > 64 {
        return Err(ServiceError::BadRequest(
            "补充消息重复，或超过 64 条".into(),
        ));
    }
    if !body.supplements.is_empty() && !matches!(body.selection, ActionSelection::Replace { .. }) {
        return Err(ServiceError::BadRequest(
            "补充合并须完整替换主消息动作".into(),
        ));
    }
    let releases: HashSet<_> = body
        .release_supplements
        .iter()
        .map(|r| r.record_id.as_str())
        .collect();
    if matches!(body.selection, ActionSelection::Automatic { .. })
        && overrides
            .choices()
            .values()
            .any(|c| c.absorbed_by.as_deref() == Some(key))
    {
        return Err(ServiceError::BadRequest(
            "此主条有关联补充；整组恢复请使用最近事务撤回，或先显式解除补充并核对动作".into(),
        ));
    }
    for v in &targets {
        let r = records
            .iter()
            .find(|r| r.record_id == v.record_id)
            .ok_or_else(|| ServiceError::NotFound("选定消息不存在".into()))?;
        if r.group_id != main.group_id || (r.timestamp_ms, r.seq) < (main.timestamp_ms, main.seq) {
            return Err(ServiceError::BadRequest(
                "主条须为选定范围最早消息，且所有记录来自同一群".into(),
            ));
        }
        if overrides.record_revision(&r.record_id, r.record_revision) != v.record_revision {
            return Err(ServiceError::Conflict(
                json!({"error":"stale_record_revision","record_id":r.record_id,"message":"选定消息已变化，请保留草稿后核对"}),
            ));
        }
        if let Some(owner) = overrides.absorbed_by(&r.record_id) {
            if owner != key {
                return Err(ServiceError::Conflict(
                    json!({"error":"already_absorbed","main_record_id":owner,"message":"此消息已归属另一主条，请在原主条操作或撤回原事务"}),
                ));
            }
        }
        if releases.contains(r.record_id.as_str())
            && overrides.absorbed_by(&r.record_id) != Some(key)
        {
            return Err(ServiceError::BadRequest(
                "只能解除当前主条的既有补充".into(),
            ));
        }
        if r.record_id != key
            && overrides
                .choices()
                .values()
                .any(|c| c.absorbed_by.as_deref() == Some(&r.record_id))
        {
            return Err(ServiceError::BadRequest(
                "不能把已有补充的主条再吸收到另一主条".into(),
            ));
        }
    }
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|r| (r.timestamp_ms, r.seq));
    let groups = session::message_groups(&cfg, &ordered, &overrides);
    for g in &groups {
        if g.iter()
            .any(|&i| ids.contains(ordered[i].record_id.as_str()))
            && !g
                .iter()
                .all(|&i| ids.contains(ordered[i].record_id.as_str()))
        {
            return Err(ServiceError::Conflict(
                json!({"error":"auto_group_requires_transaction","message":"须选择该自动组的全部成员","required_records":g.iter().map(|&i| ordered[i].record_id.clone()).collect::<Vec<_>>()}),
            ));
        }
    }
    let own_group = groups
        .iter()
        .find(|g| g.iter().any(|&i| ordered[i].record_id == key))
        .ok_or_else(|| ServiceError::BadRequest("主消息缺少执行单元".into()))?;
    let members: Vec<_> = own_group.iter().map(|&i| ordered[i]).collect();
    let auto = members
        .last()
        .and_then(|r| r.processing.as_ref())
        .and_then(|p| p.latest_for(&members));
    let mut understood = if matches!(body.selection, ActionSelection::Automatic { .. }) {
        auto.map(|a| a.understanding.clone()).ok_or_else(|| {
            ServiceError::Unprocessable("原自动基线缺失或不匹配，无法恢复；可直接人工补正".into())
        })?
    } else {
        action_override::compile(&cfg, main, &body.selection).map_err(ServiceError::BadRequest)?
    };
    let baseline = compute(&state.messages, &cfg, &records, &overrides).await?;
    let primary = overrides
        .selection_for_record(key)
        .and_then(|s| {
            s.events.iter().find_map(|e| match &e.payload {
                DomainEvent::ClaimCreated(c) if e.event_id.0 == format!("evt-{key}-primary") => {
                    Some(c.claim_id.clone())
                }
                _ => None,
            })
        })
        .or_else(|| {
            auto.and_then(|a| {
                a.understanding
                    .events
                    .iter()
                    .find_map(|e| match &e.payload {
                        DomainEvent::ClaimCreated(c) => Some(c.claim_id.clone()),
                        _ => None,
                    })
            })
        });
    for event in &mut understood.events {
        if event.event_id.0 == format!("evt-{key}-primary") {
            if let (Some(id), DomainEvent::ClaimCreated(c)) = (&primary, &mut event.payload) {
                c.claim_id = id.clone();
            }
        }
        if let DomainEvent::ClaimCancelled(c) = &event.payload {
            if let Some(id) = &c.target_claim_id {
                let valid = baseline.events.iter().any(|e| {
                    e.user_id.0 == main.user_id
                        && (e.effective_at.timestamp_millis(), e.sequence)
                            < (main.timestamp_ms, main.seq)
                        && matches!(&e.payload,DomainEvent::ClaimCreated(c) if &c.claim_id==id)
                });
                let unchanged=overrides.selection_for_record(key).is_some_and(|s|s.events.iter().any(|e|matches!(&e.payload,DomainEvent::ClaimCancelled(c) if c.target_claim_id.as_ref()==Some(id))));
                if !valid && !unchanged {
                    return Err(ServiceError::BadRequest(
                        "只能撤销本成员在原消息之前的申购".into(),
                    ));
                }
            }
        }
    }
    let mut changes = vec![];
    for target in &targets {
        let r = records
            .iter()
            .find(|r| r.record_id == target.record_id)
            .ok_or_else(|| ServiceError::NotFound("选定消息不存在".into()))?;
        let is_main = r.record_id == key;
        if releases.contains(r.record_id.as_str()) {
            // Recover the exact choice before this binding, not an invented automatic result.
            let previous = events.iter().rev().filter_map(tx::decode).flat_map(|t| t.changes)
                .find(|c| c.record_id==r.record_id && c.after.as_ref().and_then(|c| c.absorbed_by.as_deref())==Some(key)
                    && c.before.as_ref().and_then(|c| c.absorbed_by.as_deref())!=Some(key))
                .ok_or_else(|| ServiceError::Conflict(json!({"error":"supplement_baseline_missing","message":"补充的原选择缺失，未作猜测恢复"})))?;
            changes.push(Change {
                record_id: r.record_id.clone(),
                revision: overrides
                    .record_revision(&r.record_id, r.record_revision)
                    .saturating_add(1),
                before: overrides.choices().get(&r.record_id).cloned(),
                after: previous.before,
            });
            continue;
        }
        let selection = if is_main {
            body.selection.clone()
        } else {
            ActionSelection::Reject {
                note: format!("补充已纳入主消息 #{}", main.seq),
            }
        };
        let saved = SavedSelection {
            action_selection_version: 1,
            transaction_id: body.transaction_id.clone(),
            request_hash: hash.clone(),
            selection: selection.clone(),
            events: if is_main {
                understood.events.clone()
            } else {
                vec![]
            },
        };
        changes.push(Change {
            record_id: r.record_id.clone(),
            revision: overrides
                .record_revision(&r.record_id, r.record_revision)
                .saturating_add(1),
            before: overrides.choices().get(&r.record_id).cloned(),
            after: Some(Choice {
                value: serde_json::to_value(saved).map_err(anyhow::Error::from)?,
                reason: selection.note().into(),
                absorbed_by: if is_main { None } else { Some(key.into()) },
            }),
        });
    }
    let t = Transaction {
        action_transaction_version: 1,
        transaction_id: body.transaction_id,
        request_hash: hash,
        kind: "save".into(),
        main_record_id: key.into(),
        config_revision: cfg.revision,
        undo_of: None,
        changes,
        checkpoint: checkpoint(&cfg, &records, &events, &overrides, &baseline)?,
    };
    publish(state, &cfg, &records, &events, t, overrides).await
}

pub async fn undo(state: &ApiState, body: UndoInput) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if cfg.round.round_id != body.round_id {
        return Err(ServiceError::Conflict(json!({"error":"round_changed"})));
    }
    let records = state.messages.read_only(&body.round_id).await?;
    let events = journal(&state.messages, &body.round_id).await?;
    let hash = tx::hash(&body)?;
    let overrides = crate::parser::parsed_event::collect_parse_overrides(&events);
    if duplicate(&events, &body.transaction_id, &hash)? {
        let computed = compute(&state.messages, &cfg, &records, &overrides).await?;
        state.pipeline.apply_replay(&computed, &records).await;
        return Ok(
            json!({"ok":true,"deduplicated":true,"result":replay::replay_result_json(&computed),"undo":undo_view(&events)}),
        );
    }
    check_versions(
        &cfg,
        &body.round_id,
        body.config_revision,
        &body.transaction_id,
    )?;
    check_lock(state, &cfg).await?;
    let original = last_manual(&events)
        .and_then(tx::decode)
        .filter(|t| t.kind == "save" && t.transaction_id == body.undo_token)
        .ok_or_else(|| {
            ServiceError::Conflict(
                json!({"error":"stale_undo","message":"最近人工事务已变化或已撤回"}),
            )
        })?;
    let cp = &original.checkpoint;
    if tx::hash(&tx::policies(&cfg))? != cp.policy_hash || tx::phase_hash(&events)? != cp.phase_hash
    {
        return Err(ServiceError::Conflict(
            json!({"error":"undo_policy_changed","message":"业务配置或阶段已变化，不能使用旧检查点撤回"}),
        ));
    }
    if events.len() < cp.event_cursor
        || tx::hash(&&events[..cp.event_cursor])? != cp.event_prefix_hash
    {
        return Err(ServiceError::Conflict(
            json!({"error":"undo_journal_changed","message":"原事件日志已变化，检查点不可用"}),
        ));
    }
    for reference in &cp.message_refs {
        if !records
            .iter()
            .any(|r| ProcessingMember::from_record(r) == *reference)
        {
            return Err(ServiceError::Conflict(
                json!({"error":"undo_input_changed","message":"原消息输入已变化，检查点不可用"}),
            ));
        }
    }
    for (id, refs) in &cp.automatic_refs {
        let valid = records
            .iter()
            .find(|r| &r.record_id == id)
            .and_then(|r| r.processing.as_ref())
            .is_some_and(|p| {
                refs.iter()
                    .all(|id| p.results.iter().any(|r| &r.result_id == id))
            });
        if !valid {
            return Err(ServiceError::Conflict(
                json!({"error":"undo_automatic_missing","message":"原自动产物缺失，不能猜测恢复"}),
            ));
        }
    }
    let mut changes = vec![];
    for c in &original.changes {
        let r = records
            .iter()
            .find(|r| r.record_id == c.record_id)
            .ok_or_else(|| ServiceError::NotFound("撤回目标不存在".into()))?;
        if overrides.record_revision(&r.record_id, r.record_revision) != c.revision {
            return Err(ServiceError::Conflict(
                json!({"error":"stale_undo","message":"撤回目标已有新的人工修订"}),
            ));
        }
        changes.push(Change {
            record_id: c.record_id.clone(),
            revision: c.revision.saturating_add(1),
            before: overrides.choices().get(&c.record_id).cloned(),
            after: c.before.clone(),
        });
    }
    let baseline = compute(&state.messages, &cfg, &records, &overrides).await?;
    let t = Transaction {
        action_transaction_version: 1,
        transaction_id: body.transaction_id,
        request_hash: hash,
        kind: "undo".into(),
        main_record_id: original.main_record_id,
        config_revision: cfg.revision,
        undo_of: Some(original.transaction_id),
        changes,
        checkpoint: checkpoint(&cfg, &records, &events, &overrides, &baseline)?,
    };
    publish(state, &cfg, &records, &events, t, overrides).await
}
