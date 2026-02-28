use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, watch};
use tracing::{debug, warn};

use super::PriceTick;

#[derive(Debug, Clone, Default)]
pub struct PriceState {
    pub spot_price: f64,
    pub implied_vol: Option<f64>,
    pub feed_count: usize,
    pub timestamp_ms: u64,
}

struct FeedEntry {
    price: f64,
    timestamp_ms: u64,
}

pub fn spawn(
    mut tick_rx: mpsc::Receiver<PriceTick>,
    price_tx: watch::Sender<PriceState>,
    stale_secs: u64,
) {
    tokio::spawn(async move {
        let mut feeds: HashMap<&'static str, FeedEntry> = HashMap::new();
        let mut current_iv: Option<f64> = None;

        while let Some(tick) = tick_rx.recv().await {
            if tick.source == "deribit_iv" {
                current_iv = Some(tick.price);
                continue;
            }

            feeds.insert(
                tick.source,
                FeedEntry {
                    price: tick.price,
                    timestamp_ms: tick.timestamp_ms,
                },
            );

            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let stale_ms = stale_secs * 1000;

            let mut active_prices: Vec<f64> = feeds
                .iter()
                .filter(|(_, e)| now_ms.saturating_sub(e.timestamp_ms) < stale_ms)
                .map(|(_, e)| e.price)
                .collect();

            if active_prices.is_empty() {
                warn!("no active price feeds");
                continue;
            }

            active_prices.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            let median = if active_prices.len() % 2 == 0 {
                let mid = active_prices.len() / 2;
                (active_prices[mid - 1] + active_prices[mid]) / 2.0
            } else {
                active_prices[active_prices.len() / 2]
            };

            let state = PriceState {
                spot_price: median,
                implied_vol: current_iv,
                feed_count: active_prices.len(),
                timestamp_ms: now_ms,
            };

            debug!(
                spot = %format!("{:.2}", median),
                feeds = active_prices.len(),
                iv = ?current_iv,
                "price update"
            );

            let _ = price_tx.send(state);
        }
    });
}
