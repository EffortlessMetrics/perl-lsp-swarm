#!/usr/bin/env bash
# Offline self-test for release-turnkey manual/partial merge handoffs (#16798).
#
# Falsifiers:
#   1. --no-auto-merge dispatches Release Orchestration.
#   2. --no-wait-pr-merge dispatches while merge is only requested/armed.
#   3. Unchanged target SHA after alleged merge is warning-only and continues.
#   4. A handoff omits PR/head/version identity.
#   5. An old handoff is reused after PR head movement.
#   6. Dry-run writes an authoritative transaction or mutates GitHub.
#   7. Exit 0 is treated as orchestration success for a typed handoff.
#
# Opposite-direction control: auto-merge + proven merge + moved target SHA
# still dispatches Release Orchestration.
#
# Everything runs offline against stub `gh`/`cargo` prepended to PATH.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
LIB="${REPO_ROOT}/scripts/lib/release-turnkey-handoff.sh"
TURNKEY="${REPO_ROOT}/scripts/release-turnkey-pr.sh"

PASS=0
FAIL=0
TMP_ROOT=""

cleanup() {
  if [[ -n "${TMP_ROOT:-}" && -d "$TMP_ROOT" ]]; then
    rm -rf "$TMP_ROOT"
  fi
}
trap cleanup EXIT

pass() { printf 'PASS %s\n' "$1"; PASS=$((PASS + 1)); }
fail() { printf 'FAIL %s\n' "$1"; FAIL=$((FAIL + 1)); }

expect_eq() {
  local label="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    pass "$label"
  else
    fail "$label (expected '${expected}', got '${actual}')"
  fi
}

expect_contains() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -Fq -- "$needle"; then
    pass "$label"
  else
    fail "$label (missing: $needle)"
  fi
}

expect_not_contains() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -Fq -- "$needle"; then
    fail "$label (unexpected: $needle)"
  else
    pass "$label"
  fi
}

if [[ ! -f "$LIB" ]]; then
  echo "ERROR: missing $LIB"
  exit 1
fi
if [[ ! -f "$TURNKEY" ]]; then
  echo "ERROR: missing $TURNKEY"
  exit 1
fi
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq required"; exit 1; }

# shellcheck source=../lib/release-turnkey-handoff.sh
. "$LIB"

TMP_ROOT="$(mktemp -d)"
ORIGIN="$TMP_ROOT/origin.git"
WORK="$TMP_ROOT/work"
STUB_BIN="$TMP_ROOT/bin"
STUB_STATE="$TMP_ROOT/gh-state"
mkdir -p "$STUB_BIN" "$STUB_STATE" "$WORK"

git init --bare -q "$ORIGIN"
git -C "$WORK" init -b main -q
git -C "$WORK" config user.email "turnkey-test@example.test"
git -C "$WORK" config user.name "Turnkey Test"
git -C "$WORK" config commit.gpgsign false
echo 'base' >"$WORK/README"
git -C "$WORK" add README
git -C "$WORK" commit -m 'initial' >/dev/null
git -C "$WORK" branch -M main
git -C "$WORK" remote add origin "$ORIGIN"
git -C "$WORK" push -u origin main >/dev/null
BASE_SHA="$(git -C "$WORK" rev-parse HEAD)"

cat >"$STUB_BIN/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_STUB_LOG"

