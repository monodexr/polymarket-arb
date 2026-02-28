pub mod positions;

use std::str::FromStr;

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
use crate::strategy::divergence::Signal;

type AuthClient = Client<Authenticated<Normal>>;
pub async fn spawn(mut signal_rx: mpsc::Receiver<Signal>, cfg: Config) -> Result<()> {
    let private_key = cfg.private_key()?;

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
        while let Some(signal) = signal_rx.recv().await {
            if let Err(e) = execute_signal(&client, &signer, &signal, &mut positions).await {
                error!(%e, market = %signal.market_name, "order execution failed");
            }
        }
        warn!("signal channel closed, executor shutting down");
    });

    Ok(())
}

async fn execute_signal<S: Signer + Send + Sync>(
    client: &AuthClient,
    signer: &S,
    signal: &Signal,
    positions: &mut positions::PositionTracker,
) -> Result<()> {
    let token_id = U256::from_str(&signal.token_id)
        .context("parsing token_id as U256")?;

    let price = Decimal::from_f64(signal.price)
        .context("invalid price")?;
    let size = Decimal::from_f64(signal.size_usd / signal.price)
        .context("invalid size")?;

    info!(
        event = "PLACING_ORDER",
        market = %signal.market_name,
        side = %signal.side,
        price = %price,
        size = %size,
        edge = %format!("{:.2}%", signal.edge_pct * 100.0),
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

    info!(
        event = "ORDER_PLACED",
        market = %signal.market_name,
        order_id = %response.order_id,
        success = response.success,
    );

    positions.record_open(signal.clone());

    Ok(())
}
