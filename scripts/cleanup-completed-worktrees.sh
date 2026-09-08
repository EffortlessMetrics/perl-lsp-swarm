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
#   worktree-manager owner           → KEEP (another runtime owns it)
#   everything else                  → REMOVE (reconstructible from the remote)
#
# An open PR is deliberately NOT a reason to keep a worktree. A fully pushed
# branch is restored with one `git worktree add`; the branch, the PR, and the
# review all survive removal. Keeping it only preserves a build cache.
#
# This sweep does not delete local branches. If a branch-deletion action is
# added, it must call scripts/branch-deletion-admission (plan --pr <number>)
# after the worktree is gone and treat every non-zero result as retention.
# The shared admission is bound to the exact parent PR, repository, branch tip,
# live child graph, and #3957 worktree ownership.
#
# --dry-run is an inspection front door and is strictly read-only: it performs no
# fetch, no `git worktree prune`, no worktree removal, no branch deletion, and no
# other write to Git metadata, refs, config, or the filesystem. Observation must
# never call a primitive that can erase the evidence it is observing — a global
# prune can drop an administrative registration whose path is merely unreachable
# from the current OS view (a Windows-registered worktree seen from WSL), which
# both destroys the registration and makes the row vanish from this report.
#
# Read-only is not the same as read-shaped. `git status` opportunistically
# refreshes the index and rewrites `.git/worktrees/<id>/index` whenever a tracked
# file's cached stat data is stale, so an apparently innocent query writes to Git
# metadata. Every observation runs through `git_read`, which passes
# `--no-optional-locks` to suppress exactly that class of incidental write.
#
# Because --dry-run does not fetch, it judges against the remote-tracking refs as
# they already stand. Those refs usually only lag the remote, so verdicts usually
# err toward KEEP — but they are not monotonic: a forced update can move
# `origin/X` backward and a deleted branch can remove it, so a stale ref may still
# contain commits the remote has since dropped. A dry-run REMOVE is therefore a
# proposal, never proof. Nothing is removed on that evidence: the mutating sweep
# fetches first and re-classifies against fresh refs before it acts.
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

# Every observation goes through here. `--no-optional-locks` stops git from
# taking the index lock for opportunistic work — without it `git status` rewrites
# a worktree's index whenever cached stat data is stale, which is a metadata
# write on a path that promises none.
git_read() {
    git --no-optional-locks "$@"
}

REPO_ROOT="$(git_read rev-parse --path-format=absolute --git-common-dir | sed 's|/\.git$||')"
MAIN_WT="$(git_read -C "$REPO_ROOT" rev-parse --show-toplevel)"
CURRENT_WT="$(git_read rev-parse --show-toplevel)"
STATE_FILE="$REPO_ROOT/.ops-perl-lsp/worktree-manager/state.json"

# When emitting JSON, git stdout/stderr must not pollute the stream.
git_out() {
    if $JSON; then
        "$@" >/dev/null 2>&1
    else
        "$@"
    fi
}

