#!/usr/bin/env bash

set -euo pipefail

: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
: "${VERIFIED_SOURCE_SHA:?VERIFIED_SOURCE_SHA is required}"
: "${GENERATED_PR_NUMBER:?GENERATED_PR_NUMBER is required}"
: "${GENERATED_HEAD_SHA:?GENERATED_HEAD_SHA is required}"
: "${GENERATED_HEAD_REF:?GENERATED_HEAD_REF is required}"
: "${EXPECTED_HEAD_REF:?EXPECTED_HEAD_REF is required}"
: "${EXPECTED_BASE_REPOSITORY:?EXPECTED_BASE_REPOSITORY is required}"
: "${EXPECTED_BASE_REF:?EXPECTED_BASE_REF is required}"

if [[ "$VERIFIED_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  :
else
  echo "::error::Verified generation source is not a 40-character commit SHA"
  exit 1
fi
if [[ "$GENERATED_PR_NUMBER" =~ ^[1-9][0-9]*$ ]]; then
  :
else
  echo "::error::Generated PR number is missing or malformed"
  exit 1
fi
if [[ "$GENERATED_HEAD_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  :
else
  echo "::error::Generated PR head output is missing or malformed"
  exit 1
fi
if [[ "$GENERATED_HEAD_REF" != "$EXPECTED_HEAD_REF" ]]; then
  echo "::error::Generated PR branch output is not the expected automation branch"
  exit 1
fi

# The action outputs are the authority for the generated PR head SHA and branch.
# Query the PR only for repository and base metadata, which the action does not
# expose. A failed or incomplete query stops before any workflow is dispatched.
generated_metadata="$(
  gh api \
    "repos/${GITHUB_REPOSITORY}/pulls/${GENERATED_PR_NUMBER}" \
    --jq '[.head.repo.full_name, .base.repo.full_name, .base.ref] | @tsv'
)"
IFS=$'\t' read -r generated_head_repo generated_base_repo generated_base_ref <<< "$generated_metadata"
if [[ -z "$generated_head_repo" || -z "$generated_base_repo" || -z "$generated_base_ref" ]]; then
  echo "::error::Generated PR metadata is incomplete"
  exit 1
fi
if [[ "$generated_head_repo" != "$GITHUB_REPOSITORY" ]]; then
  echo "::error::Generated PR head is not in the expected repository"
  exit 1
fi
if [[ "$generated_base_repo" != "$EXPECTED_BASE_REPOSITORY" ||
      "$generated_base_ref" != "$EXPECTED_BASE_REF" ]]; then
  echo "::error::Generated PR base is not the expected repository and branch"
  exit 1
fi

# The action output identifies the exact commit to dispatch. Verify its
# ancestry separately so a recreated or multi-parent generated commit cannot
# receive the required checks under an untrusted subject.
commit_metadata="$(
  gh api \
    "repos/${GITHUB_REPOSITORY}/commits/${GENERATED_HEAD_SHA}" \
    --jq '[.parents | length, .parents[0].sha] | @tsv'
)"
IFS=$'\t' read -r generated_parent_count generated_parent_sha <<< "$commit_metadata"
if [[ "$generated_parent_count" != "1" ||
      "$generated_parent_sha" != "$VERIFIED_SOURCE_SHA" ]]; then
  echo "::error::Generated PR head is not a single child of the verified generation source"
  exit 1
fi

# Keep attempting every required workflow so one transient dispatch failure
# cannot strand the later required checks. The final status remains failing
# when any dispatch failed.
set +e
failed=0
for workflow in ci.yml em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
  if [[ "$workflow" == "ci.yml" ]]; then
    if gh workflow run "$workflow" --ref "$GENERATED_HEAD_REF" \
      -f "base_sha=$VERIFIED_SOURCE_SHA" \
      -f "head_sha=$GENERATED_HEAD_SHA"; then
      continue
    fi
  elif gh workflow run "$workflow" --ref "$GENERATED_HEAD_REF"; then
    continue
  fi
  echo "::error::Failed to dispatch $workflow"
  failed=1
done
exit "$failed"
