pub mod positions;

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{LocalSigner, Normal, Signer};
use polymarket_client_sdk::clob::types::{
    AssetType, Side, SignableOrder, SignatureType, SignedOrder,
};
use polymarket_client_sdk::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk::clob::{Client, Config as ClobConfig};
use polymarket_client_sdk::types::{Address, Decimal, U256};
use polymarket_client_sdk::POLYGON;
use rust_decimal::prelude::FromPrimitive;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::data::{self, SharedLiveStats};
use crate::strategy::divergence::{DivEvent, Signal};

type AuthClient = Client<Authenticated<Normal>>;

pub async fn spawn(
    mut event_rx: mpsc::Receiver<DivEvent>,
    cfg: Config,
    live_stats: SharedLiveStats,
) -> Result<()> {
    let private_key = cfg.private_key()?;
    let proxy_wallet = cfg.proxy_wallet()?;
    let order_timeout = Duration::from_secs(cfg.strategy.order_timeout_secs);

    let signer = LocalSigner::from_str(&private_key)
        .context("parsing private key")?
        .with_chain_id(Some(POLYGON));

    let funder: Address = proxy_wallet.parse()
        .context("parsing POLYMARKET_PROXY_WALLET address")?;

    let client: AuthClient = Client::new("https://clob.polymarket.com", ClobConfig::default())?
        .authentication_builder(&signer)
        .funder(funder)
        .signature_type(SignatureType::GnosisSafe)
        .authenticate()
        .await
        .context("CLOB authentication")?;

    info!(proxy = %proxy_wallet, "CLOB authenticated with proxy wallet");

    let bal_client = client.clone();
    let bal_stats = live_stats.clone();
    tokio::spawn(async move {
        let mut first = true;
        loop {
            match query_balance(&bal_client).await {
                Ok(bal) => {
                    let mut stats = bal_stats.lock().unwrap();
                    stats.balance = bal;
                    if first {
                        stats.session_start_balance = bal;
                        first = false;
                        info!(balance = %format!("${:.2}", bal), "initial CLOB balance");
                    }
                }
                Err(e) => {
                    warn!(%e, "CLOB balance query failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    let mut positions = positions::PositionTracker::new();

    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                DivEvent::Signal(signal) => {
                    {
                        let mut stats = live_stats.lock().unwrap();
                        stats.open += 1;
                    }
                    if let Err(e) = execute_signal(
                        &client, &signer, &signal, &mut positions, order_timeout,
                    ).await {
                        error!(%e, market = %signal.market_name, "order execution failed");
                        let mut stats = live_stats.lock().unwrap();
                        stats.open = stats.open.saturating_sub(1);
                    }
                }
                DivEvent::Converged { market_name, duration_ms, peak_edge } => {
                    info!(
                        event = "CONVERGED_EXEC",
                        market = %market_name,
                        duration_ms,
                        peak_edge = %format!("${:.4}", peak_edge),
                    );
                }
            }
        }
        warn!("event channel closed, executor shutting down");
    });

    Ok(())
}

async fn query_balance(client: &AuthClient) -> Result<f64> {
    let request = BalanceAllowanceRequest::builder()
        .asset_type(AssetType::Collateral)
        .build();
    let result = client.balance_allowance(request).await
        .context("balance_allowance")?;
    use rust_decimal::prelude::ToPrimitive;
    Ok(result.balance.to_f64().unwrap_or(0.0))
}

async fn execute_signal<S: Signer + Send + Sync>(
    client: &AuthClient,
    signer: &S,
    signal: &Signal,
    positions: &mut positions::PositionTracker,
    timeout: Duration,
) -> Result<()> {
    let token_id = U256::from_str(&signal.token_id)
        .context("parsing token_id as U256")?;

    let price = Decimal::from_f64(signal.price)
        .context("invalid price")?
        .round_dp(2);
    let size = Decimal::from_f64(signal.size_usd / signal.price)
        .context("invalid size")?
        .round_dp(2);

    if size <= Decimal::ZERO {
        return Ok(());
    }

    info!(
        event = "PLACING_ORDER",
        market = %signal.market_name,
        side = %signal.side,
        price = %price,
        size = %size,
        edge = %format!("${:.4}", signal.edge),
    );

    let order: SignableOrder = client
        .limit_order()
        .token_id(token_id)
        .price(price)
        .size(size)
        .side(Side::Buy)
        .build()
        .await
        .context("building order")?;

    let signed: SignedOrder = client.sign(signer, order).await
        .context("signing order")?;

    let response = client.post_order(signed).await
        .context("posting order")?;

    let order_id = response.order_id.to_string();

    info!(
        event = "ORDER_PLACED",
        market = %signal.market_name,
        order_id = %order_id,
        success = response.success,
    );

    data::alert("INFO", "arb.fill",
        &format!("Placed {} @ ${:.4} on {}", signal.side, signal.price, signal.market_name),
        serde_json::json!({
            "market": signal.market_name, "side": signal.side.to_string(),
            "price": signal.price, "edge": signal.edge,
        }));

    positions.record_open(signal.clone());

    let cancel_client = client.clone();
    let cancel_order_id = order_id.clone();
    let cancel_market = signal.market_name.clone();
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        match cancel_client.cancel_order(&cancel_order_id).await {
            Ok(_) => info!(
                event = "ORDER_CANCELLED",
                market = %cancel_market,
                order_id = %cancel_order_id,
                reason = "timeout",
            ),
            Err(e) => info!(
                event = "CANCEL_SKIPPED",
                market = %cancel_market,
                reason = %e,
            ),
        }
    });

    Ok(())
}
