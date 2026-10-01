#!/usr/bin/env bash
# pre-merge-check.sh — native GitHub pre-merge guard
#
# Checks that a PR is ready to hand to GitHub's protected squash-merge path:
#   1. Not draft.
#   2. GitHub reports it mergeable, with live required checks green and
#      every advisory red classified when its summary is UNSTABLE.
#   3. Title contains an issue reference (#NNN).
#   4. Native review requests/findings have converged.
#   5. One subject-bound substantive review is REVIEW_CURRENT.
#
# GitHub protection remains authoritative for queue state and final merge
# authorization. This helper never rebases or mutates a branch
# to manufacture exact-head evidence.
set -euo pipefail

PR="${1:?usage: $0 <pr-number>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_CONVERGENCE="$SCRIPT_DIR/ci/check-pr-review-convergence"
SEMANTIC_CURRENTNESS_BIN="${SEMANTIC_CURRENTNESS_BIN:-$SCRIPT_DIR/ci/check-pr-semantic-review-currentness.py}"

json_read() {
    local filter="$1"
    printf '%s' "$PR_JSON" | jq -r "$filter" | tr -d '\r'
}

PR_JSON="$(gh pr view "$PR" --json isDraft,title,mergeable,mergeStateStatus,headRefOid,baseRefName)"

IS_DRAFT="$(json_read '.isDraft')"
MERGEABLE="$(json_read '.mergeable // empty')"
MERGE_STATE="$(json_read '.mergeStateStatus // empty')"
TITLE="$(json_read '.title')"

FAILED=0

if [[ "$IS_DRAFT" == "true" ]]; then
    echo "FAIL PR #$PR: still in draft state" >&2
    FAILED=1
fi

if [[ "$MERGEABLE" != "MERGEABLE" || ( "$MERGE_STATE" != "CLEAN" && "$MERGE_STATE" != "BEHIND" && "$MERGE_STATE" != "UNSTABLE" ) ]]; then
    echo "FAIL PR #$PR: native merge state is not queue-eligible (mergeable=$MERGEABLE, state=$MERGE_STATE)" >&2
    echo "     Resolve the actual conflict, review, required-check, or queue condition reported by GitHub" >&2
    FAILED=1
fi

if ! printf '%s' "$TITLE" | grep -qE '\(#[0-9]+\)'; then
    echo "FAIL PR #$PR: title missing issue reference — add (#NNN)" >&2
    echo "     Current title: $TITLE" >&2
    FAILED=1
fi

TMP_DIR="$(mktemp -d)"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT

read_pr_checks() {
    local output="$1" rc=0
    shift
    gh pr checks "$PR" "$@" >"$output" 2>"$output.err" || rc=$?
    if [[ "$rc" -ne 0 && "$rc" -ne 1 && "$rc" -ne 8 ]] ||
       [[ -s "$output.err" ]] || ! jq -e 'type == "array"' "$output" >/dev/null 2>&1; then
        echo "FAIL PR #$PR: gh pr checks did not return a reliable current snapshot (exit=$rc)" >&2
        cat "$output.err" >&2
        return 1
    fi
}

read_classic_policy() {
    local endpoint="repos/$REVIEW_REPO/branches/$BASE_BRANCH/protection/required_status_checks"
    local rc=0
    gh api "$endpoint" >"$TMP_DIR/classic.json" 2>"$TMP_DIR/classic.err" || rc=$?
    if [[ "$rc" -eq 0 ]]; then
        if jq -e 'type == "object" and (.contexts | type == "array") and (.checks | type == "array")' \
            "$TMP_DIR/classic.json" >/dev/null 2>&1; then
            return 0
        fi
        echo "FAIL PR #$PR: classic protection response is malformed" >&2
        return 1
    fi
    if grep -Fq '(HTTP 404)' "$TMP_DIR/classic.err" &&
       jq -e 'type == "object" and .status == "404" and .message == "Branch not protected"' \
           "$TMP_DIR/classic.json" >/dev/null 2>&1; then
        printf '%s' '{"contexts":[],"checks":[]}' >"$TMP_DIR/classic.json"
        return 0
    fi
    echo "FAIL PR #$PR: classic protection is unreadable (exit=$rc)" >&2
    cat "$TMP_DIR/classic.err" >&2
    return 1
}

REVIEW_REPO="${GITHUB_REPOSITORY:-}"
if [[ -z "$REVIEW_REPO" ]]; then
    REVIEW_REPO="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
fi

BASE_BRANCH="$(json_read '.baseRefName')"
if [[ -z "$BASE_BRANCH" || "$BASE_BRANCH" == "null" ]]; then
    echo "FAIL PR #$PR: base branch is NOT_PROVEN" >&2
    FAILED=1
