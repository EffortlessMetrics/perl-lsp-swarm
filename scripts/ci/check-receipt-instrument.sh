#!/usr/bin/env bash
# check-receipt-instrument.sh — M7 (#3849) / #3947 receipt-instrument gate.
#
# Reads one or more `cargo xtask gates --receipt` JSON files (schema:
# `xtask/src/tasks/gates.rs` `Receipt`) and asserts:
#
#   1. `metadata.git_sha` matches the expected head SHA -- the receipt is
#      bound to the commit it claims to prove, not stale/reused.
#   2. `metadata.timestamp` is a valid, non-future UTC timestamp less than
#      RECEIPT_MAX_AGE_SECONDS (default 3600s) old.
#   3. Every gate's `status` is pass/skip (fail/timeout/error rejects).
#   4. For any gate reporting `metrics.tests_total`: tests_total > 0,
#      tests_passed > 0, and tests_skipped < tests_total. This is the #3599
#      lesson made concrete: a bare `exit_code: 0` does not prove the
#      instrument exercised anything -- a silently-vacuous test run (matches
#      zero tests, or reports it "skipped" everything) also exits 0.
#   5. At least one gate across all receipts reports test metrics --
#      otherwise this check cannot confirm any test instrument ran at all.
#
# Canonical live producer: `cargo xtask gates --receipt` (what `just
# pr-fast` runs), default path `target/receipts/receipt.json`; CI merge-gate
# shards write `target/receipts/shards/<gate>.json` via --receipt-path.
#
# Note: the live `parse_test_metrics` in gates.rs does not currently
# populate `tests_skipped` from `cargo test` output (only
# tests_passed/tests_failed/tests_ignored) -- rule 4's tests_skipped check
# is kept for forward-compat with producers that do populate it, but today
# the operative discriminator is tests_total == 0 / tests_passed == 0.
#
# Usage: check-receipt-instrument.sh <expected-git-sha> <receipt.json> [...]
# Exit 0 = every receipt verified; Exit 2 = rejected (reason printed).

set -u

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <expected-git-sha> <receipt.json> [<receipt.json> ...]" >&2
  exit 2
fi

EXPECTED_SHA="$1"
shift

if ! command -v jq &>/dev/null; then
  echo "check-receipt-instrument: jq is required but not found in PATH." >&2
  exit 2
fi

MAX_AGE_SECONDS="${RECEIPT_MAX_AGE_SECONDS:-3600}"

MISSING=()
STALE=()
FAILED=()
TEST_METRICS_SEEN=0
FOUND_ANY=0

