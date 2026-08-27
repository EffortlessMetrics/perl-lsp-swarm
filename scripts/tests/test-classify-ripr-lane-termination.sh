#!/usr/bin/env bash
# Self-tests for the ripr lane-termination classifier (#12563, complementing
# #12771's bounded auto-retry).
#
# Discriminating intent: a GENUINE ripr red ("quality gate failed; see
# receipt") must keep redding the required gate even when the runner was torn
# down afterwards — it must never be classified infra-no-proof or auto-retried.
# ONLY positive runner-teardown evidence without such a receipt may take the
# infra-no-proof path.
#
# Run: bash scripts/tests/test-classify-ripr-lane-termination.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CLASSIFIER="${REPO_ROOT}/scripts/ci/classify-ripr-lane-termination"

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

classify_field() {
  # $1=log file $2=output field -> value
  bash "$CLASSIFIER" "$1" | sed -n "s/^$2=//p" | head -1
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

# Fixture: alternate eviction rendering (exit 143 pair / lone marker shapes).
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

# Fixture: build progress only, then silence (unknown kill, no evidence).
SILENT_KILL="${WORK}/silent-kill.log"
cat >"${SILENT_KILL}" <<'EOF'
2026-08-26T00:10:01Z info: analyzing 1480 changed files
2026-08-26T00:12:30Z info: exposure pass 3/5
EOF

# Fixture: empty log (retrieval produced nothing usable).
EMPTY_LOG="${WORK}/empty.log"
: >"${EMPTY_LOG}"

# --- Classifier verdicts ------------------------------------------------------

expect_eq "eviction signature classifies infra-no-proof" \
  "infra-no-proof" "$(classify_field "${EVICTED}" classification)"

expect_eq "exit-143/cancelled rendering also classifies infra-no-proof" \
  "infra-no-proof" "$(classify_field "${SIGTERM_EVICTED}" classification)"

expect_eq "DISCRIMINATOR: genuine gap receipt outranks later teardown marker" \
  "ripr-failure" "$(classify_field "${REAL_FAILURE}" classification)"

expect_eq "plain genuine failure is ripr-failure" \
  "ripr-failure" "$(classify_field "${PLAIN_FAILURE}" classification)"

expect_eq "silence fails closed to ripr-failure" \
  "ripr-failure" "$(classify_field "${SILENT_KILL}" classification)"

expect_eq "empty log fails closed to ripr-failure" \
  "ripr-failure" "$(classify_field "${EMPTY_LOG}" classification)"

if bash "$CLASSIFIER" "${WORK}/does-not-exist.log" >/dev/null 2>&1; then
  pass "missing log still exits cleanly with ripr-failure"
else
  fail "missing log should not hard-fail the classifier"
fi

expect_eq "missing log classifies ripr-failure" \
  "ripr-failure" "$(bash "$CLASSIFIER" "${WORK}/does-not-exist.log" | sed -n 's/^classification=//p' | head -1)"

# Evidence counters make every application auditable.
expect_eq "eviction fixture counts teardown markers" \
  "1" "$(classify_field "${EVICTED}" shutdown_signal_matches)"
expect_eq "gap receipt counter present on discriminator" \
  "1" "$(classify_field "${REAL_FAILURE}" gap_receipt_matches)"

# Partial reads are flagged AND fail closed even when a teardown marker sits
# in the scanned prefix: an unseen suffix could still hold a gap receipt
# (#12563 review P2).
PARTIAL="${WORK}/partial.log"
cat >"${PARTIAL}" <<'EOF'
padding line 1
padding line 2
The runner has received a shutdown signal.
EOF
OUT_PARTIAL=$(bash "$CLASSIFIER" "${PARTIAL}" 16)
if [[ "$OUT_PARTIAL" == *"partial_read=true"* ]]; then
  pass "truncated scan flags partial_read=true"
else
  fail "truncated scan must flag partial_read=true"
fi
expect_eq "P2 DISCRIMINATOR: capped scan fails closed despite prefix marker" \
  "ripr-failure" "$(printf '%s\n' "$OUT_PARTIAL" | sed -n 's/^classification=//p' | head -1)"

# Verdict alias and boundary documentation are part of the output contract.
OUT_EVICT=$(bash "$CLASSIFIER" "${EVICTED}")
if [[ "$OUT_EVICT" == *"decision_boundary="* ]]; then
  pass "classifier documents its decision boundary in output"
else
  fail "classifier must emit its decision_boundary line"
fi

# --- Review round-2 boundary pins ---------------------------------------------
#
# Pins for the two scenarios raised in review (lone cancellation, wall-clock
# timeout). The adopted taxonomy (#6807/#12563, inheriting #12771's marker
# set) counts API cancellation as positive runner-teardown evidence, so both
# classify infra-no-proof. This is deliberately bounded, not proof-hiding:
#   - the gate handles lane_result==cancelled upstream with a blocking error,
#     so user/API cancels never reach this classifier;
#   - a misclassified termination costs at most ONE same-head rerun whose own
#     trusted outcome arbitrates the SHA — the classification arms the retry,
#     it is never the verdict;
#   - a deterministic timeout reproduces on attempt 2 and lands on the loud
#     not-proven-infra-retry-exhausted bound; no further automation runs.

LONE_CANCEL="${WORK}/lone-cancel.log"
cat >"${LONE_CANCEL}" <<'EOF'
2026-08-26T08:12:04Z ##[error]The operation was canceled.
EOF
expect_eq "PIN: lone API-cancellation is teardown evidence under adopted taxonomy" \
  "infra-no-proof" "$(classify_field "${LONE_CANCEL}" classification)"

TIMEOUT_KILL="${WORK}/timeout-kill.log"
cat >"${TIMEOUT_KILL}" <<'EOF'
2026-08-26T02:30:00Z info: ripr exposure pass 4/5
2026-08-26T06:00:00Z ##[error]The job running on runner cx43-03 has exceeded the maximum execution time of 210 minutes.
2026-08-26T06:00:01Z ##[error]The operation was canceled.
EOF
expect_eq "PIN: wall-clock timeout without receipt reruns once then lands on the loud bound" \
  "infra-no-proof" "$(classify_field "${TIMEOUT_KILL}" classification)"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
