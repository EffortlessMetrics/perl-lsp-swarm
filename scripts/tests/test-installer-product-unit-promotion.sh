#!/usr/bin/env bash
# Discriminating product-unit promotion proof for scripts/install.sh (#8359).
#
# Readers of PATH-visible names and of .perl-lsp/current must observe the old
# complete unit or the new complete unit, never a mixed or partial pair.

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
DAP_BIN_NAME="perl-dap"

hash_file() {
    hash_product_member "$1"
}

stage_pair() {
    local dest="$1" server_payload="$2" dap_payload="$3"
    rm -rf "$dest"
    mkdir -p "$dest"
    printf '%s\n' "$server_payload" > "${dest}/${BIN_NAME}"
    printf '%s\n' "$dap_payload" > "${dest}/${DAP_BIN_NAME}"
}

stage_server_only() {
    local dest="$1" server_payload="$2"
    rm -rf "$dest"
    mkdir -p "$dest"
    printf '%s\n' "$server_payload" > "${dest}/${BIN_NAME}"
}

setup_root() {
    CASE_ROOT="$TMP/run"
    rm -rf "$CASE_ROOT"
    mkdir -p "$CASE_ROOT/install" "$CASE_ROOT/stage"
    INSTALL_DIR="$CASE_ROOT/install"
    EXTRACT_DIR="$CASE_ROOT/stage"
    unset PERL_LSP_INSTALL_FAULT
}

run_promote() {
    local mode="${1:-release}"
    set +e
    LAST_OUTPUT="$(install_binaries "$mode" 2>&1)"
    LAST_STATUS=$?
    set -e
}

path_server() { printf '%s\n' "${INSTALL_DIR}/${BIN_NAME}"; }
path_dap() { printf '%s\n' "${INSTALL_DIR}/${DAP_BIN_NAME}"; }

assert_complete_pair() {
    local server_payload="$1" dap_payload="$2"
    printf '%s\n' "$server_payload" > "$TMP/expect-server"
    printf '%s\n' "$dap_payload" > "$TMP/expect-dap"
    want_server="$(hash_file "$TMP/expect-server")"
    want_dap="$(hash_file "$TMP/expect-dap")"
    current="$(observe_current_product_unit)"
    pathv="$(observe_path_visible_product_unit)"
    got_server="$(hash_file "$(path_server)")"
    got_dap="$(hash_file "$(path_dap)")"
    [ -L "$(path_server)" ] || return 1
    [ -L "$(path_dap)" ] || return 1
    [ "$(dirname "$(readlink "$(path_server)")")" = "$(dirname "$(readlink "$(path_dap)")")" ] || return 1
    [ "$got_server" = "$want_server" ] || return 1
    [ "$got_dap" = "$want_dap" ] || return 1
    [[ "$current" == *"server_sha256=${want_server}"* ]] || return 1
    [[ "$current" == *"dap_sha256=${want_dap}"* ]] || return 1
    [[ "$current" == *"state=selected"* ]] || return 1
    [[ "$pathv" != state=mixed* ]] || return 1
    [[ "$pathv" == *"server_sha256=${want_server}"* ]] || return 1
    [[ "$pathv" == *"dap_sha256=${want_dap}"* ]] || return 1
}

printf '=== standalone product-unit promotion (#8359) ===\n'

if grep -Fq 'cp "$_src_bin" "$INSTALL_DIR/$BIN_NAME"' "$INSTALLER"; then
    fail_case "independent perllsp destination copy is gone" "scripts/install.sh still copies perllsp onto INSTALL_DIR before perl-dap"
else
    pass "independent perllsp destination copy is gone"
fi

if grep -Fq 'cp "$_src_dap" "$INSTALL_DIR/$DAP_BIN_NAME"' "$INSTALLER"; then
    fail_case "independent perl-dap destination copy is gone" "scripts/install.sh still copies perl-dap onto INSTALL_DIR independently"
else
    pass "independent perl-dap destination copy is gone"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
