mod config;
mod executor;
mod feeds;
mod logging;
mod markets;
mod strategy;

use anyhow::Result;
use clap::Parser;
use tokio::sync::{mpsc, watch};
use tracing::info;

use crate::config::Config;
use crate::feeds::aggregator::PriceState;
use crate::strategy::divergence::DivEvent;

#[derive(Parser)]
#[command(name = "polymarket-arb", about = "Polymarket latency arbitrage bot")]
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

    if cli.dry_run {
        info!("DRY RUN — signals will be logged but no orders placed");
    }

    let (price_tx, price_rx) = watch::channel(PriceState::default());
    let (event_tx, event_rx) = mpsc::channel::<DivEvent>(256);

    let market_rx = markets::discovery::spawn(cfg.clone());
    let book_rx = markets::spawn_clob_ws(market_rx.clone());

    feeds::spawn_all(price_tx, &cfg);

    strategy::divergence::spawn(
        price_rx.clone(),
        book_rx,
        market_rx,
        event_tx,
        cfg.clone(),
    );

    if cli.dry_run {
        logging::spawn_dry_run_logger(event_rx, price_rx);
    } else {
        executor::spawn(event_rx, cfg.clone()).await?;
    }

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}
