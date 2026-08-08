#!/usr/bin/env bash
# Offline regression suite for semantic review convergence and retired writers.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$SCRIPT_DIR/../ci/check-pr-review-convergence"
STATE_SCRIPT="$SCRIPT_DIR/../reviews/state"
WRITER_SCRIPT="$SCRIPT_DIR/../reviews/run"
FIXTURES_ROOT="$SCRIPT_DIR/../ci/fixtures/convergence"
PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

for required in "$SCRIPT" "$STATE_SCRIPT" "$WRITER_SCRIPT"; do
    [[ -f "$required" ]] || { echo "ERROR: missing $required"; exit 1; }
done
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq required"; exit 1; }

RUN_EXIT=0
RUN_STDOUT=""
STATE_EXIT=0
STATE_STDOUT=""

run_case() {
    local case_name="$1"
    local fixture_dir="$FIXTURES_ROOT/$case_name"
    [[ -d "$fixture_dir" ]] || { echo "ERROR: missing fixture $fixture_dir" >&2; exit 1; }
    RUN_EXIT=0
    RUN_STDOUT="$(CONVERGENCE_TEST_FIXTURE_DIR="$fixture_dir" bash "$SCRIPT" 9999 test-owner/test-repo 2>/dev/null)" || RUN_EXIT=$?
}

run_case_enforce() {
    local case_name="$1"
    local fixture_dir="$FIXTURES_ROOT/$case_name"
    [[ -d "$fixture_dir" ]] || { echo "ERROR: missing fixture $fixture_dir" >&2; exit 1; }
    RUN_EXIT=0
    RUN_STDOUT="$(CONVERGENCE_TEST_FIXTURE_DIR="$fixture_dir" REVIEW_PROTOCOL_ENFORCE=1 bash "$SCRIPT" 9999 test-owner/test-repo 2>/dev/null)" || RUN_EXIT=$?
}

run_state_case() {
    local case_name="$1"
    local fixture_dir="$FIXTURES_ROOT/$case_name"
    STATE_EXIT=0
    STATE_STDOUT="$(CONVERGENCE_TEST_FIXTURE_DIR="$fixture_dir" bash "$STATE_SCRIPT" 9999 test-owner/test-repo 2>/dev/null)" || STATE_EXIT=$?
}

json_blob() {
    printf '%s' "$1" | sed -n '/^{/,$p'
}

expect_case() {
    local fixture="$1" expected_exit="$2" jq_expr="$3" description="$4"
    run_case "$fixture"
    local json
    json="$(json_blob "$RUN_STDOUT")"
    if [[ "$RUN_EXIT" -eq "$expected_exit" ]] && jq -e "$jq_expr" >/dev/null <<<"$json"; then
        pass "$description"
    else
        fail "$description — exit=$RUN_EXIT output=$RUN_STDOUT"
    fi
}

expect_case "outdated-unresolved-blocks" 1 \
    '.converged == false and .unresolved_outdated == 1 and .formal_review.classification == "FINDINGS_OPEN"' \
    "outdated unresolved thread blocks"

expect_case "active-unresolved-blocks" 1 \
    '.converged == false and .unresolved_active == 1 and .formal_review.classification == "FINDINGS_OPEN"' \
    "active unresolved thread blocks"

expect_case "resolved-without-disposition-blocks" 1 \
    '.converged == false and .resolved_without_disposition >= 1' \
    "resolved-to-clear thread blocks"

expect_case "resolved-with-disposition-ok" 0 \
    '.converged == true and .resolved_without_disposition == 0' \
    "evidence-backed disposition permits convergence"

expect_case "pending-independent-review-blocks" 1 \
    '.converged == false and .formal_review.classification == "PENDING"' \
    "native review request is durable pending state"

expect_case "current-change-request-blocks" 1 \
    '.converged == false and .formal_review.classification == "FINDINGS_OPEN"' \
    "submitted change request blocks"

expect_case "all-resolved-converges" 0 \
    '.converged == true and .formal_review.classification == "NOT_APPLICABLE" and .exact_head_review_required == false' \
    "no native review requirement converges without a receipt"

expect_case "formal-review-current" 0 \
    '.converged == true and .formal_review.classification == "REVIEWED" and .material_claim_receipt_required == false' \
    "submitted useful review is classified REVIEWED"

# The fixture contains a human review submitted on an earlier candidate head.
# The earlier conclusion remains usable; changed-seam review decides whether a
# focused refresh is needed, not the SHA mismatch by itself.
expect_case "formal-review-stale" 0 \
    '.converged == true and .formal_review.classification == "REVIEWED" and (.legacy_receipt_observations.stale_reviews | length) >= 1' \
    "earlier-head human review remains usable"

# Receipt-only compatibility state cannot create liveness or a blocker.
expect_case "review-run-receipt-still-running-blocks" 0 \
    '.converged == true and .exact_head_review_required == false' \
    "running review receipt is ignored as lifecycle bookkeeping"

expect_case "receipt-bound-to-older-head-blocks" 0 \
    '.converged == true and .material_claim_receipt_required == false' \
    "older-head claim receipt does not block"

