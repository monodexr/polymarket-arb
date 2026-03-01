use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{info, warn};

const CTF_ADDRESS: &str = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045";
const USDC_E_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

#[derive(Debug, Clone)]
pub struct PendingPosition {
    pub condition_id: String,
    pub market_name: String,
    pub side: String,
    pub entry_price: f64,
    pub size_usd: f64,
}

pub struct Redeemer {
    rpc_url: String,
    private_key: String,
    pending: HashMap<String, PendingPosition>,
    http: reqwest::Client,
}

impl Redeemer {
    pub fn new(private_key: String) -> Self {
        let rpc_url = Self::find_rpc();
        Self {
            rpc_url,
            private_key,
            pending: HashMap::new(),
            http: reqwest::Client::new(),
        }
    }

    fn find_rpc() -> String {
        if let Ok(url) = std::env::var("POLYGON_RPC_URL") {
            return url;
        }
        for path in &[
            "/opt/polyscanner/config/scanner.yaml",
        ] {
            if let Ok(contents) = std::fs::read_to_string(path) {
                for line in contents.lines() {
                    if let Some(idx) = line.find("https://polygon-mainnet.g.alchemy.com") {
                        let url = line[idx..].trim().trim_matches('"').trim_matches('\'');
                        return url.to_string();
                    }
                }
            }
        }
        "https://1rpc.io/matic".to_string()
    }

    pub fn track_position(&mut self, pos: PendingPosition) {
        info!(
            condition_id = %pos.condition_id,
            market = %pos.market_name,
            "tracking position for redemption"
        );
        self.pending.insert(pos.condition_id.clone(), pos);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub async fn process_pending(&mut self) -> Vec<RedemptionResult> {
        let mut results = Vec::new();
        let condition_ids: Vec<String> = self.pending.keys().cloned().collect();

        for cid in condition_ids {
            match self.query_resolution(&cid).await {
                Ok(Some(resolution)) => {
                    let pos = self.pending.get(&cid).unwrap().clone();
                    info!(
                        condition_id = %cid,
                        market = %pos.market_name,
                        resolution = %resolution,
                        "market resolved, attempting redemption"
                    );

                    match self.redeem_positions(&cid).await {
                        Ok(tx_hash) => {
                            let won = (pos.side == "BUY_YES" && resolution == "UP")
                                || (pos.side == "BUY_NO" && resolution == "DOWN");
                            info!(
                                condition_id = %cid,
                                market = %pos.market_name,
                                won = won,
                                tx = %tx_hash,
                                "redemption successful"
                            );
                            results.push(RedemptionResult {
                                condition_id: cid.clone(),
                                market_name: pos.market_name.clone(),
                                side: pos.side.clone(),
                                entry_price: pos.entry_price,
                                size_usd: pos.size_usd,
                                won,
                                tx_hash,
                            });
                            self.pending.remove(&cid);
                        }
                        Err(e) => {
                            warn!(
                                condition_id = %cid,
                                market = %pos.market_name,
                                %e,
                                "redemption failed, will retry"
                            );
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(condition_id = %cid, %e, "resolution query failed");
                }
            }
        }

        results
    }

    async fn query_resolution(&self, condition_id: &str) -> Result<Option<String>> {
        let cid_hex = normalize_condition_id(condition_id);

        let p0 = self.call_payout_numerators(&cid_hex, 0).await?;
        let p1 = self.call_payout_numerators(&cid_hex, 1).await?;

        if p0 > 0 {
            Ok(Some("UP".to_string()))
        } else if p1 > 0 {
            Ok(Some("DOWN".to_string()))
        } else {
            Ok(None)
        }
    }

    async fn call_payout_numerators(&self, cid_hex: &str, index: u64) -> Result<u64> {
        let index_hex = format!("{:064x}", index);
        let data = format!("0xda3550f7{}{}", &cid_hex[2..], index_hex);

        let resp = self.http.post(&self.rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_call",
                "params": [{"to": CTF_ADDRESS, "data": data}, "latest"],
                "id": 1
            }))
            .send()
            .await
            .context("RPC call")?;

        let body: serde_json::Value = resp.json().await?;
        let hex = body["result"].as_str().unwrap_or("0x0");
        let clean = hex.trim_start_matches("0x");
        if clean.is_empty() || clean.chars().all(|c| c == '0') {
            return Ok(0);
        }
        Ok(u64::from_str_radix(&clean[clean.len().saturating_sub(16)..], 16).unwrap_or(0))
    }

    async fn redeem_positions(&self, condition_id: &str) -> Result<String> {
        let cid_hex = normalize_condition_id(condition_id);

        let output = tokio::process::Command::new("python3")
            .arg("-c")
            .arg(format!(
                r#"
import os, sys
from web3 import Web3
from web3.middleware import ExtraDataToPOAMiddleware

rpc = "{rpc}"
key = os.environ["POLYMARKET_PRIVATE_KEY"]

w3 = Web3(Web3.HTTPProvider(rpc))
w3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)

acct = w3.eth.account.from_key(key)
addr = acct.address

ctf_abi = [{{
    "inputs": [
        {{"name": "collateralToken", "type": "address"}},
        {{"name": "parentCollectionId", "type": "bytes32"}},
        {{"name": "conditionId", "type": "bytes32"}},
        {{"name": "indexSets", "type": "uint256[]"}}
    ],
    "name": "redeemPositions",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
}}]

ctf = w3.eth.contract(address=w3.to_checksum_address("{ctf}"), abi=ctf_abi)

cid = "{cid}"
if not cid.startswith("0x"):
    cid = "0x" + cid
cid_bytes = bytes.fromhex(cid[2:].zfill(64))

nonce = w3.eth.get_transaction_count(addr)
tx = ctf.functions.redeemPositions(
    w3.to_checksum_address("{usdc}"),
    b"\x00" * 32,
    cid_bytes,
    [1, 2]
).build_transaction({{
    "chainId": 137,
    "from": addr,
    "nonce": nonce,
    "gasPrice": w3.eth.gas_price,
    "gas": 500000,
}})

signed = w3.eth.account.sign_transaction(tx, private_key=key)
tx_hash = w3.eth.send_raw_transaction(signed.raw_transaction)
receipt = w3.eth.wait_for_transaction_receipt(tx_hash, timeout=120)
print(receipt.transactionHash.hex())
"#,
                rpc = self.rpc_url,
                ctf = CTF_ADDRESS,
                usdc = USDC_E_ADDRESS,
                cid = &cid_hex[2..],
            ))
            .output()
            .await
            .context("running redemption script")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("redemption script failed: {}", stderr);
        }

