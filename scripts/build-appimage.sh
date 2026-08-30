#!/usr/bin/env bash
# AppImage + OTA updater artifacts (Linux)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/apps/desktop"

# linuxdeploy ships an old strip that breaks on Arch/CachyOS (.relr.dyn).
export NO_STRIP=1

if [ -f "$ROOT/.tauri/updater.key" ] && [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  export TAURI_SIGNING_PRIVATE_KEY
  TAURI_SIGNING_PRIVATE_KEY="$(cat "$ROOT/.tauri/updater.key")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
fi

echo "==> Building AppImage (NO_STRIP=1 for Arch/CachyOS)"
cargo tauri build

echo
echo "Done:"
ls -la "$ROOT/target/release/bundle/appimage/" 2>/dev/null || true