# Returns non-empty owner when worktree-manager records an owner for this path.
managed_owner() {
    local path="$1"
    [[ -f "$STATE_FILE" ]] || return 0
    if command -v jq >/dev/null 2>&1; then
        jq -r --arg p "$path" --arg rr "$REPO_ROOT" '
            .slots[]?
            | select(
                ((.path | if startswith("/") then . else ($rr + "/" + .) end) == $p)
                and (.owner // "" | length > 0)
              )
            | .owner
        ' "$STATE_FILE" 2>/dev/null | head -n1
        return 0
    fi
    if command -v python3 >/dev/null 2>&1; then
        python3 - "$STATE_FILE" "$path" "$REPO_ROOT" <<'PY'
import json, os, sys
from pathlib import Path

state_file, wt_path, repo_root = sys.argv[1:4]

def normalize(path: str) -> str:
    try:
        return os.path.realpath(path)
    except OSError:
        return os.path.normpath(path)

target = normalize(wt_path)
with open(state_file, encoding="utf-8") as handle:
    state = json.load(handle)
for slot in state.get("slots", []):
    raw = slot.get("path", "")
    resolved = raw if os.path.isabs(raw) else str(Path(repo_root) / raw)
    owner = slot.get("owner") or ""
    if normalize(resolved) == target and owner:
        print(owner)
        break
PY
    fi
}

# Remove without --force so concurrent use or post-check dirtiness fails closed.
remove_worktree() {
    local path="$1"
    $DRY_RUN && return 0
    git_out git -C "$REPO_ROOT" worktree remove "$path"
}

# Canonical `owner/name` for the repository origin points at.
#
# Without it, `gh` infers the repository from the working directory, so a
# branch can be matched against a pull request in a different repository. An
# underivable origin is a fail-closed refusal, not a fallback to inference.
origin_repo_slug() {
    git -C "$REPO_ROOT" remote get-url origin 2>/dev/null \
        | sed -E 's#\.git$##; s#^.*[:/]([^/]+/[^/]+)$#\1#'
}

# Succeed only when the local branch tip is the tip the admission was granted
# for. The admission reasons about the REMOTE branch; deleting the local ref is
# a separate act, and a local tip carrying commits that never reached the
# remote is unsalvaged work no admission covered.
local_tip_is_admitted() {
    local branch="$1" local_sha remote_sha

    local_sha="$(git -C "$REPO_ROOT" rev-parse --verify --quiet "refs/heads/$branch" 2>/dev/null)" \
        || return 1
    [[ -n "$local_sha" ]] || return 1

    remote_sha="$(git -C "$REPO_ROOT" ls-remote origin "refs/heads/$branch" 2>/dev/null \
        | awk 'NR==1{print $1}')"
    if [[ -z "$remote_sha" ]]; then
        remote_sha="$(git -C "$REPO_ROOT" rev-parse --verify --quiet \
            "refs/remotes/origin/$branch" 2>/dev/null)"
    fi
    [[ -n "$remote_sha" ]] || return 1

    [[ "$local_sha" == "$remote_sha" ]] || return 1
    # Emit the proven oid: the deletion must use THIS value, not a fresh read.
    printf '%s\n' "$local_sha"
}

# Ask the shared live admission before deleting a local branch (#12885). No gh,
# an underivable origin, a failed lookup, a non-numeric result, a retaining
# admission, or a local tip that is not the admitted remote tip all retain.
branch_deletion_admitted() {
    local branch="$1" pr_number repo_slug admission

    command -v gh >/dev/null 2>&1 || return 1
    repo_slug="$(origin_repo_slug)" || return 1
    [[ -n "$repo_slug" && "$repo_slug" == */* ]] || return 1

    pr_number="$(gh pr list --repo "$repo_slug" --head "$branch" --state merged \
        --json number --jq '.[0].number' 2>/dev/null)" || return 1
    [[ "$pr_number" =~ ^[0-9]+$ ]] || return 1

    admission="$(cd "$REPO_ROOT" && pwd)/scripts/branch-deletion-admission"
    [[ -f "$admission" ]] || return 1
    bash "$admission" plan --pr "$pr_number" --remote origin >/dev/null 2>&1 || return 1

    local_tip_is_admitted "$branch"
}


delete_branch() {
    local branch="$1" admitted_tip
    $DRY_RUN && return 0
    if ! admitted_tip="$(branch_deletion_admitted "$branch")"; then
        printf '    -> retaining local branch %s: branch-deletion admission refused\n' \
            "$branch" >&2
        return 0
    fi
    # The oid comes from the admission check, not a fresh read. Re-reading would
    # make the CAS atomic only against its own read: a ref that advanced after
    # the equality check would become the new expected value and be deleted.
    local expected="$admitted_tip"
    if [[ -z "$expected" ]]; then
        printf '    -> retaining local branch %s: no admitted tip was carried\n' "$branch" >&2
        return 0
    fi
    # Atomic compare-and-delete on the admitted tip: a ref that advanced between
    # the check above and this deletion is preserved, not discarded.
    if ! git_out git -C "$REPO_ROOT" update-ref -d "refs/heads/$branch" "$expected"; then
        printf '    -> retaining local branch %s: it moved between admission and deletion\n' \
            "$branch" >&2
    fi
}

branch_landed_via_pr() {
    local branch="$1" repo_slug
    [[ -n "$branch" ]] || return 1
    command -v gh >/dev/null 2>&1 || return 1
    repo_slug="$(origin_repo_slug)" || return 1
    [[ -n "$repo_slug" && "$repo_slug" == */* ]] || return 1
    gh pr list --repo "$repo_slug" --head "$branch" --state merged --json number 2>/dev/null \
        | grep -q '"number"'
}

# Stale origin refs make "unpushed" wrong in the dangerous direction, so refresh
# before judging. A fetch failure is not fatal, but it downgrades every verdict.
#
# A fetch updates refs/remotes/**, so it is a mutation and is not permitted on the
# inspection path. Freshness is therefore tracked as its own axis, independent of
# mutation authority:
#
#   fresh   refs refreshed from the remote this run
#   stale   --dry-run; refs used as they already stand, so verdicts are proposals
#   failed  fetch attempted and failed; remote-dependent verdicts are NOT_PROVEN
FETCH_OK=true
REMOTE_STATE=fresh
if $DRY_RUN; then
    REMOTE_STATE=stale
else
    git_out git -C "$REPO_ROOT" fetch --quiet origin "$BASE" 2>/dev/null ||
        { FETCH_OK=false; REMOTE_STATE=failed; }
fi
BASE_REF="origin/$BASE"
git_read -C "$REPO_ROOT" rev-parse --verify --quiet "$BASE_REF" >/dev/null || BASE_REF="$BASE"

REMOVED=0; KEPT=0; SKIPPED=0; REVIEWED=0; TOTAL=0
ROWS=()

if ! $JSON; then
    echo "=== Worktree sweep ==="
    echo "Repo root:   $REPO_ROOT"
    echo "Base:        $BASE_REF"
    echo "Dry run:     $DRY_RUN"
    echo "Remote refs: $REMOTE_STATE"
    case "$REMOTE_STATE" in
        stale)  echo "NOTE:        read-only inspection; refs not refreshed, so verdicts are"
                echo "             provisional — the sweep re-fetches before it acts" ;;
        failed) echo "WARNING:     fetch failed; 'unpushed' verdicts are NOT_PROVEN" ;;
    esac
    echo ""
    printf "%-26s %-40s %-14s %s\n" "WORKTREE" "BRANCH" "STATE" "ACTION"
    printf "%-26s %-40s %-14s %s\n" "--------" "------" "-----" "------"