receipt_line="$(printf '%s\n' "$LAST_OUTPUT" | grep 'product_unit_receipt' | tail -n 1 || true)"
if [ "$LAST_STATUS" -eq 0 ] \
    && assert_complete_pair "server-a" "dap-a" \
    && [[ "$receipt_line" == *"product_unit_receipt"* ]] \
    && [[ "$receipt_line" == *"disposition=archive_pair_required"* ]] \
    && [[ "$receipt_line" != *"$CASE_ROOT"* ]]; then
    pass "first archive pair publishes one current complete unit"
else
    fail_case "first archive pair publishes one current complete unit" \
        "status=$LAST_STATUS output=$LAST_OUTPUT current=$(observe_current_product_unit 2>/dev/null || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
run_promote release
previous="$(readlink "${INSTALL_DIR}/.perl-lsp/previous" 2>/dev/null || true)"
if [ "$LAST_STATUS" -eq 0 ] \
    && assert_complete_pair "server-b" "dap-b" \
    && [ -n "$previous" ]; then
    old_server="$(hash_file "${INSTALL_DIR}/.perl-lsp/${previous}/${BIN_NAME}" 2>/dev/null || hash_file "${INSTALL_DIR}/.perl-lsp/previous/${BIN_NAME}")"
    printf '%s\n' "server-a" > "$TMP/expect-server"
    if [ "$old_server" = "$(hash_file "$TMP/expect-server")" ]; then
        pass "upgrade retains previous complete unit and selects the new pair"
    else
        fail_case "upgrade retains previous complete unit and selects the new pair" \
            "previous=$previous old_server=$old_server"
    fi
else
    fail_case "upgrade retains previous complete unit and selects the new pair" \
        "status=$LAST_STATUS output=$LAST_OUTPUT previous=$previous"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
PERL_LSP_INSTALL_FAULT=before_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && assert_complete_pair "server-a" "dap-a" \
    && [[ "$LAST_OUTPUT" == *"before_commit"* ]]; then
    pass "commit fault preserves the old complete pair"
else
    fail_case "commit fault preserves the old complete pair" \
        "status=$LAST_STATUS output=$LAST_OUTPUT current=$(observe_current_product_unit) path=$(observe_path_visible_product_unit)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
PERL_LSP_INSTALL_FAULT=before_publish
run_promote release
unset PERL_LSP_INSTALL_FAULT
new_candidates="$(find "${INSTALL_DIR}/.perl-lsp/candidates" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')"
if [ "$LAST_STATUS" -ne 0 ] \
    && assert_complete_pair "server-a" "dap-a" \
    && [ "$new_candidates" = "1" ]; then
    pass "publish fault does not select or leak a partial new pair"
else
    fail_case "publish fault does not select or leak a partial new pair" \
        "status=$LAST_STATUS candidates=$new_candidates output=$LAST_OUTPUT"
fi

setup_root
stage_server_only "$EXTRACT_DIR" "server-only"
run_promote release
if [ "$LAST_STATUS" -ne 0 ] \
    && [ ! -e "$(path_server)" ] \
    && [ ! -e "$(path_dap)" ] \
    && [[ "$LAST_OUTPUT" == *"complete perllsp/perl-dap pair"* ]]; then
    pass "release mode rejects a missing DAP before current moves"
else
    fail_case "release mode rejects a missing DAP before current moves" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

setup_root
stage_pair "$INSTALL_DIR" "legacy-server" "legacy-dap"
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
PERL_LSP_INSTALL_FAULT=before_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
printf '%s\n' "legacy-server" > "$TMP/expect-server"
printf '%s\n' "legacy-dap" > "$TMP/expect-dap"
if [ "$LAST_STATUS" -ne 0 ] \
    && [ -e "$(path_server)" ] && [ -e "$(path_dap)" ] \
    && [ "$(hash_file "$(path_server)")" = "$(hash_file "$TMP/expect-server")" ] \
    && [ "$(hash_file "$(path_dap)")" = "$(hash_file "$TMP/expect-dap")" ]; then
    pass "legacy regular files stay a complete pair when the new commit fails"
else
    fail_case "legacy regular files stay a complete pair when the new commit fails" \
        "status=$LAST_STATUS output=$LAST_OUTPUT server=$(hash_file "$(path_server)" 2>/dev/null || echo missing)"
