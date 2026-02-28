use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, watch};
use tracing::{debug, warn};

use super::PriceTick;

#[derive(Debug, Clone, Default)]
pub struct PriceState {
    pub spot_price: f64,
    pub timestamp_ms: u64,
}

pub fn spawn(
    mut tick_rx: mpsc::Receiver<PriceTick>,
    price_tx: watch::Sender<PriceState>,
    stale_secs: u64,
) {
    tokio::spawn(async move {
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

            let state = PriceState {
                spot_price: tick.price,
                timestamp_ms: tick.timestamp_ms,
            };

            debug!(spot = %format!("{:.2}", tick.price), "price update");
            let _ = price_tx.send(state);
        }
    });
}
