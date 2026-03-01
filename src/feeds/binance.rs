use std::time::Instant;

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use super::PriceTick;

const URL_GLOBAL: &str = "wss://stream.binance.com:9443/stream?streams=btcusdt@trade/ethusdt@trade/solusdt@trade/xrpusdt@trade";
const URL_US: &str = "wss://stream.binance.us:9443/stream?streams=btcusdt@trade/ethusdt@trade/solusdt@trade/xrpusdt@trade";

pub fn spawn(tx: mpsc::Sender<PriceTick>) {
    tokio::spawn(async move {
        loop {
            for (i, url) in [URL_GLOBAL, URL_US].iter().enumerate() {
                if i > 0 {
                    warn!(url, "FALLING BACK to secondary binance endpoint (higher latency)");
                }
                info!(url, "connecting binance WS");
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
    let parsed = url::Url::parse(url)?;
    let host = parsed.host_str().unwrap_or("stream.binance.com");
    let port = parsed.port().unwrap_or(9443);
    let tcp = TcpStream::connect(format!("{host}:{port}")).await?;
    tcp.set_nodelay(true)?;

    let (ws, _) = tokio_tungstenite::client_async_tls(url, tcp).await?;
    let (_write, mut read) = ws.split();

    let mut tick_count = 0u64;

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            if let Some(tick) = parse_combined_trade(&text) {
                tick_count += 1;
                if tick_count == 1 || tick_count % 1000 == 0 {
                    info!(
                        source = tick.source,
                        price = %format!("{:.2}", tick.price),
                        total_ticks = tick_count,
                        "binance tick"
                    );
                }
                let _ = tx.try_send(tick);
            }
        }
    }
    Ok(())
}

/// Parse combined stream format: {"stream":"btcusdt@trade","data":{...}}
fn parse_combined_trade(json: &str) -> Option<PriceTick> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;

    // Combined stream wraps in {"stream":"...","data":{...}}
    let data = v.get("data").unwrap_or(&v);

    let price: f64 = data.get("p")?.as_str()?.parse().ok()?;
    let ts: u64 = data.get("T")?.as_u64()?;

    // Identify which asset from the symbol field
    let symbol = data.get("s")?.as_str()?.to_lowercase();
    let source: &'static str = if symbol.starts_with("btc") {
        "btc"
    } else if symbol.starts_with("eth") {
        "eth"
    } else if symbol.starts_with("sol") {
        "sol"
    } else if symbol.starts_with("xrp") {
        "xrp"
    } else {
        return None;
    };

    Some(PriceTick {
        source,
        price,
        timestamp_ms: ts,
        received_at: Instant::now(),
    })
}
