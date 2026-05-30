import { createRoot } from "react-dom/client";
import App from "./App";
import "./client.css";

// NOTE: React.StrictMode is intentionally NOT used here.
//
// StrictMode double-invokes effects in development (mount → unmount →
// remount). For ordinary components that is a useful correctness check, but the
// Terminal view drives an *imperative* widget — it calls `term.open(host)` and
// spawns a backend PTY inside its effect. Double-invoking that opens two PTYs
// over the same DOM node and lets the first teardown dispose the terminal the
// second mount is using, so streamed shell output lands on a wiped/disposed
// screen (the "starting local shell …" that never advances). Production never
// double-invokes, so dropping StrictMode makes dev behave like the shipped app.
const el = document.getElementById("client-root");
if (el) {
  createRoot(el).render(<App />);
}
