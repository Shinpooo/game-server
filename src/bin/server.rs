use std::{env, io::Error};

use futures_util::StreamExt;
use log::info;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    JoinGame {
        id: Uuid,
    },
    MoveTo {
        x: u8,
        y: u8,
    },
    CastSpell {
        spell_id: u32,
        target_x: u8,
        target_y: u8,
    },
}

enum ServerMessage {
    GameStateSnapshot { game_state: GameState },
}

#[derive(Debug)]
struct Character {
    id: Uuid,
    x: u32,
    y: u32,
}

#[derive(Debug, Default)]
struct GameState {
    online_characters: HashMap<Uuid, Character>,
}

impl GameState {
    fn apply_event(&mut self, event: GameEvent) {
        match event {
            GameEvent::CharacterJoined { id } => {
                self.online_characters
                    .insert(id, Character { id: id, x: 0, y: 0 });
            }
        }
    }
}

enum GameEvent {
    CharacterJoined { id: Uuid },
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = env_logger::try_init();
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("LOG server Listening on: {}", addr);

    // let state = Arc::new(Mutex::new(GameState::default()));
    let state = GameState::default();
    let (tx, rx) = mpsc::channel::<GameEvent>(32);

    // Spawn monitoring task
    let monitor_state = state.clone();
    tokio::spawn(async move {
        loop {
            // Wait 5 seconds
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            // Lock the state and read the number of online players
            let gs = monitor_state.lock().await;
            let count = gs.online_characters.len();
            drop(gs);

            log::info!("👥 Currently connected players: {}", count);
        }
    });

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            state.apply_event(event);
        }
    });

    while let Ok((stream, _)) = listener.accept().await {
        let conn_state = state.clone();
        let client_tx = tx.clone();
        tokio::spawn(async move { accept_connection(stream, conn_state, client_tx).await });
    }

    Ok(())
}

async fn accept_connection(
    stream: TcpStream,
    state: Arc<Mutex<GameState>>,
    tx: tokio::sync::mpsc::Sender<GameEvent>,
) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    info!("Peer address: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    info!("New WebSocket connection: {}", addr);
    let mut character_id: Option<Uuid> = None;

    let (mut write, mut read) = ws_stream.split();

    while let Some(result) = read.next().await {
        match result {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(client_msg) => match client_msg {
                        ClientMessage::JoinGame { id } => {
                            tx.send(GameEvent::CharacterJoined { id });
                            let mut gs = state.lock().await;
                            gs.online_characters
                                .insert(id, Character { id, x: 0, y: 0 });
                            drop(gs);
                            info!("Character {} joined the world", id);
                            character_id = Some(id);

                            // create player, assign ID, send welcome
                        }
                        ClientMessage::MoveTo { x, y } => {
                            // update player position, broadcast
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
                // info!("📩 Received text from {}: {}", addr, text);
            }
            Ok(Message::Binary(data)) => {
                info!("📦 Received binary from {}: {:?}", addr, data);
            }
            Ok(Message::Close(_)) => {
                info!("🔚 {} closed the connection", addr);
                if let Some(id) = character_id {
                    let mut gs = state.lock().await;
                    gs.online_characters.remove_entry(&id);
                    drop(gs);
                    info!("Character {} left the world", id);
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
                    let mut gs = state.lock().await;
                    gs.online_characters.remove_entry(&id);
                    drop(gs);
                    info!("Character {} left the world", id);
                }

                break;
            }
        }
    }

    info!("👋 Disconnected: {}", addr);
}
