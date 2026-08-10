use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::StreamExt;
use tracing::{info, warn};

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
    clients: Arc<Mutex<HashMap<String, ClientHandle>>>,
}

impl WsManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, client_id: String) -> mpsc::Sender<String> {
        let (tx, _rx) = mpsc::channel(1000);
        let mut clients = self.clients.blocking_lock();
        clients.insert(client_id.clone(), ClientHandle {
            sender: tx.clone(),
            node_filter: None,
        });
        tx
    }

    pub async fn push_metrics(&self, node_id: &str, metrics: String) {
        let clients = self.clients.lock().await;
        let _to_remove: Vec<String> = Vec::new();
        
        for (id, client) in clients.iter() {
            if client.node_filter.as_ref().map(|n| n == node_id).unwrap_or(true) {
                if client.sender.send(metrics.clone()).await.is_err() {
                    warn!(client_id = %id, "Failed to send metrics, removing client");
                }
            }
        }
    }

    pub async fn push_job_update(&self, job_id: &str) {
        let clients = self.clients.lock().await;
        let payload = serde_json::to_string(&WsMessage {
            type_: "job_update".to_string(),
            job_id: Some(job_id.to_string()),
            ..Default::default()
        }).unwrap_or_default();
        for (id, client) in clients.iter() {
            if client.sender.send(payload.clone()).await.is_err() {
                warn!(client_id = %id, "Failed to send job update");
            }
        }
    }

    pub async fn push_job_log(&self, _entry: &protocol::JobLogEntry) {
        // WebSocket push for job logs handled at client level
    }

    pub async fn push_alert(&self, alert: String) {
        let clients = self.clients.lock().await;
        for (id, client) in clients.iter() {
            if client.sender.send(alert.clone()).await.is_err() {
                warn!(client_id = %id, "Failed to send alert");
            }
        }
    }

    pub async fn prune_dead_clients(&self) {
        let mut clients = self.clients.lock().await;
        let dead: Vec<String> = clients.iter()
            .filter_map(|(id, client)| {
                if client.sender.capacity() == 0 {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        
        for id in dead {
            clients.remove(&id);
        }
    }
}

/// Axum handler for `GET /ws` — upgrades the connection and registers the client.
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<std::sync::Arc<crate::AppState>>,
) -> Response {
    let handler = WsHandler {
        manager: state.ws_manager.clone(),
        node_id: None,
        jwt_secret: state.jwt_secret.clone(),
    };
    ws.on_upgrade(move |socket| async move { handler.handle(socket).await })
}

pub struct WsHandler {
    pub manager: WsManager,
    pub node_id: Option<String>,
    pub jwt_secret: String,
}

impl WsHandler {
    pub async fn handle(&self, mut ws: WebSocket) {
        let client_id = uuid::Uuid::new_v4().to_string();
        let _sender = self.manager.register(client_id.clone());
        info!(client_id = %client_id, "WebSocket client connected");

        // Send welcome
        if let Ok(welcome) = serde_json::to_string(&WsMessage {
            type_: "connected".to_string(),
            ..Default::default()
        }) {
            let _ = ws.send(Message::Text(welcome.into())).await;
        }

        loop {
            match ws.recv().await {
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

        info!(client_id = %client_id, "WebSocket client disconnected");
    }

    async fn handle_message(&self, client_id: &str, text: &str) {
        // Try to parse as subscribe request
        let parsed: Result<SubscribeRequest, _> = serde_json::from_str(text);
        
        match parsed {
            Ok(sub) => {
                let mut clients = self.manager.clients.lock().await;
                if let Some(client) = clients.get_mut(client_id) {
                    client.node_filter = sub.node_id;
                    let msg = serde_json::to_string(&WsMessage {
                        type_: "subscribed".to_string(),
                        ..Default::default()
                    }).unwrap_or_default();
                    let _ = client.sender.send(msg).await;
                }
            }
            Err(_) => {
                // Unknown message type
            }
        }
    }
}
