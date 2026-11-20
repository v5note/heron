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

# Vite runs with strictPort on a fixed port (ui/vite.config.ts) and Tauri's
# devUrl points at it (src-tauri/tauri.conf.json), so a leftover dev server
# makes `tauri dev` fail with "Port 5183 is already in use". Fail early with a
# useful message instead, and offer to reap a stray node process on request.
DEV_PORT=5183
# lsof exits non-zero when nothing is listening. Under `set -e` that status would
# abort the script, so every use of this helper must be able to fail safely.
port_pids() { lsof -nP -tiTCP:"$DEV_PORT" -sTCP:LISTEN 2>/dev/null || true; }

check_dev_port() {
  local pids
  pids="$(port_pids)"
  [ -n "$pids" ] || return 0
  echo "error: port $DEV_PORT is already in use by PID(s):$(printf ' %s' $pids)" >&2
  lsof -nP -iTCP:"$DEV_PORT" -sTCP:LISTEN >&2 || true
  if [ "${KILL_STALE_DEV:-0}" = "1" ]; then
    echo "KILL_STALE_DEV=1: stopping the process(es) above." >&2
    # shellcheck disable=SC2086
    kill $pids 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      [ -n "$(port_pids)" ] || break
      sleep 0.5
    done
    if [ -n "$(port_pids)" ]; then
      echo "error: port $DEV_PORT still busy; stop those processes manually." >&2
      exit 1
    fi
    return 0
  fi
  echo >&2
  echo "A previous dev server probably survived. Either stop it:" >&2
  echo "  kill $pids" >&2
  echo "or re-run with KILL_STALE_DEV=1 to have this script do it:" >&2
  echo "  KILL_STALE_DEV=1 scripts/dev.sh dev" >&2
  exit 1
}

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
    pnpm --dir ui ipc:check
    pnpm --dir ui build
    ;;
  dev)
    # Tauri dev: Vite on :5183 with hot reload, Rust rebuilt on change.
    check_dev_port
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
