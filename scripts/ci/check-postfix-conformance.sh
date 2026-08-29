#!/usr/bin/env bash
set -euo pipefail

fixture=".ci/postfix-conformance/postfix-modifiers.pl"
expected='if|unless|while:1|while:2|until:1|until:2|for:ALPHA,BETA|foreach:1|foreach:2|foreach:3'

test -f "$fixture"
sha256sum "$fixture"
perl -V:version
printf 'runtime=%s\n' "$(perl -e 'print $^V')"
printf 'fixture=%s\n' "$fixture"

perl -c "$fixture" >/dev/null
actual="$(perl "$fixture")"
if [[ "$actual" != "$expected" ]]; then
  printf 'postfix conformance mismatch\nexpected=%s\nactual=%s\n' "$expected" "$actual" >&2
  exit 1
fi

# Opposite-direction control: a changed fixture result must not be accepted.
if [[ "$actual" == "if|unless|while:1|while:2|until:1|until:2|for:ALPHA,BETA|foreach:1|foreach:2" ]]; then
  printf 'postfix conformance control unexpectedly accepted truncated output\n' >&2
  exit 1
fi

printf 'postfix conformance passed\n'
