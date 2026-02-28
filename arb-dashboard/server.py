#!/usr/bin/env python3
"""Arb bot dashboard server — reads bot data files and serves React SPA.

Run: python3 server.py
Serves at http://localhost:8081
"""
import json
import os
import time
from http.server import HTTPServer, SimpleHTTPRequestHandler
from pathlib import Path
from socketserver import ThreadingMixIn
from urllib.parse import parse_qs, urlparse

DATA = Path(os.environ.get("ARB_DATA_DIR", "data"))
DASHBOARD_DIR = Path(__file__).resolve().parent
DIST_DIR = DASHBOARD_DIR / "dashboard"
if not DIST_DIR.exists():
    DIST_DIR = DASHBOARD_DIR / "app" / "dist"
STATIC_DIR = DIST_DIR if DIST_DIR.exists() else DASHBOARD_DIR
PAUSE_FLAG = DATA / "pause.flag"
PNL_CONFIG = DATA / "pnl_config.json"
PORT = int(os.environ.get("DASHBOARD_PORT", "8081"))


def load_json(path: Path, default=None):
    if path.exists():
        try:
            return json.loads(path.read_text())
        except (json.JSONDecodeError, OSError):
            pass
    return default if default is not None else {}


def load_jsonl(path: Path, tail: int = 500) -> list:
    if not path.exists():
        return []
    lines = path.read_text().strip().split("\n")
    entries = []
    for line in lines[-tail:]:
        if not line.strip():
            continue
        try:
            entries.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return entries


def load_pnl_config() -> dict:
    """Read pnl_config.json for seed and wallet info."""
    return load_json(PNL_CONFIG, {"seed_usd": 0})


def compute_trade_pnl(trades: list) -> dict:
    """Compute PnL metrics from trades.jsonl entries."""
    wins = sum(1 for t in trades if t.get("outcome") == "converged")
    losses = sum(1 for t in trades if t.get("outcome") == "adverse")
    open_count = sum(1 for t in trades if t.get("outcome") == "open")
    total_pnl = sum(t.get("pnl", 0) or 0 for t in trades)

    now = time.time()
    day_start = now - 86400
    daily_pnl = sum(
        t.get("pnl", 0) or 0
        for t in trades
        if (t.get("timestamp", 0) or 0) >= day_start
    )

    return {
        "wins": wins,
        "losses": losses,
        "open": open_count,
        "total_pnl": round(total_pnl, 2),
        "daily_pnl": round(daily_pnl, 2),
    }


