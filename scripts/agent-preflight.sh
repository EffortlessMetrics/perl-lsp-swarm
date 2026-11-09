#!/usr/bin/env bash
# Agent preflight safety checks
# Select source authoring or the full local-Git workstation checks.
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
#  64 — unsupported arguments
#
# Usage:
#   bash scripts/agent-preflight.sh --source  # edits and compiler-free proof
#   bash scripts/agent-preflight.sh           # existing local-Git workstation checks
#
# Source authoring needs an owned candidate checkout, not a Cargo build or
# installed Git hooks. --source also accepts an isolated standalone checkout;
# it does not grant build, commit, push, publication, or merge authority.
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

SOURCE_ONLY=false
case "$#:${1:-}" in
    0:) ;;
    1:--source) SOURCE_ONLY=true ;;
    1:--help|1:-h)
        echo "Usage: bash scripts/agent-preflight.sh [--source]"
        echo "--source: source authoring only; Cargo, hooks and publication are not assessed."
        exit 0
        ;;
    *)
        echo "Usage: bash scripts/agent-preflight.sh [--source]" >&2
        exit 64
        ;;
esac

PASS=0
FAIL=0

ok()  { printf 'OK  %s\n' "$1"; PASS=$((PASS + 1)); }
err() { printf 'ERR %s\n' "$1"; FAIL=$((FAIL + 1)); }

echo "=== Agent Preflight Checks ==="
echo ""

if [[ "$SOURCE_ONLY" == true ]]; then
    # A task-local source check must inspect the actual cwd, not Git state
    # redirected to another checkout by inherited environment settings.
    if [[ ${GIT_DIR+x} || ${GIT_WORK_TREE+x} || ${GIT_COMMON_DIR+x} || ${GIT_INDEX_FILE+x} ]]; then
        err "Unset GIT_DIR, GIT_WORK_TREE, GIT_COMMON_DIR and GIT_INDEX_FILE; inspect the intended checkout and its actual index."
        exit 2
    fi
    if [[ "$(git rev-parse --is-inside-work-tree 2>/dev/null)" != true ]]; then
        err "Source authoring requires a non-bare Git checkout."
        exit 2
    fi
    # Normalize nested/symlinked invocations and inspect conflicts in the whole
    # current checkout. Do not infer a primary path by trimming '/.git'.
    SOURCE_ROOT="$(git rev-parse --show-toplevel)" || exit 2
    cd "$SOURCE_ROOT" || exit 2
fi

# ── Check 1: Not on master or main ───────────────────────────────────────────

CURRENT_BRANCH="$(git branch --show-current 2>/dev/null)"

if [[ -z "$CURRENT_BRANCH" ]]; then
    err "Detached HEAD state. Agents must work on a named branch."
    echo "    Fix: git checkout -b agent-<id> or use a worktree with a branch"
    BRANCH_OK=false
elif [[ "$CURRENT_BRANCH" == "master" || "$CURRENT_BRANCH" == "main" ]]; then
    err "On protected branch '$CURRENT_BRANCH'. Agents must not edit master/main directly."
    if [[ "$SOURCE_ONLY" == true ]]; then
        echo "    Fix: Create or use the candidate branch in the owned checkout; keep any shared coordination checkout separate."
    else
        echo "    Fix: Work in an isolated worktree with its own branch (isolation: worktree)"
    fi
    BRANCH_OK=false
else
    ok "Branch: $CURRENT_BRANCH (not master/main)"
    BRANCH_OK=true
fi

# ── Check 2: Running inside a git worktree (not the main checkout) ────────────

GIT_DIR="$(git rev-parse --git-dir 2>/dev/null)"
GIT_COMMON_DIR="$(git rev-parse --git-common-dir 2>/dev/null)"

