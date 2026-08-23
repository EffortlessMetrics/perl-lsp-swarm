#!/usr/bin/env bash
# Run every Zed programme contract present in the current stacked branch.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

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
