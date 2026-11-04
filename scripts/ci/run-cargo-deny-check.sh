#!/usr/bin/env bash
# Canonical cargo-deny invocation for policy gates (#17686).
#
# cargo-deny treats vacuous deny.toml skip entries as warning[unmatched-skip]
# on stderr while still exiting 0. This wrapper fails closed so the skip list
# cannot rot silently.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

if [ "$#" -eq 0 ]; then
  set -- --locked check advisories licenses bans sources
fi

: "${CARGO_DENY:=cargo-deny}"

if ! command -v "$CARGO_DENY" >/dev/null 2>&1; then
  printf 'UNAVAILABLE: %s not found on PATH\n' "$CARGO_DENY" >&2
  exit 3
fi

stderr_log="$(mktemp)"
trap 'rm -f "$stderr_log"' EXIT

set +e
"$CARGO_DENY" "$@" 2>"$stderr_log"
rc=$?
set -e

cat "$stderr_log" >&2
bash "$REPO_ROOT/scripts/ci/assert-no-cargo-deny-unmatched-skip.sh" "$stderr_log"
exit "$rc"
