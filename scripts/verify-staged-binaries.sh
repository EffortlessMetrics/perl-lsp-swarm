#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage: verify-staged-binaries.sh \
  --server PATH [--dap PATH] \
  --expected-version VERSION --expected-target TARGET \
  [--expected-candidate CANDIDATE] --receipt PATH

Runs the shared machine identity verifier before an installer promotes staged
perllsp/perl-dap bytes. The caller remains responsible for atomic replacement.
USAGE
    exit 64
}

SERVER=""
DAP=""
VERSION=""
TARGET=""
CANDIDATE=""
RECEIPT=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --server|--dap|--expected-version|--expected-target|--expected-candidate|--receipt)
            [ "$#" -ge 2 ] || usage
            case "$2" in --*) usage ;; esac
            case "$1" in
                --server) SERVER="$2" ;;
                --dap) DAP="$2" ;;
                --expected-version) VERSION="$2" ;;
                --expected-target) TARGET="$2" ;;
                --expected-candidate) CANDIDATE="$2" ;;
                --receipt) RECEIPT="$2" ;;
            esac
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            printf 'error: unknown argument: %s\n' "$1" >&2
            usage
            ;;
    esac
done

[ -n "$SERVER" ] || usage
[ -n "$VERSION" ] || usage
[ -n "$TARGET" ] || usage
[ -n "$RECEIPT" ] || usage

# A stale receipt from an earlier run must never survive as this run's result.
rm -f -- "$RECEIPT"

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
PYTHON_BIN="${PERL_LSP_PYTHON:-python3}"
command -v "$PYTHON_BIN" >/dev/null 2>&1 || {
    printf 'error: Python 3 interpreter not found: %s\n' "$PYTHON_BIN" >&2
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

exec "$PYTHON_BIN" "${args[@]}"
