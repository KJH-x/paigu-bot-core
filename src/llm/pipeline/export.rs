use super::Pipeline;

impl Pipeline {
    pub(super) async fn export_snapshot(&self) -> (&'static str, String, String) {
        let cfg = self.cfg.read().await;
        match crate::services::snapshot::export_completed_locked(&self.messages, &cfg).await {
            Ok(output) => {
                let Some(path) = output["path"].as_str() else {
                    let text = "导出结果缺少路径，请检查本机数据维护".to_string();
                    return ("Error", text.clone(), text);
                };
                ("Applied", format!("私有备份已保存：{path}（本指令前的已完成动作切面）"),
                    format!("已导出快照：{path}\n按本指令前的已完成动作计算；原始审计仍可能含未执行帧，不恢复待处理队列，不是接收器灾备；文件不可公开分享。"))
            }
            Err(error) => {
                let (_, value) = error.into_api();
                let reason = value
                    .0
                    .get("message")
                    .or_else(|| value.0.get("error"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("请检查本机备份状态");
                let text = format!("导出失败：{reason}");
                ("Error", text.clone(), text)
            }
        }
    }
}
