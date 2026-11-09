#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export WORKTREE_MANAGER_MODULE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
python3 "${SCRIPT_DIR}/test_cargo_retirement.py"
exec bash "${SCRIPT_DIR}/test-worktree-manager-base.sh" "$@"
