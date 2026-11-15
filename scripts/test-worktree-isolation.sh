#!/usr/bin/env bash
# Hermetic regression for issue #2429: tracked `.claude/` content must stay
# isolated when agents use a linked worktree outside the coordination checkout.
set -euo pipefail
PASS_COUNT=0
FAIL_COUNT=0
CLEANUP_DIRS=()

trap 'for dir in "${CLEANUP_DIRS[@]:-}"; do [[ -d "$dir" ]] && rm -rf "$dir"; done' EXIT

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

make_git_repo() {
    local tmpdir
    tmpdir="$(mktemp -d)"
    git -C "$tmpdir" init -q
    git -C "$tmpdir" config user.email "test@test.com"
    git -C "$tmpdir" config user.name "Test"
    echo "init" >"$tmpdir/README"
    git -C "$tmpdir" add README
    git -C "$tmpdir" commit -q -m "init"
    mkdir -p "$tmpdir/.claude/commands"
    echo "# original" >"$tmpdir/.claude/commands/test.md"
    git -C "$tmpdir" add .claude/
    git -C "$tmpdir" commit -q -m "add .claude"
    CLEANUP_DIRS+=("$tmpdir")
    echo "$tmpdir"
}

make_worktree() {
    local repo="$1"
    local branch="${2:-isolation-test}"
    local wtdir
    wtdir="$(mktemp -d)"
    rm -rf "$wtdir"
    git -C "$repo" worktree add -q -b "$branch" "$wtdir"
    CLEANUP_DIRS+=("$wtdir")
    echo "$wtdir"
}

# Test: .claude directory has separate inodes
test_claude_isolation() {
    local repo wt main_inode wt_inode
    repo="$(make_git_repo)"
    wt="$(make_worktree "$repo" "test-iso")"
    main_inode="$(ls -i "$repo/.claude/commands" 2>/dev/null | awk '{print $1}' || echo "0")"
    wt_inode="$(ls -i "$wt/.claude/commands" 2>/dev/null | awk '{print $1}' || echo "0")"
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    if [[ "$main_inode" != "0" && "$wt_inode" != "0" && "$main_inode" != "$wt_inode" ]]; then
        pass ".claude is isolated"
    else
        fail ".claude should be isolated — main=$main_inode, wt=$wt_inode"
    fi
}

# Test: edits don't leak to main checkout
test_no_leak() {
    local repo wt main_content
    repo="$(make_git_repo)"
    wt="$(make_worktree "$repo" "test-leak")"
    echo "# modified" >"$wt/.claude/commands/test.md"
    git -C "$wt" add .claude/commands/test.md
    git -C "$wt" commit -q -m "test: modify"
    main_content="$(cat "$repo/.claude/commands/test.md")"
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    if echo "$main_content" | grep -q "# original"; then
        pass "edits don't leak to main"
    else
        fail "edits leaked — got: $main_content"
    fi
}

echo "=== worktree-isolation test suite ==="
test_claude_isolation
test_no_leak
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="
[[ "$FAIL_COUNT" -eq 0 ]]
