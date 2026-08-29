#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MOBILE="$ROOT/apps/mobile"
FLUTTER_SDK="${FLUTTER_SDK:-$ROOT/.flutter-sdk}"

export PATH="$FLUTTER_SDK/bin:$PATH"

if ! command -v flutter >/dev/null 2>&1; then
  echo "Flutter not found. Set FLUTTER_SDK or clone to $ROOT/.flutter-sdk"
  exit 1
fi

cd "$MOBILE"

echo "==> flutter_rust_bridge codegen"
flutter_rust_bridge_codegen generate

echo "==> flutter pub get"
flutter pub get

echo "==> rust bridge (debug)"
cd rust && cargo build && cd ..

echo "Done. Run: cd apps/mobile && flutter run"
