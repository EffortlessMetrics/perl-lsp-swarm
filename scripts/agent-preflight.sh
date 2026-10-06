#!/usr/bin/env bash
# Agent preflight safety checks
# Run before any edit-capable agent starts work.
#
# Exit codes:
#   0 — all checks pass
#   1 — branch issue (on master/main or detached HEAD)
#   2 — worktree issue (not running in an isolated git worktree)
#   3 — conflict issue (unresolved merge conflicts present)
#   4 — cwd issue (running from the main repo root, not a worktree path)
#   5 — CARGO_TARGET_DIR is set (defeats automatic per-worktree isolation)
#   6 — stash issue (shared stash has entries — cross-contamination risk)
#   7 — hooks issue (installed pre-push hook missing, stale, or not executable)
#
# Usage:
#   bash scripts/agent-preflight.sh
#
# Check 5 ENFORCES target-dir isolation (issue #3854). Cargo's default
# (unconfigured) target-dir already resolves per-worktree — agents must NOT
# export CARGO_TARGET_DIR. This check FAILS (non-zero exit) when the
# variable is set: a stale shell-profile export from a prior session or a
# different worktree/branch silently overrides the correct per-worktree
# default for every subsequently-sourced shell, redirecting builds to the
# wrong worktree (the "stale-binary trap"). There is no legitimate reason
# for an agent to set it under the current convention — unset it and rely
# on the default.

set -uo pipefail

PASS=0
FAIL=0

ok()  { printf 'OK  %s\n' "$1"; PASS=$((PASS + 1)); }
err() { printf 'ERR %s\n' "$1"; FAIL=$((FAIL + 1)); }

echo "=== Agent Preflight Checks ==="
echo ""

# ── Check 1: Not on master or main ───────────────────────────────────────────

CURRENT_BRANCH="$(git branch --show-current 2>/dev/null)"

if [[ -z "$CURRENT_BRANCH" ]]; then
    err "Detached HEAD state. Agents must work on a named branch."
    echo "    Fix: git checkout -b agent-<id> or use a worktree with a branch"
    BRANCH_OK=false
elif [[ "$CURRENT_BRANCH" == "master" || "$CURRENT_BRANCH" == "main" ]]; then
    err "On protected branch '$CURRENT_BRANCH'. Agents must not edit master/main directly."
    echo "    Fix: Work in an isolated worktree with its own branch (isolation: worktree)"
    BRANCH_OK=false
else
    ok "Branch: $CURRENT_BRANCH (not master/main)"
    BRANCH_OK=true
fi

# ── Check 2: Running inside a git worktree (not the main checkout) ────────────

GIT_DIR="$(git rev-parse --git-dir 2>/dev/null)"
GIT_COMMON_DIR="$(git rev-parse --git-common-dir 2>/dev/null)"

if [[ "$GIT_DIR" == "$GIT_COMMON_DIR" ]]; then
    # git-dir equals common-dir → this IS the main checkout, not a worktree
    err "Not in an isolated git worktree. Agents require worktree isolation."
    echo "    Fix: Spawn agent with isolation: worktree in the agent definition"
    echo "    The main checkout is: $GIT_COMMON_DIR"
    WORKTREE_OK=false
else
    ok "Worktree: isolated (git-dir=$GIT_DIR)"
    WORKTREE_OK=true
fi

# ── Check 3: No unresolved merge conflicts ────────────────────────────────────

# Search for conflict markers, skipping the .git directory
CONFLICT_FILES="$(grep -rl --exclude-dir='.git' '^<<<<<<< ' . 2>/dev/null || true)"

if [[ -n "$CONFLICT_FILES" ]]; then
    err "Unresolved merge conflict markers found:"
    while IFS= read -r f; do
        echo "    $f"
    done <<< "$CONFLICT_FILES"
    echo "    Fix: Resolve conflicts, then re-run preflight"
    CONFLICT_OK=false
else
    ok "No unresolved merge conflicts"
    CONFLICT_OK=true
fi

