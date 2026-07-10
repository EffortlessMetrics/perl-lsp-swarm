#!/usr/bin/env bash
# scripts/clean-worktrees.sh
#
# Reap stale agent git worktrees to reclaim disk space.
#
# BUG FIXED (issue #3573): the old `clean-worktrees` recipe globbed a fixed
# path `../<repo>-worktrees/`, but agents actually create worktrees under
# `.claude/worktrees/` inside the repo. The glob was therefore a no-op for
# the directory that actually accumulates — on 2026-07-09 this let 129
# worktrees pile up and filled the volume to 100% (57 GB free).
#
# This script is location-agnostic: it enumerates *real* worktrees via
# `git worktree list --porcelain` instead of globbing a fixed directory.
#
# Usage:
#   bash scripts/clean-worktrees.sh              # dry-run: list what WOULD happen
#   APPLY=1 bash scripts/clean-worktrees.sh       # actually salvage + remove
#   GRACE_HOURS=6 bash scripts/clean-worktrees.sh # override the recent-activity threshold
#
# Safety guarantees (keep-list — NEVER removed):
#   - The root/primary worktree (the main checkout)
#   - Any worktree git reports as `locked` (an active agent)
#   - Any worktree whose branch has an open PR (checked once, batched)
#   - Any worktree modified within GRACE_HOURS (default 6h)
#
# Salvage boundary:
#   - A DIRTY candidate (uncommitted changes) is never silently discarded.
#     Before force-removal, a recovery packet is written to
#     .claude/worktree-archive/<YYYY-MM-DD>/<worktree-name>/ containing
#     meta.txt, staged.patch, unstaged.patch, and copies of untracked
#     non-ignored source files. target/, node_modules/, *.vsix, and other
#     git-ignored build caches are never copied.
#   - A CLEAN candidate needs no packet (lossless removal).
#
# Processing order: deepest-nested worktree paths first, so removing a
# parent can never orphan a still-registered child worktree.
#
# Dry-run by default; explicit APPLY=1 required to actually remove/salvage.

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────

APPLY="${APPLY:-0}"
GRACE_HOURS="${GRACE_HOURS:-6}"
# Disk floor: warn (never hard-fail) when free space on the worktrees volume
# drops below max(FLOOR_GB, FLOOR_PCT% of total volume size).
FLOOR_GB="${FLOOR_GB:-200}"
FLOOR_PCT="${FLOOR_PCT:-5}"

for arg in "$@"; do
    case "$arg" in
        --apply) APPLY=1 ;;
        --dry-run) APPLY=0 ;;
        --help|-h)
            grep '^#' "$0" | sed 's/^# //' | sed 's/^#//'
            exit 0
            ;;
        *)
            echo "Unknown flag: $arg" >&2
            echo "Usage: $0 [--apply|--dry-run]" >&2
            exit 1
            ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [[ "$APPLY" -eq 1 ]]; then
    echo "=== clean-worktrees: APPLY MODE (will salvage + remove) ==="
else
    echo "=== clean-worktrees: DRY-RUN (pass APPLY=1 or --apply to act) ==="
fi
echo ""

# ── Disk floor report (before) ─────────────────────────────────────────────────

report_disk_floor() {
    local label="$1"
    local df_line
    df_line="$(df -k "$REPO_ROOT" 2>/dev/null | awk 'NR==2 {print $2, $4}')"
    local total_kb avail_kb
    total_kb="$(echo "$df_line" | awk '{print $1}')"
    avail_kb="$(echo "$df_line" | awk '{print $2}')"
    if [[ -z "$total_kb" || -z "$avail_kb" ]]; then
        echo "[$label] disk usage: unavailable (df parse failed)"
        return
    fi
    local avail_gb=$((avail_kb / 1024 / 1024))
    local total_gb=$((total_kb / 1024 / 1024))
    local floor_kb_gb=$((FLOOR_GB * 1024 * 1024))
    local floor_kb_pct=$((total_kb * FLOOR_PCT / 100))
    local floor_kb=$floor_kb_gb
    if [[ "$floor_kb_pct" -gt "$floor_kb" ]]; then
        floor_kb=$floor_kb_pct
    fi
    local floor_gb=$((floor_kb / 1024 / 1024))
    echo "[$label] disk: ${avail_gb}G free / ${total_gb}G total (floor: max(${FLOOR_GB}G, ${FLOOR_PCT}% of ${total_gb}G) = ${floor_gb}G)"
    if [[ "$avail_kb" -lt "$floor_kb" ]]; then
        echo "  WARN: free space below floor (${avail_gb}G < ${floor_gb}G)"
    fi
}

