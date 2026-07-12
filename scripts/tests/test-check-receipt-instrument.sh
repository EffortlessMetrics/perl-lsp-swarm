#!/usr/bin/env bash
# Red/green regression test for scripts/ci/check-receipt-instrument.sh
# (M7, #3849 / #3947).
#
# Proves the exit criterion concretely:
#  - a zero-selection/zero-test receipt (exit_code:0, but
#    tests_skipped>=tests_total or tests_passed==0) is REJECTED (a bare
#    exit_code:0 does not prove any test actually ran)
#  - a genuine receipt (tests_passed>0, tests_skipped==0) bound to the
#    expected head SHA is ACCEPTED
#  - a receipt bound to a different (stale) head SHA is REJECTED
#  - HONEST LIMITATION, documented not "fixed": a receipt shaped like the
#    actual #3599 early-return-Ok mode (total=1, passed=1, skipped=0) is
#    NOT caught -- counts cannot distinguish it from a genuine pass. See the
#    "DOCUMENTED GAP" case below and scripts/ci/check-receipt-instrument.sh's
#    header comment for why.
#  - NARROW SCOPE: a receipt where the test gate is clean but a SEPARATE
#    tooling gate (fmt-shaped, exit_code=127) is failed/absent is ACCEPTED --
#    this check only inspects test-metrics-bearing gates (see the
#    "lightweight-advisory-runner shape" case below).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CHECK_SCRIPT="${REPO_ROOT}/scripts/ci/check-receipt-instrument.sh"

PASS=0
FAIL=0
TMPDIR_BASE=""

cleanup() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
}
trap cleanup EXIT

pass() {
  printf 'PASS %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf 'FAIL %s\n' "$1"
  FAIL=$((FAIL + 1))
}

assert_exit_zero() {
  local label="$1" code="$2"
  if [[ "$code" -eq 0 ]]; then
    pass "$label"
  else
    fail "$label (expected exit 0, got ${code})"
  fi
}

assert_exit_nonzero() {
  local label="$1" code="$2"
  if [[ "$code" -ne 0 ]]; then
    pass "$label (exit ${code} as expected)"
  else
    fail "$label (expected non-zero exit, got 0)"
  fi
}

now_utc() { date -u +"%Y-%m-%dT%H:%M:%SZ"; }

# gate_receipt <path> <sha> <ts> <tests_total> <tests_passed> <tests_skipped>
write_receipt() {
  local path="$1" sha="$2" ts="$3" total="$4" passed="$5" skipped="$6"
  jq -n \
    --arg sha "$sha" --arg ts "$ts" \
    --argjson total "$total" --argjson passed "$passed" --argjson skipped "$skipped" \
    '{
      schema_version: "1.0.0",
      metadata: {git_sha: $sha, timestamp: $ts},
      gates: [
        {
          gate_name: "unit_scoped",
          tier: "pr_fast",
          status: "pass",
          duration_ms: 500,
          command: "cargo test --locked --lib",
          exit_code: 0,
          metrics: {tests_total: $total, tests_passed: $passed, tests_skipped: $skipped}
        }
      ],
      summary: {total_gates: 1, passed: 1, failed: 0, skipped: 0, total_duration_ms: 500, overall_status: "pass"}
    }' > "$path"
}

# write_receipt_with_tooling_failure: a receipt where the test-class gate
# (tests_total/passed/skipped as given) is clean, but a SEPARATE tooling gate
# (fmt-shaped: no metrics, status=fail, exit_code=127 -- "command not found")
# is present and failed. Reproduces the lightweight-advisory-runner shape:
# `just`/doc-check tooling isn't installed, so always-on gates like fmt
# legitimately fail with exit 127, unrelated to whether the test instrument
# ran for real.
write_receipt_with_tooling_failure() {
  local path="$1" sha="$2" ts="$3" total="$4" passed="$5" skipped="$6"
  jq -n \
    --arg sha "$sha" --arg ts "$ts" \
    --argjson total "$total" --argjson passed "$passed" --argjson skipped "$skipped" \
    '{
      schema_version: "1.0.0",
      metadata: {git_sha: $sha, timestamp: $ts},
      gates: [
        {
          gate_name: "fmt",
          tier: "pr_fast",
          status: "fail",
          duration_ms: 5,
          command: "cargo xtask fmt --check",
          exit_code: 127
        },
        {
          gate_name: "unit_scoped",
          tier: "pr_fast",
          status: "pass",
          duration_ms: 500,
          command: "cargo test --locked --lib",
          exit_code: 0,
          metrics: {tests_total: $total, tests_passed: $passed, tests_skipped: $skipped}
        }
      ],
      summary: {total_gates: 2, passed: 1, failed: 1, skipped: 0, total_duration_ms: 505, overall_status: "fail"}
    }' > "$path"
}