fi

emit() {
    local name="$1" branch="$2" state="$3" action="$4"
    if $JSON; then
        command -v jq >/dev/null 2>&1 || { echo "jq required for --json" >&2; exit 2; }
        ROWS+=("$(jq -cn --arg name "$name" --arg branch "$branch" --arg state "$state" --arg action "$action" \
            '{worktree:$name, branch:$branch, state:$state, action:$action}')")
    else
        printf "%-26s %-40s %-14s %s\n" "$name" "$branch" "$state" "$action"
    fi
}

# `git worktree prune` rewrites .git/worktrees/**. It is mutation, and running it
# before classification lets observation change its own subject: a registration
# whose path is unreachable from this OS view is dropped, so the row disappears
# from the report instead of being reported for review.
prune_worktrees() {
    $DRY_RUN && return 0
    git_out git -C "$REPO_ROOT" worktree prune
}

prune_worktrees

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
    local owner; owner="$(managed_owner "$path")"
    if [[ -n "$owner" ]]; then
        emit "$name" "${branch:-(detached)}" "owned:$owner" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi
    # An absent path is not proof that the registration is stale: it may simply be
    # unreachable from this OS view. A drive-letter/backslash registration read on
    # a POSIX host is the observed cross-OS signature. Neither is ever resolved by
    # guessing a path conversion, and neither is ever a cleanup candidate.
    if [[ ! -d "$path" ]]; then
        local unreachable="missing"
        if [[ "$path" == *\\* || "$path" =~ ^[A-Za-z]:[/\\] ]]; then
            unreachable="foreign-path"
        fi
        emit "$name" "${branch:-(detached)}" "$unreachable" "REVIEW"
        REVIEWED=$((REVIEWED + 1)); return 0
    fi

    # Uncommitted work is unique by definition. Check it first and never remove.
    if [[ -n "$(git_read -C "$path" status --porcelain 2>/dev/null || echo dirty)" ]]; then
        emit "$name" "${branch:-(detached)}" "dirty" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi

    local head landed
    head="$(git_read -C "$path" rev-parse HEAD 2>/dev/null || echo "")"
    if [[ -z "$head" ]]; then
        emit "$name" "${branch:-(detached)}" "unreadable" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi
    landed=false
    git_read -C "$REPO_ROOT" merge-base --is-ancestor "$head" "$BASE_REF" 2>/dev/null && landed=true
    if ! $landed && [[ "$detached" != "true" && -n "${branch:-}" ]]; then
        branch_landed_via_pr "$branch" && landed=true
    fi

    # Detached HEAD: the old script skipped these permanently. Judge them.
    if [[ "$detached" == "true" ]]; then
        if $landed; then
            emit "$name" "(detached)" "landed" "REMOVE"
            if remove_worktree "$path"; then
                REMOVED=$((REMOVED + 1))
            else
                KEPT=$((KEPT + 1))
            fi
        else
            emit "$name" "(detached)" "unique-commits" "KEEP"; KEPT=$((KEPT + 1))
        fi
        return 0
    fi

    if $landed; then
        emit "$name" "$branch" "landed" "REMOVE"
        if remove_worktree "$path"; then
            delete_branch "$branch"
            REMOVED=$((REMOVED + 1))
        else
            KEPT=$((KEPT + 1))
        fi
        return 0
    fi

    # Not landed: is every commit already on the remote?
    if ! git_read -C "$REPO_ROOT" rev-parse --verify --quiet "origin/$branch" >/dev/null; then
        emit "$name" "$branch" "no-remote" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi
    if ! $FETCH_OK; then
        emit "$name" "$branch" "not-proven" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi

    local ahead
    ahead="$(git_read -C "$REPO_ROOT" rev-list --count "origin/$branch..$head" 2>/dev/null || echo 1)"
    if [[ "$ahead" -gt 0 ]]; then
        emit "$name" "$branch" "unpushed:$ahead" "KEEP"; KEPT=$((KEPT + 1)); return 0
    fi

    # Fully pushed. An open PR is not a reason to keep the directory: the branch,
    # PR, and review all survive, and `git worktree add` restores it on demand.
    emit "$name" "$branch" "pushed" "REMOVE"
    if remove_worktree "$path"; then
        REMOVED=$((REMOVED + 1))
    else
        KEPT=$((KEPT + 1))
    fi
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
done < <(git_read -C "$REPO_ROOT" worktree list --porcelain)
process_worktree "$WT_PATH" "$WT_BRANCH" "$WT_LOCKED" "$WT_DETACHED"

