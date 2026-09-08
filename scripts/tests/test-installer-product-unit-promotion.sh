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

if grep -Fq 'ln -sfn' "$INSTALLER"; then
    fail_case "Darwin fallback is rename not ln -sfn" "scripts/install.sh still uses ln -sfn"
else
    pass "Darwin fallback is rename not ln -sfn"
fi

if grep -E 'trap[[:space:]].*EXIT' "$INSTALLER" | grep -vq 'rm -rf'; then
    fail_case "install_binaries does not replace the process EXIT trap" \
        "$(grep -nE 'trap[[:space:]].*EXIT' "$INSTALLER" || true)"
else
    pass "install_binaries does not replace the process EXIT trap"
fi

if grep -Eq 'trap rollback_new_path_selectors_on_signal INT TERM HUP' "$INSTALLER" \
    && grep -Fq 'restore_saved_signal_trap' "$INSTALLER" \
    && grep -Fq 'committed_incoming_product_unit' "$INSTALLER"; then
    pass "selector window arms INT/TERM/HUP without touching EXIT"
else
    fail_case "selector window arms INT/TERM/HUP without touching EXIT" \
        "$(grep -nE 'trap[[:space:]]' "$INSTALLER" || true)"
fi

if grep -Fq 'BASHPID' "$INSTALLER" \
    && grep -Fq "sh -c 'echo \$PPID'" "$INSTALLER" \
    && ! grep -Fq 'BASHPID:-$$' "$INSTALLER"; then
    pass "signal inject uses BASHPID or PPID, not parent \$\$"
else
    fail_case "signal inject uses BASHPID or PPID, not parent \$\$" \
        "$(grep -n 'BASHPID\|PPID' "$INSTALLER" || true)"
fi

if grep -Fq 'rm -f "${INSTALL_DIR}/${BIN_NAME}"' "$INSTALLER"; then
    fail_case "PATH selectors do not unlink before replace" "scripts/install.sh still removes PATH-visible perllsp before creating the selector"
else
    pass "PATH selectors do not unlink before replace"
fi

if ! grep -Fq 'perl -e' "$INSTALLER"; then
    fail_case "POSIX rename fallback is present" "scripts/install.sh has no perl rename fallback"
else
    pass "POSIX rename fallback is present"
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
stage_pair "$EXTRACT_DIR" "server-first" "dap-first"
PERL_LSP_INSTALL_FAULT=before_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && [ ! -e "$(path_server)" ] && [ ! -L "$(path_server)" ] \
    && [ ! -e "$(path_dap)" ] && [ ! -L "$(path_dap)" ] \
    && [[ "$LAST_OUTPUT" == *"before_commit"* ]]; then
    pass "first-install commit fault leaves no broken selectors"
else
    fail_case "first-install commit fault leaves no broken selectors" \
        "status=$LAST_STATUS output=$LAST_OUTPUT server=$(ls -ld "$(path_server)" 2>&1 || true) dap=$(ls -ld "$(path_dap)" 2>&1 || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-sig" "dap-sig"
PERL_LSP_INSTALL_FAULT=signal_before_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && [ ! -e "$(path_server)" ] && [ ! -L "$(path_server)" ] \
    && [ ! -e "$(path_dap)" ] && [ ! -L "$(path_dap)" ]; then
    pass "first-install SIGTERM before commit leaves no broken selectors"
else
    fail_case "first-install SIGTERM before commit leaves no broken selectors" \
        "status=$LAST_STATUS output=$LAST_OUTPUT server=$(ls -ld "$(path_server)" 2>&1 || true) dap=$(ls -ld "$(path_dap)" 2>&1 || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
PERL_LSP_INSTALL_FAULT=signal_before_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && assert_complete_pair "server-a" "dap-a"; then
    pass "upgrade SIGTERM before commit preserves the old complete pair"
else
    fail_case "upgrade SIGTERM before commit preserves the old complete pair" \
        "status=$LAST_STATUS output=$LAST_OUTPUT current=$(observe_current_product_unit 2>/dev/null || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-after" "dap-after"
PERL_LSP_INSTALL_FAULT=signal_after_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && assert_complete_pair "server-after" "dap-after"; then
    pass "first-install SIGTERM after commit keeps the published pair"
else
    fail_case "first-install SIGTERM after commit keeps the published pair" \
        "status=$LAST_STATUS output=$LAST_OUTPUT current=$(observe_current_product_unit 2>/dev/null || true) path=$(observe_path_visible_product_unit 2>/dev/null || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
PERL_LSP_INSTALL_FAULT=signal_after_commit
run_promote release
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && assert_complete_pair "server-b" "dap-b"; then
    pass "upgrade SIGTERM after commit keeps the new complete pair"