        let tx_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(tx_hash)
    }
}

fn normalize_condition_id(cid: &str) -> String {
    let clean = cid.trim_start_matches("0x");
    format!("0x{:0>64}", clean)
}

pub struct RedemptionResult {
    pub condition_id: String,
    pub market_name: String,
    pub side: String,
    pub entry_price: f64,
    pub size_usd: f64,
    pub won: bool,
    pub tx_hash: String,
}

pub fn spawn_redemption_loop(
    private_key: String,
    mut position_rx: tokio::sync::mpsc::Receiver<PendingPosition>,
    live_stats: crate::data::SharedLiveStats,
) {
    tokio::spawn(async move {
        let mut redeemer = Redeemer::new(private_key);

        loop {
            while let Ok(pos) = position_rx.try_recv() {
                redeemer.track_position(pos);
            }

            if redeemer.pending_count() > 0 {
                let results = redeemer.process_pending().await;
                for r in &results {
                    let pnl = if r.won {
                        r.size_usd * (1.0 / r.entry_price - 1.0)
                    } else {
                        -r.size_usd
                    };
                    let exit_price = if r.won { 1.0 } else { 0.0 };
                    let outcome = if r.won { "converged" } else { "adverse" };

                    {
                        let mut stats = live_stats.lock().unwrap();
                        stats.open = stats.open.saturating_sub(1);
                        stats.total_pnl += pnl;
                        stats.session_pnl += pnl;
                        stats.daily_pnl += pnl;
                        if r.won { stats.wins += 1; } else { stats.losses += 1; }
                    }

                    let category = if r.won { "arb.converge" } else { "arb.adverse" };
                    let emoji = if r.won { "WIN" } else { "LOSS" };
                    crate::data::alert(
                        if r.won { "INFO" } else { "WARNING" },
                        category,
                        &format!("{} {} on {} — ${:.2} ({})", emoji, r.side, r.market_name, pnl, r.tx_hash.get(..10).unwrap_or(&r.tx_hash)),
                        serde_json::json!({
                            "market": r.market_name, "won": r.won, "pnl": pnl,
                            "entry_price": r.entry_price, "exit_price": exit_price,
                            "side": r.side, "tx_hash": r.tx_hash,
                        }),
                    );

                    crate::data::write_trade(&crate::data::TradeRecord {
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64(),
                        market: r.market_name.clone(),
                        side: r.side.clone(),
                        entry_price: r.entry_price,
                        exit_price,
                        edge_pct: 0.0,
                        pnl,
                        duration_sec: 0.0,
                        outcome: outcome.to_string(),
                    });
                }

                if !results.is_empty() {
                    info!(
                        redeemed = results.len(),
                        won = results.iter().filter(|r| r.won).count(),
                        lost = results.iter().filter(|r| !r.won).count(),
                        pending = redeemer.pending_count(),
                        "redemption cycle complete"
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
