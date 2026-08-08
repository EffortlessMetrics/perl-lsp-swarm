#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK_SCRIPT="$SCRIPT_DIR/../check-merge-gate-target.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_pass() {
  local description="$1"
  shift
  "$@" >/dev/null || fail "$description should pass"
}

assert_blocked() {
  local description="$1"
  local expected="$2"
  shift 2
  local output
  if output=$("$@" 2>&1); then
    fail "$description should block"
  fi
  [[ "$output" == *"$expected"* ]] || fail "$description omitted '$expected': $output"
}

assert_pass "main target" "$CHECK_SCRIPT" pull_request main
assert_pass "master target" "$CHECK_SCRIPT" pull_request master
assert_pass "merge-group target" "$CHECK_SCRIPT" merge_group
assert_blocked "stacked target" "Merge gate not evaluated" "$CHECK_SCRIPT" pull_request feature/base
assert_blocked "missing target" "NOT_PROVEN" "$CHECK_SCRIPT" pull_request ""
assert_blocked "unknown event" "Unsupported GitHub event" "$CHECK_SCRIPT" workflow_dispatch

echo "PASS: merge-gate target guard distinguishes protected, stacked, missing, and unsupported inputs"
