#!/usr/bin/env bash
# Run: bash scripts/tests/test-preserve-lane-log-tail.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PRESERVE="${REPO_ROOT}/scripts/ci/preserve-lane-log-tail.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

require_eq() {
  local label=$1 expected=$2 actual=$3
  if [ "$expected" != "$actual" ]; then
    printf 'FAIL %s: expected %q got %q\n' "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

require_contains() {
  local label=$1 needle=$2 haystack=$3
  if ! printf '%s' "$haystack" | grep -Fq "$needle"; then
    printf 'FAIL %s: missing %q in:\n%s\n' "$label" "$needle" "$haystack" >&2
    exit 1
  fi
}

# Small logs copy verbatim.
printf 'alpha\nomega\n' >"$WORK/small.log"
bash "$PRESERVE" "$WORK/small.log" "$WORK/small.out"
require_eq "small copy" "$(cat "$WORK/small.log")" "$(cat "$WORK/small.out")"

# Large logs keep a marked tail with the final line intact.
MAX_BYTES=$((4 * 1024 * 1024))
head_bytes=$((MAX_BYTES + 128))
printf '%*s\n' "$head_bytes" '' | tr ' ' 'x' >"$WORK/large.log"
printf 'final-line-marker\n' >>"$WORK/large.log"
bash "$PRESERVE" "$WORK/large.log" "$WORK/large.out"
require_contains "truncation banner" "[lane log truncated to last ${MAX_BYTES} bytes" "$(head -1 "$WORK/large.out")"
require_contains "tail marker" "final-line-marker" "$(tail -1 "$WORK/large.out")"

printf 'ok: preserve-lane-log-tail\n'
