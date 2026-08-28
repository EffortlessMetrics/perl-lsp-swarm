#!/usr/bin/env bash
# Dispatch every required check for the generated-status proposal.
#
# The base SHA is emitted only after the payload manifest is verified against
# the immutable workflow source. Refuse before any dispatch if that binding is
# missing, malformed, or no longer agrees with the source transaction.

set -uo pipefail

if [[ "$#" -ne 4 ]]; then
    echo "usage: $0 <branch> <verified-base-sha> <expected-source-sha> <generated-head-sha>" >&2
    exit 2
fi

branch="$1"
base_sha="$2"
expected_source_sha="$3"
head_sha="$4"

if [[ -z "$branch" ]]; then
    echo "generated-status dispatch: branch is empty" >&2
    exit 2
fi
if [[ ! "$base_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "generated-status dispatch: verified base SHA is not a lowercase 40-hex object name" >&2
    exit 2
fi
if [[ ! "$expected_source_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "generated-status dispatch: expected source SHA is not a lowercase 40-hex object name" >&2
    exit 2
fi
if [[ "$base_sha" != "$expected_source_sha" ]]; then
    echo "generated-status dispatch: verified base SHA does not match the source transaction" >&2
    exit 2
fi
if [[ ! "$head_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "generated-status dispatch: generated head SHA is not a lowercase 40-hex object name" >&2
    exit 2
fi
if ! command -v gh >/dev/null 2>&1; then
    echo "generated-status dispatch: gh is unavailable" >&2
    exit 2
fi

# Keep attempting every required workflow so one transient dispatch failure
# cannot strand later required checks. Only ci.yml accepts the immutable base
# input; the other workflow argv stays unchanged. Fail the step at the end if
# any individual dispatch failed.
set +e
failed=0
for workflow in ci.yml em-ci-routed-rust.yml ripr.yml pr-title-check.yml; do
    if [[ "$workflow" == "ci.yml" ]]; then
        gh workflow run "$workflow" --ref "$branch" \
            -f "base_sha=$base_sha" -f "head_sha=$head_sha"
    else
        gh workflow run "$workflow" --ref "$branch"
    fi
    if [[ "$?" -ne 0 ]]; then
        echo "::error::Failed to dispatch $workflow"
        failed=1
    fi
done
exit "$failed"
