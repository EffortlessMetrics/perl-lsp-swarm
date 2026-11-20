#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CHECK_SCRIPT="${REPO_ROOT}/ci/check_unsafe_prod.sh"
BASELINE="${REPO_ROOT}/ci/unsafe_prod_baseline.txt"

PASS=0
FAIL=0

pass() {
  printf 'PASS %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf 'FAIL %s\n' "$1"
  FAIL=$((FAIL + 1))
}

echo "=== check_unsafe_prod baseline + wrapper self-test ==="
echo ""

if [[ ! -x "$CHECK_SCRIPT" ]]; then
  echo "ERROR: missing executable ${CHECK_SCRIPT}"
  exit 1
fi

if ! grep -q 'ci_exec_hygiene check-unsafe-prod' "$CHECK_SCRIPT"; then
  fail "ci/check_unsafe_prod.sh delegates to check-unsafe-prod"
else
  pass "ci/check_unsafe_prod.sh delegates to check-unsafe-prod"
fi

if ! grep -q 'xtask_wrapper.sh' "$CHECK_SCRIPT"; then
  fail "ci/check_unsafe_prod.sh sources ci/xtask_wrapper.sh"
else
  pass "ci/check_unsafe_prod.sh sources ci/xtask_wrapper.sh"
fi

if [[ ! -f "$BASELINE" ]]; then
  fail "ci/unsafe_prod_baseline.txt exists"
else
  pass "ci/unsafe_prod_baseline.txt exists"
fi

trimmed="$(tr -d '[:space:]' < "$BASELINE")"
if [[ "$trimmed" =~ ^[0-9]+$ ]]; then
  fail "baseline is not a bare integer ratchet"
else
  pass "baseline is not a bare integer ratchet"
fi

mapfile -t site_lines < <(grep -Ev '^[[:space:]]*(#|$)' "$BASELINE")
if ((${#site_lines[@]} == 0)); then
  fail "baseline lists at least one path:line site"
else
  pass "baseline lists ${#site_lines[@]} path:line site(s)"
fi

prev=""
for line in "${site_lines[@]}"; do
  if [[ ! "$line" =~ ^[^:]+:[0-9]+$ ]]; then
    fail "baseline line has path:line shape: ${line}"
    continue
  fi
  if [[ -n "$prev" && "$line" < "$prev" ]]; then
    fail "baseline entries are sorted (${line} before ${prev})"
  fi
  if [[ "$line" == "$prev" ]]; then
    fail "baseline has duplicate entry: ${line}"
  fi
  prev="$line"
done
if [[ -n "$prev" ]]; then
  pass "baseline entries are sorted and unique"
fi

echo ""
echo "Results: ${PASS} passed, ${FAIL} failed"
if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