case "${1:-}" in
  auth)
    exit 0
    ;;
  repo)
    if [[ "$*" == *nameWithOwner* ]]; then
      printf '%s\n' "${GH_STUB_REPO:-EffortlessMetrics/perl-lsp-swarm}"
      exit 0
    fi
    if [[ "$*" == *defaultBranchRef* ]]; then
      printf '%s\n' "${GH_STUB_BASE:-main}"
      exit 0
    fi
    ;;
  workflow)
    if [[ "${2:-}" == "run" ]]; then
      printf '%s\n' "$*" >> "$GH_STUB_WORKFLOW_LOG"
      exit 0
    fi
    ;;
  run)
    if [[ "${2:-}" == "list" ]]; then
      created="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
      old="${GH_STUB_BASE_SHA:-}"
      new="${GH_STUB_HEAD_SHA:-}"
      if [[ -n "${GH_STUB_WORK:-}" ]]; then
        new="$(git -C "$GH_STUB_WORK" rev-parse origin/main 2>/dev/null || printf '%s' "$new")"
      fi
      jq -n --arg old "$old" --arg new "$new" --arg created "$created" \
        '[
          {databaseId: 111, headSha: $old, createdAt: $created, status: "completed", conclusion: "success"},
          {databaseId: 222, headSha: $new, createdAt: $created, status: "completed", conclusion: "success"}
        ]'
      exit 0
    fi
    if [[ "${2:-}" == "view" ]]; then
      if [[ "$*" == *status* ]]; then
        printf '%s\n' 'completed success'
        exit 0
      fi
      printf '%s\n' 'https://github.com/example/actions/runs/111'
      exit 0
    fi
    ;;
  pr)
    case "${2:-}" in
      list)
        printf '%s\n' "${GH_STUB_PR_NUMBER:-42}"
        exit 0
        ;;
      view)
        if [[ "$*" == *headRefOid* ]]; then
          printf '%s\n' "${GH_STUB_PR_HEAD:-deadbeef}"
          exit 0
        fi
        if [[ "$*" == *mergedAt* ]]; then
          printf '%s' "${GH_STUB_MERGED_AT:-}"
          exit 0
        fi
        printf '%s\n' "https://github.com/${GH_STUB_REPO:-EffortlessMetrics/perl-lsp-swarm}/pull/${GH_STUB_PR_NUMBER:-42}"
        exit 0
        ;;
      merge)
        printf '%s\n' "$*" >> "$GH_STUB_MERGE_LOG"
        if [[ -n "${GH_STUB_ADVANCE_ON_MERGE:-}" && -x "${GH_STUB_ADVANCE_ON_MERGE}" ]]; then
          "$GH_STUB_ADVANCE_ON_MERGE"
        fi
        exit 0
        ;;
    esac
    ;;
esac
echo "gh stub: unhandled: $*" >&2
exit 1
STUB
chmod +x "$STUB_BIN/gh"

cat >"$STUB_BIN/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${CARGO_STUB_LOG:-/dev/null}"
if [[ "${1:-}" == "--version" ]]; then
  printf '%s\n' 'cargo 1.95.0 (turnkey-test-stub)'
  exit 0
fi
# branch-deletion-admission retain
exit 3
STUB
chmod +x "$STUB_BIN/cargo"

run_turnkey() {
  local logfile="$1"
  shift
  GH_STUB_LOG="$STUB_STATE/gh.log"
  GH_STUB_WORKFLOW_LOG="$STUB_STATE/workflow.log"
  GH_STUB_MERGE_LOG="$STUB_STATE/merge.log"
  CARGO_STUB_LOG="$STUB_STATE/cargo.log"
  : >"$GH_STUB_LOG"
  : >"$GH_STUB_WORKFLOW_LOG"
  : >"$GH_STUB_MERGE_LOG"
  : >"$CARGO_STUB_LOG"
  (
    cd "$WORK"
    PATH="$STUB_BIN:$PATH" \
      GH_STUB_LOG="$GH_STUB_LOG" \
      GH_STUB_WORKFLOW_LOG="$GH_STUB_WORKFLOW_LOG" \
      GH_STUB_MERGE_LOG="$GH_STUB_MERGE_LOG" \
      CARGO_STUB_LOG="$CARGO_STUB_LOG" \
      GH_STUB_WORK="$WORK" \
      GH_STUB_BASE_SHA="$BASE_SHA" \
      GH_STUB_HEAD_SHA="$(git rev-parse origin/main)" \
      GH_STUB_ADVANCE_ON_MERGE="${GH_STUB_ADVANCE_ON_MERGE:-}" \
      GH_STUB_PR_NUMBER="${GH_STUB_PR_NUMBER:-42}" \
      GH_STUB_PR_HEAD="${GH_STUB_PR_HEAD:-abc123def}" \
      GH_STUB_MERGED_AT="${GH_STUB_MERGED_AT:-}" \
      GH_STUB_REPO="EffortlessMetrics/perl-lsp-swarm" \
      GH_STUB_BASE="main" \
      bash "$TURNKEY" "$@"
  ) >"$logfile" 2>&1
}

