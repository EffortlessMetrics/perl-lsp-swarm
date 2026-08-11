#!/usr/bin/env bash
# Fixture tests for scripts/pre-merge-check.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMPL="$SCRIPT_DIR/../pre-merge-check.sh"
CONVERGENCE_WRAPPER="$SCRIPT_DIR/../ci/check-pr-review-convergence"
CURRENTNESS_DOC="$REPO_ROOT/docs/agents/REVIEW_CURRENTNESS.md"
AGENT_VERIFY="$REPO_ROOT/.agents/skills/verify-live-ci/SKILL.md"
CLAUDE_VERIFY="$REPO_ROOT/.claude/skills/verify-live-ci/SKILL.md"
PR_TEMPLATE="$REPO_ROOT/.github/PULL_REQUEST_TEMPLATE.md"
PASS_COUNT=0
FAIL_COUNT=0

for required in \
    "$IMPL" \
    "$CONVERGENCE_WRAPPER" \
    "$CURRENTNESS_DOC" \
    "$AGENT_VERIFY" \
    "$CLAUDE_VERIFY" \
    "$PR_TEMPLATE"; do
    if [[ ! -f "$required" ]]; then
        echo "ERROR: required currentness surface not found at $required"
        exit 1
    fi
done

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

make_mock_gh() {
    local tmpdir json
    tmpdir="$(mktemp -d)"
    json="$1"
    cat > "$tmpdir/gh" <<EOF_MOCK
#!/usr/bin/env bash
if [[ "\$*" == *"repo view"* ]]; then
    printf '%s' 'test-owner/test-repo'
else
    printf '%s' '$json'
fi
EOF_MOCK
    chmod +x "$tmpdir/gh"
    echo "$tmpdir"
}

cleanup() {
    local dir
    for dir in "$@"; do
        [[ -d "$dir" ]] && rm -rf "$dir"
    done
}

run_check() {
    local mock_dir="$1"
    local pr_number="${2:-42}"
    local fixture="${3:-all-resolved-converges}"
    local code=0
    PATH="$mock_dir:$PATH" \
        CONVERGENCE_TEST_FIXTURE_DIR="$SCRIPT_DIR/../ci/fixtures/convergence/$fixture" \
        bash "$IMPL" "$pr_number" >/dev/null 2>&1 || code=$?
    echo "$code"
}

run_check_with_output() {
    local mock_dir="$1"
    local pr_number="${2:-42}"
    local fixture="${3:-all-resolved-converges}"
    local code=0
    local output
    output="$(PATH="$mock_dir:$PATH" \
        CONVERGENCE_TEST_FIXTURE_DIR="$SCRIPT_DIR/../ci/fixtures/convergence/$fixture" \
        bash "$IMPL" "$pr_number" 2>&1)" || code=$?
    echo "EXIT:$code"
    echo "$output"
}

test_draft_pr_fails() {
    local mock json code
    json='{"isDraft":true,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -ne 0 ]] && pass "draft PR exits non-zero" || fail "draft PR unexpectedly passed"
}

test_blocked_merge_state_fails() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BLOCKED","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -ne 0 ]] && pass "blocked native merge state exits non-zero" || fail "blocked native merge state unexpectedly passed"
}

test_conflicting_pr_fails() {
    local mock json code
    json='{"isDraft":false,"mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -ne 0 ]] && pass "conflicting PR exits non-zero" || fail "conflicting PR unexpectedly passed"
}

test_missing_issue_ref_fails() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"feat: add thing without issue ref"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -ne 0 ]] && pass "missing issue ref exits non-zero" || fail "missing issue ref unexpectedly passed"
}

test_clean_pr_passes() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -eq 0 ]] && pass "clean native snapshot exits zero" || fail "clean native snapshot failed"
}

test_behind_merge_state_passes() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BEHIND","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -eq 0 ]] && pass "behind native merge state exits zero" || fail "behind native merge state unexpectedly failed"
}

