#!/usr/bin/env bash
set -euo pipefail

# Run the revision-owned hygiene implementation in this worktree's admitted
# private output paths. Cargo selects its executable; never use a PATH binary
# or guess target/debug. The admission wrapper owns toolchain/storage refusal.
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
case "${1:-}" in
  install-githooks|check-githooks|change-set|resolve-package-name) ;;
  *) echo 'githooks-bootstrap: expected a hook bootstrap or resolver command' >&2; exit 2 ;;
esac
command="$1"
shift
admission_args=()
if [[ "${1:-}" == --budget-file ]]; then
  if [[ $# -lt 2 || -z "$2" ]]; then
    echo 'githooks-bootstrap: --budget-file requires a path' >&2
    exit 2
  fi
  admission_args=(--budget-file "$2")
  shift 2
fi
cd -- "$REPO_ROOT"
# Script identity selects this worktree. Clear selectors only in this child;
# the invoking hook keeps its Git environment, and admission sees no redirect.
unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR
exec "$REPO_ROOT/scripts/cargo-admitted" "${admission_args[@]}" run --locked -p perl-ci-hygiene -- "$command" "$@"
