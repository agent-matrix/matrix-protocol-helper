/* ============================================================
   Console.tsx — a REAL terminal with a guaranteed input bar.

   Renders xterm.js wired to a backend PTY running the user's
   actual shell (PowerShell/cmd on Windows, $SHELL on Unix), plus
   a dedicated command input bar so typing ALWAYS works even when
   the embedded WebView mishandles xterm's hidden-textarea focus
   (a known fragility on Windows WebView2 / Linux WebKitGTK).

   Design notes (why it's built this way):
   • xterm shows live output and is fully interactive when focus
     behaves. But focus inside an embedded WebView is unreliable,
     so we add a native <input> command bar that writes straight to
     the PTY. A native input never has the focus/IME problems xterm
     can have — this is the bar the user types into, guaranteed.
   • The xterm host is absolutely positioned inside a relative
     wrapper. That decouples it from flex-height resolution, which
     is what previously let the terminal collapse to 0 rows (so the
     prompt streamed in but had nowhere to render).
   • The PTY is opened only after a real fit, and the "no prompt"
     hint is cleared as soon as any byte arrives.
   ============================================================ */
import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { CI, IC } from "../icons";
import { isTauri, logFrontend, ptyClose, ptyOpen, ptyResize, ptyWrite } from "../lib/tauri";

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

/* xterm's `windowsMode` rewrites line endings for the Windows ConPTY. Enabling
   it on macOS/Linux corrupts cursor placement, so only set it on Windows. */
const IS_WINDOWS =
  typeof navigator !== "undefined" &&
  /win/i.test(
    (navigator as unknown as { userAgentData?: { platform?: string } }).userAgentData?.platform ||
      navigator.platform ||
      navigator.userAgent ||
      "",
  );

/** Everything that makes up one live terminal session. */
interface TermSession {
  term: Terminal;
  fit: FitAddon;
  ptyId: number;
  disposed: boolean;
  pending: string[];
  sawOutput: boolean;
  cleanup: Array<() => void>;
}

