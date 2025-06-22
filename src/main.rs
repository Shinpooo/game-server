use std::{env, io::Error};

use futures_util::SinkExt;
use futures_util::StreamExt;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    JoinGame {
        id: Uuid,
    },
    MoveTo {
        id: Uuid,
        x: u32,
        y: u32,
    },
    CastSpell {
        spell_id: u32,
        target_x: u32,
        target_y: u32,
    },
    Chat {
        id: Uuid,
        text: String,
    },
}

enum ServerMessage {
    GameStateSnapshot {
        game_state: GameState,
    },
    CharactersSnapshot {
        characters: HashMap<Uuid, Character>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Character {
    id: Uuid,
    x: u32,
    y: u32,
}

#[derive(Debug, Default, Clone)]
struct GameState {
    online_characters: HashMap<Uuid, Character>,
    sessions: HashMap<Uuid, UnboundedSender<OutgoingMessage>>,
}

impl GameState {
    fn apply_event(&mut self, event: GameEvent) {
        match event {
            GameEvent::CharacterJoined { id, session_tx } => {
                self.online_characters
                    .insert(id, Character { id: id, x: 0, y: 0 });
                self.sessions.insert(id, session_tx);
                info!("Character {} has joined the world.", id);

                for session_tx in self.sessions.values() {
                    let _ = session_tx.send(OutgoingMessage::CharactersSnapshot {
                        characters: (self.online_characters.clone()),
                    });
                }
            }
            GameEvent::CharacterLeft { id } => {
                self.online_characters.remove(&id);
                self.sessions.remove(&id);
                info!("Character {} has left the world.", id);

                for session_tx in self.sessions.values() {
                    let _ = session_tx.send(OutgoingMessage::CharactersSnapshot {
                        characters: (self.online_characters.clone()),
                    });
                }
            }

            GameEvent::ChatMessage { id, text } => {
                for session_tx in self.sessions.values() {
                    let _ = session_tx.send(OutgoingMessage::ChatBroadcast {
                        id,
                        text: text.clone(),
                    });
                }
            }
            GameEvent::CharacterMoved { id, x, y } => {
                if x >= 10 || y >= 10 {
                    log::info!(
                        "⛔ Rejected move for {} to out-of-bounds position: ({}, {})",
                        id,
                        x,
                        y
                    );
                    return;
                }

                if let Some(character) = self.online_characters.get_mut(&id) {
                    character.x = x;
                    character.y = y;
                }
                for session_tx in self.sessions.values() {
                    let _ = session_tx.send(OutgoingMessage::CharactersSnapshot {
                        characters: self.online_characters.clone(),
                    });
                }
            }
            GameEvent::Snapshot { sender } => {
                let n = self.online_characters.len();
                let _ = sender.send(n);
            }
        }
    }
}

enum GameEvent {
    CharacterJoined {
        id: Uuid,
        session_tx: UnboundedSender<OutgoingMessage>,
    },
    CharacterLeft {
        id: Uuid,
    },
    CharacterMoved {
        id: Uuid,
        x: u32,
        y: u32,
    },
    Snapshot {
        sender: tokio::sync::oneshot::Sender<usize>,
    },
    ChatMessage {
        id: Uuid,
        text: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum OutgoingMessage {
    CharactersSnapshot {
        characters: HashMap<Uuid, Character>,
    },
    ChatBroadcast {
        id: Uuid,
        text: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = env_logger::try_init();
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8080".to_string());

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("LOG server Listening on: {}", addr);

    // let state = Arc::new(Mutex::new(GameState::default()));
    let mut state = GameState::default();
    let (tx, mut rx) = mpsc::channel::<GameEvent>(32);
    let tx_monitor = tx.clone();
    // Spawn monitoring task
    // let monitor_state = state.clone();
    tokio::spawn(async move {
        loop {
            let (tx1, rx1) = oneshot::channel::<usize>();
            let _ = tx_monitor.send(GameEvent::Snapshot { sender: (tx1) }).await;
            let count = rx1.await.unwrap();
            log::info!("👥 Currently connected players: {}", count);
            // Wait 5 seconds
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            // Lock the state and read the number of online players
            // let gs = monitor_state.lock().await;
            // let count = gs.online_characters.len();
            // drop(gs);
        }
    });

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            state.apply_event(event);
        }
    });

    while let Ok((stream, _)) = listener.accept().await {
        // let conn_state = state.clone();
        let client_tx = tx.clone();
        tokio::spawn(async move { accept_connection(stream, client_tx).await });
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream, game_tx: tokio::sync::mpsc::Sender<GameEvent>) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    info!("Peer address: {}", addr);

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            info!("Failed WebSocket handshake from {}: {}", addr, e);
            return;
        }
    };

    info!("New WebSocket connection: {}", addr);
    let mut character_id: Option<Uuid> = None;
    // Client -> server_client_task
    let (mut write, mut read) = ws_stream.split();

    // server_client_task -> server_gs_task
    let (client_tx, mut client_rx): (
        UnboundedSender<OutgoingMessage>,
        UnboundedReceiver<OutgoingMessage>,
    ) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    let _ = write.send(Message::Text(json.into())).await;
                }
                Err(e) => {
                    log::error!("Failed to serialize characters: {}", e);
                }
            }
        }
    });

    while let Some(result) = read.next().await {
        match result {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(client_msg) => match client_msg {
                        ClientMessage::JoinGame { id } => {
                            let _ = game_tx
                                .send(GameEvent::CharacterJoined {
                                    id,
                                    session_tx: client_tx.clone(),
                                })
                                .await;
                            character_id = Some(id);
                        }
                        ClientMessage::Chat { id, text } => {
                            let _ = game_tx.send(GameEvent::ChatMessage { id, text }).await;
                        }
                        ClientMessage::MoveTo { id, x, y } => {
                            let _ = game_tx.send(GameEvent::CharacterMoved { id, x, y }).await;
                        }
                        ClientMessage::CastSpell {
                            spell_id,
                            target_x,
                            target_y,
                        } => {
                            // simulate a spell effect
                        }
                    },
                    Err(e) => {
                        eprintln!("Failed to parse message: {:?}", e);
                    }
                }
            }
            Ok(Message::Binary(data)) => {
                info!("📦 Received binary from {}: {:?}", addr, data);
            }
            Ok(Message::Close(_)) => {
                info!("🔚 {} closed the connection", addr);

                if let Some(id) = character_id {
                    let _ = game_tx.send(GameEvent::CharacterLeft { id: id }).await;
                }
                break;
            }
            Ok(msg) => {
                info!(
                    "⚠️ Ignored non-text/binary message from {}: {:?}",
                    addr, msg
                );
            }
            Err(e) => {
                // ⚠️ Unexpected socket error (client crashed, Ctrl-C, unplugged, etc.)
                info!("❌ WebSocket error from {}: {}", addr, e);
                if let Some(id) = character_id {
                    let _ = game_tx.send(GameEvent::CharacterLeft { id: id }).await;
                }

                break;
            }
        }
    }

    info!("👋 Disconnected: {}", addr);
}
