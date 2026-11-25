#!/usr/bin/env bash
# Keep inherited values out of sudo/systemd-run argv and their command logs.
set -euo pipefail
case "${1-}" in primary|fallback) mode=$1 ;; *) echo 'invalid guidance mode' >&2; exit 2 ;; esac
umask 077
snapshot=$(mktemp /dev/shm/ripr-guidance-env.XXXXXXXX)
trap 'rm -f -- "$snapshot"' EXIT
# Do not launch an unbounded or partially configured command on write failure.
env -0 > "$snapshot"
sudo -n systemd-run --scope -p MemoryMax=14G -p MemorySwapMax=0 \
  --uid="$(id -un)" --gid="$(id -gn)" -- \
  python3 scripts/ci/ripr-hosted-guidance.py "$snapshot" "$mode"
