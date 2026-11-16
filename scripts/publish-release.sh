#!/usr/bin/env bash
set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

usage() {
  cat <<'USAGE'
Legacy dispatch-only helper: this does not supply sealed-handoff eligibility inputs
or verify publication/completion. Follow #13768 / #6055 and docs/release/RUNBOOK.md.
Exact workflow-run correlation is tracked by #16799.

Usage:
  scripts/publish-release.sh <0.x.y> [--dry-run] [--ref <git-ref>]

Examples:
  scripts/publish-release.sh 0.11.0
  scripts/publish-release.sh 0.11.0 --dry-run
  scripts/publish-release.sh 0.11.0 --ref master
USAGE
}

if [[ $# -eq 0 ]]; then
  usage
  exit 1
fi

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

exec cargo xtask publish-release "$@"
