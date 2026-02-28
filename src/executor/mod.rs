pub mod positions;

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{LocalSigner, Normal, Signer};
use polymarket_client_sdk::clob::types::{Side, SignableOrder, SignedOrder};
use polymarket_client_sdk::clob::{Client, Config as ClobConfig};
use polymarket_client_sdk::types::{Decimal, U256};
use polymarket_client_sdk::POLYGON;
use rust_decimal::prelude::FromPrimitive;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::strategy::divergence::{DivEvent, Signal};

type AuthClient = Client<Authenticated<Normal>>;

pub async fn spawn(mut event_rx: mpsc::Receiver<DivEvent>, cfg: Config) -> Result<()> {
    let private_key = cfg.private_key()?;
    let order_timeout = Duration::from_secs(cfg.strategy.order_timeout_secs);

    let signer = LocalSigner::from_str(&private_key)
        .context("parsing private key")?
        .with_chain_id(Some(POLYGON));

    let client: AuthClient = Client::new("https://clob.polymarket.com", ClobConfig::default())?
        .authentication_builder(&signer)
        .authenticate()
        .await
        .context("CLOB authentication")?;

    info!("CLOB client authenticated, executor ready");

    let mut positions = positions::PositionTracker::new();

    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                DivEvent::Signal(signal) => {
                    if let Err(e) = execute_signal(
                        &client, &signer, &signal, &mut positions, order_timeout,
                    ).await {
                        error!(%e, market = %signal.market_name, "order execution failed");
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

    crate::data::alert("INFO", "arb.fill",
        &format!("Placed {} @ ${:.4} on {}", signal.side, signal.price, signal.market_name),
        serde_json::json!({
            "market": signal.market_name, "side": signal.side.to_string(),
            "price": signal.price, "edge": signal.edge,
        }));

    positions.record_open(signal.clone());

    // Cancel unfilled order after timeout
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
