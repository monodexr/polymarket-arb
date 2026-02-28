use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::watch;
use tracing::{error, info, warn};

use super::discovery::MarketStateRx;

#[derive(Debug, Clone, Default)]
pub struct TokenBook {
    pub best_bid: f64,
    pub best_ask: f64,
    pub mid: f64,
    pub bid_depth: f64,
    pub ask_depth: f64,
    pub timestamp_ms: u64,
}

pub type BookSnapshot = HashMap<String, TokenBook>;
pub type BookTx = watch::Sender<BookSnapshot>;
pub type BookRx = watch::Receiver<BookSnapshot>;

pub fn spawn(market_rx: MarketStateRx) -> BookRx {
    let (tx, rx) = watch::channel(BookSnapshot::new());

    tokio::spawn(async move {
        loop {
            match run_ws(&market_rx, &tx).await {
                Ok(()) => warn!("CLOB WS closed, reconnecting"),
                Err(e) => error!(%e, "CLOB WS error, reconnecting in 2s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    rx
}

async fn run_ws(market_rx: &MarketStateRx, book_tx: &BookTx) -> anyhow::Result<()> {
    let token_ids: Vec<String> = {
        let markets = market_rx.borrow();
        let mut ids: Vec<String> = Vec::new();
        for market in markets.values() {
            ids.push(market.yes_token.clone());
            ids.push(market.no_token.clone());
        }
        ids
    };

    if token_ids.is_empty() {
        info!("no markets discovered yet, waiting 10s");
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        return Ok(());
    }

    info!(tokens = token_ids.len(), "subscribing to CLOB WS");

    let (ws, _) = tokio_tungstenite::connect_async(
        "wss://ws-subscriptions-clob.polymarket.com/ws/market",
    )
    .await?;
    let (mut write, mut read) = ws.split();

    let sub_msg = serde_json::json!({
        "assets_ids": token_ids,
        "type": "market",
        "custom_feature_enabled": true
    });

    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    write
        .send(Message::Text(sub_msg.to_string().into()))
        .await?;

    let write = Arc::new(tokio::sync::Mutex::new(write));
    let write_ping = write.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let mut w = write_ping.lock().await;
            if w.send(Message::Ping(vec![].into())).await.is_err() {
                break;
            }
        }
    });

    // M1: Watch for market changes — reconnect when discovery finds new tokens
    let mut market_watch = market_rx.clone();
    let token_set: std::collections::HashSet<String> = token_ids.into_iter().collect();

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        process_clob_message(&text, book_tx);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                }
            }
            _ = market_watch.changed() => {
                let new_ids: std::collections::HashSet<String> = {
                    let markets = market_watch.borrow();
                    let mut ids = std::collections::HashSet::new();
                    for m in markets.values() {
                        ids.insert(m.yes_token.clone());
                        ids.insert(m.no_token.clone());
                    }
                    ids
                };
                if new_ids != token_set {
                    info!(old = token_set.len(), new = new_ids.len(), "market set changed, reconnecting CLOB WS");
                    return Ok(()); // force reconnect loop with new token set
                }
            }
        }
    }
}

fn process_clob_message(json: &str, book_tx: &BookTx) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event_type = v.get("event_type").and_then(|e| e.as_str()).unwrap_or("");
    let asset_id = v
        .get("asset_id")
        .and_then(|a| a.as_str())
        .unwrap_or("");

    if asset_id.is_empty() {
        return;
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    match event_type {
        "book" | "price_change" => {
            if let Some(book) = parse_book_update(&v) {
                book_tx.send_modify(|snap| {
                    snap.insert(asset_id.to_string(), book);
                });
            }
        }
        "best_bid_ask" => {
            if let Some((bid, ask)) = parse_best_bid_ask(&v) {
                book_tx.send_modify(|snap| {
                    let entry = snap.entry(asset_id.to_string()).or_default();
                    entry.best_bid = bid;
                    entry.best_ask = ask;
                    entry.mid = (bid + ask) / 2.0;
                    entry.timestamp_ms = now_ms;
                });
            }
        }
        _ => {}
    }
}

fn parse_book_update(v: &serde_json::Value) -> Option<TokenBook> {
    let bids = v.get("bids")?.as_array()?;
    let asks = v.get("asks")?.as_array()?;

    let best_bid = bids
        .iter()
        .filter_map(|b| {
            let price: f64 = b.get("price")?.as_str()?.parse().ok()?;
            let size: f64 = b.get("size")?.as_str()?.parse().ok()?;
            if size > 0.0 { Some(price) } else { None }
        })
        .fold(0.0f64, f64::max);

    let best_ask = asks
        .iter()
        .filter_map(|a| {
            let price: f64 = a.get("price")?.as_str()?.parse().ok()?;
            let size: f64 = a.get("size")?.as_str()?.parse().ok()?;
            if size > 0.0 { Some(price) } else { None }
        })
        .fold(f64::MAX, f64::min);

    let bid_depth: f64 = bids
        .iter()
        .filter_map(|b| b.get("size")?.as_str()?.parse::<f64>().ok())
        .sum();

    let ask_depth: f64 = asks
        .iter()
        .filter_map(|a| a.get("size")?.as_str()?.parse::<f64>().ok())
        .sum();

    let mid = if best_bid > 0.0 && best_ask < f64::MAX {
        (best_bid + best_ask) / 2.0
    } else {
        0.0
    };

    Some(TokenBook {
        best_bid,
        best_ask,
        mid,
        bid_depth,
        ask_depth,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    })
}

fn parse_best_bid_ask(v: &serde_json::Value) -> Option<(f64, f64)> {
    let bid: f64 = v.get("best_bid")?.as_str()?.parse().ok()?;
    let ask: f64 = v.get("best_ask")?.as_str()?.parse().ok()?;
    Some((bid, ask))
}
