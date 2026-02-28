import { useEffect, useMemo, useState } from "react";
import { useStatus, useAlerts, useTrades } from "./hooks/useApi";
import { useSoundSettings } from "./hooks/useSoundSettings";
import { useTick } from "./hooks/useTick";
import { formatUsd, formatTime, formatEdge, formatPrice } from "./lib/utils";
import type { ArbMarket, ArbTrade, Alert } from "./lib/types";

const StatusDot = ({ style: extraStyle }: { style?: React.CSSProperties }) => {
  const [opacity, setOpacity] = useState(1);
  useEffect(() => {
    const id = setInterval(() => setOpacity(p => p === 1 ? 0.5 : 1), 1000);
    return () => clearInterval(id);
  }, []);
  return <div style={{ width: "8px", height: "8px", background: "#2B8A3E", borderRadius: "50%", opacity, transition: "opacity 0.5s", ...extraStyle }} />;
};

const ASSET_COLORS: Record<string, string> = { btc: "#F7931A", eth: "#627EEA", sol: "#9945FF", xrp: "#23292F" };

function ContractCard({ market, raw, now, isDark }: { market: ArbMarket; raw: any; now: number; isDark: boolean }) {
  const mono = "'JetBrains Mono', monospace";
  const st = market.state;
  const asset = (raw?.asset ?? "").toUpperCase();
  const assetColor = ASSET_COLORS[(raw?.asset ?? "").toLowerCase()] ?? "#666677";
  const borderColor = st === "divergence" ? "#D97706" : st === "executing" ? "#D92525" : st === "filled" ? "#2B8A3E" : st === "converged" ? "#2B8A3E" : "rgba(255,255,255,0.07)";
  const bgTint = st === "divergence" ? "rgba(217,119,6,0.04)" : st === "executing" ? "rgba(217,37,37,0.05)" : st === "filled" ? "rgba(43,138,62,0.04)" : "transparent";
  const glowAnim = st === "divergence" ? "pulse-glow 2s ease-in-out infinite" : st === "executing" ? "pulse-red 1.2s ease-in-out infinite" : st === "filled" ? "pulse-green 2s ease-in-out infinite" : st === "converged" ? "flash-green 1s ease-out" : "none";
  const inkDim = "#666677";
  const inkPrimary = "#F0F0F2";

  const openPrice = raw?.open_price ?? 0;
  const movePct = raw?.current_move_pct ?? 0;
  const timeLeft = raw?.time_remaining_sec ?? 0;
  const windowDur = 300;
  const timerPct = timeLeft > 0 ? Math.min(100, ((windowDur - timeLeft) / windowDur) * 100) : 0;
  const timerMin = Math.floor(timeLeft / 60);
  const timerSec = Math.round(timeLeft % 60);
  const fairYes = raw?.fair_yes ?? market.fair_value ?? 0;
  const fairNo = raw?.fair_no ?? (1 - fairYes);
  const clobYes = raw?.clob_yes_mid ?? market.clob_mid ?? 0;
  const clobNo = raw?.clob_no_mid ?? 0;
  const edgeYes = raw?.edge_yes ?? 0;
  const edgeNo = raw?.edge_no ?? 0;
  const bestEdge = Math.max(Math.abs(edgeYes), Math.abs(edgeNo));
  const bestEdgeColor = bestEdge > 0.02 ? "#2B8A3E" : bestEdge > 0.005 ? "#D97706" : inkDim;
  const moveColor = movePct > 0 ? "#2B8A3E" : movePct < 0 ? "#D92525" : inkDim;
  const stateLabel = st === "monitoring" ? "MONITORING" : st.toUpperCase();
  const stateBg = st === "divergence" ? "#D97706" : st === "executing" ? "#D92525" : st === "filled" ? "#2B8A3E" : "rgba(255,255,255,0.08)";
  const stateFg = st === "divergence" || st === "executing" || st === "filled" ? "#000" : inkDim;

  return (
    <div style={{
      border: `1.5px solid ${borderColor}`,
      borderRadius: "10px",
      padding: "14px 16px",
      display: "flex",
      flexDirection: "column",
      gap: "10px",
      background: bgTint,
      animation: glowAnim,
      transition: "border-color 0.3s, background 0.3s",
    }}>
      {/* Header: Asset + State */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <div style={{ width: "6px", height: "24px", borderRadius: "3px", background: assetColor }} />
          <div>
            <div style={{ fontFamily: mono, fontSize: "16px", fontWeight: 700, color: inkPrimary, letterSpacing: "-0.5px" }}>{asset || "?"}</div>
            <div style={{ fontFamily: mono, fontSize: "8px", color: inkDim, marginTop: "-1px", maxWidth: "180px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {raw?.slug ?? market.title ?? "—"}
            </div>
          </div>
        </div>
        <div style={{ fontFamily: mono, fontSize: "8px", fontWeight: 700, padding: "2px 8px", borderRadius: "3px", background: stateBg, color: stateFg, textTransform: "uppercase", letterSpacing: "0.5px" }}>
          {stateLabel}
        </div>
      </div>

      {/* Price + Move */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", fontFamily: mono }}>
        <span style={{ fontSize: "10px", color: inkDim }}>open: <span style={{ color: inkPrimary, fontWeight: 600 }}>{openPrice > 0 ? formatPrice(openPrice) : "—"}</span></span>
        <span style={{ fontSize: "12px", fontWeight: 700, color: moveColor }}>
          {movePct > 0 ? "+" : ""}{movePct.toFixed(3)}%
        </span>
      </div>

      {/* YES / NO comparison */}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "8px" }}>
        {[
          { label: "YES", fair: fairYes, clob: clobYes, edge: edgeYes },
          { label: "NO", fair: fairNo, clob: clobNo, edge: edgeNo },
        ].map(side => {
          const eColor = Math.abs(side.edge) > 0.02 ? "#2B8A3E" : Math.abs(side.edge) > 0.005 ? "#D97706" : inkDim;
          return (
            <div key={side.label} style={{ background: "rgba(255,255,255,0.03)", borderRadius: "6px", padding: "8px 10px" }}>
              <div style={{ fontFamily: mono, fontSize: "8px", fontWeight: 600, color: inkDim, textTransform: "uppercase", letterSpacing: "0.5px", marginBottom: "4px" }}>{side.label}</div>
              <div style={{ display: "flex", justifyContent: "space-between", fontFamily: mono, fontSize: "10px", marginBottom: "2px" }}>
                <span style={{ color: inkDim }}>fair</span>
                <span style={{ color: inkPrimary, fontWeight: 600 }}>{side.fair.toFixed(3)}</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", fontFamily: mono, fontSize: "10px", marginBottom: "2px" }}>
                <span style={{ color: inkDim }}>clob</span>
                <span style={{ color: inkPrimary, fontWeight: 600 }}>{side.clob.toFixed(3)}</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", fontFamily: mono, fontSize: "10px" }}>
                <span style={{ color: inkDim }}>edge</span>
                <span style={{ color: eColor, fontWeight: 700 }}>${Math.abs(side.edge).toFixed(3)}</span>
              </div>
            </div>
          );
        })}
      </div>

      {/* Window timer */}
      {timeLeft > 0 && (
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <div style={{ flex: 1, height: "4px", background: "rgba(255,255,255,0.08)", borderRadius: "2px", overflow: "hidden" }}>
            <div style={{ height: "100%", width: `${timerPct}%`, background: timeLeft < 30 ? "#D92525" : timeLeft < 60 ? "#D97706" : "#2B8A3E", borderRadius: "2px", transition: "width 1s linear" }} />
          </div>
          <span style={{ fontFamily: mono, fontSize: "10px", fontWeight: 600, color: timeLeft < 30 ? "#D92525" : timeLeft < 60 ? "#D97706" : inkDim, minWidth: "36px", textAlign: "right" }}>
            {timerMin}:{timerSec.toString().padStart(2, "0")}
          </span>
        </div>
      )}
    </div>
  );
}

export default function ArbDashboard() {
  const [isDark] = useState(true);
  const [soundOpen, setSoundOpen] = useState(false);
  const [perfView, setPerfView] = useState<"cumulative" | "scatter">("cumulative");
  const [isMobile, setIsMobile] = useState(() => typeof window !== 'undefined' && window.innerWidth < 768);
  useEffect(() => {
    const onResize = () => setIsMobile(window.innerWidth < 768);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  const { data: status } = useStatus();
  const { data: alertData } = useAlerts();
  const { data: tradesData } = useTrades();
  const now = useTick(1000);
  const sound = useSoundSettings();

  const alerts = useMemo(() => alertData?.alerts ?? [], [alertData]);
  const allTrades = useMemo(() => tradesData?.trades ?? status?.recent_trades ?? [], [tradesData, status]);

  const statusAge = status?.timestamp ? Date.now() / 1000 - status.timestamp : 9999;
  const healthy = statusAge < 30;
  const balance = status?.balance ?? 0;
  const seed = status?.seed ?? 0;
  const trades = status?.trades ?? { wins: 0, losses: 0, open: 0, total_pnl: 0, session_pnl: 0, daily_pnl: 0, avg_edge: 0, avg_latency_ms: 0 };
  const rawWindows: any[] = useMemo(() => status?.markets ?? (status as any)?.current_windows ?? [], [status]);
  const markets: ArbMarket[] = useMemo(() => {
    return rawWindows.map((w: any) => ({
      title: w.title ?? w.slug ?? `${(w.asset ?? "?").toUpperCase()}`,
      condition_id: w.condition_id ?? w.slug ?? "",
      fair_value: w.fair_value ?? w.fair_yes ?? 0,
      clob_mid: w.clob_mid ?? w.clob_yes_mid ?? 0,
      clob_best_bid: w.clob_best_bid ?? 0,
      clob_best_ask: w.clob_best_ask ?? 0,
      edge_pct: w.edge_pct ?? (w.edge_yes != null ? w.edge_yes * 100 : 0),
      divergence_open: w.divergence_open ?? false,
      divergence_since: w.divergence_since ?? null,
      state: w.state ?? "scanning",
    }));
  }, [rawWindows]);
  const rawFeeds = (status as any)?.feeds ?? {};
  const feedsConnected = rawFeeds.binance_connected !== undefined
    ? (rawFeeds.binance_connected ? 1 : 0)
    : Object.values(rawFeeds).filter((v: any) => v && typeof v === 'object' && v.connected).length;
  const feedsTotal = rawFeeds.binance_connected !== undefined
    ? 1
    : Math.max(1, Object.keys(rawFeeds).filter(k => !k.includes('_')).length);
  const feedLatency = (rawFeeds as any)?.binance_latency_ms ?? (trades as any)?.avg_latency_ms ?? 0;
  const dailyCap = status?.daily_cap ?? { limit: 0, used_pct: 0 };

  const edgeState = (() => {
    if (markets.some(m => m?.state === "executing")) return "EXECUTING";
    if (markets.some(m => m?.state === "divergence")) return "DIVERGENCE";
    if (markets.some(m => m?.state === "filled")) return "FILLED";
    return "SCANNING";
  })();
  const edgeColor = edgeState === "EXECUTING" ? "#D92525" : edgeState === "DIVERGENCE" ? "#D97706" : edgeState === "FILLED" ? "#2B8A3E" : "#2B8A3E";
  const edgePulse = edgeState === "EXECUTING" || edgeState === "DIVERGENCE";

  const sortedIndices = useMemo(() => {
    const order: Record<string, number> = { executing: 0, divergence: 1, filled: 2, converged: 3, monitoring: 4, scanning: 5 };
    const indices = markets.map((_, i) => i).filter(i => markets[i] != null);
    indices.sort((ai, bi) => {
      const a = markets[ai]; const b = markets[bi];
      const oa = order[a?.state] ?? 6;
      const ob = order[b?.state] ?? 6;
      if (oa !== ob) return oa - ob;
      if (a?.state === "divergence" && b?.state === "divergence") return (b?.edge_pct ?? 0) - (a?.edge_pct ?? 0);
      return (a?.title ?? "").localeCompare(b?.title ?? "");
    });
    return indices;
  }, [markets]);

  const liveAlerts = useMemo(() => {
    const fmtSlug = (slug: string): string => {
      const m = slug.match(/(\d+)m-(\d{9,})$/);
      if (!m) return slug;
      const durMin = parseInt(m[1]);
      const startTs = parseInt(m[2]);
      if (isNaN(startTs)) return slug;
      const s = new Date(startTs * 1000);
      const e = new Date((startTs + durMin * 60) * 1000);
      const tf = (d: Date) => d.toLocaleTimeString("en-US", { hour12: false, hour: "numeric", minute: "2-digit" });
      return `${durMin}m • ${tf(s)}–${tf(e)}`;
    };

    return alerts.slice(-150).reverse().map(a => {
      const cat = a.category ?? "";
      const raw = a.message ?? "";
      const isWin = cat.includes("converge");
      const isLoss = cat.includes("adverse");
      const isFill = cat.includes("fill");
      const isDiv = cat.includes("divergence");
      const color = isWin ? "#2B8A3E" : isLoss ? "#D92525" : isFill ? "#FF4D00" : isDiv ? "#D97706" : undefined;

      let message = raw;
      const windowOpenMatch = raw.match(/^(\w+)\s+window opened:\s*(.*)$/i);
      const windowCloseMatch = raw.match(/^(\w+)\s+window closed:\s*(.*)$/i);
      if (windowOpenMatch) {
        const rest = windowOpenMatch[2];
        const slugPrice = rest.match(/^(\S+)\s*@\s*(.+)$/);
        if (slugPrice) {
          message = `${windowOpenMatch[1]} 🎬 · ${fmtSlug(slugPrice[1])} @${slugPrice[2].trim()}`;
        } else {
          message = `${windowOpenMatch[1]} 🎬 · ${rest}`;
        }
      } else if (windowCloseMatch) {
        message = `${windowCloseMatch[1]} 🧤 · ${windowCloseMatch[2]}`;
      } else {
        message = raw
          .replace(/\bwindow opened\b/gi, "🎬")
          .replace(/\bwindow closed\b/gi, "🧤")
          .replace(/:\s*/g, " · ");
      }

      return { time: formatTime(a.timestamp), message, color, severity: a.severity };
    });
  }, [alerts]);

  const recentTrades = useMemo(() => {
    return (status?.recent_trades ?? []).slice(-90).reverse();
  }, [status]);

  const cumulativeData = useMemo(() => {
    let sum = 0;
    return allTrades.filter(t => t.pnl != null).map(t => {
      sum += t.pnl!;
      const d = new Date(t.timestamp * 1000);
      return {
        ts: t.timestamp,
        cumPnl: sum,
        pnl: t.pnl!,
        market: t.market,
        edge: t.edge_pct,
        won: t.outcome === "converged",
        dateStr: d.toLocaleDateString("en-US", { month: "short", day: "numeric" }).toUpperCase(),
        timeStr: d.toLocaleTimeString("en-US", { hour12: true, hour: "numeric", minute: "2-digit" }),
      };
    });
  }, [allTrades]);

  const topMarkets = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const t of (status?.recent_trades ?? [])) {
      counts[t.market] = (counts[t.market] ?? 0) + 1;
    }
    return Object.entries(counts).sort((a, b) => b[1] - a[1]).slice(0, 5);
  }, [status]);

  const tradesByOutcome = useMemo(() => {
    const out = { converged: 0, adverse: 0, open: 0 };
    for (const t of (status?.recent_trades ?? [])) {
      if (t.outcome in out) out[t.outcome as keyof typeof out]++;
    }
    return out;
  }, [status]);

  const bgChassis = "#0E0E10";
  const bgSurface = "#18181C";
  const inkPrimary = "#F0F0F2";
  const inkSecondary = "#9999AA";
  const inkTertiary = "#666677";
  const borderCol = "rgba(255,255,255,0.07)";
  const borderStrong = "rgba(255,255,255,0.14)";
  const shadowElevation = "0 1px 2px rgba(0,0,0,0.3), 0 4px 8px rgba(0,0,0,0.2)";
  const scoreboardBg = "#0A0A0C";
  const mono = "'JetBrains Mono', monospace";
  const panelStyle: React.CSSProperties = {
    background: bgSurface,
    border: `1px solid ${borderCol}`,
    borderRadius: "12px",
    padding: "16px",
    boxShadow: shadowElevation,
    display: "flex",
    flexDirection: "column",
    position: "relative",
    overflow: "hidden",
  };
  const labelStyle: React.CSSProperties = {
    fontFamily: mono,
    fontSize: "10px",
    textTransform: "uppercase",
    color: inkTertiary,
    letterSpacing: "0.5px",
    fontWeight: 600,
  };

  return (
    <div style={{ backgroundColor: bgChassis, color: inkPrimary, fontFamily: "'Inter', sans-serif", height: "100vh", overflow: isMobile ? "auto" : "hidden", display: "flex", justifyContent: "center", alignItems: isMobile ? "flex-start" : "center", WebkitFontSmoothing: "antialiased" }}>
      <div style={{ width: "100%", height: isMobile ? "auto" : "100%", minHeight: isMobile ? "100vh" : undefined, maxWidth: isMobile ? undefined : "1840px", maxHeight: isMobile ? undefined : "1150px", display: "grid", gridTemplateRows: isMobile ? "auto auto" : "auto 1fr", gap: "8px", padding: isMobile ? "8px" : "16px" }}>
        {/* Top Nav */}
        <header style={{ display: "grid", gridTemplateColumns: isMobile ? "1fr auto" : "auto 1fr auto", alignItems: "center", background: bgSurface, border: `1px solid ${borderCol}`, borderRadius: "12px", padding: "8px 16px", height: "64px", boxShadow: shadowElevation }}>
          <div style={{ fontFamily: mono, fontWeight: 700, fontSize: "14px", letterSpacing: "-0.5px", display: "flex", alignItems: "center", gap: "6px" }}>
            MONODEXR ARB <span style={{ fontWeight: 400, fontSize: "10px", color: inkTertiary }}>0.0.1</span>
            {isMobile && <StatusDot style={{ marginLeft: "4px", ...(healthy ? {} : { background: "#D92525" }) }} />}
          </div>
          {!isMobile && <div style={{ display: "flex", justifyContent: "center", alignItems: "center" }}>
            <div style={{ display: "flex", alignItems: "center", gap: "8px", fontSize: "10px", fontWeight: 600, textTransform: "uppercase", color: inkSecondary, background: "rgba(0,0,0,0.03)", padding: "4px 8px", borderRadius: "4px" }}>
              <StatusDot style={healthy ? {} : { background: "#D92525" }} />
              {healthy ? "ONLINE" : "OFFLINE"}
            </div>
          </div>}
          <div style={{ display: "flex", alignItems: "center", gap: "8px", position: "relative" }}>
            <button onClick={() => setSoundOpen(p => !p)} style={{ width: "32px", height: "32px", border: `1px solid ${sound.globalMuted ? "#FFC700" : borderStrong}`, borderRadius: "50%", display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", color: sound.globalMuted ? "#FFC700" : inkSecondary, background: "linear-gradient(145deg, #222226, #18181C)" }}>
              <svg width="14" height="14" fill="currentColor" viewBox="0 0 24 24"><path d="M7 9v6h4l5 5V4l-5 5H7z" /></svg>
            </button>
            {soundOpen && isMobile && <div onClick={() => setSoundOpen(false)} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.4)", zIndex: 199 }} />}
            {soundOpen && (
              <div style={isMobile ? { position: "fixed", bottom: 0, left: 0, right: 0, background: bgSurface, border: `1px solid ${borderCol}`, borderRadius: "12px 12px 0 0", boxShadow: "0 -8px 24px rgba(0,0,0,0.3)", padding: "16px", zIndex: 200, maxHeight: "70vh", overflowY: "auto" } : { position: "absolute", top: "44px", right: 0, width: "260px", background: bgSurface, border: `1px solid ${borderCol}`, borderRadius: "10px", boxShadow: "0 8px 24px rgba(0,0,0,0.3)", padding: "14px", zIndex: 100 }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: "12px" }}>
                  <span style={{ fontFamily: mono, fontSize: "10px", fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.5px", color: inkPrimary }}>Sound Effects</span>
                  <button onClick={() => sound.globalMuted ? sound.toggleGlobalMute() : sound.muteAll()} style={{ fontFamily: mono, fontSize: "9px", color: inkTertiary, textTransform: "uppercase", background: "none", border: "none", cursor: "pointer" }}>
                    {sound.globalMuted ? "Unmute" : "Mute All"}
                  </button>
                </div>
                {sound.channels.map(ch => (
                  <div key={ch.id} style={{ display: "flex", alignItems: "center", gap: "10px", marginBottom: "10px" }}>
                    <button onClick={() => sound.setChannel(ch.id, { enabled: !ch.enabled })} style={{ width: "18px", height: "18px", borderRadius: "3px", border: `1.5px solid ${ch.enabled ? "#FF4D00" : borderStrong}`, background: ch.enabled ? "#FF4D00" : "transparent", cursor: "pointer", flexShrink: 0 }} />
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontFamily: mono, fontSize: "10px", fontWeight: 600, color: ch.enabled ? inkPrimary : inkTertiary, marginBottom: "3px" }}>{ch.label}</div>
                      <input type="range" min="0" max="100" value={ch.volume} onChange={e => sound.setChannel(ch.id, { volume: parseInt(e.target.value) })} style={{ width: "100%", height: "3px", accentColor: "#FF4D00", opacity: ch.enabled ? 1 : 0.3 }} />
                    </div>
                    <span style={{ fontFamily: mono, fontSize: "10px", color: inkTertiary, width: "24px", textAlign: "right" }}>{ch.volume}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </header>

        {/* Dashboard Grid */}
        <div style={{ display: isMobile ? "flex" : "grid", flexDirection: isMobile ? "column" : undefined, gridTemplateColumns: isMobile ? undefined : "280px 1fr 320px", gap: "8px", height: isMobile ? "auto" : "100%", overflow: isMobile ? "visible" : "hidden" }}>

          {/* Left: Event Log */}
          <aside style={{ ...panelStyle, gap: "8px", minHeight: 0, order: isMobile ? 2 : undefined, maxHeight: isMobile ? "300px" : undefined }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", paddingBottom: "8px", borderBottom: `1px solid ${borderCol}`, flexShrink: 0 }}>
              <span style={labelStyle}>Event Log</span>
              <span style={labelStyle}>LIVE</span>
            </div>
            <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
              {liveAlerts.map((entry, i) => (
                <div key={i} style={{ fontFamily: mono, fontSize: "11px", padding: "6px 0", borderBottom: i < liveAlerts.length - 1 ? `1px dashed ${borderCol}` : "none", display: "grid", gridTemplateColumns: "40px 1fr", gap: "8px", color: entry.color ?? inkSecondary }}>
                  <span style={{ color: inkTertiary }}>{entry.time}</span>
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{entry.message}</span>
                </div>
              ))}
              {liveAlerts.length === 0 && <div style={{ fontFamily: mono, fontSize: "11px", padding: "8px 0", color: inkTertiary }}>Waiting for events...</div>}
            </div>
          </aside>

          {/* Center */}
          <main style={{ display: "flex", flexDirection: "column", gap: "8px", overflow: isMobile ? "visible" : "hidden", order: isMobile ? 1 : undefined }}>
            {/* Scoreboard */}
            <div style={{ display: "grid", gridTemplateColumns: isMobile ? "1fr" : "1fr 1px 1fr 1px 1.2fr", background: scoreboardBg, color: "#F2F2F3", minHeight: isMobile ? undefined : "100px", borderRadius: "12px", overflow: "hidden", border: `1px solid ${borderCol}`, boxShadow: shadowElevation }}>
              {/* Balance */}
              <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", padding: "10px 16px", gap: "4px" }}>
                <span style={{ ...labelStyle, color: "rgba(128,128,140,0.7)" }}>Balance</span>
                <span style={{ fontFamily: mono, fontSize: "22px", fontWeight: 400, lineHeight: 1, letterSpacing: "-0.5px" }}>{formatUsd(balance)}</span>
                <span style={{ fontFamily: mono, fontSize: "9px", color: "rgba(255,255,255,0.35)" }}>seed: {formatUsd(seed)}</span>
              </div>
              {!isMobile && <div style={{ width: "1px", background: "rgba(255,255,255,0.1)", margin: "10px 0" }} />}
              {isMobile && <div style={{ height: "1px", background: "rgba(255,255,255,0.08)", margin: "0 16px" }} />}
              {/* Trades */}
              <div style={{ display: "flex", flexDirection: "column", padding: "10px 14px", gap: "3px", justifyContent: "center" }}>
                <span style={{ ...labelStyle, color: "rgba(128,128,140,0.7)" }}>Trades</span>
                <div style={{ display: "flex", flexDirection: "column", gap: "2px", fontFamily: mono, fontSize: "12px", color: "rgba(255,255,255,0.85)" }}>
                  {[
                    { color: "#2B8A3E", label: `${trades.wins} Wins` },
                    { color: "#D92525", label: `${trades.losses} Losses` },
                    { color: "rgba(255,255,255,0.4)", label: `${trades.open} Open` },
                  ].map((item, i) => (
                    <div key={i} style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                      <span style={{ width: "7px", height: "7px", borderRadius: "50%", background: item.color, flexShrink: 0 }} />
                      {item.label}
                    </div>
                  ))}
                </div>
              </div>
              {!isMobile && <div style={{ width: "1px", background: "rgba(255,255,255,0.1)", margin: "10px 0" }} />}
              {isMobile && <div style={{ height: "1px", background: "rgba(255,255,255,0.08)", margin: "0 16px" }} />}
              {/* Edge Status */}
              <div style={{ display: "flex", flexDirection: "column", padding: "10px 14px", gap: "5px", justifyContent: "center" }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <div style={{ background: edgeColor, color: edgeState === "DIVERGENCE" ? "#000" : "#fff", fontFamily: mono, fontSize: "10px", fontWeight: 700, padding: "2px 8px", borderRadius: "2px", letterSpacing: "0.5px", animation: edgePulse ? "flash-chip 1.2s infinite" : undefined }}>
                    {edgeState}
                  </div>
                </div>
                <div style={{ fontFamily: mono, fontSize: "9px", color: "rgba(255,255,255,0.5)", display: "flex", gap: "10px" }}>
                  <span>feeds: <span style={{ color: feedsConnected === feedsTotal && feedsTotal > 0 ? "#2B8A3E" : "#D92525", fontWeight: 600 }}>{feedsConnected}/{feedsTotal || "—"}</span></span>
                  <span>latency: <span style={{ fontWeight: 600, color: "rgba(255,255,255,0.7)" }}>{feedLatency > 0 ? `${feedLatency}ms` : "—"}</span></span>
                </div>
                <div style={{ fontFamily: mono, fontSize: "9px", color: "rgba(255,255,255,0.5)" }}>
                  BTC: <span style={{ fontWeight: 600, color: "rgba(255,255,255,0.7)" }}>{formatPrice(status?.spot_price ?? 0)}</span>
                  {(status?.implied_vol ?? 0) > 0 && <span> | IV: <span style={{ fontWeight: 600, color: "rgba(255,255,255,0.7)" }}>{Math.round((status?.implied_vol ?? 0) * 100)}%</span></span>}
                </div>
              </div>
            </div>

            {/* Divergence Board */}
            <div style={{ ...panelStyle, flex: sortedIndices.length > 0 ? 1 : undefined, minHeight: sortedIndices.length > 0 ? 0 : "120px", overflow: "auto" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", paddingBottom: "8px", marginBottom: "10px", borderBottom: `1px solid ${borderCol}`, flexShrink: 0 }}>
                <span style={labelStyle}>Divergence Board</span>
                <span style={labelStyle}>{markets.length} Markets</span>
              </div>
              {sortedIndices.length > 0 ? (
                <div style={{ display: "grid", gridTemplateColumns: isMobile ? "1fr" : "1fr 1fr", gap: "10px" }}>
                  {sortedIndices.map(i => <ContractCard key={markets[i].condition_id || markets[i].title} market={markets[i]} raw={rawWindows[i]} now={now} isDark={isDark} />)}
                </div>
              ) : (
                <div style={{ fontFamily: mono, fontSize: "11px", color: inkTertiary, textAlign: "center", padding: "20px 0" }}>{healthy ? "Scanning — waiting for next window" : "No markets monitored"}</div>
              )}
            </div>

            {/* Performance Vector */}
            <div style={{ ...panelStyle, padding: "10px 14px", flex: 1, minHeight: "140px" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px", flexShrink: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
                  <span style={labelStyle}>Performance Vector</span>
                  <div style={{ display: "flex", gap: "4px" }}>
                    {(["cumulative", "scatter"] as const).map(v => (
                      <button key={v} onClick={() => setPerfView(v)} style={{ fontFamily: mono, fontSize: "8px", fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.5px", padding: "2px 6px", borderRadius: "3px", border: `1px solid ${perfView === v ? "#FF4D00" : borderStrong}`, background: perfView === v ? "#FF4D00" : "transparent", color: perfView === v ? "#fff" : inkTertiary, cursor: "pointer" }}>
                        {v}
                      </button>
                    ))}
                  </div>
                </div>
                <span style={labelStyle}>{allTrades.length} Trades</span>
              </div>
              <div style={{ position: "relative", flex: 1, minHeight: 0, background: isDark ? "rgba(255,255,255,0.03)" : "rgba(0,0,0,0.03)", borderRadius: "4px" }}>
                {cumulativeData.length >= 2 ? (
                  <svg width="100%" height="100%" viewBox="0 0 800 200" preserveAspectRatio="none" style={{ position: "absolute", inset: 0 }}>
                    {(() => {
                      const data = cumulativeData;
                      const vals = perfView === "cumulative" ? data.map(d => d.cumPnl) : data.map(d => d.pnl);
                      const mn = Math.min(0, ...vals);
                      const mx = Math.max(0.01, ...vals);
                      const range = mx - mn || 1;
                      const toY = (v: number) => 10 + 180 - ((v - mn) / range) * 180;
                      const toX = (i: number) => (i / Math.max(1, data.length - 1)) * 780 + 10;
                      const zeroY = toY(0);

                      if (perfView === "cumulative") {
                        const pts = data.map((d, i) => `${toX(i)},${toY(d.cumPnl)}`).join(" ");
                        const lastVal = data[data.length - 1]?.cumPnl ?? 0;
                        const lineColor = lastVal >= 0 ? "#2B8A3E" : "#D92525";
                        return (
                          <>
                            <line x1="10" y1={zeroY} x2="790" y2={zeroY} stroke="rgba(255,255,255,0.08)" strokeWidth="1" strokeDasharray="4 4" />
                            <polyline points={pts} fill="none" stroke={lineColor} strokeWidth="2" strokeLinecap="round" />
                            <text x="795" y={toY(lastVal)} fill={lineColor} fontSize="10" fontFamily={mono} fontWeight="700" textAnchor="end" dominantBaseline="middle">
                              {formatUsd(lastVal, { sign: true })}
                            </text>
                          </>
                        );
                      } else {
                        return (
                          <>
                            <line x1="10" y1={zeroY} x2="790" y2={zeroY} stroke="rgba(255,255,255,0.08)" strokeWidth="1" strokeDasharray="4 4" />
                            {data.map((d, i) => (
                              <circle key={i} cx={toX(i)} cy={toY(d.pnl)} r="3" fill={d.won ? "#2B8A3E" : "#D92525"} opacity="0.8" />
                            ))}
                          </>
                        );
                      }
                    })()}
                  </svg>
                ) : (
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%", fontFamily: mono, fontSize: "11px", color: inkTertiary }}>Collecting trade data...</div>
                )}
              </div>
            </div>
          </main>

          {/* Right Column */}
          <aside style={{ display: "flex", flexDirection: "column", gap: "8px", minHeight: 0, overflow: isMobile ? "visible" : "hidden", order: isMobile ? 3 : undefined }}>
            {/* Session Data */}
            <div style={{ ...panelStyle, gap: 0 }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", paddingBottom: "10px", marginBottom: "12px", borderBottom: `1px solid ${borderCol}` }}>
                <span style={labelStyle}>Session Data</span>
                <span style={{ fontFamily: mono, fontSize: "9px", color: healthy ? "#2B8A3E" : "#D92525", textTransform: "uppercase", letterSpacing: "0.5px", display: "flex", alignItems: "center", gap: "5px" }}>
                  <StatusDot style={{ width: "6px", height: "6px", background: healthy ? "#2B8A3E" : "#D92525" }} />
                  {healthy ? "LIVE" : "OFFLINE"}
                </span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
                <span style={{ fontFamily: mono, fontSize: "9px", color: inkTertiary, textTransform: "uppercase", letterSpacing: "0.5px" }}>Session P&L</span>
                <span style={{ fontFamily: mono, fontSize: "28px", fontWeight: 700, color: trades.session_pnl >= 0 ? "#2B8A3E" : "#D92525", letterSpacing: "-1.5px", lineHeight: 1 }}>
                  {formatUsd(trades.session_pnl, { sign: true })}
                </span>
              </div>
              <div style={{ height: "1px", background: borderCol, width: "100%", margin: "10px 0" }} />
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
                  <span style={{ fontFamily: mono, fontSize: "9px", color: inkTertiary, textTransform: "uppercase" }}>Total P&L</span>
                  <span style={{ fontFamily: mono, fontSize: "16px", fontWeight: 600, color: trades.total_pnl >= 0 ? "#2B8A3E" : "#D92525" }}>{formatUsd(trades.total_pnl, { sign: true })}</span>
                </div>
                <div style={{ width: "1px", background: borderCol }} />
                <div style={{ display: "flex", flexDirection: "column", gap: "2px", alignItems: "flex-end" }}>
                  <span style={{ fontFamily: mono, fontSize: "9px", color: inkTertiary, textTransform: "uppercase" }}>Deposited</span>
                  <span style={{ fontFamily: mono, fontSize: "16px", fontWeight: 600, color: inkSecondary }}>{formatUsd(seed)}</span>
                </div>
              </div>
              {dailyCap.limit > 0 && (
                <>
                  <div style={{ height: "1px", background: borderCol, width: "100%", margin: "10px 0" }} />
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: "4px" }}>
                    <span style={{ fontFamily: mono, fontSize: "9px", color: inkTertiary, textTransform: "uppercase" }}>Daily P&L</span>
                    <span style={{ fontFamily: mono, fontSize: "9px", color: inkTertiary }}>{formatUsd(trades.daily_pnl, { sign: true })} / {formatUsd(dailyCap.limit)}</span>
                  </div>
                  <div style={{ height: "4px", background: "rgba(255,255,255,0.1)", borderRadius: "2px", overflow: "hidden" }}>
                    <div style={{ height: "100%", width: `${Math.min(100, dailyCap.used_pct)}%`, background: dailyCap.used_pct > 75 ? "#D92525" : dailyCap.used_pct > 50 ? "#D97706" : "#2B8A3E", transition: "width 0.5s" }} />
                  </div>
                </>
              )}
            </div>

            {/* Trade Breakdown */}
            <div style={{ ...panelStyle, padding: "12px 14px" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", paddingBottom: "8px", marginBottom: "10px", borderBottom: `1px solid ${borderCol}` }}>
                <span style={labelStyle}>Trade Breakdown</span>
                <span style={labelStyle}>{trades.wins + trades.losses + trades.open} trades</span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
                <span style={{ fontFamily: mono, fontSize: "8px", color: inkTertiary, textTransform: "uppercase", letterSpacing: "0.5px" }}>By Outcome</span>
                {[
                  { name: "converged", count: tradesByOutcome.converged, color: "#2B8A3E" },
                  { name: "adverse", count: tradesByOutcome.adverse, color: "#D92525" },
                  { name: "open", count: tradesByOutcome.open, color: "#D97706" },
                ].filter(r => r.count > 0).map(r => {
                  const max = Math.max(1, tradesByOutcome.converged, tradesByOutcome.adverse, tradesByOutcome.open);
                  return (
                    <div key={r.name} style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                      <span style={{ fontFamily: mono, fontSize: "10px", fontWeight: 600, color: inkPrimary, width: "72px", flexShrink: 0 }}>{r.name}</span>
                      <div style={{ flex: 1, height: "4px", background: borderStrong, borderRadius: "2px", overflow: "hidden" }}>
                        <div style={{ height: "100%", width: `${(r.count / max) * 100}%`, background: r.color, transition: "width 0.5s" }} />
                      </div>
                      <span style={{ fontFamily: mono, fontSize: "10px", fontWeight: 600, color: inkPrimary, width: "24px", textAlign: "right" }}>{r.count}</span>
                    </div>
                  );
                })}
                {topMarkets.length > 0 && (
                  <>
                    <span style={{ fontFamily: mono, fontSize: "8px", color: inkTertiary, textTransform: "uppercase", letterSpacing: "0.5px", marginTop: "6px" }}>Top Markets</span>
                    {topMarkets.map(([market, count]) => (
                      <div key={market} style={{ display: "flex", justifyContent: "space-between", fontFamily: mono, fontSize: "10px" }}>
                        <span style={{ color: inkSecondary, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "180px" }}>{market}</span>
                        <span style={{ color: inkPrimary, fontWeight: 600, flexShrink: 0, marginLeft: "8px" }}>{count}</span>
                      </div>
                    ))}
                  </>
                )}
              </div>
            </div>

            {/* Stats */}
            <div style={{ ...panelStyle }}>
              <div style={{ display: "flex", alignItems: "baseline", gap: "8px", marginBottom: "12px", paddingBottom: "8px", borderBottom: `1px solid ${borderCol}` }}>
                <span style={labelStyle}>Stats</span>
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "10px" }}>
                {[
                  { val: trades.wins + trades.losses > 0 ? `${Math.round((trades.wins / (trades.wins + trades.losses)) * 100)}%` : "—", lbl: "Win Rate" },
                  { val: feedLatency > 0 ? `${feedLatency}ms` : "—", lbl: "Latency" },
                  { val: (trades as any)?.avg_edge > 0 ? `${Number((trades as any).avg_edge).toFixed(1)}%` : "—", lbl: "Avg Edge" },
                  { val: feedsTotal > 0 ? `${feedsConnected}/${feedsTotal}` : "—", lbl: "Feeds" },
                ].map((item, i) => (
                  <div key={i} style={{ background: "transparent", padding: "8px", borderRadius: "4px", border: `1px solid ${borderStrong}` }}>
                    <span style={{ fontFamily: mono, fontSize: "16px", fontWeight: 700, color: inkPrimary, display: "block" }}>{item.val}</span>
                    <span style={{ fontSize: "9px", color: inkTertiary, textTransform: "uppercase" }}>{item.lbl}</span>
                  </div>
                ))}
              </div>
            </div>

            {/* Recent Trades */}
            <div style={{ ...panelStyle, flex: 1, minHeight: 0, overflow: "hidden" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", paddingBottom: "8px", marginBottom: "8px", borderBottom: `1px solid ${borderCol}`, flexShrink: 0 }}>
                <span style={labelStyle}>Recent Trades</span>
              </div>
              <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
                <div style={{ display: "grid", gridTemplateColumns: "40px 1fr 48px 56px", fontFamily: mono, fontSize: "9px", padding: "4px 0", borderBottom: `1px solid ${borderCol}`, color: inkTertiary, textTransform: "uppercase" }}>
                  <span>Time</span>
                  <span>Market</span>
                  <span>Edge</span>
                  <span style={{ textAlign: "right" }}>P&L</span>
                </div>
                {recentTrades.map((t, i) => (
                  <div key={i} style={{ display: "grid", gridTemplateColumns: "40px 1fr 48px 56px", fontFamily: mono, fontSize: "11px", padding: "5px 0", borderBottom: `1px solid ${borderCol}`, color: inkPrimary }}>
                    <span style={{ color: inkTertiary }}>{formatTime(t.timestamp)}</span>
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{t.market}</span>
                    <span style={{ color: t.edge_pct > 0 ? "#2B8A3E" : inkTertiary }}>{formatEdge(t.edge_pct)}</span>
                    <span style={{ textAlign: "right", color: (t.pnl ?? 0) >= 0 ? "#2B8A3E" : "#D92525" }}>{t.pnl != null ? formatUsd(t.pnl, { sign: true }) : "—"}</span>
                  </div>
                ))}
                {recentTrades.length === 0 && <div style={{ fontFamily: mono, fontSize: "11px", padding: "8px 0", color: inkTertiary }}>No trades yet</div>}
              </div>
            </div>
          </aside>
        </div>
      </div>
    </div>
  );
}
