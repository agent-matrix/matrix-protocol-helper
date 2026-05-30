/* ============================================================
   Console.tsx — embedded Matrix CLI terminal.
   Streams real matrix-cli output via the Rust `run_command`.
   ============================================================ */
import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { CI, IC } from "../icons";
import { runCommand } from "../lib/tauri";

type Tone = "user" | "sys" | "ok" | "warn" | "err" | "dim" | "matrix";
interface Block {
  tone: Tone;
  lines: string[];
}

const T_TONE: Record<Tone, string> = {
  user: "var(--acc-bright)",
  sys: "var(--ink-2)",
  ok: "var(--acc-bright)",
  warn: "var(--amber)",
  err: "var(--red)",
  dim: "var(--ink-3)",
  matrix: "var(--ink)",
};

const T_BOOT = [
  "matrix-cli · local runner",
  "Ask Matrix in plain English, or run a command.",
];

// Valid matrix-cli commands (see matrix_cli/__main__.py). `list` and
// `mcp test` are not real subcommands (they exit with code 2); groups use
// `--help` so a chip never errors.
const CHIPS = [
  "matrix help",
  "matrix --version",
  "matrix search github",
  "matrix ps",
  "matrix connection --help",
  "matrix mcp probe --help",
];

function TermLine({ text, tone }: { text: string; tone: Tone }) {
  return (
    <p style={{ margin: 0, color: T_TONE[tone] || "var(--ink)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
      {tone === "user" && <span style={{ color: "var(--acc-dim)" }}>{"┌─ "}</span>}
      {text}
    </p>
  );
}

export function ClientConsole({ hubUrl }: { hubUrl: string }) {
  const [value, setValue] = useState("");
  const [history, setHistory] = useState<Block[]>([]);
  const [booted, setBooted] = useState(false);
  const [bootShown, setBootShown] = useState<string[]>([]);
  const [streaming, setStreaming] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const cmdHist = useRef<string[]>([]);
  const cmdIdx = useRef(-1);

  // boot typewriter (line-by-line)
  useEffect(() => {
    let i = 0;
    let timer: ReturnType<typeof setTimeout>;
    function step() {
      setBootShown(T_BOOT.slice(0, i + 1));
      i++;
      if (i < T_BOOT.length) timer = setTimeout(step, 260);
      else {
        setBooted(true);
        inputRef.current?.focus();
      }
    }
    timer = setTimeout(step, 200);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (scroller.current) scroller.current.scrollTop = scroller.current.scrollHeight;
  }, [history, bootShown, streaming]);

  function appendToLast(line: string) {
    setHistory((h) => {
      const copy = h.slice();
      const last = copy[copy.length - 1];
      if (last) copy[copy.length - 1] = { ...last, lines: [...last.lines, line] };
      return copy;
    });
  }

  async function run(raw: string) {
    const clean = raw.trim();
    if (!clean || streaming) return;
    cmdHist.current.push(clean);
    cmdIdx.current = cmdHist.current.length;
    setValue("");
    if (clean.toLowerCase() === "clear" || clean === "/clear") {
      setHistory([]);
      return;
    }
    setHistory((h) => [...h, { tone: "user", lines: [clean] }, { tone: "matrix", lines: [] }]);
    setStreaming(true);
    try {
      const code = await runCommand(clean, (line) => appendToLast(line));
      if (code !== 0) appendToLast(`exited with code ${code}`);
    } catch (e) {
      appendToLast(String(e));
    } finally {
      setStreaming(false);
      inputRef.current?.focus();
    }
  }

  function onKey(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (cmdIdx.current > 0) {
        cmdIdx.current--;
        setValue(cmdHist.current[cmdIdx.current] || "");
      }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (cmdIdx.current < cmdHist.current.length - 1) {
        cmdIdx.current++;
        setValue(cmdHist.current[cmdIdx.current] || "");
      } else {
        cmdIdx.current = cmdHist.current.length;
        setValue("");
      }
    }
  }

  return (
    <div className="view-pad rise" style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0, height: "100%", maxWidth: 820, paddingBottom: 26 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, flexShrink: 0 }}>
        <div>
          <p className="eyebrow">Local runner</p>
          <h1 className="h1" style={{ marginTop: 8 }}>Terminal</h1>
        </div>
        <span className="pill ok">
          <span className="dot on" /> online
        </span>
      </div>

      {/* terminal surface */}
      <div style={{ marginTop: 18, flex: 1, minHeight: 0, display: "flex", flexDirection: "column", background: "var(--inset)", border: "1px solid var(--line-2)", borderRadius: "var(--r-md)", overflow: "hidden" }}>
        {/* strip */}
        <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "9px 14px", borderBottom: "1px solid var(--line)", background: "linear-gradient(180deg, rgba(41,224,122,0.04), transparent)", flexShrink: 0 }}>
          <span style={{ display: "inline-flex", color: "var(--acc)" }}>
            <CI d={IC.cli} size={15} sw={2} />
          </span>
          <span className="mono" style={{ fontSize: 12, color: "var(--ink-2)", fontWeight: 600, letterSpacing: "0.04em" }}>matrix · local session</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 11, color: "var(--ink-4)" }}>~/.matrix</span>
        </div>

        {/* output */}
        <div ref={scroller} style={{ flex: 1, overflowY: "auto", padding: "16px 16px", fontFamily: "var(--mono)", fontSize: 12.5, lineHeight: 1.75 }}>
          <div style={{ marginBottom: 14 }}>
            <p style={{ margin: 0, color: "var(--acc-dim)" }}>hub {hubUrl}</p>
            {bootShown.map((l, i) => (
              <p key={i} style={{ margin: 0, color: "var(--ink-3)", whiteSpace: "pre-wrap" }}>{l}</p>
            ))}
            {!booted && <span className="cur" />}
          </div>
          {history.map((item, idx) => (
            <div key={idx} style={{ marginBottom: 12 }}>
              {item.lines.map((line, li) => (
                <TermLine key={li} text={line} tone={item.tone} />
              ))}
            </div>
          ))}
          {streaming && <span className="cur" />}
        </div>

        {/* composer */}
        <div style={{ borderTop: "1px solid var(--line)", background: "rgba(0,0,0,0.25)", padding: 12, flexShrink: 0 }}>
          <div className="term-chips" style={{ display: "flex", gap: 8, overflowX: "auto", marginBottom: 10, paddingBottom: 2 }}>
            {CHIPS.map((c) => (
              <button
                key={c}
                onClick={() => run(c)}
                disabled={streaming}
                style={{ flexShrink: 0, padding: "6px 11px", borderRadius: 99, border: "1px solid var(--line-2)", background: "rgba(41,224,122,0.04)", color: "var(--ink-2)", fontFamily: "var(--mono)", fontSize: 11.5, opacity: streaming ? 0.5 : 1, transition: "all .15s" }}
              >
                {c}
              </button>
            ))}
          </div>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              run(value);
            }}
            style={{ display: "flex", alignItems: "center", gap: 10, height: 46, padding: "0 13px", borderRadius: "var(--r-sm)", border: "1px solid var(--line-2)", background: "var(--app)" }}
          >
            <span className="mono" style={{ fontSize: 13.5, color: "var(--acc)", fontWeight: 700 }}>
              matrix<span style={{ color: "var(--acc-dim)" }}>&gt;</span>
            </span>
            <input
              ref={inputRef}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={onKey}
              placeholder="ask anything, or run a command…"
              spellCheck={false}
              style={{ flex: 1, minWidth: 0, height: "100%", background: "transparent", border: "none", outline: "none", color: "var(--acc-bright)", fontFamily: "var(--mono)", fontSize: 13.5 }}
            />
            <button
              type="submit"
              aria-label="Run"
              disabled={streaming}
              style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", width: 34, height: 34, borderRadius: "var(--r-sm)", background: "var(--acc)", color: "#052012", opacity: streaming ? 0.5 : 1 }}
            >
              <CI d={IC.arrow} size={15} sw={2.2} />
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
