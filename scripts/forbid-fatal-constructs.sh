#!/usr/bin/env bash
# Compatibility wrapper for forbidden-fatal-construct checks.
# Canonical implementation: cargo xtask forbid-fatal-constructs.

set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

exec cargo xtask forbid-fatal-constructs -- "$@"
