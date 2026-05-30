/* ============================================================
   Flows.tsx — install-approval modal + first-run setup wizard
   ============================================================ */
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CI, IC } from "../icons";
import { getCliStatus, installCli, installComponent, installPython, openUrl, relaunchApp, testHub } from "../lib/tauri";

const PYTHON_DOWNLOAD_URL = "https://www.python.org/downloads/";
import type { InstallReq, LogTone } from "../store";

type Logger = (t: LogTone, m: string) => void;

/* ---------- Install approval modal (incoming matrix:// request) ---------- */
export function InstallFlow({
  req,
  cliNeeded,
  onClose,
  onDone,
  log,
}: {
  req: InstallReq;
  cliNeeded: boolean;
  onClose: () => void;
  onDone: (req: InstallReq) => void;
  log: Logger;
}) {
  const [phase, setPhase] = useState<"confirm" | "installing" | "done" | "error">("confirm");
  const [lines, setLines] = useState<string[]>([]);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [lines]);

  async function begin() {
    setPhase("installing");
    setLines([]);
    log("info", `install requested → ${req.id}`);
    try {
      const code = await installComponent(req, (line) => {
        setLines((L) => [...L, line]);
        log("dim", line);
      });
      if (code === 0) {
        setPhase("done");
        log("ok", `✓ installed ${req.id}`);
      } else {
        setPhase("error");
        log("err", `install failed (exit ${code}) · ${req.id}`);
      }
    } catch (e) {
      setLines((L) => [...L, String(e)]);
      setPhase("error");
      log("err", `install error · ${String(e)}`);
    }
  }

  return createPortal(
    <div
      className="scrim"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && phase !== "installing") onClose();
      }}
    >
      <div className="rise" style={{ width: "100%", maxWidth: 480, background: "var(--app-2)", border: "1px solid var(--line-2)", borderRadius: "var(--r-lg)", overflow: "hidden", boxShadow: "0 30px 80px rgba(0,0,0,0.6)" }}>
        {/* header */}
        <div style={{ padding: "16px 20px", borderBottom: "1px solid var(--line)", display: "flex", alignItems: "center", gap: 11 }}>
          <span style={{ display: "inline-flex", color: "var(--acc)" }}>
            <CI d={IC.shield} size={18} />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 11, fontWeight: 700, letterSpacing: "0.14em", textTransform: "uppercase", color: "var(--ink-3)" }}>
              {phase === "done" ? "Install complete" : phase === "error" ? "Install failed" : "Install with MatrixHub Client"}
            </div>
          </div>
          {phase !== "installing" && (
            <button className="navitem" style={{ width: 30, height: 30, padding: 0, justifyContent: "center" }} onClick={onClose}>
              <CI d={IC.x} size={16} />
            </button>
          )}
        </div>

        <div style={{ padding: 20 }}>
          {/* component identity */}
          <div style={{ display: "flex", gap: 13, alignItems: "center" }}>
            <div style={{ width: 46, height: 46, borderRadius: 11, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", background: "linear-gradient(150deg, rgba(41,224,122,0.18), rgba(15,61,39,0.5))", color: "var(--acc)", fontWeight: 800, fontFamily: "var(--mono)" }}>
              {req.initials}
            </div>
            <div style={{ minWidth: 0 }}>
              <div style={{ fontSize: 16, fontWeight: 700 }}>{req.name}</div>
              <div style={{ fontSize: 12, color: "var(--ink-3)", marginTop: 2 }}>Verified component · MatrixHub registry</div>
            </div>
          </div>

          {phase === "confirm" && (
            <>
              <div style={{ marginTop: 16, display: "grid", gap: 1, borderRadius: "var(--r-sm)", overflow: "hidden", border: "1px solid var(--line)" }}>
                {[
                  ["Source", req.hub || "MatrixHub verified registry"],
                  ["Destination", "Local Matrix environment"],
                  ["Command", req.cmd],
                ].map(([k, v]) => (
                  <div key={k} style={{ display: "flex", gap: 12, padding: "11px 13px", background: "var(--inset)" }}>
                    <span style={{ width: 92, flexShrink: 0, fontSize: 12, color: "var(--ink-3)", textTransform: "uppercase", letterSpacing: "0.05em" }}>{k}</span>
                    <span className="mono" style={{ fontSize: 12.5, color: k === "Command" ? "var(--acc-bright)" : "var(--ink)", wordBreak: "break-all" }}>{v}</span>
                  </div>
                ))}
              </div>
              {cliNeeded && (
                <div style={{ marginTop: 12, fontSize: 12.5, color: "var(--amber)", display: "flex", gap: 8, alignItems: "center" }}>
                  <CI d={IC.spark} size={15} /> Matrix CLI will be installed automatically first.
                </div>
              )}
              <div style={{ display: "flex", gap: 10, marginTop: 20 }}>
                <button className="btn btn-primary" style={{ flex: 1 }} onClick={begin}>
                  <CI d={IC.download} size={16} /> Install
                </button>
                <button className="btn btn-ghost" onClick={onClose}>Cancel</button>
              </div>
            </>
          )}

          {phase === "installing" && (
            <div style={{ marginTop: 18 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, color: "var(--acc)", fontSize: 13, fontWeight: 600 }}>
                <span className="dot on" style={{ animation: "ccPing 1.2s infinite" }} /> Installing…
              </div>
              <div ref={logRef} className="logbox" style={{ marginTop: 12, height: 180, fontSize: 11.5 }}>
                {lines.length === 0 && <div style={{ color: "var(--ink-4)" }}>starting matrix-cli…</div>}
                {lines.map((l, i) => (
                  <div key={i} style={{ color: "var(--ink-2)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{l}</div>
                ))}
                <span className="cur" />
              </div>
            </div>
          )}

          {phase === "done" && (
            <>
              <div style={{ marginTop: 18, display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", padding: "8px 0 4px" }}>
                <div style={{ width: 52, height: 52, borderRadius: 99, background: "rgba(41,224,122,0.14)", color: "var(--acc)", display: "flex", alignItems: "center", justifyContent: "center", boxShadow: "0 0 24px var(--acc-glow)" }}>
                  <CI d={IC.check} size={26} sw={2.6} />
                </div>
                <div style={{ fontSize: 17, fontWeight: 700, marginTop: 14 }}>Installed successfully</div>
                <div className="mono" style={{ fontSize: 12, color: "var(--ink-3)", marginTop: 5 }}>{req.id}</div>
              </div>
              <div style={{ display: "flex", gap: 10, marginTop: 20 }}>
                <button className="btn btn-primary" style={{ flex: 1 }} onClick={() => onDone(req)}>Done</button>
                <button className="btn btn-ghost" onClick={() => navigator.clipboard?.writeText(req.cmd)}>
                  <CI d={IC.copy} size={15} /> Copy command
                </button>
              </div>
            </>
          )}

          {phase === "error" && (
            <>
              <div className="logbox" style={{ marginTop: 16, height: 160, fontSize: 11.5 }}>
                {lines.map((l, i) => (
                  <div key={i} style={{ color: "var(--ink-2)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{l}</div>
                ))}
              </div>
              <div style={{ display: "flex", gap: 10, marginTop: 18 }}>
                <button className="btn btn-primary" style={{ flex: 1 }} onClick={begin}>
                  <CI d={IC.refresh} size={15} /> Retry
                </button>
                <button className="btn btn-ghost" onClick={onClose}>Close</button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

/* ---------- First-run setup wizard ---------- */
export function SetupWizard({
  hubUrl,
  log,
  onReady,
  onFinish,
}: {
  hubUrl: string;
  log: Logger;
  onReady: () => void;
  onFinish: () => void;
}) {
  const [stage, setStage] = useState<"welcome" | "installing" | "ready">("welcome");
  const [done, setDone] = useState({ cli: false, proto: false, hub: false });
  // Python preflight: null = checking, true = present, false = missing.
  const [pyOk, setPyOk] = useState<boolean | null>(null);
  const [pyVer, setPyVer] = useState<string | null>(null);
  const [pyBusy, setPyBusy] = useState(false);
  const [pyNeedsRestart, setPyNeedsRestart] = useState(false);

  // Detect Python on first render (best practice: verify the prerequisite first).
  useEffect(() => {
    getCliStatus()
      .then((s) => {
        setPyOk(s.python);
        setPyVer(s.pythonVersion);
      })
      .catch(() => setPyOk(false));
  }, []);

  async function installPy() {
    setPyBusy(true);
    log("info", "setup · checking Python");
    try {
      const ok = await installPython((line) => log("dim", line));
      // PATH may not refresh until restart — re-probe to be sure.
      const s = await getCliStatus().catch(() => null);
      const present = ok || (s?.python ?? false);
      setPyOk(present);
      setPyVer(s?.pythonVersion ?? null);
      if (!present) {
        setPyNeedsRestart(true);
        log("warn", "Python installed — restart MatrixHub Client to detect it.");
      } else {
        log("ok", `✓ Python ready${s?.pythonVersion ? ` · ${s.pythonVersion}` : ""}`);
      }
    } finally {
      setPyBusy(false);
    }
  }

  async function install() {
    if (!pyOk) {
      log("warn", "Python 3.11+ is required before installing the Matrix CLI.");
      return;
    }
    setStage("installing");
    log("info", "setup · installing matrix-cli");
    try {
      const ok = await installCli((line) => log("dim", line));
      setDone((d) => ({ ...d, cli: ok }));
      // Deep-link protocol is registered by the installed desktop app.
      setDone((d) => ({ ...d, proto: true }));
      log("dim", "registering matrix:// protocol links");
      const ms = await testHub(hubUrl).catch(() => -1);
      setDone((d) => ({ ...d, hub: ms >= 0 }));
      log(ms >= 0 ? "ok" : "warn", ms >= 0 ? `✓ hub online · ${ms}ms` : "hub unreachable");
      onReady();
      setStage("ready");
      log("ok", "✓ environment ready");
    } catch (e) {
      log("err", `setup failed · ${String(e)}`);
      setStage("welcome");
    }
  }

  const checks = [
    { k: "client", label: "MatrixHub Client installed", done: true },
    { k: "python", label: pyVer ? `Python runtime · ${pyVer}` : "Python 3.11+ runtime", done: pyOk === true },
    { k: "cli", label: "Matrix CLI", done: done.cli },
    { k: "proto", label: "Protocol links (matrix://)", done: done.proto },
    { k: "hub", label: "Hub connection", done: done.hub },
  ];

  return (
    <div className="main-inner rise" style={{ maxWidth: 560, margin: "0 auto" }}>
      <div style={{ display: "flex", justifyContent: "center", marginTop: 8 }}>
        <div style={{ width: 56, height: 56, borderRadius: 15, display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid var(--line-3)", background: "rgba(41,224,122,0.08)", color: "var(--acc)", boxShadow: "0 0 30px var(--acc-glow), inset 0 0 18px rgba(0,0,0,0.5)" }}>
          <CI d={IC.cli} size={26} sw={2} />
        </div>
      </div>
      <h1 className="h1" style={{ textAlign: "center" }}>{stage === "ready" ? "MatrixHub Client ready" : "Welcome to MatrixHub Client"}</h1>
      <p className="lead" style={{ textAlign: "center", marginLeft: "auto", marginRight: "auto", maxWidth: 420 }}>
        {stage === "ready"
          ? "Your local AI environment is set up. You can now install components from MatrixHub with one click."
          : "The official desktop companion connects MatrixHub with your machine and installs the Matrix CLI for you."}
      </p>

      <div className="card" style={{ marginTop: 24, padding: "8px 20px" }}>
        {checks.map((c) => (
          <div className={"check" + (c.done ? " done" : "")} key={c.k}>
            <span className="box">{c.done ? <CI d={IC.check} size={14} sw={2.6} /> : <span className="dot off" />}</span>
            <span className="tx">{c.label}</span>
            {!c.done && stage === "installing" && (
              <span className="mono" style={{ marginLeft: "auto", fontSize: 11.5, color: "var(--ink-4)" }}>pending</span>
            )}
          </div>
        ))}
      </div>

      {stage === "installing" && (
        <div style={{ marginTop: 18 }}>
          <div className="bar"><span style={{ width: "100%", animation: "ccBlink 1.4s steps(1) infinite" }} /></div>
          <div className="mono" style={{ fontSize: 11.5, color: "var(--ink-3)", marginTop: 8, textAlign: "right" }}>working…</div>
        </div>
      )}

      <div style={{ marginTop: 22, display: "flex", flexDirection: "column", alignItems: "center", gap: 12 }}>
        {stage === "welcome" && pyOk === null && (
          <button className="btn btn-ghost" disabled>
            <CI d={IC.refresh} size={16} /> Checking environment…
          </button>
        )}

        {/* Python missing → guide the install (winget/brew), with python.org fallback. */}
        {stage === "welcome" && pyOk === false && !pyNeedsRestart && (
          <>
            <button className="btn btn-primary" onClick={installPy} disabled={pyBusy}>
              <CI d={IC.download} size={16} /> {pyBusy ? "Installing Python…" : "Install Python 3"}
            </button>
            <button className="btn btn-ghost" onClick={() => openUrl(PYTHON_DOWNLOAD_URL)}>
              <CI d={IC.link} size={15} /> Download from python.org
            </button>
          </>
        )}

        {/* Python installed but not yet on PATH → offer a restart. */}
        {stage === "welcome" && pyOk === false && pyNeedsRestart && (
          <button className="btn btn-primary" onClick={() => relaunchApp()}>
            <CI d={IC.refresh} size={16} /> Restart MatrixHub Client
          </button>
        )}

        {/* Python present → install the CLI. */}
        {stage === "welcome" && pyOk === true && (
          <button className="btn btn-primary" onClick={install}>
            <CI d={IC.download} size={16} /> Install Matrix CLI
          </button>
        )}

        {stage === "ready" && (
          <button className="btn btn-primary" onClick={onFinish}>
            Return to MatrixHub <CI d={IC.arrow} size={16} />
          </button>
        )}
      </div>

      {stage === "welcome" && (
        <p style={{ textAlign: "center", marginTop: 16, fontSize: 12.5, color: "var(--ink-4)" }}>
          {pyOk === false
            ? "MatrixHub Client needs Python 3.11+. We install it with your system package manager (winget on Windows, Homebrew on macOS) — or grab it from python.org."
            : "The Matrix CLI installs into an isolated environment (pipx). No terminal or PATH editing required."}
        </p>
      )}
    </div>
  );
}
