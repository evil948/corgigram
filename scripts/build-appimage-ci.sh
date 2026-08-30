#!/usr/bin/env bash
# Called by tauri-action on Linux CI: build AppImage and patch Wayland/EGL compatibility.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export NO_STRIP=1

if ! cargo tauri --version >/dev/null 2>&1; then
  echo "==> Installing Tauri CLI"
  cargo install tauri-cli --locked
fi

cd "$ROOT/apps/desktop"
# tauri-action invokes this script as: build-appimage-ci.sh build --target ...
cargo tauri "$@"

APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/*.AppImage | head -1)"
"$ROOT/scripts/fix-appimage-wayland.sh" "$APPIMAGE"
