use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use regex::Regex;
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

pub type MarketState = Arc<RwLock<HashMap<String, Market>>>;

pub fn spawn(cfg: Config) -> MarketState {
    let state: MarketState = Arc::new(RwLock::new(HashMap::new()));
    let state_clone = state.clone();

    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(cfg.discovery.poll_interval_secs);
        loop {
            match discover_markets(&cfg).await {
                Ok(markets) => {
                    let count = markets.len();
                    let mut guard = state_clone.write().unwrap();
                    guard.clear();
                    for m in markets {
                        guard.insert(m.yes_token.clone(), m);
                    }
                    info!(markets = count, "discovery refresh complete");
                }
                Err(e) => error!(%e, "market discovery failed"),
            }
            tokio::time::sleep(interval).await;
        }
    });

    state
}

async fn discover_markets(cfg: &Config) -> anyhow::Result<Vec<Market>> {
    let client = reqwest::Client::new();
    let strike_re = Regex::new(r"\$([0-9,]+)")?;

    let resp: serde_json::Value = client
        .get("https://gamma-api.polymarket.com/events")
        .query(&[
            ("active", "true"),
            ("closed", "false"),
            ("limit", "100"),
        ])
        .send()
        .await?
        .json()
        .await?;

    let empty = vec![];
    let events = resp.as_array().unwrap_or(&empty);
    let mut markets = Vec::new();

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

        // Skip 5-min and 15-min markets (they have taker fees)
        let slug = event.get("slug").and_then(|s| s.as_str()).unwrap_or("");
        if slug.contains("5-minute") || slug.contains("15-minute") {
            continue;
        }

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

            let tokens = match mkt.get("clobTokenIds").and_then(|t| t.as_array()) {
                Some(t) if t.len() >= 2 => t,
                _ => continue,
            };

            let yes_token = tokens[0].as_str().unwrap_or("").to_string();
            let no_token = tokens[1].as_str().unwrap_or("").to_string();

            if yes_token.is_empty() || no_token.is_empty() {
                continue;
            }

            // Parse strike price from market question
            let question = mkt
                .get("question")
                .and_then(|q| q.as_str())
                .unwrap_or(title);

            let strike = match strike_re.captures(question) {
                Some(caps) => {
                    let raw = caps.get(1).unwrap().as_str().replace(',', "");
                    match raw.parse::<f64>() {
                        Ok(v) => v,
                        Err(_) => continue,
                    }
                }
                None => continue,
            };

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
                continue;
            }

            // Check fee rate — skip markets with taker fees
            let fee_ok = check_fee_free(&client, &yes_token).await;
            if !fee_ok {
                continue;
            }

            markets.push(Market {
                condition_id,
                yes_token,
                no_token,
                strike,
                expiry,
                title: question.to_string(),
                slug: Some(slug.to_string()),
            });
        }
    }

    Ok(markets)
}

async fn check_fee_free(client: &reqwest::Client, token_id: &str) -> bool {
    let url = format!(
        "https://clob.polymarket.com/fee-rate?tokenID={}",
        token_id
    );
    match client.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => {
                let taker = v.get("taker").and_then(|t| t.as_f64()).unwrap_or(1.0);
                let maker = v.get("maker").and_then(|m| m.as_f64()).unwrap_or(1.0);
                taker == 0.0 && maker == 0.0
            }
            Err(_) => false,
        },
        Err(_) => false,
    }
}
