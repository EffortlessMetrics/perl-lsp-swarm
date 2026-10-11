#!/usr/bin/env bash
# scripts/clean-tmp-targets.sh
#
# Advisory inspection of legacy agent /tmp build-target directories.
#
# These directories are created by agents via CARGO_TARGET_DIR=/tmp/agent-*-target
# (and similar patterns). Finished/zombie agents leave them behind; this script
# reports names, recency and size. These observations do not establish ownership,
# consumer closure or reclaimability. Exact reclaim planning is owned by #11671.
#
# Usage:
#   bash scripts/clean-tmp-targets.sh            # advisory report
#   bash scripts/clean-tmp-targets.sh --prune    # retired; refuses with exit 78
#   APPLY=1 bash scripts/clean-tmp-targets.sh   # retired; refuses with exit 78
#
# Inspection scope:
#   - Reports /tmp/agent-*-target and /tmp/wt-*-target naming patterns
#   - Annotates matching registered-worktree names and recent modification
#   - Missing Git observations refuse rather than implying orphaned state
#   - Performs no target deletion; name/age are never deletion authority

set -euo pipefail

# Refuse every destructive request before help, flag overrides or discovery.
# This legacy reaper does not participate in exact resource/consumer ownership.
if [[ "${APPLY:-0}" != 0 ]]; then
    echo "REFUSING: legacy temporary target deletion is retired; request an exact root-owned cleanup proposal" >&2
    exit 78
fi
for arg in "$@"; do
    if [[ "$arg" == --prune ]]; then
        echo "REFUSING: legacy temporary target deletion is retired; request an exact root-owned cleanup proposal" >&2
        exit 78
    fi
done

# ── Configuration ─────────────────────────────────────────────────────────────

# Recency annotation only; recent or old state does not prove consumer status.
GRACE_MINUTES="${GRACE_MINUTES:-5}"

# Parse advisory flags. Destructive flags have already refused.
for arg in "$@"; do
    case "$arg" in
        --dry-run) ;;
        --help|-h)
            grep '^#' "$0" | sed 's/^# //' | sed 's/^#//'
            exit 0
            ;;
        *)
            echo "Unknown flag: $arg" >&2
            echo "Usage: $0 [--dry-run] (--prune is retired)" >&2
            exit 1
            ;;
    esac
done

# ── Observe registered worktree names ──────────────────────────────────────────
#
# Registered names are advisory annotations, not exact allocation ownership.
#
# Heuristic: given a worktree at /path/to/worktrees/agent-XXXX the expected
# CARGO_TARGET_DIR is /tmp/agent-XXXX-target (the per-branch convention from
# agent-preflight.sh and CLAUDE.md). We derive registered name fragments
# from worktree paths and compare against candidate dir basenames.

collect_live_worktree_target_names() {
    local repo_root worktree_rows
    if ! repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || [[ -z "$repo_root" ]]; then
        echo "REFUSING: repository identity is unavailable" >&2
        return 75
    fi
    if ! worktree_rows="$(git -C "$repo_root" worktree list --porcelain 2>/dev/null)"; then
        echo "REFUSING: registered worktree observation failed" >&2
        return 75
    fi
    if ! grep -q '^worktree ' <<< "$worktree_rows"; then
        echo "REFUSING: registered worktree observation is empty or incomplete" >&2
        return 75
    fi

    # Collect all registered worktree paths and branches from git's porcelain
    # listing. We emit possible associated target basenames from three sources:
    #
    # 1. The worktree PATH basename — agent-preflight.sh names the target dir
    #    after the worktree dir (for the main checkout or early conventions).
    #    e.g. worktree path ending in "agent-XXXX" → "agent-XXXX-target"
    #
    # 2. The BRANCH NAME with '/' replaced by '-' — this is the primary
    #    convention from agent-preflight.sh:
    #    CARGO_TARGET_DIR=/tmp/agent-<branch | tr '/' '-'>-target
    #    e.g. branch "worktree-agent-XXXX" → "agent-worktree-agent-XXXX-target"
    #
    # 3. The worktree PATH basename under the "agent-worktree-<basename>"
    #    convention. Agents typically start on a default branch literally
    #    named "worktree-<dirname>" (matching #2 above at CARGO_TARGET_DIR
    #    creation time), then LATER rename/checkout to their real work
    #    branch (e.g. "impl/NNN-slug") once a spec/issue is assigned.
    #    CARGO_TARGET_DIR is fixed at session start and never updates on
    #    branch rename, so deriving names from the *current* branch
    #    (source #2) misses this case entirely — confirmed in production:
    #    PR #3577's `--prune` run reclaimed a live open-PR worktree's 4.5GB
    #    cache (`agent-worktree-agent-adb287b548d29729c-target`) because its
    #    branch had since been renamed to `impl/3396-...`, and neither #1
    #    nor #2 (evaluated against the current branch) matched the dir name
    #    that was fixed back when the branch was still "worktree-<dirname>".
    #    The path basename, unlike the branch, is stable across renames, so
    #    this pattern is derived from it directly rather than from history
    #    we can't observe.
    #
    # We annotate old, new, and renamed-branch naming conventions. Even an exact
    # name match cannot establish current allocation ownership or consumer state.

    local wt_path="" branch_ref=""
    while IFS= read -r line; do
        case "$line" in
            "worktree "*)
                # Flush previous worktree entry.
                if [[ -n "$wt_path" ]]; then
                    local wt_base
                    wt_base="$(basename "$wt_path")"
                    echo "agent-${wt_base#agent-}-target"
                    echo "${wt_base}-target"
                    echo "agent-worktree-${wt_base}-target"
                fi
                wt_path="${line#worktree }"
                branch_ref=""
                ;;
            "branch "*)
                branch_ref="${line#branch }"
                # Strip refs/heads/ prefix to get the short branch name.
                local branch_name="${branch_ref#refs/heads/}"
                # Convert '/' to '-' matching agent-preflight.sh convention.
                local branch_slug="${branch_name//\//-}"
                echo "agent-${branch_slug}-target"
                ;;
        esac
    done <<< "$worktree_rows"

    # Flush final entry.
    if [[ -n "$wt_path" ]]; then
        local wt_base
        wt_base="$(basename "$wt_path")"
        echo "agent-${wt_base#agent-}-target"
        echo "${wt_base}-target"
        echo "agent-worktree-${wt_base}-target"
    fi
}

