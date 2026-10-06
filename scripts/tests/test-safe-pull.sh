#!/usr/bin/env bash
# scripts/tests/test-safe-pull.sh
#
# Regression tests for issue #17403: safe-pull.sh must never irrecoverably
# delete colliding untracked files. Colliding content must survive
# byte-identical (in place or in the named salvage packet), and
# collision-free pulls must keep working (no-collision control).
#
# Usage:
#   bash scripts/tests/test-safe-pull.sh
#
# The script under test can be overridden (falsification seam — point it at
# the pre-fix script to prove this suite fails without the repair):
#   SAFE_PULL_SCRIPT=/tmp/safe-pull-prefix.sh bash scripts/tests/test-safe-pull.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SAFE_PULL="${SAFE_PULL_SCRIPT:-${REPO_ROOT}/scripts/safe-pull.sh}"

PASS=0
FAIL=0
TMPDIR_BASE=""
ORIGIN=""
WORK=""
ADV=""

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

assert_contains() {
  local label="$1"
  local haystack="$2"
  local needle="$3"

  if grep -qF "$needle" <<<"$haystack"; then
    pass "$label"
  else
    fail "$label"
    printf 'missing: %s\noutput:\n%s\n' "$needle" "$haystack"
  fi
}

assert_not_contains() {
  local label="$1"
  local haystack="$2"
  local needle="$3"

  if grep -qF "$needle" <<<"$haystack"; then
    fail "$label"
    printf 'unexpected: %s\noutput:\n%s\n' "$needle" "$haystack"
  else
    pass "$label"
  fi
}

assert_byte_identical() {
  local label="$1"
  local actual="$2"
  local expected="$3"

  if [[ -f "$actual" ]] && cmp -s "$actual" "$expected"; then
    pass "$label"
  else
    fail "$label"
    printf 'not byte-identical: %s vs %s\n' "$actual" "$expected"
  fi
}

# Fixture: bare origin with a base commit on main, plus two clones.
# Sets ORIGIN, WORK (the pull target), and ADV (the origin advancer).
setup_repos() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
  TMPDIR_BASE="$(mktemp -d)"
  ORIGIN="${TMPDIR_BASE}/origin.git"
  WORK="${TMPDIR_BASE}/work"
  ADV="${TMPDIR_BASE}/advancer"

  git init -q -b main --bare "$ORIGIN"
  git clone -q "$ORIGIN" "$ADV" 2>/dev/null
  git -C "$ADV" config user.email "safe-pull-test@example.com"
  git -C "$ADV" config user.name "safe-pull-test"
  git -C "$ADV" config commit.gpgsign false
  echo "base" > "${ADV}/base.txt"
  git -C "$ADV" add base.txt
  git -C "$ADV" commit -qm "base"
  git -C "$ADV" push -q -u origin main

  git clone -q "$ORIGIN" "$WORK" 2>/dev/null
  git -C "$WORK" config user.email "safe-pull-test@example.com"
  git -C "$WORK" config user.name "safe-pull-test"
  git -C "$WORK" config commit.gpgsign false
}

run_safe_pull() {
  # $1 = directory to pull in; prints output, returns script exit status
  local dir="$1"
  local out rc
  rc=0
  out="$(cd "$dir" && bash "$SAFE_PULL" 2>&1)" || rc=$?
  printf '%s' "$out"
  return "$rc"
}

test_colliding_untracked_file_survives_with_content() {
  setup_repos

  # Untracked files in $WORK holding known content (multi-line + trailing
  # newline so the byte-identity check is meaningful), including one nested
  # path to cover salvage-dir structure preservation.
  printf 'KNOWN-CONTENT-17403\nsecond line\n' > "${WORK}/notes.txt"
  cp "${WORK}/notes.txt" "${TMPDIR_BASE}/expected.txt"
  mkdir -p "${WORK}/docs"
  printf 'NESTED-KNOWN-17403\n' > "${WORK}/docs/plan.txt"
  cp "${WORK}/docs/plan.txt" "${TMPDIR_BASE}/expected-nested.txt"

  # Origin advances the same paths so the pull reports
  # "would be overwritten by merge".
  printf 'REMOTE-CONTENT\n' > "${ADV}/notes.txt"
  mkdir -p "${ADV}/docs"
  printf 'REMOTE-NESTED\n' > "${ADV}/docs/plan.txt"
  git -C "$ADV" add notes.txt docs/plan.txt
  git -C "$ADV" commit -qm "add notes"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "collision: safe-pull exits 0"
  else
    fail "collision: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "collision: output names the salvage packet" "$out" "salvage"

  local salvage_dir
  salvage_dir="$(grep -F 'salvage -> ' <<<"$out" | head -n 1 | sed 's/.*salvage -> //' || true)"
  if [[ -z "${salvage_dir:-}" ]]; then
    fail "collision: salvage directory is printed"
    return
  fi
  pass "collision: salvage directory is printed"

  # Content must be byte-identical either in place or in the named salvage
  # packet (per the Builder Spec: in place wins when the helper keeps it).
  if [[ -f "${WORK}/notes.txt" ]] && cmp -s "${WORK}/notes.txt" "${TMPDIR_BASE}/expected.txt"; then
    pass "collision: known content survives byte-identical (in place)"
  elif [[ -f "${salvage_dir}/notes.txt" ]] && cmp -s "${salvage_dir}/notes.txt" "${TMPDIR_BASE}/expected.txt"; then
    pass "collision: known content survives byte-identical (salvaged)"
  else
    fail "collision: known content survives byte-identical"
    printf 'work notes.txt:\n'
    cat "${WORK}/notes.txt" 2>/dev/null || printf '(missing)\n'
    printf 'salvage packet listing (%s):\n' "$salvage_dir"
    find "$salvage_dir" -type f 2>/dev/null || printf '(missing)\n'
  fi

  # Nested colliding path must also survive byte-identical with structure.
  assert_byte_identical "collision: nested content salvaged byte-identical" \
    "${salvage_dir}/docs/plan.txt" "${TMPDIR_BASE}/expected-nested.txt"

  # The merge itself must have completed: work now tracks origin's version.
  assert_byte_identical "collision: pull delivers remote content" \
    "${WORK}/notes.txt" "${ADV}/notes.txt"
}

test_clean_pull_still_succeeds() {
  setup_repos

  # No-collision control: origin advances an unrelated path; the pull must
  # succeed without touching the salvage path.
  printf 'REMOTE-ONLY\n' > "${ADV}/other.txt"
  git -C "$ADV" add other.txt
  git -C "$ADV" commit -qm "add other"
  git -C "$ADV" push -q origin main

  local out rc
  rc=0
  out="$(run_safe_pull "$WORK")" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    pass "control: safe-pull exits 0"
  else
    fail "control: safe-pull exits 0 (got $rc)"
    printf 'output:\n%s\n' "$out"
    return
  fi
  assert_contains "control: pull reports success" "$out" "Pull succeeded"
  assert_not_contains "control: no salvage on a clean pull" "$out" "salvage"
  assert_byte_identical "control: pull delivers remote content" \
    "${WORK}/other.txt" "${ADV}/other.txt"
}

test_colliding_untracked_file_survives_with_content
test_clean_pull_still_succeeds

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
