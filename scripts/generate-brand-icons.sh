#!/usr/bin/env bash
# Regenerate korki app icons from apps/desktop/icons/icon.svg
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICON_DIR="$ROOT/apps/desktop/icons"
UI_ASSETS="$ROOT/apps/desktop/ui/assets"

rsvg-convert -w 1024 -h 1024 "$ICON_DIR/icon.svg" -o "$ICON_DIR/icon.png"
rsvg-convert -w 32 -h 32 "$ICON_DIR/icon.svg" -o "$ICON_DIR/32x32.png"
rsvg-convert -w 128 -h 128 "$ICON_DIR/icon.svg" -o "$ICON_DIR/128x128.png"
rsvg-convert -w 256 -h 256 "$ICON_DIR/icon.svg" -o "$ICON_DIR/128x128@2x.png"
cp "$ICON_DIR/32x32.png" "$UI_ASSETS/favicon-32.png"

for sz in 16 32 48 64 128 256; do
  rsvg-convert -w "$sz" -h "$sz" "$ICON_DIR/icon.svg" -o "/tmp/korki-$sz.png"
done
magick /tmp/korki-16.png /tmp/korki-32.png /tmp/korki-48.png /tmp/korki-64.png \
  /tmp/korki-128.png /tmp/korki-256.png "$ICON_DIR/icon.ico"

echo "==> Icons updated in $ICON_DIR"
