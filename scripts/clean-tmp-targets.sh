#!/usr/bin/env bash
# scripts/clean-tmp-targets.sh
#
# Advisory inspection of agent /tmp build-target directories (#17594).
#
# DELETION RETIRED: --prune and APPLY=1 refuse; naming and shallow mtime do not
# establish resource ownership or safe reclamation (#11671 owns exact reclaim).
#
# Usage:
#   bash scripts/clean-tmp-targets.sh            # advisory report (dry-run)
#   bash scripts/clean-tmp-targets.sh --dry-run  # same as default
#   bash scripts/clean-tmp-targets.sh --prune    # refused (exit 78)
#   APPLY=1 bash scripts/clean-tmp-targets.sh    # refused (exit 78)
#
# Contract:
#   * only lists dirs matching /tmp/agent-*-target or /tmp/wt-*-target;
#   * cross-checks basenames against live git worktree observations when Git
#     succeeds — failed or missing Git observations refuse before listing;
#   * never deletes; reports are not reclaim clearance.

set -euo pipefail

REFUSE_DELETION='REFUSING: temporary /tmp target pruning is retired; naming and age do not prove ownership — request an exact root-owned cleanup proposal (#11671)'

# Destructive entrypoints refuse before any discovery or later flag handling.
if [[ "${APPLY:-0}" == "1" ]]; then
    echo "$REFUSE_DELETION" >&2
    exit 78
fi
for arg in "$@"; do
    case "$arg" in
        --prune)
            echo "$REFUSE_DELETION" >&2
            exit 78
            ;;
    esac
done

# ── Configuration ─────────────────────────────────────────────────────────────

# Minutes within which a directory is considered "recently touched" (informational).
GRACE_MINUTES="${GRACE_MINUTES:-5}"

# Parse remaining flags.
for arg in "$@"; do
    case "$arg" in
        --dry-run) ;;
        --help|-h)
            grep '^#' "$0" | sed 's/^# //' | sed 's/^#//'
            exit 0
            ;;
        --prune) ;;
        *)
            echo "Unknown flag: $arg" >&2
            echo "Usage: $0 [--dry-run]" >&2
            exit 1
            ;;
    esac
done

# ── Discover live worktree paths ───────────────────────────────────────────────

collect_live_worktree_target_names() {
    local repo_root wt_path branch_ref

    if ! repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"; then
        echo "REFUSING: cannot resolve git repository root for worktree observation" >&2
        return 75
    fi

    local porcelain
    if ! porcelain="$(git -C "$repo_root" worktree list --porcelain 2>/dev/null)"; then
        echo "REFUSING: git worktree list failed; cannot derive live target names" >&2
        return 75
    fi

    wt_path=""
    branch_ref=""
    while IFS= read -r line; do
        case "$line" in
            "worktree "*)
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
                local branch_name="${branch_ref#refs/heads/}"
                local branch_slug="${branch_name//\//-}"
                echo "agent-${branch_slug}-target"
                ;;
        esac
    done <<< "$porcelain"

    if [[ -n "$wt_path" ]]; then
        local wt_base
        wt_base="$(basename "$wt_path")"
        echo "agent-${wt_base#agent-}-target"
        echo "${wt_base}-target"
        echo "agent-worktree-${wt_base}-target"
    fi
}

SAFE_NAMES_FILE="$(mktemp)"
trap 'rm -f "$SAFE_NAMES_FILE"' EXIT

worktree_names=""
if ! worktree_names="$(collect_live_worktree_target_names)"; then
    exit 75
fi
printf '%s\n' "$worktree_names" | sort -u > "$SAFE_NAMES_FILE"

# ── Find candidate dirs in /tmp ────────────────────────────────────────────────

declare -a PATTERNS=(
    "/tmp/agent-*-target"
    "/tmp/wt-*-target"
)

# ── Main loop ─────────────────────────────────────────────────────────────────

found_any=0
total_candidate_kb=0
candidate_count=0
skip_count=0
live_count=0

echo "=== clean-tmp-targets: ADVISORY (no deletion; not reclaim clearance) ==="
echo ""

for pattern in "${PATTERNS[@]}"; do
    for candidate in $pattern; do
        [[ -d "$candidate" ]] || continue
        found_any=1

        name="$(basename "$candidate")"

        if grep -qxF "$name" "$SAFE_NAMES_FILE" 2>/dev/null; then
            echo "  LIVE    $candidate  (basename matches a registered worktree observation)"
            live_count=$((live_count + 1))
            continue
        fi

        recently_touched="$(find "$candidate" -maxdepth 1 -mmin "-${GRACE_MINUTES}" 2>/dev/null | head -1)"
        if [[ -n "$recently_touched" ]]; then
            echo "  RECENT  $candidate  (modified <${GRACE_MINUTES}min — informational only)"
            skip_count=$((skip_count + 1))
            continue
        fi

        dir_kb=0
        dir_kb="$(du -sk "$candidate" 2>/dev/null | awk '{print $1}' || echo 0)"
        dir_human="$(du -sh "$candidate" 2>/dev/null | awk '{print $1}' || echo '?')"
        total_candidate_kb=$((total_candidate_kb + dir_kb))
        candidate_count=$((candidate_count + 1))

        echo "  CANDIDATE  $candidate  ($dir_human; age/name only — not reclaim clearance)"
    done
done

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
if [[ "$found_any" -eq 0 ]]; then
    echo "No agent /tmp target dirs found (patterns: ${PATTERNS[*]})."
else
    total_human=""
    if [[ "$total_candidate_kb" -gt 0 ]]; then
        total_human=" ($(( total_candidate_kb / 1024 ))M in age/name-selected candidates)"
    fi

    echo "Live worktree name matches:     $live_count"
    echo "Recently-modified (informational): $skip_count"
    echo "Age/name-selected candidates:   $candidate_count${total_human}"
fi

echo ""
echo "clean-tmp-targets: advisory report only — no ownership or reclaimability proof."
echo "Request an exact root-owned cleanup proposal (#11671); do not infer deletion from this listing."
