#!/usr/bin/env bash
set -euo pipefail

# The full merge gate intentionally runs only for pull requests targeting the
# protected branches. This cheap guard makes the skipped state visible for
# stacked pull requests instead of letting incidental checks look green.

event_name="${1:-${GITHUB_EVENT_NAME:-}}"
base_ref="${2:-${GITHUB_BASE_REF:-}}"

case "$event_name" in
  merge_group)
    echo "Merge gate target: merge-group evaluation is eligible."
    ;;
  pull_request)
    if [[ "$base_ref" == "main" || "$base_ref" == "master" ]]; then
      echo "Merge gate target: pull request targets protected branch '$base_ref'."
      exit 0
    fi

    if [[ -z "$base_ref" ]]; then
      echo "::error title=Merge gate target unknown::Pull request base branch was not provided; merge-gate evaluation is NOT_PROVEN." >&2
      exit 1
    fi

    echo "::error title=Merge gate not evaluated::Pull request targets '$base_ref', so the expensive merge gate did not run." >&2
    echo "Merge gate target: '$base_ref' is not main/master; review and merge readiness are NOT_PROVEN." >&2
    exit 1
    ;;
  *)
    echo "::error title=Merge gate target unknown::Unsupported GitHub event '$event_name'; merge-gate evaluation is NOT_PROVEN." >&2
    exit 1
    ;;
esac
