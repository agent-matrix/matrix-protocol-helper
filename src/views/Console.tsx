/* ============================================================
   Console.tsx — a REAL terminal.

   Renders xterm.js wired to a backend PTY running the user's
   actual shell (PowerShell/cmd on Windows, $SHELL on Unix).
   This is a genuine terminal — colors, prompts, live output and
   interactivity all work, exactly like a native one.
   ============================================================ */
import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { IDisposable } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { CI, IC } from "../icons";
import { isTauri, ptyClose, ptyOpen, ptyResize, ptyWrite } from "../lib/tauri";

const THEME = {
  background: "#0b110e",
  foreground: "#e7f3ec",
  cursor: "#29e07a",
  cursorAccent: "#0b110e",
  selectionBackground: "rgba(41,224,122,0.25)",
  black: "#0b110e",
  red: "#ff5d6c",
  green: "#29e07a",
  yellow: "#f5b945",
  blue: "#5b9dff",
  magenta: "#b48cff",
  cyan: "#46d4c8",
  white: "#a9c2b6",
  brightBlack: "#4a5d54",
  brightRed: "#ff8089",
  brightGreen: "#6dffa6",
  brightYellow: "#ffd27a",
  brightBlue: "#8bbcff",
  brightMagenta: "#cdb4ff",
  brightCyan: "#7fe9df",
  brightWhite: "#e7f3ec",
};

const CHIPS = ["matrix --help", "matrix --version", "matrix search github", "matrix ps"];

export function ClientConsole({ hubUrl }: { hubUrl: string }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const idRef = useRef(0);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const inputRef = useRef<IDisposable | null>(null);
  const resizeRef = useRef<IDisposable | null>(null);
  const pendingInputRef = useRef<string[]>([]);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      theme: THEME,
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: true,
      convertEol: true,
      windowsMode: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    termRef.current = term;
    fitRef.current = fit;

    const focusTerm = () => {
      term.focus();
      // xterm focuses its hidden textarea; this fallback helps in some
      // Windows WebView2 builds where click focus is unreliable.
      term.textarea?.focus();
    };

    const safeFit = () => {
      try {
        fit.fit();
        const id = idRef.current;
        if (id) void ptyResize(id, term.cols, term.rows);
      } catch {
        /* element not measurable yet */
      }
    };

    // Fit after layout settles. A zero-sized terminal can look alive but not
    // accept input correctly because xterm cannot place its textarea.
    requestAnimationFrame(() => {
      safeFit();
      focusTerm();
    });

    let disposed = false;
    let sawShellOutput = false;

    const pointerFocus = () => focusTerm();
    host.addEventListener("pointerdown", pointerFocus);

    // Register input immediately. If the user types before the backend has
    // returned a PTY id, buffer those bytes instead of silently dropping them.
    inputRef.current = term.onData((data) => {
      const id = idRef.current;
      if (id) {
        void ptyWrite(id, data).catch((e) => {
          term.writeln(`\r\n\x1b[31mterminal write failed: ${String(e)}\x1b[0m`);
        });
      } else {
        pendingInputRef.current.push(data);
      }
    });

    resizeRef.current = term.onResize(({ cols, rows }) => {
      const id = idRef.current;
      if (id) void ptyResize(id, cols, rows);
    });

    if (isTauri()) {
      term.writeln("\x1b[90mstarting local shell …\x1b[0m");
      ptyOpen(
        (bytes) => {
          sawShellOutput = true;
          term.write(bytes);
        },
        Math.max(term.cols, 2),
        Math.max(term.rows, 2),
      )
        .then(async (id) => {
          if (disposed) {
            await ptyClose(id);
            return;
          }
          idRef.current = id;
          setReady(true);
          safeFit();
          focusTerm();

          const pending = pendingInputRef.current.splice(0);
          for (const data of pending) await ptyWrite(id, data);

          window.setTimeout(() => {
            if (!disposed && !sawShellOutput) {
              term.writeln("\r\n\x1b[33mPTY opened, but the shell produced no prompt yet. Click inside this terminal and type a command.\x1b[0m");
            }
          }, 1500);
        })
        .catch((e) => {
          term.writeln(`\r\n\x1b[31mFailed to start terminal: ${String(e)}\x1b[0m`);
          setReady(false);
        });
    } else {
      term.writeln("\x1b[32mMatrixHub Client\x1b[0m — real terminal is available in the desktop app.");
    }

    const onWinResize = () => safeFit();
    window.addEventListener("resize", onWinResize);
    const ro = new ResizeObserver(() => safeFit());
    ro.observe(host);

    return () => {
      disposed = true;
      setReady(false);
      window.removeEventListener("resize", onWinResize);
      host.removeEventListener("pointerdown", pointerFocus);
      ro.disconnect();
      inputRef.current?.dispose();
      resizeRef.current?.dispose();
      inputRef.current = null;
      resizeRef.current = null;
      const id = idRef.current;
      idRef.current = 0;
      if (id) void ptyClose(id);
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
      pendingInputRef.current = [];
    };
  }, []);

  function send(cmd: string) {
    const id = idRef.current;
    const term = termRef.current;
    if (!term) return;
    if (id) {
      void ptyWrite(id, cmd + "\r");
      term.focus();
      term.textarea?.focus();
    } else {
      term.writeln("\r\n\x1b[33mTerminal is still starting. Try again in a moment.\x1b[0m");
    }
  }

  return (
    <div className="view-pad rise" style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0, height: "100%", maxWidth: 900, paddingBottom: 22 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, flexShrink: 0 }}>
        <div>
          <p className="eyebrow">Local runner</p>
          <h1 className="h1" style={{ marginTop: 8 }}>Terminal</h1>
        </div>
        <span className={"pill " + (ready ? "ok" : "")}><span className={"dot " + (ready ? "on" : "warn")} /> {ready ? "live shell" : "starting"}</span>
      </div>

      <div style={{ marginTop: 18, flex: 1, minHeight: 0, display: "flex", flexDirection: "column", background: "var(--inset)", border: "1px solid var(--line-2)", borderRadius: "var(--r-md)", overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "9px 14px", borderBottom: "1px solid var(--line)", background: "linear-gradient(180deg, rgba(41,224,122,0.04), transparent)", flexShrink: 0 }}>
          <span style={{ display: "inline-flex", color: "var(--acc)" }}><CI d={IC.term} size={15} sw={2} /></span>
          <span className="mono" style={{ fontSize: 12, color: "var(--ink-2)", fontWeight: 600, letterSpacing: "0.04em" }}>system shell</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 11, color: "var(--ink-4)" }}>{hubUrl.replace(/^https?:\/\//, "")}</span>
        </div>

        <div
          ref={hostRef}
          tabIndex={0}
          aria-label="MatrixHub terminal"
          style={{ flex: 1, minHeight: 0, padding: "8px 10px", outline: "none" }}
          onFocus={() => termRef.current?.focus()}
        />

        <div className="term-chips" style={{ display: "flex", gap: 8, overflowX: "auto", padding: "10px 12px", borderTop: "1px solid var(--line)", background: "rgba(0,0,0,0.25)", flexShrink: 0 }}>
          {CHIPS.map((c) => (
            <button
              key={c}
              onClick={() => send(c)}
              disabled={!ready}
              style={{ flexShrink: 0, padding: "6px 11px", borderRadius: 99, border: "1px solid var(--line-2)", background: "rgba(41,224,122,0.04)", color: "var(--ink-2)", fontFamily: "var(--mono)", fontSize: 11.5 }}
            >
              {c}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
