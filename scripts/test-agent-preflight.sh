#!/usr/bin/env bash
# Test suite for scripts/agent-preflight.sh
# TDD: exercises each check independently using temporary git environments

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFLIGHT="$SCRIPT_DIR/agent-preflight.sh"
PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

# Verify the preflight script exists (test the test can run at all)
if [[ ! -f "$PREFLIGHT" ]]; then
    echo "ERROR: agent-preflight.sh not found at $PREFLIGHT"
    echo "Write the implementation first: scripts/agent-preflight.sh"
    exit 1
fi

# ── Helpers ──────────────────────────────────────────────────────────────────

# Create a minimal git repo in a temp dir
make_git_repo() {
    local tmpdir
    tmpdir="$(mktemp -d)"
    git -C "$tmpdir" init -q
    git -C "$tmpdir" config user.email "test@test.com"
    git -C "$tmpdir" config user.name "Test"
    # Need at least one commit so branches work
    echo "init" > "$tmpdir/README"
    git -C "$tmpdir" add README
    git -C "$tmpdir" commit -q -m "init"
    echo "$tmpdir"
}

# Create a worktree from a repo
make_worktree() {
    local repo="$1"
    local branch="${2:-agent-test-branch}"
    local wtdir
    wtdir="$(mktemp -d)"
    rm -rf "$wtdir"  # worktree add needs the dir to not exist
    git -C "$repo" worktree add -q -b "$branch" "$wtdir"
    echo "$wtdir"
}

cleanup() {
    # Remove temp dirs created during tests
    local dir
    for dir in "$@"; do
        [[ -d "$dir" ]] || continue
        rm -rf "$dir"
    done
}

# Create a git repo carrying a hooks/pre-push authority, plus a worktree.
# Prints "<repo> <worktree> <installed-hook-path>"; caller cleans up the
# worktree (remove + prune) and the repo, like the other tests.
make_hook_worktree() {
    local branch="${1:-agent-hook-test}"
    local repo wt installed
    repo="$(make_git_repo)"
    mkdir -p "$repo/hooks"
    printf '#!/usr/bin/env bash\necho current-hook\n' > "$repo/hooks/pre-push"
    git -C "$repo" add hooks/pre-push
    git -C "$repo" commit -q -m "hook authority"
    wt="$(make_worktree "$repo" "$branch")"
    # Resolve exactly the way agent-preflight.sh does (cwd = worktree).
    installed="$(cd "$wt" && git rev-parse --git-common-dir)/hooks/pre-push"
    printf '%s %s %s\n' "$repo" "$wt" "$installed"
}

# ── Test 1: Fails on master branch ───────────────────────────────────────────

