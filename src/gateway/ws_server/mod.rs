use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, warn};

#[cfg(test)]
mod export_tests;

use crate::bus::EventSink;
use crate::messages::MessageLog;
use crate::settings::ConfigStore;

use super::onebot::{self, RouteDecision, RouteKind, RouteMessageEvent, RoutePolicy};
mod intake;

type ClientTx = mpsc::UnboundedSender<WsMessage>;

impl Drop for Gateway {
    fn drop(&mut self) {
        if let Ok(worker) = self.worker.get_mut() {
            if let Some(worker) = worker.take() {
                worker.abort();
            }
        }
    }
}

pub struct Gateway {
    cfg: Arc<ConfigStore>,
    sink: Arc<dyn EventSink>,
    messages: Arc<MessageLog>,
    clients: Mutex<HashMap<u64, ClientTx>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    next_client_id: AtomicU64,
    listening: AtomicBool,
    bound_addr: Mutex<String>,
    last_error: Mutex<Option<String>>,
    intake: Arc<intake::Intake>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Gateway {
    fn start_worker(self: &Arc<Self>) {
        let mut worker = self.worker.lock().unwrap();
        if worker.is_none() {
            if let Some(receiver) = self.intake.receiver() {
                *worker = Some(tokio::spawn(intake::run(
                    self.intake.clone(),
                    self.cfg.clone(),
                    self.sink.clone(),
                    self.messages.clone(),
                    Arc::downgrade(self),
                    receiver,
                )));
            }
        }
    }
    pub fn new(cfg: Arc<ConfigStore>, sink: Arc<dyn EventSink>) -> Arc<Self> {
        let messages = sink.messages();
        let intake = intake::Intake::new(&messages);
        Arc::new(Self {
            cfg,
            sink,
            messages,
            clients: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            next_client_id: AtomicU64::new(1),
            listening: AtomicBool::new(false),
            bound_addr: Mutex::new(String::new()),
            last_error: Mutex::new(None),
            intake,
            worker: Mutex::new(None),
        })
    }

    pub async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        self.start_worker();
        loop {
            let bind = self.cfg.get().await.gateway.bind.clone();
            let addr: SocketAddr = match bind.parse() {
                Ok(a) => a,
                Err(e) => {
                    self.set_error(format!("invalid bind '{bind}': {e}"));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    *self.bound_addr.lock().unwrap() = bind.clone();
                    self.listening.store(true, Ordering::SeqCst);
                    *self.last_error.lock().unwrap() = None;
                    debug!(%bind, "gateway listening");
                    self.accept_loop(listener).await;
                    self.listening.store(false, Ordering::SeqCst);
                }
                Err(e) => {
                    self.set_error(format!("bind {bind} failed: {e}"));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    pub async fn send_action(&self, action: &str, params: Value) -> anyhow::Result<Value> {
        let cfg = self.cfg.get().await;
        if action.starts_with("send_") {
            if !cfg.gateway.reply_enabled {
                warn!(action = %action, "forbidden send action: reply_enabled=false");
                anyhow::bail!("forbidden send action (reply_enabled=false): {action}");
            }
            if !cfg.gateway.allowed_actions.iter().any(|a| a == action) {
                warn!(action = %action, "forbidden send action: not in allowed_actions");
                anyhow::bail!("forbidden send action (not allowed): {action}");
            }
        } else if !cfg.gateway.allowed_actions.iter().any(|a| a == action) {
            anyhow::bail!("action not allowed: {action}");
        }

        self.dispatch(action, params).await
    }

    /// 直接下发 OneBot 动作（**不做白名单/开关校验**）：仅供已由配置显式授权的内部调用
    /// （如 `react_emoji_id` 触发的 `set_msg_emoji_like`）使用。
    async fn dispatch(&self, action: &str, params: Value) -> anyhow::Result<Value> {
        let sender = {
            let clients = self.clients.lock().unwrap();
            clients.values().next().cloned()
        }
        .ok_or_else(|| anyhow::anyhow!("no connected client"))?;

        let echo = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(echo.clone(), tx);

        let frame = json!({ "action": action, "params": params, "echo": echo });
        if sender
            .send(WsMessage::Text(frame.to_string().into()))
            .is_err()
        {
            self.pending.lock().unwrap().remove(&echo);
            anyhow::bail!("client channel closed");
        }

        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => {
                self.pending.lock().unwrap().remove(&echo);
                anyhow::bail!("action response dropped: {action}")
            }
            Err(_) => {
                self.pending.lock().unwrap().remove(&echo);
                anyhow::bail!("action timeout: {action}")
            }
        }
    }

    pub async fn status(&self) -> Value {
        let mut intake = self.intake.status().await;
        intake["worker_running"] = json!(self
            .worker
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|worker| !worker.is_finished()));
        if intake["pending"].as_u64().unwrap_or(0) > 0 && intake["worker_running"] == false {
            intake["paused"] = json!(true);
            if intake["last_error"].is_null() {
                intake["last_error"] =
                    json!("消息处理器未运行；本地待处理记录已保留，请检查后重试");
            }
        }
        json!({
            "listening": self.listening.load(Ordering::SeqCst),
            "bound_addr": self.bound_addr.lock().unwrap().clone(),
            "clients": self.clients.lock().unwrap().len(),
            "last_error": self.last_error.lock().unwrap().clone(),
            "intake": intake,
        })
    }

    pub async fn retry_intake(self: &Arc<Self>) -> Value {
        self.intake.retry().await;
        {
            let mut worker = self.worker.lock().unwrap();
            if worker.as_ref().is_some_and(|worker| worker.is_finished()) {
                worker.take();
            }
        }
        {
            let mut error = self.last_error.lock().unwrap();
            if error.as_deref() == Some("入站记录保存失败，已停止此连接；未跳过本地待处理数据")
            {
                *error = None;
            }
        }
        self.start_worker();
        self.status().await
    }

    fn set_error(&self, msg: String) {
        warn!(error = %msg, "gateway error");
        *self.last_error.lock().unwrap() = Some(msg);
    }

    fn dispatch_response(&self, val: &Value) -> bool {
        let echo = match val.get("echo").and_then(Value::as_str) {
            Some(e) => e,
            None => return false,
        };
        match self.pending.lock().unwrap().remove(echo) {
            Some(tx) => {
                if let Err(e) = tx.send(val.clone()) {
                    warn!(error = ?e, "action response receiver dropped");
                }
                true
            }
            None => false,
        }
    }

    async fn accept_loop(self: &Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    let id = self.next_client_id.fetch_add(1, Ordering::SeqCst);
                    let this = self.clone();
                    let cleanup = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this.handle_connection(stream, id).await {
                            debug!(error = %e, "gateway connection ended");
                        }
                        cleanup.clients.lock().unwrap().remove(&id);
                    });
                }
                Err(e) => {
                    self.set_error(format!("accept failed: {e}"));
                    break;
                }
            }
        }
    }

    async fn handle_connection(
        self: Arc<Self>,
        stream: TcpStream,
        client_id: u64,
    ) -> anyhow::Result<()> {
        let limits = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(1024 * 1024))
            .max_frame_size(Some(1024 * 1024));
        let ws = tokio_tungstenite::accept_async_with_config(stream, Some(limits)).await?;
        let (mut write, mut read) = ws.split();

        let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();
        self.clients.lock().unwrap().insert(client_id, tx);

        let writer = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let heartbeat = self.cfg.get().await.gateway.heartbeat_secs.max(1);
        let mut ticker = tokio::time::interval(Duration::from_secs(heartbeat));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let closed = {
                        let clients = self.clients.lock().unwrap();
                        match clients.get(&client_id) {
                            Some(tx) => tx.send(WsMessage::Ping(Vec::new().into())).is_err(),
                            None => true,
                        }
                    };
                    if closed {
                        break;
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if self.on_text(text.as_str()).await.is_err() {
                                self.set_error("入站记录保存失败，已停止此连接；未跳过本地待处理数据".into());
                                break;
                            }
                        }
                        Some(Ok(WsMessage::Ping(data))) => {
                            let clients = self.clients.lock().unwrap();
                            if let Some(tx) = clients.get(&client_id) {
                                if let Err(e) = tx.send(WsMessage::Pong(data)) {
                                    warn!(error = ?e, "failed to send pong");
                                }
                            }
                        }
                        Some(Ok(WsMessage::Pong(_))) => {}
                        Some(Ok(WsMessage::Close(_))) => break,
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            debug!(error = %e, "gateway ws read error");
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        self.clients.lock().unwrap().remove(&client_id);
        writer.abort();
        Ok(())
    }

    async fn on_text(self: &Arc<Self>, text: &str) -> anyhow::Result<()> {
        anyhow::ensure!(text.len() <= 1024 * 1024, "入站帧过大，未保存");
        let val: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => {
                debug!(frame = %onebot::sanitize(text), "gateway drop: unparsable frame");
                return Ok(());
            }
        };

        if self.dispatch_response(&val) {
            return Ok(());
        }

        let ev: RouteMessageEvent = match serde_json::from_value(val.clone()) {
            Ok(e) => e,
            Err(_) => {
                debug!("gateway drop: not a onebot event");
                return Ok(());
            }
        };

        let _accept = self.messages.ingress_accept_guard().await;
        let cfg = self.cfg.get().await;
        let policy = RoutePolicy {
            whitelist_groups: cfg.gateway.whitelist_groups.clone(),
            whitelist_members: cfg.gateway.whitelist_members.clone(),
        };

        let drop_reason = match onebot::decide_route(&ev, &policy) {
            RouteKind::Message => None,
            RouteKind::Drop => Some(match onebot::decide_route_with_reason(&ev, &policy) {
                RouteDecision::Drop(reason) => reason,
                RouteDecision::Message => "not_routable".into(),
            }),
        };
        if let Err(error) = self.intake.accept_raw(&cfg, ev, drop_reason, val).await {
            self.intake
                .fail("入站保存失败；已保存的待处理记录未清空")
                .await;
            return Err(error);
        }
        self.start_worker();
        Ok(())
    }
}

#[cfg(test)]
mod intake_tests;
#[cfg(test)]
mod tests;
