#!/usr/bin/env bash
# Runner for the adversarial agentic-editing trap suite (T1-T11).
#
# Executes every trap, prints a per-trap PASS/FAIL/UNGUARDED table plus a
# total, and exits 0 iff the harness is green: every trap exited 0 and
# reported exactly one verdict. UNGUARDED is an informative verdict (a
# documented missing gate), not a harness failure; FAIL or ERROR exits 1.
# The verdict id must match the trap filename (t02_*.sh reports T2); a
# mismatch is a harness error. The collected ids must also be exactly
# T1-T11: a missing trap plus a same-named duplicate would otherwise
# total 11 without running the missing trap. On a red run the failing traps' full logs
# print below the table (logs live in a temp dir that is always removed).
set -euo pipefail

RUN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$(mktemp -d)"
trap 'rm -rf "$LOG_DIR"' EXIT

PASS=0; FAIL=0; UNGUARDED=0; ERRORS=0
declare -a ROWS=()
declare -a BAD_LOGS=()
declare -a IDS=()

for trapfile in "$RUN_DIR"/t[0-9][0-9]_*.sh; do
    name="$(basename "$trapfile")"
    log="$LOG_DIR/$name.log"
    code=0
    bash "$trapfile" >"$log" 2>&1 || code=$?
    verdicts="$(grep -c '^VERDICT: ' "$log" || true)"
    if [[ "$code" -ne 0 || "$verdicts" -ne 1 ]]; then
        ERRORS=$((ERRORS + 1))
        ROWS+=("$(printf '%-4s %-9s %s' '??' 'ERROR' "$name (exit=$code, verdicts=$verdicts)")")
        BAD_LOGS+=("$log")
        continue
    fi
    line="$(grep '^VERDICT: ' "$log")"
    # VERDICT: <Tn> <RESULT> <detail...>
    id="$(printf '%s' "$line" | awk '{print $2}')"
    result="$(printf '%s' "$line" | awk '{print $3}')"
    detail="$(printf '%s' "$line" | cut -d' ' -f4-)"
    # The verdict id must name the trap that ran: t02_*.sh reports T2.
    num="${name#t}"
    num="${num%%_*}"
    expected="T$((10#$num))"
    if [[ "$id" != "$expected" ]]; then
        ERRORS=$((ERRORS + 1))
        ROWS+=("$(printf '%-4s %-9s %s' '??' 'ERROR' "$name reported $id, expected $expected")")
        BAD_LOGS+=("$log")
        continue
    fi
    IDS+=("$id")
    case "$result" in
        PASS) PASS=$((PASS + 1)) ;;
        FAIL) FAIL=$((FAIL + 1)); BAD_LOGS+=("$log") ;;
        UNGUARDED) UNGUARDED=$((UNGUARDED + 1)) ;;
        *) ERRORS=$((ERRORS + 1)); result="ERROR"; BAD_LOGS+=("$log") ;;
    esac
    ROWS+=("$(printf '%-4s %-9s %s' "$id" "$result" "$detail")")
done

TOTAL=$((PASS + FAIL + UNGUARDED))
# A per-file id match is not enough: a missing trap plus a same-named
# duplicate would still total 11. Every expected id must appear; with
# TOTAL == 11 that also excludes duplicates.
MISSING_IDS=""
for n in 1 2 3 4 5 6 7 8 9 10 11; do
    hit=0
    for got in "${IDS[@]}"; do
        if [[ "$got" == "T$n" ]]; then hit=1; break; fi
    done
    if [[ "$hit" -eq 0 ]]; then MISSING_IDS="$MISSING_IDS T$n"; fi
done
if [[ -n "$MISSING_IDS" ]]; then
    ERRORS=$((ERRORS + 1))
    ROWS+=("$(printf '%-4s %-9s %s' '??' 'ERROR' "missing trap ids:$MISSING_IDS")")
fi
printf 'TRAP VERDICT    DETAIL\n'
printf '%s\n' "${ROWS[@]}"
printf 'TOTAL: %d PASS / %d FAIL / %d UNGUARDED (%d traps)\n' "$PASS" "$FAIL" "$UNGUARDED" "$TOTAL"
if [[ "$ERRORS" -eq 0 && "$FAIL" -eq 0 && "$TOTAL" -eq 11 ]]; then
    printf 'HARNESS: GREEN (all 11 traps reported PASS or UNGUARDED, none errored)\n'
    exit 0
fi
printf 'HARNESS: RED (%d trap(s) errored, %d FAIL; failing logs follow)\n' "$ERRORS" "$FAIL"
for bad in "${BAD_LOGS[@]}"; do
    printf '%s\n' "--- $bad ---"
    cat "$bad"
done
exit 1
