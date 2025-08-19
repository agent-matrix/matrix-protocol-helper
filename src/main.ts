import { listen } from "@tauri-apps/api/event";
import { appWindow } from "@tauri-apps/api/window";

/**
 * Appends a line of text to the log element and auto-scrolls to the bottom.
 * @param logEl The <pre> element to append log lines to.
 * @param line The string to append to the log.
 */
function appendLog(logEl: HTMLPreElement, line: string) {
  // Normalize line endings to ensure consistent display.
  logEl.textContent += line.replace(/\r\n?|\n/g, "\n") + "\n";
  // Keep the most recent log output visible.
  logEl.scrollTop = logEl.scrollHeight;
}

/**
 * Main async function to set up event listeners from the Rust backend.
 */
async function setupListeners() {
  // Get references to the DOM elements we'll be updating.
  const logEl = document.getElementById("log") as HTMLPreElement;
  const statusEl = document.getElementById("status") as HTMLDivElement;

  // Set an initial status message.
  if (statusEl) {
    statusEl.textContent = "🚀 Waiting for installation to begin...";
  }

  // Listen for 'log-line' events emitted by the Rust backend during CLI execution.
  await listen<string>("log-line", (event) => {
    if (logEl) {
        appendLog(logEl, event.payload || "");
    }
  });

  // Listen for the final 'install-complete' event.
  await listen<{ ok: boolean; code: number; alias: string }>("install-complete", (event) => {
    if (!statusEl) return;
    const { ok, code, alias } = event.payload;

    // Update the status text and apply the appropriate CSS class.
    statusEl.textContent = ok
      ? `✅ Install complete. Alias '${alias}' is ready.`
      : `❌ Install failed (exit code: ${code}). See logs for details.`;
    statusEl.className = ok ? "small ok" : "small fail";

    // On success, automatically close the progress window after a short delay.
    if (ok) {
      setTimeout(() => {
        appWindow.close();
      }, 2500);
    }
  });
}

// Wait for the DOM to be fully loaded before running the setup script.
// This is crucial to ensure that document.getElementById() can find the elements.
document.addEventListener("DOMContentLoaded", () => {
  setupListeners();
});
