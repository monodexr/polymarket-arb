# polymarket-arb

Latency arbitrage bot for Polymarket fee-free BTC markets. Exploits price lag between exchange spot feeds and Polymarket CLOB.

## Quick start

```bash
cargo build --release

# Dry run (logs signals, no trades)
POLYMARKET_PRIVATE_KEY=0x... ./target/release/polymarket-arb --dry-run

# Live
POLYMARKET_PRIVATE_KEY=0x... ./target/release/polymarket-arb
```

## Architecture

- **Direct exchange WebSockets** (Binance, Coinbase, Kraken, OKX, Deribit) for sub-10ms price feeds
- **polymarket-client-sdk** for CLOB order signing, WebSocket book updates, and Gamma API market discovery
- **Black-Scholes binary pricing** with Deribit implied volatility
- **7-17ms tick-to-order** hot path on Vultr NJ VPS