report_disk_floor "BEFORE"
echo ""

# ── Reap orphaned /tmp build-target dirs first (existing behavior, preserved) ──

echo "Reaping orphaned /tmp agent build targets..."
if [[ "$APPLY" -eq 1 ]]; then
    APPLY=1 bash "$REPO_ROOT/scripts/clean-tmp-targets.sh" --prune || true
else
    bash "$REPO_ROOT/scripts/clean-tmp-targets.sh" || true
fi
echo ""

echo "Pruning unreferenced worktree metadata..."
git worktree prune
echo ""

# ── Enumerate real worktrees (location-agnostic) ───────────────────────────────

echo "Scanning registered worktrees (git worktree list --porcelain)..."

# Batch the open-PR lookup ONCE — never call gh per worktree.
OPEN_PR_BRANCHES_FILE="$(mktemp)"
gh pr list --state open --json headRefName -L 800 --jq '.[].headRefName' \
    > "$OPEN_PR_BRANCHES_FILE" 2>/dev/null || true

is_open_pr_branch() {
    local branch="$1"
    [[ -n "$branch" ]] || return 1
    grep -qxF "$branch" "$OPEN_PR_BRANCHES_FILE" 2>/dev/null
}

# Parse `git worktree list --porcelain` into parallel arrays.
declare -a WT_PATHS=()
declare -a WT_BRANCHES=()
declare -a WT_LOCKED=()

wt_path="" wt_branch="" wt_locked="0"
flush_worktree() {
    if [[ -n "$wt_path" ]]; then
        WT_PATHS+=("$wt_path")
        WT_BRANCHES+=("$wt_branch")
        WT_LOCKED+=("$wt_locked")
    fi
}
while IFS= read -r line; do
    case "$line" in
        "worktree "*)
            flush_worktree
            wt_path="${line#worktree }"
            wt_branch=""
            wt_locked="0"
            ;;
        "branch "*)
            wt_branch="${line#branch }"
            wt_branch="${wt_branch#refs/heads/}"
            ;;
        "locked"*)
            wt_locked="1"
            ;;
    esac
done < <(git worktree list --porcelain)
flush_worktree

# Root worktree is always the first entry from `git worktree list`.
ROOT_WORKTREE="${WT_PATHS[0]:-}"

# ── Build candidate list, excluding the root worktree ───────────────────────────

declare -a CANDIDATE_PATHS=()
declare -a CANDIDATE_DEPTHS=()

for i in "${!WT_PATHS[@]}"; do
    [[ "$i" -eq 0 ]] && continue   # skip root
    p="${WT_PATHS[$i]}"
    depth="$(grep -o "/" <<<"$p" | wc -l)"
    CANDIDATE_PATHS+=("$p")
    CANDIDATE_DEPTHS+=("$depth")
done

# Sort candidate indices by depth descending (deepest-nested first) so a
# parent removal never orphans a still-registered child worktree.
ORDER=()
if [[ "${#CANDIDATE_PATHS[@]}" -gt 0 ]]; then
    while IFS= read -r idx; do
        ORDER+=("$idx")
    done < <(
        for i in "${!CANDIDATE_PATHS[@]}"; do
            echo "${CANDIDATE_DEPTHS[$i]} $i"
        done | sort -rn -k1,1 | awk '{print $2}'
    )
fi

# ── Salvage helpers ──────────────────────────────────────────────────────────

worktree_last_activity_ts() {
    # Activity signal is the mtime of the *worktree-specific* HEAD ref file
    # (git-dir/HEAD, e.g. .git/worktrees/<name>/HEAD) — this updates only
    # when THIS worktree does a checkout/commit/rebase, so it reflects real
    # local activity. Deliberately NOT the tip commit's author-date (git log
    # -1 --format=%ct): a worktree freshly branched from an active main tip
    # would inherit a "recent" author date despite zero local activity,
    # producing false KEEPs.
    local wt="$1"
    local git_dir head_mtime dirty_max=0
    git_dir="$(git -C "$wt" rev-parse --absolute-git-dir 2>/dev/null || echo "")"
    head_mtime=0
    if [[ -n "$git_dir" && -f "$git_dir/HEAD" ]]; then
        head_mtime="$(stat -c %Y "$git_dir/HEAD" 2>/dev/null || echo 0)"
    fi
    [[ -z "$head_mtime" ]] && head_mtime=0
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        local rel="${line:3}"
        local f="$wt/$rel"
        if [[ -e "$f" ]]; then
            local m
            m="$(stat -c %Y "$f" 2>/dev/null || echo 0)"
            [[ "$m" -gt "$dirty_max" ]] && dirty_max="$m"
        fi
    done < <(git -C "$wt" status --porcelain 2>/dev/null || true)
    local max=$head_mtime
    [[ "$dirty_max" -gt "$max" ]] && max=$dirty_max
    echo "$max"
}

