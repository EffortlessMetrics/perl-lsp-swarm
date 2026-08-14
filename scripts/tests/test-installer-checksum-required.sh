#!/usr/bin/env bash
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

assert_failure_contains() {
    local name="$1" needle="$2"
    if [ "$LAST_STATUS" -ne 0 ] && [[ "$LAST_OUTPUT" == *"$needle"* ]]; then
        pass "$name"
    else
        fail_case "$name" "expected failure containing '$needle'; status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
}

# Load functions without running installer main.
PERL_LSP_INSTALLER_LIBRARY_ONLY=1 source "$INSTALLER"

ASSET="perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz"
GOOD_HASH="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
SUMS="$TMP/SHA256SUMS"

run_checksum_parse() {
    set +e
    LAST_OUTPUT="$( (checksum_for_asset "$SUMS" "$ASSET") 2>&1)"
    LAST_STATUS=$?
    set -e
}

printf '=== required release checksum contract (#6097) ===\n'

printf '%s  %s\n' "$GOOD_HASH" "$ASSET" > "$SUMS"
run_checksum_parse
if [ "$LAST_STATUS" -eq 0 ] && [ "$LAST_OUTPUT" = "$GOOD_HASH" ]; then
    pass "exact GNU-style checksum row is accepted"
else
    fail_case "exact GNU-style checksum row is accepted" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf '%s *%s\r\n' "$GOOD_HASH" "$ASSET" > "$SUMS"
run_checksum_parse
if [ "$LAST_STATUS" -eq 0 ] && [ "$LAST_OUTPUT" = "$GOOD_HASH" ]; then
    pass "binary-marker CRLF checksum row is normalized"
else
    fail_case "binary-marker CRLF checksum row is normalized" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf '%s  prefix-%s\n' "$GOOD_HASH" "$ASSET" > "$SUMS"
run_checksum_parse
assert_failure_contains "substring filename cannot satisfy exact asset identity" "contains no exact entry"

printf '%s  other.tar.gz\n' "$GOOD_HASH" > "$SUMS"
run_checksum_parse
assert_failure_contains "missing asset row fails closed" "contains no exact entry"

printf '%s  %s\n%s *%s\n' "$GOOD_HASH" "$ASSET" "$GOOD_HASH" "$ASSET" > "$SUMS"
run_checksum_parse
assert_failure_contains "duplicate asset rows fail closed" "contains duplicate entries"

printf '%s  %s\n' "abc123" "$ASSET" > "$SUMS"
run_checksum_parse
assert_failure_contains "short checksum row fails closed" "expected 64 hexadecimal characters"

printf '%s  %s\n' "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" "$ASSET" > "$SUMS"
run_checksum_parse
assert_failure_contains "non-lowercase checksum row fails closed" "must be lowercase hexadecimal"

NO_SHA_PATH="$TMP/no-sha"
mkdir -p "$NO_SHA_PATH"
set +e
LAST_OUTPUT="$( (PATH="$NO_SHA_PATH"; select_sha256_tool) 2>&1)"
LAST_STATUS=$?
set -e
assert_failure_contains "missing SHA-256 implementation fails before network" "is required to verify release artifacts"

# End-to-end download/verify seam with a hermetic transport.
ARCHIVE_SOURCE="$TMP/archive-source"
printf 'bounded archive bytes\n' > "$ARCHIVE_SOURCE"
SHA_TOOL="$(select_sha256_tool)"
ARCHIVE_HASH="$(calculate_sha256 "$SHA_TOOL" "$ARCHIVE_SOURCE")"
CURL_LOG="$TMP/curl.log"
SUMS_SOURCE="$TMP/sums-source"
CURL_SUMS_FAIL=0
CURL_ASSET_FAIL=0

