#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DISPATCHER="$ROOT/scripts/ci/dispatch-post-merge-status.sh"
WORKFLOW="$ROOT/.github/workflows/post-merge-status.yml"
CI_WORKFLOW="$ROOT/.github/workflows/ci.yml"
RUST_WORKFLOW="$ROOT/.github/workflows/em-ci-routed-rust.yml"
RIPR_WORKFLOW="$ROOT/.github/workflows/ripr.yml"
TITLE_WORKFLOW="$ROOT/.github/workflows/pr-title-check.yml"
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
[[ -f "$RUST_WORKFLOW" ]] || fail "routed Rust workflow is missing"
[[ -f "$RIPR_WORKFLOW" ]] || fail "ripr workflow is missing"
[[ -f "$TITLE_WORKFLOW" ]] || fail "title-check workflow is missing"

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
    printf '%s\n' "${MOCK_PR_METADATA:?}"
    exit 0
  fi
  if [[ "$*" == *"/git/ref/heads/"* ]]; then
    ref_call=0
    if [[ -f "$MOCK_REF_STATE" ]]; then
      ref_call=$(<"$MOCK_REF_STATE")
    fi
    ref_call=$((ref_call + 1))
    printf '%s\n' "$ref_call" > "$MOCK_REF_STATE"
    IFS=, read -r -a ref_sequence <<< "${MOCK_REF_SEQUENCE:?}"
    ref_index=$((ref_call - 1))
    if (( ref_index >= ${#ref_sequence[@]} )); then
      ref_index=$((${#ref_sequence[@]} - 1))
    fi
    printf '%s\n' "${ref_sequence[$ref_index]}"
    exit 0
  fi
  if [[ "$*" == *"/commits/"* ]]; then
    printf '%s\n' "${MOCK_COMMIT_METADATA:?}"
    exit 0
  fi
  exit 43
fi

if [[ "${1:-}" == "workflow" && "${2:-}" == "run" ]]; then
  case "${3:-}" in
    ci.yml)
      expected="workflow run ci.yml --ref ${MOCK_EXPECTED_REF:?} -f base_sha=${MOCK_EXPECTED_BASE_SHA:?} -f head_sha=${MOCK_EXPECTED_HEAD_SHA:?} -f expected_head_sha=${MOCK_EXPECTED_HEAD_SHA:?}"
      ;;
    em-ci-routed-rust.yml|ripr.yml|pr-title-check.yml)
      expected="workflow run $3 --ref ${MOCK_EXPECTED_REF:?} -f expected_head_sha=${MOCK_EXPECTED_HEAD_SHA:?}"
      ;;
    *)
      exit 46
      ;;
  esac
  if [[ "$*" != "$expected" ]]; then
    exit 47
  fi
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
    MOCK_REF_SEQUENCE="$MOCK_REF_SEQUENCE" \
    MOCK_REF_STATE="$MOCK_REF_STATE" \
    MOCK_EXPECTED_REF="$automation_ref" \
    MOCK_EXPECTED_BASE_SHA="$source_sha" \
    MOCK_EXPECTED_HEAD_SHA="$head_sha" \
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
  MOCK_REF_STATE="$CASE_DIR/ref-calls"
  unset GENERATED_HEAD_SHA_OVERRIDE GENERATED_HEAD_REF_OVERRIDE
  unset MOCK_API_FAILURE MOCK_FAIL_WORKFLOW MOCK_REF_SEQUENCE
  MOCK_REF_SEQUENCE="$head_sha"
  printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s\t%s' \
    "$repository" "$automation_ref" "$head_sha" "$repository" main
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
grep -Fxq "gh workflow run ci.yml --ref $automation_ref -f base_sha=$source_sha -f head_sha=$head_sha -f expected_head_sha=$head_sha" "$CASE_LOG" \
  || fail "ci.yml did not receive the exact subject inputs"
for workflow in em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  grep -Fxq "gh workflow run $workflow --ref $automation_ref -f expected_head_sha=$head_sha" "$CASE_LOG" \
    || fail "$workflow did not receive the exact expected head input"
done
! grep -Fq 'force_target' "$CASE_LOG" \
  || fail "force_target was injected into a dispatch"
pass "mocked valid identity dispatches four exact subject-bound workflows without force_target"

new_case malformed_head
GENERATED_HEAD_SHA_OVERRIDE=not-a-sha run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "malformed action head output was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "malformed action head output fails closed before dispatch"

new_case cross_repository
printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s\t%s' \
  evil-owner/other-repo "$automation_ref" "$head_sha" "$repository" main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "cross-repository generated head was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "cross-repository generated head fails closed"

new_case missing_pr_head_identity
printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s' \
  "$repository" "$automation_ref" "$repository" main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "incomplete generated PR identity was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "incomplete generated PR identity fails closed"

new_case mismatched_pr_head_ref
printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s\t%s' \
  "$repository" automation/other-branch "$head_sha" "$repository" main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "mismatched generated PR branch was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "mismatched generated PR branch fails closed"

new_case mismatched_pr_head_sha
printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s\t%s' \
  "$repository" "$automation_ref" cccccccccccccccccccccccccccccccccccccccc "$repository" main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "mismatched generated PR SHA was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "mismatched generated PR SHA fails closed"

new_case initial_ref_mismatch
MOCK_REF_SEQUENCE=cccccccccccccccccccccccccccccccccccccccc
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "moved generated branch was accepted before dispatch"
assert_no_workflow_calls "$CASE_LOG"
pass "initial generated branch movement fails closed"

new_case ref_moves_between_dispatches
MOCK_REF_SEQUENCE="$head_sha,$head_sha,cccccccccccccccccccccccccccccccccccccccc"
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "generated branch movement between dispatches was accepted"
[[ "$(count_workflow_calls "$CASE_LOG")" -eq 1 ]] \
  || fail "branch race did not stop before the second dispatch"
grep -Fxq "gh workflow run ci.yml --ref $automation_ref -f base_sha=$source_sha -f head_sha=$head_sha -f expected_head_sha=$head_sha" "$CASE_LOG" \
  || fail "branch race did not preserve the one validated first dispatch"
pass "generated branch movement between dispatches fails closed"

new_case wrong_branch
GENERATED_HEAD_REF_OVERRIDE=automation/other-branch \
  run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "wrong generated branch output was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "wrong generated branch output fails closed"

new_case wrong_base_repository
printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s\t%s' \
  "$repository" "$automation_ref" "$head_sha" evil-owner/other-repo main
run_dispatch "$CASE_DIR/bin" "$CASE_LOG" \
  && fail "wrong generated base repository was accepted"
assert_no_workflow_calls "$CASE_LOG"
pass "wrong generated base repository fails closed"

new_case wrong_base_ref
printf -v MOCK_PR_METADATA '%s\t%s\t%s\t%s\t%s' \
  "$repository" "$automation_ref" "$head_sha" "$repository" develop
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

new_case reject_unbound_workflow_arguments
PATH="$CASE_DIR/bin:$PATH" \
  MOCK_CALL_LOG="$CASE_LOG" \
  MOCK_EXPECTED_REF="$automation_ref" \
  MOCK_EXPECTED_BASE_SHA="$source_sha" \
  MOCK_EXPECTED_HEAD_SHA="$head_sha" \
  gh workflow run ripr.yml --ref "$automation_ref" \
  && fail "mock gh accepted a dispatch without expected_head_sha"
pass "mock gh rejects unbound workflow arguments"

# These workflow assertions keep the production wiring tied to the action's
# immutable outputs, validates both PR and ref identity, and keeps all four
# dispatch arguments subject-bound without adding router overrides.
grep -Fq 'GENERATED_HEAD_SHA: ${{ steps.create-pr.outputs.pull-request-head-sha }}' "$WORKFLOW" \
  || fail "workflow does not consume pull-request-head-sha output"
grep -Fq 'GENERATED_HEAD_REF: ${{ steps.create-pr.outputs.pull-request-branch }}' "$WORKFLOW" \
  || fail "workflow does not consume pull-request-branch output"
grep -Fq 'EXPECTED_BASE_REPOSITORY: ${{ github.repository }}' "$WORKFLOW" \
  || fail "workflow does not pass expected base repository"
grep -Fq 'EXPECTED_BASE_REF: ${{ github.event.repository.default_branch }}' "$WORKFLOW" \
  || fail "workflow does not pass expected base branch"
grep -Fq 'repos/${GITHUB_REPOSITORY}/git/ref/heads/${GENERATED_HEAD_REF}' "$DISPATCHER" \
  || fail "dispatcher does not revalidate the generated branch tip"
grep -Fq 'expected_head_sha=$GENERATED_HEAD_SHA' "$DISPATCHER" \
  || fail "dispatcher does not bind every generated dispatch to the expected head"
! grep -Fq 'force_target' "$DISPATCHER" \
  || fail "dispatcher contains a force_target override"
for receiver in "$CI_WORKFLOW" "$RUST_WORKFLOW" "$RIPR_WORKFLOW" "$TITLE_WORKFLOW"; do
  grep -Fq 'expected_head_sha:' "$receiver" \
    || fail "receiver $(basename "$receiver") does not accept expected_head_sha"
done
grep -Fq "if: github.event_name == 'workflow_dispatch' && needs.draft-pr-check.result != 'success'" "$CI_WORKFLOW" \
  || fail "CI aggregate does not fail after a dispatched entry-guard failure"
grep -Fq "if: github.event_name == 'workflow_dispatch' && needs.route-rust-small.result != 'success'" "$RUST_WORKFLOW" \
  || fail "Rust aggregate does not fail after a dispatched entry-guard failure"
grep -Fq "if: github.event_name == 'workflow_dispatch' && needs.route-ripr.result != 'success'" "$RIPR_WORKFLOW" \
  || fail "ripr aggregate does not fail after a dispatched entry-guard failure"
pass "workflow wiring uses output identity, ref revalidation, guarded aggregates, and no force_target override"

printf 'PASS: executable mocked-gh post-merge dispatch contract\n'
