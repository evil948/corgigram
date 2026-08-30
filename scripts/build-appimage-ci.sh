#!/usr/bin/env bash
# Called by tauri-action on Linux CI: build AppImage and patch Wayland/EGL compatibility.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export NO_STRIP=1

cd "$ROOT/apps/desktop"
cargo tauri build "$@"

APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/*.AppImage | head -1)"
"$ROOT/scripts/fix-appimage-wayland.sh" "$APPIMAGE"
