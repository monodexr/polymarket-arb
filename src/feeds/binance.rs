use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, warn};

use super::PriceTick;

// Binance.com blocks US IPs (451). Use Binance US as primary, fall back to .com.
const URL_US: &str = "wss://stream.binance.us:9443/ws/btcusdt@trade";
const URL_GLOBAL: &str = "wss://stream.binance.com:9443/ws/btcusdt@trade";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            // Try Binance US first (works from US IPs), then global
            for url in [URL_US, URL_GLOBAL] {
                match run(&tx, url).await {
                    Ok(()) => warn!(url, "binance WS closed, trying next"),
                    Err(e) => {
                        error!(url, %e, "binance WS error");
                        continue;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn run(tx: &mpsc::Sender<PriceTick>, url: &str) -> anyhow::Result<()> {
    let (ws, _) = connect_async(url).await?;
    let (mut _write, mut read) = ws.split();

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
    let price: f64 = v.get("p")?.as_str()?.parse().ok()?;
    let ts: u64 = v.get("T")?.as_u64()?;
    Some(PriceTick {
        source: "binance",
        price,
        timestamp_ms: ts,
    })
}
