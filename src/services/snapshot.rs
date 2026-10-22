use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::messages::MessageRecord;
use crate::replay::session::{self, ReplayOverrides, ReplayResult};
use crate::settings::AppConfig;
use crate::snapshot_bundle::SnapshotBundle;

use super::replay::replay_result_json;
use super::ServiceError;

fn default_snapshot_dir(cfg: &AppConfig) -> PathBuf {
    let base = std::env::var("PAIGU_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/snapshots"));
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    base.join(format!("{}-{}", cfg.round.round_id, stamp))
}

fn build_bundle(
    cfg: &AppConfig,
    records: &[MessageRecord],
    result: &ReplayResult,
    journal: Value,
    draft: Value,
    created_at: String,
) -> anyhow::Result<SnapshotBundle> {
    SnapshotBundle::seal(
        cfg.round.round_id.clone(),
        cfg.revision,
        created_at,
        serde_json::to_value(cfg)?,
        serde_json::to_value(records)?,
        journal,
        serde_json::to_value(&result.board)?,
        json!({"calculated":result.settlement,"draft":draft}),
    )
}

#[derive(Deserialize, Default)]
pub struct ExportBody {
    #[serde(default)]
    pub overrides: ReplayOverrides,
    #[serde(default)]
    pub out_dir: Option<String>,
    /// `"file"` = 单一 JSON 快照（C-2）；缺省 `"dir"` = 目录式。
    #[serde(default)]
    pub format: Option<String>,
}

pub async fn snapshot_export(state: &ApiState, body: ExportBody) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    {
        let _fence = state.messages.ingress_commit_guard().await;
        super::configuration::ensure_no_pending_ingress(&state.messages, &cfg.round.round_id)?;
    }
    let records = state
        .messages
        .read_only(&cfg.round.round_id)
        .await
        .map_err(ServiceError::from)?;
    let result = session::replay(&state.messages, &cfg, body.overrides)
        .await
        .map_err(ServiceError::from)?;

    let bundle = build_bundle(
        &cfg,
        &records,
        &result,
        json!(
            state
                .messages
                .read_events_strict(&cfg.round.round_id)
                .await?
        ),
        super::settlement::checkpoint(&cfg.round.round_id)?,
        chrono::Utc::now().to_rfc3339(),
    )
    .map_err(ServiceError::from)?;
    let base = body
        .out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| default_snapshot_dir(&cfg));
    let single_file = body.format.as_deref() == Some("file");
    let path = if single_file {
        let target = if base.extension().is_some() {
            base
        } else {
            base.join(format!("{}.snapshot.json", cfg.round.round_id))
        };
        bundle.export_file(&target).map_err(ServiceError::from)?
    } else {
        bundle.export_dir(&base).map_err(ServiceError::from)?
    };
    let manifest = bundle.manifest_typed().map_err(ServiceError::from)?;

    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "manifest": manifest,
        "board": result.board,
        "diff": result.diff,
    }))
}

#[derive(Deserialize, Default)]
pub struct ImportBody {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub bundle: Option<SnapshotBundle>,
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub round_id: Option<String>,
    #[serde(default)]
    pub preflight_token: Option<String>,
}

