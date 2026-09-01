#!/usr/bin/env bash
# Called by tauri-action on Linux CI: build AppImage and patch Wayland/EGL compatibility.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export NO_STRIP=1

# shellcheck source=lib/tauri.sh
source "$ROOT/scripts/lib/tauri.sh"

cd "$ROOT/apps/desktop"
# tauri-action invokes this script as: build-appimage-ci.sh build --target ...
run_tauri "$@"

APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/*.AppImage | head -1)"
"$ROOT/scripts/fix-appimage-wayland.sh" "$APPIMAGE"
