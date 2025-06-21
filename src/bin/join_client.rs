use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    JoinGame { id: Uuid },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Connect to the WebSocket server
    let (ws_stream, _) = connect_async("ws://127.0.0.1:8080").await?;
    println!("✅ Connected to server");

    let (mut write, mut read) = ws_stream.split();

    // 2. Generate a UUID and send JoinGame message
    let id = Uuid::new_v4();
    let msg = ClientMessage::JoinGame { id };
    let json = serde_json::to_string(&msg)?;
    write.send(Message::Text(json.into())).await?;
    println!("🚀 Sent JoinGame with id: {id}");

    // 3. Read server responses
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                println!("📩 Server: {text}");
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
