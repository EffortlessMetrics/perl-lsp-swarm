#!/usr/bin/env bash
set -euo pipefail

fixture=".ci/postfix-conformance/postfix-modifiers.pl"
expected='if|unless|while:1|while:2|until:1|until:2|for:ALPHA,BETA|foreach:1|foreach:2|foreach:3'

matches_expected() {
  [[ "$1" == "$expected" ]]
}

test -f "$fixture"
sha256sum "$fixture"
# Record the selected interpreter's version, build configuration and platform;
# a matrix version selector alone is not an identity for the executed runtime.
perl -V
printf 'runtime=%s\n' "$(perl -e 'print $^V')"
printf 'fixture=%s\n' "$fixture"

perl -c "$fixture" >/dev/null
actual="$(perl "$fixture")"
if ! matches_expected "$actual"; then
  printf 'postfix conformance mismatch\nexpected=%s\nactual=%s\n' "$expected" "$actual" >&2
  exit 1
fi

# Exercise the same predicate with a missing final iteration. Comparing the
# already-accepted actual output to a different string cannot test rejection.
truncated="${expected%|foreach:3}"
if matches_expected "$truncated"; then
  printf 'postfix conformance control unexpectedly accepted truncated output\n' >&2
  exit 1
fi
printf 'postfix conformance control rejected truncated output\n'

printf 'postfix conformance passed\n'
