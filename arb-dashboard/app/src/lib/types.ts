export interface FeedStatus {
  connected: boolean;
  price: number;
  latency_ms: number;
}

export interface ArbMarket {
  title: string;
  condition_id: string;
  fair_value: number;
  clob_mid: number;
  clob_best_bid: number;
  clob_best_ask: number;
  edge_pct: number;
  divergence_open: boolean;
  divergence_since: number | null;
  state: "scanning" | "divergence" | "executing" | "filled" | "converged";
}

export interface TradesSummary {
  wins: number;
  losses: number;
  open: number;
  total_pnl: number;
  session_pnl: number;
  daily_pnl: number;
  avg_edge: number;
  avg_latency_ms: number;
}

export interface ArbTrade {
  timestamp: number;
  market: string;
  side: string;
  entry_price: number;
  exit_price: number | null;
  edge_pct: number;
  pnl: number | null;
  duration_sec: number | null;
  outcome: "converged" | "adverse" | "open";
  latency_ms?: number;
}

export interface DailyCap {
  limit: number;
  used_pct: number;
}

export interface ArbStatus {
  timestamp: number;
  balance: number;
  seed: number;
  feeds: Record<string, FeedStatus>;
  spot_price: number;
  implied_vol: number;
  markets: ArbMarket[];
  trades: TradesSummary;
  recent_trades: ArbTrade[];
  daily_cap: DailyCap;
}

export interface Alert {
  timestamp: number;
  severity: string;
  category: string;
  message: string;
  data?: Record<string, unknown>;
}
