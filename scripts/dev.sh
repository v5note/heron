#!/usr/bin/env bash
# Local equivalents of CI plus the two development entry points.
#
#   scripts/dev.sh check    fmt + clippy (same as CI)
#   scripts/dev.sh test     all workspace tests (same as CI)
#   scripts/dev.sh ui       frontend typecheck + i18n parity + build (same as CI)
#   scripts/dev.sh dev      run the desktop app against the Vite dev server (hot reload)
#   scripts/dev.sh web      run only the frontend in a browser (no Rust backend)
#   scripts/dev.sh cli      build and run the CLI against an isolated data dir
#   scripts/dev.sh bundle   produce a signed-capable bundle (needs tauri-cli)
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$PWD/.cargo-home}"
# Keep the toolchain stable even when a developer runs with a custom HOME.
if [ -z "${RUSTUP_HOME:-}" ] && [ -d "${HOME:-/nonexistent}/.rustup" ]; then
  export RUSTUP_HOME="$HOME/.rustup"
fi

have_tauri() { cargo tauri --version >/dev/null 2>&1; }

case "${1:-check}" in
  check)
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    ;;
  test)
    cargo test --workspace --all-targets
    ;;
  ui)
    pnpm --dir ui typecheck
    pnpm --dir ui i18n:check
    pnpm --dir ui build
    ;;
  dev)
    # Tauri dev: Vite on :5183 with hot reload, Rust rebuilt on change.
    if have_tauri; then
      exec cargo tauri dev
    fi
    echo "tauri-cli not installed; falling back to the manual two-process flow:" >&2
    echo "  terminal 1: pnpm --dir ui dev" >&2
    echo "  terminal 2: cargo run -p heron-app" >&2
    echo >&2
    echo "cargo install tauri-cli --version '^2' --locked   # to enable hot-reloading backend" >&2
    exit 1
    ;;
  web)
    exec pnpm --dir ui dev
    ;;
  cli)
    cargo build -p heron-cli
    exec ./target/debug/heron --data-dir "${HERON_DEV_DATA:-/tmp/heron-dev}" "$@"
    ;;
  bundle)
    if ! have_tauri; then
      echo "bundle needs tauri-cli: cargo install tauri-cli --version '^2' --locked" >&2
      exit 1
    fi
    pnpm --dir ui build
    exec cargo tauri build
    ;;
  *)
    echo "usage: $0 [check|test|ui|dev|web|cli|bundle]" >&2
    exit 2
    ;;
esac
