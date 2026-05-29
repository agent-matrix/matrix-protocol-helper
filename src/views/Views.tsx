/* ============================================================
   Views.tsx — Home (environment), Logs, Settings, Installs
   ============================================================ */
import { useEffect, useRef } from "react";
import type { CSSProperties } from "react";
import { CI, IC } from "../icons";
import { persist } from "../store";
import type { EnvState, InstallReq, LogLine, RecentInstall, Settings } from "../store";

type InstallReqLite = InstallReq;

/* ---------- Environment dashboard (Home) ---------- */
export function HomeView({
  env,
  onTest,
  onInstallCli,
  recent,
  onOpenLogs,
  busy,
  cliVer,
  pythonVer,
  hubMs,
}: {
  env: EnvState;
  onTest: () => void;
  onInstallCli: () => void;
  recent: RecentInstall[];
  onOpenLogs: () => void;
  busy: boolean;
  cliVer: string | null;
  pythonVer: string | null;
  hubMs: number | null;
}) {
  const rows = [
    { key: "cli", ic: IC.cli, nm: "Matrix CLI", ds: "Backend that installs & runs components", val: env.cli ? cliVer || "installed" : "not installed", on: env.cli },
    { key: "python", ic: IC.python, nm: "Python runtime", ds: "Required for runner environments", val: env.python ? pythonVer || "available" : "missing", on: env.python },
    { key: "proto", ic: IC.link, nm: "Protocol links", ds: "Handles matrix:// one-click installs", val: env.proto ? "matrix://" : "disabled", on: env.proto },
    { key: "hub", ic: IC.cloud, nm: "Hub connection", ds: "api.matrixhub.io", val: env.hub ? (hubMs != null ? `${hubMs}ms` : "online") : "offline", on: env.hub },
  ];
  const ready = env.cli && env.python && env.proto && env.hub;
  return (
    <div className="main-inner rise">
      <p className="eyebrow">Local runtime</p>
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 16, flexWrap: "wrap" }}>
        <h1 className="h1">Environment</h1>
        <span className={"pill" + (ready ? " ok" : "")}>
          <span className={"dot " + (ready ? "on" : "warn")} /> {ready ? "Ready" : "Action needed"}
        </span>
      </div>
      <p className="lead">
        MatrixHub Client prepares your machine so MatrixHub installs are one click. The Matrix CLI is the local backend — it
        installs automatically if missing.
      </p>

      <div className="card" style={{ marginTop: 22 }}>
        {rows.map((r) => (
          <div className="statrow" key={r.key}>
            <span className="ic" style={r.on ? undefined : { color: "var(--ink-4)", background: "rgba(120,200,160,0.03)" }}>
              <CI d={r.ic} size={17} />
            </span>
            <div style={{ minWidth: 0 }}>
              <div className="nm">{r.nm}</div>
              <div className="ds">{r.ds}</div>
            </div>
            <span className="rt">
              <span className="mono" style={{ color: r.on ? "var(--ink-2)" : "var(--ink-4)" }}>{r.val}</span>
              {r.on ? (
                <span style={{ display: "inline-flex", color: "var(--acc)" }}>
                  <CI d={IC.check} size={16} sw={2.4} />
                </span>
              ) : (
                <span className="dot off" />
              )}
            </span>
          </div>
        ))}
      </div>

      {!env.cli && (
        <div
          className="card"
          style={{ marginTop: 16, padding: 18, display: "flex", gap: 14, alignItems: "center", borderColor: "rgba(245,185,69,0.25)", background: "rgba(245,185,69,0.05)" }}
        >
          <span style={{ display: "inline-flex", color: "var(--amber)" }}>
            <CI d={IC.spark} size={20} />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontWeight: 700, fontSize: 14 }}>Finish setup</div>
            <div style={{ fontSize: 12.5, color: "var(--ink-3)", marginTop: 2 }}>Matrix CLI is required to install and run components locally.</div>
          </div>
          <button className="btn btn-primary" onClick={onInstallCli} disabled={busy}>
            <CI d={IC.download} size={16} /> Install Matrix CLI
          </button>
        </div>
      )}

      <div style={{ display: "flex", gap: 12, marginTop: 22, flexWrap: "wrap" }}>
        <button className="btn btn-ghost" onClick={onTest} disabled={busy}>
          <CI d={IC.refresh} size={16} /> Test connection
        </button>
        <button className="btn btn-ghost" onClick={onOpenLogs}>
          <CI d={IC.logs} size={16} /> Open logs
        </button>
      </div>

      <p className="eyebrow" style={{ marginTop: 34 }}>Recent installs</p>
      <div className="card" style={{ marginTop: 12 }}>
        {recent.length === 0 && (
          <div style={{ padding: 22, textAlign: "center", color: "var(--ink-3)", fontSize: 13.5 }}>No components installed yet.</div>
        )}
        {recent.map((c, i) => (
          <div className="statrow" key={i}>
            <span className="ic">
              <CI d={IC.box} size={16} />
            </span>
            <div style={{ minWidth: 0 }}>
              <div className="nm">{c.name}</div>
              <div className="ds mono">{c.id}</div>
            </div>
            <span className="rt">
              <span className="dot on" /> installed
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ---------- Logs view ---------- */
export function LogsView({ lines }: { lines: LogLine[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [lines]);
  const palette: Record<string, string> = {
    ok: "var(--acc-bright)",
    dim: "var(--ink-3)",
    warn: "var(--amber)",
    err: "var(--red)",
    info: "var(--ink-2)",
  };
  return (
    <div className="main-inner rise">
      <p className="eyebrow">Activity</p>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
        <h1 className="h1">Logs</h1>
        <span className="pill mono">client.log</span>
      </div>
      <div className="logbox" ref={ref} style={{ marginTop: 20, height: 420 }}>
        {lines.map((l, i) => (
          <div key={i} style={{ color: palette[l.t] || "var(--ink-2)", whiteSpace: "pre-wrap" }}>
            <span style={{ color: "var(--ink-4)" }}>{l.ts}&nbsp;&nbsp;</span>
            {l.m}
          </div>
        ))}
        <span className="cur" />
      </div>
    </div>
  );
}

/* ---------- Settings view ---------- */
const toggleRows: [keyof Settings, string, string][] = [
  ["autoCli", "Auto-install Matrix CLI", "Install or update the CLI automatically when missing or outdated."],
  ["approve", "Ask approval before install", "Always confirm before running an install command."],
  ["sidebarRain", "Sidebar rain animation", "Show the Matrix code-rain at the top of the sidebar."],
  ["autostart", "Launch at login", "Start MatrixHub Client in the background when you sign in."],
];

export function SettingsView({ cfg, setCfg }: { cfg: Settings; setCfg: (fn: (c: Settings) => Settings) => void }) {
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) =>
    setCfg((c) => {
      const n = { ...c, [k]: v };
      persist({ settings: n });
      return n;
    });
  const inputStyle: CSSProperties = {
    width: "100%",
    height: 42,
    padding: "0 13px",
    fontSize: 14,
    color: "var(--ink)",
    background: "var(--inset)",
    border: "1px solid var(--line-2)",
    borderRadius: "var(--r-sm)",
    outline: "none",
    fontFamily: "var(--mono)",
  };
  return (
    <div className="main-inner rise">
      <p className="eyebrow">Client</p>
      <h1 className="h1">Settings</h1>
      <p className="lead">Configure how MatrixHub Client connects and installs on this machine.</p>
      <div style={{ display: "grid", gap: 18, marginTop: 24 }}>
        <label>
          <div style={{ fontSize: 13, fontWeight: 700, color: "var(--ink-2)", marginBottom: 7 }}>Hub URL</div>
          <input style={inputStyle} value={cfg.hubUrl} onChange={(e) => set("hubUrl", e.target.value)} spellCheck={false} />
        </label>
        <label>
          <div style={{ fontSize: 13, fontWeight: 700, color: "var(--ink-2)", marginBottom: 7 }}>Install location</div>
          <input style={inputStyle} value={cfg.dir} onChange={(e) => set("dir", e.target.value)} spellCheck={false} />
        </label>
        {toggleRows.map(([k, nm, ds]) => {
          const val = k === "sidebarRain" ? cfg[k] !== false : Boolean(cfg[k]);
          return (
            <div
              key={k}
              style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 14, padding: "13px 0", borderTop: "1px solid var(--line)" }}
            >
              <div>
                <div style={{ fontSize: 14, fontWeight: 600 }}>{nm}</div>
                <div style={{ fontSize: 12.5, color: "var(--ink-3)", marginTop: 2 }}>{ds}</div>
              </div>
              <button
                onClick={() => set(k, !val as Settings[typeof k])}
                aria-pressed={val}
                style={{ width: 42, height: 24, borderRadius: 99, padding: 2, flexShrink: 0, background: val ? "var(--acc)" : "var(--line-3)", transition: "background .15s" }}
              >
                <span
                  style={{ display: "block", width: 20, height: 20, borderRadius: 99, background: "#fff", transform: val ? "translateX(18px)" : "translateX(0)", transition: "transform .15s", boxShadow: "0 1px 3px rgba(0,0,0,0.3)" }}
                />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/* ---------- Installs catalogue (within client) ---------- */
// Real, installable registry entities (verified against the matrix-cli docs).
// "Hello SSE Server" is the official install smoke-test.
const AVAILABLE = [
  {
    name: "Hello SSE Server",
    id: "mcp_server:hello-sse-server@0.1.0",
    initials: "HS",
    cmd: "matrix install mcp_server:hello-sse-server@0.1.0",
    ds: "Minimal MCP server — the official one-click install smoke-test.",
  },
  {
    name: "watsonx Agent",
    id: "mcp_server:watsonx-agent@0.1.0",
    initials: "WX",
    cmd: "matrix install mcp_server:watsonx-agent@0.1.0",
    ds: "IBM watsonx-powered chat agent, served over MCP.",
  },
];

export function InstallsView({
  recent,
  pending,
  onRequest,
  onReview,
  onDecline,
}: {
  recent: RecentInstall[];
  pending: InstallReqLite[];
  onRequest: (c: { name: string; id: string; initials: string; cmd: string }) => void;
  onReview: (req: InstallReqLite) => void;
  onDecline: (req: InstallReqLite) => void;
}) {
  const has = (id: string) => recent.some((r) => r.id === id);
  return (
    <div className="main-inner rise">
      <p className="eyebrow">Local components</p>
      <h1 className="h1">Installs</h1>
      <p className="lead">Components installed into your local Matrix environment. Install new ones from MatrixHub with one click.</p>

      {pending.length > 0 && (
        <>
          <p className="eyebrow" style={{ marginTop: 26, color: "var(--amber)" }}>
            Install requests · {pending.length}
          </p>
          <div className="card" style={{ marginTop: 12, borderColor: "rgba(245,185,69,0.25)", background: "rgba(245,185,69,0.04)" }}>
            {pending.map((p) => (
              <div className="statrow" key={p.id}>
                <span className="ic" style={{ color: "var(--amber)", background: "rgba(245,185,69,0.08)" }}>
                  <CI d={IC.bell} size={16} />
                </span>
                <div style={{ minWidth: 0 }}>
                  <div className="nm">{p.name}</div>
                  <div className="ds mono">{p.id}{p.hub ? ` · from ${p.hub}` : ""}</div>
                </div>
                <span className="rt" style={{ gap: 8 }}>
                  <button className="btn btn-primary" style={{ height: 34, fontSize: 12.5, padding: "0 13px" }} onClick={() => onReview(p)}>
                    Review
                  </button>
                  <button className="btn btn-ghost" style={{ height: 34, fontSize: 12.5, padding: "0 13px" }} onClick={() => onDecline(p)}>
                    Decline
                  </button>
                </span>
              </div>
            ))}
          </div>
        </>
      )}

      <p className="eyebrow" style={{ marginTop: 26 }}>From MatrixHub</p>
      <div className="card" style={{ marginTop: 12 }}>
        {AVAILABLE.map((c) => (
          <div className="statrow" key={c.id}>
            <span className="ic">
              <CI d={IC.box} size={16} />
            </span>
            <div style={{ minWidth: 0 }}>
              <div className="nm">{c.name}</div>
              <div className="ds">{c.ds}</div>
            </div>
            <span className="rt">
              {has(c.id) ? (
                <span className="pill ok">
                  <span className="dot on" /> installed
                </span>
              ) : (
                <button className="btn btn-ghost" style={{ height: 34, fontSize: 12.5, padding: "0 13px" }} onClick={() => onRequest(c)}>
                  <CI d={IC.download} size={14} /> Install
                </button>
              )}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
