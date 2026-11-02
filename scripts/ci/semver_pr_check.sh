#!/usr/bin/env bash
# PR-time cargo-semver-checks loop over the API ratchet list (#14607).
#
# Detected breaks must fail this step (exit non-zero) so the check-run conclusion
# is truthful (#17208). The rail stays merge-advisory via required=false in
# .ci/policies/required-checks.toml, not by reporting success after detection.
set -euo pipefail

repo_root="${1:-.}"
cd "${repo_root}"

: "${SEMVER_BASELINE:?SEMVER_BASELINE must be set to a release tag rev}"

ratchet_file=".ci/public-api-baselines/ratchet-crates.txt"
mapfile -t crates < <(
  sed -e 's/#.*$//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' "${ratchet_file}" | grep -v '^$'
)

if [ "${#crates[@]}" -eq 0 ]; then
  echo "::error::.ci/public-api-baselines/ratchet-crates.txt lists no crates; refusing a vacuous semver pass"
  exit 1
fi
if [ -n "$(printf '%s\n' "${crates[@]}" | sort | uniq -d)" ]; then
  echo "::error::.ci/public-api-baselines/ratchet-crates.txt lists a crate twice; fix the list rather than checking it twice"
  exit 1
fi

breaking=0
for crate in "${crates[@]}"; do
  if cargo semver-checks check-release -p "${crate}" \
    --baseline-rev "${SEMVER_BASELINE}"; then
    echo "semver: ${crate} compatible"
  else
    echo "::warning::Breaking changes detected in ${crate}"
    breaking=$((breaking + 1))
  fi
done

if [ "${breaking}" -gt 0 ]; then
  echo "::error::semver-pr: ${breaking} ratchet crate(s) have breaking changes vs ${SEMVER_BASELINE} (advisory rail; conclusion must not read as pass when breaks were detected #17208)"
  exit 1
fi
