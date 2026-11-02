#!/usr/bin/env bash
# Owned-state uninstall proof for scripts/install.sh (#8372).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
HELPER="$ROOT/scripts/standalone_uninstall.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail_case() {
    printf 'FAIL  %s\n%s\n' "$1" "$2" >&2
    FAIL=$((FAIL + 1))
}

if ! command -v python3 >/dev/null 2>&1; then
    echo "SKIP  python3 required for owned-state uninstall tests" >&2
    exit 0
fi

PERL_LSP_INSTALLER_LIBRARY_ONLY=1
# shellcheck source=scripts/install.sh
source "$INSTALLER"

BIN_NAME="perllsp"
DAP_BIN_NAME="perl-dap"

stage_pair() {
    local dest="$1" server_payload="$2" dap_payload="$3"
    rm -rf "$dest"
    mkdir -p "$dest"
    printf '%s\n' "$server_payload" > "${dest}/${BIN_NAME}"
    chmod 755 "${dest}/${BIN_NAME}"
    printf '%s\n' "$dap_payload" > "${dest}/${DAP_BIN_NAME}"
    chmod 755 "${dest}/${DAP_BIN_NAME}"
}

setup_case() {
    CASE_ROOT="$TMP/run"
    rm -rf "$CASE_ROOT"
    mkdir -p "$CASE_ROOT/install" "$CASE_ROOT/stage"
    INSTALL_DIR="$CASE_ROOT/install"
    EXTRACT_DIR="$CASE_ROOT/stage"
}

run_uninstall() {
    local _status=0 _out
    set +e
    _out="$(python3 "$HELPER" uninstall --install-dir "$INSTALL_DIR" 2>&1)"
    _status=$?
    set -e
    LAST_OUT="$_out"
    LAST_STATUS="$_status"
}

# ── already absent ────────────────────────────────────────────────────────────

setup_case
run_uninstall
if [ "$LAST_STATUS" -eq 0 ] && [[ "$LAST_OUT" == *"result=already_absent_owned_state"* ]]; then
    pass "empty install dir reports already_absent_owned_state"
else
    fail_case "empty install dir" "status=$LAST_STATUS output=$LAST_OUT"
fi

# ── legacy store without manifest refuses destructive work ────────────────────

setup_case
mkdir -p "${INSTALL_DIR}/.perl-lsp/candidates/legacy-id"
printf 'x\n' > "${INSTALL_DIR}/.perl-lsp/candidates/legacy-id/${BIN_NAME}"
run_uninstall
if [ "$LAST_STATUS" -eq 2 ] && [[ "$LAST_OUT" == *"legacy_install_without_owned_manifest"* ]]; then
    pass "legacy store without manifest returns not_proven"
else
    fail_case "legacy refusal" "status=$LAST_STATUS output=$LAST_OUT"
fi
if [ -d "${INSTALL_DIR}/.perl-lsp/candidates/legacy-id" ]; then
    pass "legacy refusal preserves store contents"
else
    fail_case "legacy preservation" "store was mutated"
fi

# ── install, record manifest, uninstall, idempotent second pass ───────────────

setup_case
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
if ! install_binaries release >/dev/null 2>&1; then
    fail_case "install_binaries" "promotion failed"
else
    pass "install_binaries promotes product unit"
fi

if [ ! -f "${INSTALL_DIR}/.perl-lsp/owned_state.v1.json" ]; then
    fail_case "manifest recorded" "owned_state.v1.json missing after install"
else
    pass "install records owned_state manifest"
fi

if [ ! -L "${INSTALL_DIR}/${BIN_NAME}" ]; then
    fail_case "path selector" "perllsp symlink missing"
else
    pass "path selector exists before uninstall"
fi

run_uninstall
if [ "$LAST_STATUS" -eq 0 ] && [[ "$LAST_OUT" == *"result=removed"* ]]; then
    pass "manifest-backed uninstall completes"
else
    fail_case "uninstall removed" "status=$LAST_STATUS output=$LAST_OUT"
fi

if [ -e "${INSTALL_DIR}/.perl-lsp" ] || [ -L "${INSTALL_DIR}/${BIN_NAME}" ]; then
    fail_case "post-uninstall cleanup" "store or selector still present"
else
    pass "uninstall removes store and PATH selectors"
fi

run_uninstall
if [ "$LAST_STATUS" -eq 0 ] && [[ "$LAST_OUT" == *"already_absent_owned_state"* ]]; then
    pass "second uninstall is idempotent already_absent"
else
    fail_case "idempotent uninstall" "status=$LAST_STATUS output=$LAST_OUT"
fi

# ── CLI surface via library-sourced --uninstall path ──────────────────────────

setup_case
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
install_binaries release >/dev/null
UNINSTALL=1
WITH_CLAUDE=0
set +e
CLI_OUT="$(run_standalone_uninstall 2>&1)"
CLI_STATUS=$?
set -e
if [ "$CLI_STATUS" -eq 0 ] && [[ "$CLI_OUT" == *"result=removed"* ]]; then
    pass "install.sh run_standalone_uninstall helper succeeds"
else
    fail_case "run_standalone_uninstall" "status=$CLI_STATUS output=$CLI_OUT"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
if [ "$FAIL" -ne 0 ]; then
    exit 1
fi
