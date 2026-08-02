#!/usr/bin/env bash
# Fixture tests for scripts/pre-merge-check.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMPL="$SCRIPT_DIR/../pre-merge-check.sh"
PASS_COUNT=0
FAIL_COUNT=0

if [[ ! -f "$IMPL" ]]; then
    echo "ERROR: pre-merge-check.sh not found at $IMPL"
    exit 1
fi

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

make_mock_gh() {
    local tmpdir json
    tmpdir="$(mktemp -d)"
    json="$1"
    cat > "$tmpdir/gh" <<EOF
#!/usr/bin/env bash
printf '%s' '$json'
EOF
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
    local code=0
    PATH="$mock_dir:$PATH" bash "$IMPL" "$pr_number" >/dev/null 2>&1 || code=$?
    echo "$code"
}

run_check_with_output() {
    local mock_dir="$1"
    local pr_number="${2:-42}"
    local code=0
    local output
    output="$(PATH="$mock_dir:$PATH" bash "$IMPL" "$pr_number" 2>&1)" || code=$?
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
test_error_messages_are_native
test_no_pr_number_fails
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="

[[ "$FAIL_COUNT" -eq 0 ]]
