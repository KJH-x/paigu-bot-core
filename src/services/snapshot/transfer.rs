use serde::Deserialize;
use serde_json::json;

use super::{complete_bundle, snapshot_import, ImportBody};
use crate::{
    api::ApiState,
    services::ServiceError,
    snapshot_bundle::{SnapshotBundle, SnapshotFile},
};

pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadBody {
    pub round_id: String,
    pub config_revision: u64,
    pub private_ack: bool,
}

pub struct Download {
    pub bytes: Vec<u8>,
    pub filename: String,
}

pub async fn download(state: &ApiState, body: DownloadBody) -> Result<Download, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    if !body.private_ack {
        return Err(ServiceError::BadRequest(
            "请先确认备份含私有数据，不用于公开分享".into(),
        ));
    }
    if body.round_id != cfg.round.round_id || body.config_revision != cfg.revision {
        return Err(ServiceError::Conflict(
            json!({"error":"snapshot_context_changed","message":"轮次或配置已变化，请重新读取后下载"}),
        ));
    }
    let (bundle, _) = complete_bundle(&state.messages, &cfg, Default::default()).await?;
    let filename = format!(
        "paigu-private-{}.snapshot.json",
        bundle.manifest_typed()?.hash
    );
    let bytes = serde_json::to_vec_pretty(&bundle.as_file()?).map_err(anyhow::Error::from)?;
    check_size(bytes.len())?;
    Ok(Download { bytes, filename })
}

pub(crate) fn check_size(size: usize) -> Result<(), ServiceError> {
    if size > MAX_FILE_BYTES {
        return Err(ServiceError::PayloadTooLarge(
            "浏览器完整备份上限为64MiB；请改用处理主机路径，不会截断备份".into(),
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportFileQuery {
    pub round_id: String,
    pub revision: u64,
    #[serde(default)]
    pub apply: bool,
    pub preflight_token: Option<String>,
}

pub async fn import_file(
    state: &ApiState,
    query: ImportFileQuery,
    file: SnapshotFile,
) -> Result<serde_json::Value, ServiceError> {
    let bundle = SnapshotBundle::from_file(file)
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    snapshot_import(
        state,
        ImportBody {
            bundle: Some(bundle),
            round_id: Some(query.round_id),
            revision: Some(query.revision),
            apply: query.apply,
            preflight_token: query.preflight_token,
            path: None,
        },
    )
    .await
}