else
    fail_case "upgrade SIGTERM after commit keeps the new complete pair" \
        "status=$LAST_STATUS output=$LAST_OUTPUT current=$(observe_current_product_unit 2>/dev/null || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "pair-server" "pair-dap"
run_promote release
stage_server_only "$EXTRACT_DIR" "source-server"
PERL_LSP_INSTALL_FAULT=before_commit
run_promote source
unset PERL_LSP_INSTALL_FAULT
if [ "$LAST_STATUS" -ne 0 ] \
    && assert_complete_pair "pair-server" "pair-dap" \
    && [ -e "$(path_dap)" ] && [ -L "$(path_dap)" ]; then
    pass "release-to-source commit fault preserves the paired selector"
else
    fail_case "release-to-source commit fault preserves the paired selector" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
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

setup_root
mkdir -p "${CASE_ROOT}/a" "${CASE_ROOT}/b"
ln -s a "${CASE_ROOT}/current"
ln -s b "${CASE_ROOT}/current.tmp"
if perl -e 'rename($ARGV[0], $ARGV[1]) or exit 1' -- "${CASE_ROOT}/current.tmp" "${CASE_ROOT}/current" \
    && [ -L "${CASE_ROOT}/current" ] \
    && [ "$(readlink "${CASE_ROOT}/current")" = "b" ] \
    && [ ! -e "${CASE_ROOT}/current.tmp" ]; then
    pass "perl rename replaces a directory symlink without an unlink gap"
else
    fail_case "perl rename replaces a directory symlink without an unlink gap" \
        "$(ls -ld "${CASE_ROOT}/current" "${CASE_ROOT}/current.tmp" 2>&1 || true)"
fi

setup_root
mkdir -p "${INSTALL_DIR}/.perl-lsp/candidates/cid"
printf '%s\n' "from-current" > "${INSTALL_DIR}/.perl-lsp/candidates/cid/${BIN_NAME}"
ln -s "candidates/cid" "${INSTALL_DIR}/.perl-lsp/current"
printf '%s\n' "legacy-bytes" > "$(path_server)"
INSTALL_DIR="$INSTALL_DIR" ensure_path_visible_selectors 0
if [ -L "$(path_server)" ] \
    && [ "$(readlink "$(path_server)")" = ".perl-lsp/current/${BIN_NAME}" ] \
    && [ "$(hash_file "$(path_server)")" = "$(hash_file "${INSTALL_DIR}/.perl-lsp/candidates/cid/${BIN_NAME}")" ]; then
    pass "legacy regular PATH file is replaced atomically by a current selector"
else
    fail_case "legacy regular PATH file is replaced atomically by a current selector" \
        "$(ls -l "$(path_server)" 2>&1 || true)"
fi

setup_root
stage_server_only "$EXTRACT_DIR" "source-server"
run_promote source
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
_id="$(publish_immutable_candidate "$EXTRACT_DIR" archive_pair_required 0)"
ensure_path_visible_selectors 1 1
commit_current_selection "$_id" 0
if [ -L "$(path_dap)" ] \
    && [ "$(readlink "$(path_dap)")" = ".perl-lsp/current/${DAP_BIN_NAME}" ] \
    && assert_complete_pair "server-b" "dap-b"; then
    pass "source-to-release pair is fully selected by the commit, not by post-commit repair"
else
    fail_case "source-to-release pair is fully selected by the commit, not by post-commit repair" \
        "current=$(observe_current_product_unit 2>/dev/null || true) dap=$(ls -l "$(path_dap)" 2>&1 || true)"
fi

setup_root
stage_server_only "$EXTRACT_DIR" "source-server"
run_promote source
printf '%s\n' "stale-dap-bytes" > "$(path_dap)"
stage_server_only "$EXTRACT_DIR" "source-server-2"
run_promote source
if [ "$LAST_STATUS" -eq 0 ] \
    && [ ! -e "$(path_dap)" ] \
    && [ -L "$(path_server)" ]; then
    pass "server-only promotion removes a stale regular DAP selector"
else
    fail_case "server-only promotion removes a stale regular DAP selector" \
        "status=$LAST_STATUS dap=$(ls -l "$(path_dap)" 2>&1 || true)"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-a" "dap-a"
run_promote release
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
obs="$TMP/observe.txt"
PERL_LSP_INSTALL_OBSERVE=between_path_members
PERL_LSP_INSTALL_OBSERVE_FILE="$obs"
run_promote release
unset PERL_LSP_INSTALL_OBSERVE
unset PERL_LSP_INSTALL_OBSERVE_FILE
obs_text=""
if [ -f "$obs" ]; then
    obs_text="$(cat "$obs")"