prune_worktrees

if $JSON; then
    WORKTREES_JSON="$(printf '%s\n' "${ROWS[@]}" | jq -s '.')"
    # `fetch_ok` is retained for compatibility and means "remote-dependent
    # verdicts are admissible", not "a fetch was performed" — under --dry-run no
    # fetch happens and it is still true. `remote_state` is the precise axis.
    jq -cn \
        --argjson removed "$REMOVED" \
        --argjson kept "$KEPT" \
        --argjson skipped "$SKIPPED" \
        --argjson review "$REVIEWED" \
        --argjson total "$TOTAL" \
        --argjson fetch_ok "$FETCH_OK" \
        --arg remote_state "$REMOTE_STATE" \
        --argjson dry_run "$DRY_RUN" \
        --argjson worktrees "$WORKTREES_JSON" \
        '{removed:$removed, kept:$kept, skipped:$skipped, review:$review, total:$total, fetch_ok:$fetch_ok, remote_state:$remote_state, dry_run:$dry_run, worktrees:$worktrees}'
else
    echo ""
    echo "=== Summary ==="
    # A read-only inspection has removed nothing. Naming the count "Removed"
    # would report a proposed action as one already taken.
    if $DRY_RUN; then
        echo "Propose: $REMOVED (removal proposed, not performed)"
    else
        echo "Removed: $REMOVED"
    fi
    echo "Kept:    $KEPT"
    echo "Skipped: $SKIPPED"
    echo "Review:  $REVIEWED"
    echo "Total:   $TOTAL"
    # Guard the exit status: a bare `$DRY_RUN && ...` tail returns 1 on a real
    # sweep, so every successful non-dry-run exited non-zero.
    if $DRY_RUN; then
        echo ""
        echo "(Dry run — read-only: no fetch, prune, removal, or branch deletion.)"
    fi
fi
