#!/usr/bin/env bash
# Test the worktree guard in .claude/hooks/pre-tool-use.sh (#4464).
# Each case simulates a Claude pre-tool-use invocation from either the actual
# main checkout or a temporary linked worktree and asserts the hook exit code.

set -eu

INVOKING_ROOT="$(git rev-parse --show-toplevel)"
HOOK="$INVOKING_ROOT/.claude/hooks/pre-tool-use.sh"
FAIL=0

run_case() {
  local label="$1" cwd="$2" cmd="$3" expected_exit="$4"
  local actual
  actual=$(
    cd "$cwd" && echo "{\"tool_input\":{\"command\":\"$cmd\"}}" | bash "$HOOK" >/dev/null 2>&1
    echo $?
  )
  if [ "$actual" = "$expected_exit" ]; then
    echo "PASS  $label (exit $actual)"
  else
    echo "FAIL  $label (expected $expected_exit, got $actual) CWD=$cwd CMD=$cmd"
    FAIL=1
  fi
}

# Resolve the real main checkout through the shared Git common directory. This
# remains correct when the test itself starts in a linked worktree, where
# `git rev-parse --show-toplevel` identifies the linked checkout instead.
COMMON_DIR="$(git rev-parse --git-common-dir)"
COMMON_ABS="$(cd "$COMMON_DIR" && pwd -P)"
MAIN="$(dirname "$COMMON_ABS")"
if [ ! -e "$MAIN/.git" ]; then
  echo "FAIL  unable to resolve main checkout from git common dir: $COMMON_ABS"
  exit 1
fi

WORKTREE_TMP="$(mktemp -d)/wt-test-$$"
git -C "$MAIN" worktree add --detach "$WORKTREE_TMP" HEAD >/dev/null 2>&1
trap 'git -C "$MAIN" worktree remove --force "$WORKTREE_TMP" >/dev/null 2>&1 || true' EXIT

# --- Main checkout: all git ops allowed ---
run_case "main checkout: git checkout master"     "$MAIN" "git checkout master"     0
run_case "main checkout: git checkout -b new"     "$MAIN" "git checkout -b new"     0
run_case "main checkout: git switch master"       "$MAIN" "git switch master"       0
run_case "main checkout: git switch -c new"       "$MAIN" "git switch -c new"       0
run_case "main checkout: git worktree add x y"    "$MAIN" "git worktree add x y"    0

# --- Worktree: branch-switching blocked ---
run_case "worktree: git checkout master (block)"        "$WORKTREE_TMP" "git checkout master"        2
run_case "worktree: git switch master (block)"          "$WORKTREE_TMP" "git switch master"          2
run_case "worktree: git worktree add foo (block)"       "$WORKTREE_TMP" "git worktree add foo"       2

# --- Worktree: safe forms allowed ---
run_case "worktree: git checkout -b new (allow)"        "$WORKTREE_TMP" "git checkout -b new"        0
run_case "worktree: git checkout -B new (allow)"        "$WORKTREE_TMP" "git checkout -B new"        0
run_case "worktree: git checkout -- file.rs (allow)"    "$WORKTREE_TMP" "git checkout -- file.rs"    0
run_case "worktree: git checkout --ours path (allow)"   "$WORKTREE_TMP" "git checkout --ours path"   0
run_case "worktree: git switch -c new (allow)"          "$WORKTREE_TMP" "git switch -c new"          0

if [ "$FAIL" -eq 0 ]; then
  echo
  echo "All test cases passed."
  exit 0
else
  echo
  echo "Some test cases failed."
  exit 1
fi
