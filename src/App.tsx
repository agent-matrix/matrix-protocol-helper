/* ============================================================
   App.tsx — MatrixHub Client shell + state machine
   ============================================================ */
import { useCallback, useEffect, useRef, useState } from "react";
import { CI, IC } from "./icons";
import { SidebarRain } from "./components/SidebarRain";
import { HomeView, InstallsView, LogsView, SettingsView } from "./views/Views";
import { ClientConsole } from "./views/Console";
import { InstallFlow, SetupWizard } from "./flows/Flows";
import { UpdateModal, UpdateToast } from "./flows/UpdateModal";
import {
  checkUpdate,
  exportLogs,
  getAppInfo,
  getCliStatus,
  getInstallId,
  installCli,
  onInstallRequest,
  openDataDir,
  resetCli,
  testHub,
  windowControls,
  type DeepLinkRequest,
  type UpdateInfo,
} from "./lib/tauri";
import { STORE, persist, now } from "./store";
import type { EnvState, InstallReq, LogLine, LogTone, RecentInstall, Settings } from "./store";

const APP_VERSION = "0.2.0";

/** Build an InstallReq from a matrix:// deep-link payload. */
function reqFromDeepLink(d: DeepLinkRequest): InstallReq {
  const base = d.entity.split("@")[0];
  const slug = base.includes(":") ? base.split(":").pop()! : base;
  const name = slug.replace(/[-_.]+/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  const initials = (slug.replace(/[^a-zA-Z]/g, "").slice(0, 2) || "MX").toUpperCase();
  return {
    id: d.entity,
    name,
    initials,
    cmd: `matrix install ${d.entity}${d.alias ? ` --alias ${d.alias}` : ""}`,
    alias: d.alias ?? undefined,
    hub: d.hub ?? undefined,
  };
}

export default function App() {
  const [env, setEnv] = useState<EnvState>(STORE.env || { cli: false, python: false, proto: false, hub: false });
  const [ver, setVer] = useState<{ cli: string | null; python: string | null }>({ cli: null, python: null });
  const [hubMs, setHubMs] = useState<number | null>(null);
  const [route, setRoute] = useState<string>(STORE.setupDone ? "home" : "setup");
  const [installReq, setInstallReq] = useState<InstallReq | null>(null);
  const [pending, setPending] = useState<InstallReq[]>([]);
  const [sideOpen, setSideOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [recent, setRecent] = useState<RecentInstall[]>(STORE.recent || []);
  const [cfg, setCfg] = useState<Settings>(
    STORE.settings || { hubUrl: "https://api.matrixhub.io", dir: "~/.matrix", autoCli: true, approve: true, autostart: false },
  );
  const [logs, setLogs] = useState<LogLine[]>([
    { ts: now(), t: "info", m: "MatrixHub Client started" },
    { ts: now(), t: "dim", m: "loading local environment …" },
  ]);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updateOpen, setUpdateOpen] = useState(false);
  const [updateDismissed, setUpdateDismissed] = useState(false);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [appVer, setAppVer] = useState(APP_VERSION);
  const installId = useRef(getInstallId());
  const cfgRef = useRef(cfg);
  cfgRef.current = cfg;

  const log = useCallback((t: LogTone, m: string) => {
    setLogs((L) => [...L, { ts: now(), t, m }].slice(-200));
  }, []);

  /** Re-detect the local environment (CLI / python) and hub connectivity. */
  const refresh = useCallback(async () => {
    const st = await getCliStatus().catch(() => null);
    setVer({ cli: st?.cliVersion ?? null, python: st?.pythonVersion ?? null });
    const ms = await testHub(cfgRef.current.hubUrl).catch(() => -1);
    const hubOk = ms >= 0;
    setHubMs(hubOk ? ms : null);
    setEnv((prev) => {
      const next: EnvState = {
        cli: st ? st.cli : prev.cli,
        python: st ? st.python : prev.python,
        proto: "__TAURI_INTERNALS__" in window,
        hub: hubOk,
      };
      persist({ env: next });
      return next;
    });
    if (st) log(st.cli ? "ok" : "warn", st.cli ? `✓ matrix-cli ${st.cliVersion || "ready"}` : "matrix-cli not found");
    log(hubOk ? "ok" : "warn", hubOk ? `✓ hub online · ${ms}ms` : "hub offline");
  }, [log]);

  const checkForUpdates = useCallback(
    async (silent: boolean) => {
      setCheckingUpdate(true);
      try {
        const info = await checkUpdate();
        if (info.available) {
          setUpdate(info);
          setUpdateDismissed(false);
          if (!silent) setUpdateOpen(true);
          log("info", `update available · v${info.version}`);
        } else if (!silent) {
          log("ok", `you're on the latest version (v${info.currentVersion})`);
        }
      } catch (e) {
        if (!silent) log("warn", `update check unavailable · ${String(e)}`);
      } finally {
        setCheckingUpdate(false);
      }
    },
    [log],
  );

  async function onResetCli() {
    setBusy(true);
    log("info", "repairing matrix-cli (reset) …");
    try {
      const ok = await resetCli((line) => log("dim", line));
      log(ok ? "ok" : "err", ok ? "✓ matrix-cli reset complete" : "reset failed");
    } finally {
      await refresh();
      setBusy(false);
    }
  }

  async function onExportLogs() {
    const text = logs.map((l) => `${l.ts}\t${l.t.toUpperCase()}\t${l.m}`).join("\n");
    try {
      const path = await exportLogs(text);
      log("ok", `logs exported · ${path}`);
    } catch (e) {
      log("err", `export failed · ${String(e)}`);
    }
  }

  async function onOpenDataDir() {
    try {
      const path = await openDataDir();
      log("dim", `opened data folder · ${path}`);
    } catch (e) {
      log("warn", `could not open data folder · ${String(e)}`);
    }
  }

  useEffect(() => {
    refresh();
    getAppInfo().then((i) => setAppVer(i.version)).catch(() => {});
    // Background update check shortly after launch (premium app behaviour).
    const t = setTimeout(() => checkForUpdates(true), 1500);
    const un = onInstallRequest((d) => {
      log("info", `matrix:// install request · ${d.entity}`);
      const req = reqFromDeepLink(d);
      // The request lands as a pending item in Installs (badged) so it is
      // never lost if the review modal is dismissed, then opens the modal.
      setPending((p) => [req, ...p.filter((x) => x.id !== req.id)].slice(0, 8));
      setInstallReq(req);
      setSideOpen(false);
    });
    return () => {
      clearTimeout(t);
      un.then((f) => f());
    };
  }, [refresh, log, checkForUpdates]);

  const nav = [
    { id: "home", label: "Home", ic: IC.home, badge: null as number | null },
    { id: "terminal", label: "Terminal", ic: IC.term, badge: null },
    { id: "installs", label: "Installs", ic: IC.box, badge: pending.length || null },
    { id: "logs", label: "Logs", ic: IC.logs, badge: null },
    { id: "settings", label: "Settings", ic: IC.gear, badge: null },
  ];

  async function testConn() {
    setBusy(true);
    log("dim", `ping ${cfg.hubUrl} …`);
    const ms = await testHub(cfg.hubUrl).catch(() => -1);
    setEnv((e) => {
      const next = { ...e, hub: ms >= 0 };
      persist({ env: next });
      return next;
    });
    log(ms >= 0 ? "ok" : "warn", ms >= 0 ? `✓ hub online · ${ms}ms` : "hub unreachable");
    setBusy(false);
  }

  async function installCliQuick() {
    setBusy(true);
    log("info", "installing matrix-cli …");
    try {
      const ok = await installCli((line) => log("dim", line));
      log(ok ? "ok" : "err", ok ? "✓ matrix-cli installed" : "matrix-cli install failed");
    } finally {
      await refresh();
      setBusy(false);
    }
  }

  function onInstalled(req: InstallReq) {
    const next = [{ name: req.name, id: req.id }, ...recent.filter((r) => r.id !== req.id)].slice(0, 6);
    setRecent(next);
    persist({ recent: next });
    setPending((p) => p.filter((x) => x.id !== req.id));
    setInstallReq(null);
    setRoute("home");
    refresh();
  }

  function go(id: string) {
    setRoute(id);
    setSideOpen(false);
  }

  return (
    <div className="win">
      {/* titlebar */}
      <div className="titlebar" data-tauri-drag-region>
        <button
          className="menu-btn navitem"
          style={{ width: 30, height: 30, padding: 0, justifyContent: "center", flex: "none" }}
          onClick={() => setSideOpen((v) => !v)}
        >
          <CI d={IC.menu} size={16} />
        </button>
        <div className="lights">
          <button className="r" aria-label="Close" title="Close" onClick={windowControls.close} />
          <button className="y" aria-label="Minimize" title="Minimize" onClick={windowControls.minimize} />
          <button className="g" aria-label="Maximize" title="Maximize" onClick={windowControls.toggleMaximize} />
        </div>
        <div className="tt">
          <CI d={IC.cli} size={15} style={{ color: "var(--acc)" }} /> MatrixHub Client
        </div>
        <div className="spacer" data-tauri-drag-region />
        <span className="ver">v{appVer}</span>
      </div>

      <div className="body">
        {/* sidebar */}
        <aside className={"side" + (sideOpen ? " open" : "")}>
          {cfg.sidebarRain !== false && <SidebarRain />}
          <div className="brand">
            <span className="logo">
              <CI d={IC.cli} size={18} sw={2} />
            </span>
            <div>
              <div className="nm">MATRIXHUB</div>
              <div className="sub">client runtime</div>
            </div>
          </div>
          <div className="navlabel">Workspace</div>
          <nav className="nav">
            {nav.map((n) => (
              <button key={n.id} className={"navitem" + (route === n.id ? " active" : "")} onClick={() => go(n.id)}>
                <span className="ic">
                  <CI d={n.ic} size={17} />
                </span>{" "}
                {n.label}
                {n.badge ? <span className="badge">{n.badge}</span> : null}
              </button>
            ))}
          </nav>

          <div className="side-foot">
            <div className="envline">
              <span className={"dot " + (env.cli ? "on" : "warn")} /> CLI {env.cli ? "ready" : "not set up"}
            </div>
            <div className="envline">
              <span className={"dot " + (env.hub ? "on" : "off")} /> Hub {env.hub ? "online" : "offline"}
            </div>
          </div>
        </aside>

        {/* main */}
        <main className="main" style={route === "terminal" ? { overflowY: "hidden", display: "flex" } : undefined}>
          {route === "setup" && (
            <SetupWizard
              hubUrl={cfg.hubUrl}
              log={log}
              onReady={() => {
                persist({ setupDone: true });
                refresh();
              }}
              onFinish={() => setRoute("home")}
            />
          )}
          {route === "home" && (
            <HomeView
              env={env}
              recent={recent}
              busy={busy}
              cliVer={ver.cli}
              pythonVer={ver.python}
              hubMs={hubMs}
              onTest={testConn}
              onInstallCli={installCliQuick}
              onOpenLogs={() => setRoute("logs")}
            />
          )}
          {route === "terminal" && <ClientConsole hubUrl={cfg.hubUrl} />}
          {route === "installs" && (
            <InstallsView
              recent={recent}
              pending={pending}
              onRequest={(c) => setInstallReq({ ...c })}
              onReview={(req) => setInstallReq(req)}
              onDecline={(req) => {
                setPending((p) => p.filter((x) => x.id !== req.id));
                log("warn", `declined install request · ${req.id}`);
              }}
            />
          )}
          {route === "logs" && <LogsView lines={logs} />}
          {route === "settings" && (
            <SettingsView
              cfg={cfg}
              setCfg={setCfg}
              diag={{
                appVersion: appVer,
                identifier: "io.matrixhub.client",
                installId: installId.current,
                checkingUpdate,
                onCheckUpdates: () => checkForUpdates(false),
                onResetCli,
                onExportLogs,
                onOpenDataDir,
                busy,
              }}
            />
          )}
        </main>
      </div>

      {update?.available && !updateOpen && !updateDismissed && (
        <UpdateToast info={update} onOpen={() => setUpdateOpen(true)} onDismiss={() => setUpdateDismissed(true)} />
      )}
      {update?.available && updateOpen && <UpdateModal info={update} onClose={() => setUpdateOpen(false)} />}

      {installReq && (
        <InstallFlow
          req={installReq}
          cliNeeded={!env.cli}
          log={log}
          onClose={() => setInstallReq(null)}
          onDone={onInstalled}
        />
      )}
    </div>
  );
}
