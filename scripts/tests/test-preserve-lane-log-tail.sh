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

# Small logs copy verbatim.
printf 'alpha\nomega\n' >"$WORK/small.log"
bash "$PRESERVE" "$WORK/small.log" "$WORK/small.out"
require_eq "small copy" "$(cat "$WORK/small.log")" "$(cat "$WORK/small.out")"

# Large logs keep an exact marked 4 MiB tail.
MAX_BYTES=$((4 * 1024 * 1024))
head_bytes=$((MAX_BYTES + 128))
printf '%*s\n' "$head_bytes" '' | tr ' ' 'x' >"$WORK/large.log"
printf 'final-line-marker\n' >>"$WORK/large.log"
size=$(wc -c <"$WORK/large.log" | tr -d ' ')
{
  printf '[lane log truncated to last %s bytes of %s]\n' "$MAX_BYTES" "$size"
  tail -c "$MAX_BYTES" "$WORK/large.log"
} >"$WORK/large.expected"
bash "$PRESERVE" "$WORK/large.log" "$WORK/large.out"
require_eq "large tail bytes" "$(cat "$WORK/large.expected")" "$(cat "$WORK/large.out")"

if bash "$PRESERVE" "$WORK/large.log" "$WORK/large.log"; then
  printf 'FAIL same-path: preserve must refuse identical logfile and outfile\n' >&2
  exit 1
fi

printf 'ok: preserve-lane-log-tail\n'
