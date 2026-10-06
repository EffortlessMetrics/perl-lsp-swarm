#!/usr/bin/env bash
# Runner for the adversarial agentic-editing trap suite (T1-T11).
#
# Executes every trap, prints a per-trap PASS/FAIL/UNGUARDED table plus a
# total, and exits 0 iff the harness is green: every trap exited 0 and
# reported exactly one verdict. UNGUARDED is an informative verdict (a
# documented missing gate), not a harness failure; FAIL or ERROR exits 1.
set -euo pipefail

RUN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$(mktemp -d)"
trap 'rm -rf "$LOG_DIR"' EXIT

PASS=0; FAIL=0; UNGUARDED=0; ERRORS=0
declare -a ROWS=()

for trapfile in "$RUN_DIR"/t[0-9][0-9]_*.sh; do
    name="$(basename "$trapfile")"
    log="$LOG_DIR/$name.log"
    code=0
    bash "$trapfile" >"$log" 2>&1 || code=$?
    verdicts="$(grep -c '^VERDICT: ' "$log" || true)"
    if [[ "$code" -ne 0 || "$verdicts" -ne 1 ]]; then
        ERRORS=$((ERRORS + 1))
        ROWS+=("$(printf '%-4s %-9s %s' '??' 'ERROR' "$name (exit=$code, verdicts=$verdicts; see $log)")")
        continue
    fi
    line="$(grep '^VERDICT: ' "$log")"
    # VERDICT: <Tn> <RESULT> <detail...>
    id="$(printf '%s' "$line" | awk '{print $2}')"
    result="$(printf '%s' "$line" | awk '{print $3}')"
    detail="$(printf '%s' "$line" | cut -d' ' -f4-)"
    case "$result" in
        PASS) PASS=$((PASS + 1)) ;;
        FAIL) FAIL=$((FAIL + 1)) ;;
        UNGUARDED) UNGUARDED=$((UNGUARDED + 1)) ;;
        *) ERRORS=$((ERRORS + 1)); result="ERROR" ;;
    esac
    ROWS+=("$(printf '%-4s %-9s %s' "$id" "$result" "$detail")")
done

TOTAL=$((PASS + FAIL + UNGUARDED))
printf 'TRAP VERDICT    DETAIL\n'
printf '%s\n' "${ROWS[@]}"
printf 'TOTAL: %d PASS / %d FAIL / %d UNGUARDED (%d traps)\n' "$PASS" "$FAIL" "$UNGUARDED" "$TOTAL"
if [[ "$ERRORS" -eq 0 && "$TOTAL" -eq 11 ]]; then
    printf 'HARNESS: GREEN (all 11 traps reported a verdict, none errored)\n'
    exit 0
fi
printf 'HARNESS: RED (%d trap(s) errored or missing; logs in %s)\n' "$ERRORS" "$LOG_DIR"
exit 1
