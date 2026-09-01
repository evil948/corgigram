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

APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/korki_*.AppImage 2>/dev/null | head -1)"
if [ -z "$APPIMAGE" ]; then
  APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/Corgigram_*.AppImage 2>/dev/null | head -1)"
fi
if [ -z "$APPIMAGE" ]; then
  APPIMAGE="$(ls -1 "$ROOT"/target/*/release/bundle/appimage/*.AppImage | head -1)"
fi

"$ROOT/scripts/fix-appimage-wayland.sh" "$APPIMAGE"

# Drop lowercase duplicate bundles so tauri-action cannot pick a stale signature.
for dup in "$ROOT"/target/*/release/bundle/appimage/corgigram_*.AppImage; do
  [ -e "$dup" ] || continue
  rm -f "$dup" "$dup.sig" "${dup}.tar.gz" "${dup}.tar.gz.sig"
done
