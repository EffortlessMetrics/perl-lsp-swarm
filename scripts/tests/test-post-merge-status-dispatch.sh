#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DISPATCHER="$ROOT/scripts/ci/dispatch-post-merge-status.sh"
WORKFLOW="$ROOT/.github/workflows/post-merge-status.yml"
CI_WORKFLOW="$ROOT/.github/workflows/ci.yml"
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

pass() {
  printf 'PASS: %s\n' "$1"
}

[[ -f "$DISPATCHER" ]] || fail "dispatcher is missing"
[[ -f "$WORKFLOW" ]] || fail "post-merge workflow is missing"
[[ -f "$CI_WORKFLOW" ]] || fail "ci workflow is missing"

source_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
head_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
repository=EffortlessMetrics/perl-lsp-swarm
automation_ref=automation/post-merge-status
pr_number=13041

make_mock_gh() {
  local mock_dir="$1"
  mkdir -p "$mock_dir"
  cat > "$mock_dir/gh" <<'MOCK_GH'
#!/usr/bin/env bash
set -euo pipefail

printf 'gh' >> "$MOCK_CALL_LOG"
printf ' %q' "$@" >> "$MOCK_CALL_LOG"
printf '\n' >> "$MOCK_CALL_LOG"

  if [[ "${1:-}" == "api" ]]; then
    if [[ "${MOCK_API_FAILURE:-0}" == "1" ]]; then
      exit 42
    fi
    if [[ "$*" == *"/pulls/"* ]]; then
      if [[ "$*" == *".head.sha"* || "$*" == *".head.ref"* ]]; then
        exit 45
      fi
      printf '%s\n' "${MOCK_PR_METADATA:?}"
      exit 0
  fi
  if [[ "$*" == *"/commits/"* ]]; then
    printf '%s\n' "${MOCK_COMMIT_METADATA:?}"
    exit 0
  fi
  exit 43
fi

if [[ "${1:-}" == "workflow" && "${2:-}" == "run" ]]; then
  if [[ "${3:-}" == "${MOCK_FAIL_WORKFLOW:-}" ]]; then
    exit 17
  fi
  exit 0
fi

exit 44
MOCK_GH
  chmod +x "$mock_dir/gh"
}

run_dispatch() {
  local mock_dir="$1"
  local log="$2"
  local code=0
  PATH="$mock_dir:$PATH" \
    GITHUB_REPOSITORY="$repository" \
    VERIFIED_SOURCE_SHA="$source_sha" \
    GENERATED_PR_NUMBER="$pr_number" \
    GENERATED_HEAD_SHA="${GENERATED_HEAD_SHA_OVERRIDE:-$head_sha}" \
    GENERATED_HEAD_REF="${GENERATED_HEAD_REF_OVERRIDE:-$automation_ref}" \
    EXPECTED_HEAD_REF="$automation_ref" \
    EXPECTED_BASE_REPOSITORY="$repository" \
    EXPECTED_BASE_REF=main \
    MOCK_CALL_LOG="$log" \
    MOCK_PR_METADATA="$MOCK_PR_METADATA" \
    MOCK_COMMIT_METADATA="$MOCK_COMMIT_METADATA" \
    MOCK_API_FAILURE="${MOCK_API_FAILURE:-0}" \
    MOCK_FAIL_WORKFLOW="${MOCK_FAIL_WORKFLOW:-}" \
    bash "$DISPATCHER" >/dev/null 2>&1 || code=$?
  return "$code"
}

new_case() {
  local name="$1"
  CASE_DIR="$TMP_ROOT/$name"
  mkdir -p "$CASE_DIR/bin"
  make_mock_gh "$CASE_DIR/bin"
  CASE_LOG="$CASE_DIR/calls.log"
  : > "$CASE_LOG"
  unset GENERATED_HEAD_SHA_OVERRIDE GENERATED_HEAD_REF_OVERRIDE
  unset MOCK_API_FAILURE MOCK_FAIL_WORKFLOW
  printf -v MOCK_PR_METADATA '%s\t%s\t%s' "$repository" "$repository" main
  printf -v MOCK_COMMIT_METADATA '1\t%s' "$source_sha"
}

count_workflow_calls() {
  local log="$1"
  (grep -c '^gh workflow run ' "$log" || true)
}

