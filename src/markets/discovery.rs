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

/// Fee check results cached per token_id to avoid redundant HTTP requests (H3 fix).
struct FeeCache {
    cache: HashMap<String, bool>,
}

impl FeeCache {
    fn new() -> Self {
        Self { cache: HashMap::new() }
    }

    async fn is_fee_free(&mut self, client: &reqwest::Client, token_id: &str) -> bool {
        if let Some(&cached) = self.cache.get(token_id) {
            return cached;
        }
        let result = check_fee_free(client, token_id).await;
        self.cache.insert(token_id.to_string(), result);
        result
    }
}

pub fn spawn(cfg: Config) -> MarketStateRx {
    let (tx, rx) = watch::channel(MarketSnapshot::new());

    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(cfg.discovery.poll_interval_secs);
        let mut fee_cache = FeeCache::new();

        loop {
            match discover_markets(&cfg, &mut fee_cache).await {
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

async fn discover_markets(cfg: &Config, fee_cache: &mut FeeCache) -> anyhow::Result<Vec<Market>> {
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

    // First pass: collect all candidates without fee checks
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

    // H3: Parallel fee checks with semaphore, using cache for previously seen tokens
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
    let client_ref = &client;

    let mut fee_futures = Vec::new();
    let mut cached_results: Vec<(usize, bool)> = Vec::new();

    for (i, (_, token)) in candidates.iter().enumerate() {
        if let Some(&cached) = fee_cache.cache.get(token) {
            cached_results.push((i, cached));
        } else {
            let sem = sem.clone();
            let token = token.clone();
            fee_futures.push(async move {
                let _permit = sem.acquire().await.unwrap();
                let result = check_fee_free(client_ref, &token).await;
                (i, token, result)
            });
        }
    }

    let fresh_results = futures_util::future::join_all(fee_futures).await;

    // Update cache with fresh results
    for (_, token, result) in &fresh_results {
        fee_cache.cache.insert(token.clone(), *result);
    }

    let mut fee_ok: HashMap<usize, bool> = HashMap::new();
    for (i, ok) in cached_results {
        fee_ok.insert(i, ok);
    }
    for (i, _, result) in fresh_results {
        fee_ok.insert(i, result);
    }

    let markets: Vec<Market> = candidates
        .into_iter()
        .enumerate()
        .filter(|(i, _)| fee_ok.get(i).copied().unwrap_or(false))
        .map(|(_, (m, _))| m)
        .collect();

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