# ── Check 4: cwd must not be the main repo root ─────────────────────────────
# An agent in a worktree can still accidentally cd to (or be spawned in) the
# main checkout path.  The Write/Edit tools resolve absolute paths relative to
# cwd, so writing from the main checkout puts files in the wrong place.

MAIN_REPO_RAW="$(git rev-parse --git-common-dir 2>/dev/null | sed 's|/\.git$||; s|^\.git$|.|')"
# Resolve both paths through readlink/pwd -P so symlinks don't cause mismatches
if [[ -n "$MAIN_REPO_RAW" ]]; then
    MAIN_REPO="$(cd "$MAIN_REPO_RAW" 2>/dev/null && pwd -P)" || MAIN_REPO=""
else
    MAIN_REPO=""
fi
CWD="$(pwd -P)"

if [[ -n "$MAIN_REPO" && "$CWD" = "$MAIN_REPO" ]]; then
    err "cwd is the main repo root ($MAIN_REPO). Agents must run from their worktree."
    echo "    Fix: cd \$(git worktree list | grep \$(git branch --show-current) | awk '{print \$1}')"
    CWD_OK=false
else
    ok "cwd is not the main repo root"
    CWD_OK=true
fi

# ── Check 5: CARGO_TARGET_DIR isolation ─────────────────────────────────────
# Cargo's default (unconfigured) target-dir resolves to
# <workspace-root>/target, which for a git-worktree checkout is
# <this-worktree>/target — already isolated, automatically, with no setup
# step (issue #3854). A stray CARGO_TARGET_DIR in the environment (usually a
# stale `export` left in a shell profile from a prior session or a different
# worktree/branch) silently overrides that per-worktree default for every
# subsequently-sourced shell — the "stale-binary trap." This check FAILS if
# it's set: unsetting it (not documenting around it) is the enforcement
# point.

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    err "CARGO_TARGET_DIR is set (\"$CARGO_TARGET_DIR\") — unset it; cargo's default is already per-worktree isolated and a stale export redirects builds to the wrong worktree (see WORKTREE_PROTOCOL.md / #3854)."
    echo "    Fix: unset CARGO_TARGET_DIR"
    echo "    If this came from a shell profile (~/.bashrc, ~/.zshrc), remove that"
    echo "    line — it is a leftover from another worktree/branch/session, not a"
    echo "    legitimate setting under the current convention."
    TARGET_DIR_OK=false
else
    ok "CARGO_TARGET_DIR is unset — cargo will use this worktree's own target/ (isolation is automatic)"
    TARGET_DIR_OK=true
fi

# ── Check 6: No git stash entries (shared across worktrees) ──────────────────

STASH_COUNT="$(git stash list 2>/dev/null | wc -l)"

if [[ "$STASH_COUNT" -gt 0 ]]; then
    err "Git stash has $STASH_COUNT entries. Stash is SHARED across all worktrees — cross-contamination risk."
    echo "    The stash list is a single global list. Do not pop, drop, or clear another worktree's work."
    echo "    Fix: Identify the owner of each stash entry and have that owner preserve or resolve"
    echo "    their work. Do not edit until ownership and salvage are known; then re-run preflight."
    STASH_OK=false
else
    ok "No git stash entries (stash is shared — safe)"
    STASH_OK=true
fi

# ── Check 7: pre-push hook is provisioned and current (issue #17406) ──────────
# Failing check — a missing or stale hook voids every hook-assumed guard
# (placeholder-identity refusal, hook currency, the pre-push gate itself),
# so an agent must not reason "the hooks will catch it" until this passes.
# Local hooks cannot be strictly enforced (fresh clones, --no-verify
# bypass); this check is the drift detector at agent entry, and
# scripts/worktree-add.sh + worktree-manager allocate are the provisioning
# paths that keep it green by default.

REPO_ROOT_AGENT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
INSTALLED_HOOK="$(git rev-parse --git-common-dir 2>/dev/null)/hooks/pre-push"
INSTALLED_COMMIT_HOOK="$(git rev-parse --git-common-dir 2>/dev/null)/hooks/pre-commit"
CHECKED_IN_HOOK="$REPO_ROOT_AGENT/hooks/pre-push"
HOOK_INSTALLER_FIX="bash scripts/install-githooks.sh"
if [[ ! -f "$CHECKED_IN_HOOK" ]]; then
    # No authority in this checkout (e.g. a bare fixture repo, not perl-lsp):
    # nothing to verify against — skip without failing.
    ok "pre-push hook check skipped (no hooks/pre-push authority here)"
    HOOKS_OK=true
