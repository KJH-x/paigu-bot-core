use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
    #[serde(default)]
    pub supplements: Vec<RecordVersion>,
    #[serde(default)]
    pub release_supplements: Vec<RecordVersion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordVersion {
    pub record_id: String,
    pub record_revision: u64,
}

mod suggest;
mod transaction;
pub use suggest::{suggest, SuggestInput};
pub use transaction::{undo, undo_status, UndoInput};

async fn journal(store: &MessageLog, round: &str) -> Result<Vec<EventEnvelope>, ServiceError> {
    Ok(store.domain_events(round).await?)
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
        "model_draft_available":cfg.llm.enabled,
        "original_automatic":original,"current_automatic":auto,
        "original_action_summary":original_summary,
        "automatic_history_count":record.processing.as_ref().map_or(0, |p| p.results.len()),
        "automatic_missing":original.is_none(),"automatic_matches":auto.is_some(),
        "effective_source":source,"effective_parsed":if saved.is_some_and(|s| !matches!(s.selection, ActionSelection::Automatic { .. })) { None } else { manual.or_else(|| auto.and_then(|a| a.understanding.parsed.as_ref())) },
        "manual_note":note,"selection":saved.map(|s| &s.selection),"editable_selection":draft,
        "catalog":cfg.round.items,"cancel_targets":targets,
        "automatic_group":{"representative_record_id":rep.record_id,"members":members.iter().map(|r| projected(r, &computed)).collect::<Vec<_>>()},
        "editable":members.len()==1 && overrides.absorbed_by(key).is_none(),"can_restore_automatic":members.len()==1 && auto.is_some() && !records.iter().any(|r| overrides.absorbed_by(&r.record_id)==Some(key)),
        "group_main":projected(members[0],&computed),
        "absorbed_by":overrides.absorbed_by(key),
        "absorbed_main":overrides.absorbed_by(key).and_then(|id| records.iter().find(|r| r.record_id==id)).map(|r| projected(r,&computed)),
        "linked_supplements":records.iter().filter(|r| overrides.absorbed_by(&r.record_id)==Some(key)).map(|r| projected(r,&computed)).collect::<Vec<_>>(),
        "supplement_candidates":ordered.iter().filter(|r| r.group_id==record.group_id && (r.timestamp_ms,r.seq)>(record.timestamp_ms,record.seq) && overrides.absorbed_by(&r.record_id).is_none_or(|id| id==key)).map(|r| projected(r,&computed)).collect::<Vec<_>>(),
        "supplement_units":groups.iter().filter(|g| g.iter().all(|&i| ordered[i].group_id==record.group_id && (ordered[i].timestamp_ms,ordered[i].seq)>(record.timestamp_ms,record.seq) && overrides.absorbed_by(&ordered[i].record_id).is_none_or(|id| id==key))).map(|g| g.iter().map(|&i| projected(ordered[i],&computed)).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "undo":transaction::undo_view(&journal(&state.messages,round_id).await?),
        "current_outcome":{"status":message["status"],"detail":message["detail"],"summary":current_summary}
    }))
}

pub async fn save(state: &ApiState, key: &str, body: SaveActions) -> Result<Value, ServiceError> {
    transaction::save(state, key, body).await
}
