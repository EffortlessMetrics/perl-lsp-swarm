#!/usr/bin/env bash
# Discriminating archive-safety proof for scripts/install.sh (#8352).
#
# Hostile fixtures are generated at runtime. A sentinel outside staging must
# stay unchanged, and INSTALL_DIR must not be touched, on every rejection.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
FIXTURE_PY="$ROOT/scripts/tests/lib/standalone_archive_fixtures.py"
POLICY="$ROOT/policy/standalone-archive-safety.v1.toml"
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

PACKAGE="perllsp-0.18.0-x86_64-unknown-linux-gnu"
BIN_NAME="perllsp"
DAP_BIN_NAME="perl-dap"
VERSION_NUM="0.18.0"
TARGET="x86_64-unknown-linux-gnu"

make_case() {
    local name="$1"
    python3 "$FIXTURE_PY" --case "$name" --out "$TMP/${name}.tar.gz"
}

sentinel_setup() {
    CASE_ROOT="$TMP/run"
    rm -rf "$CASE_ROOT"
    mkdir -p "$CASE_ROOT/tmp" "$CASE_ROOT/install"
    SENTINEL="$CASE_ROOT/sentinel"
    printf 'untouched\n' > "$SENTINEL"
    INSTALL_DIR="$CASE_ROOT/install"
    TMPDIR="$CASE_ROOT/tmp"
    ARCHIVE_PATH=""
    EXTRACT_DIR=""
    STAGING_ROOT=""
}

run_extract() {
    set +e
    LAST_OUTPUT="$(
        {
            extract_archive
            _status=$?
            printf 'EXTRACT_DIR=%s\n' "${EXTRACT_DIR:-}"
            exit "$_status"
        } 2>&1
    )"
    LAST_STATUS=$?
    set -e
    EXTRACT_DIR="$(printf '%s\n' "$LAST_OUTPUT" | sed -n 's/^EXTRACT_DIR=//p' | tail -n 1)"
    LAST_OUTPUT="$(printf '%s\n' "$LAST_OUTPUT" | sed '/^EXTRACT_DIR=/d')"
}

assert_sentinel_untouched() {
    [ "$(cat "$SENTINEL")" = "untouched" ]
}

assert_install_untouched() {
    [ -z "$(ls -A "$INSTALL_DIR" 2>/dev/null || true)" ]
}

policy_id_from_toml() {
    awk -F'"' '/^policy_id / {print $2; exit}' "$POLICY"
}

printf '=== standalone archive safety (#8352) ===\n'

if [ "$(archive_safety_policy_id)" = "$(policy_id_from_toml)" ]; then
    pass "embedded policy id matches policy/standalone-archive-safety.v1.toml"
else
    fail_case "embedded policy id matches policy/standalone-archive-safety.v1.toml" \
        "adapter=$(archive_safety_policy_id) toml=$(policy_id_from_toml)"
fi

if grep -Fq 'tar -xzf "$ARCHIVE_PATH" -C "$TMPDIR"' "$INSTALLER"; then
    fail_case "unguarded full-tree tar extract is gone" "scripts/install.sh still unpacks the whole archive into TMPDIR"
else
    pass "unguarded full-tree tar extract is gone"
fi

if grep -Fq 'Expand-Archive -Path $ZipPath' "$ROOT/install.ps1"; then
    fail_case "PowerShell Expand-Archive is not the extract path" "install.ps1 still calls Expand-Archive on the release zip"
else
    pass "PowerShell Expand-Archive is not the extract path"
fi

# The fixture carries parent components independently of whether a particular
# host tar follows them. Current GNU tar refuses `../` by default; the adapter
# must still reject the member during inspect so older extractors cannot run.
sentinel_setup
make_case traversal_parent
if tar -tzf "$TMP/traversal_parent.tar.gz" 2>/dev/null | grep -F '..' >/dev/null; then
    pass "traversal fixture carries parent-component members"
else
    fail_case "traversal fixture carries parent-component members" "tar -tzf did not list a parent component"
fi

