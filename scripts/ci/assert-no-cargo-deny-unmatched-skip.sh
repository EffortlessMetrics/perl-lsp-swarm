#!/usr/bin/env bash
# Fail closed when cargo-deny reports vacuous deny.toml skip entries.
set -euo pipefail

if [ "$#" -gt 1 ]; then
  printf 'usage: %s [cargo-deny-stderr-file]\n' "$0" >&2
  exit 2
fi

if [ "$#" -eq 0 ]; then
  if grep -Fq 'unmatched-skip'; then
    printf 'FAIL: cargo-deny stderr contains unmatched-skip (vacuous deny.toml skip entry)\n' >&2
    exit 1
  fi
  exit 0
fi

target=$1
if [ ! -f "$target" ]; then
  printf 'FAIL: cargo-deny stderr log missing: %s\n' "$target" >&2
  exit 2
fi

if grep -Fq 'unmatched-skip' "$target"; then
  printf 'FAIL: cargo-deny stderr contains unmatched-skip (vacuous deny.toml skip entry)\n' >&2
  exit 1
fi