salvage_worktree() {
    local wt="$1"
    local name
    name="$(basename "$wt")"
    local date_dir
    date_dir="$(date +%Y-%m-%d)"
    local archive_dir="$REPO_ROOT/.claude/worktree-archive/$date_dir/$name"

    echo "    salvage -> $archive_dir"
    mkdir -p "$archive_dir"

    local branch head_sha status_out
    branch="$(git -C "$wt" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")"
    head_sha="$(git -C "$wt" rev-parse HEAD 2>/dev/null || echo "unknown")"
    status_out="$(git -C "$wt" status --porcelain 2>/dev/null || echo "")"

    {
        echo "worktree_path: $wt"
        echo "branch: $branch"
        echo "head_sha: $head_sha"
        echo "archived_at: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo ""
        echo "git status --porcelain:"
        echo "$status_out"
    } > "$archive_dir/meta.txt"

    git -C "$wt" diff --cached --binary > "$archive_dir/staged.patch" 2>/dev/null || true
    git -C "$wt" diff --binary > "$archive_dir/unstaged.patch" 2>/dev/null || true

    # Copy untracked, non-ignored files — explicitly skip build caches even
    # if --exclude-standard somehow lets one through.
    local untracked_dir="$archive_dir/untracked"
    while IFS= read -r rel; do
        [[ -z "$rel" ]] && continue
        case "$rel" in
            target/*|*/target/*|node_modules/*|*/node_modules/*|*.vsix) continue ;;
        esac
        local src="$wt/$rel"
        [[ -f "$src" ]] || continue
        local dest="$untracked_dir/$rel"
        mkdir -p "$(dirname "$dest")"
        cp -p "$src" "$dest" 2>/dev/null || true
    done < <(git -C "$wt" ls-files --others --exclude-standard 2>/dev/null || true)
}

# ── Main reap loop ───────────────────────────────────────────────────────────

removed=0
kept=0
now_ts="$(date +%s)"
grace_seconds=$((GRACE_HOURS * 3600))

for idx in "${ORDER[@]}"; do
    wt="${CANDIDATE_PATHS[$idx]}"
    [[ -d "$wt" ]] || continue
    name="$(basename "$wt")"
    branch=""
    for i in "${!WT_PATHS[@]}"; do
        if [[ "${WT_PATHS[$i]}" == "$wt" ]]; then
            branch="${WT_BRANCHES[$i]}"
            locked="${WT_LOCKED[$i]}"
        fi
    done

    if [[ "${locked:-0}" == "1" ]]; then
        echo "  KEEP $name (locked — active agent)"
        kept=$((kept + 1))
        continue
    fi

    if is_open_pr_branch "$branch"; then
        echo "  KEEP $name (open PR on $branch)"
        kept=$((kept + 1))
        continue
    fi

    last_ts="$(worktree_last_activity_ts "$wt")"
    age_seconds=$((now_ts - last_ts))
    if [[ "$last_ts" -gt 0 && "$age_seconds" -lt "$grace_seconds" ]]; then
        age_h=$((age_seconds / 3600))
        echo "  KEEP $name (active within last ${age_h}h, grace=${GRACE_HOURS}h)"
        kept=$((kept + 1))
        continue
    fi

    is_dirty=0
    if [[ -n "$(git -C "$wt" status --porcelain 2>/dev/null)" ]]; then
        is_dirty=1
    fi

    if [[ "$is_dirty" -eq 1 ]]; then
        echo "  REMOVE $name (dirty — will salvage recovery packet first)"
    else
        echo "  REMOVE $name (clean — lossless removal)"
    fi

    if [[ "$APPLY" -eq 1 ]]; then
        if [[ "$is_dirty" -eq 1 ]]; then
            salvage_worktree "$wt"
        fi
        git worktree remove --force "$wt" 2>/dev/null || rm -rf "$wt"
    fi
    removed=$((removed + 1))
done

if [[ "$APPLY" -eq 1 ]]; then
    git worktree prune
fi

rm -f "$OPEN_PR_BRANCHES_FILE"

echo ""
if [[ "$APPLY" -eq 1 ]]; then
    echo "Done: removed $removed, kept $kept"
else
    echo "Done (dry-run): would remove $removed, would keep $kept"
    if [[ "$removed" -gt 0 ]]; then
        echo "Re-run with APPLY=1 (or --apply) to actually salvage + remove."
    fi
fi

echo ""
report_disk_floor "AFTER"