echo "=== check-receipt-instrument test suite ==="
echo ""

if [[ ! -f "$CHECK_SCRIPT" ]]; then
  echo "ERROR: check-receipt-instrument.sh not found at ${CHECK_SCRIPT}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
SHA="0123456789abcdef0123456789abcdef01234567"
OTHER_SHA="fedcba9876543210fedcba9876543210fedcba98"

# ─── RED: zero-selection/zero-test receipt (exit_code:0 but
# tests_skipped>=tests_total, and separately tests_passed==0) must be
# rejected, not trusted on exit_code alone.
VACUOUS_DIR="${TMPDIR_BASE}/vacuous"
mkdir -p "$VACUOUS_DIR"
write_receipt "${VACUOUS_DIR}/receipt.json" "$SHA" "$(now_utc)" 5 0 5
code=0
OUT="$(bash "$CHECK_SCRIPT" "$SHA" "${VACUOUS_DIR}/receipt.json" 2>&1)" || code=$?
assert_exit_nonzero "RED: rejects vacuous receipt (tests_skipped>=tests_total, tests_passed==0)" "$code"
if printf '%s' "$OUT" | grep -q "vacuous"; then
  pass "RED: rejection message names the vacuous condition"
else
  fail "RED: rejection message names the vacuous condition (output: ${OUT})"
fi

# ─── GREEN: genuine receipt (tests actually ran and passed), bound to the
# expected head SHA, must be accepted.
GENUINE_DIR="${TMPDIR_BASE}/genuine"
mkdir -p "$GENUINE_DIR"
write_receipt "${GENUINE_DIR}/receipt.json" "$SHA" "$(now_utc)" 5 5 0
code=0
bash "$CHECK_SCRIPT" "$SHA" "${GENUINE_DIR}/receipt.json" > "${GENUINE_DIR}/out.txt" 2>&1 || code=$?
assert_exit_zero "GREEN: accepts genuine receipt (tests_passed>0, tests_skipped==0)" "$code"

# ─── HONEST LIMITATION (documents, does not "fix"): the actual #3599 shape
# -- a test function that does an early `return Ok(())` / `return;` before
# its real assertions -- is counted by cargo as PASSED. tests_total=1,
# tests_passed=1, tests_skipped absent/0. This is indistinguishable from a
# genuine pass at the count level, so this check CANNOT catch it and is not
# claimed to. This fixture exists so no future reader assumes counts close
# the #3599 hole: #3599's own fix used a fail-loud, per-suite `assert!()`
# inside the test harness itself (the only place that can tell "ran its
# content" from "silently no-op'd while still returning Ok"), and that
# per-suite pattern remains the actual mitigation for this mode.
THREE599_DIR="${TMPDIR_BASE}/3599-shape"
mkdir -p "$THREE599_DIR"
write_receipt "${THREE599_DIR}/receipt.json" "$SHA" "$(now_utc)" 1 1 0
code=0
bash "$CHECK_SCRIPT" "$SHA" "${THREE599_DIR}/receipt.json" > "${THREE599_DIR}/out.txt" 2>&1 || code=$?
assert_exit_zero "DOCUMENTED GAP: the #3599 early-return-Ok shape (total=1,passed=1,skipped=0) is NOT caught by counts -- passes here by design" "$code"