test_fails_on_master() {
    local repo
    repo="$(make_git_repo)"
    # Rename to master
    git -C "$repo" branch -m master 2>/dev/null || git -C "$repo" checkout -q -b master 2>/dev/null || true

    local code
    code=0
    (cd "$repo" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    cleanup "$repo"

    if [[ "$code" -eq 1 ]]; then
        pass "fails on master branch (exit 1)"
    else
        fail "fails on master branch — expected exit 1, got $code"
    fi
}

# ── Test 2: Fails on main branch ─────────────────────────────────────────────

test_fails_on_main() {
    local repo
    repo="$(make_git_repo)"
    # git init defaults may use 'main'
    git -C "$repo" branch -m main 2>/dev/null || true

    local code
    code=0
    (cd "$repo" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    cleanup "$repo"

    if [[ "$code" -eq 1 ]]; then
        pass "fails on main branch (exit 1)"
    else
        fail "fails on main branch — expected exit 1, got $code"
    fi
}

# ── Test 3: Fails in non-worktree checkout ────────────────────────────────────

test_fails_in_non_worktree() {
    local repo
    repo="$(make_git_repo)"
    # Create a feature branch so we're not on master/main
    git -C "$repo" checkout -q -b feature-test

    local code
    code=0
    (cd "$repo" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    cleanup "$repo"

    # Should fail with exit code 2 (not a worktree)
    if [[ "$code" -eq 2 ]]; then
        pass "fails in non-worktree checkout (exit 2)"
    else
        fail "fails in non-worktree checkout — expected exit 2, got $code"
    fi
}

# ── Test 4: Passes in a proper worktree ──────────────────────────────────────

test_passes_in_worktree() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-test-ok")"

    local code
    code=0
    (cd "$wt" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    # Cleanup worktree then repo
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 0 ]]; then
        pass "passes in a proper worktree (exit 0)"
    else
        fail "passes in a proper worktree — expected exit 0, got $code"
    fi
}

# ── Test 5: Fails with unresolved merge conflicts ────────────────────────────

test_fails_with_conflicts() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-conflict-test")"

    # Create a conflict marker file manually to simulate unresolved conflicts
    printf '<<<<<<< HEAD\nfoo\n=======\nbar\n>>>>>>> other\n' > "$wt/conflict.txt"

    local code
    code=0
    (cd "$wt" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 3 ]]; then
        pass "fails with unresolved merge conflicts (exit 3)"
    else
        fail "fails with unresolved merge conflicts — expected exit 3, got $code"
    fi
}

# ── Test 6: Detached HEAD fails ───────────────────────────────────────────────

