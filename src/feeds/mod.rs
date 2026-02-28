pub mod aggregator;
pub mod binance;
pub mod coinbase;
pub mod deribit;
pub mod kraken;
pub mod okx;

use tokio::sync::{mpsc, watch};
use tracing::info;

use crate::config::Config;
use aggregator::PriceState;

#[derive(Debug, Clone)]
pub struct PriceTick {
    pub source: &'static str,
    pub price: f64,
    pub timestamp_ms: u64,
}

pub fn spawn_all(price_tx: watch::Sender<PriceState>, cfg: &Config) {
    let (tick_tx, tick_rx) = mpsc::channel::<PriceTick>(4096);

    aggregator::spawn(tick_rx, price_tx, cfg.strategy.stale_price_secs);

    binance::spawn(tick_tx.clone());
    coinbase::spawn(tick_tx.clone());
    kraken::spawn(tick_tx.clone());
    okx::spawn(tick_tx.clone());
    deribit::spawn(tick_tx);

    info!("all price feeds spawned");
}