# ─── GREEN (lightweight-advisory-runner shape): a receipt where the
# test-class gate is genuinely clean but a SEPARATE tooling gate (fmt-shaped,
# exit_code=127, no metrics) is failed/absent must still be ACCEPTED -- this
# check is scoped to test-metrics-bearing gates only and must not fail
# because unrelated tooling wasn't installed in a lightweight CI runner.
TOOLING_FAIL_DIR="${TMPDIR_BASE}/tooling-fail"
mkdir -p "$TOOLING_FAIL_DIR"
write_receipt_with_tooling_failure "${TOOLING_FAIL_DIR}/receipt.json" "$SHA" "$(now_utc)" 5 5 0
code=0
bash "$CHECK_SCRIPT" "$SHA" "${TOOLING_FAIL_DIR}/receipt.json" > "${TOOLING_FAIL_DIR}/out.txt" 2>&1 || code=$?
assert_exit_zero "GREEN: a failed tooling gate (fmt, exit 127) alongside a clean test gate does NOT block this check" "$code"

# ─── GREEN (stale head): a receipt bound to a DIFFERENT commit than expected
# must be rejected, even though its own gate content is genuine/non-vacuous.
STALE_DIR="${TMPDIR_BASE}/stale"
mkdir -p "$STALE_DIR"
write_receipt "${STALE_DIR}/receipt.json" "$OTHER_SHA" "$(now_utc)" 5 5 0
code=0
OUT="$(bash "$CHECK_SCRIPT" "$SHA" "${STALE_DIR}/receipt.json" 2>&1)" || code=$?
assert_exit_nonzero "GREEN: rejects a receipt bound to a different (stale) head SHA" "$code"
if printf '%s' "$OUT" | grep -q "expected ${SHA:0:12}"; then
  pass "GREEN: stale-head rejection names the expected SHA"
else
  fail "GREEN: stale-head rejection names the expected SHA (output: ${OUT})"
fi

# ─── Hardening: missing receipt file entirely.
MISSING_DIR="${TMPDIR_BASE}/missing"
mkdir -p "$MISSING_DIR"
code=0
bash "$CHECK_SCRIPT" "$SHA" "${MISSING_DIR}/does-not-exist.json" > "${MISSING_DIR}/out.txt" 2>&1 || code=$?
assert_exit_nonzero "hardening: rejects when the receipt file does not exist" "$code"

# ─── Hardening: a stale (>1h old) timestamp is rejected even with a matching
# SHA and genuine test counts.
STALE_TS_DIR="${TMPDIR_BASE}/stale-ts"
mkdir -p "$STALE_TS_DIR"
OLD_TS="$(date -u -d '@'"$(( $(date -u +%s) - 7200 ))" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || true)"
if [[ -n "$OLD_TS" ]]; then
  write_receipt "${STALE_TS_DIR}/receipt.json" "$SHA" "$OLD_TS" 5 5 0
  code=0
  bash "$CHECK_SCRIPT" "$SHA" "${STALE_TS_DIR}/receipt.json" > "${STALE_TS_DIR}/out.txt" 2>&1 || code=$?
  assert_exit_nonzero "hardening: rejects a receipt older than the freshness window" "$code"
else
  echo "SKIP hardening: could not compute a 2h-old timestamp on this platform"
fi

# ─── Hardening: a gate with metrics absent from every receipt (no test
# instrument reported at all) must be rejected as "cannot confirm ran".
NO_METRICS_DIR="${TMPDIR_BASE}/no-metrics"
mkdir -p "$NO_METRICS_DIR"
jq -n --arg sha "$SHA" --arg ts "$(now_utc)" '{
  schema_version: "1.0.0",
  metadata: {git_sha: $sha, timestamp: $ts},
  gates: [{gate_name: "clippy_scoped", tier: "pr_fast", status: "pass", duration_ms: 100, command: "cargo clippy --lib", exit_code: 0}],
  summary: {total_gates: 1, passed: 1, failed: 0, skipped: 0, total_duration_ms: 100, overall_status: "pass"}
}' > "${NO_METRICS_DIR}/receipt.json"
code=0
OUT="$(bash "$CHECK_SCRIPT" "$SHA" "${NO_METRICS_DIR}/receipt.json" 2>&1)" || code=$?
assert_exit_nonzero "hardening: rejects when no gate reports test metrics at all" "$code"
if printf '%s' "$OUT" | grep -q "test metrics"; then
  pass "hardening: no-test-metrics rejection names the gap"
else
  fail "hardening: no-test-metrics rejection names the gap (output: ${OUT})"
fi

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
