use std::fmt;
use std::time::Instant;

use tracing::info;

use crate::config::StrategyConfig;
use crate::markets::book::BookSnapshot;
use crate::markets::discovery::Window;
use crate::markets::fair_value;

#[derive(Debug, Clone)]
pub enum Side {
    BuyYes,
    BuyNo,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::BuyYes => write!(f, "BUY_YES"),
            Side::BuyNo => write!(f, "BUY_NO"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DivEvent {
    Signal(Signal),
    Converged {
        market_name: String,
        duration_ms: u128,
        peak_edge: f64,
    },
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub market_name: String,
    pub asset: String,
    pub condition_id: String,
    pub token_id: String,
    pub side: Side,
    pub fair_value: f64,
    pub clob_mid: f64,
    pub edge: f64,
    pub price: f64,
    pub size_usd: f64,
    pub move_pct: f64,
    pub time_remaining_frac: f64,
}

pub struct OpenDivergence {
    opened_at: Instant,
    peak_edge: f64,
}

/// Evaluate all active windows for divergences. Called on every price or book update.
/// Returns events to send downstream.
pub fn evaluate(
    windows: &[Window],
    spot: f64,
    books: &BookSnapshot,
    cfg: &StrategyConfig,
    open_divs: &mut std::collections::HashMap<String, OpenDivergence>,
) -> Vec<DivEvent> {
    let mut events = Vec::new();
    let _now = crate::markets::discovery::now_secs();

    for window in windows {
        if !window.is_active() || window.open_price <= 0.0 {
            continue;
        }

        // Late-window guard: no signals in last N seconds
        if window.time_remaining() < cfg.late_window_guard_secs as f64 {
            continue;
        }

        let move_pct = (spot - window.open_price) / window.open_price;
        if move_pct.abs() < cfg.min_move_pct {
            // Close any open divergences when move drops back to noise
            if let Some(div) = open_divs.remove(&window.slug) {
                events.push(DivEvent::Converged {
                    market_name: window.slug.clone(),
                    duration_ms: div.opened_at.elapsed().as_millis(),
                    peak_edge: div.peak_edge,
                });
            }
            continue;
        }

        let time_frac = window.time_remaining_frac();
        let fv_yes = fair_value::fair_yes(spot, window.open_price, time_frac);
        let fv_no = fair_value::fair_no(spot, window.open_price, time_frac);

        let yes_book = books.get(&window.yes_token);
        let no_book = books.get(&window.no_token);

        let yes_mid = yes_book.map(|b| b.mid).unwrap_or(0.0);
        let no_mid = no_book.map(|b| b.mid).unwrap_or(0.0);

        if yes_mid <= 0.0 && no_mid <= 0.0 {
            continue;
        }

        // Check YES side edge
        let yes_edge = fv_yes - yes_mid;
        // Check NO side edge
        let no_edge = fv_no - no_mid;

        let (edge, side, token_id, price, fair, clob_mid) = if yes_edge > no_edge && yes_edge > cfg.min_edge {
            let ask = yes_book.map(|b| b.best_ask).unwrap_or(0.0);
            (yes_edge, Side::BuyYes, window.yes_token.clone(), ask, fv_yes, yes_mid)
        } else if no_edge > cfg.min_edge {
            let ask = no_book.map(|b| b.best_ask).unwrap_or(0.0);
            (no_edge, Side::BuyNo, window.no_token.clone(), ask, fv_no, no_mid)
        } else {
            // No edge — check if an open divergence should close
            if let Some(div) = open_divs.remove(&window.slug) {
                events.push(DivEvent::Converged {
                    market_name: window.slug.clone(),
                    duration_ms: div.opened_at.elapsed().as_millis(),
                    peak_edge: div.peak_edge,
                });
            }
            continue;
        };

        if price <= 0.0 || price >= 1.0 {
            continue;
        }

        // Track divergence duration
        let div = open_divs.entry(window.slug.clone()).or_insert_with(|| OpenDivergence {
            opened_at: Instant::now(),
            peak_edge: 0.0,
        });
        div.peak_edge = div.peak_edge.max(edge);

        info!(
            event = "DIVERGENCE",
            market = %window.slug,
            asset = %window.asset,
            side = %side,
            fair = %format!("{:.4}", fair),
            clob = %format!("{:.4}", clob_mid),
            edge = %format!("${:.4}", edge),
            move_pct = %format!("{:.3}%", move_pct * 100.0),
            time_remaining = %format!("{:.0}s", window.time_remaining()),
        );

        events.push(DivEvent::Signal(Signal {
            market_name: window.slug.clone(),
            asset: window.asset.clone(),
            condition_id: window.condition_id.clone(),
            token_id,
            side,
            fair_value: fair,
            clob_mid,
            edge,
            price,
            size_usd: 0.0, // sized by risk manager
            move_pct,
            time_remaining_frac: time_frac,
        }));
    }

    events
}
