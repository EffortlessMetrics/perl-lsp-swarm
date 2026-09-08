#!/usr/bin/env bash
# Compatibility shim for the Rust gate runner.
#
# Usage:
#   RUN_FULL=1 ./scripts/run-gates.sh   # optional all-tier gate
#   ./scripts/run-gates.sh --format json

set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIER="merge-gate"
if [[ "${RUN_FULL:-}" == "1" ]]; then
    TIER="all"
fi

cd "$ROOT"
exec cargo xtask gates \
    --tier "$TIER" \
    --receipt \
    --receipt-path "$ROOT/target/receipts/receipt.json" \
    "$@"