fi

setup_root
stage_pair "$INSTALL_DIR" "legacy-server" "legacy-dap"
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
run_promote release
if [ "$LAST_STATUS" -eq 0 ] && assert_complete_pair "server-b" "dap-b"; then
    pass "legacy regular pair is imported then atomically replaced by the new pair"
else
    fail_case "legacy regular pair is imported then atomically replaced by the new pair" \
        "status=$LAST_STATUS output=$LAST_OUTPUT current=$(observe_current_product_unit)"
fi

setup_root
stage_server_only "$EXTRACT_DIR" "source-server"
run_promote source
current="$(observe_current_product_unit)"
pathv="$(observe_path_visible_product_unit)"
printf '%s\n' "source-server" > "$TMP/expect-server"
if [ "$LAST_STATUS" -eq 0 ] \
    && [ -L "$(path_server)" ] \
    && [ ! -e "$(path_dap)" ] \
    && [ "$(hash_file "$(path_server)")" = "$(hash_file "$TMP/expect-server")" ] \
    && [[ "$current" == *"disposition=advanced_source_server_only"* ]] \
    && [[ "$current" == *"dap_sha256=-"* ]] \
    && [[ "$pathv" != state=mixed* ]]; then
    pass "source mode publishes an explicit server-only unit, not a pair"
else
    fail_case "source mode publishes an explicit server-only unit, not a pair" \
        "status=$LAST_STATUS current=$current path=$pathv output=$LAST_OUTPUT"
fi

setup_root
stage_pair "$EXTRACT_DIR" "pair-server" "pair-dap"
run_promote release
stage_server_only "$EXTRACT_DIR" "source-server"
run_promote source
printf '%s\n' "source-server" > "$TMP/expect-server"
current="$(observe_current_product_unit)"
if [ "$LAST_STATUS" -eq 0 ] \
    && [ "$(hash_file "$(path_server)")" = "$(hash_file "$TMP/expect-server")" ] \
    && [ ! -e "$(path_dap)" ] \
    && [[ "$current" == *"advanced_source_server_only"* ]]; then
    pass "source upgrade does not keep the previous DAP as current"
else
    dap_state=missing
    if [ -e "$(path_dap)" ]; then
        dap_state=present
    fi
    fail_case "source upgrade does not keep the previous DAP as current" \
        "status=$LAST_STATUS current=$current dap=$dap_state"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
id_a="$(readlink "${INSTALL_DIR}/.perl-lsp/current")"
id_a="${id_a##*/}"
stage_pair "$EXTRACT_DIR" "server-a2" "dap-a2"
run_promote release
if [ -f "${INSTALL_DIR}/.perl-lsp/candidates/${id_a}/${BIN_NAME}" ]; then
    printf '%s\n' "server-a" > "$TMP/expect-server"
    if [ "$(hash_file "${INSTALL_DIR}/.perl-lsp/candidates/${id_a}/${BIN_NAME}")" = "$(hash_file "$TMP/expect-server")" ]; then
        pass "same-name repair publishes a new candidate and leaves prior bytes immutable"
    else
        fail_case "same-name repair publishes a new candidate and leaves prior bytes immutable" \
            "candidate $id_a bytes changed"
    fi
else
    fail_case "same-name repair publishes a new candidate and leaves prior bytes immutable" \
        "missing candidate $id_a"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
if [ -d "${INSTALL_DIR}/.perl-lsp/current/${BIN_NAME}" ]; then
    fail_case "current is a symlink not a directory of loose files" "current resolved as a directory of the server name"
else
    if [ -L "${INSTALL_DIR}/.perl-lsp/current" ]; then
        pass "current selection is a single symlink to one candidate directory"
    else
        fail_case "current selection is a single symlink to one candidate directory" \
            "$(ls -ld "${INSTALL_DIR}/.perl-lsp/current" 2>&1 || true)"
    fi
fi

if [ "$FAIL" -ne 0 ]; then
    printf 'FAILED %s  passed %s\n' "$FAIL" "$PASS" >&2
    exit 1
fi
printf 'passed %s\n' "$PASS"
