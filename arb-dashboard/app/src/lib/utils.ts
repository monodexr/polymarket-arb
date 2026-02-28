export function formatUsd(n: number | null | undefined, opts?: { sign?: boolean }): string {
  if (n == null) return "—";
  const abs = Math.abs(n);
  const str = abs >= 1000
    ? `$${abs.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
    : `$${abs.toFixed(2)}`;
  if (opts?.sign) return n >= 0 ? `+${str}` : `-${str}`;
  return n < 0 ? `-${str}` : str;
}

export function formatTime(ts: number): string {
  const d = new Date(ts * 1000);
  return d.toLocaleTimeString("en-US", { hour12: false, hour: "2-digit", minute: "2-digit" });
}

export function formatEdge(pct: number): string {
  return `${pct >= 0 ? "+" : ""}${pct.toFixed(1)}%`;
}

export function formatPrice(p: number): string {
  if (p >= 1000) return `$${p.toLocaleString("en-US", { maximumFractionDigits: 0 })}`;
  if (p >= 1) return `$${p.toFixed(2)}`;
  return p.toFixed(2);
}

export function formatDuration(sec: number): string {
  if (sec < 60) return `${sec.toFixed(1)}s`;
  return `${Math.floor(sec / 60)}m ${Math.round(sec % 60)}s`;
}
