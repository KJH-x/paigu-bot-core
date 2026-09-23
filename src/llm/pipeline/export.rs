use serde_json::Value;

use super::Pipeline;

impl Pipeline {
    /// `/导出`：导出**单一 JSON 快照**（C-2）到 `data/snapshots/<round>.snapshot.json`。
    pub(super) async fn export_snapshot(&self) -> (&'static str, String, String) {
        let cfg = self.cfg.get().await;
        let round_id = cfg.round.round_id.clone();
        let records = self.messages.read_all(&round_id).await.unwrap_or_default();
        let events = self
            .messages
            .read_raw_events(&round_id)
            .await
            .unwrap_or_default();
        let board = {
            let st = self.state.lock().await;
            st.snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok())
                .unwrap_or(Value::Null)
        };

        let bundle = match crate::snapshot_bundle::SnapshotBundle::seal(
            round_id.clone(),
            cfg.revision,
            chrono::Utc::now().to_rfc3339(),
            serde_json::to_value(&cfg).unwrap_or(Value::Null),
            serde_json::to_value(&records).unwrap_or(Value::Null),
            serde_json::to_value(&events).unwrap_or(Value::Null),
            board,
            Value::Null,
        ) {
            Ok(bundle) => bundle,
            Err(e) => {
                let msg = format!("导出失败: {e}");
                return ("Error", msg.clone(), msg);
            }
        };

        let path =
            std::path::PathBuf::from("data/snapshots").join(format!("{round_id}.snapshot.json"));
        match bundle.export_file(&path) {
            Ok(path) => (
                "Applied",
                "已导出快照".to_string(),
                format!("已导出快照：{}", path.display()),
            ),
            Err(e) => {
                let msg = format!("导出失败: {e}");
                ("Error", msg.clone(), msg)
            }
        }
    }
}
