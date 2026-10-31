#!/usr/bin/env bash
# Thin wrapper (#15350): the canonical typed receipt producer is
# `cargo xtask receipts`. The previous fail-open shell implementation is
# replaced; it parsed partial output into complete-looking summaries and
# ignored producer exit statuses. Flags pass through (e.g. --tests-only,
# --docs-only, --output-dir).
# Usage: ./scripts/generate-receipts.sh [--tests-only|--docs-only]

set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

exec cargo xtask receipts "$@"