for RECEIPT in "$@"; do
  if [[ ! -f "${RECEIPT}" ]]; then
    MISSING+=("${RECEIPT} (not found)")
    continue
  fi
  FOUND_ANY=1
  LABEL="$(basename "${RECEIPT}")"

  if ! jq -e '(.metadata.git_sha | type) == "string" and (.metadata.timestamp | type) == "string" and (.gates | type) == "array"' "${RECEIPT}" &>/dev/null; then
    FAILED+=("${LABEL} (malformed: missing/invalid metadata.git_sha, metadata.timestamp, or gates array)")
    continue
  fi

  RECEIPT_SHA="$(jq -r '.metadata.git_sha' "${RECEIPT}")"
  if [[ "${RECEIPT_SHA}" != "${EXPECTED_SHA}" ]]; then
    STALE+=("${LABEL} (receipt is for ${RECEIPT_SHA:0:12}, expected ${EXPECTED_SHA:0:12})")
    continue
  fi

  TS_STR="$(jq -r '.metadata.timestamp' "${RECEIPT}")"
  if ! [[ "${TS_STR}" =~ Z$ || "${TS_STR}" =~ \+00:00$ ]]; then
    FAILED+=("${LABEL} (timestamp not UTC: ${TS_STR})")
    continue
  fi
  TS_EPOCH="$(date -d "${TS_STR}" -u +%s 2>/dev/null)" || {
    FAILED+=("${LABEL} (unparseable timestamp: ${TS_STR})")
    continue
  }
  NOW_EPOCH="$(date -u +%s)"
  if [[ "${TS_EPOCH}" -gt "${NOW_EPOCH}" ]]; then
    FAILED+=("${LABEL} (future timestamp: ${TS_STR})")
    continue
  fi
  AGE=$(( NOW_EPOCH - TS_EPOCH ))
  if [[ "${AGE}" -gt "${MAX_AGE_SECONDS}" ]]; then
    STALE+=("${LABEL} (${AGE}s old, max ${MAX_AGE_SECONDS}s)")
    continue
  fi

  GATE_COUNT="$(jq -r '.gates | length' "${RECEIPT}")"
  if [[ -z "${GATE_COUNT}" || "${GATE_COUNT}" == "0" ]]; then
    FAILED+=("${LABEL} (empty gates array -- no instrument recorded)")
    continue
  fi

  while IFS= read -r GATE_JSON; do
    GATE_NAME="$(jq -r '.gate_name // "<unnamed>"' <<<"${GATE_JSON}")"
    GATE_STATUS="$(jq -r '.status // "<none>"' <<<"${GATE_JSON}")"
    GATE_EXIT="$(jq -r '.exit_code // empty' <<<"${GATE_JSON}")"

    # Canonical statuses (xtask/src/tasks/gates.rs): pass|fail|skip|timeout|error.
    # Long forms accepted too (tolerance the repo's own CI scripts already use).
    case "${GATE_STATUS,,}" in
      pass|passed|skip|skipped) : ;;
      *)
        FAILED+=("${GATE_NAME} (status=${GATE_STATUS} exit=${GATE_EXIT:-n/a})")
        continue
        ;;
    esac

    TESTS_TOTAL="$(jq -r '.metrics.tests_total // empty' <<<"${GATE_JSON}")"
    if [[ -n "${TESTS_TOTAL}" ]]; then
      TEST_METRICS_SEEN=1
      TESTS_PASSED="$(jq -r '.metrics.tests_passed // 0' <<<"${GATE_JSON}")"
      TESTS_SKIPPED="$(jq -r '.metrics.tests_skipped // empty' <<<"${GATE_JSON}")"

      # The #3599 lesson: don't trust exit_code:0 alone -- a vacuous,
      # nothing-ran instrument also exits 0. Reject that shape here.
      if [[ "${TESTS_TOTAL}" -eq 0 ]]; then
        FAILED+=("${GATE_NAME} (vacuous: tests_total=0 -- instrument matched/ran nothing)")
      elif [[ -n "${TESTS_SKIPPED}" && "${TESTS_SKIPPED}" -ge "${TESTS_TOTAL}" ]]; then
        FAILED+=("${GATE_NAME} (vacuous: tests_skipped=${TESTS_SKIPPED} of tests_total=${TESTS_TOTAL})")
      elif [[ "${TESTS_PASSED}" -eq 0 ]]; then
        FAILED+=("${GATE_NAME} (vacuous: tests_passed=0 of tests_total=${TESTS_TOTAL})")
      fi
    fi
  done < <(jq -c '.gates[]' "${RECEIPT}" 2>/dev/null)
done

if [[ "${FOUND_ANY}" -eq 0 ]]; then
  MISSING+=("no receipt files found among: $*")
fi

if [[ ${#MISSING[@]} -eq 0 && ${#STALE[@]} -eq 0 && ${#FAILED[@]} -eq 0 && "${TEST_METRICS_SEEN}" -eq 0 ]]; then
  MISSING+=("test metrics (no gate reported tests_total -- cannot confirm a test instrument ran)")
fi

BLOCKED=0
if [[ ${#MISSING[@]} -gt 0 ]]; then
  echo "Receipt-instrument check failed: missing: ${MISSING[*]}"
  BLOCKED=1
fi
if [[ ${#STALE[@]} -gt 0 ]]; then
  echo "Receipt-instrument check failed: stale/mismatched: ${STALE[*]}"
  BLOCKED=1
fi
if [[ ${#FAILED[@]} -gt 0 ]]; then
  echo "Receipt-instrument check failed: failed/vacuous: ${FAILED[*]}"
  BLOCKED=1
fi

if [[ "${BLOCKED}" -eq 1 ]]; then
  exit 2
fi

echo "Receipt-instrument check passed: all gate receipts bound to ${EXPECTED_SHA:0:12} report a real (non-vacuous) run."
exit 0
