#!/usr/bin/env bash
# Retained Claude safety-hook behavior tests — plain bash, no bats required.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOKS_DIR="$REPO_ROOT/.claude/hooks"
PRE_TOOL_USE="$HOOKS_DIR/pre-tool-use.sh"
WORKTREE_TEST="$HOOKS_DIR/tests/test_pre_tool_use_worktree.sh"

PASS=0
FAIL=0

assert_exit() {
  local expected="$1"; shift
  local desc="$1"; shift
  local actual=0
  "$@" || actual=$?
  if [[ "$actual" -eq "$expected" ]]; then
    echo "  PASS: $desc (exit $actual)"
    PASS=$(( PASS + 1 ))
  else
    echo "  FAIL: $desc — expected exit $expected, got $actual" >&2
    FAIL=$(( FAIL + 1 ))
  fi
}

assert_exists() {
  local path="$1" desc="$2"
  if [[ -f "$path" ]]; then
    echo "  PASS: $desc"
    PASS=$(( PASS + 1 ))
  else
    echo "  FAIL: $desc — missing: $path" >&2
    FAIL=$(( FAIL + 1 ))
  fi
}

assert_executable() {
  local path="$1" desc="$2"
  if [[ -f "$path" && -x "$path" ]]; then
    echo "  PASS: $desc"
    PASS=$(( PASS + 1 ))
  else
    echo "  FAIL: $desc — missing or not executable: $path" >&2
    FAIL=$(( FAIL + 1 ))
  fi
}

payload() {
  printf '{"tool_input":{"command":"%s"}}' "$1"
}

run_hook() {
  local command="$1"
  payload "$command" | bash "$PRE_TOOL_USE"
}

echo
echo "=== Registered safety-hook files ==="
assert_executable "$PRE_TOOL_USE" "pre-tool-use.sh exists and is executable"
# The fixture is deliberately invoked through bash and therefore needs only to
# be a tracked regular file, not an independently executable hook surface.
assert_exists "$WORKTREE_TEST" "linked-worktree guard test exists"

echo
echo "=== Destructive-command safety ==="
assert_exit 0 "allows git status" run_hook "git status"
assert_exit 0 "allows empty command" run_hook ""
assert_exit 0 "allows bounded subpath cleanup" run_hook "rm -rf /tmp/perl-lsp-hook-test"
assert_exit 2 "blocks force push" run_hook "git push --force"
assert_exit 2 "blocks hard reset" run_hook "git reset --hard"
assert_exit 2 "blocks cargo publish" run_hook "cargo publish"
assert_exit 2 "blocks destructive git clean" run_hook "git clean -fd"
assert_exit 2 "blocks force refspec" run_hook "git push origin +HEAD:main"
assert_exit 2 "blocks shared worktree stash" run_hook "git stash"
assert_exit 2 "blocks whole shared-temp deletion" run_hook "rm -rf /tmp"

echo
echo "=== Linked-worktree safety ==="
assert_exit 0 "linked-worktree branch-mutation guard" bash "$WORKTREE_TEST"

echo
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
