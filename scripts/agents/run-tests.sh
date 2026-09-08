#!/usr/bin/env bash
# scripts/agents/run-tests.sh
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=guard.sh
source "${SCRIPT_DIR}/guard.sh"
. "${SCRIPT_DIR}/../lib/cargo-toolchain-guard.sh"
cargo_toolchain_guard

lock_acquire
require_lane

run_or_plan "git-status" git status -sb
run_or_plan "cargo-check" cargo check --workspace

# Print chosen features and command
CMD=$(nextest_cmd)
echo "→ PLAN: nextest (built)"
echo "   ${CMD}"

# Execute nextest only if gated
if [[ -n "${AGENT_EXECUTE}" ]]; then
  # shellcheck disable=SC2086
  timeout "${AGENT_TIMEOUT_SEC}" bash -lc "${CMD}" 2>&1 | tee -a "${AGENT_LOG_DIR}/nextest.log"
fi