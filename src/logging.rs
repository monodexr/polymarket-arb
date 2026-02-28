use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::config::Config;
use crate::feeds::aggregator::PriceState;
use crate::markets::book::BookState;
use crate::markets::discovery::MarketState;
use crate::strategy::divergence::Signal;

pub fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();
}

pub fn spawn_dry_run_logger(
    mut signal_rx: mpsc::Receiver<Signal>,
    price_rx: watch::Receiver<PriceState>,
    _book_state: BookState,
    _market_state: MarketState,
    _cfg: Config,
) {
    tokio::spawn(async move {
        let start = Instant::now();
        let mut total_signals = 0u64;
        let mut edge_sum = 0.0f64;
        let _durations: Vec<f64> = Vec::new();
        let mut per_market: HashMap<String, u64> = HashMap::new();

        while let Some(signal) = signal_rx.recv().await {
            total_signals += 1;
            edge_sum += signal.edge_pct.abs();

            *per_market.entry(signal.market_name.clone()).or_default() += 1;

            let spot = price_rx.borrow().spot_price;

            info!(
                event = "SIGNAL",
                market = %signal.market_name,
                side = %signal.side,
                fair = %format!("{:.4}", signal.fair_value),
                clob_mid = %format!("{:.4}", signal.clob_mid),
                edge_pct = %format!("{:.2}%", signal.edge_pct * 100.0),
                spot = %format!("{:.2}", spot),
                dry_run = true,
            );

            if total_signals % 50 == 0 {
                let elapsed = start.elapsed().as_secs_f64() / 3600.0;
                let rate = total_signals as f64 / elapsed.max(0.001);
                let avg_edge = edge_sum / total_signals as f64;
                info!(
                    event = "SUMMARY",
                    total_signals,
                    signals_per_hour = %format!("{:.1}", rate),
                    avg_edge_pct = %format!("{:.3}%", avg_edge * 100.0),
                    unique_markets = per_market.len(),
                );
            }
        }

        warn!("signal channel closed, logging final summary");
        let elapsed_hrs = start.elapsed().as_secs_f64() / 3600.0;
        info!(
            event = "FINAL_SUMMARY",
            total_signals,
            elapsed_hours = %format!("{:.2}", elapsed_hrs),
            avg_edge_pct = %format!("{:.3}%", if total_signals > 0 { edge_sum / total_signals as f64 * 100.0 } else { 0.0 }),
            unique_markets = per_market.len(),
        );
    });
}
