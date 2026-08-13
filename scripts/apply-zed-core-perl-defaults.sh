#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
patch="$repo_root/.ci/fixtures/zed-perl-upstream/zed-core/perl-defaults.patch"
expected_head="7733b9922665f103abda7c6a3fde6b9dfdc8eba9"
expected_blob="a03ad8874243f167e86deba8f975268eb384d20f"
target="assets/settings/default.json"

if [[ $# -ne 1 ]]; then
  echo "usage: $0 /path/to/zed" >&2
  exit 64
fi

checkout="$1"
[[ -d "$checkout/.git" ]] || { echo "error: not a Git checkout" >&2; exit 1; }
[[ -z "$(git -C "$checkout" status --porcelain)" ]] || { echo "error: checkout must be clean" >&2; exit 1; }
[[ "$(git -C "$checkout" rev-parse HEAD)" == "$expected_head" ]] || { echo "error: wrong Zed base" >&2; exit 1; }
[[ "$(git -C "$checkout" hash-object "$target")" == "$expected_blob" ]] || { echo "error: default settings blob drifted" >&2; exit 1; }

git -C "$checkout" apply --check "$patch"
git -C "$checkout" apply "$patch"
git -C "$checkout" diff --check

echo "Applied the staged Perl server-order patch. Review the resulting diff before submission."
