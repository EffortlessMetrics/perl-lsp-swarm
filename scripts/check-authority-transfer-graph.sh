#!/usr/bin/env bash
# Validate the stable authority-transfer programme graph (#11697).
#
# Canonical validator: xtask/src/bin/authority-transfer-graph.rs
# Stable manifest:     .ci/authority-transfer-programme/graph.v1.json
# Committed projection: .ci/authority-transfer-programme/generated/normalized-graph.v1.json
#
# The gate fails when the stable graph drifts from its committed normalized
# projection, when any shift-left fixture stops rejecting for exactly its
# pinned typed reason, or when the positive control accepts an invalid graph.
set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

readonly BIN="xtask/src/bin/authority-transfer-graph.rs"
readonly TEST_NAME="authority-transfer-graph"

echo "==> rustfmt --check $BIN"
rustfmt --edition 2024 --check "$BIN"

echo "==> cargo test -p xtask --bin $TEST_NAME --locked"
cargo test -p xtask --bin "$TEST_NAME" --locked

echo "==> cargo run -p xtask --bin $TEST_NAME -- check"
cargo run -q -p xtask --bin "$TEST_NAME" -- check
