mod config;
mod data;
mod executor;
mod feeds;
mod logging;
mod markets;
mod strategy;

use std::collections::HashMap;

use anyhow::Result;
use clap::Parser;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::feeds::aggregator::PriceState;
use crate::markets::book::{BookRx, TokenSubTx};
use crate::markets::discovery::{self};
use crate::strategy::divergence::{self, DivEvent};

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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    logging::init();
    data::ensure_data_dir();

    if cli.dry_run {
        info!("DRY RUN — signals will be logged but no orders placed");
    }

    let (price_tx, price_rx) = watch::channel(PriceState::default());
    let (event_tx, event_rx) = mpsc::channel::<DivEvent>(256);

    feeds::spawn_all(price_tx, &cfg);

    let (book_rx, token_sub_tx) = markets::book::spawn();

    // Spawn one window lifecycle task per asset
    for asset in &cfg.discovery.assets {
        let asset = asset.clone();
        let cfg = cfg.clone();
        let price_rx = price_rx.clone();
        let book_rx = book_rx.clone();
        let token_sub_tx = token_sub_tx.clone();
        let event_tx = event_tx.clone();

        tokio::spawn(async move {
            run_asset_loop(asset, cfg, price_rx, book_rx, token_sub_tx, event_tx).await;
        });
    }

    // Drop our copy so the book handler can detect when all senders are gone
    drop(token_sub_tx);

    if cli.dry_run {
        logging::spawn_dry_run_logger(event_rx);
    } else {
        executor::spawn(event_rx, cfg.clone()).await?;
    }

    // Status writer task
    let status_price_rx = price_rx.clone();
    let _status_book_rx = book_rx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let ps = status_price_rx.borrow();
            let status = data::Status {
                timestamp: discovery::now_secs(),
                spot_price: ps.spot_price,
                spot_source: "binance",
                current_windows: vec![],
                feeds: data::FeedStatus {
                    binance_connected: ps.spot_price > 0.0,
                    binance_price: ps.spot_price,
                    binance_latency_ms: 0,
                },
                trades: data::TradeStats::default(),
                recent_trades: vec![],
            };
            data::write_status(&status);
        }
    });

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}

async fn run_asset_loop(
    asset: String,
    cfg: Config,
    price_rx: watch::Receiver<PriceState>,
    book_rx: BookRx,
    token_sub_tx: TokenSubTx,
    event_tx: mpsc::Sender<DivEvent>,
) {
    let dur = cfg.discovery.window_duration_secs;
    let pre_discover = cfg.discovery.pre_discover_secs;
    let _open_divs: HashMap<String, std::time::Instant> = HashMap::new();
    let _div_state: HashMap<String, strategy::divergence::OpenDivergence> = HashMap::new();

    info!(asset = %asset, "asset lifecycle started");

    loop {
        // Check pause
        if data::is_paused() {
            info!(asset = %asset, "PAUSED — skipping window");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        // Compute next window timing
        let now = discovery::now_secs();
        let _current_start = discovery::current_window_start(dur);
        let next_start = discovery::next_window_start(dur);
        let discover_at = next_start as f64 - pre_discover as f64;

        // Sleep until discovery time (or discover immediately if we're past it)
        let sleep_secs = (discover_at - now).max(0.0);
        if sleep_secs > 0.0 {
            info!(
                asset = %asset,
                sleep_secs = %format!("{:.0}", sleep_secs),
                next_window = next_start,
                "waiting for next discovery window"
            );
            tokio::time::sleep(std::time::Duration::from_secs_f64(sleep_secs)).await;
        }

        // Discover window
        let window = match discovery::discover_window(&asset, next_start, &cfg.discovery).await {
            Some(w) => w,
            None => {
                data::alert("WARNING", "arb.discovery_fail",
                    &format!("Failed to discover {} window {}", asset, next_start),
                    serde_json::json!({"asset": asset, "window_start": next_start}));
                continue;
            }
        };

        // Subscribe CLOB WS to this window's tokens
        let tokens = vec![window.yes_token.clone(), window.no_token.clone()];
        if token_sub_tx.send(tokens).await.is_err() {
            error!(asset = %asset, "token subscription channel closed");
            break;
        }

        // Wait for window to actually open
        let wait_for_open = (window.open_time - discovery::now_secs()).max(0.0);
        if wait_for_open > 0.0 {
            tokio::time::sleep(std::time::Duration::from_secs_f64(wait_for_open)).await;
        }

        // Capture opening price
        let open_price = price_rx.borrow().spot_price;
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

        // Clear per-window divergence state
        let mut window_div_state: HashMap<String, strategy::divergence::OpenDivergence> = HashMap::new();
        let windows = vec![window.clone()];

        // Monitor loop: select on price or book updates until window ends
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

            let spot = price_watch.borrow().spot_price;
            let books = book_watch.borrow().clone();

            let events = divergence::evaluate(
                &windows, spot, &books, &cfg.strategy, &mut window_div_state,
            );

            for ev in events {
                let _ = event_tx.send(ev).await;
            }
        }

        // Window ended — log summary
        let move_pct = (price_rx.borrow().spot_price - open_price) / open_price;
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
    }
}
