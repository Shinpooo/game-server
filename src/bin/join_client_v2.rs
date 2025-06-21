use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    JoinGame { id: Uuid },
}

#[derive(Serialize, Deserialize, Debug)]
struct Character {
    id: Uuid,
    x: u32,
    y: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (ws_stream, _) = connect_async("ws://127.0.0.1:8080").await?;
    println!("✅ Connected to server");

    let (mut write, mut read) = ws_stream.split();

    let id = Uuid::new_v4();
    let msg = ClientMessage::JoinGame { id };
    let json = serde_json::to_string(&msg)?;
    write.send(Message::Text(json.into())).await?;
    println!("🚀 Sent JoinGame with id: {id}");

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Try to parse the server's response into a HashMap of characters
                match serde_json::from_str::<HashMap<Uuid, Character>>(&text) {
                    Ok(characters) => {
                        println!("📦 Received character snapshot:");
                        for (uuid, character) in characters {
                            println!(
                                "🧍 Character {uuid} is at ({}, {})",
                                character.x, character.y
                            );
                        }
                    }
                    Err(e) => {
                        println!("⚠️ Failed to parse characters: {e}");
                        println!("📝 Raw message: {text}");
                    }
                }
            }
            Ok(Message::Close(_)) => {
                println!("🔚 Server closed the connection");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("❌ Error reading message: {e}");
                break;
            }
        }
    }

    Ok(())
}