fi
if [ "$LAST_STATUS" -eq 0 ] \
    && assert_complete_pair "server-b" "dap-b" \
    && [[ "$obs_text" == *state=selected* ]] \
    && [[ "$obs_text" == *state=path_visible* ]] \
    && [[ "$obs_text" != *state=mixed* ]] \
    && [[ "$obs_text" != *state=none* ]] \
    && [[ "$obs_text" == *server_sha256=* ]] \
    && [[ "$obs_text" == *dap_sha256=* ]] \
    && [[ "$obs_text" != *server_sha256=-* ]] \
    && [[ "$obs_text" != *dap_sha256=-* ]]; then
    pass "interleaved PATH reader never sees mixed members or a missing current"
else
    fail_case "interleaved PATH reader never sees mixed members or a missing current" \
        "status=$LAST_STATUS obs=$obs_text output=$LAST_OUTPUT"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-first" "dap-first"
obs="$TMP/observe-first.txt"
PERL_LSP_INSTALL_OBSERVE=between_path_members
PERL_LSP_INSTALL_OBSERVE_FILE="$obs"
run_promote release
unset PERL_LSP_INSTALL_OBSERVE
unset PERL_LSP_INSTALL_OBSERVE_FILE
obs_text=""
if [ -f "$obs" ]; then
    obs_text="$(cat "$obs")"
fi
if [ "$LAST_STATUS" -eq 0 ] \
    && assert_complete_pair "server-first" "dap-first" \
    && [[ "$obs_text" == *state=none* ]] \
    && [[ "$obs_text" != *state=selected* ]] \
    && [[ "$obs_text" != *state=mixed* ]]; then
    pass "first-install pre-commit observe is kept and is not mixed"
else
    fail_case "first-install pre-commit observe is kept and is not mixed" \
        "status=$LAST_STATUS obs=$obs_text output=$LAST_OUTPUT"
fi

setup_root
stage_server_only "$EXTRACT_DIR" "source-server"
run_promote source
stage_pair "$EXTRACT_DIR" "server-b" "dap-b"
obs="$TMP/observe-source-to-release.txt"
PERL_LSP_INSTALL_OBSERVE=between_path_members
PERL_LSP_INSTALL_OBSERVE_FILE="$obs"
run_promote release
unset PERL_LSP_INSTALL_OBSERVE
unset PERL_LSP_INSTALL_OBSERVE_FILE
obs_text=""
if [ -f "$obs" ]; then
    obs_text="$(cat "$obs")"
fi
if [ "$LAST_STATUS" -eq 0 ] \
    && assert_complete_pair "server-b" "dap-b" \
    && [[ "$obs_text" == *state=selected* ]] \
    && [[ "$obs_text" != *state=mixed* ]] \
    && [[ "$obs_text" != *state=none* ]]; then
    pass "source-to-release pre-commit observe is kept and is not mixed"
else
    fail_case "source-to-release pre-commit observe is kept and is not mixed" \
        "status=$LAST_STATUS obs=$obs_text output=$LAST_OUTPUT"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-trap" "dap-trap"
_trap_marker="$TMP/caller-exit-trap"
rm -f "$_trap_marker"
# shellcheck disable=SC2064
trap 'printf survived > "$_trap_marker"' EXIT
set +e
install_binaries release >/dev/null
_trap_status=$?
_trap_text="$(trap -p EXIT)"
trap 'rm -rf "$TMP"' EXIT
set -e
if [ "$_trap_status" -eq 0 ] \
    && assert_complete_pair "server-trap" "dap-trap" \
    && [[ "$_trap_text" == *printf\ survived* ]]; then
    pass "successful promotion keeps the caller EXIT trap"
else
    fail_case "successful promotion keeps the caller EXIT trap" \
        "status=$_trap_status trap=$_trap_text"
fi

_term_text="$(trap -p TERM)$(trap -p INT)$(trap -p HUP)"
if [ -z "$_term_text" ]; then
    pass "successful promotion restores default INT/TERM/HUP traps"
else
    fail_case "successful promotion restores default INT/TERM/HUP traps" \
        "traps=$_term_text"
fi

setup_root
stage_pair "$EXTRACT_DIR" "server-term" "dap-term"
trap 'printf term-survived' TERM
set +e
install_binaries release >/dev/null
_term_status=$?
_term_saved="$(trap -p TERM)"
trap - TERM
set -e
if [ "$_term_status" -eq 0 ] \
    && assert_complete_pair "server-term" "dap-term" \
    && [[ "$_term_saved" == *term-survived* ]]; then
    pass "successful promotion restores the caller TERM trap"
else
    fail_case "successful promotion restores the caller TERM trap" \
        "status=$_term_status trap=$_term_saved"
fi

if [ "$FAIL" -ne 0 ]; then
    printf 'FAILED %s  passed %s\n' "$FAIL" "$PASS" >&2
    exit 1
fi
printf 'passed %s\n' "$PASS"
