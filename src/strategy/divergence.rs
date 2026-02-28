use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use tokio::sync::{mpsc, watch};
use tracing::{debug, info};

use crate::config::Config;
use crate::feeds::aggregator::PriceState;
use crate::markets::book::{BookState, TokenBook};
use crate::markets::discovery::{Market, MarketState};
use crate::markets::fair_value;
use crate::strategy::risk::RiskManager;

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
pub struct Signal {
    pub market_name: String,
    pub condition_id: String,
    pub token_id: String,
    pub side: Side,
    pub fair_value: f64,
    pub clob_mid: f64,
    pub edge_pct: f64,
    pub price: f64,
    pub size_usd: f64,
}

struct OpenDivergence {
    opened_at: Instant,
    market_name: String,
    peak_edge: f64,
}

pub fn spawn(
    mut price_rx: watch::Receiver<PriceState>,
    book_state: BookState,
    market_state: MarketState,
    signal_tx: mpsc::Sender<Signal>,
    cfg: Config,
) {
    tokio::spawn(async move {
        let mut risk = RiskManager::new(&cfg);
        let mut open_divergences: HashMap<String, OpenDivergence> = HashMap::new();
        let min_edge = cfg.strategy.min_edge;
        let vol_default = cfg.pricing.default_vol;
        let rate = cfg.pricing.risk_free_rate;

        loop {
            if price_rx.changed().await.is_err() {
                break;
            }

            let price_state = price_rx.borrow().clone();
            if price_state.spot_price <= 0.0 {
                continue;
            }

            let spot = price_state.spot_price;
            let vol = price_state.implied_vol.unwrap_or(vol_default);

            let markets: Vec<Market> = {
                let guard = market_state.read().unwrap();
                guard.values().cloned().collect()
            };

            let books: HashMap<String, TokenBook> = {
                let guard = book_state.read().unwrap();
                guard.clone()
            };

            for market in &markets {
                let time_years = fair_value::time_to_expiry_years(market.expiry);
                let fv = fair_value::binary_fair_value(spot, market.strike, time_years, vol, rate);

                let yes_book = books.get(&market.yes_token);
                let no_book = books.get(&market.no_token);

                let clob_mid = match yes_book {
                    Some(b) if b.mid > 0.0 => b.mid,
                    _ => continue,
                };

                // Cross-check: YES + NO should be close to 1.0
                let no_mid = no_book.map(|b| b.mid).unwrap_or(0.0);
                let pair_sum = clob_mid + no_mid;
                if pair_sum > 0.0 && (pair_sum < 0.90 || pair_sum > 1.10) {
                    debug!(
                        market = %market.title,
                        pair_sum = %format!("{:.3}", pair_sum),
                        "thin market (YES+NO far from 1.0), skipping"
                    );
                    continue;
                }

                let edge = fv - clob_mid;
                let edge_pct = if clob_mid > 0.0 {
                    edge / clob_mid
                } else {
                    continue;
                };

                let key = market.yes_token.clone();

                if edge_pct.abs() > min_edge {
                    // Track divergence duration
                    let div = open_divergences
                        .entry(key.clone())
                        .or_insert_with(|| OpenDivergence {
                            opened_at: Instant::now(),
                            market_name: market.title.clone(),
                            peak_edge: 0.0,
                        });
                    div.peak_edge = div.peak_edge.max(edge_pct.abs());

                    if !risk.can_trade() {
                        continue;
                    }

                    let (side, token_id, price) = if edge > 0.0 {
                        let ask = yes_book.map(|b| b.best_ask).unwrap_or(0.0);
                        (Side::BuyYes, market.yes_token.clone(), ask)
                    } else {
                        let no_ask = no_book
                            .map(|b| b.best_ask)
                            .unwrap_or(0.0);
                        (Side::BuyNo, market.no_token.clone(), no_ask)
                    };

                    if price <= 0.0 || price >= 1.0 {
                        continue;
                    }

                    let size_usd = risk.position_size(edge_pct.abs(), price);

                    let signal = Signal {
                        market_name: market.title.clone(),
                        condition_id: market.condition_id.clone(),
                        token_id,
                        side,
                        fair_value: fv,
                        clob_mid,
                        edge_pct,
                        price,
                        size_usd,
                    };

                    info!(
                        event = "DIVERGENCE",
                        market = %signal.market_name,
                        side = %signal.side,
                        fair = %format!("{:.4}", fv),
                        clob = %format!("{:.4}", clob_mid),
                        edge = %format!("{:.2}%", edge_pct * 100.0),
                        price = %format!("{:.4}", price),
                        size = %format!("${:.2}", size_usd),
                    );

                    let _ = signal_tx.send(signal).await;
                } else {
                    // Divergence closed — log duration
                    if let Some(div) = open_divergences.remove(&key) {
                        let duration = div.opened_at.elapsed();
                        info!(
                            event = "CONVERGED",
                            market = %div.market_name,
                            duration_ms = duration.as_millis(),
                            peak_edge = %format!("{:.2}%", div.peak_edge * 100.0),
                        );
                    }
                }
            }
        }
    });
}
