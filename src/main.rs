mod config;
mod data;
mod executor;
mod feeds;
mod logging;
mod markets;
mod strategy;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use clap::Parser;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::data::WindowStatus;
use crate::feeds::aggregator::PriceState;
use crate::markets::book::{BookRx, TokenSubTx};
use crate::markets::discovery;
use crate::markets::fair_value;
use crate::strategy::divergence::{self, DivEvent};
use crate::strategy::risk::RiskManager;

#[derive(Parser)]
#[command(name = "polymarket-arb", about = "Polymarket 5-min market divergence bot")]
struct Cli {
    /// Log signals without placing orders
    #[arg(long)]
    dry_run: bool,

    /// Path to config file
    #[arg(long, default_value = "config.toml")]
    config: String,
}

/// Shared window status for the status writer (M3 fix).
type SharedWindowStates = Arc<Mutex<Vec<WindowStatus>>>;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    logging::init();
    data::ensure_data_dir();

    if cli.dry_run {
        info!("DRY RUN — signals will be logged but no orders placed");
    }

    let seed_usd = cfg.strategy.seed_usd;
    info!(seed_usd = %format!("${:.2}", seed_usd), "PnL tracking seed configured");

    let (price_tx, price_rx) = watch::channel(PriceState::default());
    let (event_tx, event_rx) = mpsc::channel::<DivEvent>(256);

    feeds::spawn_all(price_tx, &cfg);

    let (book_rx, token_sub_tx) = markets::book::spawn();

    let window_states: SharedWindowStates = Arc::new(Mutex::new(Vec::new()));
    let live_stats = data::new_shared_live_stats();

    for asset in &cfg.discovery.assets {
        let asset = asset.clone();
        let cfg = cfg.clone();
        let price_rx = price_rx.clone();
        let book_rx = book_rx.clone();
        let token_sub_tx = token_sub_tx.clone();
        let event_tx = event_tx.clone();
        let window_states = window_states.clone();

        tokio::spawn(async move {
            run_asset_loop(asset, cfg, price_rx, book_rx, token_sub_tx, event_tx, window_states).await;
        });
    }

    drop(token_sub_tx);

    if cli.dry_run {
        logging::spawn_dry_run_logger(event_rx);
    } else {
        executor::spawn(event_rx, cfg.clone(), live_stats.clone()).await?;
    }

    // Status writer task
    let status_price_rx = price_rx.clone();
    let status_windows = window_states.clone();
    let status_live = live_stats.clone();
    let status_cfg = cfg.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let ps = status_price_rx.borrow();
            let btc_price = ps.spot_price("btc");
            let windows = {
                let guard = status_windows.lock().unwrap();
                guard.clone()
            };

            let (balance, trade_stats) = {
                let stats = status_live.lock().unwrap();
                let seed = status_cfg.strategy.seed_usd;
                let bal = stats.balance;
                let ts = data::TradeStats {
                    wins: stats.wins,
                    losses: stats.losses,
                    open: stats.open,
                    total_pnl: if seed > 0.0 && bal > 0.0 { bal - seed } else { stats.total_pnl },
                    session_pnl: if stats.session_start_balance > 0.0 {
                        bal - stats.session_start_balance
                    } else {
                        stats.session_pnl
                    },
                    daily_pnl: stats.daily_pnl,
                };
                (bal, ts)
            };

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let feed_latency = ps.prices.get("btc")
                .map(|p| now_ms.saturating_sub(p.timestamp_ms))
                .unwrap_or(0);

            let status = data::Status {
                timestamp: discovery::now_secs(),
                balance,
                seed: status_cfg.strategy.seed_usd,
                spot_price: btc_price,
                spot_source: "binance",
                current_windows: windows,
                feeds: data::FeedStatus {
                    binance_connected: !ps.prices.is_empty(),
                    binance_price: btc_price,
                    binance_latency_ms: feed_latency,
                },
                trades: trade_stats,
                recent_trades: load_recent_trades(50),
            };
            data::write_status(&status);
        }
    });

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}

fn load_recent_trades(n: usize) -> Vec<serde_json::Value> {
    let path = std::path::Path::new("data/trades.jsonl");
    if !path.exists() {
        return vec![];
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return vec![],
    };
    let lines: Vec<&str> = text.lines().collect();
    lines.iter().rev().take(n).filter_map(|line| {
        serde_json::from_str(line).ok()
    }).collect()
}

