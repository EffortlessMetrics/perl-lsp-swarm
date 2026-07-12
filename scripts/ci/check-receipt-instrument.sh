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
#   3. For any gate reporting `metrics.tests_total` (i.e. a test-class
#      gate): `status` is pass, tests_total > 0, tests_passed > 0, and
#      tests_skipped < tests_total. This rejects a zero-selection /
#      zero-test / all-skipped test run (an empty ci-scope selection, a
#      `--gate` that matched nothing, or a suite that reports every test
#      skipped) from being trusted on a bare `exit_code: 0`.
#   4. At least one gate across all receipts reports test metrics --
#      otherwise this check cannot confirm any test instrument ran at all.
#
# SCOPE, DELIBERATELY NARROW: gates that do NOT report `metrics.tests_total`
# (fmt, release_history, compile_all_targets, clippy_full, and any other
# tier gate) are IGNORED ENTIRELY here -- their status/exit_code is never
# inspected, and their absence/failure/`exit 127` (e.g. missing tooling in a
# lightweight advisory runner that only installs the Rust toolchain) does
# NOT fail this check. Those gates are the required checks' job
# (`pr-smoke`/merge-gate shards), not this one's. This check answers
# exactly one question: did a real test instrument run, bound to this head,
# without a vacuous zero-test/all-skipped result -- nothing broader.
#
# Canonical live producer: `cargo xtask gates --receipt` (what `just
# pr-fast` runs), default path `target/receipts/receipt.json`; CI merge-gate
# shards write `target/receipts/shards/<gate>.json` via --receipt-path.
# Note: `cargo xtask gates` has no supported way to resolve ci-scope package
# args for a single named `rust_scoped` gate in isolation -- `--gate` always
# bypasses ci-scope planning (verified: `--tier pr-fast --gate unit_scoped`
# still errors "unresolved {package_args} placeholder"). So a `--tier
# pr-fast` run is the only way to get a real, ci-scope-resolved test-gate
# receipt, and it necessarily runs every pr_fast-tagged gate, not just the
# test ones -- hence the narrow scope above.
#
# HONEST SCOPE / WHAT THIS DOES NOT CATCH (important -- do not overclaim):
# this check proves a test gate actually ran with a nonzero, non-fully-
# skipped test count, bound to the expected head. It does NOT, and cannot,
# catch the #3599 shape itself: a test function that does an early
# `return Ok(())` (or an early `return;`) before its real assertions. Cargo
# counts that function as PASSED -- `tests_total=1, tests_passed=1`,
# `tests_skipped` absent/0 -- indistinguishable at the count level from a
# genuine pass. That mode is NOT addressable by counts at all; #3599's own
# fix used a fail-loud, per-suite `assert!()`/env-gated precondition inside
# the test harness itself (e.g. `PERL_LSP_UX_REQUIRE_BINARY`), because only
# code inside the test body can tell "ran its content" from "silently
# no-op'd while still returning Ok". See
# scripts/tests/test-check-receipt-instrument.sh's
# "documents the #3599 shape is NOT caught by counts" case for a fixture
# that demonstrates this passes here by design, not by oversight.
#
# What this check DOES catch: a gate that matched/ran zero tests at all
# (ci-scope selected no crates, a `--gate` name matched nothing, or every
# test reported as skipped), and a receipt bound to the wrong commit.
#
# Note: the live `parse_test_metrics` in gates.rs does not currently
# populate `tests_skipped` from `cargo test` output (only
# tests_passed/tests_failed/tests_ignored) -- rule 4's tests_skipped check
# is kept for forward-compat with producers that do populate it, but today
# the operative discriminator is tests_total == 0.
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
    # Filter FIRST on test-metrics presence. Gates without
    # `metrics.tests_total` (fmt, release_history, compile_all_targets,
    # clippy_full, ...) are out of scope for this check -- skip them
    # entirely without inspecting status/exit_code at all. This is what
    # keeps a lightweight advisory runner (Rust toolchain only, no `just`/
    # doc-check tooling) from being falsely rejected for tooling gates it
    # was never meant to run.
    TESTS_TOTAL="$(jq -r '.metrics.tests_total // empty' <<<"${GATE_JSON}")"
    if [[ -z "${TESTS_TOTAL}" ]]; then
      continue
    fi

    TEST_METRICS_SEEN=1
    GATE_NAME="$(jq -r '.gate_name // "<unnamed>"' <<<"${GATE_JSON}")"
    GATE_STATUS="$(jq -r '.status // "<none>"' <<<"${GATE_JSON}")"
    GATE_EXIT="$(jq -r '.exit_code // empty' <<<"${GATE_JSON}")"
    TESTS_PASSED="$(jq -r '.metrics.tests_passed // 0' <<<"${GATE_JSON}")"
    TESTS_SKIPPED="$(jq -r '.metrics.tests_skipped // empty' <<<"${GATE_JSON}")"

    # Canonical statuses (xtask/src/tasks/gates.rs): pass|fail|skip|timeout|error.
    # Long forms accepted too (tolerance the repo's own CI scripts already use).
    # A test-metrics-bearing gate reporting anything other than pass is a
    # real failure (e.g. genuine test failures), not merely out of scope.
    case "${GATE_STATUS,,}" in
      pass|passed) : ;;
      *)
        FAILED+=("${GATE_NAME} (status=${GATE_STATUS} exit=${GATE_EXIT:-n/a})")
        continue
        ;;
    esac

    # Reject a zero-selection / zero-test / all-skipped shape here -- a
    # gate that matched/ran nothing also exits 0. NOTE: this does NOT
    # catch an early-return-Ok test body (counted as passed by cargo);
    # see the module header for why that mode is out of scope for counts.
    if [[ "${TESTS_TOTAL}" -eq 0 ]]; then
      FAILED+=("${GATE_NAME} (vacuous: tests_total=0 -- instrument matched/ran nothing)")
    elif [[ -n "${TESTS_SKIPPED}" && "${TESTS_SKIPPED}" -ge "${TESTS_TOTAL}" ]]; then
      FAILED+=("${GATE_NAME} (vacuous: tests_skipped=${TESTS_SKIPPED} of tests_total=${TESTS_TOTAL})")
    elif [[ "${TESTS_PASSED}" -eq 0 ]]; then
      FAILED+=("${GATE_NAME} (vacuous: tests_passed=0 of tests_total=${TESTS_TOTAL})")
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
