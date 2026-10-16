use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{messages::message_view, replay, ServiceError};
use crate::api::ApiState;
use crate::domain::event::{DomainEvent, EventEnvelope, EventStatus, ParseOverrideEvent};
use crate::domain::ids::{EventId, RoundId, UserId};
use crate::messages::{MessageLog, MessageRecord};
use crate::parser::action_override::{self, ActionSelection, SavedSelection};
use crate::parser::parsed_event::ParseOverrides;
use crate::replay::session::{self, ReplayOverrides, ReplayResult};
use crate::settings::AppConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SaveActions {
    pub transaction_id: String,
    pub round_id: String,
    pub config_revision: u64,
    pub record_revision: u64,
    pub selection: ActionSelection,
}

async fn journal(store: &MessageLog, round: &str) -> Result<Vec<EventEnvelope>, ServiceError> {
    Ok(store
        .read_raw_events(round, 0)
        .await?
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

pub fn projected(record: &MessageRecord, computed: &ReplayResult) -> Value {
    let mut view = message_view(record);
    if let Some(current) = computed
        .messages
        .iter()
        .find(|m| m.record_id == record.record_id)
    {
        view["record_revision"] = json!(current.record_revision);
        view["status"] = json!(current.status);
        view["detail"] = json!(current.detail);
        view["manual_note"] = json!(current.manual_note);
        view["round_id"] = json!(computed.round_id);
    }
    view
}

async fn compute(
    store: &MessageLog,
    cfg: &AppConfig,
    records: &[MessageRecord],
    overrides: &ParseOverrides,
) -> anyhow::Result<ReplayResult> {
    session::replay_messages_with_parse(
        cfg,
        records,
        ReplayOverrides::default(),
        overrides,
        session::load_manual_phase(store, &cfg.round.round_id).await,
    )
    .await
}

pub async fn read(state: &ApiState, key: &str, round_id: &str) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if cfg.round.round_id != round_id {
        return Err(ServiceError::Conflict(
            json!({"error":"round_changed", "round_id":cfg.round.round_id}),
        ));
    }
    let records = state.messages.read_only(round_id).await?;
    let record = records
        .iter()
        .find(|r| r.record_id == key)
        .ok_or_else(|| ServiceError::NotFound("消息不存在或缺少稳定身份".into()))?;
    let overrides = crate::parser::parsed_event::collect_parse_overrides(
        &journal(&state.messages, round_id).await?,
    );
    let computed = compute(&state.messages, &cfg, &records, &overrides).await?;
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|r| (r.timestamp_ms, r.seq));
    let groups = session::message_groups(&cfg, &ordered, &overrides);
    let group = groups
        .iter()
        .find(|g| g.iter().any(|&i| ordered[i].record_id == key))
        .ok_or_else(|| ServiceError::BadRequest("消息缺少执行单元".into()))?;
    let members: Vec<_> = group.iter().map(|&i| ordered[i]).collect();
    let rep = *members
        .last()
        .ok_or_else(|| ServiceError::BadRequest("执行单元为空".into()))?;
    let auto = rep.processing.as_ref().and_then(|p| p.latest_for(&members));
    let original = record.processing.as_ref().and_then(|p| p.results.first());
    let manual = overrides.for_record(key);
    let saved = overrides.selection_for_record(key);
    let note = overrides.reason_for_record(key).unwrap_or("").to_string();
    let mut understood = None;
    if let Some(parsed) = manual {
        understood = Some(
            session::prepare_understanding(
                &cfg,
                record,
                parsed.clone(),
                crate::messages::processing::UnderstandingSource::Control,
            )
            .await,
        );
    }
    let effective = if overrides.invalid_selection(key) {
        None
    } else {
        understood
            .as_ref()
            .or_else(|| auto.map(|r| &r.understanding))
    };
    let editable = saved
        .map(|s| s.selection.clone())
        .unwrap_or_else(|| ActionSelection::Replace {
            actions: effective.map_or_else(Vec::new, |u| {
                action_override::editable(&u.events, u.is_modify)
            }),
            note: note.clone(),
        });
    // Opening automatic mode still starts from its current actions; saving is explicit.
    let draft = if matches!(editable, ActionSelection::Automatic { .. }) {
        ActionSelection::Replace {
            actions: auto.map_or_else(Vec::new, |a| {
                action_override::editable(&a.understanding.events, a.understanding.is_modify)
            }),
            note: note.clone(),
        }
    } else {
        editable.clone()
    };
    let targets: Vec<_> = computed.events.iter().filter(|e| e.user_id.0 == record.user_id && (e.effective_at.timestamp_millis(), e.sequence) < (record.timestamp_ms, record.seq)).filter_map(|e| match &e.payload {
        DomainEvent::ClaimCreated(c) => Some(json!({"claim_id":c.claim_id.0,"summary":action_override::describe_event(e, &cfg),"message_seq":e.sequence})), _ => None,
    }).collect();
    let source = if overrides.invalid_selection(key) {
        "invalid_manual"
    } else if saved.is_some_and(|s| !matches!(s.selection, ActionSelection::Automatic { .. }))
        || manual.is_some()
    {
        "manual"
    } else if auto.is_some() {
        "saved_automatic"
    } else {
        "legacy_or_missing"
    };
    let message = projected(record, &computed);
    let current_summary = computed
        .events
        .iter()
        .filter(|e| e.sequence == record.seq && e.user_id.0 == record.user_id)
        .map(|e| action_override::describe_event(e, &cfg))
        .collect::<Vec<_>>()
        .join("；");
    let original_summary = original.map_or_else(String::new, |a| {
        a.understanding
            .events
            .iter()
            .map(|e| action_override::describe_event(e, &cfg))
            .collect::<Vec<_>>()
            .join("；")
    });
    Ok(json!({
        "round_id":round_id,"config_revision":cfg.revision,"message":message,
        "original_automatic":original,"current_automatic":auto,
        "original_action_summary":original_summary,
        "automatic_history_count":record.processing.as_ref().map_or(0, |p| p.results.len()),
        "automatic_missing":original.is_none(),"automatic_matches":auto.is_some(),
        "effective_source":source,"effective_parsed":if saved.is_some_and(|s| !matches!(s.selection, ActionSelection::Automatic { .. })) { None } else { manual.or_else(|| auto.and_then(|a| a.understanding.parsed.as_ref())) },
        "manual_note":note,"selection":saved.map(|s| &s.selection),"editable_selection":draft,
        "catalog":cfg.round.items,"cancel_targets":targets,
        "automatic_group":{"representative_record_id":rep.record_id,"members":members.iter().map(|r| projected(r, &computed)).collect::<Vec<_>>()},
        "editable":members.len()==1,"can_restore_automatic":members.len()==1 && auto.is_some(),
        "current_outcome":{"status":message["status"],"detail":message["detail"],"summary":current_summary}
    }))
}