export function ClientConsole({ hubUrl }: { hubUrl: string }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const sessRef = useRef<TermSession | null>(null);

  const [ready, setReady] = useState(false);
  const [cmd, setCmd] = useState("");
  // Command history for the input bar (↑/↓), newest last.
  const historyRef = useRef<string[]>([]);
  const histPosRef = useRef<number>(-1);

  /** Write raw bytes to the PTY (used by the input bar and chips). */
  function writeToPty(data: string) {
    const s = sessRef.current;
    if (!s || s.disposed) return;
    if (s.ptyId) {
      void ptyWrite(s.ptyId, data).catch(() => {});
    } else {
      s.pending.push(data);
    }
  }

  /** Send a full command line from the input bar. */
  function runCommand(raw: string) {
    const line = raw.replace(/\r?\n$/, "");
    if (line.trim()) {
      historyRef.current.push(line);
      if (historyRef.current.length > 200) historyRef.current.shift();
    }
    histPosRef.current = -1;
    writeToPty(line + "\r");
    setCmd("");
    // Mirror focus back into the terminal output area so it scrolls.
    sessRef.current?.term.scrollToBottom();
  }

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      theme: THEME,
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      cursorStyle: "bar",
      scrollback: 5000,
      allowProposedApi: true,
      convertEol: true,
      windowsMode: IS_WINDOWS,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);

    const session: TermSession = {
      term,
      fit,
      ptyId: 0,
      disposed: false,
      pending: [],
      sawOutput: false,
      cleanup: [],
    };
    sessRef.current = session;

    const tryFit = (): boolean => {
      if (session.disposed) return false;
      if (host.clientWidth < 2 || host.clientHeight < 2) return false;
      try {
        fit.fit();
        if (session.ptyId) void ptyResize(session.ptyId, term.cols, term.rows);
        return term.cols > 0 && term.rows > 0;
      } catch {
        return false;
      }
    };

    // Retry the initial fit until the host has a real measured size, THEN open
    // the PTY so cols/rows are correct from the very first byte.
    let fitTries = 0;
    const ensureFitThen = (next: () => void) => {
      const attempt = () => {
        if (session.disposed) return;
        if (tryFit() || fitTries > 90) {
          next();
          return;
        }
        fitTries += 1;
        requestAnimationFrame(attempt);
      };
      requestAnimationFrame(attempt);
    };

    // Typing directly into xterm still works when focus cooperates; buffer
    // pre-open keystrokes so nothing is lost.
    const onData = term.onData((data) => {
      if (session.disposed) return;
      if (session.ptyId) {
        void ptyWrite(session.ptyId, data).catch(() => {});
      } else {
        session.pending.push(data);
      }
    });
    session.cleanup.push(() => onData.dispose());

    const onResize = term.onResize(({ cols, rows }) => {
      if (!session.disposed && session.ptyId) void ptyResize(session.ptyId, cols, rows);
    });
    session.cleanup.push(() => onResize.dispose());

    const onWinResize = () => tryFit();
    window.addEventListener("resize", onWinResize);
    session.cleanup.push(() => window.removeEventListener("resize", onWinResize));
    const ro = new ResizeObserver(() => tryFit());
    ro.observe(host);
    session.cleanup.push(() => ro.disconnect());

    if (isTauri()) {
      term.writeln("\x1b[90mstarting local shell …\x1b[0m");

      ensureFitThen(() => {
        if (session.disposed) return;
        let bytesSeen = 0;
        ptyOpen(
          (bytes) => {
            if (session.disposed) return;
            if (!session.sawOutput) {
              logFrontend("info", `terminal: first PTY output received (${bytes.length} bytes)`);
            }
            session.sawOutput = true;
            bytesSeen += bytes.length;
            term.write(bytes);
          },
          Math.max(term.cols, 2),
          Math.max(term.rows, 2),
        )
          .then(async (id) => {
            if (session.disposed) {
              await ptyClose(id);
              return;
            }
            session.ptyId = id;
            setReady(true);
            tryFit();
            logFrontend("info", `terminal: PTY ${id} open, ${term.cols}x${term.rows}`);

            const pending = session.pending.splice(0);
            for (const data of pending) await ptyWrite(id, data);

            // Focus the command bar so the user can type immediately.
            inputRef.current?.focus();

            // Some interactive shells (bash -i with pyenv, slow PowerShell
            // profiles) take a few seconds to print the first prompt. Only nudge
            // if truly nothing arrived, and keep it gentle.
            window.setTimeout(() => {
              if (!session.disposed && !session.sawOutput) {
                logFrontend(
                  "warn",
                  `terminal: no PTY output in frontend after 4s (bytesSeen=${bytesSeen}) — transport or render issue`,
                );
                term.writeln(
                  "\r\n\x1b[90m(the shell is taking a moment to start — type a command below and press Enter)\x1b[0m",
                );
              }
            }, 4000);
          })
          .catch((e) => {
            if (session.disposed) return;
            term.writeln(`\r\n\x1b[31mFailed to start terminal: ${String(e)}\x1b[0m`);
            setReady(false);
          });
      });
    } else {
      ensureFitThen(() => {});
      term.writeln("\x1b[32mMatrixHub Client\x1b[0m — real terminal is available in the desktop app.");
    }

    return () => {
      session.disposed = true;
      setReady(false);
      for (const fn of session.cleanup.splice(0)) {
        try {
          fn();
        } catch {
          /* best-effort */
        }
      }
      const id = session.ptyId;
      session.ptyId = 0;
      if (id) void ptyClose(id);
      term.dispose();
      if (sessRef.current === session) sessRef.current = null;
    };
  }, []);

  // Chips send a full command line through the input path.
  function sendChip(c: string) {
    if (!ready) return;
    runCommand(c);
  }

  function onInputKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      e.preventDefault();
      runCommand(cmd);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      const h = historyRef.current;
      if (h.length === 0) return;
      histPosRef.current = histPosRef.current < 0 ? h.length - 1 : Math.max(0, histPosRef.current - 1);
      setCmd(h[histPosRef.current] ?? "");
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      const h = historyRef.current;
      if (histPosRef.current < 0) return;
      histPosRef.current += 1;
      if (histPosRef.current >= h.length) {
        histPosRef.current = -1;
        setCmd("");
      } else {
        setCmd(h[histPosRef.current] ?? "");
      }
    } else if (e.key === "c" && e.ctrlKey) {
      // Forward Ctrl+C to the shell to interrupt the running command.
      e.preventDefault();
      writeToPty("\x03");
    } else if (e.key === "l" && e.ctrlKey) {
      e.preventDefault();
      sessRef.current?.term.clear();
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

        {/* Output area: relative wrapper + absolutely-positioned xterm host so
            it can never collapse to 0px from flex-height quirks. */}
        <div style={{ position: "relative", flex: 1, minHeight: 160 }}>
          <div
            ref={hostRef}
            aria-label="MatrixHub terminal output"
            onPointerDown={() => sessRef.current?.term.focus()}
            style={{ position: "absolute", inset: 0, padding: "8px 10px" }}
          />
        </div>

        {/* Command input bar — the guaranteed place to type. Always works,
            independent of xterm focus quirks in the WebView. */}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            runCommand(cmd);
          }}
          style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 12px", borderTop: "1px solid var(--line)", background: "rgba(0,0,0,0.28)", flexShrink: 0 }}
        >
          <span className="mono" style={{ color: "var(--acc)", fontSize: 13, flexShrink: 0, userSelect: "none" }}>›_</span>
          <input
            ref={inputRef}
            value={cmd}
            onChange={(e) => setCmd(e.target.value)}
            onKeyDown={onInputKeyDown}
            placeholder={ready ? "type a command and press Enter…" : "starting shell…"}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            autoComplete="off"
            disabled={!ready}
            aria-label="Terminal command input"
            style={{
              flex: 1,
              minWidth: 0,
              height: 34,
              background: "var(--inset)",
              border: "1px solid var(--line-2)",
              borderRadius: "var(--r-sm)",
              padding: "0 12px",
              color: "var(--ink)",
              fontFamily: "var(--mono)",
              fontSize: 13,
              outline: "none",
            }}
          />
          <button
            type="submit"
            disabled={!ready}
            aria-label="Send command"
            style={{ flexShrink: 0, display: "inline-flex", alignItems: "center", justifyContent: "center", width: 36, height: 34, borderRadius: "var(--r-sm)", border: "1px solid var(--line-2)", background: ready ? "rgba(41,224,122,0.12)" : "var(--inset)", color: ready ? "var(--acc-bright)" : "var(--ink-4)" }}
          >
            <CI d={IC.arrow} size={16} sw={2} />
          </button>
        </form>

        <div className="term-chips" style={{ display: "flex", gap: 8, overflowX: "auto", padding: "10px 12px", borderTop: "1px solid var(--line)", background: "rgba(0,0,0,0.25)", flexShrink: 0 }}>
          {CHIPS.map((c) => (
            <button
              key={c}
              onClick={() => sendChip(c)}
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
