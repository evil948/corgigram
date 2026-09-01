#!/usr/bin/env bash
# korki release build — Linux (native)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
OUT="$ROOT/dist/corgigram-${VERSION}-linux-x86_64"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"

cd "$ROOT"
mkdir -p "$OUT"

echo "==> korki release ${VERSION} (Linux x86_64)"
echo "    Output: $OUT"
echo

echo "==> cargo test (default workspace)"
cargo test

echo "==> CLI release"
cargo build --release -p corgigram
install -Dm755 "$TARGET/release/corgigram" "$OUT/corgigram"

DESKTOP_OK=0
if pkg-config --exists javascriptcoregtk-4.1 2>/dev/null; then
  echo "==> Desktop release (Tauri)"
  cargo build --release -p corgigram-desktop
  install -Dm755 "$TARGET/release/corgigram-desktop" "$OUT/corgigram-desktop"
  DESKTOP_OK=1

  if command -v cargo-tauri >/dev/null 2>&1; then
    echo "==> Tauri bundle (.deb / .AppImage)"
    (cd apps/desktop && cargo tauri build --ci 2>/dev/null) || true
    if [ -d "$TARGET/release/bundle" ]; then
      cp -a "$TARGET/release/bundle" "$OUT/tauri-bundle" 2>/dev/null || true
    fi
  fi
else
  echo "==> Desktop SKIPPED — install WebKit GTK:"
  echo "    sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3 base-devel"
fi

cp "$ROOT/docs/release-test.md" "$OUT/TESTING.md"
cat > "$OUT/README.txt" <<EOF
korki ${VERSION} — Linux x86_64

  corgigram          CLI (WebRTC + E2E demo)
  corgigram-desktop  GUI (если собран)

Данные: ~/.local/share/corgigram/
Firebase настроен по умолчанию.

См. TESTING.md — как протестировать Linux + Windows.
EOF

echo
echo "Done."
echo "  CLI:     $OUT/corgigram"
if [ "$DESKTOP_OK" = 1 ]; then
  echo "  Desktop: $OUT/corgigram-desktop"
fi
ls -la "$OUT"