if [[ "$SOURCE_ONLY" == true && "$GIT_DIR" == "$GIT_COMMON_DIR" ]]; then
    # A standalone cloud/task clone has one registered tree. The primary of
    # a shared multi-worktree repository remains a coordination checkout.
    # Inspect Git registration only; do not scan other checkouts or claim
    # that registration proves semantic ownership or writer liveness.
    REGISTERED_TREES="$(git worktree list --porcelain)" || exit 2
    TREE_COUNT=0
    while IFS= read -r entry; do
        [[ "$entry" == worktree\ * ]] && TREE_COUNT=$((TREE_COUNT + 1))
    done <<< "$REGISTERED_TREES"
    if [[ "$TREE_COUNT" == 1 ]]; then
        ok "Source checkout: standalone candidate (one registered tree)"
        WORKTREE_OK=true
    else
        err "Primary checkout has $TREE_COUNT registered trees; use the owned candidate checkout, not the shared coordination checkout."
        WORKTREE_OK=false
    fi
elif [[ "$GIT_DIR" == "$GIT_COMMON_DIR" ]]; then
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
UNMERGED_FILES=""
if [[ "$SOURCE_ONLY" == true ]]; then
    UNMERGED_FILES="$(git diff --name-only --diff-filter=U)" || exit 3
fi

if [[ -n "$UNMERGED_FILES" ]]; then
    err "Unmerged index entries remain:"
    printf '    %s\n' "$UNMERGED_FILES"
    CONFLICT_OK=false
elif [[ -n "$CONFLICT_FILES" ]]; then
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

if [[ "$SOURCE_ONLY" == true ]]; then
    ok "Source cwd: $SOURCE_ROOT (checkout topology checked above)"
    CWD_OK=true
elif [[ -n "$MAIN_REPO" && "$CWD" = "$MAIN_REPO" ]]; then
    err "cwd is the main repo root ($MAIN_REPO). Agents must run from their worktree."
    RECOVERY_WT=""
    while IFS= read -r wt_line || [[ -n "$wt_line" ]]; do
        if [[ "$wt_line" == worktree\ * ]]; then
            wt_candidate="${wt_line#worktree }"
            wt_resolved=""
            if wt_resolved="$(cd "$wt_candidate" 2>/dev/null && pwd -P)"; then
                if [[ "$wt_resolved" != "$MAIN_REPO" ]]; then
                    RECOVERY_WT="$wt_resolved"
                    break
                fi
            fi
        fi
    done < <(git worktree list --porcelain 2>/dev/null || true)
    if [[ -n "$RECOVERY_WT" ]]; then
        echo "    Fix: run git worktree list --porcelain, pick an isolated worktree other than the main checkout, then:"
        echo "    cd \"$RECOVERY_WT\""
    else
        echo "    Fix: Spawn agent with isolation: worktree in the agent definition"
        echo "    The main checkout is: $GIT_COMMON_DIR"
    fi
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

if [[ "$SOURCE_ONLY" == true ]]; then
    ok "Cargo environment not assessed for source authoring; use exact-command admission before building"
    TARGET_DIR_OK=true
elif [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
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
# --git-path honors the installer's repo-local core.hooksPath, so this checks
# this tree's own hooks dir (#17414 rule C). The output is relative to the
# cwd, which is where this script runs, so no --path-format is needed.
INSTALLED_HOOK="$(git rev-parse --git-path hooks 2>/dev/null)/pre-push"
INSTALLED_COMMIT_HOOK="$(git rev-parse --git-path hooks 2>/dev/null)/pre-commit"
CHECKED_IN_HOOK="$REPO_ROOT_AGENT/hooks/pre-push"
HOOK_INSTALLER_FIX="bash scripts/install-githooks.sh"
if [[ "$SOURCE_ONLY" == true ]]; then
    ok "Git hooks not assessed for source authoring; local Git writes retain their hook requirements"
    HOOKS_OK=true
elif [[ ! -f "$CHECKED_IN_HOOK" ]]; then
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
if [[ "$SOURCE_ONLY" == true ]]; then
    echo "Source preflight passed. Cargo, local Git hooks, publication and merge are not assessed."
else
    echo "Preflight passed. Safe to begin work."
fi
exit 0
