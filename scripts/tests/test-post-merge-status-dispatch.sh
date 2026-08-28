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
grep -Fq 'generated_parent_sha" != "$VERIFIED_SOURCE_SHA"' <<<"$dispatch_block" \
  || fail 'the generated head parent must be checked against the verified source'
grep -Fq 'generated_parent_count" != "1"' <<<"$dispatch_block" \
  || fail 'multi-parent generated heads must be rejected'

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
for workflow in em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  dispatch_args_are_valid "$workflow" \
    || fail "$workflow dispatch inputs changed"
done

printf 'PASS: post-merge-status dispatch identity and four-workflow contract\n'
