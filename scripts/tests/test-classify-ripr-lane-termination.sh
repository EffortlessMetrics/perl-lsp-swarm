#!/usr/bin/env bash
# Self-tests for the ripr lane-termination classifier and the bounded-retry
# decision table (#12563, #6807).
#
# Discriminating intent: a GENUINE ripr red ("quality gate failed; see
# receipt") must keep redding the gate even when the runner was torn down
# afterwards; ONLY positive runner-teardown evidence without such a receipt
# may take the infra-no-verdict / single-retry path. Feeding known-bad inputs
# that must NOT be classified as infra is the point of this suite.
#
# Run: bash scripts/tests/test-classify-ripr-lane-termination.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CLASSIFIER="${REPO_ROOT}/scripts/ci/classify-ripr-lane-termination"
DECIDER="${REPO_ROOT}/scripts/ci/ripr-bounded-retry"

PASS=0
FAIL=0
WORK=""

cleanup() {
  if [[ -n "${WORK:-}" && -d "${WORK}" ]]; then
    rm -rf "${WORK}"
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

expect_eq() {
  local label="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    pass "$label"
  else
    fail "$label (expected '${expected}', got '${actual}')"
  fi
}

decide_action_for() {
  # $1=lane_result $2=attempt $3=log file -> ACTION token
  bash "$DECIDER" --decide "$1" "$2" "$3" | sed -n 's/^ACTION=//p' | head -1
}

decide_verdict_for() {
  # $1=lane_result $2=attempt $3=log file -> RIPR_GATE_VERDICT token
  bash "$DECIDER" --decide "$1" "$2" "$3" | sed -n 's/^RIPR_GATE_VERDICT=//p' | head -1
}

classify_verdict_for() {
  # $1=log file -> classifier verdict token
  bash "$CLASSIFIER" "$1" | sed -n 's/^verdict=//p' | head -1
}

WORK="$(mktemp -d)"

# --- Fixtures -----------------------------------------------------------------

# Fixture: verbatim eviction signature recorded on #12277/#12563 (issue body).
EVICTED="${WORK}/evicted.log"
cat >"${EVICTED}" <<'EOF'
2026-08-25T02:46:40.1234567Z ##[group]Run cargo xtask ripr-pr --base origin/main --head HEAD --pr-head ""
2026-08-25T02:46:41Z info: cloning worktree ...
2026-08-25T02:47:51.928Z ##[error]The runner has received a shutdown signal. This can happen when the runner service is stopped, or a manually started runner is canceled.
2026-08-25T02:47:52.031Z ##[error]The operation was canceled.
2026-08-25T02:47:52.095Z Cleaning up orphan processes
EOF

# Fixture: alternate eviction rendering (exit 143 pair, no shutdown line).
SIGTERM_EVICTED="${WORK}/sigterm-evicted.log"
cat >"${SIGTERM_EVICTED}" <<'EOF'
2026-08-24T23:35:10Z ##[error]The operation was canceled.
2026-08-24T23:35:11Z ##[error]Process completed with exit code 143.
EOF

# Fixture: DISCRIMINATOR — genuine gap receipt printed before the teardown.
# The genuine failure must outrank the infra markers so the gate stays red.
REAL_FAILURE="${WORK}/real-failure.log"
cat >"${REAL_FAILURE}" <<'EOF'
2026-08-25T04:39:50Z thread 'main' panicked at xtask: quality gate failed; see receipt target/receipts/quality/quality-gate-ripr.json and summary target/receipts/quality/quality-gate-ripr.md
2026-08-25T04:39:51Z ##[error]Process completed with exit code 101.
2026-08-25T04:40:02Z ##[error]The runner has received a shutdown signal. This can happen when the runner service is stopped, or a manually started runner is canceled.
EOF

# Fixture: genuine failure with NO infra markers at all (classic red).
PLAIN_FAILURE="${WORK}/plain-failure.log"
cat >"${PLAIN_FAILURE}" <<'EOF'
2026-08-25T04:11:12Z quality gate failed; see receipt target/receipts/quality/quality-gate-ripr.json and summary target/receipts/quality/quality-gate-ripr.md
2026-08-25T04:11:13Z ##[error]Process completed with exit code 101.
EOF

# Fixture: lone manual/API cancellation rendering (no teardown evidence).
MANUAL_CANCEL="${WORK}/manual-cancel.log"
cat >"${MANUAL_CANCEL}" <<'EOF'
2026-08-26T01:02:03Z ##[error]The operation was canceled.
2026-08-26T01:02:03Z Cleaning up orphan processes
EOF

# Fixture: empty log (retrieval produced nothing).
EMPTY_LOG="${WORK}/empty.log"
: >"${EMPTY_LOG}"

# --- Classifier verdicts ------------------------------------------------------

expect_eq "eviction signature classifies as infra" \
  "infra-eviction-shutdown-signal" "$(classify_verdict_for "${EVICTED}")"

expect_eq "exit-143+cancelled pair classifies as infra variant" \
  "infra-eviction-sigterm-canceled" "$(classify_verdict_for "${SIGTERM_EVICTED}")"

expect_eq "genuine gap receipt outranks later teardown marker" \
  "source-gap-terminal" "$(classify_verdict_for "${REAL_FAILURE}")"

expect_eq "plain genuine failure is source-gap-terminal" \
  "source-gap-terminal" "$(classify_verdict_for "${PLAIN_FAILURE}")"

expect_eq "lone operation-canceled is not teardown evidence" \
  "no-terminal-receipt" "$(classify_verdict_for "${MANUAL_CANCEL}")"

expect_eq "empty log fails closed to no-terminal-receipt" \
  "no-terminal-receipt" "$(classify_verdict_for "${EMPTY_LOG}")"

if bash "$CLASSIFIER" "${WORK}/does-not-exist.log" >/dev/null 2>&1; then
  pass "missing log still exits cleanly"
else
  fail "missing log should not hard-fail the classifier"
fi

# --- Decision table -----------------------------------------------------------

expect_eq "attempt-1 eviction arms exactly-one retry" \
  "ARM_RETRY" "$(decide_action_for failure 1 "${EVICTED}")"

expect_eq "attempt-1 eviction verdict token is machine-checkable" \
  "infra-retry-requested" "$(decide_verdict_for failure 1 "${EVICTED}")"

expect_eq "attempt-2 eviction exhausts retry budget to NOT_PROVEN" \
  "NOT_PROVEN_INFRA" "$(decide_action_for failure 2 "${EVICTED}")"

expect_eq "attempt-2 exhaustion uses loud verdict token" \
  "not-proven-infra-retry-exhausted" "$(decide_verdict_for failure 2 "${EVICTED}")"

expect_eq "DISCRIMINATOR: real failure never retried despite teardown marker" \
  "RIPR_FAILURE" "$(decide_action_for failure 1 "${REAL_FAILURE}")"

expect_eq "real failure carries plain ripr-failure verdict" \
  "ripr-failure" "$(decide_verdict_for failure 1 "${REAL_FAILURE}")"

expect_eq "plain failure fails closed to ripr-failure" \
  "RIPR_FAILURE" "$(decide_action_for failure 1 "${PLAIN_FAILURE}")"

expect_eq "empty-log failure fails closed" \
  "FAIL_CLOSED" "$(decide_action_for failure 1 "${EMPTY_LOG}")"

expect_eq "human cancellation stays blocking without retry" \
  "CANCELLED_NO_VERDICT" "$(decide_action_for cancelled 1 "${MANUAL_CANCEL}")"

expect_eq "human cancellation verdict token" \
  "cancelled-no-verdict" "$(decide_verdict_for cancelled 1 "${MANUAL_CANCEL}")"

expect_eq "cancelled WITH teardown evidence retries once" \
  "ARM_RETRY" "$(decide_action_for cancelled 1 "${EVICTED}")"

expect_eq "successful lanes are not applicable" \
  "NOT_APPLICABLE" "$(decide_action_for success 1 "${EVICTED}")"

expect_eq "skipped lanes are not applicable" \
  "NOT_APPLICABLE" "$(decide_action_for skipped 3 "${EMPTY_LOG}")"

# Boundary documentation must be present in every decision for auditability.
if bash "$DECIDER" --decide failure 1 "${EVICTED}" | grep -q '^RIPR_GATE_DECISION boundary='; then
  pass "decision block documents its boundary in run-log output"
else
  fail "decision block must echo its boundary line"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
