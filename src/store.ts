/* ============================================================
   store.ts — persisted client state + shared types
   ============================================================ */

export interface EnvState {
  cli: boolean;
  python: boolean;
  proto: boolean;
  hub: boolean;
}

export interface Settings {
  hubUrl: string;
  dir: string;
  autoCli: boolean;
  approve: boolean;
  autostart: boolean;
  sidebarRain?: boolean;
}

export interface RecentInstall {
  name: string;
  id: string;
}

export interface PersistedState {
  env?: EnvState;
  settings?: Settings;
  recent?: RecentInstall[];
  setupDone?: boolean;
}

export interface InstallReq {
  /** Entity id, e.g. "mcp_server:retell-ai@0.2.2". */
  id: string;
  name: string;
  initials: string;
  /** Display command, e.g. "matrix install retell-ai-mcp-server". */
  cmd: string;
  /** Optional alias / hub override forwarded from a matrix:// deep link. */
  alias?: string;
  hub?: string;
}

const KEY = "mhc-state";

export const STORE: PersistedState = (() => {
  try {
    return JSON.parse(localStorage.getItem(KEY) || "{}") as PersistedState;
  } catch {
    return {};
  }
})();

export function persist(patch: Partial<PersistedState>) {
  try {
    const cur = JSON.parse(localStorage.getItem(KEY) || "{}");
    localStorage.setItem(KEY, JSON.stringify({ ...cur, ...patch }));
  } catch {
    /* ignore quota / private-mode errors */
  }
}

export type LogTone = "ok" | "dim" | "warn" | "err" | "info";
export interface LogLine {
  ts: string;
  t: LogTone;
  m: string;
}

export const now = () => new Date().toLocaleTimeString("en-GB", { hour12: false });