pub async fn save(state: &ApiState, key: &str, body: SaveActions) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if cfg.round.round_id != body.round_id {
        return Err(ServiceError::Conflict(
            json!({"error":"round_changed","round_id":cfg.round.round_id}),
        ));
    }
    if !action_override::valid_id(&body.transaction_id) {
        return Err(ServiceError::BadRequest("提交标识无效".into()));
    }
    let records = state.messages.read_all(&cfg.round.round_id).await?;
    let record = records
        .iter()
        .find(|r| r.record_id == key)
        .ok_or_else(|| ServiceError::NotFound("消息不存在".into()))?;
    let events = journal(&state.messages, &cfg.round.round_id).await?;
    let hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(key, &body)).map_err(anyhow::Error::from)?)
    );
    let mut overrides = crate::parser::parsed_event::collect_parse_overrides(&events);
    for event in &events {
        if let DomainEvent::ParseOverride(old) = &event.payload {
            if let Ok(saved) =
                serde_json::from_value::<SavedSelection>(old.corrected_parsed_message.clone())
            {
                if saved.transaction_id == body.transaction_id {
                    if saved.request_hash != hash {
                        return Err(ServiceError::Conflict(
                            json!({"error":"transaction_id_conflict","message":"同一提交标识不能用于不同内容"}),
                        ));
                    }
                    let computed = compute(&state.messages, &cfg, &records, &overrides).await?;
                    state.pipeline.apply_replay(&computed, &records).await;
                    return Ok(
                        json!({"ok":true,"deduplicated":true,"transaction_id":body.transaction_id,"message":projected(record,&computed),"result":replay::replay_result_json(&computed)}),
                    );
                }
            }
        }
    }
    if cfg.revision != body.config_revision {
        return Err(ServiceError::StaleRevision(cfg.revision));
    }
    if state.pipeline.is_locked().await
        || crate::services::settlement::round_locked(&cfg.round.round_id)?
    {
        return Err(ServiceError::Locked("轮次已锁定，不能修订动作".into()));
    }
    let revision = overrides.record_revision(key, record.record_revision);
    if revision != body.record_revision {
        return Err(ServiceError::Conflict(
            json!({"error":"stale_record_revision","message":"这条消息已被修改，请保留草稿后重新核对","record_revision":revision}),
        ));
    }
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|r| (r.timestamp_ms, r.seq));
    if session::message_groups(&cfg, &ordered, &overrides)
        .iter()
        .any(|g| g.len() > 1 && g.iter().any(|&i| ordered[i].record_id == key))
    {
        return Err(ServiceError::Conflict(
            json!({"error":"auto_group_requires_transaction","message":"自动合并组须整组修订，当前未修改任何记录"}),
        ));
    }
    if body.selection.note().chars().count() > 2000 {
        return Err(ServiceError::BadRequest("管理员解释最多 2000 字".into()));
    }
    let understanding = if matches!(body.selection, ActionSelection::Automatic { .. }) {
        record
            .processing
            .as_ref()
            .and_then(|p| p.latest_for(&[record]))
            .map(|a| a.understanding.clone())
            .ok_or_else(|| {
                ServiceError::Unprocessable(
                    "原自动基线缺失或不匹配，无法恢复；可直接人工补正".into(),
                )
            })?
    } else {
        action_override::compile(&cfg, record, &body.selection).map_err(ServiceError::BadRequest)?
    };
    let baseline = compute(&state.messages, &cfg, &records, &overrides).await?;
    for event in &understanding.events {
        if let DomainEvent::ClaimCancelled(cancel) = &event.payload {
            if let Some(id) = &cancel.target_claim_id {
                let valid = baseline.events.iter().any(|e| {
                    e.user_id.0 == record.user_id
                        && (e.effective_at.timestamp_millis(), e.sequence)
                            < (record.timestamp_ms, record.seq)
                        && matches!(&e.payload, DomainEvent::ClaimCreated(c) if &c.claim_id==id)
                });
                let unchanged = overrides.selection_for_record(key).is_some_and(|old| old.events.iter().any(|e| matches!(&e.payload,DomainEvent::ClaimCancelled(c) if c.target_claim_id.as_ref()==Some(id))));
                if !valid && !unchanged {
                    return Err(ServiceError::BadRequest(
                        "只能撤销本成员在此消息之前的申购；原目标不可用".into(),
                    ));
                }
            }
        }
    }
    let saved = SavedSelection {
        action_selection_version: 1,
        transaction_id: body.transaction_id.clone(),
        request_hash: hash,
        selection: body.selection.clone(),
        events: understanding.events,
    };
    overrides.insert_selection(key.to_string(), saved.clone(), revision.saturating_add(1));
    let computed = compute(&state.messages, &cfg, &records, &overrides).await?;
    let now = Utc::now();
    let event_id = EventId(uuid::Uuid::new_v4().to_string());
    let event = EventEnvelope {
        event_id: event_id.clone(),
        round_id: RoundId(cfg.round.round_id.clone()),
        group_id: record.group_id.clone(),
        user_id: UserId("admin".into()),
        raw_message_id: Some(record.message_id.clone()),
        event_type: "parse_override".into(),
        effective_at: now,
        sequence: crate::messages::next_seq(),
        status: EventStatus::Active,
        payload: DomainEvent::ParseOverride(ParseOverrideEvent {
            event_id,
            round_id: RoundId(cfg.round.round_id.clone()),
            target_raw_message_id: record.message_id.clone(),
            target_record_id: Some(key.into()),
            target_record_revision: Some(revision.saturating_add(1)),
            corrected_parsed_message: serde_json::to_value(saved).map_err(anyhow::Error::from)?,
            admin_user_id: UserId("admin".into()),
            reason: body.selection.note().into(),
            occurred_at: now,
        }),
    };
    state
        .messages
        .append_durable_event(
            &cfg.round.round_id,
            &serde_json::to_value(event).map_err(anyhow::Error::from)?,
        )
        .await?;
    state.pipeline.apply_replay(&computed, &records).await;
    replay::remember_replay(&computed);
    Ok(
        json!({"ok":true,"transaction_id":body.transaction_id,"message":projected(record,&computed),"result":replay::replay_result_json(&computed)}),
    )
}
