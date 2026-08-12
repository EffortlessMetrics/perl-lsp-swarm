#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage: verify-staged-binaries.sh SERVER [DAP] VERSION TARGET [CANDIDATE] RECEIPT

Runs the shared machine identity verifier before an installer promotes staged
perllsp/perl-dap bytes. The caller remains responsible for atomic replacement.
USAGE
    exit 64
}

[ "$#" -ge 5 ] && [ "$#" -le 6 ] || usage

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
SERVER="$1"
shift

if [ "$#" -eq 5 ]; then
    DAP="$1"
    shift
else
    DAP=""
fi

VERSION="$1"
TARGET="$2"
CANDIDATE="${3:-}"
RECEIPT="${4:-${3:-}}"

command -v python3 >/dev/null 2>&1 || {
    printf '%s\n' 'error: python3 is required for staged identity verification' >&2
    exit 2
}

args=(
    "$SCRIPT_DIR/verify_binary_identity.py"
    --server "$SERVER"
    --expected-version "$VERSION"
    --expected-target "$TARGET"
    --receipt "$RECEIPT"
)

if [ -n "$DAP" ]; then
    args+=(--dap "$DAP" --require-dap)
fi
if [ -n "$CANDIDATE" ]; then
    args+=(--expected-candidate "$CANDIDATE")
fi

exec python3 "${args[@]}"
