#!/usr/bin/env bash
set -euo pipefail
echo "--- Installing dependencies ---"
pnpm install
echo "--- Building Tauri application ---"
pnpm tauri build
