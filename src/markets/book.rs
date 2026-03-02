use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
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

pub type TokenSubTx = mpsc::Sender<Vec<String>>;
pub type TokenSubRx = mpsc::Receiver<Vec<String>>;

const CLOB_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const CLOB_WS_HOST: &str = "ws-subscriptions-clob.polymarket.com:443";

pub fn spawn() -> (BookRx, TokenSubTx) {
    let (book_tx, book_rx) = watch::channel(BookSnapshot::new());
    let (token_tx, token_rx) = mpsc::channel::<Vec<String>>(16);

    tokio::spawn(async move {
        run_loop(book_tx, token_rx).await;
    });

    (book_rx, token_tx)
}

async fn run_loop(book_tx: watch::Sender<BookSnapshot>, mut token_rx: TokenSubRx) {
    let mut subscribed_tokens: HashSet<String> = HashSet::new();

    loop {
        let ws = match connect_ws().await {
            Ok(ws) => ws,
            Err(e) => {
                error!(%e, "CLOB WS connection failed, retrying in 2s");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        };

        let (mut write, mut read) = ws.split();

        let write = Arc::new(tokio::sync::Mutex::new(write));
        let write_ping = write.clone();
        let ping_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let mut w = write_ping.lock().await;
                if w.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        });

        if !subscribed_tokens.is_empty() {
            let ids: Vec<String> = subscribed_tokens.iter().cloned().collect();
            if let Err(e) = subscribe(&write, &ids).await {
                error!(%e, "failed to resubscribe existing tokens");
            } else {
                info!(tokens = ids.len(), "resubscribed existing tokens on reconnect");
            }
        }

        let mut ws_alive = true;

        while ws_alive {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            process_clob_message(&text, &book_tx);
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            warn!(%e, "CLOB WS read error, reconnecting");
                            ws_alive = false;
                        }
                        None => {
                            warn!("CLOB WS closed, reconnecting");
                            ws_alive = false;
                        }
                    }
                }
                new_tokens = token_rx.recv() => {
                    match new_tokens {
                        Some(tokens) => {
                            let new: Vec<String> = tokens.into_iter()
                                .filter(|t| !subscribed_tokens.contains(t))
                                .collect();
                            if new.is_empty() {
                                continue;
                            }
                            info!(new_tokens = new.len(), total = subscribed_tokens.len() + new.len(), "subscribing new tokens");
                            if let Err(e) = subscribe(&write, &new).await {
                                warn!(%e, "failed to subscribe new tokens, reconnecting");
                                ws_alive = false;
                            } else {
                                subscribed_tokens.extend(new);
                            }
                        }
                        None => {
                            info!("token subscription channel closed");
                            return;
                        }
                    }
                }
            }
        }

        ping_task.abort();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn connect_ws() -> anyhow::Result<tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
>> {
    let tcp = tokio::net::TcpStream::connect(CLOB_WS_HOST).await?;
    tcp.set_nodelay(true)?;
    let (ws, _) = tokio_tungstenite::client_async_tls(CLOB_WS_URL, tcp).await?;
    info!("CLOB WS connected");
    Ok(ws)
}

type WsWrite = Arc<tokio::sync::Mutex<
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
        >,
        Message
    >
>>;

async fn subscribe(write: &WsWrite, token_ids: &[String]) -> anyhow::Result<()> {
    let sub_msg = serde_json::json!({
        "assets_ids": token_ids,
        "type": "market",
        "custom_feature_enabled": true
    });
    let mut w = write.lock().await;
    w.send(Message::Text(sub_msg.to_string().into())).await?;
    info!(tokens = token_ids.len(), "CLOB WS subscription sent");
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
