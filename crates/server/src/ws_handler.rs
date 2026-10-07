use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::auth_middleware;

/// Per-source-IP connection cap. The global `max_concurrent_ws_clients` is a
/// soft limit that a burst can overshoot; this one stops a single client
/// from pinning hundreds of connections (each holds an mpsc buffer of
/// `ws_max_backlog` messages + a task) when the server runs in read-only
/// mode where the upgrade requires no token at all.
const MAX_WS_CONNS_PER_IP: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubscribeRequest {
    #[serde(default)]
    pub node_id: Option<String>,
}

/// JSON message pushed to WebSocket clients (mirrors proto `WebSocketMessage`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WsMessage {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ClientHandle {
    pub sender: mpsc::Sender<String>,
    pub node_filter: Option<String>,
    /// Source IP the client connected from (for per-IP connection caps).
    pub ip: Option<String>,
}

#[derive(Clone)]
pub struct WsManager {
    clients: Arc<tokio::sync::Mutex<HashMap<String, ClientHandle>>>,
    /// ip -> number of live connections (mirrors `clients`; kept separately
    /// so the per-IP cap can be checked without iterating all clients).
    ip_counts: Arc<tokio::sync::Mutex<HashMap<String, usize>>>,
}

impl WsManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            ip_counts: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(
        &self,
        client_id: String,
        sender: mpsc::Sender<String>,
        ip: Option<String>,
    ) {
        let mut clients = self.clients.lock().await;
        clients.insert(
            client_id,
            ClientHandle {
                sender,
                node_filter: None,
                ip: ip.clone(),
            },
        );
        if let Some(ip) = ip {
            let mut counts = self.ip_counts.lock().await;
            *counts.entry(ip).or_default() += 1;
        }
    }

    pub async fn unregister(&self, client_id: &str) {
        let mut clients = self.clients.lock().await;
        if let Some(handle) = clients.remove(client_id)
            && let Some(ip) = handle.ip
        {
            let mut counts = self.ip_counts.lock().await;
            if let Some(c) = counts.get_mut(&ip) {
                *c -= 1;
                if *c == 0 {
                    counts.remove(&ip);
                }
            }
        }
    }

    /// Number of live connections from a source IP (for the per-IP cap).
    pub async fn ip_conn_count(&self, ip: &str) -> usize {
        self.ip_counts.lock().await.get(ip).copied().unwrap_or(0)
    }

    /// Number of currently connected clients (soft limit check before upgrade).
    pub async fn len(&self) -> usize {
        self.clients.lock().await.len()
    }

    /// Broadcast a message. `filter`: when set, only clients subscribed to that
    /// node (or to everything) receive it. Slow/dead clients are dropped.
    async fn broadcast(&self, message: String, filter: Option<&str>) {
        let mut clients = self.clients.lock().await;
        let mut dead: Vec<String> = Vec::new();

        for (id, client) in clients.iter_mut() {
            if let Some(node) = filter
                && let Some(f) = &client.node_filter
                && f != node
            {
                continue;
            }
            match client.sender.try_send(message.clone()) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Slow consumer: drop it rather than block the broadcast.
                    warn!(client_id = %id, "WebSocket client too slow, disconnecting");
                    dead.push(id.clone());
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    dead.push(id.clone());
                }
            }
        }

        for id in dead {
            clients.remove(&id);
        }
    }

    pub async fn push_metrics(&self, node_id: &str, metrics: String) {
        self.broadcast(metrics, Some(node_id)).await;
    }

    pub async fn push_job_update(&self, job_id: &str) {
        let payload = serde_json::to_string(&WsMessage {
            type_: "job_update".to_string(),
            job_id: Some(job_id.to_string()),
            ..Default::default()
        })
        .unwrap_or_default();
        self.broadcast(payload, None).await;
    }

    pub async fn push_alert(&self, alert: String) {
        self.broadcast(alert, None).await;
    }
}

