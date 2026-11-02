#!/usr/bin/env bash
set -euo pipefail

# Use the same repository toolchain and xtask alias for the probe and command.
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$SCRIPT_DIR/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

exec cargo xtask ci-hygiene test-capped "$@"
