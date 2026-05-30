/* ============================================================
   UpdateModal.tsx — premium, AAA-style auto-update experience.

   - UpdateToast: a dismissible "new version available" notification.
   - UpdateModal: changelog + one-click "Update now" with a live,
     animated download/install progress bar, then auto-relaunch.
   ============================================================ */
import { useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CI, IC } from "../icons";
import { installUpdate, type UpdateInfo, type UpdateProgress } from "../lib/tauri";

/* ---------- "update available" toast ---------- */
export function UpdateToast({
  info,
  onOpen,
  onDismiss,
}: {
  info: UpdateInfo;
  onOpen: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="upd-toast rise">
      <span className="upd-toast-glyph">
        <CI d={IC.spark} size={18} />
      </span>
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ fontSize: 13, fontWeight: 700, color: "var(--ink)" }}>Update available</div>
        <div className="mono" style={{ fontSize: 11.5, color: "var(--ink-3)", marginTop: 2 }}>
          v{info.currentVersion} → <span style={{ color: "var(--acc-bright)" }}>v{info.version}</span>
        </div>
      </div>
      <button className="btn btn-primary" style={{ height: 32, fontSize: 12.5, padding: "0 13px" }} onClick={onOpen}>
        What’s new
      </button>
      <button className="navitem" style={{ width: 28, height: 28, padding: 0, justifyContent: "center", flex: "none" }} onClick={onDismiss} aria-label="Dismiss">
        <CI d={IC.x} size={14} />
      </button>
    </div>
  );
}

/* ---------- full update modal ---------- */
export function UpdateModal({
  info,
  onClose,
}: {
  info: UpdateInfo;
  onClose: () => void;
}) {
  const [phase, setPhase] = useState<"notes" | "working" | "error">("notes");
  const [pct, setPct] = useState(0);
  const [status, setStatus] = useState("Preparing…");
  const [err, setErr] = useState("");
  const indeterminate = useRef(false);

  async function begin() {
    setPhase("working");
    setStatus("Starting download…");
    indeterminate.current = false;
    try {
      await installUpdate((p: UpdateProgress) => {
        if (p.phase === "install") {
          setPct(100);
          setStatus("Installing…");
        } else if (p.total) {
          setPct(p.pct);
          setStatus(`Downloading… ${p.pct}%`);
        } else {
          indeterminate.current = true;
          setStatus("Downloading…");
        }
      });
      // On success the backend relaunches the app, so we rarely reach here.
      setStatus("Restarting…");
    } catch (e) {
      setErr(String(e));
      setPhase("error");
    }
  }

  return createPortal(
    <div className="scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && phase !== "working") onClose(); }}>
      <div className="rise upd-card" style={{ width: "100%", maxWidth: 460 }}>
        {/* hero */}
        <div className="upd-hero">
          <div className="upd-hero-glow" />
          <div className="upd-badge">
            <CI d={IC.download} size={26} sw={2} />
          </div>
          <div style={{ fontSize: 11, fontWeight: 700, letterSpacing: "0.16em", textTransform: "uppercase", color: "var(--acc)" }}>
            New version
          </div>
          <div style={{ fontSize: 22, fontWeight: 700, marginTop: 4 }}>MatrixHub Client v{info.version}</div>
          <div className="mono" style={{ fontSize: 12, color: "var(--ink-3)", marginTop: 4 }}>
            you’re on v{info.currentVersion}
            {info.date ? ` · released ${info.date.slice(0, 10)}` : ""}
          </div>
        </div>

        <div style={{ padding: "18px 22px 22px" }}>
          {phase === "notes" && (
            <>
              <div style={{ fontSize: 12, fontWeight: 700, color: "var(--ink-2)", textTransform: "uppercase", letterSpacing: "0.08em" }}>
                What’s new
              </div>
              <div className="upd-notes">
                {info.notes ? info.notes : "Performance improvements and bug fixes."}
              </div>
              <div style={{ display: "flex", gap: 10, marginTop: 18 }}>
                <button className="btn btn-primary" style={{ flex: 1 }} onClick={begin}>
                  <CI d={IC.download} size={16} /> Update now
                </button>
                <button className="btn btn-ghost" onClick={onClose}>Later</button>
              </div>
              <p style={{ textAlign: "center", marginTop: 12, fontSize: 11.5, color: "var(--ink-4)" }}>
                Signed update · verified before install
              </p>
            </>
          )}

          {phase === "working" && (
            <div style={{ paddingTop: 6 }}>
              <div className={"bar" + (indeterminate.current ? " upd-bar-indet" : "")} style={{ height: 9 }}>
                <span style={{ width: pct + "%" }} />
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", marginTop: 10 }}>
                <span style={{ fontSize: 13, fontWeight: 600, color: "var(--acc-bright)" }}>{status}</span>
                {!indeterminate.current && <span className="mono" style={{ fontSize: 12, color: "var(--ink-3)" }}>{pct}%</span>}
              </div>
              <p style={{ textAlign: "center", marginTop: 16, fontSize: 11.5, color: "var(--ink-4)" }}>
                The app will restart automatically to finish updating.
              </p>
            </div>
          )}

          {phase === "error" && (
            <>
              <div className="logbox" style={{ height: 120, fontSize: 11.5, color: "var(--red)" }}>{err}</div>
              <div style={{ display: "flex", gap: 10, marginTop: 16 }}>
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