/// Axum handler for `GET /ws` — upgrades the connection and registers the client.
/// When `auth_required` is set, a valid JWT must be supplied via
/// `?token=` query param or `Authorization: Bearer` header.
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<std::sync::Arc<crate::AppState>>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let client_ip = addr.ip().to_string();
    if state.config.auth_required {
        // Prefer the Authorization header (does not leak into proxy logs,
        // browser history or referrer strings the way a ?token= query param
        // does). The query param remains supported for clients that cannot
        // set headers on a WebSocket upgrade (e.g. the browser `WebSocket`
        // API).
        let token = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(|s| s.to_string())
            .or_else(|| params.get("token").cloned());
        // Same checks as the HTTP middleware: signature + the subject must
        // still be an enabled, unlocked user (a deleted/disabled account's
        // unexpired token must not keep a live metrics feed).
        let authorized = match token
            .as_deref()
            .map(|t| auth_middleware::validate_token(t, &state.jwt_secret))
        {
            Some(Ok(claims)) => auth_middleware::user_is_active(&state, &claims.sub).await,
            _ => false,
        };
        if !authorized {
            return (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response();
        }
    }

    // Per-IP cap first: cheaper than the global check and stops one source
    // from pinning many connections (relevant in read-only mode where no
    // token is needed at all).
    if state.ws_manager.ip_conn_count(&client_ip).await >= MAX_WS_CONNS_PER_IP {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many websocket connections from this address",
        )
            .into_response();
    }

    // Enforce the configured concurrent-client limit (soft: checked before
    // upgrade, so a burst of simultaneous connections can still overshoot by
    // a few). Prevents unbounded connection/memory growth from many clients.
    if state.config.max_concurrent_ws_clients > 0
        && state.ws_manager.len().await >= state.config.max_concurrent_ws_clients
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "too many websocket clients",
        )
            .into_response();
    }

    let handler = WsHandler {
        manager: state.ws_manager.clone(),
        max_backlog: state.config.ws_max_backlog,
        client_ip,
        // Ping interval for detecting half-open connections (a dead peer
        // would otherwise hold its per-IP/global connection slot forever).
        heartbeat_interval: std::time::Duration::from_secs(
            state.config.ws_heartbeat_interval_secs.max(1),
        ),
    };
    ws.on_upgrade(move |socket| async move { handler.handle(socket).await })
}

pub struct WsHandler {
    pub manager: WsManager,
    /// Outbound queue depth per client (from config; slow clients are
    /// disconnected when they fill it).
    pub max_backlog: usize,
    /// Source IP of this connection (reported on connect/disconnect and
    /// used by the manager to maintain the per-IP connection count).
    pub client_ip: String,
    /// How often to send a WebSocket Ping frame; a peer that stops answering
    /// (or whose TCP half is dead) is dropped by the send error or the
    /// socket error on the next recv, freeing its slot.
    pub heartbeat_interval: std::time::Duration,
}

impl WsHandler {
    pub async fn handle(&self, mut ws: WebSocket) {
        let client_id = uuid::Uuid::new_v4().to_string();

        // Channel between broadcasters and this socket. The receiver lives in
        // this task; broadcasters only hold the sender half.
        let (tx, mut rx) = mpsc::channel::<String>(self.max_backlog.max(1));
        self.manager
            .register(client_id.clone(), tx, Some(self.client_ip.clone()))
            .await;
        info!(client_id = %client_id, ip = %self.client_ip, "WebSocket client connected");

        // Send welcome
        if let Ok(welcome) = serde_json::to_string(&WsMessage {
            type_: "connected".to_string(),
            ..Default::default()
        }) {
            let _ = ws.send(Message::Text(welcome.into())).await;
        }

        // Periodic Ping frames; the first tick fires immediately, so consume
        // it before the loop to avoid an instant ping.
        let mut heartbeat = tokio::time::interval(self.heartbeat_interval);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        heartbeat.tick().await;

        // Interleave inbound socket messages with outbound broadcast queue.
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if ws.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
                inbound = ws.recv() => {
                    match inbound {
                        Some(Ok(Message::Text(text))) => {
                            self.handle_message(&client_id, &text).await;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = ws.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) => {
                            break;
                        }
                        Some(Err(e)) => {
                            warn!(client_id = %client_id, error = %e, "WebSocket error");
                            break;
                        }
                        None => {
                            break;
                        }
                        _ => {
                            // Ignore Binary/Pong messages
                        }
                    }
                }
                outbound = rx.recv() => {
                    match outbound {
                        Some(text) => {
                            if ws.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        self.manager.unregister(&client_id).await;
        info!(client_id = %client_id, "WebSocket client disconnected");
    }

    async fn handle_message(&self, client_id: &str, text: &str) {
        // Try to parse as subscribe request
        let parsed: Result<SubscribeRequest, _> = serde_json::from_str(text);

        match parsed {
            Ok(sub) => {
                let mut clients = self.manager.clients.lock().await;
                if let Some(client) = clients.get_mut(client_id) {
                    // Empty string is treated as "all nodes" (no filter).
                    client.node_filter = sub.node_id.filter(|id| !id.is_empty());
                    let msg = serde_json::to_string(&WsMessage {
                        type_: "subscribed".to_string(),
                        ..Default::default()
                    })
                    .unwrap_or_default();
                    let _ = client.sender.try_send(msg);
                }
            }
            Err(_) => {
                // Unknown message type
            }
        }
    }
}
