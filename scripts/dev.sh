#!/usr/bin/env bash
# Local equivalent of CI. Usage: scripts/dev.sh [check|test|ui|bundle]
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$PWD/.cargo-home}"
case "${1:-check}" in
  check) cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings ;;
  test)  cargo test --workspace --all-targets ;;
  ui)    pnpm --dir ui typecheck && pnpm --dir ui i18n:check && pnpm --dir ui build ;;
  bundle) pnpm --dir ui build && cargo tauri build ;;
  *) echo "usage: $0 [check|test|ui|bundle]" >&2; exit 2 ;;
esac
