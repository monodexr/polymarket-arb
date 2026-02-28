use std::collections::HashMap;

use chrono::{DateTime, Utc};
use regex::Regex;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct Market {
    pub condition_id: String,
    pub yes_token: String,
    pub no_token: String,
    pub strike: f64,
    pub expiry: DateTime<Utc>,
    pub title: String,
    pub slug: Option<String>,
}

pub type MarketSnapshot = HashMap<String, Market>;
pub type MarketStateTx = watch::Sender<MarketSnapshot>;
pub type MarketStateRx = watch::Receiver<MarketSnapshot>;

pub fn spawn(cfg: Config) -> MarketStateRx {
    let (tx, rx) = watch::channel(MarketSnapshot::new());

    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(cfg.discovery.poll_interval_secs);

        loop {
            match discover_markets(&cfg).await {
                Ok(markets) => {
                    let count = markets.len();
                    let mut snapshot = MarketSnapshot::new();
                    for m in markets {
                        snapshot.insert(m.yes_token.clone(), m);
                    }
                    let _ = tx.send(snapshot);
                    info!(markets = count, "discovery refresh complete");
                }
                Err(e) => error!(%e, "market discovery failed"),
            }
            tokio::time::sleep(interval).await;
        }
    });

    rx
}

/// Parse strike price from text, handling $150k, $1m, $100,000 etc.
fn parse_strike(text: &str) -> Option<f64> {
    let re = Regex::new(r"\$([0-9,]+\.?[0-9]*)\s*([kKmMbB])?").ok()?;
    let caps = re.captures(text)?;
    let raw = caps.get(1)?.as_str().replace(',', "");
    let mut value: f64 = raw.parse().ok()?;
    if let Some(suffix) = caps.get(2) {
        match suffix.as_str() {
            "k" | "K" => value *= 1_000.0,
            "m" | "M" => value *= 1_000_000.0,
            "b" | "B" => value *= 1_000_000_000.0,
            _ => {}
        }
    }
    if value > 0.0 { Some(value) } else { None }
}

async fn discover_markets(cfg: &Config) -> anyhow::Result<Vec<Market>> {
    let client = reqwest::Client::new();

    // Paginate to get all active events (100 per page)
    let mut all_events: Vec<serde_json::Value> = Vec::new();
    for offset in (0..500).step_by(100) {
        let resp: serde_json::Value = client
            .get("https://gamma-api.polymarket.com/events")
            .query(&[
                ("active", "true"),
                ("closed", "false"),
                ("limit", "100"),
                ("offset", &offset.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;

        let page = resp.as_array().cloned().unwrap_or_default();
        let count = page.len();
        all_events.extend(page);
        if count < 100 {
            break;
        }
    }

    let events = &all_events;
    info!(total_events = events.len(), "gamma API returned events");

    let mut candidates: Vec<(Market, String)> = Vec::new();

    for event in events {
        let title = event
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let matches_keyword = cfg
            .discovery
            .filter_keywords
            .iter()
            .any(|kw| title.to_lowercase().contains(&kw.to_lowercase()));
        if !matches_keyword {
            continue;
        }

        let slug = event.get("slug").and_then(|s| s.as_str()).unwrap_or("");
        if slug.contains("5-minute") || slug.contains("15-minute") {
            info!(title, slug, "skipping short-term market");
            continue;
        }

        info!(title, slug, "BTC event found, checking markets");

        let event_markets = match event.get("markets").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => continue,
        };

        for mkt in event_markets {
            let condition_id = mkt
                .get("conditionId")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();

            // clobTokenIds is a JSON string containing an array, not an array directly
            let token_ids: Vec<String> = match mkt.get("clobTokenIds") {
                Some(serde_json::Value::Array(arr)) => {
                    arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                }
                Some(serde_json::Value::String(s)) => {
                    serde_json::from_str::<Vec<String>>(s).unwrap_or_default()
                }
                _ => continue,
            };

            if token_ids.len() < 2 {
                continue;
            }

            let yes_token = token_ids[0].clone();
            let no_token = token_ids[1].clone();

            if yes_token.is_empty() || no_token.is_empty() {
                continue;
            }

            let question = mkt
                .get("question")
                .and_then(|q| q.as_str())
                .unwrap_or(title);

            let strike = match parse_strike(question) {
                Some(v) => v,
                None => {
                    info!(question, "no parseable strike price, skipping");
                    continue;
                }
            };

            // Must be a price target (e.g. "hit $150k"), not a holdings question
            let q_lower = question.to_lowercase();
            let is_price_target = q_lower.contains("hit")
                || q_lower.contains("above")
                || q_lower.contains("below")
                || q_lower.contains("reach");
            if !is_price_target {
                info!(question, strike, "not a price target market, skipping");
                continue;
            }

            let expiry_str = mkt
                .get("endDate")
                .or_else(|| event.get("endDate"))
                .and_then(|d| d.as_str())
                .unwrap_or("");

            let expiry = match expiry_str.parse::<DateTime<Utc>>() {
                Ok(dt) => dt,
                Err(_) => {
                    warn!(market = %question, "could not parse expiry, skipping");
                    continue;
                }
            };

            if expiry <= Utc::now() {
                info!(question, %expiry, "expired, skipping");
                continue;
            }

            // Use feesEnabled from Gamma response (more reliable than CLOB fee-rate endpoint)
            let fees_enabled = mkt
                .get("feesEnabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true); // assume fees if field missing

            if fees_enabled {
                info!(question, "fees enabled, skipping");
                continue;
            }

            info!(question, strike, %expiry, "candidate market found (fee-free)");

            candidates.push((
                Market {
                    condition_id,
                    yes_token: yes_token.clone(),
                    no_token,
                    strike,
                    expiry,
                    title: question.to_string(),
                    slug: Some(slug.to_string()),
                },
                yes_token,
            ));
        }
    }

    // Fee check is now done inline via feesEnabled field from Gamma
    // All candidates that reach here are already fee-free
    let markets: Vec<Market> = candidates.into_iter().map(|(m, _)| m).collect();

    info!(markets_found = markets.len(), "discovery complete");
    Ok(markets)
}