# Capture the complete observation before allocating a report scratch file.
# A failed producer never becomes an empty protected-name set.
registered_names="$(collect_live_worktree_target_names)" || exit $?
KNOWN_NAMES_FILE="$(mktemp)"
trap 'rm -f "$KNOWN_NAMES_FILE"' EXIT
sort -u <<< "$registered_names" > "$KNOWN_NAMES_FILE"

# ── Find candidate dirs in /tmp ────────────────────────────────────────────────
#
# Only these naming patterns are touched — never arbitrary paths.
declare -a PATTERNS=(
    "/tmp/agent-*-target"
    "/tmp/wt-*-target"
)

# ── Main loop ─────────────────────────────────────────────────────────────────

found_any=0
total_observed_kb=0
unverified_count=0
skip_count=0
live_count=0

echo "=== clean-tmp-targets: advisory report; ownership and reclaimability unverified ==="
echo ""

for pattern in "${PATTERNS[@]}"; do
    # Use glob expansion; if nothing matches, the literal glob is returned.
    for candidate in $pattern; do
        [[ -d "$candidate" ]] || continue
        found_any=1

        name="$(basename "$candidate")"

        # Name match only: this does not identify a live resource consumer.
        if grep -qxF "$name" "$KNOWN_NAMES_FILE" 2>/dev/null; then
            echo "  NAMED   $candidate  (registered worktree name — ownership unverified)"
            live_count=$((live_count + 1))
            continue
        fi

        # Recency only: no owner or process conclusion follows.
        # find -mmin -N lists files modified within N minutes.
        recently_touched="$(find "$candidate" -maxdepth 1 -mmin "-${GRACE_MINUTES}" 2>/dev/null | head -1)"
        if [[ -n "$recently_touched" ]]; then
            echo "  RECENT  $candidate  (modified <${GRACE_MINUTES}min — skipping)"
            skip_count=$((skip_count + 1))
            continue
        fi

        # Observe size without claiming that the bytes are reclaimable.
        dir_kb=0
        dir_kb="$(du -sk "$candidate" 2>/dev/null | awk '{print $1}' || echo 0)"
        dir_human="$(du -sh "$candidate" 2>/dev/null | awk '{print $1}' || echo '?')"
        total_observed_kb=$((total_observed_kb + dir_kb))
        unverified_count=$((unverified_count + 1))
        echo "  UNVERIFIED  $candidate  ($dir_human observed; no ownership clearance)"
    done
done

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
if [[ "$found_any" -eq 0 ]]; then
    echo "No agent /tmp target dirs found (patterns: ${PATTERNS[*]})."
else
    total_human=""
    if [[ "$total_observed_kb" -gt 0 ]]; then
        total_human=" ($(( total_observed_kb / 1024 ))M observed)"
    fi

    echo "Registered name matches:        $live_count"
    echo "Recently-modified dirs skipped: $skip_count"
    echo "Unverified targets observed:    $unverified_count${total_human}"
    echo "Exact owner, consumer and evidence verification is required before any cleanup."
fi
