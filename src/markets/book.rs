use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Default)]
pub struct TokenBook {
    pub best_bid: f64,
    pub best_ask: f64,
    pub mid: f64,
    pub timestamp_ms: u64,
}

pub type BookSnapshot = HashMap<String, TokenBook>;
pub type BookRx = watch::Receiver<BookSnapshot>;

/// Channel for sending new token IDs to subscribe to (per window cycle).
pub type TokenSubTx = mpsc::Sender<Vec<String>>;
pub type TokenSubRx = mpsc::Receiver<Vec<String>>;

pub fn spawn() -> (BookRx, TokenSubTx) {
    let (book_tx, book_rx) = watch::channel(BookSnapshot::new());
    let (token_tx, token_rx) = mpsc::channel::<Vec<String>>(16);

    tokio::spawn(async move {
        run_loop(book_tx, token_rx).await;
    });

    (book_rx, token_tx)
}

async fn run_loop(book_tx: watch::Sender<BookSnapshot>, mut token_rx: TokenSubRx) {
    let mut current_tokens: HashSet<String> = HashSet::new();

    loop {
        // Wait for at least one token subscription
        let first = match token_rx.recv().await {
            Some(t) => t,
            None => break,
        };

        // Drain all pending subscriptions (other assets may have queued theirs
        // within milliseconds of the first). Wait 500ms for stragglers.
        let mut all_new: HashSet<String> = first.into_iter().collect();
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                token_rx.recv(),
            )
            .await
            {
                Ok(Some(more)) => {
                    all_new.extend(more);
                }
                _ => break,
            }
        }

        // Merge with existing tokens (keep subscriptions from other active windows)
        current_tokens.extend(all_new);

        if current_tokens.is_empty() {
            continue;
        }

        let ids: Vec<String> = current_tokens.iter().cloned().collect();
        info!(tokens = ids.len(), "subscribing to CLOB WS");

        match run_ws(&ids, &book_tx).await {
            Ok(()) => warn!("CLOB WS closed, will resubscribe on next window"),
            Err(e) => error!(%e, "CLOB WS error"),
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn run_ws(
    token_ids: &[String],
    book_tx: &watch::Sender<BookSnapshot>,
) -> anyhow::Result<()> {
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

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            process_clob_message(&text, book_tx);
        }
    }

    Ok(())
}

fn process_clob_message(json: &str, book_tx: &watch::Sender<BookSnapshot>) {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event_type = v.get("event_type").and_then(|e| e.as_str()).unwrap_or("");
    let asset_id = v.get("asset_id").and_then(|a| a.as_str()).unwrap_or("");

    if asset_id.is_empty() {
        return;
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    match event_type {
        "book" | "price_change" => {
            if let Some(book) = parse_book_update(&v, now_ms) {
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

fn parse_book_update(v: &serde_json::Value, now_ms: u64) -> Option<TokenBook> {
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

    let mid = if best_bid > 0.0 && best_ask < f64::MAX {
        (best_bid + best_ask) / 2.0
    } else {
        0.0
    };

    Some(TokenBook {
        best_bid,
        best_ask,
        mid,
        timestamp_ms: now_ms,
    })
}

fn parse_best_bid_ask(v: &serde_json::Value) -> Option<(f64, f64)> {
    let bid: f64 = v.get("best_bid")?.as_str()?.parse().ok()?;
    let ask: f64 = v.get("best_ask")?.as_str()?.parse().ok()?;
    Some((bid, ask))
}