test_earlier_head_review_passes() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BEHIND","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock" 42 formal-review-stale)"
    cleanup "$mock"
    [[ "$code" -eq 0 ]] && pass "earlier-head review remains usable on a behind PR" || fail "earlier-head review incorrectly forced refresh"
}

test_public_semantic_wrapper_is_authority() {
    if grep -Fq 'bash scripts/ci/check-pr-review-convergence-core' "$IMPL"; then
        fail "pre-merge check calls the legacy exact-head collector directly"
    elif grep -Fq 'bash scripts/ci/check-pr-review-convergence ' "$IMPL"; then
        pass "pre-merge check calls the public semantic convergence wrapper"
    else
        fail "pre-merge check has no recognizable semantic convergence call"
    fi
}

test_currentness_policy_surfaces() {
    local ok=true

    grep -Fq 'Rebase is an ordinary integration tool, not a freshness ceremony.' "$CURRENTNESS_DOC" || ok=false
    grep -Fq 'Its main accepted use is while resolving an actual merge conflict.' "$CURRENTNESS_DOC" || ok=false
    grep -Fq 'there is no mechanical one-rebase limit.' "$CURRENTNESS_DOC" || ok=false
    grep -Fq 'rebase is acceptable when resolving an actual conflict' "$AGENT_VERIFY" || ok=false
    grep -Fq 'rebase is acceptable when resolving an actual conflict' "$CLAUDE_VERIFY" || ok=false
    grep -Fq 'Rebasing is ordinary integration work when it solves a concrete problem.' "$PR_TEMPLATE" || ok=false
    grep -Fq 'There is no one-rebase limit' "$PR_TEMPLATE" || ok=false
    grep -Fq 'a completed result on an earlier PR head remains usable semantic evidence' "$AGENT_VERIFY" || ok=false
    grep -Fq 'a completed result on an earlier PR head remains usable semantic evidence' "$CLAUDE_VERIFY" || ok=false
    grep -Fq 'Do not describe hosted CI as an "exact-head proof authority."' "$PR_TEMPLATE" || ok=false
    grep -Fq 'This is the only policy-bearing review-convergence entrypoint.' "$CONVERGENCE_WRAPPER" || ok=false

    if grep -Fq 'Success on an older candidate is stale evidence, not current green.' "$AGENT_VERIFY" "$CLAUDE_VERIFY"; then
        ok=false
    fi
    if grep -Fq 'one rebase immediately before merge' "$CURRENTNESS_DOC"; then
        ok=false
    fi
    if grep -Fq 'one optional late rebase, once' "$AGENT_VERIFY" "$CLAUDE_VERIFY"; then
        ok=false
    fi
    if grep -Fq 'optional one-time decision' "$PR_TEMPLATE"; then
        ok=false
    fi

    if [[ "$ok" == "true" ]]; then
        pass "active policy surfaces reject exact-head churn and treat rebase as integration work"
    else
        fail "currentness policy surfaces drifted toward exact-head churn or a one-rebase rule"
    fi
}

test_error_messages_are_native() {
    local mock json output
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BLOCKED","title":"feat: no issue ref"}'
    mock="$(make_mock_gh "$json")"
    output="$(run_check_with_output "$mock")"
    cleanup "$mock"
    if echo "$output" | grep -qi "native merge state" && echo "$output" | grep -qi "title"; then
        pass "failure output names native state and title"
    else
        fail "failure output does not identify the native failures"
    fi
}

test_no_pr_number_fails() {
    local code=0
    bash "$IMPL" >/dev/null 2>&1 || code=$?
    [[ "$code" -ne 0 ]] && pass "missing PR number exits non-zero" || fail "missing PR number unexpectedly passed"
}

echo "=== native pre-merge-check test suite ==="
test_draft_pr_fails
test_blocked_merge_state_fails
test_conflicting_pr_fails
test_missing_issue_ref_fails
test_clean_pr_passes
test_behind_merge_state_passes
test_earlier_head_review_passes
test_public_semantic_wrapper_is_authority
test_currentness_policy_surfaces
test_error_messages_are_native
test_no_pr_number_fails
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

[[ "$FAIL_COUNT" -eq 0 ]]
