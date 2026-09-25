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

classify_api_field() {
  # $1=annotations.json $2=steps.json $3=gap-receipt file ("" for none)
  # $4=output field -> value
  local out
  if [[ -n "$3" ]]; then
    out=$(bash "$CLASSIFIER" --api-evidence "$1" "$2" --gap-receipt "$3")
  else
    out=$(bash "$CLASSIFIER" --api-evidence "$1" "$2")
  fi
  printf '%s\n' "$out" | sed -n "s/^$4=//p" | head -1
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

# --- API-evidence mode (#16431) ------------------------------------------------
#
# The second classifier mode classifies from the lane job's check-run
# annotations and its steps state when the bounded log fetch fails or comes
# back without any teardown marker. The fixtures below are verbatim API
# shapes measured on the evicted runs named in #16431 (35444262230,
# 35501271465, 35507463908). The boundary is unchanged: only positive
# runner-teardown evidence arms infra-no-proof, a genuine gap receipt always
# outranks it, and unreadable/absent evidence fails closed.

if ! command -v jq >/dev/null 2>&1; then
  fail "api-evidence fixtures require the jq executable (classifier fails closed without it)"
  printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
  exit 1
fi

# Fixture: verbatim annotation of an exit-143 eviction (run 35507463908). All
# of that job's steps had concluded; the annotation is the only teardown
# evidence the API kept.
ANN_143="${WORK}/ann-exit143.json"
cat >"${ANN_143}" <<'EOF'
[
  {
    "path": ".github",
    "blob_href": "https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/f5781cc4423bac7759311827d9174f6f297ca100/.github",
    "start_line": 1,
    "start_column": null,
    "end_line": 1,
    "end_column": null,
    "annotation_level": "failure",
    "title": "",
    "message": "Process completed with exit code 143.",
    "raw_details": ""
  }
]
EOF

# Fixture: verbatim annotation of a lost-communication eviction (runs
# 35444262230 / 35501271465). That eviction class ends mid-cargo with zero
# in-log teardown markers, so only this API-side evidence can see it.
ANN_LOST="${WORK}/ann-lost-comm.json"
cat >"${ANN_LOST}" <<'EOF'
[
  {
    "path": ".github",
    "blob_href": "https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/f5781cc4423bac7759311827d9174f6f297ca100/.github",
    "start_line": 1,
    "start_column": null,
    "end_line": 1,
    "end_column": null,
    "annotation_level": "failure",
    "title": "",
    "message": "The hosted runner lost communication with the server. Anything in your workflow that terminates the runner process, starves it for CPU/Memory, or blocks its network access can cause this error.",
    "raw_details": ""
  }
]
EOF

ANN_EMPTY="${WORK}/ann-empty.json"
printf '[]' >"${ANN_EMPTY}"

# Fixture: a truncated annotation read (invalid JSON). Analogous to log
# mode's partial_read: unreadable evidence cannot prove absence of a genuine
# receipt, so it must never arm the infra path.
ANN_TRUNCATED="${WORK}/ann-truncated.json"
printf '[{"path":".github","annotation_level":"failure","message":"Process completed w' >"${ANN_TRUNCATED}"

# Fixture: steps state of the lost-communication eviction (run 35444262230):
# the lane failed while the step it was running never concluded.
STEPS_STUCK="${WORK}/steps-stuck.json"
cat >"${STEPS_STUCK}" <<'EOF'
{
  "conclusion": "failure",
  "steps": [
    {"name": "Set up job", "status": "completed", "conclusion": "success"},
    {"name": "Checkout", "status": "completed", "conclusion": "success"},
    {"name": "Generate PR evidence", "status": "completed", "conclusion": "success"},
    {"name": "Generate review guidance", "status": "in_progress", "conclusion": null},
    {"name": "Generate impacted evidence", "status": "pending", "conclusion": null},
    {"name": "Enforce new RIPR gap quality gate", "status": "pending", "conclusion": null},
    {"name": "Complete job", "status": "pending", "conclusion": null}
  ]
}
EOF

# Fixture: steps state of the exit-143 eviction (run 35507463908): every
# step, including the failing one, had concluded before the job failed.
STEPS_CONCLUDED="${WORK}/steps-concluded.json"
cat >"${STEPS_CONCLUDED}" <<'EOF'
{
  "conclusion": "failure",
  "steps": [
    {"name": "Set up job", "status": "completed", "conclusion": "success"},
    {"name": "Generate review guidance", "status": "completed", "conclusion": "failure"},
    {"name": "Generate impacted evidence", "status": "skipped", "conclusion": "skipped"},
    {"name": "Enforce new RIPR gap quality gate", "status": "skipped", "conclusion": "skipped"},
    {"name": "Complete job", "status": "completed", "conclusion": "success"}
  ]
}
EOF

# Fixture: a cancelled job with an unconcluded step. The steps-state rule is
# bounded to conclusion == failure, exactly as designed (#16431).
STEPS_CANCELLED="${WORK}/steps-cancelled.json"
cat >"${STEPS_CANCELLED}" <<'EOF'
{
  "conclusion": "cancelled",
  "steps": [
    {"name": "Generate review guidance", "status": "in_progress", "conclusion": null}
  ]
}
EOF

# Fixture: the lane job missing from the jobs listing (the API answers null
# for an unmatched first() projection).
STEPS_MISSING="${WORK}/steps-missing.json"
printf 'null' >"${STEPS_MISSING}"

# Fixture: a retrieved lane log holding the terminal receipt.
RECEIPT_LOG="${WORK}/receipt.log"
cat >"${RECEIPT_LOG}" <<'EOF'
2026-08-25T04:39:50Z quality gate failed; see receipt target/receipts/quality/quality-gate-ripr.json
EOF

# Fixture: a retrieved lane log with build progress but no markers and no
# receipt (the silence shape that hands classification to API evidence).
SILENT_LOG="${WORK}/silent.log"
cat >"${SILENT_LOG}" <<'EOF'
2026-08-26T00:10:01Z info: analyzing 1480 changed files
2026-08-26T00:12:30Z info: exposure pass 3/5
EOF

expect_eq "API DISCRIMINATOR core: exit-143 annotation classifies infra-no-proof" \
  "infra-no-proof" "$(classify_api_field "${ANN_143}" "${STEPS_CONCLUDED}" "" classification)"

expect_eq "API: lost-communication annotation classifies infra-no-proof" \
  "infra-no-proof" "$(classify_api_field "${ANN_LOST}" "${STEPS_CONCLUDED}" "" classification)"

expect_eq "API: steps stuck in_progress with no receipt arms without annotations" \
  "infra-no-proof" "$(classify_api_field "${ANN_EMPTY}" "${STEPS_STUCK}" "" classification)"

expect_eq "API: stuck-steps counter pins the evidence kind" \
  "1" "$(classify_api_field "${ANN_EMPTY}" "${STEPS_STUCK}" "" stuck_in_progress_steps)"

expect_eq "API: lost-communication counter pins the evidence kind" \
  "1" "$(classify_api_field "${ANN_LOST}" "${STEPS_STUCK}" "" lost_communication_matches)"

expect_eq "API PIN: no markers and all steps concluded stays ripr-failure" \
  "ripr-failure" "$(classify_api_field "${ANN_EMPTY}" "${STEPS_CONCLUDED}" "" classification)"

expect_eq "API PIN: cancelled job with an unconcluded step stays ripr-failure" \
  "ripr-failure" "$(classify_api_field "${ANN_EMPTY}" "${STEPS_CANCELLED}" "" classification)"

expect_eq "API PIN: lane job missing from the listing fails closed" \
  "ripr-failure" "$(classify_api_field "${ANN_EMPTY}" "${STEPS_MISSING}" "" classification)"

expect_eq "API PIN: missing job reports steps as unreadable" \
  "false" "$(classify_api_field "${ANN_EMPTY}" "${STEPS_MISSING}" "" steps_read)"

expect_eq "API PIN: truncated annotation read fails closed" \
  "ripr-failure" "$(classify_api_field "${ANN_TRUNCATED}" "${STEPS_CONCLUDED}" "" classification)"

expect_eq "API PIN: truncated annotation read reports unreadable evidence" \
  "false" "$(classify_api_field "${ANN_TRUNCATED}" "${STEPS_CONCLUDED}" "" annotations_read)"

expect_eq "API DISCRIMINATOR: a retrieved genuine receipt outranks teardown annotations" \
  "ripr-failure" "$(classify_api_field "${ANN_143}" "${STEPS_STUCK}" "${RECEIPT_LOG}" classification)"

expect_eq "API: receipt counter carries into api-evidence mode" \
  "1" "$(classify_api_field "${ANN_143}" "${STEPS_STUCK}" "${RECEIPT_LOG}" gap_receipt_matches)"

expect_eq "API: silent retrieved log without markers takes the api-evidence path" \
  "infra-no-proof" "$(classify_api_field "${ANN_LOST}" "${STEPS_CONCLUDED}" "${SILENT_LOG}" classification)"

expect_eq "API: missing annotations file is absent evidence, not positive evidence" \
  "ripr-failure" "$(classify_api_field "${WORK}/does-not-exist.json" "${STEPS_CONCLUDED}" "" classification)"

if bash "$CLASSIFIER" --api-evidence "${ANN_EMPTY}" >/dev/null 2>&1; then
  fail "incomplete api-evidence arguments must be a usage error"
else
  pass "incomplete api-evidence arguments are a usage error (exit 64)"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
