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

use crate::bus::EventSink;
use crate::messages::MessageLog;
use crate::settings::ConfigStore;

use super::onebot::{self, RouteDecision, RouteKind, RouteMessageEvent, RoutePolicy};

type ClientTx = mpsc::UnboundedSender<WsMessage>;

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
}

impl Gateway {
    pub fn new(cfg: Arc<ConfigStore>, sink: Arc<dyn EventSink>) -> Arc<Self> {
        let messages = sink.messages();
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
        })
    }

    pub async fn run(self: Arc<Self>) -> anyhow::Result<()> {
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
        json!({
            "listening": self.listening.load(Ordering::SeqCst),
            "bound_addr": self.bound_addr.lock().unwrap().clone(),
            "clients": self.clients.lock().unwrap().len(),
            "last_error": self.last_error.lock().unwrap().clone(),
        })
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
        let ws = tokio_tungstenite::accept_async(stream).await?;
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
                            self.on_text(text.as_str()).await;
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

    async fn on_text(self: &Arc<Self>, text: &str) {
        let val: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => {
                debug!(frame = %onebot::sanitize(text), "gateway drop: unparsable frame");
                return;
            }
        };

        if self.dispatch_response(&val) {
            return;
        }

        let ev: RouteMessageEvent = match serde_json::from_value(val.clone()) {
            Ok(e) => e,
            Err(_) => {
                debug!("gateway drop: not a onebot event");
                return;
            }
        };

        let cfg = self.cfg.get().await;
        let policy = RoutePolicy {
            whitelist_groups: cfg.gateway.whitelist_groups.clone(),
            whitelist_members: cfg.gateway.whitelist_members.clone(),
        };

        match onebot::decide_route(&ev, &policy) {
            RouteKind::Drop => {
                let reason = match onebot::decide_route_with_reason(&ev, &policy) {
                    RouteDecision::Drop(reason) => reason,
                    RouteDecision::Message => "not_routable".to_string(),
                };
                debug!(
                    post_type = %ev.post_type,
                    group_id = %onebot::group_id_string(&ev).unwrap_or_default(),
                    reason = %reason,
                    "gateway drop: not routable"
                );
                let rec =
                    onebot::to_message_record(&ev, &format!("drop:{reason}"), "Dropped", &reason);
                if let Err(e) = self.messages.append(&cfg.round.round_id, &rec).await {
                    warn!(error = %e, "failed to persist dropped message");
                }
                // C-3：被丢弃的事件同样保留原始 JSON
                if let Ok(raw) = serde_json::to_value(&ev) {
                    if let Err(e) = self
                        .messages
                        .append_raw_event(&cfg.round.round_id, &raw)
                        .await
                    {
                        warn!(error = %e, "failed to persist dropped raw event");
                    }
                }
            }
            RouteKind::Message => {
                let incoming = onebot::to_incoming_event(&ev);
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    sink.handle(incoming).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
