use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::error;


const DATA_DIR: &str = "data";
const ALERTS_MAX_LINES: usize = 10_000;

pub fn ensure_data_dir() {
    std::fs::create_dir_all(DATA_DIR).ok();
}

pub fn is_paused() -> bool {
    Path::new("data/pause.flag").exists()
}

// --- status.json ---

#[derive(Serialize)]
pub struct Status {
    pub timestamp: f64,
    pub spot_price: f64,
    pub spot_source: &'static str,
    pub current_windows: Vec<WindowStatus>,
    pub feeds: FeedStatus,
    pub trades: TradeStats,
    pub recent_trades: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct WindowStatus {
    pub slug: String,
    pub asset: String,
    pub open_price: f64,
    pub current_move_pct: f64,
    pub time_remaining_sec: f64,
    pub fair_yes: f64,
    pub fair_no: f64,
    pub clob_yes_mid: f64,
    pub clob_no_mid: f64,
    pub edge_yes: f64,
    pub edge_no: f64,
    pub divergence_open: bool,
    pub state: String,
}

#[derive(Serialize, Default)]
pub struct FeedStatus {
    pub binance_connected: bool,
    pub binance_price: f64,
    pub binance_latency_ms: u64,
}

#[derive(Serialize, Default)]
pub struct TradeStats {
    pub wins: u64,
    pub losses: u64,
    pub open: u64,
    pub total_pnl: f64,
    pub session_pnl: f64,
    pub daily_pnl: f64,
}

pub fn write_status(status: &Status) {
    let json = match serde_json::to_string(status) {
        Ok(j) => j,
        Err(e) => {
            error!(%e, "failed to serialize status");
            return;
        }
    };
    if let Err(e) = std::fs::write("data/status.json", json) {
        error!(%e, "failed to write status.json");
    }
}

// --- alerts.jsonl ---

#[derive(Serialize)]
struct Alert {
    timestamp: f64,
    severity: String,
    category: String,
    message: String,
    data: serde_json::Value,
}

static LAST_ALERT: std::sync::Mutex<Option<HashMap<String, f64>>> = std::sync::Mutex::new(None);

pub fn alert(severity: &str, category: &str, message: &str, data: serde_json::Value) {
    let now = now_f64();

    // Rate limit: max 1 per category per 10 seconds
    {
        let mut guard = LAST_ALERT.lock().unwrap();
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(&last) = map.get(category) {
            if now - last < 10.0 {
                return;
            }
        }
        map.insert(category.to_string(), now);
    }

    let entry = Alert {
        timestamp: now,
        severity: severity.to_string(),
        category: category.to_string(),
        message: message.to_string(),
        data,
    };

    let line = match serde_json::to_string(&entry) {
        Ok(j) => j,
        Err(_) => return,
    };

    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("data/alerts.jsonl")
    {
        let _ = writeln!(f, "{}", line);
    }
}

// --- simulated_trades.jsonl ---

#[derive(Serialize)]
pub struct SimulatedTrade {
    pub timestamp: f64,
    pub market: String,
    pub asset: String,
    pub side: String,
    pub fair_value: f64,
    pub clob_mid: f64,
    pub edge: f64,
    pub move_pct: f64,
    pub simulated_pnl: f64,
    pub duration_sec: f64,
    pub outcome: String,
}

pub fn write_simulated_trade(trade: &SimulatedTrade) {
    let line = match serde_json::to_string(trade) {
        Ok(j) => j,
        Err(_) => return,
    };

    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("data/simulated_trades.jsonl")
    {
        let _ = writeln!(f, "{}", line);
    }
}

fn now_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