curl() {
    local out="" url=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            -o)
                out="$2"
                shift 2
                ;;
            --progress-bar|-fsSL)
                shift
                ;;
            *)
                url="$1"
                shift
                ;;
        esac
    done
    printf '%s\n' "$url" >> "$CURL_LOG"
    case "$url" in
        */SHA256SUMS)
            if [ "$CURL_SUMS_FAIL" = "1" ]; then
                return 22
            fi
            cp "$SUMS_SOURCE" "$out"
            ;;
        *.tar.gz)
            if [ "$CURL_ASSET_FAIL" = "1" ]; then
                return 22
            fi
            cp "$ARCHIVE_SOURCE" "$out"
            ;;
        *)
            return 2
            ;;
    esac
}

prepare_download() {
    rm -rf "$TMP/download"
    mkdir -p "$TMP/download"
    : > "$CURL_LOG"
    TMPDIR="$TMP/download"
    TAG="v0.18.0"
    VERSION_NUM="0.18.0"
    TARGET="x86_64-unknown-linux-gnu"
    ARCHIVE_PATH=""
    EXTRACT_DIR=""
    CURL_SUMS_FAIL=0
    CURL_ASSET_FAIL=0
}

run_download() {
    set +e
    LAST_OUTPUT="$( (download_and_verify) 2>&1)"
    LAST_STATUS=$?
    set -e
}

prepare_download
printf '%s  %s\n' "$ARCHIVE_HASH" "$ASSET" > "$SUMS_SOURCE"
run_download
EXPECTED_ARCHIVE="$TMP/download/$ASSET"
if [ "$LAST_STATUS" -eq 0 ] \
    && [ -f "$EXPECTED_ARCHIVE" ] \
    && cmp -s "$ARCHIVE_SOURCE" "$EXPECTED_ARCHIVE" \
    && [ "$(sed -n '1p' "$CURL_LOG")" = "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v0.18.0/SHA256SUMS" ] \
    && [ "$(sed -n '2p' "$CURL_LOG")" = "https://github.com/EffortlessMetrics/perl-lsp/releases/download/v0.18.0/$ASSET" ]; then
    pass "required checksum is selected before archive download and exact bytes verify"
else
    fail_case "required checksum is selected before archive download and exact bytes verify" "status=$LAST_STATUS output=$LAST_OUTPUT log=$(cat "$CURL_LOG")"
fi

prepare_download
CURL_SUMS_FAIL=1
printf '%s  %s\n' "$ARCHIVE_HASH" "$ASSET" > "$SUMS_SOURCE"
run_download
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"failed to download required checksum manifest"* ]] \
    && [ "$(wc -l < "$CURL_LOG")" -eq 1 ]; then
    pass "missing checksum manifest stops before artifact download"
else
    fail_case "missing checksum manifest stops before artifact download" "status=$LAST_STATUS output=$LAST_OUTPUT log=$(cat "$CURL_LOG")"
fi

prepare_download
printf '%s  other.tar.gz\n' "$ARCHIVE_HASH" > "$SUMS_SOURCE"
run_download
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"contains no exact entry"* ]] \
    && [ "$(wc -l < "$CURL_LOG")" -eq 1 ]; then
    pass "missing asset checksum stops before artifact download"
else
    fail_case "missing asset checksum stops before artifact download" "status=$LAST_STATUS output=$LAST_OUTPUT log=$(cat "$CURL_LOG")"
fi

prepare_download
printf '%064d  %s\n' 0 "$ASSET" > "$SUMS_SOURCE"
run_download
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"checksum mismatch"* ]] \
    && [ "$(wc -l < "$CURL_LOG")" -eq 2 ]; then
    pass "archive checksum mismatch stops before extraction or promotion"
else
    fail_case "archive checksum mismatch stops before extraction or promotion" "status=$LAST_STATUS output=$LAST_OUTPUT log=$(cat "$CURL_LOG")"
fi

prepare_download
printf '%s  %s\n' "$ARCHIVE_HASH" "$ASSET" > "$SUMS_SOURCE"
CURL_ASSET_FAIL=1
run_download
assert_failure_contains "artifact download failure remains distinct" "download failed"

printf '\n=== Results: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