assert_no_workflow_calls() {
  local log="$1"
  [[ "$(count_workflow_calls "$log")" -eq 0 ]] \
    || fail "unexpected workflow dispatch in $log"
}

new_case valid
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  || fail "valid identity was rejected"
[[ "$(count_workflow_calls "$CASE_LOG")" -eq 4 ]] \
  || fail "valid identity did not dispatch all four workflows"
grep -Fq "gh workflow run ci.yml --ref $automation_ref -f base_sha=$source_sha -f head_sha=$head_sha" "$CASE_LOG" \
  || fail "ci.yml did not receive the exact source/head inputs"
for workflow in em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  grep -Fq "gh workflow run $workflow --ref $automation_ref" "$CASE_LOG" \
    || fail "$workflow was not dispatched from the action-output branch"
done
! grep -Fq 'force_target' "$CASE_LOG" \
  || fail "force_target was injected into a dispatch"
pass "mocked valid identity dispatches all four workflows without force_target"

new_case malformed_head
GENERATED_HEAD_SHA_OVERRIDE=not-a-sha run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "malformed action head output was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "malformed action head output fails closed before dispatch"

new_case cross_repository
printf -v MOCK_PR_METADATA '%s\t%s\t%s' evil-owner/other-repo "$repository" main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "cross-repository generated head was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "cross-repository generated head fails closed"

new_case wrong_branch
GENERATED_HEAD_REF_OVERRIDE=automation/other-branch \
  run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "wrong generated branch output was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "wrong generated branch output fails closed"

new_case wrong_base_repository
printf -v MOCK_PR_METADATA '%s\t%s\t%s' "$repository" evil-owner/other-repo main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "wrong generated base repository was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "wrong generated base repository fails closed"

new_case wrong_base_ref
printf -v MOCK_PR_METADATA '%s\t%s\t%s' "$repository" "$repository" develop
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "wrong generated base branch was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "wrong generated base branch fails closed"

new_case api_failure
MOCK_API_FAILURE=1 run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "GitHub API failure was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "GitHub API failure fails closed before dispatch"

new_case multi_parent
printf -v MOCK_COMMIT_METADATA '2\t%s' "$source_sha"
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "multi-parent generated head was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "multi-parent generated head fails closed"

new_case dispatch_failure
MOCK_FAIL_WORKFLOW=ripr.yml run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "dispatch failure was incorrectly reported as success"
[[ "$(count_workflow_calls "$CASE_LOG")" -eq 4 ]] \
  || fail "a dispatch failure prevented later workflows from being attempted"
! grep -Fq 'force_target' "$CASE_LOG" \
  || fail "force_target was injected during continuation"
pass "one mocked dispatch failure preserves all four attempts and fails"

# These workflow assertions keep the production wiring tied to the action's
# immutable outputs and prevent a future edit from moving the dispatcher back
# into mutable head/ref API lookups or adding router overrides.
grep -Fq 'GENERATED_HEAD_SHA: ${{ steps.create-pr.outputs.pull-request-head-sha }}' "$WORKFLOW" \
  || fail "workflow does not consume pull-request-head-sha output"
grep -Fq 'GENERATED_HEAD_REF: ${{ steps.create-pr.outputs.pull-request-branch }}' "$WORKFLOW" \
  || fail "workflow does not consume pull-request-branch output"
grep -Fq 'EXPECTED_BASE_REPOSITORY: ${{ github.repository }}' "$WORKFLOW" \
  || fail "workflow does not pass expected base repository"
grep -Fq 'EXPECTED_BASE_REF: ${{ github.event.repository.default_branch }}' "$WORKFLOW" \
  || fail "workflow does not pass expected base branch"
! grep -Fq '.head.sha' "$DISPATCHER" \
  || fail "dispatcher restored mutable head SHA lookup"
! grep -Fq '.head.ref' "$DISPATCHER" \
  || fail "dispatcher restored mutable head branch lookup"
! grep -Fq 'force_target' "$DISPATCHER" \
  || fail "dispatcher contains a force_target override"
pass "workflow wiring uses immutable action outputs and no force_target override"

printf 'PASS: executable mocked-gh post-merge dispatch contract\n'
