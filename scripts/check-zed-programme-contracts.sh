#!/usr/bin/env bash
# Run every Zed programme contract present in the current stacked branch.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly CANDIDATE_SOURCE="$REPO_ROOT/.ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs"
cd "$REPO_ROOT"

if grep -Fq 'remove_old_downloads("perllsp-"' "$CANDIDATE_SOURCE"; then
  echo "error: staged perllsp route deletes older managed versions before Zed proves launch success" >&2
  exit 1
fi
if ! grep -Fq 'Retain older perllsp versions' "$CANDIDATE_SOURCE"; then
  echo "error: staged perllsp route does not record the known-good preservation boundary" >&2
  exit 1
fi

shopt -s nullglob

checks=(scripts/check-zed-*.sh)
for check in "${checks[@]}"; do
  if [[ "$check" == "scripts/check-zed-programme-contracts.sh" ]]; then
    continue
  fi
  echo "==> $check"
  bash "$check"
done

tests=(xtask/tests/zed_*.rs)
if [[ ${#tests[@]} -eq 0 ]]; then
  echo "error: no Zed xtask contract tests found" >&2
  exit 1
fi

for test_path in "${tests[@]}"; do
  test_name="$(basename "$test_path" .rs)"
  echo "==> rustfmt $test_path"
  rustfmt --edition 2024 --check "$test_path"
  echo "==> cargo test -p xtask --test $test_name"
  cargo test -p xtask --test "$test_name" --locked
done
