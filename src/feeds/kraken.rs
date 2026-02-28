use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, warn};

use super::PriceTick;

const URL: &str = "wss://ws.kraken.com/v2";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            match run(&tx).await {
                Ok(()) => warn!("kraken WS closed, reconnecting"),
                Err(e) => error!(%e, "kraken WS error, reconnecting in 1s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn run(tx: &mpsc::Sender<PriceTick>) -> anyhow::Result<()> {
    let (ws, _) = connect_async(URL).await?;
    let (mut write, mut read) = ws.split();

    let sub = serde_json::json!({
        "method": "subscribe",
        "params": {
            "channel": "trade",
            "symbol": ["XBT/USD"]
        }
    });
    write.send(Message::Text(sub.to_string().into())).await?;

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            if let Some(tick) = parse_trade(&text) {
                let _ = tx.send(tick).await;
            }
        }
    }
    Ok(())
}

fn parse_trade(json: &str) -> Option<PriceTick> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("channel")?.as_str()? != "trade" {
        return None;
    }
    let data = v.get("data")?.as_array()?;
    let last = data.last()?;
    let price: f64 = last.get("price")?.as_f64()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    Some(PriceTick {
        source: "kraken",
        price,
        timestamp_ms: now_ms,
    })
}
