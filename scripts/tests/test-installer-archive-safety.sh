#!/usr/bin/env bash
# Discriminating archive-safety proof for scripts/install.sh (#8352, #11508).
#
# Hostile fixtures are generated at runtime. A sentinel outside staging must
# stay unchanged, and INSTALL_DIR must not be touched, on every rejection.
#
# The corpus runs once per available tar profile. GNU tar, bsdtar (libarchive,
# the macOS tar family), and BusyBox tar disagree on how they render a listing:
# BusyBox prints a hardlink with a regular-file type char and strips leading
# `/` and `../` from names. Evidence from one profile therefore cannot stand in
# for another, and a profile that is not installed is reported NOT PROVEN
# rather than silently counted as covered (#11508).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"

# ── Profile driver ────────────────────────────────────────────────────────────
# Without a selected profile this script is the driver: it re-execs itself once
# per available tar implementation with that implementation shimmed in as
# `tar`, then aggregates. With one selected it runs the corpus below.

if [ -z "${PERL_LSP_TEST_TAR_PROFILE:-}" ]; then
    DRIVER_TMP="$(mktemp -d)"
    trap 'rm -rf "$DRIVER_TMP"' EXIT

    # `tar` as the host resolves it is always exercised, under its real name.
    PROFILES=("system")
    for candidate in bsdtar busybox; do
        if command -v "$candidate" > /dev/null 2>&1; then
            mkdir -p "$DRIVER_TMP/$candidate"
            case "$candidate" in
                busybox) printf '#!/bin/sh\nexec busybox tar "$@"\n' > "$DRIVER_TMP/$candidate/tar" ;;
                *) printf '#!/bin/sh\nexec %s "$@"\n' "$candidate" > "$DRIVER_TMP/$candidate/tar" ;;
            esac
            chmod +x "$DRIVER_TMP/$candidate/tar"
            PROFILES+=("$candidate")
        else
            printf 'NOT PROVEN  tar profile %s is not installed on this host\n' "$candidate"
        fi
    done

    DRIVER_FAIL=0
    for profile in "${PROFILES[@]}"; do
        printf '\n##### tar profile: %s (%s) #####\n' \
            "$profile" \
            "$(PATH="${DRIVER_TMP}/${profile}:$PATH" tar --version 2>/dev/null | head -n 1)"
        if PERL_LSP_TEST_TAR_PROFILE="$profile" \
            PATH="${DRIVER_TMP}/${profile}:$PATH" \
            bash "${BASH_SOURCE[0]}"; then
            :
        else
            DRIVER_FAIL=1
        fi
    done

    printf '\n=== tar profiles exercised: %s ===\n' "${PROFILES[*]}"
    exit "$DRIVER_FAIL"
fi

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

printf '=== standalone archive safety (#8352, #11508) — tar profile %s ===\n' \
    "$PERL_LSP_TEST_TAR_PROFILE"

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
#
# The stored names are read with Python's tarfile rather than `tar -t`: BusyBox
# tar sanitizes `../` out of the name it prints, so a listing-based self-check
# would report the fixture as toothless when it is in fact hostile (#11508).
sentinel_setup
make_case traversal_parent
if python3 -c '
import sys, tarfile
with tarfile.open(sys.argv[1]) as a:
    sys.exit(0 if any(".." in n.split("/") for n in a.getnames()) else 1)
' "$TMP/traversal_parent.tar.gz"; then
    pass "traversal fixture carries parent-component members"
else
    fail_case "traversal fixture carries parent-component members" "no stored name carries a parent component"
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

# Availability control for the header walk (#11508). Every rejection case below
# is generated by Python's tarfile; this one is written by the profile's own
# `tar czf` from a real directory tree, with sub-second mtimes as APFS produces
# them. Release archives are built exactly this way (`tar czf` on the GNU and
# macOS runners), so a walk that fails closed on extended records has to be
# shown not to fail closed on the archives this project actually ships.
sentinel_setup
BUILD_ROOT="$TMP/native-build"
rm -rf "$BUILD_ROOT"
mkdir -p "$BUILD_ROOT/$PACKAGE"
for member in perllsp perl-dap README.md LICENSE-APACHE LICENSE-MIT SHA256SUMS.txt; do
    printf 'native %s\n' "$member" > "$BUILD_ROOT/$PACKAGE/$member"
