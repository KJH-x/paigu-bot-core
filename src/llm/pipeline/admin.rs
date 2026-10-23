use super::understanding::ProcessingContext;
use super::Pipeline;
use crate::bus::{IncomingEvent, PipelineOutcome};
use crate::messages::processing::{ProcessingOutcome, Understanding, UnderstandingSource};
use crate::settings::AppConfig;

impl Pipeline {
    /// 管理员斜杠命令**落地执行**（D-1，仅在 `gateway.admin_commands_enabled` 开启时调用）。
    pub(super) async fn run_admin_command(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
        context: &mut ProcessingContext,
        cmd: &str,
    ) -> PipelineOutcome {
        let authorization = Understanding::terminal(
            UnderstandingSource::Control,
            "Applied",
            "控制命令身份检查",
            None,
        );
        let evaluated = crate::replay::session::evaluate_understanding(
            cfg,
            &context.record,
            &authorization,
            context.manual_phase,
        );
        if evaluated.outcome.status != "Applied" {
            context.understanding = Understanding::terminal(
                UnderstandingSource::Control,
                &evaluated.outcome.status,
                evaluated.outcome.detail.clone(),
                None,
            );
            return self.finish(cfg, context, ev, evaluated.outcome).await;
        }
        if let Err(error) = self
            .messages
            .read_ingress_records(&cfg.round.round_id)
            .await
        {
            return self.persistence_failure(ev, false, error).await;
        }
        if let Err(error) = self.messages.domain_events(&cfg.round.round_id).await {
            return self.persistence_failure(ev, false, error).await;
        }
        let persistent_lock = match crate::services::settlement::round_locked(&cfg.round.round_id) {
            Ok(locked) => locked,
            Err(_) => {
                return self
                    .persistence_failure(
                        ev,
                        false,
                        anyhow::anyhow!("持久结算锁不可校验，未执行控制命令"),
                    )
                    .await
            }
        };
        let head = cmd.split_whitespace().next().unwrap_or("").to_string();
        let (status, detail, reply): (&str, String, String) = match head.as_str() {
            "开团" | "解锁" | "open" | "unlock" => {
                if persistent_lock {
                    (
                        "Rejected",
                        "订单已持久化锁定，请在结算页解锁".to_string(),
                        "订单已锁定；请在结算页确认并解锁。".to_string(),
                    )
                } else {
                    if let Err(error) = self.set_locked_locked(cfg, false).await {
                        return self.persistence_failure(ev, false, error).await;
                    }
                    let v = self.state.lock().await.version;
                    (
                        "Applied",
                        "已开团（解锁）".to_string(),
                        format!("已开团（解锁），当前版本 #{v}"),
                    )
                }
            }
            "锁位" | "锁定" | "结团" | "结束" | "lock" | "close" => {
                if let Err(error) = self.set_locked_locked(cfg, true).await {
                    return self.persistence_failure(ev, false, error).await;
                }
                let v = self.state.lock().await.version;
                (
                    "Applied",
                    "已锁定".to_string(),
                    format!("已锁定，当前版本 #{v}"),
                )
            }
            "状态" | "status" => {
                let st = self.state.lock().await;
                (
                    "Applied",
                    "状态".to_string(),
                    format!("版本 #{}，锁定={}", st.version, st.locked),
                )
            }
            "导出" | "export" => self.export_snapshot().await,
            other => (
                "Rejected",
                format!("未支持的管理员命令：{other}"),
                format!("未支持的管理员命令：{other}"),
            ),
        };

        context.record.routed = "admin".into();
        context.understanding = Understanding::terminal(
            UnderstandingSource::Control,
            status,
            detail.clone(),
            Some(reply.clone()),
        );
        self.finish(
            cfg,
            context,
            ev,
            ProcessingOutcome::new(status, detail, Some(reply)),
        )
        .await
    }
}
