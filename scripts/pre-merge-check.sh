#!/usr/bin/env bash
# pre-merge-check.sh — native GitHub pre-merge guard
#
# Checks that a PR is ready to hand to GitHub's protected squash-merge path:
#   1. Not draft.
#   2. GitHub reports it mergeable and CLEAN or BEHIND.
#   3. Title contains an issue reference (#NNN).
#   4. Native review state has no unresolved thread, current change request,
#      deliberately pending reviewer, or silently resolved thread.
#
# This helper deliberately ignores stale-review timestamps, review-run receipts,
# material-claim digests, and exact-head review bookkeeping. GitHub protection remains
# authoritative for required checks, queue state, and final merge authorization.
#
# Usage:
#   scripts/pre-merge-check.sh <pr-number>
#
# Exit codes:
#   0  Local handoff checks passed
#   1  One or more checks failed

set -euo pipefail

PR="${1:?usage: $0 <pr-number>}"

json_read() {
    local filter="$1"
    printf '%s' "$PR_JSON" | jq -r "$filter" | tr -d '\r'
}

PR_JSON="$(gh pr view "$PR" --json isDraft,title,mergeable,mergeStateStatus)"

IS_DRAFT="$(json_read '.isDraft')"
MERGEABLE="$(json_read '.mergeable // empty')"
MERGE_STATE="$(json_read '.mergeStateStatus // empty')"
TITLE="$(json_read '.title')"

FAILED=0

if [[ "$IS_DRAFT" == "true" ]]; then
    echo "FAIL PR #$PR: still in draft state" >&2
    FAILED=1
fi

if [[ "$MERGEABLE" != "MERGEABLE" || ( "$MERGE_STATE" != "CLEAN" && "$MERGE_STATE" != "BEHIND" ) ]]; then
    echo "FAIL PR #$PR: native merge state is not queue-eligible (mergeable=$MERGEABLE, state=$MERGE_STATE)" >&2
    echo "     Resolve the actual conflict, review, required-check, or queue condition reported by GitHub" >&2
    FAILED=1
fi

if ! printf '%s' "$TITLE" | grep -qE '\(#[0-9]+\)'; then
    echo "FAIL PR #$PR: title missing issue reference — add (#NNN)" >&2
    echo "     Current title: $TITLE" >&2
    FAILED=1
fi

# Use the public semantic convergence command. The sibling -core script is only a
# compatibility fact collector; its exact-head fields and exit status are not policy.
TMP_DIR="$(mktemp -d)"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT

REVIEW_REPO="${GITHUB_REPOSITORY:-}"
if [[ -z "$REVIEW_REPO" ]]; then
    REVIEW_REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
fi

set +e
bash scripts/ci/check-pr-review-convergence "$PR" "$REVIEW_REPO" \
    >"$TMP_DIR/review.out" 2>"$TMP_DIR/review.err"
REVIEW_RC=$?
set -e

REVIEW_JSON="$(sed -n '/^{/,$p' "$TMP_DIR/review.out")"
if [[ "$REVIEW_RC" -ge 2 || -z "$REVIEW_JSON" ]] || ! jq -e . >/dev/null 2>&1 <<<"$REVIEW_JSON"; then
    echo "FAIL PR #$PR: native review facts are NOT_PROVEN" >&2
    cat "$TMP_DIR/review.err" >&2
    FAILED=1
else
    REVIEW_CURRENTNESS="$(jq -r '.review_currentness // empty' <<<"$REVIEW_JSON")"
    EXACT_HEAD_REQUIRED="$(jq -r '.exact_head_review_required // empty' <<<"$REVIEW_JSON")"

    if [[ "$REVIEW_CURRENTNESS" != "semantic_changed_seam" || "$EXACT_HEAD_REQUIRED" != "false" ]]; then
        echo "FAIL PR #$PR: review facts did not come from the semantic convergence authority" >&2
        FAILED=1
    else
        PENDING_REVIEWERS="$(jq -r '.pending_reviewers | length' <<<"$REVIEW_JSON")"
        CHANGE_REQUESTS="$(jq -r '.current_change_requests | length' <<<"$REVIEW_JSON")"
        UNRESOLVED_THREADS="$(jq -r '.unresolved_total // 0' <<<"$REVIEW_JSON")"
        SILENT_RESOLUTIONS="$(jq -r '.resolved_without_disposition // 0' <<<"$REVIEW_JSON")"

        if [[ "$PENDING_REVIEWERS" -gt 0 ]]; then
            echo "FAIL PR #$PR: $PENDING_REVIEWERS requested reviewer(s) still pending" >&2
            FAILED=1
        fi
        if [[ "$CHANGE_REQUESTS" -gt 0 ]]; then
            echo "FAIL PR #$PR: $CHANGE_REQUESTS current change-request review(s) remain" >&2
            FAILED=1
        fi
        if [[ "$UNRESOLVED_THREADS" -gt 0 ]]; then
            echo "FAIL PR #$PR: $UNRESOLVED_THREADS review thread(s) remain unresolved" >&2
            FAILED=1
        fi
        if [[ "$SILENT_RESOLUTIONS" -gt 0 ]]; then
            echo "FAIL PR #$PR: $SILENT_RESOLUTIONS resolved thread(s) lack a disposition reply" >&2
            FAILED=1
        fi
    fi
fi

if [[ "$FAILED" -eq 0 ]]; then
    echo "OK   PR #$PR: pre-merge handoff checks passed (ready, queue-eligible, issue-linked, review findings dispositioned)"
    exit 0
fi

exit 1
