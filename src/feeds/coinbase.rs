use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, warn};

use super::PriceTick;

const URL: &str = "wss://ws-feed.exchange.coinbase.com";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            match run(&tx).await {
                Ok(()) => warn!("coinbase WS closed, reconnecting"),
                Err(e) => error!(%e, "coinbase WS error, reconnecting in 1s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn run(tx: &mpsc::Sender<PriceTick>) -> anyhow::Result<()> {
    let (ws, _) = connect_async(URL).await?;
    let (mut write, mut read) = ws.split();

    let sub = serde_json::json!({
        "type": "subscribe",
        "product_ids": ["BTC-USD"],
        "channels": ["matches"]
    });
    write.send(Message::Text(sub.to_string().into())).await?;

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            if let Some(tick) = parse_match(&text) {
                let _ = tx.send(tick).await;
            }
        }
    }
    Ok(())
}

fn parse_match(json: &str) -> Option<PriceTick> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("type")?.as_str()? != "match" {
        return None;
    }
    let price: f64 = v.get("price")?.as_str()?.parse().ok()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    Some(PriceTick {
        source: "coinbase",
        price,
        timestamp_ms: now_ms,
    })
}
