#!/usr/bin/env bash
# verify_install must fail closed: a binary that is present and executable
# but cannot run `--version` is corrupt, and the installer must not print
# success for it (#16310).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0
LAST_STATUS=0
LAST_OUTPUT=""

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail_case() {
    printf 'FAIL  %s\n%s\n' "$1" "$2" >&2
    FAIL=$((FAIL + 1))
}

PERL_LSP_INSTALLER_LIBRARY_ONLY=1
# shellcheck source=scripts/install.sh
source "$INSTALLER"

BIN_NAME="perllsp"

run_verify() {
    set +e
    LAST_OUTPUT="$(verify_install 2>&1)"
    LAST_STATUS=$?
    set -e
}

# A healthy binary verifies cleanly.
mkdir -p "$TMP/healthy"
printf '#!/bin/sh\necho "perllsp 0.0.0-test"\n' > "$TMP/healthy/$BIN_NAME"
chmod +x "$TMP/healthy/$BIN_NAME"
INSTALL_DIR="$TMP/healthy"
run_verify
if [ "$LAST_STATUS" -eq 0 ] && [[ "$LAST_OUTPUT" == *"verified:"* ]]; then
    pass "healthy binary verifies cleanly"
else
    fail_case "healthy binary verifies cleanly" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

# A corrupt-but-executable binary must fail, naming the binary, not warn.
mkdir -p "$TMP/corrupt"
printf '#!/bin/sh\nexit 127\n' > "$TMP/corrupt/$BIN_NAME"
chmod +x "$TMP/corrupt/$BIN_NAME"
INSTALL_DIR="$TMP/corrupt"
run_verify
if [ "$LAST_STATUS" -ne 0 ] && [[ "$LAST_OUTPUT" == *"$BIN_NAME --version"* ]]; then
    pass "corrupt binary fails closed and names the binary"
else
    fail_case "corrupt binary fails closed and names the binary" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf 'summary: PASS=%d FAIL=%d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
