#!/usr/bin/env bash
# pre-merge-check.sh — native GitHub pre-merge guard
#
# Checks that a PR is ready to hand to GitHub's protected squash-merge path:
#   1. Not draft.
#   2. GitHub reports it mergeable and CLEAN or BEHIND.
#   3. Title contains an issue reference (#NNN).
#
# GitHub branch protection and rulesets remain authoritative for required checks,
# unresolved conversations, change requests, and queue state. This helper does not
# impose an additional exact-head review receipt or claim-digest protocol.
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

if [[ "$FAILED" -eq 0 ]]; then
    echo "OK   PR #$PR: pre-merge handoff checks passed (ready, queue-eligible, issue-linked)"
    exit 0
fi

exit 1
