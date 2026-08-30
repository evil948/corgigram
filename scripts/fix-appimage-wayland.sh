#!/usr/bin/env bash
# Repack AppImage without bundled libwayland (fixes blank window on Arch/CachyOS + Wayland).
set -euo pipefail

APPIMAGE="${1:?Usage: fix-appimage-wayland.sh path/to/AppImage}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ ! -f "$APPIMAGE" ]; then
  echo "error: AppImage not found: $APPIMAGE" >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "==> Extracting $(basename "$APPIMAGE")"
chmod +x "$APPIMAGE"
"$APPIMAGE" --appimage-extract

echo "==> Removing bundled libwayland (use host libraries instead)"
find squashfs-root/usr/lib -maxdepth 1 -name 'libwayland-*.so*' -delete 2>/dev/null || true

echo "==> Installing Wayland compatibility hook"
mkdir -p squashfs-root/apprun-hooks
install -m 755 "$ROOT/scripts/apprun-wayland-compat.sh" squashfs-root/apprun-hooks/99-wayland-compat.sh

GTK_HOOK="squashfs-root/apprun-hooks/linuxdeploy-plugin-gtk.sh"
if [ -f "$GTK_HOOK" ]; then
  sed -i 's/^export GDK_BACKEND=x11.*$/export GDK_BACKEND="${GDK_BACKEND:-wayland,x11}"/' "$GTK_HOOK"
fi

echo "==> Repacking AppImage"
APPIMAGETOOL="${APPIMAGETOOL:-}"
if [ -z "$APPIMAGETOOL" ] || [ ! -x "$APPIMAGETOOL" ]; then
  APPIMAGETOOL="$WORK/appimagetool.AppImage"
  if [ ! -x "$APPIMAGETOOL" ]; then
    curl -fsSL -o "$APPIMAGETOOL" \
      "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
  fi
fi

OUTPUT="$WORK/repacked.AppImage"
ARCH="${ARCH:-$(uname -m)}" APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" squashfs-root "$OUTPUT"

mv "$OUTPUT" "$APPIMAGE"
chmod +x "$APPIMAGE"

if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "==> Re-signing updater artifacts"
  (
    cd "$ROOT/apps/desktop"
    cargo tauri signer sign "$APPIMAGE"
  )
fi

echo "==> Fixed: $APPIMAGE"
