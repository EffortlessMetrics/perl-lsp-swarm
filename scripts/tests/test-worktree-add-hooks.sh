#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export WORKTREE_MANAGER_MODULE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
exec bash "${SCRIPT_DIR}/test-worktree-add-hooks-base.sh" "$@"
