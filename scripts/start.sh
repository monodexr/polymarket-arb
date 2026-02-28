#!/bin/bash
# Secure startup script for the arb bot.
# Sources the private key from .env file — never pass keys as CLI args.
#
# Usage:
#   ./scripts/start.sh          # foreground
#   ./scripts/start.sh daemon   # background (survives SSH disconnect)

set -euo pipefail

BOT_DIR="/opt/polymarket-arb"
ENV_FILE="${BOT_DIR}/.env"
LOG_FILE="/var/log/arb-bot.log"
PID_FILE="/var/run/arb-bot.pid"

cd "$BOT_DIR"

if [ ! -f "$ENV_FILE" ]; then
    echo "ERROR: $ENV_FILE not found."
    echo "Create it with: nano $ENV_FILE"
    echo "Format: export POLYMARKET_PRIVATE_KEY=\"0x...\""
    exit 1
fi

source "$ENV_FILE"

if [ -z "${POLYMARKET_PRIVATE_KEY:-}" ]; then
    echo "ERROR: POLYMARKET_PRIVATE_KEY not set after sourcing $ENV_FILE"
    echo "Check the file format: export POLYMARKET_PRIVATE_KEY=\"0x...\""
    exit 1
fi

if [ ! -f "target/release/polymarket-arb" ]; then
    echo "ERROR: Binary not found. Run: cargo build --release"
    exit 1
fi

if [ ! -f "config.toml" ]; then
    echo "ERROR: config.toml not found in $BOT_DIR"
    exit 1
fi

# Kill any existing instance
if [ -f "$PID_FILE" ]; then
    OLD_PID=$(cat "$PID_FILE")
    if kill -0 "$OLD_PID" 2>/dev/null; then
        echo "Stopping existing bot (PID $OLD_PID)..."
        kill "$OLD_PID"
        sleep 2
    fi
    rm -f "$PID_FILE"
fi

if [ "${1:-}" = "daemon" ]; then
    nohup ./target/release/polymarket-arb > "$LOG_FILE" 2>&1 &
    echo $! > "$PID_FILE"
    disown
    sleep 2
    if kill -0 "$(cat $PID_FILE)" 2>/dev/null; then
        echo "Bot started (PID $(cat $PID_FILE))"
        echo "Logs: tail -f $LOG_FILE"
        echo "Stop: kill \$(cat $PID_FILE)"
        tail -10 "$LOG_FILE"
    else
        echo "ERROR: Bot failed to start. Check $LOG_FILE"
        tail -20 "$LOG_FILE"
        exit 1
    fi
else
    exec ./target/release/polymarket-arb
fi