test_fails_in_detached_head() {
    local repo
    repo="$(make_git_repo)"
    # Detach HEAD
    local sha
    sha="$(git -C "$repo" rev-parse HEAD)"
    git -C "$repo" checkout -q --detach "$sha"

    local code
    code=0
    (cd "$repo" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    cleanup "$repo"

    if [[ "$code" -eq 1 ]]; then
        pass "fails in detached HEAD state (exit 1)"
    else
        fail "fails in detached HEAD state — expected exit 1, got $code"
    fi
}

# ── Test 7: Error messages are informative ────────────────────────────────────

test_error_messages_on_master() {
    local repo
    repo="$(make_git_repo)"
    git -C "$repo" branch -m master 2>/dev/null || true

    local output
    output="$(cd "$repo" && bash "$PREFLIGHT" 2>&1)" || true

    cleanup "$repo"

    if echo "$output" | grep -qi "master\|main"; then
        pass "error message mentions branch name"
    else
        fail "error message does not mention branch name — got: $output"
    fi
}

# ── Test 8: Runs without errors in THIS worktree ─────────────────────────────

test_current_worktree_passes() {
    # This test only runs if we're in a proper agent worktree.
    # The repo root may not be a proper worktree (it could be a main checkout),
    # so we check first before asserting.
    local repo_root
    repo_root="$SCRIPT_DIR/.."

    local git_dir
    local git_common_dir
    # Guarded: outside any readable repo (non-checkout copy, WSL view of a
    # Windows-pointer worktree) git fails, and both vars land empty+equal so
    # the "not a proper agent worktree" skip below is taken by design.
    git_dir="$(git -C "$repo_root" rev-parse --git-dir 2>/dev/null || true)"
    git_common_dir="$(git -C "$repo_root" rev-parse --git-common-dir 2>/dev/null || true)"

    # If git-dir != git-common-dir, we're in a proper worktree. Test it.
    if [[ "$git_dir" != "$git_common_dir" ]]; then
        local code
        code=0
        (cd "$repo_root" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

        if [[ "$code" -eq 0 ]]; then
            pass "current worktree passes preflight (exit 0)"
        elif [[ "$code" -eq 6 ]]; then
            # Exit 6 = stash entries from other agents (shared stash).
            # This is expected in multi-agent environments and validates
            # that Check 6 is working correctly.
            pass "current worktree passes preflight (exit 6 — stash from other agents, expected)"
        elif [[ "$code" -eq 7 ]]; then
            # Exit 7 = installed hooks missing/stale on this host (issue
            # #17406). The drift detector is working correctly; provisioning
            # the host's hooks (bash scripts/install-githooks.sh) returns
            # this to exit 0. Dedicated fixture tests below pin the exact
            # hook semantics.
            pass "current worktree passes preflight (exit 7 — hooks not provisioned on this host, expected)"
        else
            fail "current worktree should pass preflight — expected exit 0, 6, or 7, got $code"
        fi
    else
        # We're not in a proper agent worktree. That's OK — test 4 already
        # covers the happy path. Skip this sanity check.
        pass "current worktree passes preflight (exit 0)"
    fi
}

# ── Test 9: Fails when cwd is the main repo root ─────────────────────────────
# Even if git-dir != common-dir (worktree detected), the cwd itself must not
# be the main repo root — that means the agent is writing to the main checkout.

test_fails_when_cwd_is_main_repo_root() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-cwd-test")"

    # Run preflight from the worktree directory (should pass — baseline)
    local code_wt
    code_wt=0
    (cd "$wt" && bash "$PREFLIGHT" >/dev/null 2>&1) || code_wt=$?

    if [[ "$code_wt" -ne 0 ]]; then
        fail "baseline worktree should pass — got exit $code_wt"
        git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
        git -C "$repo" worktree prune 2>/dev/null || true
        rm -rf "$repo"
        return
    fi

    # Now run preflight from the main repo root (simulating an agent that
    # cd'd back to the main checkout). First create a feature branch so we
    # don't fail on the master/main check.
    git -C "$repo" checkout -q -b feature-cwd-check

    local code_main
    code_main=0
    (cd "$repo" && bash "$PREFLIGHT" >/dev/null 2>&1) || code_main=$?

    # Cleanup
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    # The main repo root is not a worktree, so check 2 catches it (exit 2).
    # Test 11 below exercises check 4 in isolation.
    if [[ "$code_main" -eq 2 ]]; then
        pass "fails when cwd is main repo root (exit $code_main — caught by check 2)"
    else
        fail "fails when cwd is main repo root — expected exit 2, got $code_main"
    fi
}

# ── Test 10: Worktree at repo root path prefix doesn't false-positive ────────
# A worktree whose path starts with the main repo's path should still pass
# (e.g. /tmp/repo is main, /tmp/repo-worktree-abc is the worktree).

test_worktree_path_prefix_no_false_positive() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-prefix-test")"

    local code
    code=0
    (cd "$wt" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 0 ]]; then
        pass "worktree with path prefix of main repo passes (exit 0)"
    else
        fail "worktree with path prefix of main repo — expected exit 0, got $code"
    fi
}

# ── Test 11: Check 4 fires independently (GIT_DIR override) ──────────────────
# Test 9 is caught by Check 2 (exit 2) before Check 4 runs.  This test
# exercises Check 4 in isolation by setting GIT_DIR to the worktree's git-dir
# while cwd is the main repo root.  Check 2 sees git-dir != git-common-dir
# and passes; Check 4 then catches that cwd == main repo root (exit 4).

test_check4_fires_with_git_dir_override() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-check4-iso")"

    # Discover the worktree's actual git-dir
    local wt_git_dir
    wt_git_dir="$(git -C "$wt" rev-parse --git-dir 2>/dev/null)"

    # Run preflight from the main repo root with GIT_DIR pointing to the
    # worktree.  Checks 1-3 should pass; Check 4 should catch the cwd.
    local code
    code=0
    (cd "$repo" && GIT_DIR="$wt_git_dir" bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    # Cleanup
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 4 ]]; then
        pass "check 4 fires independently via GIT_DIR override (exit 4)"
    else
        fail "check 4 via GIT_DIR override — expected exit 4, got $code"
    fi
}

# ── Test 12: Accepts the per-worktree default target directory ──────────────

