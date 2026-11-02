#!/usr/bin/env bash
# Preserve the tail of a hosted ripr evidence lane log for post-timeout diagnosis.
# GitHub's job log UI often truncates to an early build prefix; the gate already
# retrieves the lane log via the Actions API (#6807). This helper keeps the last
# four mebibytes so the retrieval is durable in workflow artifacts (#17450).
set -euo pipefail

usage() {
  printf 'usage: %s <logfile> <outfile>\n' "${0##*/}" >&2
}

if [ "$#" -ne 2 ]; then
  usage
  exit 2
fi

src=$1
dest=$2

if [ ! -f "$src" ]; then
  printf 'error: logfile %s is missing\n' "$src" >&2
  exit 2
fi

MAX_BYTES=$((4 * 1024 * 1024))
size=$(wc -c <"$src" | tr -d ' ')
case "$size" in
  ''|*[!0-9]*)
    printf 'error: could not read byte size of %s\n' "$src" >&2
    exit 2
    ;;
esac

if [ "$size" -le "$MAX_BYTES" ]; then
  cp "$src" "$dest"
  exit 0
fi

{
  printf '[lane log truncated to last %s bytes of %s]\n' "$MAX_BYTES" "$size"
  tail -c "$MAX_BYTES" "$src"
} >"$dest"