done
chmod 0755 "$BUILD_ROOT/$PACKAGE/perllsp" "$BUILD_ROOT/$PACKAGE/perl-dap"
python3 -c '
import os, sys, time
root = sys.argv[1]
for name in os.listdir(root):
    stamp = time.time() + 0.123456789
    os.utime(os.path.join(root, name), (stamp, stamp))
' "$BUILD_ROOT/$PACKAGE"
( cd "$BUILD_ROOT" && tar czf "$TMP/native_profile.tar.gz" "$PACKAGE" )
ARCHIVE_PATH="$TMP/native_profile.tar.gz"
run_extract
if [ "$LAST_STATUS" -eq 0 ] \
    && [ -f "${EXTRACT_DIR}/perllsp" ] \
    && [ -f "${EXTRACT_DIR}/SHA256SUMS.txt" ]; then
    pass "archive written by this profile's own tar is accepted"
else
    fail_case "archive written by this profile's own tar is accepted" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
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
run_reject reserved_device_name "unsafe archive member"
run_reject trailing_dot "unsafe archive member"

# Listing-profile discriminators (#11508). Each supplies its hostile entry
# under an accepted topology name, so no incidental rule can reject it first
# and only the header-derived type or path can. Both were admitted, staged,
# and attested by the receipt under BusyBox tar while the adapter classified
# entries from `tar -t` / `tar -tv` output.
run_reject hardlink_topology_member "archive links are not accepted"
run_reject absolute_topology_member "unsafe archive member"
run_reject newline_in_member_name "unsafe archive member"
run_reject extended_pax_header "extended archive headers are not accepted"

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
# 32 is above every valid topology member in the fixture and below the
# 64-byte README. That isolates the streamed per-entry ceiling from uid
# columns that a GNU tar -tv first-numeric parse used to treat as size.
PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRY_BYTES=32 run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"entry size"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "per-entry byte ceiling fails closed on streamed extract"
else
    fail_case "per-entry byte ceiling fails closed on streamed extract" "status=$LAST_STATUS output=$LAST_OUTPUT"
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
make_case valid_posix
ARCHIVE_PATH="$TMP/valid_posix.tar.gz"
PERL_LSP_ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES=8 run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"uncompressed size"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "uncompressed-size ceiling fails before listing extract"
else
    fail_case "uncompressed-size ceiling fails before listing extract" "status=$LAST_STATUS output=$LAST_OUTPUT"
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

# Decompresses cleanly, so the gzip guard cannot catch it: only the header
# checksum distinguishes a corrupt header from a member (#11508).
sentinel_setup
python3 "$FIXTURE_PY" --case corrupt_header_checksum --out "$TMP/corrupt_header_checksum.tar.gz"
ARCHIVE_PATH="$TMP/corrupt_header_checksum.tar.gz"
run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"malformed"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "corrupt tar header fails closed"
else
    fail_case "corrupt tar header fails closed" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

# The walk must agree with a conformant reader about which entries exist, not
# merely fail closed by accident when it does not (#11508).
sentinel_setup
python3 "$FIXTURE_PY" --case sized_directory_entry --out "$TMP/sized_directory_entry.tar.gz"
ARCHIVE_PATH="$TMP/sized_directory_entry.tar.gz"
run_extract
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"declares data on a type that carries none"* ]] \
    && assert_sentinel_untouched \
    && assert_install_untouched; then
    pass "dataless entry type declaring a size fails closed"
else
    fail_case "dataless entry type declaring a size fails closed" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

# The receipt names the tar that staged the members. Inspection no longer
# depends on it, but the profile has to be visible in installer evidence for a
# host report to mean anything (#11508).
sentinel_setup
make_case valid_posix
ARCHIVE_PATH="$TMP/valid_posix.tar.gz"
run_extract
if [ "$LAST_STATUS" -eq 0 ] \
    && [[ "$LAST_OUTPUT" == *"extractor=$(archive_extractor_profile)"* ]] \
    && [[ "$LAST_OUTPUT" != *"extractor=unknown"* ]]; then
    pass "safety receipt records the resolved tar profile"
else
    fail_case "safety receipt records the resolved tar profile" \
        "profile=$(archive_extractor_profile) status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf '\n=== Results: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