# An inherited legacy enforcement variable cannot reactivate the retired axes.
run_case_enforce "formal-review-stale"
if [[ "$RUN_EXIT" -eq 0 ]] && jq -e '.converged == true and .exact_head_review_required == false' >/dev/null <<<"$(json_blob "$RUN_STDOUT")"; then
    pass "REVIEW_PROTOCOL_ENFORCE cannot restore exact-head review authority"
else
    fail "legacy enforce flag restored receipt authority — exit=$RUN_EXIT output=$RUN_STDOUT"
fi

# Provider/instrument failure remains explicit NOT_PROVEN.
code=0
out="$(CONVERGENCE_TEST_FIXTURE_DIR="$FIXTURES_ROOT/does-not-exist" bash "$SCRIPT" 9999 test-owner/test-repo 2>/dev/null)" || code=$?
if [[ "$code" -eq 2 ]] && jq -e '.formal_review.classification == "NOT_PROVEN"' >/dev/null <<<"$(json_blob "$out")"; then
    pass "missing provider facts report NOT_PROVEN"
else
    fail "missing provider facts should be NOT_PROVEN — exit=$code output=$out"
fi

# Malformed numeric collector facts must also reach callers as structured
# NOT_PROVEN output. Copy the wrapper beside a fake core so this exercises the
# production command-substitution boundary rather than source-text assertions.
TMP_NUMERIC="$(mktemp -d)"
cp "$SCRIPT" "$TMP_NUMERIC/check-pr-review-convergence"
cat >"$TMP_NUMERIC/check-pr-review-convergence-core" <<'EOF'
#!/usr/bin/env bash
cat <<'JSON'
{
  "is_draft": false,
  "headRefOid": "fixture-head",
  "pending_reviewers": [],
  "review_decision": "",
  "current_change_requests": [],
  "unresolved_active": "not-a-number",
  "unresolved_outdated": 0,
  "unresolved_total": 0,
  "resolved_without_disposition": 0,
  "human_review_count": 0,
  "dismissed_human_review_count": 0
}
JSON
EOF
numeric_exit=0
numeric_output="$(bash "$TMP_NUMERIC/check-pr-review-convergence" 9999 test-owner/test-repo 2>/dev/null)" || numeric_exit=$?
if [[ "$numeric_exit" -eq 2 ]] && jq -e '.formal_review.classification == "NOT_PROVEN" and .formal_review.reason == "invalid_numeric_review_fact"' >/dev/null <<<"$(json_blob "$numeric_output")"; then
    pass "malformed numeric fact reports structured NOT_PROVEN"
else
    fail "malformed numeric fact should preserve structured verdict — exit=$numeric_exit output=$numeric_output"
fi
rm -rf "$TMP_NUMERIC"

# The state helper projects native facts, not FIXED_HEAD/VERIFIED_HEAD stages.
run_state_case "current-change-request-blocks"
if [[ "$STATE_EXIT" -eq 0 ]] && jq -e '.state == "FINDINGS_OPEN" and .exact_head_review_required == false' >/dev/null <<<"$STATE_STDOUT"; then
    pass "state helper reports findings without an exact-head lifecycle"
else
    fail "state helper findings projection — exit=$STATE_EXIT output=$STATE_STDOUT"
fi

run_state_case "formal-review-stale"
if [[ "$STATE_EXIT" -eq 0 ]] && jq -e '.state == "REVIEWED" and .review_currentness == "semantic_changed_seam"' >/dev/null <<<"$STATE_STDOUT"; then
    pass "state helper preserves useful earlier-head review"
else
    fail "state helper should report REVIEWED — exit=$STATE_EXIT output=$STATE_STDOUT"
fi

# The retired writer must fail before discovering or invoking gh. This catches
# warm sessions and stale callers, not only current skill text.
TMP_WRITER="$(mktemp -d)"
trap 'rm -rf "$TMP_WRITER"' EXIT
cat >"$TMP_WRITER/gh" <<'EOF'
#!/usr/bin/env bash
echo invoked >>"${GH_SENTINEL:?}"
exit 99
EOF
chmod +x "$TMP_WRITER/gh"
export GH_SENTINEL="$TMP_WRITER/gh-invocations"

for subcommand in review-start review-done verify; do
    writer_exit=0
    writer_output="$(PATH="$TMP_WRITER:$PATH" bash "$WRITER_SCRIPT" "$subcommand" --pr 42 --repo owner/repo --dry-run 2>&1)" || writer_exit=$?
    if [[ "$writer_exit" -eq 2 ]] && [[ "$writer_output" == *"RETIRED"* ]] && [[ ! -e "$GH_SENTINEL" ]]; then
        pass "$subcommand fails closed before any GitHub call"
    else
        fail "$subcommand writer boundary — exit=$writer_exit output=$writer_output gh_called=$([[ -e "$GH_SENTINEL" ]] && echo yes || echo no)"
    fi
done

echo ""
echo "=== Results: $PASS_COUNT passed, $FAIL_COUNT failed ==="
[[ "$FAIL_COUNT" -eq 0 ]]
