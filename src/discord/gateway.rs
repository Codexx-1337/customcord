use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{error, info, warn};

use crate::models::{Channel, Guild, Message, User};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connecting,
    Connected,
    Ready {
        user: User,
        guilds: Vec<Guild>,
        private_channels: Vec<Channel>,
    },
    MessageCreate(Message),
    GuildCreate(Guild),
    Disconnected(String),
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct GatewayPayload {
    op: u8,
    #[serde(default)]
    d: Option<Value>,
    #[serde(default)]
    s: Option<i64>,
    #[serde(default)]
    t: Option<String>,
}

pub struct GatewayClient {
    token: String,
    event_tx: UnboundedSender<ClientEvent>,
}

impl GatewayClient {
    pub fn new(token: String, event_tx: UnboundedSender<ClientEvent>) -> Self {
        Self { token, event_tx }
    }

    pub async fn run(&self) {
        let _ = self.event_tx.send(ClientEvent::Connecting);

        loop {
            info!("Connecting to Discord Gateway...");
            match connect_async(GATEWAY_URL).await {
                Ok((ws_stream, _)) => {
                    info!("WebSocket connection established.");
                    let _ = self.event_tx.send(ClientEvent::Connected);
                    let (mut write, mut read) = ws_stream.split();

                    let last_seq = Arc::new(AtomicI64::new(-1));
                    let mut heartbeat_task: Option<tokio::task::JoinHandle<()>> = None;

                    // Channel for outgoing WS messages
                    let (outgoing_tx, mut outgoing_rx) = tokio::sync::mpsc::channel::<String>(100);

                    // Task to forward outgoing messages to WS
                    let write_task = tokio::spawn(async move {
                        while let Some(msg) = outgoing_rx.recv().await {
                            if let Err(e) = write.send(WsMessage::Text(msg)).await {
                                warn!("Failed to write to WebSocket: {}", e);
                                break;
                            }
                        }
                    });

                    while let Some(msg_result) = read.next().await {
                        match msg_result {
                            Ok(WsMessage::Text(text)) => {
                                if let Ok(payload) = serde_json::from_str::<GatewayPayload>(&text) {
                                    if let Some(s) = payload.s {
                                        last_seq.store(s, Ordering::Relaxed);
                                    }

                                    match payload.op {
                                        10 => {
                                            // HELLO
                                            if let Some(d) = payload.d {
                                                if let Some(interval) = d.get("heartbeat_interval").and_then(|v| v.as_u64()) {
                                                    let seq_clone = Arc::clone(&last_seq);
                                                    let out_tx = outgoing_tx.clone();
                                                    
                                                    // Spawn heartbeat loop
                                                    heartbeat_task = Some(tokio::spawn(async move {
                                                        let mut interval_timer = tokio::time::interval(Duration::from_millis(interval));
                                                        loop {
                                                            interval_timer.tick().await;
                                                            let s = seq_clone.load(Ordering::Relaxed);
                                                            let hb = json!({
                                                                "op": 1,
                                                                "d": if s >= 0 { json!(s) } else { json!(null) }
                                                            });
                                                            if out_tx.send(hb.to_string()).await.is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }));

                                                    // Send IDENTIFY
                                                    let identify = json!({
                                                        "op": 2,
                                                        "d": {
                                                            "token": self.token,
                                                            "capabilities": 8189,
                                                            "properties": {
                                                                "os": "Windows",
                                                                "browser": "Discord Client",
                                                                "device": "discord-desktop",
                                                                "system_locale": "en-US",
                                                                "browser_user_agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
                                                                "browser_version": "130.0.0.0",
                                                                "os_version": "10.0.19045",
                                                                "release_channel": "stable",
                                                                "client_build_number": 341000
                                                            },
                                                            "presence": {
                                                                "status": "online",
                                                                "since": 0,
                                                                "activities": [],
                                                                "afk": false
                                                            },
                                                            "compress": false
                                                        }
                                                    });
                                                    let _ = outgoing_tx.send(identify.to_string()).await;
                                                }
                                            }
                                        }
                                        1 => {
                                            // Requested Heartbeat
                                            let s = last_seq.load(Ordering::Relaxed);
                                            let hb = json!({
                                                "op": 1,
                                                "d": if s >= 0 { json!(s) } else { json!(null) }
                                            });
                                            let _ = outgoing_tx.send(hb.to_string()).await;
                                        }
                                        0 => {
                                            // DISPATCH
                                            if let (Some(event_name), Some(data)) = (payload.t, payload.d) {
                                                self.handle_dispatch(&event_name, data);
                                            }
                                        }
                                        7 | 9 => {
                                            // RECONNECT or INVALID_SESSION
                                            warn!("Gateway requested reconnect (op {})", payload.op);
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(WsMessage::Close(reason)) => {
                                warn!("WebSocket closed: {:?}", reason);
                                break;
                            }
                            Err(e) => {
                                error!("WebSocket read error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }

                    if let Some(task) = heartbeat_task {
                        task.abort();
                    }
                    write_task.abort();
                    let _ = self.event_tx.send(ClientEvent::Disconnected("Connection lost".to_string()));
                }
                Err(e) => {
                    error!("Failed to connect to gateway: {}", e);
                    let _ = self.event_tx.send(ClientEvent::Error(format!("Connection error: {}", e)));
                }
            }

            info!("Waiting 5 seconds before reconnecting...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    fn handle_dispatch(&self, event_name: &str, data: Value) {
        match event_name {
            "READY" => {
                info!("Gateway READY event received");
                let user: Result<User, _> = serde_json::from_value(data.get("user").cloned().unwrap_or(Value::Null));
                let guilds: Vec<Guild> = data
                    .get("guilds")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|g| serde_json::from_value::<Guild>(g.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default();

                let private_channels: Vec<Channel> = data
                    .get("private_channels")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| serde_json::from_value::<Channel>(c.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default();

                if let Ok(user) = user {
                    let _ = self.event_tx.send(ClientEvent::Ready {
                        user,
                        guilds,
                        private_channels,
                    });
                }
            }
            "MESSAGE_CREATE" => {
                if let Ok(msg) = serde_json::from_value::<Message>(data) {
                    let _ = self.event_tx.send(ClientEvent::MessageCreate(msg));
                }
            }
            "GUILD_CREATE" => {
                if let Ok(guild) = serde_json::from_value::<Guild>(data) {
                    let _ = self.event_tx.send(ClientEvent::GuildCreate(guild));
                }
            }
            _ => {}
        }
    }
}
