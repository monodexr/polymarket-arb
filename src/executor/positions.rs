use std::collections::HashMap;
use std::time::Instant;

use tracing::info;

use crate::strategy::divergence::Signal;

#[derive(Debug)]
struct OpenPosition {
    signal: Signal,
    opened_at: Instant,
    filled: bool,
}

pub struct PositionTracker {
    positions: HashMap<String, OpenPosition>,
}

impl PositionTracker {
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
        }
    }

    pub fn record_open(&mut self, signal: Signal) {
        let key = signal.token_id.clone();
        self.positions.insert(
            key,
            OpenPosition {
                signal,
                opened_at: Instant::now(),
                filled: false,
            },
        );
    }

    pub fn record_fill(&mut self, token_id: &str) {
        if let Some(pos) = self.positions.get_mut(token_id) {
            pos.filled = true;
            info!(
                event = "FILL",
                market = %pos.signal.market_name,
                price = %format!("{:.4}", pos.signal.price),
                size = %format!("${:.2}", pos.signal.size_usd),
                latency_ms = pos.opened_at.elapsed().as_millis(),
            );
        }
    }

    pub fn record_close(&mut self, token_id: &str, exit_price: f64) -> Option<f64> {
        if let Some(pos) = self.positions.remove(token_id) {
            let entry_price = pos.signal.price;
            let pnl_per_share = exit_price - entry_price;
            let shares = pos.signal.size_usd / entry_price;
            let pnl = pnl_per_share * shares;

            info!(
                event = "CLOSE",
                market = %pos.signal.market_name,
                entry = %format!("{:.4}", entry_price),
                exit = %format!("{:.4}", exit_price),
                pnl = %format!("${:.2}", pnl),
                hold_secs = pos.opened_at.elapsed().as_secs(),
            );
            Some(pnl)
        } else {
            None
        }
    }

    pub fn open_count(&self) -> usize {
        self.positions.len()
    }

    pub fn open_positions(&self) -> Vec<&Signal> {
        self.positions.values().map(|p| &p.signal).collect()
    }
}