cat >"$STUB_STATE/advance-origin" <<ADV
#!/usr/bin/env bash
set -euo pipefail
echo "landed \$(date +%s)" >>"$WORK/README"
git -C "$WORK" add README
git -C "$WORK" commit -m 'landed preparation' >/dev/null
git -C "$WORK" push origin main >/dev/null
ADV
chmod +x "$STUB_STATE/advance-origin"

echo "=== release-turnkey handoff library ==="

record="$(turnkey_build_handoff \
  "$TURNKEY_STAGE_MANUAL_MERGE" true \
  "EffortlessMetrics/perl-lsp-swarm" main 0.9.2 "$BASE_SHA" \
  42 "https://example.test/pull/42" abc123def main \
  "merge the PR" "wake when merged" \
  "$(turnkey_default_invalidators)")" || {
  fail "build handoff record"
  record="{}"
}

expect_eq "schema version" "$TURNKEY_SCHEMA_VERSION" "$(printf '%s' "$record" | jq -r '.schema_version')"
expect_eq "stage" "$TURNKEY_STAGE_MANUAL_MERGE" "$(printf '%s' "$record" | jq -r '.stage')"
expect_eq "PR number present" "42" "$(printf '%s' "$record" | jq -r '.pr.number')"
expect_eq "PR head present" "abc123def" "$(printf '%s' "$record" | jq -r '.pr.head')"
expect_eq "version present" "0.9.2" "$(printf '%s' "$record" | jq -r '.requested_version')"
digest="$(printf '%s' "$record" | jq -r '.digest')"
if [[ "$digest" == sha256:* && "${#digest}" -eq 71 ]]; then
  pass "digest is sha256"
else
  fail "digest is sha256 (got $digest)"
fi

recomputed="$(printf '%s' "$record" | jq 'del(.digest)' | turnkey_attach_digest | jq -r '.digest')"
expect_eq "digest is deterministic" "$digest" "$recomputed"

missing_pr="$(
  if turnkey_build_handoff \
    "$TURNKEY_STAGE_MANUAL_MERGE" true \
    "EffortlessMetrics/perl-lsp-swarm" main 0.9.2 "$BASE_SHA" \
    "" "https://example.test/pull/42" "" main \
    "merge" "wake" "$(turnkey_default_invalidators)" 2>/dev/null; then
    echo ok
  else
    echo refused
  fi
)"
expect_eq "handoff without PR identity is refused" "refused" "$missing_pr"

tx_path="$TMP_ROOT/tx.json"
turnkey_write_authoritative "$tx_path" "$record"
if turnkey_validate_existing_record "$tx_path" "abc123def"; then
  pass "matching PR head accepts record"
else
  fail "matching PR head accepts record"
fi
if turnkey_validate_existing_record "$tx_path" "moved-head"; then
  fail "moved PR head invalidates record"
else
  pass "moved PR head invalidates record"
fi
if turnkey_validate_existing_record "$TMP_ROOT/missing.json" "abc123def"; then
  fail "missing record refuses rediscovery"
else
  pass "missing record refuses rediscovery"
fi

plan="$(turnkey_build_handoff \
  "$TURNKEY_STAGE_MANUAL_MERGE" false \
  "EffortlessMetrics/perl-lsp-swarm" main 0.9.2 "$BASE_SHA" \
  42 "https://example.test/pull/42" abc123def main \
  "merge" "wake" "$(turnkey_default_invalidators)")"
if turnkey_write_authoritative "$TMP_ROOT/plan.json" "$plan" 2>/dev/null; then
  fail "non-authoritative record is not persisted"
else
  pass "non-authoritative record is not persisted"