elif [[ ! -f "$INSTALLED_HOOK" ]]; then
    err "pre-push hook is missing ($INSTALLED_HOOK). Hook-assumed guards are void until it is installed."
    echo "    Fix: $HOOK_INSTALLER_FIX (run from the repo root)"
    HOOKS_OK=false
elif [[ "$(tr -d '\r' < "$INSTALLED_HOOK")" != "$(tr -d '\r' < "$CHECKED_IN_HOOK")" ]]; then
    # Command-substitution comparison strips trailing newlines, mirroring
    # check_githooks' normalization: the installer appends one "\n" to the
    # generated script, so a byte-exact diff would always report drift.
    err "pre-push hook is stale (installed copy differs from hooks/pre-push; Windows os error 206 risk)."
    echo "    Fix: $HOOK_INSTALLER_FIX (run from the repo root)"
    HOOKS_OK=false
else
    case "$(uname -s 2>/dev/null || echo unknown)" in
        MINGW* | MSYS* | CYGWIN*)
            HOOK_EXEC_OK=true
            ;; # Windows has no exec-bit semantics (mirrors check_githooks' cfg gate)
        *)
            if [[ -x "$INSTALLED_HOOK" ]]; then
                HOOK_EXEC_OK=true
            else
                HOOK_EXEC_OK=false
            fi
            ;;
    esac
    if [[ "$HOOK_EXEC_OK" == true ]]; then
        # pre-commit has no checked-in authority (its bytes are generated
        # inside the installer), so it gets presence + executable instead of
        # a byte comparison. The installer writes both hooks in one
        # invocation, so a current pre-push implies a current pre-commit
        # unless the latter was deleted or de-executed outright.
        COMMIT_OK=true
        COMMIT_PROBLEM=""
        if [[ ! -f "$INSTALLED_COMMIT_HOOK" ]]; then
            COMMIT_OK=false
            COMMIT_PROBLEM="missing ($INSTALLED_COMMIT_HOOK). Commits run without the staged gate."
        else
            case "$(uname -s 2>/dev/null || echo unknown)" in
                MINGW* | MSYS* | CYGWIN*) ;;
                *)
                    if [[ ! -x "$INSTALLED_COMMIT_HOOK" ]]; then
                        COMMIT_OK=false
                        COMMIT_PROBLEM="not executable ($INSTALLED_COMMIT_HOOK). Git silently skips non-executable hooks."
                    fi
                    ;;
            esac
        fi
        if [[ "$COMMIT_OK" == true ]]; then
            ok "pre-push hook is current"
            HOOKS_OK=true
        else
            err "pre-commit hook is $COMMIT_PROBLEM"
            echo "    Fix: $HOOK_INSTALLER_FIX (run from the repo root)"
            HOOKS_OK=false
        fi
    else
        err "pre-push hook is not executable ($INSTALLED_HOOK). Git silently skips non-executable hooks."
        echo "    Fix: $HOOK_INSTALLER_FIX (run from the repo root)"
        HOOKS_OK=false
    fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "=== $PASS passed, $FAIL failed ==="

if [[ "$BRANCH_OK" == false ]]; then
    exit 1
fi

if [[ "$WORKTREE_OK" == false ]]; then
    exit 2
fi

if [[ "$CONFLICT_OK" == false ]]; then
    exit 3
fi

if [[ "$CWD_OK" == false ]]; then
    exit 4
fi

if [[ "$TARGET_DIR_OK" == false ]]; then
    exit 5
fi

if [[ "$STASH_OK" == false ]]; then
    exit 6
fi

if [[ "$HOOKS_OK" == false ]]; then
    exit 7
fi

echo ""
echo "Preflight passed. Safe to begin work."
exit 0
