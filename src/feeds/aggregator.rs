use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use super::PriceTick;

/// Per-asset price state. Keys are asset names ("btc", "eth", "sol", "xrp").
#[derive(Debug, Clone, Default)]
pub struct PriceState {
    pub prices: HashMap<String, AssetPrice>,
}

#[derive(Debug, Clone)]
pub struct AssetPrice {
    pub price: f64,
    pub timestamp_ms: u64,
}

impl PriceState {
    pub fn spot_price(&self, asset: &str) -> f64 {
        self.prices.get(asset).map(|p| p.price).unwrap_or(0.0)
    }
}

pub fn spawn(
    mut tick_rx: mpsc::Receiver<PriceTick>,
    price_tx: watch::Sender<PriceState>,
    stale_secs: u64,
) {
    tokio::spawn(async move {
        let mut first_tick = true;

        while let Some(tick) = tick_rx.recv().await {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let stale_ms = stale_secs * 1000;

            if now_ms.saturating_sub(tick.timestamp_ms) > stale_ms {
                warn!(source = tick.source, age_ms = now_ms - tick.timestamp_ms, "stale tick, ignoring");
                continue;
            }

            if first_tick {
                info!(source = tick.source, price = %format!("{:.2}", tick.price), "first price tick received");
                first_tick = false;
            }

            price_tx.send_modify(|state| {
                state.prices.insert(
                    tick.source.to_string(),
                    AssetPrice {
                        price: tick.price,
                        timestamp_ms: tick.timestamp_ms,
                    },
                );
            });
        }
    });
}
