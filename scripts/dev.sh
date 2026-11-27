#!/usr/bin/env bash
# Local equivalents of CI plus the two development entry points.
#
#   scripts/dev.sh check    fmt + clippy (same as CI)
#   scripts/dev.sh test     all workspace tests (same as CI)
#   scripts/dev.sh ui       frontend typecheck + i18n parity + build (same as CI)
#   scripts/dev.sh dev      run the desktop app against the Vite dev server (hot reload;
#                           without tauri-cli it starts Vite + `cargo run -p heron-app` itself)
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

# cargo resolves `cargo <subcommand>` through PATH alone, and ADR-0001 lets CARGO_HOME
# be the in-repo .cargo-home, whose bin/ is usually not on PATH. Without this, a
# perfectly installed tauri-cli answers "error: no such command: `tauri`".
case ":$PATH:" in
  *":$CARGO_HOME/bin:"*) ;;
  *) [ -d "$CARGO_HOME/bin" ] && export PATH="$CARGO_HOME/bin:$PATH" ;;
esac

# Vite runs with strictPort on a fixed port (ui/vite.config.ts) and Tauri's
# devUrl points at it (src-tauri/tauri.conf.json), so a leftover dev server
# makes `tauri dev` fail with "Port 5183 is already in use". Fail early with a
# useful message instead, and offer to reap a stray node process on request.
DEV_PORT=5183
# lsof exits non-zero when nothing is listening. Under `set -e` that status would
# abort the script, so every use of this helper must be able to fail safely.
port_pids() { lsof -nP -tiTCP:"$DEV_PORT" -sTCP:LISTEN 2>/dev/null || true; }

have_tauri() {
  # HERON_NO_TAURI=1 exercises the fallback on a machine that does have tauri-cli.
  [ "${HERON_NO_TAURI:-0}" = "1" ] && return 1
  cargo tauri --version >/dev/null 2>&1
}

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

# ui/ is a standalone pnpm project (no workspace file), so a fresh clone has no
# ui/node_modules and Vite dies as "command not found: vite" deep inside `dev`.
# Check up front and run the install the README asks for.
ensure_ui_deps() {
  local marker=ui/node_modules/vite/package.json
  [ -f "$marker" ] && return 0
  echo "ui/node_modules is missing or incomplete: running pnpm --dir ui install ..." >&2
  pnpm --dir ui install || { echo "error: pnpm install failed." >&2; return 1; }
  [ -f "$marker" ] || { echo "error: vite is still missing after install; check the pnpm output above." >&2; return 1; }
}

# `tauri dev` does two things for us: keep Vite on :5183 and rebuild/launch the shell.
# The debug binary already reads devUrl from tauri.conf.json, so running both by hand
# costs only the Rust-side hot reload (re-run `dev` after editing Rust).
MANUAL_DEV_PID=''
stop_manual_dev() {
  # pnpm is a wrapper around node. Never trust a stored PID on this exit path: the
  # shell may already be gone and the number reused, and the last thing a finished
  # dev run should do is signal a stranger's process.
  [ -n "$MANUAL_DEV_PID" ] || return 0
  kill -0 "$MANUAL_DEV_PID" 2>/dev/null && kill "$MANUAL_DEV_PID" 2>/dev/null || true
  wait "$MANUAL_DEV_PID" 2>/dev/null || true
  MANUAL_DEV_PID=''
  return 0
}

run_manual_dev() {
  command -v cargo >/dev/null 2>&1 || { echo "error: 'cargo' not found on PATH." >&2; exit 127; }
  command -v pnpm >/dev/null 2>&1 || { echo "error: 'pnpm' not found on PATH." >&2; exit 127; }
  echo "tauri-cli not installed: starting Vite and 'cargo run -p heron-app' directly." >&2
  echo "  the backend will not hot-reload; re-run this command after editing Rust." >&2
  echo "  cargo install tauri-cli --version '^2' --locked   # to get the full tauri dev flow" >&2
  echo >&2
  pnpm --dir ui dev &
  MANUAL_DEV_PID=$!
  trap stop_manual_dev EXIT INT TERM
  local rc=0
  # A cold Vite start pre-bundles dependencies, which is slow on Windows: 2 minutes.
  for _ in $(seq 1 240); do
    [ -n "$(port_pids)" ] && break
    kill -0 "$MANUAL_DEV_PID" 2>/dev/null || break
    sleep 0.5
  done
  if [ -z "$(port_pids)" ]; then
    echo "error: the Vite dev server never listened on :$DEV_PORT; see its output above." >&2
    exit 1
  fi
  cargo run -p heron-app || rc=$?
  stop_manual_dev
  # node can outlive the pnpm wrapper; the port was free when we started, so any
  # listener left behind is ours and must not block the next `dev` run.
  local left
  left="$(port_pids)"
  # shellcheck disable=SC2086
  [ -n "$left" ] && kill $left 2>/dev/null || true
  return $rc
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
    ensure_ui_deps
    pnpm --dir ui typecheck
    pnpm --dir ui i18n:check
    pnpm --dir ui ipc:check
    pnpm --dir ui build
    ;;
  dev)
    # Tauri dev: Vite on :5183 with hot reload, Rust rebuilt on change.
    check_dev_port
    ensure_ui_deps
    if have_tauri; then
      exec cargo tauri dev
    fi
    run_manual_dev
    ;;
  web)
    ensure_ui_deps
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
    ensure_ui_deps
    pnpm --dir ui build
    exec cargo tauri build
    ;;
  *)
    echo "usage: $0 [check|test|ui|dev|web|cli|bundle]" >&2
    exit 2
    ;;
esac
