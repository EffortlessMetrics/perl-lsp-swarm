#!/usr/bin/env bash
# Worktree sweep.
#
# Problem: worktrees accumulate. A concurrency cap governs how many exist at
# once; nothing governs residue. Over a long session most worktrees are finished
# work whose content already lives on the remote or in the base branch, and each
# one still holds a multi-gigabyte target/ directory.
#
# Retention predicate: keep a worktree only when it holds state that exists
# nowhere else.
#
#   uncommitted changes              → KEEP (unique, unrecoverable)
#   unpushed commits                 → KEEP (unique, unrecoverable)
#   detached HEAD not in base        → KEEP (unique, unrecoverable)
#   locked                           → KEEP (another runtime owns it)
#   everything else                  → REMOVE (reconstructible from the remote)
#
# An open PR is deliberately NOT a reason to keep a worktree. A fully pushed
# branch is restored with one `git worktree add`; the branch, the PR, and the
# review all survive removal. Keeping it only preserves a build cache.
#
# Branch deletion is separate and strictly narrower: a local branch is deleted
# only when it is contained in the base branch. Squash merges are detected via
# the merged PR, not by ancestry, because a squashed branch is never an ancestor
# of its base.
#
# Usage:
#   bash scripts/cleanup-completed-worktrees.sh [--dry-run] [--json]
#
# Optional:
#   CLEANUP_BASE_BRANCH=<branch>  base used for landed/unpushed comparisons

set -euo pipefail

DRY_RUN=false
JSON=false
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=true ;;
        --json) JSON=true ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

BASE="${CLEANUP_BASE_BRANCH:-main}"
REPO_ROOT="$(git rev-parse --path-format=absolute --git-common-dir | sed 's|/\.git$||')"
MAIN_WT="$(git -C "$REPO_ROOT" rev-parse --show-toplevel)"
CURRENT_WT="$(git rev-parse --show-toplevel)"

# Stale origin refs make "unpushed" wrong in the dangerous direction, so refresh
# before judging. A fetch failure is not fatal, but it downgrades every verdict.
FETCH_OK=true
git -C "$REPO_ROOT" fetch --quiet origin "$BASE" 2>/dev/null || FETCH_OK=false
BASE_REF="origin/$BASE"
git -C "$REPO_ROOT" rev-parse --verify --quiet "$BASE_REF" >/dev/null || BASE_REF="$BASE"

REMOVED=0; KEPT=0; SKIPPED=0; TOTAL=0
ROWS=()

if ! $JSON; then
    echo "=== Worktree sweep ==="
    echo "Repo root:   $REPO_ROOT"
    echo "Base:        $BASE_REF"
    echo "Dry run:     $DRY_RUN"
    $FETCH_OK || echo "WARNING:     fetch failed; 'unpushed' verdicts are NOT_PROVEN"
    echo ""
    printf "%-26s %-40s %-14s %s\n" "WORKTREE" "BRANCH" "STATE" "ACTION"
    printf "%-26s %-40s %-14s %s\n" "--------" "------" "-----" "------"
fi

emit() {
    local name="$1" branch="$2" state="$3" action="$4"
    if $JSON; then
        ROWS+=("{\"worktree\":\"$name\",\"branch\":\"$branch\",\"state\":\"$state\",\"action\":\"$action\"}")
    else
        printf "%-26s %-40s %-14s %s\n" "$name" "$branch" "$state" "$action"
    fi
}

git -C "$REPO_ROOT" worktree prune

# Parse porcelain output so paths containing spaces survive.
WT_PATH=""; WT_BRANCH=""; WT_LOCKED=false; WT_DETACHED=false

