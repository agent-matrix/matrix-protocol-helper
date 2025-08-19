#!/usr/bin/env bash
set -euo pipefail
echo "--- Installing dependencies ---"
pnpm install
echo "--- Starting Tauri in dev mode ---"
pnpm tauri dev
