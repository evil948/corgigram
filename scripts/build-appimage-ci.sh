#!/usr/bin/env bash
# Called by tauri-action on Linux CI: build AppImage and patch Wayland/EGL compatibility.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export NO_STRIP=1

run_tauri() {
  if command -v tauri >/dev/null 2>&1; then
    tauri "$@"
  elif cargo tauri --version >/dev/null 2>&1; then
    cargo tauri "$@"
  else
    echo "==> Installing Tauri CLI (npm)"
    npm install -g @tauri-apps/cli@2
    tauri "$@"
  fi
}

cd "$ROOT/apps/desktop"
# tauri-action invokes this script as: build-appimage-ci.sh build --target ...
run_tauri "$@"

APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/*.AppImage | head -1)"
"$ROOT/scripts/fix-appimage-wayland.sh" "$APPIMAGE"
