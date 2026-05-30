/* ============================================================
   tauri.ts — typed bridge to the Rust backend.

   Every call degrades gracefully: inside the Tauri webview it
   invokes real commands (which drive matrix-cli); in a plain
   browser (e.g. `vite preview`) it falls back to a simulation so
   the UI is still reviewable.
   ============================================================ */
import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { InstallReq } from "../store";

export interface CliStatus {
  cli: boolean;
  cliVersion: string | null;
  python: boolean;
  pythonVersion: string | null;
}

export interface DeepLinkRequest {
  entity: string;
  alias: string | null;
  hub: string | null;
}

export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/* ---------- environment status ---------- */
export async function getCliStatus(): Promise<CliStatus> {
  if (!isTauri()) {
    return { cli: false, cliVersion: null, python: false, pythonVersion: null };
  }
  return invoke<CliStatus>("cli_status");
}

/* ---------- hub connectivity (returns round-trip ms) ---------- */
export async function testHub(url: string): Promise<number> {
  if (!isTauri()) {
    await wait(600);
    return 41;
  }
  return invoke<number>("test_hub", { url });
}

/* ---------- runtime diagnostics (verify the .venv backend is installed) ---------- */
export interface RuntimeDiagnostics {
  pythonOk: boolean;
  pythonVersion: string | null;
  venvPath: string | null;
  venvPythonOk: boolean;
  matrixInstalled: boolean;
  matrixPath: string | null;
  matrixVersion: string | null;
  ready: boolean;
}

export async function getRuntimeDiagnostics(): Promise<RuntimeDiagnostics> {
  if (!isTauri()) {
    return {
      pythonOk: false,
      pythonVersion: null,
      venvPath: null,
      venvPythonOk: false,
      matrixInstalled: false,
      matrixPath: null,
      matrixVersion: null,
      ready: false,
    };
  }
  return invoke<RuntimeDiagnostics>("runtime_diagnostics");
}

/* ---------- full diagnosis report (Debug button) ---------- */
/** Generate a complete Markdown diagnosis bundle (build info + runtime + log
 *  tail) for bug reports or to hand to a developer/AI to debug the app. */
export async function generateDiagnosis(hubUrl?: string): Promise<string> {
  if (!isTauri()) {
    return [
      "# MatrixHub Client — Diagnosis Report",
      "",
      "(browser preview — the native backend is not available, so this is a stub)",
      "",
      `- Hub: ${hubUrl ?? "n/a"}`,
    ].join("\n");
  }
  return invoke<string>("generate_diagnosis", { hubUrl: hubUrl ?? null });
}

/* ---------- streaming helpers ---------- */
type OnLine = (line: string) => void;

function channel(onLine: OnLine): Channel<string> {
  const ch = new Channel<string>();
  ch.onmessage = onLine;
  return ch;
}

/** Install Python via the platform package manager (winget/brew). Streams output. */
export async function installPython(onLine: OnLine): Promise<boolean> {
  if (!isTauri()) {
    onLine("(preview) would install Python 3");
    return true;
  }
  return invoke<boolean>("install_python", { onLine: channel(onLine) });
}

/** Open an http(s) URL in the default browser. */
export async function openUrl(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  await invoke("open_url", { url });
}

/** Relaunch the app (used after installing Python so PATH refreshes). */
export async function relaunchApp(): Promise<void> {
  if (!isTauri()) return;
  await invoke("relaunch");
}

/** Provision the managed runtime (.venv + matrix-cli). Streams output; resolves true on success. */
export async function installCli(onLine: OnLine): Promise<boolean> {
  if (!isTauri()) {
    for (const l of [
      "downloading matrix-cli 0.1.6",
      "preparing python 3.11 environment",
      "✓ matrix-cli installed",
    ]) {
      await wait(500);
      onLine(l);
    }
    return true;
  }
  return invoke<boolean>("install_cli", { onLine: channel(onLine) });
}

/** Install a component via matrix-cli. Streams output; resolves with exit code. */
export async function installComponent(
  req: InstallReq,
  onLine: OnLine,
): Promise<number> {
  if (!isTauri()) {
    for (const l of [`matrix install ${req.id}`, "resolving manifest …", "✓ done"]) {
      await wait(500);
      onLine(l);
    }
    return 0;
  }
  return invoke<number>("install_component", {
    entity: req.id,
    alias: req.alias ?? null,
    hub: req.hub ?? null,
    onLine: channel(onLine),
  });
}

/** Run an arbitrary `matrix …` command. Streams output; resolves with exit code. */
export async function runCommand(line: string, onLine: OnLine): Promise<number> {
  if (!isTauri()) {
    await wait(300);
    onLine(`(browser preview) matrix-cli not available — would run: ${line}`);
    return 0;
  }
  return invoke<number>("run_command", { line, onLine: channel(onLine) });
}

/* ---------- diagnostics bridge (frontend → client.log) ---------- */
/** Record a line from the UI into the persistent diagnostics log so the
 *  diagnosis report captures what the frontend actually observed. Fire-and-forget. */
export function logFrontend(
  level: "debug" | "info" | "warn" | "error",
  message: string,
): void {
  if (!isTauri()) return;
  void invoke("log_frontend", { level, message }).catch(() => {});
}

