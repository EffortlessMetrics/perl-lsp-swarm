#!/usr/bin/env bash
set -euo pipefail

# The full merge gate intentionally runs only for pull requests targeting the
# protected branches. This cheap guard makes the skipped state visible for
# stacked pull requests instead of letting incidental checks look green.

event_name="${1:-${GITHUB_EVENT_NAME:-}}"
base_ref="${2:-${GITHUB_BASE_REF:-}}"
ref_name="${3:-${GITHUB_REF:-}}"

case "$event_name" in
  merge_group)
    # A merge-group event is eligible only when the queue belongs to a
    # protected branch; accept an explicit main/master merge-queue ref and
    # treat anything else (including a missing ref) as NOT_PROVEN.
    if [[ "$ref_name" == refs/heads/main-merge-queue/* || "$ref_name" == refs/heads/master-merge-queue/* ]]; then
      echo "Merge gate target: merge-group evaluation is eligible for protected queue '$ref_name'."
    else
      echo "::error title=Merge gate target unknown::Merge-group ref '$ref_name' is not a protected-branch merge queue; merge-gate evaluation is NOT_PROVEN." >&2
      exit 1
    fi
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