else
    if ! gh api "repos/$REVIEW_REPO/rules/branches/$BASE_BRANCH" >"$TMP_DIR/rules.json" ||
       ! read_classic_policy; then
        echo "FAIL PR #$PR: live required-check policy is NOT_PROVEN" >&2
        FAILED=1
    else
        # gh pr checks may use exit 1 for red or 8 for pending; malformed or
        # partially failed API reads cannot stand in for a complete snapshot.
        if ! read_pr_checks "$TMP_DIR/required.json" --required --json name,state,link ||
           ! read_pr_checks "$TMP_DIR/checks.json" --json name,state,bucket,link,workflow; then
            FAILED=1
        else
            printf '%s' "$PR_JSON" >"$TMP_DIR/pr.json"
            if [[ -n "${PRE_MERGE_ADVISORY_EVIDENCE:-}" ]]; then
                if ! jq -n --slurpfile pr "$TMP_DIR/pr.json" --slurpfile rules "$TMP_DIR/rules.json" \
                    --slurpfile classic "$TMP_DIR/classic.json" --slurpfile required_checks "$TMP_DIR/required.json" \
                    --slurpfile checks "$TMP_DIR/checks.json" \
                    --slurpfile evidence "$PRE_MERGE_ADVISORY_EVIDENCE" \
                    '{pr:$pr[0],rules:$rules[0],classic:$classic[0],required_checks:$required_checks[0],checks:$checks[0],evidence:$evidence[0]}' \
                    | python3 "$SCRIPT_DIR/ci/classify-pre-merge-status.py"; then
                    FAILED=1
                fi
            elif ! jq -n --slurpfile pr "$TMP_DIR/pr.json" --slurpfile rules "$TMP_DIR/rules.json" \
                --slurpfile classic "$TMP_DIR/classic.json" --slurpfile required_checks "$TMP_DIR/required.json" \
                --slurpfile checks "$TMP_DIR/checks.json" \
                '{pr:$pr[0],rules:$rules[0],classic:$classic[0],required_checks:$required_checks[0],checks:$checks[0]}' \
                | python3 "$SCRIPT_DIR/ci/classify-pre-merge-status.py"; then
                FAILED=1
            fi
        fi
    fi
fi

set +e
bash "$NATIVE_CONVERGENCE" "$PR" "$REVIEW_REPO" \
    >"$TMP_DIR/native-review.out" 2>"$TMP_DIR/native-review.err"
NATIVE_RC=$?
set -e

NATIVE_JSON="$(sed -n '/^{/,$p' "$TMP_DIR/native-review.out")"
if [[ "$NATIVE_RC" -ge 2 || -z "$NATIVE_JSON" ]] || ! jq -e . >/dev/null 2>&1 <<<"$NATIVE_JSON"; then
    echo "FAIL PR #$PR: native review facts are NOT_PROVEN" >&2
    cat "$TMP_DIR/native-review.err" >&2
    FAILED=1
else
    NATIVE_CONVERGED="$(jq -r '.native_review_facts_converged // false' <<<"$NATIVE_JSON")"
    WRAPPER_CURRENTNESS="$(jq -r '.review_currentness // empty' <<<"$NATIVE_JSON")"
    SEMANTIC_REQUIRED="$(jq -r '.semantic_currentness_required // false' <<<"$NATIVE_JSON")"
    if [[ "$NATIVE_CONVERGED" != "true" ]]; then
        echo "FAIL PR #$PR: native review requests or finding threads have not converged" >&2
        FAILED=1
    fi
    if [[ "$WRAPPER_CURRENTNESS" != "NOT_PROVEN" || "$SEMANTIC_REQUIRED" != "true" ]]; then
        echo "FAIL PR #$PR: native convergence wrapper improperly claimed semantic currentness" >&2
        FAILED=1
    fi
fi

set +e
python3 "$SEMANTIC_CURRENTNESS_BIN" "$PR" "$REVIEW_REPO" \
    >"$TMP_DIR/semantic-review.out" 2>"$TMP_DIR/semantic-review.err"
SEMANTIC_RC=$?
set -e
SEMANTIC_JSON="$(tail -n 1 "$TMP_DIR/semantic-review.out")"
if [[ "$SEMANTIC_RC" -ge 2 || -z "$SEMANTIC_JSON" ]] || ! jq -e . >/dev/null 2>&1 <<<"$SEMANTIC_JSON"; then
    echo "FAIL PR #$PR: semantic review currentness is NOT_PROVEN" >&2
    cat "$TMP_DIR/semantic-review.err" >&2
    FAILED=1
else
    SEMANTIC_CLASS="$(jq -r '.classification // "NOT_PROVEN"' <<<"$SEMANTIC_JSON")"
    if [[ "$SEMANTIC_RC" -ne 0 || "$SEMANTIC_CLASS" != "REVIEW_CURRENT" ]]; then
        SEMANTIC_REASON="$(jq -r '.reason // "unknown"' <<<"$SEMANTIC_JSON")"
        echo "FAIL PR #$PR: substantive review is $SEMANTIC_CLASS ($SEMANTIC_REASON)" >&2
        FAILED=1
    fi
fi

if [[ "$FAILED" -eq 0 ]]; then
    echo "OK   PR #$PR: pre-merge handoff checks passed (queue-eligible, issue-linked, native findings dispositioned, semantic review current)"
    exit 0
fi

exit 1