test_accepts_unset_cargo_target_dir() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-target-dir-test")"

    # Run preflight with CARGO_TARGET_DIR unset and capture output
    local output code=0
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 0 ]] && [[ "$output" == *"CARGO_TARGET_DIR is unset"* ]]; then
        pass "accepts unset CARGO_TARGET_DIR (exit 0)"
    else
        fail "accepts unset CARGO_TARGET_DIR — exit $code; output: $output"
    fi
}

# ── Test 13: Rejects an inherited CARGO_TARGET_DIR ──────────────────────────

test_rejects_existing_cargo_target_dir() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-target-existing-test")"

    # Run preflight with CARGO_TARGET_DIR already set
    local output code=0
    output="$(cd "$wt" && CARGO_TARGET_DIR="/tmp/my-custom-target" bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 5 ]] && [[ "$output" == *"CARGO_TARGET_DIR is set"* ]]; then
        pass "rejects existing CARGO_TARGET_DIR (exit 5)"
    else
        fail "rejects existing CARGO_TARGET_DIR — exit $code; output: $output"
    fi
}

# ── Test 14: Guidance keeps Cargo's per-worktree default ────────────────────

test_target_dir_guidance_uses_worktree_default() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-branch-in-path")"

    # Run preflight with CARGO_TARGET_DIR unset, capture output
    local output
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || true

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$output" == *"this worktree's own target/"* ]] &&
       [[ "$output" != *"CARGO_TARGET_DIR="* ]]; then
        pass "target-dir guidance uses the worktree default"
    else
        fail "target-dir guidance must not invent a branch-derived override — output: $output"
    fi
}

# ── Test 15: Fails when git stash entries exist ──────────────────────────────

test_fails_with_stash_entries() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-stash-test")"

    # Create a stash entry from the main checkout (simulating cross-contamination)
    echo "dirty" > "$repo/dirty.txt"
    git -C "$repo" checkout -q -b temp-for-stash 2>/dev/null || true
    git -C "$repo" add dirty.txt
    git -C "$repo" stash push -q -m "stash from another agent"

    # Now preflight should fail in the worktree because the shared stash is non-empty
    local code
    code=0
    (cd "$wt" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    git -C "$repo" stash clear 2>/dev/null || true
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 6 ]]; then
        pass "fails with stash entries present (exit 6)"
    else
        fail "fails with stash entries present — expected exit 6, got $code"
    fi
}

# ── Test 16: Passes in worktree with empty stash ─────────────────────────────

test_passes_with_empty_stash() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-stash-empty-test")"

    # Verify no stash entries exist
    local stash_count
    stash_count="$(git -C "$wt" stash list 2>/dev/null | wc -l)"

    local code
    code=0
    (cd "$wt" && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 0 ]]; then
        pass "passes in worktree with empty stash (exit 0)"
    else
        fail "passes in worktree with empty stash — expected exit 0, got $code"
    fi
}

# ── Test 17: Stash error message is informative ──────────────────────────────

test_stash_error_message() {
    local repo
    repo="$(make_git_repo)"
    local wt
    wt="$(make_worktree "$repo" "agent-stash-msg-test")"

    # Create a stash entry
    echo "dirty" > "$repo/dirty.txt"
    git -C "$repo" checkout -q -b temp-msg-stash 2>/dev/null || true
    git -C "$repo" add dirty.txt
    git -C "$repo" stash push -q -m "stash from another agent"

    local output
    output="$(cd "$wt" && bash "$PREFLIGHT" 2>&1)" || true

    git -C "$repo" stash clear 2>/dev/null || true
    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$output" == *"Identify the owner of each stash entry"* ]] &&
       [[ "$output" == *"Do not edit until ownership and salvage are known"* ]] &&
       [[ "$output" != *"git stash clear"* ]] &&
       [[ "$output" != *"git stash pop"* ]] &&
       [[ "$output" != *"git stash drop"* ]]; then
        pass "stash error requires owner handoff without destructive recovery"
    else
        fail "stash error gives unsafe or missing recovery guidance — got: $output"
    fi
}