fi
expect_eq "manual-merge exit code" "$TURNKEY_EXIT_MANUAL_MERGE" "$(turnkey_exit_code_for_stage "$TURNKEY_STAGE_MANUAL_MERGE")"
expect_eq "merge-requested exit code" "$TURNKEY_EXIT_MERGE_REQUESTED" "$(turnkey_exit_code_for_stage "$TURNKEY_STAGE_MERGE_REQUESTED")"

echo ""
echo "=== release-turnkey production path ==="

TX1="$TMP_ROOT/manual.json"
out="$TMP_ROOT/no-auto-merge.log"
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --no-auto-merge --workflow-timeout 8 --transaction "$TX1"
code=$?
set -e
expect_eq "--no-auto-merge exit code" "$TURNKEY_EXIT_MANUAL_MERGE" "$code"
if grep -Fq "Release Orchestration" "$STUB_STATE/workflow.log"; then
  fail "--no-auto-merge does not dispatch Release Orchestration"
else
  pass "--no-auto-merge does not dispatch Release Orchestration"
fi
expect_not_contains "--no-auto-merge does not request merge" "pr merge" "$(cat "$STUB_STATE/merge.log")"
if [[ -f "$TX1" ]]; then
  pass "--no-auto-merge writes a transaction file"
  expect_eq "manual stage" "$TURNKEY_STAGE_MANUAL_MERGE" "$(jq -r '.stage' "$TX1")"
  expect_eq "manual authoritative" "true" "$(jq -r '.authoritative' "$TX1")"
  expect_eq "manual PR" "42" "$(jq -r '.pr.number' "$TX1")"
  expect_eq "manual head" "abc123def" "$(jq -r '.pr.head' "$TX1")"
  expect_eq "manual version" "0.9.2" "$(jq -r '.requested_version' "$TX1")"
else
  fail "--no-auto-merge writes a transaction file"
fi
expect_contains "manual handoff names stage" "typed handoff: manual_merge_required" "$(cat "$out")"
expect_not_contains "manual handoff is not a release failure" "[error]" "$(cat "$out")"
expect_contains "manual handoff is not completed preparation" "not completed preparation" "$(cat "$out")"

TX2="$TMP_ROOT/waiting.json"
out="$TMP_ROOT/no-wait.log"
GH_STUB_MERGED_AT=""
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --no-wait-pr-merge --workflow-timeout 8 --transaction "$TX2"
code=$?
set -e
expect_eq "--no-wait-pr-merge exit code while unmerged" "$TURNKEY_EXIT_MERGE_REQUESTED" "$code"
if grep -Fq "Release Orchestration" "$STUB_STATE/workflow.log"; then
  fail "--no-wait-pr-merge does not dispatch while unmerged"
else
  pass "--no-wait-pr-merge does not dispatch while unmerged"
fi
expect_contains "--no-wait-pr-merge requests merge" "pr merge" "$(cat "$STUB_STATE/merge.log")"
if [[ -f "$TX2" ]]; then
  expect_eq "waiting stage" "$TURNKEY_STAGE_MERGE_REQUESTED" "$(jq -r '.stage' "$TX2")"
else
  fail "--no-wait-pr-merge writes waiting transaction"
fi

out="$TMP_ROOT/unchanged.log"
GH_STUB_MERGED_AT="2026-09-29T00:00:00Z"
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --workflow-timeout 8 --transaction "$TMP_ROOT/auto-unchanged.json"
code=$?
set -e
if [[ "$code" -eq 0 ]]; then
  fail "unchanged target SHA is blocking (got exit 0)"
else
  pass "unchanged target SHA is blocking (exit $code)"
fi
if grep -Fq "Release Orchestration" "$STUB_STATE/workflow.log"; then
  fail "unchanged target SHA does not dispatch orchestration"
else
  pass "unchanged target SHA does not dispatch orchestration"
fi
expect_contains "unchanged SHA is not warning-only continue" "refusing to dispatch Release Orchestration" "$(cat "$out")"

