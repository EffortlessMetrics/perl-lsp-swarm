#!/usr/bin/env bash
# Compatibility wrapper for dead code detection.
# Canonical implementation: cargo xtask dead-code.

set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

if [ "$#" -eq 0 ]; then
  set -- check
fi

if [ "${DEAD_CODE_STRICT:-false}" = "true" ] && [[ "$*" != *"--strict"* ]]; then
  set -- "$@" --strict
fi

exec cargo xtask dead-code "$@"
