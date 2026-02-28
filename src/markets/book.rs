use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use futures_util::StreamExt;
use tracing::{error, info, warn};

use super::discovery::MarketState;

#[derive(Debug, Clone, Default)]
pub struct TokenBook {
    pub best_bid: f64,
    pub best_ask: f64,
    pub mid: f64,
    pub bid_depth: f64,
    pub ask_depth: f64,
    pub timestamp_ms: u64,
}

pub type BookState = Arc<RwLock<HashMap<String, TokenBook>>>;

pub fn spawn(market_state: MarketState) -> BookState {
    let state: BookState = Arc::new(RwLock::new(HashMap::new()));
    let state_clone = state.clone();

    tokio::spawn(async move {
        loop {
            match run_ws(&market_state, &state_clone).await {
                Ok(()) => warn!("CLOB WS closed, reconnecting"),
                Err(e) => error!(%e, "CLOB WS error, reconnecting in 2s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    state
}

async fn run_ws(market_state: &MarketState, book_state: &BookState) -> anyhow::Result<()> {
    // Collect all token IDs from discovered markets
    let token_ids: Vec<String> = {
        let guard = market_state.read().unwrap();
        let mut ids: Vec<String> = Vec::new();
        for market in guard.values() {
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

    // Ping keepalive every 10s
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

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            process_clob_message(&text, book_state);
        }
    }

    Ok(())
}

fn process_clob_message(json: &str, book_state: &BookState) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Handle different message types from the CLOB WS
    let event_type = v.get("event_type").and_then(|e| e.as_str()).unwrap_or("");
    let asset_id = v
        .get("asset_id")
        .and_then(|a| a.as_str())
        .unwrap_or("");

    if asset_id.is_empty() {
        return;
    }

    match event_type {
        "book" | "price_change" => {
            if let Some(book) = parse_book_update(&v) {
                let mut guard = book_state.write().unwrap();
                guard.insert(asset_id.to_string(), book);
            }
        }
        "best_bid_ask" => {
            if let Some((bid, ask)) = parse_best_bid_ask(&v) {
                let mut guard = book_state.write().unwrap();
                let entry = guard.entry(asset_id.to_string()).or_default();
                entry.best_bid = bid;
                entry.best_ask = ask;
                entry.mid = (bid + ask) / 2.0;
                entry.timestamp_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
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
