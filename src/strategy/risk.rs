use std::time::{SystemTime, UNIX_EPOCH};

use tracing::warn;

use crate::config::Config;

pub struct RiskManager {
    bankroll: f64,
    max_position_pct: f64,
    max_daily_loss_pct: f64,
    max_open_positions: usize,

    daily_pnl: f64,
    open_positions: usize,
    day_start_epoch: u64,
    killed: bool,
}

impl RiskManager {
    pub fn new(cfg: &Config) -> Self {
        Self {
            bankroll: 100.0, // Updated when we query balance
            max_position_pct: cfg.strategy.max_position_pct,
            max_daily_loss_pct: cfg.strategy.max_daily_loss_pct,
            max_open_positions: cfg.strategy.max_open_positions,
            daily_pnl: 0.0,
            open_positions: 0,
            day_start_epoch: current_day_epoch(),
            killed: false,
        }
    }

    pub fn can_trade(&mut self) -> bool {
        self.maybe_reset_day();

        if self.killed {
            return false;
        }

        if self.open_positions >= self.max_open_positions {
            warn!(
                open = self.open_positions,
                max = self.max_open_positions,
                "max positions reached"
            );
            return false;
        }

        let max_loss = self.bankroll * self.max_daily_loss_pct;
        if self.daily_pnl < -max_loss {
            warn!(
                daily_pnl = %format!("{:.2}", self.daily_pnl),
                cap = %format!("{:.2}", -max_loss),
                "daily loss cap hit, killing trading"
            );
            self.killed = true;
            return false;
        }

        true
    }

    pub fn position_size(&self, edge_pct: f64, _price: f64) -> f64 {
        let base = self.bankroll * self.max_position_pct;
        // Scale up slightly for larger edges (Kelly-lite)
        let edge_mult = (edge_pct / 0.003).min(3.0);
        let size = base * edge_mult;
        // Don't buy more than what's reasonable at this price
        size.min(self.bankroll * 0.02)
    }

    pub fn record_fill(&mut self, _size_usd: f64) {
        self.open_positions += 1;
    }

    pub fn record_close(&mut self, pnl: f64) {
        self.open_positions = self.open_positions.saturating_sub(1);
        self.daily_pnl += pnl;
    }

    pub fn update_bankroll(&mut self, balance: f64) {
        self.bankroll = balance;
    }

    fn maybe_reset_day(&mut self) {
        let today = current_day_epoch();
        if today != self.day_start_epoch {
            self.daily_pnl = 0.0;
            self.killed = false;
            self.day_start_epoch = today;
        }
    }
}

fn current_day_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 86400
}
