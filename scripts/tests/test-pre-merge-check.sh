#!/usr/bin/env bash
# Fixture tests for scripts/pre-merge-check.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMPL="$SCRIPT_DIR/../pre-merge-check.sh"
CONVERGENCE_WRAPPER="$SCRIPT_DIR/../ci/check-pr-review-convergence"
SEMANTIC_CHECKER="$SCRIPT_DIR/../ci/check-pr-semantic-review-currentness.py"
STATUS_FIXTURES="$SCRIPT_DIR/test-pre-merge-status.py"
CURRENTNESS_DOC="$REPO_ROOT/docs/agents/REVIEW_CURRENTNESS.md"
AGENT_VERIFY="$REPO_ROOT/.agents/skills/verify-live-ci/SKILL.md"
CLAUDE_VERIFY="$REPO_ROOT/.claude/skills/verify-live-ci/SKILL.md"
PR_TEMPLATE="$REPO_ROOT/.github/PULL_REQUEST_TEMPLATE.md"
PASS_COUNT=0
FAIL_COUNT=0

for required in \
    "$IMPL" \
    "$CONVERGENCE_WRAPPER" \
    "$SEMANTIC_CHECKER" \
    "$STATUS_FIXTURES" \
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
    local tmpdir json semantic_class semantic_reason semantic_rc
    tmpdir="$(mktemp -d)"
    json="$1"
    semantic_class="${2:-REVIEW_CURRENT}"
    semantic_reason="${3:-fixture}"
    semantic_rc="${4:-0}"
    json="$(jq -c '. + {headRefOid:"fixture-head",baseRefName:"main"}' <<<"$json")"
    printf '%s' "$json" >"$tmpdir/pr.json"
    printf '%s' '[{"name":"Required A","state":"SUCCESS","link":"https://example.test/required"}]' >"$tmpdir/checks.json"
    printf '%s' '[{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"Required A"}]}}]' >"$tmpdir/rules.json"
    printf '%s' '{"contexts":[],"checks":[]}' >"$tmpdir/classic.json"
    printf '%s' '[{"name":"Required A","state":"SUCCESS","link":"https://example.test/required"}]' >"$tmpdir/required.json"
    cat > "$tmpdir/gh" <<EOF_MOCK
#!/usr/bin/env bash
case "\$*" in
    'repo view'*) printf '%s' 'test-owner/test-repo' ;;
    *'rules/branches/'*) cat '$tmpdir/rules.json' ;;
    *'protection/required_status_checks'*) cat '$tmpdir/classic.json' ;;
    *'--required'*) cat '$tmpdir/required.json' ;;
    *'pr checks'*)
        cat '$tmpdir/checks.json'
        if [[ -f '$tmpdir/checks-error' ]]; then
            echo 'API partial response' >&2
            exit 1
        fi
        ;;
    *) cat '$tmpdir/pr.json' ;;
esac
EOF_MOCK
    cat > "$tmpdir/semantic-currentness.py" <<EOF_SEMANTIC
#!/usr/bin/env python3
import json
print(json.dumps({"classification": "$semantic_class", "reason": "$semantic_reason"}))
raise SystemExit($semantic_rc)
EOF_SEMANTIC
    chmod +x "$tmpdir/gh" "$tmpdir/semantic-currentness.py"
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
        PRE_MERGE_ADVISORY_EVIDENCE="${PRE_MERGE_ADVISORY_EVIDENCE:-}" \
        SEMANTIC_CURRENTNESS_BIN="$mock_dir/semantic-currentness.py" \
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
        PRE_MERGE_ADVISORY_EVIDENCE="${PRE_MERGE_ADVISORY_EVIDENCE:-}" \
        SEMANTIC_CURRENTNESS_BIN="$mock_dir/semantic-currentness.py" \
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

test_clean_review_current_pr_passes() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -eq 0 ]] && pass "clean native plus semantic snapshot exits zero" || fail "clean snapshot failed"
}

test_behind_merge_state_passes() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BEHIND","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json")"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -eq 0 ]] && pass "behind native merge state exits zero" || fail "behind native merge state unexpectedly failed"
}

test_unstable_inherited_advisory_requires_evidence() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"UNSTABLE","title":"fix: launcher (#16939)"}'
    mock="$(make_mock_gh "$json")"
    jq '. += [{name:"PR Smoke",state:"FAILURE",link:"https://example.test/advisory"}]' "$mock/checks.json" >"$mock/updated.json"
    mv "$mock/updated.json" "$mock/checks.json"
    code="$(run_check "$mock")"
    if [[ "$code" -eq 0 ]]; then
        fail "UNSTABLE advisory without evidence unexpectedly passed"
    else
        pass "UNSTABLE advisory without evidence fails closed"
    fi
    printf '%s' '{"headRefOid":"fixture-head","advisories":[{"name":"PR Smoke","link":"https://example.test/advisory","classification":"inherited","discriminator":"same failing gate and signature at merge base","evidenceUrl":"https://example.test/advisory","mergeBaseRunUrl":"https://example.test/merge-base"}]}' >"$mock/evidence.json"
    PRE_MERGE_ADVISORY_EVIDENCE="$mock/evidence.json" code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -eq 0 ]] && pass "UNSTABLE inherited advisory with exact-head evidence passes" || fail "UNSTABLE inherited advisory with evidence failed"
}

