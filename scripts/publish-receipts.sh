#!/usr/bin/env bash
# Compatibility wrapper for review receipt publication.
# Canonical implementation: cargo xtask publish-receipts.

set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

if [ $# -gt 0 ]; then
  exec cargo xtask publish-receipts "$1"
else
  exec cargo xtask publish-receipts
fi
