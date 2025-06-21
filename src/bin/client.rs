use futures_util::SinkExt;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() {
    let url = "ws://127.0.0.1:8080";
    let (mut ws_stream, _) = connect_async(url).await.expect("Failed to connect");

    println!("✅ Connected to server");

    let mut count = 0;
    loop {
        let msg = format!("Ping #{}", count);
        ws_stream
            .send(Message::Text(msg.into()))
            .await
            .expect("Send failed");
        println!("📨 Sent Ping #{}", count);
        count += 1;
        sleep(Duration::from_secs(5)).await;
    }
}

