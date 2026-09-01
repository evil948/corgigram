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

APPIMAGE="$(ls -1 "$ROOT"/target/release/bundle/appimage/korki_*.AppImage 2>/dev/null | head -1)"
if [ -z "$APPIMAGE" ]; then
  APPIMAGE="$(ls -1 "$ROOT"/target/release/bundle/appimage/Corgigram_*.AppImage 2>/dev/null | head -1)"
fi
if [ -z "$APPIMAGE" ]; then
  APPIMAGE="$(ls -1 "$ROOT"/target/release/bundle/appimage/*.AppImage 2>/dev/null | head -1)"
fi
if [ -n "$APPIMAGE" ]; then
  echo "==> Patching AppImage for Wayland hosts"
  "$ROOT/scripts/fix-appimage-wayland.sh" "$APPIMAGE"
fi

for dup in "$ROOT"/target/release/bundle/appimage/corgigram_*.AppImage; do
  [ -e "$dup" ] || continue
  rm -f "$dup" "$dup.sig" "${dup}.tar.gz" "${dup}.tar.gz.sig"
done

echo
echo "Done:"
ls -la "$ROOT/target/release/bundle/appimage/" 2>/dev/null || true