pub async fn snapshot_import(state: &ApiState, body: ImportBody) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(ServiceError::StaleRevision(cfg.revision));
        }
    }

    let bundle = match (body.path, body.bundle) {
        (Some(path), _) => {
            let p = PathBuf::from(&path);
            if p.is_file() {
                SnapshotBundle::import_file(&p)
                    .map_err(|e| ServiceError::BadRequest(e.to_string()))?
            } else {
                SnapshotBundle::import_dir(&p)
                    .map_err(|e| ServiceError::BadRequest(e.to_string()))?
            }
        }
        (None, Some(bundle)) => {
            bundle
                .verify()
                .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
            bundle
        }
        (None, None) => return Err(ServiceError::BadRequest("需要 path 或 bundle".to_string())),
    };
    let manifest = bundle
        .verify()
        .map_err(|e| ServiceError::BadRequest(e.to_string()))?;

    let records: Vec<MessageRecord> = serde_json::from_value(bundle.messages.clone())
        .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
    let mut ids = std::collections::HashSet::new();
    if records
        .iter()
        .any(|r| r.record_id.is_empty() || !ids.insert(&r.record_id))
    {
        return Err(ServiceError::BadRequest(
            "备份消息缺少稳定身份或身份重复，未猜测迁移".into(),
        ));
    }
    let imported_cfg: AppConfig = serde_json::from_value(bundle.config.clone())
        .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
    let applicable =
        manifest.round_id == cfg.round.round_id && imported_cfg.round.round_id == manifest.round_id;
    let current = state.messages.read_only(&cfg.round.round_id).await?;
    let current_events = state
        .messages
        .read_events_strict(&cfg.round.round_id)
        .await?;
    let token = crate::messages::transactions::hash(&(
        cfg.revision,
        &cfg.round.round_id,
        &manifest.hash,
        &current,
        &current_events,
    ))?;
    let mut events: Vec<Value> = serde_json::from_value(bundle.events.clone())
        .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
    let mut domain = vec![];
    for event in &events {
        if event.get("event_type").is_some() {
            let envelope: crate::domain::event::EventEnvelope =
                serde_json::from_value(event.clone())
                    .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
            if envelope.round_id.0 != cfg.round.round_id {
                return Err(ServiceError::BadRequest("备份事件包含其他轮次".into()));
            }
            domain.push(envelope);
        }
    }
    let overrides = crate::parser::parsed_event::collect_parse_overrides(&domain);
    let phase = domain
        .iter()
        .filter(|e| {
            matches!(
                e.payload,
                crate::domain::event::DomainEvent::RoundPhaseSet(_)
            )
        })
        .max_by_key(|e| (e.effective_at, e.sequence))
        .and_then(|e| match &e.payload {
            crate::domain::event::DomainEvent::RoundPhaseSet(p) => p.phase,
            _ => None,
        });
    let computed = session::replay_messages_with_parse(
        &cfg,
        &records,
        ReplayOverrides::default(),
        &overrides,
        phase,
    )
    .await?;
    if !body.apply {
        return Ok(
            json!({"ok":true,"verified":true,"applied":false,"applicable":applicable,"manifest":manifest,
            "target_round_id":cfg.round.round_id,"config_revision":cfg.revision,"preflight_token":token,
            "current_messages":current.len(),"import_messages":records.len(),
            "effect":"替换当前轮消息与审计，保留当前配置和订单草稿；按当前配置确定性重算，旧完整日志留私有备份。不会重解包或发群消息。"}),
        );
    }
    if !applicable || body.round_id.as_deref() != Some(&cfg.round.round_id) {
        return Err(ServiceError::Conflict(
            json!({"error":"round_changed","message":"仅能向预检时的同一激活轮次应用，不自动切轮"}),
        ));
    }
    if body.revision != Some(cfg.revision)
        || body.preflight_token.as_deref() != Some(token.as_str())
    {
        return Err(ServiceError::Conflict(
            json!({"error":"stale_import_preflight","message":"预检后配置、消息或事件已变化，未覆盖新数据；请重新预检"}),
        ));
    }
    if state.pipeline.is_locked().await || super::settlement::round_locked(&cfg.round.round_id)? {
        return Err(ServiceError::Locked("轮次已锁定，不能应用备份".into()));
    }
    let _ingress_fence = state.messages.ingress_commit_guard().await;
    super::configuration::ensure_no_pending_ingress(&state.messages, &cfg.round.round_id)?;
    let audit = crate::domain::event::EventEnvelope {
        event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
        round_id: crate::domain::ids::RoundId(cfg.round.round_id.clone()),
        group_id: cfg.round.group_id.clone(),
        user_id: crate::domain::ids::UserId("admin".into()),
        raw_message_id: None,
        event_type: "snapshot_applied".into(),
        effective_at: chrono::Utc::now(),
        sequence: crate::messages::next_seq(),
        status: crate::domain::event::EventStatus::Active,
        payload: crate::domain::event::DomainEvent::SnapshotApplied {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            manifest_hash: manifest.hash.clone(),
            message_count: records.len(),
        },
    };
    events.push(serde_json::to_value(audit).map_err(anyhow::Error::from)?);
    let backup = state
        .messages
        .replace_round_durable(&cfg.round.round_id, &records, &events)
        .await?;
    state.pipeline.apply_replay(&computed, &records).await;
    super::replay::remember_replay(&computed);
    Ok(
        json!({"ok":true,"verified":true,"applied":true,"manifest":manifest,"backup_path":backup,"result":replay_result_json(&computed)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_config;

    #[test]
    fn default_snapshot_dir_contains_round_id() {
        let cfg = default_config();
        let dir = default_snapshot_dir(&cfg);
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with(&cfg.round.round_id));
    }
}