stale="$TMP_ROOT/stale.json"
printf '%s\n' "$record" >"$stale"
jq '.pr.head = "old-head"' "$stale" >"$stale.tmp" && mv "$stale.tmp" "$stale"
out="$TMP_ROOT/stale.log"
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --no-auto-merge --workflow-timeout 8 --transaction "$stale"
code=$?
set -e
if [[ "$code" -eq "$TURNKEY_EXIT_MANUAL_MERGE" ]]; then
  fail "stale PR head is not reused as a fresh handoff"
elif [[ "$code" -eq 0 ]]; then
  fail "stale PR head does not continue"
else
  pass "stale PR head is rejected (exit $code)"
fi
if grep -Fq "Release Orchestration" "$STUB_STATE/workflow.log"; then
  fail "stale handoff does not dispatch orchestration"
else
  pass "stale handoff does not dispatch orchestration"
fi

out="$TMP_ROOT/dry-run.log"
dry_tx="$TMP_ROOT/dry.json"
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --dry-run --no-auto-merge --transaction "$dry_tx"
code=$?
set -e
expect_eq "dry-run exit 0" "0" "$code"
if [[ -e "$dry_tx" ]]; then
  fail "dry-run does not write a transaction file"
else
  pass "dry-run does not write a transaction file"
fi
if grep -Fq "workflow run" "$STUB_STATE/workflow.log"; then
  fail "dry-run performs zero workflow mutation"
else
  pass "dry-run performs zero workflow mutation"
fi
if grep -Fq "pr merge" "$STUB_STATE/merge.log"; then
  fail "dry-run does not request merge"
else
  pass "dry-run does not request merge"
fi
expect_contains "dry-run names a non-authoritative plan" "DRY RUN" "$(cat "$out")"

out="$TMP_ROOT/auto-landed.log"
GH_STUB_MERGED_AT="2026-09-29T00:00:00Z"
GH_STUB_ADVANCE_ON_MERGE="$STUB_STATE/advance-origin"
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --no-wait-release --workflow-timeout 8 --transaction "$TMP_ROOT/landed.json"
code=$?
set -e
unset GH_STUB_ADVANCE_ON_MERGE
expect_eq "proven landing still dispatches (exit 0)" "0" "$code"
if grep -Fq "Release Orchestration" "$STUB_STATE/workflow.log"; then
  pass "proven landing dispatches Release Orchestration"
else
  fail "proven landing dispatches Release Orchestration"
fi

out="$TMP_ROOT/no-wait-already-landed.log"
GH_STUB_MERGED_AT="2026-09-29T00:00:00Z"
GH_STUB_ADVANCE_ON_MERGE="$STUB_STATE/advance-origin"
set +e
run_turnkey "$out" --version 0.9.2 --base-branch main --no-wait-pr-merge --no-wait-release --workflow-timeout 8 --transaction "$TMP_ROOT/already.json"
code=$?
set -e
unset GH_STUB_ADVANCE_ON_MERGE
expect_eq "--no-wait-pr-merge continues only when already landed" "0" "$code"
if grep -Fq "Release Orchestration" "$STUB_STATE/workflow.log"; then
  pass "--no-wait-pr-merge dispatches when landing is already proven"
else
  fail "--no-wait-pr-merge dispatches when landing is already proven"
fi

echo ""
echo "=== $PASS passed, $FAIL failed ==="
if [[ "$FAIL" -ne 0 ]]; then
  echo "---- no-auto-merge log ----"
  cat "$TMP_ROOT/no-auto-merge.log" 2>/dev/null || true
  echo "---- no-wait log ----"
  cat "$TMP_ROOT/no-wait.log" 2>/dev/null || true
  echo "---- unchanged log ----"
  cat "$TMP_ROOT/unchanged.log" 2>/dev/null || true
  echo "---- stale log ----"
  cat "$TMP_ROOT/stale.log" 2>/dev/null || true
  echo "---- auto-landed log ----"
  cat "$TMP_ROOT/auto-landed.log" 2>/dev/null || true
  echo "---- already-landed log ----"
  cat "$TMP_ROOT/no-wait-already-landed.log" 2>/dev/null || true
  exit 1
fi
exit 0
