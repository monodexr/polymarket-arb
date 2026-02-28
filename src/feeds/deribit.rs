use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, warn};

use super::PriceTick;

const URL: &str = "wss://www.deribit.com/ws/api/v2";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            match run(&tx).await {
                Ok(()) => warn!("deribit WS closed, reconnecting"),
                Err(e) => error!(%e, "deribit WS error, reconnecting in 5s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

async fn run(tx: &mpsc::Sender<PriceTick>) -> anyhow::Result<()> {
    let (ws, _) = connect_async(URL).await?;
    let (mut write, mut read) = ws.split();

    let sub_index = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "public/subscribe",
        "params": {
            "channels": ["deribit_price_index.btc_usd"]
        }
    });
    write.send(Message::Text(sub_index.to_string().into())).await?;

    let sub_perp = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "public/subscribe",
        "params": {
            "channels": ["ticker.BTC-PERPETUAL.raw"]
        }
    });
    write.send(Message::Text(sub_perp.to_string().into())).await?;

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            parse_and_send(&text, tx).await;
        }
    }
    Ok(())
}

async fn parse_and_send(json: &str, tx: &mpsc::Sender<PriceTick>) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return,
    };

    let params = match v.get("params") {
        Some(p) => p,
        None => return,
    };
    let channel = match params.get("channel").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return,
    };
    let data = match params.get("data") {
        Some(d) => d,
        None => return,
    };

    // M2: Use exchange-side timestamp ("timestamp" field = epoch ms) with local fallback
    let ts_ms = data
        .get("timestamp")
        .and_then(|t| t.as_u64())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        });

    if channel == "deribit_price_index.btc_usd" {
        if let Some(price) = data.get("price").and_then(|p| p.as_f64()) {
            let _ = tx
                .send(PriceTick {
                    source: "deribit",
                    price,
                    timestamp_ms: ts_ms,
                })
                .await;
        }
    }

    if channel.starts_with("ticker.BTC-PERPETUAL") {
        if let Some(iv) = data.get("mark_iv").and_then(|v| v.as_f64()) {
            let annualized = iv / 100.0;
            debug!(iv = %format!("{:.2}%", iv), "deribit IV update");
            let _ = tx
                .send(PriceTick {
                    source: "deribit_iv",
                    price: annualized,
                    timestamp_ms: ts_ms,
                })
                .await;
        }
    }
}
