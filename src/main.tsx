import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./client.css";

const el = document.getElementById("client-root");
if (el) {
  createRoot(el).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
