#!/usr/bin/env bash
# Shared Tauri CLI resolver (npm global or cargo subcommand).
run_tauri() {
  if command -v tauri >/dev/null 2>&1; then
    tauri "$@"
  elif cargo tauri --version >/dev/null 2>&1; then
    cargo tauri "$@"
  else
    echo "error: tauri CLI not found (npm @tauri-apps/cli or cargo-tauri)" >&2
    return 127
  fi
}