test_partial_check_snapshot_fails_closed() {
    local mock json output
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"fix: thing (#16958)"}'
    mock="$(make_mock_gh "$json")"
    touch "$mock/checks-error"
    output="$(run_check_with_output "$mock")"
    cleanup "$mock"
    if grep -Fq 'gh pr checks did not return a reliable current snapshot' <<<"$output" &&
       grep -Fq 'EXIT:1' <<<"$output"; then
        pass "partial gh pr checks response fails closed"
    else
        fail "partial gh pr checks response unexpectedly passed"
    fi
}

test_semantic_not_proven_fails_even_when_native_facts_converge() {
    local mock json code output
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json" NOT_PROVEN material_content_change_after_review 1)"
    output="$(run_check_with_output "$mock")"
    cleanup "$mock"
    code="$(printf '%s\n' "$output" | sed -n 's/^EXIT://p')"
    if [[ "$code" -ne 0 ]] && grep -Fq 'substantive review is NOT_PROVEN' <<<"$output"; then
        pass "material post-review change requires focused re-review"
    else
        fail "semantic NOT_PROVEN was not enforced"
    fi
}

test_zero_or_generic_review_cannot_become_review_current() {
    local mock json code
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","title":"feat: add thing (#3321)"}'
    mock="$(make_mock_gh "$json" NOT_PROVEN no_substantive_review_currentness_marker 1)"
    code="$(run_check "$mock")"
    cleanup "$mock"
    [[ "$code" -ne 0 ]] && pass "generic or absent review cannot satisfy semantic currentness" || fail "generic review incorrectly passed"
}

test_public_wrappers_keep_authority_split() {
    local ok=true
    grep -Fq 'check-pr-review-convergence' "$IMPL" || ok=false
    grep -Fq 'check-pr-semantic-review-currentness.py' "$IMPL" || ok=false
    grep -Fq 'review_currentness: "NOT_PROVEN"' "$CONVERGENCE_WRAPPER" || ok=false
    grep -Fq 'semantic_currentness_required: true' "$CONVERGENCE_WRAPPER" || ok=false
    if grep -Fq 'review_currentness: "semantic_changed_seam"' "$CONVERGENCE_WRAPPER"; then
        ok=false
    fi
    [[ "$ok" == "true" ]] && pass "native facts and semantic review have separate authorities" || fail "review authority split drifted"
}

test_currentness_policy_surfaces() {
    local ok=true
    prose_has() {
        tr '\n' ' ' < "$1" | tr -s '[:space:]' ' ' | grep -Fq "$2"
    }

    prose_has "$CURRENTNESS_DOC" 'Rebase is an ordinary integration tool, not a freshness ceremony.' || ok=false
    prose_has "$CURRENTNESS_DOC" 'there is no mechanical one-rebase limit.' || ok=false
    prose_has "$AGENT_VERIFY" 'a completed result on an earlier PR head remains usable semantic evidence' || ok=false
    prose_has "$CLAUDE_VERIFY" 'a completed result on an earlier PR head remains usable semantic evidence' || ok=false
    prose_has "$PR_TEMPLATE" 'Do not describe hosted CI as an "exact-head proof authority."' || ok=false
    prose_has "$CURRENTNESS_DOC" 'Required live statuses remain head-bound integration facts.' || ok=false
    prose_has "$CURRENTNESS_DOC" 'is **pending**, not' || ok=false
    grep -Fq 'semantic-review:v1' "$SEMANTIC_CHECKER" || ok=false
    grep -Fq 'post-review change is not in a whitespace-insensitive prose file' "$SEMANTIC_CHECKER" || ok=false

    if prose_has "$CURRENTNESS_DOC" 'one rebase immediately before merge'; then
        ok=false
    fi
    if prose_has "$PR_TEMPLATE" 'optional one-time decision'; then
        ok=false
    fi

    [[ "$ok" == "true" ]] && pass "active policy rejects exact-head churn without fabricating review currentness" || fail "currentness policy surfaces drifted"
}

test_error_messages_are_native() {
    local mock json output
    json='{"isDraft":false,"mergeable":"MERGEABLE","mergeStateStatus":"BLOCKED","title":"feat: no issue ref"}'
    mock="$(make_mock_gh "$json")"
    output="$(run_check_with_output "$mock")"
    cleanup "$mock"
    if grep -qi "native merge state" <<<"$output" && grep -qi "title" <<<"$output"; then
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
test_clean_review_current_pr_passes
test_behind_merge_state_passes
test_unstable_inherited_advisory_requires_evidence
test_partial_check_snapshot_fails_closed
test_semantic_not_proven_fails_even_when_native_facts_converge
test_zero_or_generic_review_cannot_become_review_current
test_public_wrappers_keep_authority_split
test_currentness_policy_surfaces
test_error_messages_are_native
test_no_pr_number_fails
if python3 "$STATUS_FIXTURES"; then
    pass "required and advisory status fixtures"
else
    fail "required and advisory status fixtures"
fi
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

[[ "$FAIL_COUNT" -eq 0 ]]