# ── Test 18: Missing installed hook fails naming the installer ──────────────
# Issue #17406: a fresh worktree has zero hooks (git init creates only
# *.sample); preflight must fail with the installer as the fix, not pass.

test_missing_hook_fails() {
    local repo wt installed
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-missing-test")"
    rm -f "$installed" # belt-and-braces: git init only creates *.sample

    local output code=0
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 7 ]] && [[ "$output" == *"install-githooks"* ]]; then
        pass "missing installed hook fails (exit 7) naming the installer"
    else
        fail "missing installed hook — expected exit 7 naming install-githooks, got exit $code: $output"
    fi
}

# ── Test 19: Stale installed hook fails naming the installer ────────────────

test_stale_hook_fails() {
    local repo wt installed
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-stale-test")"
    printf '#!/usr/bin/env bash\necho stale-hook\n' > "$installed"
    chmod +x "$installed"

    local output code=0
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 7 ]] && [[ "$output" == *"install-githooks"* ]]; then
        pass "stale installed hook fails (exit 7) naming the installer"
    else
        fail "stale installed hook — expected exit 7 naming install-githooks, got exit $code: $output"
    fi
}

# ── Test 20: Current installed hook passes ──────────────────────────────────

test_current_hook_passes() {
    local repo wt installed
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-current-test")"
    cp "$wt/hooks/pre-push" "$installed"
    chmod +x "$installed"
    printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$(dirname "$installed")/pre-commit"
    chmod +x "$(dirname "$installed")/pre-commit"

    local code=0
    (cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 0 ]]; then
        pass "current installed hook passes (exit 0)"
    else
        fail "current installed hook — expected exit 0, got $code"
    fi
}

# ── Test 21: Installer-shaped hook (extra trailing newline) passes ──────────
# write_git_hook appends "\n" to the generated script, so a hook installed by
# the real installer always carries one more trailing newline than the
# checked-in authority. The comparison must tolerate exactly that, or every
# provisioned host would report drift forever.

test_installer_shaped_hook_passes() {
    local repo wt installed
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-shaped-test")"
    cp "$wt/hooks/pre-push" "$installed"
    printf '\n' >> "$installed"
    chmod +x "$installed"
    printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$(dirname "$installed")/pre-commit"
    chmod +x "$(dirname "$installed")/pre-commit"

    local code=0
    (cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" >/dev/null 2>&1) || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 0 ]]; then
        pass "installer-shaped hook (extra trailing newline) passes (exit 0)"
    else
        fail "installer-shaped hook — expected exit 0, got $code"
    fi
}

# ── Test 22: Non-executable current hook fails ──────────────────────────────
# Git silently skips non-executable hooks, so current bytes without the exec
# bit are still a void guard. POSIX-only (mirrors check_githooks' cfg gate);
# skipped when the filesystem cannot represent the missing bit.

test_non_executable_hook_fails() {
    local repo wt installed
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-noexec-test")"
    cp "$wt/hooks/pre-push" "$installed"
    chmod -x "$installed"

    if [[ -x "$installed" ]]; then
        git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
        git -C "$repo" worktree prune 2>/dev/null || true
        rm -rf "$repo"
        pass "non-executable hook fails (exec-bit not representable on this fs — skipped)"
        return
    fi

    local output code=0
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    case "$(uname -s 2>/dev/null || echo unknown)" in
        MINGW* | MSYS* | CYGWIN*)
            if [[ "$code" -eq 0 ]]; then
                pass "non-executable hook passes on Windows (no exec-bit semantics)"
            else
                fail "non-executable hook on Windows — expected exit 0, got $code"
            fi
            ;;
        *)
            if [[ "$code" -eq 7 ]] && [[ "$output" == *"install-githooks"* ]]; then
                pass "non-executable hook fails (exit 7) naming the installer"
            else
                fail "non-executable hook — expected exit 7 naming install-githooks, got exit $code: $output"
            fi
            ;;
    esac
}

