#!/usr/bin/env bash
# Build all platform artifacts (best effort on current machine)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(grep '^version' "$ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
DIST="$ROOT/dist"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
FLUTTER="${FLUTTER_SDK:-$ROOT/.flutter-sdk}/bin/flutter"
export PATH="${FLUTTER_SDK:-$ROOT/.flutter-sdk}/bin:$PATH"

mkdir -p "$DIST"
cd "$ROOT"

need_pkg() {
  if ! pkg-config --exists "$1" 2>/dev/null; then
    echo "MISSING: $1 — run: sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3 base-devel mingw-w64-gcc jre-openjdk cmake"
    return 1
  fi
  return 0
}

echo "=== korki multi-platform build v${VERSION} ==="

# Linux
echo ">>> Linux CLI + Desktop"
./scripts/build-release-linux.sh

# Windows CLI (.exe) via MinGW cross-compile
if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  echo ">>> Windows CLI (.exe)"
  rustup target add x86_64-pc-windows-gnu 2>/dev/null || true
  WIN_OUT="$DIST/corgigram-${VERSION}-windows-x86_64"
  mkdir -p "$WIN_OUT"
  cargo build --release -p corgigram --target x86_64-pc-windows-gnu
  cp "$TARGET/x86_64-pc-windows-gnu/release/corgigram.exe" "$WIN_OUT/"
  echo "    $WIN_OUT/corgigram.exe"
  echo "NOTE: Desktop .exe requires building on Windows (WebView2). See scripts/build-release-windows.ps1"
else
  echo ">>> Windows SKIPPED — install mingw-w64-gcc"
fi

# Android APK
if [ -n "${JAVA_HOME:-}" ] || command -v java >/dev/null 2>&1; then
  if [ -d "${ANDROID_HOME:-$HOME/Android/Sdk}" ]; then
    echo ">>> Android APK"
    export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
    "$FLUTTER" config --android-sdk "$ANDROID_HOME"
    cd apps/mobile
    "$FLUTTER" pub get
    "$FLUTTER" build apk --release
    APK="$ROOT/apps/mobile/build/app/outputs/flutter-apk/app-release.apk"
    cp "$APK" "$DIST/corgigram-${VERSION}.apk"
    echo "    $DIST/corgigram-${VERSION}.apk"
    cd "$ROOT"
  else
    echo ">>> Android SKIPPED — set up Android SDK (see docs/mobile-build.md)"
  fi
else
  echo ">>> Android SKIPPED — install jre-openjdk"
fi

echo ">>> iOS (.ipa)"
echo "    IMPOSSIBLE on Linux — requires macOS + Xcode. Build on Mac: flutter build ipa"

echo
echo "=== Done ==="
ls -la "$DIST"/ 2>/dev/null || true
