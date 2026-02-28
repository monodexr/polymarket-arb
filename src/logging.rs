use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::feeds::aggregator::PriceState;
use crate::strategy::divergence::DivEvent;

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
    mut event_rx: mpsc::Receiver<DivEvent>,
    price_rx: watch::Receiver<PriceState>,
) {
    tokio::spawn(async move {
        let start = Instant::now();
        let mut total_signals = 0u64;
        let mut total_converged = 0u64;
        let mut edge_sum = 0.0f64;
        let mut durations_ms: Vec<u128> = Vec::new();
        let mut per_market: HashMap<String, u64> = HashMap::new();

        while let Some(event) = event_rx.recv().await {
            match event {
                DivEvent::Signal(signal) => {
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
                }
                DivEvent::Converged { market_name, duration_ms, peak_edge_pct } => {
                    total_converged += 1;
                    durations_ms.push(duration_ms);

                    info!(
                        event = "CONVERGED",
                        market = %market_name,
                        duration_ms,
                        peak_edge = %format!("{:.2}%", peak_edge_pct * 100.0),
                        dry_run = true,
                    );
                }
            }

            let total_events = total_signals + total_converged;
            if total_events % 50 == 0 && total_events > 0 {
                print_summary(
                    &start, total_signals, &durations_ms, edge_sum, &per_market,
                );
            }
        }

        warn!("event channel closed, logging final summary");
        print_summary(
            &start, total_signals, &durations_ms, edge_sum, &per_market,
        );
    });
}

fn print_summary(
    start: &Instant,
    total_signals: u64,
    durations_ms: &[u128],
    edge_sum: f64,
    per_market: &HashMap<String, u64>,
) {
    let elapsed_hrs = start.elapsed().as_secs_f64() / 3600.0;
    let rate = total_signals as f64 / elapsed_hrs.max(0.001);
    let avg_edge = if total_signals > 0 {
        edge_sum / total_signals as f64
    } else {
        0.0
    };

    let median_duration_ms = if !durations_ms.is_empty() {
        let mut sorted = durations_ms.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    } else {
        0
    };

    let pct_under_100ms = if !durations_ms.is_empty() {
        durations_ms.iter().filter(|&&d| d < 100).count() as f64
            / durations_ms.len() as f64
            * 100.0
    } else {
        0.0
    };

    let pct_over_1s = if !durations_ms.is_empty() {
        durations_ms.iter().filter(|&&d| d > 1000).count() as f64
            / durations_ms.len() as f64
            * 100.0
    } else {
        0.0
    };

    info!(
        event = "SUMMARY",
        total_signals,
        total_converged = durations_ms.len(),
        signals_per_hour = %format!("{:.1}", rate),
        avg_edge_pct = %format!("{:.3}%", avg_edge * 100.0),
        median_duration_ms,
        pct_under_100ms = %format!("{:.1}%", pct_under_100ms),
        pct_over_1s = %format!("{:.1}%", pct_over_1s),
        unique_markets = per_market.len(),
        elapsed_hours = %format!("{:.2}", elapsed_hrs),
    );
}