def enrich_status(status: dict) -> dict:
    """Inject PnL data from config and trades.jsonl when bot doesn't provide it."""
    pnl_cfg = load_pnl_config()
    seed = pnl_cfg.get("seed_usd", 0)

    if not status.get("seed") and seed > 0:
        status["seed"] = seed

    trades_data = load_jsonl(DATA / "trades.jsonl", 5000)
    if trades_data:
        computed = compute_trade_pnl(trades_data)
        existing_trades = status.get("trades", {})

        if not existing_trades.get("wins") and not existing_trades.get("losses"):
            status["trades"] = {
                **existing_trades,
                **computed,
                "session_pnl": existing_trades.get("session_pnl", computed["total_pnl"]),
                "avg_edge": existing_trades.get("avg_edge", 0),
                "avg_latency_ms": existing_trades.get("avg_latency_ms", 0),
            }

    balance = status.get("balance", 0)
    seed_val = status.get("seed", seed)

    # Ground truth PnL: balance - seed (only when bot reports a real balance)
    if balance > 0 and seed_val > 0:
        trades_block = status.get("trades", {})
        if not trades_block.get("total_pnl"):
            trades_block["total_pnl"] = round(balance - seed_val, 2)
            status["trades"] = trades_block

    # If balance is 0 but seed is set, don't show false negative —
    # use trade-computed PnL or just 0 (fresh wallet, no data yet)
    if balance == 0 and seed_val > 0:
        trades_block = status.get("trades", {})
        if trades_block.get("total_pnl", 0) == 0 and trades_data:
            trades_block["total_pnl"] = computed.get("total_pnl", 0)
        status["balance"] = seed_val + trades_block.get("total_pnl", 0)
        status["trades"] = trades_block

    return status


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(STATIC_DIR), **kwargs)

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path.rstrip("/")

        if path == "/api/status":
            status = load_json(DATA / "status.json", {})
            self._send_json(enrich_status(status))
        elif path == "/api/alerts":
            raw = load_jsonl(DATA / "alerts.jsonl", 500)
            kept = []
            for a in reversed(raw):
                kept.append(a)
                if len(kept) >= 200:
                    break
            self._send_json({"alerts": list(reversed(kept))})
        elif path == "/api/trades":
            trades = load_jsonl(DATA / "trades.jsonl", 5000)
            self._send_json({"trades": trades})
        elif path == "/api/pause":
            if self.command == "GET":
                self._send_json({"paused": PAUSE_FLAG.exists()})
            return
        elif path == "/api/alerts/stream":
            self._serve_sse()
        elif path.startswith("/api/"):
            self.send_error(404)
        else:
            if not Path(str(STATIC_DIR) + path).exists() and not path.startswith("/assets"):
                self.path = "/index.html"
            super().do_GET()

    def do_POST(self):
        parsed = urlparse(self.path)
        if parsed.path == "/api/pause":
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length)) if length else {}
            if body.get("paused"):
                PAUSE_FLAG.parent.mkdir(parents=True, exist_ok=True)
                PAUSE_FLAG.touch()
            else:
                PAUSE_FLAG.unlink(missing_ok=True)
            self._send_json({"paused": PAUSE_FLAG.exists()})
        else:
            self.send_error(404)

    def _send_json(self, data):
        body = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def _serve_sse(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()

        status_path = DATA / "status.json"
        alerts_path = DATA / "alerts.jsonl"
        last_status_mtime = 0.0
        last_alerts_size = 0

        try:
            while True:
                changed = False
                if status_path.exists():
                    mt = status_path.stat().st_mtime
                    if mt > last_status_mtime:
                        last_status_mtime = mt
                        status = enrich_status(load_json(status_path, {}))
                        self.wfile.write(f"event: status\ndata: {json.dumps(status)}\n\n".encode())
                        changed = True

                if alerts_path.exists():
                    sz = alerts_path.stat().st_size
                    if sz > last_alerts_size:
                        new_data = ""
                        with open(alerts_path) as f:
                            if last_alerts_size > 0:
                                f.seek(last_alerts_size)
                            new_data = f.read()
                        last_alerts_size = sz
                        for line in new_data.strip().split("\n"):
                            if line.strip():
                                try:
                                    alert = json.loads(line)
                                    self.wfile.write(f"event: alert\ndata: {json.dumps(alert)}\n\n".encode())
                                    changed = True
                                except json.JSONDecodeError:
                                    pass

                if changed:
                    self.wfile.flush()

                time.sleep(1)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, fmt, *args):
        if "/api/alerts/stream" not in (args[0] if args else ""):
            super().log_message(fmt, *args)


class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True


if __name__ == "__main__":
    print(f"Arb dashboard serving on http://0.0.0.0:{PORT}")
    print(f"Data dir: {DATA.resolve()}")
    print(f"Static dir: {STATIC_DIR.resolve()}")
    if PNL_CONFIG.exists():
        cfg = load_pnl_config()
        print(f"PnL config: seed=${cfg.get('seed_usd', 0):.2f}")
    else:
        print(f"No pnl_config.json found at {PNL_CONFIG} — create one with {{\"seed_usd\": YOUR_AMOUNT}}")
    server = ThreadedHTTPServer(("0.0.0.0", PORT), Handler)
    server.serve_forever()
