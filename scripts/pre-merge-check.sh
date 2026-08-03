#!/usr/bin/env bash
# pre-merge-check.sh — Native GitHub pre-merge guard
#
# Checks that a PR is safe to hand to GitHub's protected merge path:
#   1. Not in draft state
#   2. GitHub reports the PR as mergeable and its merge state as CLEAN or
#      BEHIND. A conflict-free behind candidate may enter the protected queue;
#      unrelated base movement does not require a refresh.
#   3. Title contains an issue reference (#NNN)
#
# The merge state is read once. This helper does not poll, synthesize a
# readiness label, or treat labels as authority.
#   4. Canonical review convergence passes against the current head
#
# Usage:
#   scripts/pre-merge-check.sh <pr-number>
#
# Exit codes:
#   0  All checks passed — hand off to the protected merge path
#   1  One or more checks failed — do not merge this PR

set -euo pipefail

PR="${1:?usage: $0 <pr-number>}"

json_read() {
    local filter="$1"
    printf '%s' "$PR_JSON" | jq -r "$filter" | tr -d '\r'
}

# ── Fetch one native GitHub snapshot ─────────────────────────────────────────

PR_JSON="$(gh pr view "$PR" --json isDraft,title,mergeable,mergeStateStatus)"

IS_DRAFT="$(json_read '.isDraft')"
MERGEABLE="$(json_read '.mergeable // empty')"
MERGE_STATE="$(json_read '.mergeStateStatus // empty')"
TITLE="$(json_read '.title')"

# ── Run checks ────────────────────────────────────────────────────────────────

FAILED=0

if [[ "$IS_DRAFT" == "true" ]]; then
    echo "FAIL PR #$PR: still in draft state — mark it ready for review first" >&2
    FAILED=1
fi

# Check 2: Native mergeability and queue-eligible state
if [[ "$MERGEABLE" != "MERGEABLE" || ( "$MERGE_STATE" != "CLEAN" && "$MERGE_STATE" != "BEHIND" ) ]]; then
    echo "FAIL PR #$PR: native merge state is not queue-eligible (mergeable=$MERGEABLE, state=$MERGE_STATE)" >&2
    echo "     Resolve the reported conflict, review, or required-check state before merging" >&2
    FAILED=1
fi

if ! printf '%s' "$TITLE" | grep -qE '\(#[0-9]+\)'; then
    echo "FAIL PR #$PR: title missing issue reference — add (#NNN) to the PR title" >&2
    echo "     Current title: $TITLE" >&2
    FAILED=1
fi

# Check 4: Canonical review convergence, with lifecycle labels excluded
REVIEW_EXIT=0
REVIEW_REPO="${GITHUB_REPOSITORY:-}"
if [[ -z "$REVIEW_REPO" ]]; then
    REVIEW_REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
fi
REVIEW_OUTPUT="$(REVIEW_PROTOCOL_ENFORCE=1 bash scripts/ci/check-pr-review-convergence "$PR" "$REVIEW_REPO" 2>&1)" || REVIEW_EXIT=$?
if [[ "$REVIEW_EXIT" -ne 0 ]]; then
    echo "FAIL PR #$PR: canonical review convergence did not pass" >&2
    printf '%s\n' "$REVIEW_OUTPUT" >&2
    FAILED=1
fi

# ── Result ────────────────────────────────────────────────────────────────────

if [[ "$FAILED" -eq 0 ]]; then
    echo "OK   PR #$PR: pre-merge checks passed (not draft, native merge state queue-eligible, issue ref in title, review converged)"
    exit 0
fi

exit 1