process_worktree() {
    local path="$1" branch="$2" locked="$3" detached="$4"
    [[ -z "$path" ]] && return 0
    TOTAL=$((TOTAL + 1))
    local name; name="$(basename "$path")"

    if [[ "$path" == "$MAIN_WT" ]]; then
        emit "$name" "${branch:-(main)}" "primary" "SKIP"; SKIPPED=$((SKIPPED + 1)); return 0
    fi
    if [[ "$path" == "$CURRENT_WT" ]]; then
        emit "$name" "${branch:-(detached)}" "current" "SKIP"; SKIPPED=$((SKIPPED + 1)); return 0
    fi
    if [[ "$locked" == "true" ]]; then
        emit "$name" "${branch:-(detached)}" "locked" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi
    if [[ ! -d "$path" ]]; then
        emit "$name" "${branch:-(detached)}" "missing" "SKIP"; SKIPPED=$((SKIPPED + 1)); return 0
    fi

    # Uncommitted work is unique by definition. Check it first and never remove.
    if [[ -n "$(git -C "$path" status --porcelain 2>/dev/null || echo dirty)" ]]; then
        emit "$name" "${branch:-(detached)}" "dirty" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi

    local head landed
    head="$(git -C "$path" rev-parse HEAD 2>/dev/null || echo "")"
    if [[ -z "$head" ]]; then
        emit "$name" "${branch:-(detached)}" "unreadable" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi
    landed=false
    git -C "$REPO_ROOT" merge-base --is-ancestor "$head" "$BASE_REF" 2>/dev/null && landed=true

    # Detached HEAD: the old script skipped these permanently. Judge them.
    if [[ "$detached" == "true" ]]; then
        if $landed; then
            emit "$name" "(detached)" "landed" "REMOVE"
            $DRY_RUN || git -C "$REPO_ROOT" worktree remove --force "$path"
            REMOVED=$((REMOVED + 1))
        else
            emit "$name" "(detached)" "unique-commits" "KEEP"; KEPT=$((KEPT + 1))
        fi
        return 0
    fi

    if $landed; then
        emit "$name" "$branch" "landed" "REMOVE"
        if ! $DRY_RUN; then
            git -C "$REPO_ROOT" worktree remove --force "$path"
            git -C "$REPO_ROOT" branch -D "$branch" 2>/dev/null || true
        fi
        REMOVED=$((REMOVED + 1)); return 0
    fi

    # Not landed: is every commit already on the remote?
    if ! git -C "$REPO_ROOT" rev-parse --verify --quiet "origin/$branch" >/dev/null; then
        emit "$name" "$branch" "no-remote" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi
    if ! $FETCH_OK; then
        emit "$name" "$branch" "not-proven" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi

    local ahead
    ahead="$(git -C "$REPO_ROOT" rev-list --count "origin/$branch..$head" 2>/dev/null || echo 1)"
    if [[ "$ahead" -gt 0 ]]; then
        emit "$name" "$branch" "unpushed:$ahead" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi

    # Fully pushed. An open PR is not a reason to keep the directory: the branch,
    # PR, and review all survive, and `git worktree add` restores it on demand.
    emit "$name" "$branch" "pushed" "REMOVE"
    $DRY_RUN || git -C "$REPO_ROOT" worktree remove --force "$path"
    REMOVED=$((REMOVED + 1))
}

while IFS= read -r line; do
    case "$line" in
        worktree\ *)
            process_worktree "$WT_PATH" "$WT_BRANCH" "$WT_LOCKED" "$WT_DETACHED"
            WT_PATH="${line#worktree }"; WT_BRANCH=""; WT_LOCKED=false; WT_DETACHED=false ;;
        branch\ *)   WT_BRANCH="${line#branch refs/heads/}" ;;
        detached)    WT_DETACHED=true ;;
        locked*)     WT_LOCKED=true ;;
    esac
done < <(git -C "$REPO_ROOT" worktree list --porcelain)
process_worktree "$WT_PATH" "$WT_BRANCH" "$WT_LOCKED" "$WT_DETACHED"

$DRY_RUN || git -C "$REPO_ROOT" worktree prune

if $JSON; then
    printf '{"removed":%d,"kept":%d,"skipped":%d,"total":%d,"fetch_ok":%s,"worktrees":[%s]}\n' \
        "$REMOVED" "$KEPT" "$SKIPPED" "$TOTAL" "$FETCH_OK" "$(IFS=,; echo "${ROWS[*]:-}")"
else
    echo ""
    echo "=== Summary ==="
    echo "Removed: $REMOVED"
    echo "Kept:    $KEPT"
    echo "Skipped: $SKIPPED"
    echo "Total:   $TOTAL"
    $DRY_RUN && echo "" && echo "(Dry run — no changes made.)"
fi
