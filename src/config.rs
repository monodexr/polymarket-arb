use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub strategy: StrategyConfig,
    pub discovery: DiscoveryConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    #[serde(default)]
    pub seed_usd: f64,
    pub min_edge: f64,
    pub min_move_pct: f64,
    pub max_position_pct: f64,
    pub max_daily_loss_pct: f64,
    pub max_open_positions: usize,
    pub order_timeout_secs: u64,
    pub stale_price_secs: u64,
    pub late_window_guard_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveryConfig {
    pub assets: Vec<String>,
    pub window_duration_secs: u64,
    pub pre_discover_secs: u64,
    pub gamma_url: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config from {path}"))?;
        toml::from_str(&text).with_context(|| "parsing config TOML")
    }

    pub fn private_key(&self) -> Result<String> {
        std::env::var("POLYMARKET_PRIVATE_KEY")
            .with_context(|| "POLYMARKET_PRIVATE_KEY env var not set")
    }

    pub fn proxy_wallet(&self) -> Result<String> {
        std::env::var("POLYMARKET_PROXY_WALLET")
            .with_context(|| "POLYMARKET_PROXY_WALLET env var not set")
    }
}
