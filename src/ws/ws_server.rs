use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::{watch, mpsc, Mutex};
use tokio_tungstenite::tungstenite::{
    Message as WsMessage,
    handshake::server::{Request, Response, ErrorResponse},
};
use futures_util::{SinkExt, StreamExt};
use tracing::{info, warn, error};
use serde::{Deserialize, Serialize};
use chrono::Utc;

use crate::config::WsServerConfig;
use crate::inbound::qq_message::IncomingQqMessage;
use crate::services::message_service::MessageService;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsServerStatus {
    pub connected: bool,
    pub state: String,
    pub bind_addr: String,
    pub client_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsIncomingPayload {
    #[serde(rename = "qq_message")]
    QqMessage {
        group_id: String,
        user_id: String,
        nickname: String,
        message_id: String,
        text: String,
        timestamp_ms: i64,
        is_admin: bool,
    },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsOutgoingReply {
    pub message_id: Option<String>,
    pub reply_type: String,
    pub text: Option<String>,
    pub confirm_token: Option<String>,
}

pub struct WsServer {
    config: Arc<tokio::sync::RwLock<WsServerConfig>>,
    message_service: Arc<MessageService>,
    status: Arc<tokio::sync::RwLock<WsServerStatus>>,
    reload_tx: watch::Sender<u64>,
    shutdown: Arc<Mutex<bool>>,
}

impl WsServer {
    pub fn new(config: WsServerConfig, message_service: Arc<MessageService>) -> Self {
        let status = WsServerStatus {
            connected: false,
            state: "idle".to_string(),
            bind_addr: format!("ws://{}:{}", config.host, config.port),
            client_count: 0,
            last_error: None,
        };
        Self {
            config: Arc::new(tokio::sync::RwLock::new(config)),
            message_service,
            status: Arc::new(tokio::sync::RwLock::new(status)),
            reload_tx: watch::channel(0).0,
            shutdown: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn run_forever(&self) {
        loop {
            if *self.shutdown.lock().await {
                info!("WS server shutdown requested, exiting");
                break;
            }

            let config = self.config.read().await.clone();
            if !config.enabled {
                info!("WS server disabled, sleeping...");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }

            self.status.write().await.state = "listening".to_string();
            info!("WS server starting on {}:{}", config.host, config.port);

            let addr_str = format!("{}:{}", config.host, config.port);
            let addr: SocketAddr = match addr_str.parse() {
                Ok(a) => a,
                Err(e) => {
                    error!("Invalid bind address '{}': {}", addr_str, e);
                    self.status.write().await.last_error = Some(format!("Invalid address: {}", e));
                    self.status.write().await.state = "error".to_string();
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let listener = match self.create_reuse_listener(addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind WS listener: {}", e);
                    self.status.write().await.last_error = Some(format!("Bind failed: {}", e));
                    self.status.write().await.state = "error".to_string();
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            info!("WS server listening on {}", addr);
            self.status.write().await.bind_addr = format!("ws://{}:{}", config.host, config.port);

            self.accept_loop(listener).await;
        }
    }

    async fn create_reuse_listener(&self, addr: SocketAddr) -> anyhow::Result<tokio::net::TcpListener> {
        use socket2::{Socket, Domain, Type, Protocol};
        let domain = if addr.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 };
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        socket.bind(&addr.into())?;
        socket.listen(128)?;
        Ok(tokio::net::TcpListener::from_std(socket.into())?)
    }

    async fn accept_loop(&self, listener: tokio::net::TcpListener) {
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            info!("WS client connected from {}", peer_addr);

                            let config = self.config.read().await.clone();
                            let msg_svc = self.message_service.clone();
                            let status = self.status.clone();

                            status.write().await.connected = true;
                            status.write().await.state = "connected".to_string();
                            {
                                let mut s = status.write().await;
                                s.client_count += 1;
                            }

                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, config, msg_svc).await {
                                    warn!("WS connection error: {}", e);
                                }
                                let mut s = status.write().await;
                                s.connected = false;
                                s.state = "listening".to_string();
                                if s.client_count > 0 { s.client_count -= 1; }
                            });
                        }
                        Err(e) => {
                            warn!("Accept error: {}", e);
                        }
                    }
                }
                _ = self.shutdown_signal() => {
                    info!("WS accept loop shutdown");
                    break;
                }
            }
        }
    }

    async fn shutdown_signal(&self) {
        loop {
            if *self.shutdown.lock().await {
                return;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    pub async fn shutdown(&self) {
        *self.shutdown.lock().await = true;
    }

    pub async fn request_reload(&self) {
        let _ = self.reload_tx.send(self.reload_tx.borrow().wrapping_add(1));
    }

    pub async fn get_status(&self) -> WsServerStatus {
        self.status.read().await.clone()
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    config: WsServerConfig,
    message_service: Arc<MessageService>,
) -> anyhow::Result<()> {
    let peer_addr = stream.peer_addr()?;

    let ws_stream = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp: Response| {
        if config.token.is_empty() {
            info!("WS handshake OK (no token required) from {}", peer_addr);
            return Ok(resp);
        }

        let auth_header = req.headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let expected = format!("Bearer {}", config.token);
        if auth_header == expected {
            info!("WS handshake OK from {}", peer_addr);
            return Ok(resp);
        }

        warn!("WS handshake rejected (bad token) from {}", peer_addr);
        let err_resp = ErrorResponse::new(Some("Unauthorized: invalid token".to_string()));
        Err(err_resp)
    }).await?;

    info!("WS connection established with {}", peer_addr);
    let (mut write, mut read) = ws_stream.split();

    let mut ping_interval = tokio::time::interval(tokio::time::Duration::from_secs(15));

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if write.send(WsMessage::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
            message = read.next() => {
                match message {
                    Some(Ok(WsMessage::Text(text))) => {
                        let reply = process_incoming_message(&text, &message_service).await;
                        if let Some(reply_str) = reply {
                            let _ = write.send(WsMessage::Text(reply_str.into())).await;
                        }
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        let _ = write.send(WsMessage::Pong(data)).await;
                    }
                    Some(Ok(WsMessage::Close(_))) => {
                        info!("WS close from {}", peer_addr);
                        break;
                    }
                    Some(Ok(WsMessage::Pong(_))) => {}
                    Some(Err(e)) => {
                        warn!("WS error from {}: {}", peer_addr, e);
                        break;
                    }
                    None => {
                        info!("WS stream ended from {}", peer_addr);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!("WS disconnected from {}", peer_addr);
    Ok(())
}

async fn process_incoming_message(
    text: &str,
    message_service: &MessageService,
) -> Option<String> {
    let payload: WsIncomingPayload = match serde_json::from_str(text) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to parse WS message: {}", e);
            let reply = WsOutgoingReply {
                message_id: None,
                reply_type: "error".to_string(),
                text: Some(format!("Invalid JSON: {}", e)),
                confirm_token: None,
            };
            return Some(serde_json::to_string(&reply).unwrap_or_default());
        }
    };

    match payload {
        WsIncomingPayload::Ping => {
            let reply = WsOutgoingReply {
                message_id: None,
                reply_type: "pong".to_string(),
                text: None,
                confirm_token: None,
            };
            Some(serde_json::to_string(&reply).unwrap_or_default())
        }
        WsIncomingPayload::QqMessage {
            group_id, user_id, nickname, message_id, text, timestamp_ms, is_admin
        } => {
            let timestamp = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                .unwrap_or_else(Utc::now);

            let msg = IncomingQqMessage {
                group_id: group_id.clone(),
                user_id: user_id.clone(),
                nickname,
                message_id,
                text,
                timestamp,
                is_admin,
                attachments: vec![],
            };

            match message_service.handle_incoming(msg).await {
                Ok(reply) => {
                    let outgoing = match reply {
                        crate::inbound::command_router::BotReply::Silent => None,
                        crate::inbound::command_router::BotReply::Text(t) => {
                            Some(WsOutgoingReply {
                                message_id: None,
                                reply_type: "text".to_string(),
                                text: Some(t),
                                confirm_token: None,
                            })
                        }
                        crate::inbound::command_router::BotReply::NeedConfirm { text, confirm_token } => {
                            Some(WsOutgoingReply {
                                message_id: None,
                                reply_type: "need_confirm".to_string(),
                                text: Some(text),
                                confirm_token: Some(confirm_token),
                            })
                        }
                        crate::inbound::command_router::BotReply::AdminOnly(t) => {
                            Some(WsOutgoingReply {
                                message_id: None,
                                reply_type: "admin_only".to_string(),
                                text: Some(t),
                                confirm_token: None,
                            })
                        }
                    };
                    match outgoing {
                        Some(o) => {
                            info!("Sending reply: {} (type={})", 
                                o.text.as_deref().unwrap_or(""), o.reply_type);
                            Some(serde_json::to_string(&o).unwrap_or_default())
                        }
                        None => None,
                    }
                }
                Err(e) => {
                    let reply = WsOutgoingReply {
                        message_id: None,
                        reply_type: "error".to_string(),
                        text: Some(format!("处理失败: {}", e)),
                        confirm_token: None,
                    };
                    Some(serde_json::to_string(&reply).unwrap_or_default())
                }
            }
        }
    }
}