/* ---------- real terminal (PTY) ---------- */
/** Decode a base64 string (PTY output transport) to raw bytes. */
function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Active unlisten callbacks keyed by session id, cleaned up in ptyClose. */
const _ptyUnlisteners = new Map<number, Array<UnlistenFn>>();

/**
 * Open a PTY session and stream output to `onData`.
 *
 * Transport: the Rust side emits Tauri events "pty-data-{id}" (base64 payload)
 * and "pty-exit-{id}" (shell closed) via AppHandle::emit(), which is
 * guaranteed thread-safe on all platforms. This replaces Channel<T>::send(),
 * which proved unreliable on Windows WebView2 / Linux WebKitGTK when called
 * from background threads (messages enqueued in Rust but JS onmessage never
 * fired, leaving bytesSeen=0 in the frontend).
 */
export async function ptyOpen(
  onData: (bytes: Uint8Array) => void,
  cols: number,
  rows: number,
): Promise<number> {
  if (!isTauri()) return 0;
  // Start the PTY; the Rust side will emit events once the shell produces output.
  const id = await invoke<number>("pty_open", { cols, rows });
  if (!id) return id;

  // Subscribe to PTY output events for this session.
  const unlistenData = await listen<{ data: string }>(`pty-data-${id}`, (e) => {
    try {
      onData(b64ToBytes(e.payload.data));
    } catch (err) {
      logFrontend("error", `pty decode failed (session ${id}): ${String(err)}`);
    }
  });

  // Subscribe to shell-exit event so callers can react (optional).
  const unlistenExit = await listen(`pty-exit-${id}`, () => {
    logFrontend("info", `terminal: PTY session ${id} shell exited`);
  });

  _ptyUnlisteners.set(id, [unlistenData, unlistenExit]);
  return id;
}

export async function ptyWrite(id: number, data: string): Promise<void> {
  if (!isTauri() || !id) return;
  await invoke("pty_write", { id, data });
}

export async function ptyResize(id: number, cols: number, rows: number): Promise<void> {
  if (!isTauri() || !id) return;
  await invoke("pty_resize", { id, cols, rows }).catch(() => {});
}

export async function ptyClose(id: number): Promise<void> {
  if (!isTauri() || !id) return;
  // Tear down event listeners before closing the backend session.
  const fns = _ptyUnlisteners.get(id);
  if (fns) {
    for (const fn_ of fns) fn_();
    _ptyUnlisteners.delete(id);
  }
  await invoke("pty_close", { id }).catch(() => {});
}

/* ---------- deep-link install requests ---------- */
export async function onInstallRequest(
  cb: (req: DeepLinkRequest) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<DeepLinkRequest>("install-request", (e) => cb(e.payload));
}

/* ---------- auto-update ---------- */
export interface UpdateInfo {
  available: boolean;
  currentVersion: string;
  version: string;
  notes: string | null;
  date: string | null;
}
export interface UpdateProgress {
  downloaded: number;
  total: number | null;
  pct: number;
  phase: "download" | "install" | string;
}

/** Check the update endpoint for a newer signed release. */
export async function checkUpdate(): Promise<UpdateInfo> {
  if (!isTauri()) {
    return { available: false, currentVersion: "0.2.0", version: "0.2.0", notes: null, date: null };
  }
  return invoke<UpdateInfo>("check_update");
}

/** Download + install the update, streaming progress. The app relaunches on success. */
export async function installUpdate(onProgress: (p: UpdateProgress) => void): Promise<void> {
  if (!isTauri()) return;
  const ch = new Channel<UpdateProgress>();
  ch.onmessage = onProgress;
  await invoke("install_update", { onProgress: ch });
}

/* ---------- diagnostics / supportability ---------- */
export interface AppInfo {
  name: string;
  version: string;
  identifier: string;
  tauriVersion: string;
  os: string;
  arch: string;
}

export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) {
    return { name: "MatrixHub Client", version: "0.2.0", identifier: "io.matrixhub.client", tauriVersion: "2", os: "web", arch: "-" };
  }
  return invoke<AppInfo>("app_info");
}

/** Repair the Matrix CLI (uninstall + reinstall). Streams output. */
export async function resetCli(onLine: OnLine): Promise<boolean> {
  if (!isTauri()) {
    onLine("(preview) would reset matrix-cli");
    return true;
  }
  return invoke<boolean>("reset_cli", { onLine: channel(onLine) });
}

export async function openDataDir(): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("open_data_dir");
}

export async function exportLogs(content: string): Promise<string> {
  if (!isTauri()) {
    const blob = new Blob([content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "matrixhub-client.log";
    a.click();
    URL.revokeObjectURL(url);
    return "matrixhub-client.log";
  }
  return invoke<string>("export_logs", { content });
}

/** Stable per-install identifier for support tickets. */
export function getInstallId(): string {
  const KEY = "mhc-install-id";
  try {
    let id = localStorage.getItem(KEY);
    if (!id) {
      id = crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2) + Date.now().toString(36);
      localStorage.setItem(KEY, id);
    }
    return id;
  } catch {
    return "unknown";
  }
}

/* ---------- window controls (custom titlebar) ---------- */
export const windowControls = {
  minimize: () => isTauri() && void getCurrentWindow().minimize(),
  toggleMaximize: () => isTauri() && void getCurrentWindow().toggleMaximize(),
  close: () => isTauri() && void getCurrentWindow().close(),
};
