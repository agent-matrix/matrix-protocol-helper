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

/* ---------- streaming helpers ---------- */
type OnLine = (line: string) => void;

function channel(onLine: OnLine): Channel<string> {
  const ch = new Channel<string>();
  ch.onmessage = onLine;
  return ch;
}

/** Install the Matrix CLI (pipx/pip). Streams output; resolves true on success. */
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

/* ---------- deep-link install requests ---------- */
export async function onInstallRequest(
  cb: (req: DeepLinkRequest) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<DeepLinkRequest>("install-request", (e) => cb(e.payload));
}

/* ---------- window controls (custom titlebar) ---------- */
export const windowControls = {
  minimize: () => isTauri() && void getCurrentWindow().minimize(),
  toggleMaximize: () => isTauri() && void getCurrentWindow().toggleMaximize(),
  close: () => isTauri() && void getCurrentWindow().close(),
};
