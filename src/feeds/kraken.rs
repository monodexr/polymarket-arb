use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, warn};

use super::PriceTick;

// V2 may reset on some IPs; try both endpoints
const URL_V2: &str = "wss://ws.kraken.com/v2";
const URL_V1: &str = "wss://ws.kraken.com";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            for (url, version) in [(URL_V2, "v2"), (URL_V1, "v1")] {
                match run(&tx, url, version).await {
                    Ok(()) => warn!(url, "kraken WS closed, trying next"),
                    Err(e) => {
                        error!(url, %e, "kraken WS error");
                        continue;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn run(tx: &mpsc::Sender<PriceTick>, url: &str, version: &str) -> anyhow::Result<()> {
    let (ws, _) = connect_async(url).await?;
    let (mut write, mut read) = ws.split();

    let sub = if version == "v2" {
        serde_json::json!({
            "method": "subscribe",
            "params": {
                "channel": "trade",
                "symbol": ["XBT/USD"]
            }
        })
    } else {
        serde_json::json!({
            "event": "subscribe",
            "pair": ["XBT/USD"],
            "subscription": { "name": "trade" }
        })
    };
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

    let now_ms = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };

    // V2 format: {"channel":"trade","data":[{"price":...,"timestamp":"..."}]}
    if v.get("channel").and_then(|c| c.as_str()) == Some("trade") {
        let data = v.get("data")?.as_array()?;
        let last = data.last()?;
        let price: f64 = last.get("price")?.as_f64()?;
        let ts_ms = last
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp_millis() as u64)
            .unwrap_or_else(now_ms);
        return Some(PriceTick { source: "kraken", price, timestamp_ms: ts_ms });
    }

    // V1 format: [channelID, [["price","volume","time","side","type","misc"], ...], "trade", "XBT/USD"]
    if let Some(arr) = v.as_array() {
        if arr.len() >= 4 && arr.last().and_then(|v| v.as_str()) == Some("XBT/USD") {
            let trades = arr.get(1)?.as_array()?;
            let last = trades.last()?.as_array()?;
            let price: f64 = last.first()?.as_str()?.parse().ok()?;
            let ts: f64 = last.get(2)?.as_str()?.parse().ok()?;
            let ts_ms = (ts * 1000.0) as u64;
            return Some(PriceTick { source: "kraken", price, timestamp_ms: ts_ms });
        }
    }

    None
}
