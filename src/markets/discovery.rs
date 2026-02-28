use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::config::DiscoveryConfig;

#[derive(Debug, Clone)]
pub struct Window {
    pub slug: String,
    pub asset: String,
    pub condition_id: String,
    pub yes_token: String,
    pub no_token: String,
    pub open_time: f64,
    pub close_time: f64,
    pub open_price: f64,
}

impl Window {
    pub fn time_remaining(&self) -> f64 {
        let now = now_secs();
        (self.close_time - now).max(0.0)
    }

    pub fn time_remaining_frac(&self) -> f64 {
        let duration = self.close_time - self.open_time;
        if duration <= 0.0 {
            return 0.0;
        }
        self.time_remaining() / duration
    }

    pub fn is_active(&self) -> bool {
        let now = now_secs();
        now >= self.open_time && now < self.close_time
    }

    pub fn is_expired(&self) -> bool {
        now_secs() >= self.close_time
    }
}

pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

pub fn current_window_start(duration_secs: u64) -> u64 {
    let now = now_secs() as u64;
    now - (now % duration_secs)
}

pub fn next_window_start(duration_secs: u64) -> u64 {
    current_window_start(duration_secs) + duration_secs
}

/// Discover a 5-minute window by deterministic slug lookup.
/// Retries internally if the market isn't created yet.
pub async fn discover_window(
    asset: &str,
    window_start: u64,
    cfg: &DiscoveryConfig,
) -> Option<Window> {
    let dur = cfg.window_duration_secs;
    let slug = format!("{}-updown-5m-{}", asset, window_start);
    let window_end = window_start + dur;

    // Try slug lookup, retry up to 6 times (30s total)
    for attempt in 0..6 {
        match slug_lookup(&cfg.gamma_url, &slug, asset, window_start as f64, window_end as f64).await {
            Some(w) => {
                info!(slug = %w.slug, asset, "window discovered");
                return Some(w);
            }
            None => {
                if attempt < 5 {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    // Fallback: search Gamma events
    match search_gamma_events(&cfg.gamma_url, asset, window_start as f64, window_end as f64).await {
        Some(w) => {
            info!(slug = %w.slug, asset, "window found via fallback search");
            Some(w)
        }
        None => {
            warn!(asset, window_start, "window discovery failed after all retries");
            None
        }
    }
}

async fn slug_lookup(
    gamma_url: &str,
    slug: &str,
    asset: &str,
    start: f64,
    end: f64,
) -> Option<Window> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(format!("{}/events", gamma_url))
        .query(&[("slug", slug), ("limit", "1")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let events = resp.as_array()?;
    let ev = events.first()?;

    // Try markets array first, then treat event as single market
    if let Some(markets) = ev.get("markets").and_then(|m| m.as_array()) {
        for m in markets {
            if let Some(w) = parse_market(m, ev, asset, slug, start, end) {
                return Some(w);
            }
        }
    }

    parse_market(ev, ev, asset, slug, start, end)
}

async fn search_gamma_events(
    gamma_url: &str,
    asset: &str,
    start: f64,
    end: f64,
) -> Option<Window> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get(format!("{}/events", gamma_url))
        .query(&[
            ("active", "true"),
            ("closed", "false"),
            ("limit", "50"),
            ("order", "volume24hr"),
            ("ascending", "false"),
        ])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let keywords: &[&str] = match asset {
        "btc" => &["bitcoin"],
        "eth" => &["ethereum"],
        "sol" => &["solana"],
        "xrp" => &["xrp", "ripple"],
        _ => &[asset],
    };

    for ev in resp.as_array()? {
        let title = ev.get("title").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if !title.contains("up or down") {
            continue;
        }
        if !keywords.iter().any(|kw| title.contains(kw)) {
            continue;
        }
        if let Some(markets) = ev.get("markets").and_then(|m| m.as_array()) {
            for m in markets {
                let slug = m.get("slug").and_then(|s| s.as_str()).unwrap_or("");
                if slug.contains("5m") || title.contains("5 min") {
                    if let Some(w) = parse_market(m, ev, asset, slug, start, end) {
                        if w.is_active() || w.open_time > now_secs() - 60.0 {
                            return Some(w);
                        }
                    }
                }
            }
        }
    }

    None
}

fn parse_market(
    market: &serde_json::Value,
    event: &serde_json::Value,
    asset: &str,
    slug: &str,
    mut start: f64,
    mut end: f64,
) -> Option<Window> {
    // Parse clobTokenIds (handle string-encoded JSON)
    let token_ids: Vec<String> = match market.get("clobTokenIds") {
        Some(serde_json::Value::Array(arr)) => {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        }
        Some(serde_json::Value::String(s)) => {
            serde_json::from_str::<Vec<String>>(s).unwrap_or_default()
        }
        _ => return None,
    };

    if token_ids.len() < 2 {
        return None;
    }

    let condition_id = market
        .get("conditionId")
        .or_else(|| market.get("condition_id"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    // Use API endDate to correct for clock drift
    let end_date = market
        .get("endDate")
        .or_else(|| market.get("end_date"))
        .or_else(|| event.get("endDate"))
        .and_then(|d| d.as_str())
        .unwrap_or("");

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(
        &end_date.replace('Z', "+00:00"),
    ) {
        let api_end = dt.timestamp() as f64;
        if (api_end - end).abs() < 600.0 {
            end = api_end;
            start = end - 300.0;
        }
    }

    Some(Window {
        slug: slug.to_string(),
        asset: asset.to_string(),
        condition_id,
        yes_token: token_ids[0].clone(),
        no_token: token_ids[1].clone(),
        open_time: start,
        close_time: end,
        open_price: 0.0,
    })
}