sentinel_setup
make_case valid_posix
ARCHIVE_PATH="$TMP/valid_posix.tar.gz"
run_extract
if [ "$LAST_STATUS" -eq 0 ] \
    && [ -f "${EXTRACT_DIR}/perllsp" ] \
    && [ -f "${EXTRACT_DIR}/perl-dap" ] \
    && [ -f "${EXTRACT_DIR}/README.md" ] \
    && [ -f "${EXTRACT_DIR}/LICENSE-APACHE" ] \
    && [ -f "${EXTRACT_DIR}/LICENSE-MIT" ] \
    && [ -f "${EXTRACT_DIR}/SHA256SUMS.txt" ] \
    && assert_sentinel_untouched \
    && assert_install_untouched \
    && [[ "$LAST_OUTPUT" == *"policy=standalone-archive-safety.v1"* ]] \
    && [[ "$LAST_OUTPUT" != *"$CASE_ROOT"* ]]; then
    pass "valid nested topology stages accepted members only"
else
    fail_case "valid nested topology stages accepted members only" \
        "status=$LAST_STATUS extract_dir=$EXTRACT_DIR output=$LAST_OUTPUT"
fi

if [ "$LAST_STATUS" -eq 0 ]; then
    if [ -x "${EXTRACT_DIR}/perllsp" ] && [ -x "${EXTRACT_DIR}/perl-dap" ]; then
        pass "topology executables receive reviewed 0755 modes"
    else
        fail_case "topology executables receive reviewed 0755 modes" "modes were not applied"
    fi
fi

run_reject() {
    local case_name="$1"
    local needle="$2"
    sentinel_setup
    make_case "$case_name"
    ARCHIVE_PATH="$TMP/${case_name}.tar.gz"
    run_extract
    if [ "$LAST_STATUS" -ne 0 ] \
        && [[ "$LAST_OUTPUT" == *"$needle"* ]] \
        && assert_sentinel_untouched \
        && assert_install_untouched \
        && [[ "$LAST_OUTPUT" != *"$CASE_ROOT"* ]]; then
        pass "$case_name fails closed before destination writes"
    else
        fail_case "$case_name fails closed before destination writes" \
            "status=$LAST_STATUS output=$LAST_OUTPUT sentinel=$(cat "$SENTINEL")"
    fi
}

run_reject traversal_parent "unsafe archive member"
run_reject absolute_path "unsafe archive member"
run_reject windows_drive "unsafe archive member"
run_reject backslash_separator "unsafe archive member"
run_reject empty_component "unsafe archive member"
run_reject symlink_entry "archive links are not accepted"
run_reject hardlink_entry "archive links are not accepted"
run_reject fifo_entry "special archive entry"
run_reject duplicate_path "duplicate archive member"
run_reject case_collision "case-fold collision"
run_reject missing_dap "missing required member"
run_reject duplicate_server "unexpected executable"
run_reject extra_executable "unexpected executable"

sentinel_setup
make_case too_many_entries
ARCHIVE_PATH="$TMP/too_many_entries.tar.gz"
PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRIES=8 run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"entry count"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "entry-count ceiling fails before extract"
else
    fail_case "entry-count ceiling fails before extract" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

sentinel_setup
make_case oversized_entry
ARCHIVE_PATH="$TMP/oversized_entry.tar.gz"
PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRY_BYTES=8 run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"entry size"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "per-entry byte ceiling fails closed"
else
    fail_case "per-entry byte ceiling fails closed" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

sentinel_setup
make_case valid_posix
ARCHIVE_PATH="$TMP/valid_posix.tar.gz"
PERL_LSP_ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES=16 run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"compressed size"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "compressed-size ceiling fails before extract"
else
    fail_case "compressed-size ceiling fails before extract" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

sentinel_setup
python3 "$FIXTURE_PY" --case truncated_garbage --out "$TMP/truncated_garbage.tar.gz"
ARCHIVE_PATH="$TMP/truncated_garbage.tar.gz"
run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"malformed"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "malformed archive fails closed"
else
    fail_case "malformed archive fails closed" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf '\n=== Results: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