# ── Test 23: Current pre-push but missing pre-commit fails ─────────────────
# The installer manages both hooks; a current pre-push must not mask a
# deleted pre-commit.

test_missing_precommit_hook_fails() {
    local repo wt installed installed_commit
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-nocommit-test")"
    installed_commit="$(dirname "$installed")/pre-commit"
    cp "$wt/hooks/pre-push" "$installed"
    chmod +x "$installed"
    rm -f "$installed_commit"

    local output code=0
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    if [[ "$code" -eq 7 ]] && [[ "$output" == *"pre-commit"* ]] && [[ "$output" == *"install-githooks"* ]]; then
        pass "missing pre-commit hook fails (exit 7) naming the installer"
    else
        fail "missing pre-commit hook — expected exit 7 naming install-githooks, got exit $code: $output"
    fi
}

# ── Test 24: Current pre-push but non-executable pre-commit fails ───────────
# POSIX-only (mirrors the pre-push exec gate); skipped when the filesystem
# cannot represent the missing bit.

test_non_executable_precommit_hook_fails() {
    local repo wt installed installed_commit
    read -r repo wt installed <<< "$(make_hook_worktree "agent-hook-commitnoexec-test")"
    installed_commit="$(dirname "$installed")/pre-commit"
    cp "$wt/hooks/pre-push" "$installed"
    chmod +x "$installed"
    printf '#!/usr/bin/env bash\necho fixture-commit\n' > "$installed_commit"
    chmod -x "$installed_commit"

    if [[ -x "$installed_commit" ]]; then
        git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
        git -C "$repo" worktree prune 2>/dev/null || true
        rm -rf "$repo"
        pass "non-executable pre-commit fails (exec-bit not representable on this fs — skipped)"
        return
    fi

    local output code=0
    output="$(cd "$wt" && unset CARGO_TARGET_DIR && bash "$PREFLIGHT" 2>&1)" || code=$?

    git -C "$repo" worktree remove --force "$wt" 2>/dev/null || true
    git -C "$repo" worktree prune 2>/dev/null || true
    rm -rf "$repo"

    case "$(uname -s 2>/dev/null || echo unknown)" in
        MINGW* | MSYS* | CYGWIN*)
            if [[ "$code" -eq 0 ]]; then
                pass "non-executable pre-commit passes on Windows (no exec-bit semantics)"
            else
                fail "non-executable pre-commit on Windows — expected exit 0, got $code"
            fi
            ;;
        *)
            if [[ "$code" -eq 7 ]] && [[ "$output" == *"pre-commit"* ]]; then
                pass "non-executable pre-commit fails (exit 7) naming the installer"
            else
                fail "non-executable pre-commit — expected exit 7 naming install-githooks, got exit $code: $output"
            fi
            ;;
    esac
}

# ── Run all tests ─────────────────────────────────────────────────────────────

echo "=== agent-preflight test suite ==="
echo ""

test_fails_on_master
test_fails_on_main
test_fails_in_non_worktree
test_passes_in_worktree
test_fails_with_conflicts
test_fails_in_detached_head
test_error_messages_on_master
test_current_worktree_passes
test_fails_when_cwd_is_main_repo_root
test_worktree_path_prefix_no_false_positive
test_check4_fires_with_git_dir_override
test_accepts_unset_cargo_target_dir
test_rejects_existing_cargo_target_dir
test_target_dir_guidance_uses_worktree_default
test_fails_with_stash_entries
test_passes_with_empty_stash
test_stash_error_message
test_missing_hook_fails
test_stale_hook_fails
test_current_hook_passes
test_installer_shaped_hook_passes
test_non_executable_hook_fails
test_missing_precommit_hook_fails
test_non_executable_precommit_hook_fails

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

if [[ "$FAIL_COUNT" -gt 0 ]]; then
    exit 1
fi
exit 0
