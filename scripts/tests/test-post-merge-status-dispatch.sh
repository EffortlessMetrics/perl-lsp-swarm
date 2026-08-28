#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/post-merge-status.yml"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

dispatch_block="$(awk '
  /- name: Raise CI on the generated PR/ { in_block = 1 }
  in_block { print }
  in_block && /^  [A-Za-z0-9_-]+:/ && $0 !~ /Raise CI/ { exit }
' "$WORKFLOW")"

grep -Fq 'for workflow in ci.yml em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do' <<<"$dispatch_block" \
  || fail 'the producer must retain all four workflow attempts'
grep -Fq 'gh workflow run "$workflow" --ref "$BRANCH" \' <<<"$dispatch_block" \
  || fail 'ci.yml must be dispatched from the generated branch'
grep -Fq -- '-f "base_sha=$VERIFIED_SOURCE_SHA"' <<<"$dispatch_block" \
  || fail 'ci.yml must receive the verified source base_sha'
grep -Fq -- '-f "head_sha=$GENERATED_HEAD_SHA"' <<<"$dispatch_block" \
  || fail 'ci.yml must receive the generated PR head_sha'
grep -Fq -- 'elif gh workflow run "$workflow" --ref "$BRANCH" \' <<<"$dispatch_block" \
  || fail 'the non-ci workflows must be dispatched from the generated branch'
grep -Fq -- '-f "expected_head_sha=$GENERATED_HEAD_SHA"' <<<"$dispatch_block" \
  || fail 'the three non-ci workflows must receive the generated PR head_sha'
grep -Fq 'generated_parent_sha" != "$VERIFIED_SOURCE_SHA"' <<<"$dispatch_block" \
  || fail 'the generated head parent must be checked against the verified source'
grep -Fq 'generated_parent_count" != "1"' <<<"$dispatch_block" \
  || fail 'multi-parent generated heads must be rejected'

ci_workflow="$ROOT/.github/workflows/ci.yml"
grep -Fq 'EXPECTED_HEAD_SHA: ${{ inputs.head_sha }}' "$ci_workflow" \
  || fail 'ci.yml production jobs must consume the producer-supplied head_sha'
grep -Fq 'Verify dispatched CI subject' "$ci_workflow" \
  || fail 'ci.yml must inline the dispatch subject check in an existing production job'
grep -Fq 'Verify dispatched formatter subject' "$ci_workflow" \
  || fail 'Rust formatting must inline the dispatch subject check'
grep -Fq 'if [[ ! "$EXPECTED_HEAD_SHA" =~ ^[0-9a-f]{40}$ || "$GITHUB_SHA" != "$EXPECTED_HEAD_SHA" ]]' "$ci_workflow" \
  || fail 'ci.yml must fail closed on malformed or raced dispatch subjects'

for workflow in ci.yml em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  workflow_path="$ROOT/.github/workflows/$workflow"
  if grep -Fq 'dispatch-subject:' "$workflow_path" || grep -Fq 'needs: dispatch-subject' "$workflow_path"; then
    fail "$workflow must not add a dispatch-subject prerequisite job"
  fi
done
grep -Fq 'Verify dispatched Rust subject' "$ROOT/.github/workflows/em-ci-routed-rust.yml" \
  || fail 'EM Rust route must inline the dispatch subject check'
grep -Fq 'Verify dispatched ripr subject' "$ROOT/.github/workflows/ripr.yml" \
  || fail 'ripr route must inline the dispatch subject check'
grep -Fq 'Verify dispatched title-check subject' "$ROOT/.github/workflows/pr-title-check.yml" \
  || fail 'title validation must inline the dispatch subject check'

source_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
head_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb

identity_is_valid() {
  local source="$1"
  local head="$2"
  local parent="$3"
  [[ "$source" =~ ^[0-9a-f]{40}$ ]] \
    && [[ "$head" =~ ^[0-9a-f]{40}$ ]] \
    && [[ "$parent" == "$source" ]] \
    && [[ "$head" != "$source" ]]
}

dispatch_args_are_valid() {
  local workflow="$1"
  local base_sha="${2:-}"
  local generated_head_sha="${3:-}"
  if [[ "$workflow" == "ci.yml" ]]; then
    [[ "$base_sha" == "$source_sha" && "$generated_head_sha" == "$head_sha" ]]
  else
    [[ -z "$base_sha" && -z "$generated_head_sha" ]]
  fi
}

identity_is_valid "$source_sha" "$head_sha" "$source_sha" \
  || fail 'valid source/head/parent identity was rejected'
identity_is_valid "$source_sha" '' "$source_sha" \
  && fail 'missing head identity was accepted'
identity_is_valid "$source_sha" "$head_sha" cccccccccccccccccccccccccccccccccccccccc \
  && fail 'mismatched parent identity was accepted'
identity_is_valid "$source_sha" "$source_sha" "$source_sha" \
  && fail 'reused source/head identity was accepted'

dispatch_args_are_valid ci.yml "$source_sha" "$head_sha" \
  || fail 'ci.yml lost its exact base/head arguments'
dispatch_args_are_valid ci.yml '' "$head_sha" \
  && fail 'omitted ci.yml base_sha was accepted'
dispatch_args_are_valid ci.yml "$source_sha" '' \
  && fail 'omitted ci.yml head_sha was accepted'
dispatch_args_are_valid ci.yml cccccccccccccccccccccccccccccccccccccccc "$head_sha" \
  && fail 'mismatched ci.yml base_sha was accepted'

# Model the production gate's fail-closed ordering: identity is checked before
# any dispatch, and a race or malformed identity prevents all four attempts.
dispatch_log=()
dispatch_workflows() {
  local observed="$1"
  local expected="$2"
  local base="$3"
  dispatch_log=()
  if [[ ! "$expected" =~ ^[0-9a-f]{40}$ || "$observed" != "$expected" || "$base" != "$source_sha" ]]; then
    return 1
  fi
  for workflow in ci.yml em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
    dispatch_log+=("$workflow")
  done
}

dispatch_workflows "$head_sha" "$head_sha" "$source_sha" \
  || fail 'valid immutable dispatch identity was rejected'
[[ "${#dispatch_log[@]}" -eq 4 ]] \
  || fail 'valid identity did not preserve all four dispatches'
dispatch_workflows cccccccccccccccccccccccccccccccccccccccc "$head_sha" "$source_sha" \
  && fail 'branch movement after producer validation was accepted'
[[ "${#dispatch_log[@]}" -eq 0 ]] \
  || fail 'race failure dispatched a workflow before failing closed'
dispatch_workflows "$head_sha" not-a-sha "$source_sha" \
  && fail 'malformed producer identity was accepted'
[[ "${#dispatch_log[@]}" -eq 0 ]] \
  || fail 'malformed identity dispatched a workflow before failing closed'

# Once identity is valid, a single gh failure must not prevent later required
# workflows from being attempted; the production loop records failure and
# continues.  This is intentionally separate from the pre-dispatch fail-close.
attempted=()
failed=0
for workflow in ci.yml em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  attempted+=("$workflow")
  if [[ "$workflow" == "ripr.yml" ]]; then
    failed=1
  fi
done
[[ "${#attempted[@]}" -eq 4 && "$failed" -eq 1 ]] \
  || fail 'one dispatch failure did not preserve later workflow attempts'
for workflow in em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  dispatch_args_are_valid "$workflow" \
    || fail "$workflow dispatch inputs changed"
done

printf 'PASS: post-merge-status dispatch identity and four-workflow contract\n'
