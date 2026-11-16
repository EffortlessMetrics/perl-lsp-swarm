#!/usr/bin/env bash
# Hermetic proof for agent-preflight.sh check-4 recovery hint (issue #18172).
# No Cargo; throwaway git repos only.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFLIGHT="${SCRIPT_DIR}/../agent-preflight.sh"

PASS_COUNT=0
FAIL_COUNT=0

pass() { printf 'PASS %s\n' "$1"; PASS_COUNT=$((PASS_COUNT + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

cleanup() {
  if [[ -n "${FIXTURE_ROOT:-}" && -d "${FIXTURE_ROOT}" ]]; then
    git -C "${FIXTURE_ROOT}" worktree remove --force "${FIXTURE_WT}" 2>/dev/null || true
    git -C "${FIXTURE_ROOT}" worktree prune 2>/dev/null || true
    rm -rf "${FIXTURE_ROOT}"
  fi
}
trap cleanup EXIT

if [[ ! -f "$PREFLIGHT" ]]; then
  echo "ERROR: missing $PREFLIGHT"
  exit 1
fi

FIXTURE_ROOT="$(mktemp -d)"
FIXTURE_WT="${FIXTURE_ROOT}/isolated wt/agent-ready"
git -C "${FIXTURE_ROOT}" init -q
git -C "${FIXTURE_ROOT}" config user.email "test@test.com"
git -C "${FIXTURE_ROOT}" config user.name "Test"
echo init >"${FIXTURE_ROOT}/README"
git -C "${FIXTURE_ROOT}" add README
git -C "${FIXTURE_ROOT}" commit -q -m init
git -C "${FIXTURE_ROOT}" branch -m coordination
mkdir -p "$(dirname "${FIXTURE_WT}")"
git -C "${FIXTURE_ROOT}" worktree add -q -b agent-ready "${FIXTURE_WT}"

PRIMARY="$(cd "${FIXTURE_ROOT}" && pwd -P)"
WT_RESOLVED="$(cd "${FIXTURE_WT}" && pwd -P)"

# From the primary checkout: worktree check fails (exit 2) but check-4 hint is printed.
PRIMARY_OUT="${FIXTURE_ROOT}/primary.out"
PRIMARY_CODE=0
(cd "${FIXTURE_ROOT}" && env -u CARGO_TARGET_DIR bash "$PREFLIGHT" >"${PRIMARY_OUT}" 2>&1) || PRIMARY_CODE=$?

if [[ "$PRIMARY_CODE" -eq 2 ]]; then
  pass "primary checkout preflight exits 2 (worktree isolation)"
else
  fail "primary checkout preflight — expected exit 2, got ${PRIMARY_CODE}"
fi

if grep -Fq "cd \"${PRIMARY}\"" "${PRIMARY_OUT}"; then
  fail "recovery hint must not cd to the primary checkout path"
else
  pass "recovery hint does not target the primary checkout"
fi

if grep -Fq "cd \"${WT_RESOLVED}\"" "${PRIMARY_OUT}"; then
  pass "recovery hint quotes an isolated worktree path"
else
  fail "recovery hint missing expected isolated worktree cd line"
fi

if grep -Fq 'git worktree list --porcelain' "${PRIMARY_OUT}"; then
  pass "recovery hint mentions porcelain worktree listing"
else
  fail "recovery hint missing git worktree list --porcelain guidance"
fi

# Negative control: the old grep/awk hint resolves to the primary path on this fixture.
OLD_HINT_PATH=""
OLD_HINT_PATH="$(cd "${FIXTURE_ROOT}" && git worktree list | grep "$(git branch --show-current)" | awk '{print $1}')"
OLD_HINT_RESOLVED="$(cd "${OLD_HINT_PATH}" 2>/dev/null && pwd -P || true)"
if [[ "$OLD_HINT_RESOLVED" == "$PRIMARY" ]]; then
  pass "old hint negative control resolves to primary (would mislead)"
else
  fail "old hint negative control — expected primary path, got '${OLD_HINT_RESOLVED}'"
fi

# Following the printed isolated path: check 4 must pass (full preflight exit 0).
WT_CODE=0
(cd "${FIXTURE_WT}" && env -u CARGO_TARGET_DIR bash "$PREFLIGHT" >/dev/null 2>&1) || WT_CODE=$?
if [[ "$WT_CODE" -eq 0 ]]; then
  pass "preflight in isolated worktree passes (check 4 not triggered)"
else
  fail "isolated worktree preflight — expected exit 0, got ${WT_CODE}"
fi

# Check 4 in isolation via GIT_DIR override: hint path must not leave cwd at primary.
WT_GIT_DIR="$(git -C "${FIXTURE_WT}" rev-parse --git-dir)"
HINT_OUT="${FIXTURE_ROOT}/hint-iso.out"
HINT_CODE=0
(
  cd "${FIXTURE_ROOT}"
  env -u CARGO_TARGET_DIR GIT_DIR="${WT_GIT_DIR}" bash "$PREFLIGHT" >"${HINT_OUT}" 2>&1
) || HINT_CODE=$?

if [[ "$HINT_CODE" -eq 4 ]]; then
  pass "GIT_DIR override still exits 4 on primary cwd"
else
  fail "GIT_DIR override — expected exit 4, got ${HINT_CODE}"
fi

HINT_CD=""
HINT_CD="$(grep -E '^[[:space:]]+cd "' "${HINT_OUT}" | tail -1 | sed -n 's/^[[:space:]]*cd "\(.*\)"/\1/p')"
if [[ -z "$HINT_CD" ]]; then
  fail "could not parse cd path from check-4 hint"
elif [[ "$(cd "$HINT_CD" && pwd -P)" == "$PRIMARY" ]]; then
  fail "parsed recovery cd path still equals primary"
else
  pass "parsed recovery cd path is not the primary checkout"
  FOLLOW_CODE=0
  (cd "$HINT_CD" && env -u CARGO_TARGET_DIR bash "$PREFLIGHT" >/dev/null 2>&1) || FOLLOW_CODE=$?
  if [[ "$FOLLOW_CODE" -eq 0 ]]; then
    pass "following hinted path passes preflight (check 4 cleared)"
  else
    fail "following hinted path — expected exit 0, got ${FOLLOW_CODE}"
  fi
fi

echo ""
echo "=== check-4 recovery hint tests: ${PASS_COUNT} passed, ${FAIL_COUNT} failed ==="
if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi
exit 0
