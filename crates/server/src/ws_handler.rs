use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::auth_middleware;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubscribeRequest {
    #[serde(default)]
    pub node_id: Option<String>,
}

/// JSON message pushed to WebSocket clients (mirrors proto `WebSocketMessage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for WsMessage {
    fn default() -> Self {
        Self {
            type_: String::new(),
            node_id: None,
            job_id: None,
            payload: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientHandle {
    pub sender: mpsc::Sender<String>,
    pub node_filter: Option<String>,
}

#[derive(Clone)]
pub struct WsManager {
    clients: Arc<tokio::sync::Mutex<HashMap<String, ClientHandle>>>,
}

impl WsManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(&self, client_id: String, sender: mpsc::Sender<String>) {
        let mut clients = self.clients.lock().await;
        clients.insert(
            client_id,
            ClientHandle {
                sender,
                node_filter: None,
            },
        );
    }

    pub async fn unregister(&self, client_id: &str) {
        let mut clients = self.clients.lock().await;
        clients.remove(client_id);
    }

    /// Broadcast a message. `filter`: when set, only clients subscribed to that
    /// node (or to everything) receive it. Slow/dead clients are dropped.
    async fn broadcast(&self, message: String, filter: Option<&str>) {
        let mut clients = self.clients.lock().await;
        let mut dead: Vec<String> = Vec::new();

        for (id, client) in clients.iter_mut() {
            if let Some(node) = filter {
                if let Some(f) = &client.node_filter {
                    if f != node {
                        continue;
                    }
                }
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
) -> Response {
    if state.config.auth_required {
        let token = params.get("token").cloned().or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer "))
                .map(|s| s.to_string())
        });
        let authorized = token
            .as_deref()
            .map(|t| auth_middleware::validate_token(t, &state.jwt_secret).is_ok())
            .unwrap_or(false);
        if !authorized {
            return (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response();
        }
    }

    let handler = WsHandler {
        manager: state.ws_manager.clone(),
    };
    ws.on_upgrade(move |socket| async move { handler.handle(socket).await })
}

pub struct WsHandler {
    pub manager: WsManager,
}

impl WsHandler {
    pub async fn handle(&self, mut ws: WebSocket) {
        let client_id = uuid::Uuid::new_v4().to_string();

        // Channel between broadcasters and this socket. The receiver lives in
        // this task; broadcasters only hold the sender half.
        let (tx, mut rx) = mpsc::channel::<String>(1000);
        self.manager.register(client_id.clone(), tx).await;
        info!(client_id = %client_id, "WebSocket client connected");

        // Send welcome
        if let Ok(welcome) = serde_json::to_string(&WsMessage {
            type_: "connected".to_string(),
            ..Default::default()
        }) {
            let _ = ws.send(Message::Text(welcome.into())).await;
        }

        // Interleave inbound socket messages with outbound broadcast queue.
        loop {
            tokio::select! {
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
