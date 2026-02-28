use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, warn};

use super::PriceTick;

const URL: &str = "wss://ws.okx.com:8443/ws/v5/public";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            match run(&tx).await {
                Ok(()) => warn!("okx WS closed, reconnecting"),
                Err(e) => error!(%e, "okx WS error, reconnecting in 1s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

async fn run(tx: &mpsc::Sender<PriceTick>) -> anyhow::Result<()> {
    let (ws, _) = connect_async(URL).await?;
    let (mut write, mut read) = ws.split();

    let sub = serde_json::json!({
        "op": "subscribe",
        "args": [{"channel": "trades", "instId": "BTC-USDT"}]
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
    let data = v.get("data")?.as_array()?;
    let trade = data.first()?;
    let price: f64 = trade.get("px")?.as_str()?.parse().ok()?;
    let ts: u64 = trade.get("ts")?.as_str()?.parse().ok()?;
    Some(PriceTick {
        source: "okx",
        price,
        timestamp_ms: ts,
    })
}
