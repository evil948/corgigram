#!/usr/bin/env bash
# Install Corgigram CLI + desktop to user local paths
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
DATA_DIR="$HOME/.local/share/corgigram"

mkdir -p "$BIN_DIR" "$APP_DIR" "$DATA_DIR"

echo "==> Building CLI..."
cd "$ROOT"
cargo build --release -p corgigram
install -Dm755 "$TARGET/release/corgigram" "$BIN_DIR/corgigram"

if pkg-config --exists javascriptcoregtk-4.1 2>/dev/null; then
  echo "==> Building desktop..."
  cargo build --release -p corgigram-desktop
  install -Dm755 "$TARGET/release/corgigram-desktop" "$BIN_DIR/corgigram-desktop"

  cat > "$APP_DIR/corgigram.desktop" <<EOF
[Desktop Entry]
Name=Corgigram
Comment=E2E messenger
Exec=$BIN_DIR/corgigram-desktop
Icon=internet-mail
Terminal=false
Type=Application
Categories=Network;InstantMessaging;
EOF
  echo "Desktop: $BIN_DIR/corgigram-desktop"
  echo "Menu entry: $APP_DIR/corgigram.desktop"
else
  echo "WARN: webkit2gtk-4.1 not found — desktop not built."
  echo "Run: sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3 base-devel"
  echo "Then re-run: ./scripts/install-linux.sh"
fi

echo
echo "Installed CLI: $BIN_DIR/corgigram"
echo "Ensure \$HOME/.local/bin is in PATH."