async fn run_asset_loop(
    asset: String,
    cfg: Config,
    price_rx: watch::Receiver<PriceState>,
    book_rx: BookRx,
    token_sub_tx: TokenSubTx,
    event_tx: mpsc::Sender<DivEvent>,
    window_states: SharedWindowStates,
) {
    let dur = cfg.discovery.window_duration_secs;
    let pre_discover = cfg.discovery.pre_discover_secs;

    let mut risk = RiskManager::new(&cfg);

    info!(asset = %asset, "asset lifecycle started");

    loop {
        if data::is_paused() {
            info!(asset = %asset, "PAUSED — skipping window");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        let now = discovery::now_secs();
        let next_start = discovery::next_window_start(dur);
        let discover_at = next_start as f64 - pre_discover as f64;

        let sleep_secs = (discover_at - now).max(0.0);
        if sleep_secs > 0.0 {
            update_window_state(&window_states, &asset, None);

            info!(
                asset = %asset,
                sleep_secs = %format!("{:.0}", sleep_secs),
                next_window = next_start,
                "waiting for next discovery window"
            );
            tokio::time::sleep(std::time::Duration::from_secs_f64(sleep_secs)).await;
        }

        let window = match discovery::discover_window(&asset, next_start, &cfg.discovery).await {
            Some(w) => w,
            None => {
                data::alert("WARNING", "arb.discovery_fail",
                    &format!("Failed to discover {} window {}", asset, next_start),
                    serde_json::json!({"asset": asset, "window_start": next_start}));
                continue;
            }
        };

        let tokens = vec![window.yes_token.clone(), window.no_token.clone()];
        if token_sub_tx.send(tokens).await.is_err() {
            error!(asset = %asset, "token subscription channel closed");
            break;
        }

        let book_wait_deadline = discovery::now_secs() + 10.0;
        loop {
            let has_data = {
                let books = book_rx.borrow();
                books.contains_key(&window.yes_token)
            };
            if has_data {
                break;
            }
            if discovery::now_secs() > book_wait_deadline {
                warn!(asset = %asset, "no book data after 10s, proceeding anyway");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let wait_for_open = (window.open_time - discovery::now_secs()).max(0.0);
        if wait_for_open > 0.0 {
            tokio::time::sleep(std::time::Duration::from_secs_f64(wait_for_open)).await;
        }

        let open_price = price_rx.borrow().spot_price(&asset);
        if open_price <= 0.0 {
            warn!(asset = %asset, "no spot price at window open, skipping");
            continue;
        }

        let mut window = window;
        window.open_price = open_price;

        info!(
            asset = %asset,
            slug = %window.slug,
            open_price = %format!("{:.2}", open_price),
            "window opened"
        );

        data::alert("INFO", "arb.window_open",
            &format!("{} window opened: {} @ ${:.2}", asset.to_uppercase(), window.slug, open_price),
            serde_json::json!({
                "asset": asset, "slug": window.slug,
                "open_price": open_price,
            }));

        let mut window_div_state: HashMap<String, strategy::divergence::OpenDivergence> = HashMap::new();
        let windows = vec![window.clone()];

        let mut price_watch = price_rx.clone();
        let mut book_watch = book_rx.clone();

        loop {
            if window.is_expired() {
                break;
            }

            tokio::select! {
                res = price_watch.changed() => {
                    if res.is_err() { break; }
                }
                res = book_watch.changed() => {
                    if res.is_err() { break; }
                }
            }

            let spot = price_watch.borrow().spot_price(&asset);
            let books = book_watch.borrow().clone();

            let events = divergence::evaluate(
                &windows, spot, &books, &cfg.strategy, &mut window_div_state,
            );

            let events: Vec<DivEvent> = events
                .into_iter()
                .filter_map(|ev| match ev {
                    DivEvent::Signal(mut sig) => {
                        if !risk.can_trade() {
                            return None;
                        }
                        sig.size_usd = risk.position_size(sig.edge, sig.price);
                        risk.record_fill(sig.size_usd);
                        Some(DivEvent::Signal(sig))
                    }
                    other => Some(other),
                })
                .collect();

            let fv_yes = fair_value::fair_yes(spot, window.open_price, window.time_remaining_frac());
            let yes_mid = books.get(&window.yes_token).map(|b| b.mid).unwrap_or(0.0);
            let no_mid = books.get(&window.no_token).map(|b| b.mid).unwrap_or(0.0);
            let move_pct = if open_price > 0.0 { (spot - open_price) / open_price } else { 0.0 };

            update_window_state(&window_states, &asset, Some(WindowStatus {
                slug: window.slug.clone(),
                asset: asset.clone(),
                open_price,
                current_move_pct: move_pct * 100.0,
                time_remaining_sec: window.time_remaining(),
                fair_yes: fv_yes,
                fair_no: 1.0 - fv_yes,
                clob_yes_mid: yes_mid,
                clob_no_mid: no_mid,
                edge_yes: fv_yes - yes_mid,
                edge_no: (1.0 - fv_yes) - no_mid,
                divergence_open: !window_div_state.is_empty(),
                state: if window_div_state.is_empty() { "monitoring".to_string() } else { "divergence".to_string() },
            }));

            for ev in events {
                let _ = event_tx.send(ev).await;
            }
        }

        let move_pct = (price_rx.borrow().spot_price(&asset) - open_price) / open_price;
        info!(
            asset = %asset,
            slug = %window.slug,
            final_move_pct = %format!("{:.3}%", move_pct * 100.0),
            "window closed"
        );

        data::alert("INFO", "arb.window_close",
            &format!("{} window closed: {:.3}% move", asset.to_uppercase(), move_pct * 100.0),
            serde_json::json!({
                "asset": asset, "slug": window.slug,
                "move_pct": move_pct * 100.0,
            }));

        update_window_state(&window_states, &asset, None);
    }
}

fn update_window_state(
    states: &SharedWindowStates,
    asset: &str,
    new_state: Option<WindowStatus>,
) {
    let mut guard = states.lock().unwrap();
    guard.retain(|ws| ws.asset != asset);
    if let Some(ws) = new_state {
        guard.push(ws);
    }
}
