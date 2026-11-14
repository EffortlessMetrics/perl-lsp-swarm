#!/usr/bin/env bash
set -euo pipefail

# Delegate to the existing revision-owned installer/checker. Admission retains
# the pinned-toolchain guard and worktree-private output policy; Cargo selects
# the executable instead of trusting PATH or guessing an output path.
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
case "${1:-}" in
  install-githooks|check-githooks) ;;
  *) echo 'githooks-bootstrap: expected install-githooks or check-githooks' >&2; exit 2 ;;
esac
command="$1"
shift
admission_args=()
seen_preflight=false
seen_budget=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --preflight)
      if $seen_preflight; then
        echo 'githooks-bootstrap: duplicate --preflight' >&2
        exit 2
      fi
      seen_preflight=true
      admission_args+=(--preflight)
      shift
      ;;
    --budget-file)
      if $seen_budget || [[ $# -lt 2 || -z "$2" ]]; then
        echo 'githooks-bootstrap: --budget-file requires one path' >&2
        exit 2
      fi
      seen_budget=true
      admission_args+=(--budget-file "$2")
      shift 2
      ;;
    *) break ;;
  esac
done
cd -- "$REPO_ROOT"
# The script's revision selects this worktree; inherited Git selectors cannot
# redirect its installation/check into another repository. Only the child is
# normalized, leaving the invoking hook's Git environment intact.
unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR
exec "$REPO_ROOT/scripts/cargo-admitted" "${admission_args[@]}" \
  run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- "$command" "$@"
