#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WRAPPER="$ROOT/install.sh"
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

assert_status() {
    local name="$1" expected="$2"
    if [ "$LAST_STATUS" -eq "$expected" ]; then
        pass "$name"
    else
        fail_case "$name" "expected status $expected, got $LAST_STATUS: $LAST_OUTPUT"
    fi
}

assert_failure_contains() {
    local name="$1" needle="$2"
    if [ "$LAST_STATUS" -ne 0 ] && [[ "$LAST_OUTPUT" == *"$needle"* ]]; then
        pass "$name"
    else
        fail_case "$name" "expected failure containing '$needle'; status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
}

PAYLOAD="$TMP/canonical-installer.sh"
cat > "$PAYLOAD" <<'PAYLOAD'
#!/usr/bin/env bash
set -euo pipefail
{
    printf 'version=%s\n' "${VERSION:-}"
    printf 'install_dir=%s\n' "${INSTALL_DIR:-}"
    printf 'args=%s\n' "$*"
} > "$INSTALLER_SENTINEL"
PAYLOAD
chmod +x "$PAYLOAD"

DIGEST="$(python3 - "$PAYLOAD" <<'PY'
import hashlib
import pathlib
import sys
print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
COMMIT_REF="0123456789abcdef0123456789abcdef01234567"
TAG_REF="v0.18.0-rc.1"
SENTINEL="$TMP/executed"
CURL_LOG="$TMP/curl.log"

FAKE_BIN="$TMP/bin"
mkdir -p "$FAKE_BIN"
cat > "$FAKE_BIN/curl" <<'CURL'
#!/bin/bash
set -euo pipefail
out=""
url=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            out="$2"
            shift 2
            ;;
        --proto|--write-out)
            shift 2
            ;;
        --silent|--show-error)
            shift
            ;;
        -L|--location)
            echo "fake curl: redirect-following flags are not supported in bootstrap tests" >&2
            exit 1
            ;;
        *)
            url="$1"
            shift
            ;;
    esac
done
printf '%s\n' "$url" > "$FAKE_CURL_LOG"
cp "$FAKE_INSTALLER_PAYLOAD" "$out"
printf '%s' "${FAKE_CURL_STATUS:-200}"
CURL
chmod +x "$FAKE_BIN/curl"

run_remote() {
    local command_path="$1"
    shift
    rm -f "$SENTINEL" "$CURL_LOG"
    set +e
    LAST_OUTPUT="$(
        cat "$WRAPPER" | env \
            PATH="$command_path" \
            FAKE_INSTALLER_PAYLOAD="$PAYLOAD" \
            FAKE_CURL_LOG="$CURL_LOG" \
            INSTALLER_SENTINEL="$SENTINEL" \
            "$@" \
            bash -s -- --probe 2>&1
    )"
    LAST_STATUS=$?
    set -e
}

printf '=== installer bootstrap identity contract (#6097) ===\n'

# Clone-local execution must remain independent of the remote bootstrap inputs.
LOCAL_ROOT="$TMP/local"
mkdir -p "$LOCAL_ROOT/scripts"
cp "$WRAPPER" "$LOCAL_ROOT/install.sh"
cp "$PAYLOAD" "$LOCAL_ROOT/scripts/install.sh"
chmod +x "$LOCAL_ROOT/scripts/install.sh"
rm -f "$SENTINEL"
set +e
LAST_OUTPUT="$(INSTALLER_SENTINEL="$SENTINEL" bash "$LOCAL_ROOT/install.sh" 1.2.3 "$TMP/bin-out" --local 2>&1)"
LAST_STATUS=$?
set -e
if [ "$LAST_STATUS" -eq 0 ] \
    && grep -qx 'version=1.2.3' "$SENTINEL" \
    && grep -qx "install_dir=$TMP/bin-out" "$SENTINEL" \
    && grep -qx 'args=--local' "$SENTINEL"; then
    pass "clone-local wrapper executes the sibling installer"
else
    fail_case "clone-local wrapper executes the sibling installer" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

run_remote "$FAKE_BIN:$PATH"
assert_failure_contains "remote bootstrap requires an explicit ref" "requires PERL_LSP_INSTALLER_REF"
if [ ! -e "$CURL_LOG" ] && [ ! -e "$SENTINEL" ]; then
    pass "missing ref fails before network or execution"
else
    fail_case "missing ref fails before network or execution" "curl or installer was reached"
fi

for bad_ref in main master HEAD refs/heads/main 'feature/test' '$(touch boom)' $'v0.18.0\nnext' "$TAG_REF" v1.2.3; do
    run_remote "$FAKE_BIN:$PATH" \
        "PERL_LSP_INSTALLER_REF=$bad_ref" \
        "PERL_LSP_INSTALLER_SHA256=$DIGEST"
    if [ "$LAST_STATUS" -ne 0 ] \
        && [[ "$LAST_OUTPUT" == *"must be a full lowercase commit SHA"* ]] \
        && [ ! -e "$CURL_LOG" ] \
        && [ ! -e "$SENTINEL" ]; then
        pass "rejects mutable or shell-shaped ref: ${bad_ref//$'\n'/\\n}"
    else
        fail_case "rejects mutable or shell-shaped ref: ${bad_ref//$'\n'/\\n}" "status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
done

run_remote "$FAKE_BIN:$PATH" "PERL_LSP_INSTALLER_REF=$COMMIT_REF"
assert_failure_contains "remote bootstrap requires an exact digest" "must be exactly 64 lowercase hexadecimal characters"

run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=${DIGEST^^}"
assert_failure_contains "uppercase digest is rejected" "must be exactly 64 lowercase hexadecimal characters"

run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$DIGEST"
EXPECTED_URL="https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/$COMMIT_REF/scripts/install.sh"
if [ "$LAST_STATUS" -eq 0 ] \
    && [ -f "$SENTINEL" ] \
    && grep -qx 'args=--probe' "$SENTINEL" \
    && grep -qx "$EXPECTED_URL" "$CURL_LOG"; then
    pass "verified commit-bound installer executes with preserved arguments"
else
    fail_case "verified commit-bound installer executes with preserved arguments" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

BAD_DIGEST="${DIGEST%?}0"
if [ "$BAD_DIGEST" = "$DIGEST" ]; then
    BAD_DIGEST="${DIGEST%?}1"
fi
run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$BAD_DIGEST"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"SHA-256 mismatch"* ]] \
    && [ ! -e "$SENTINEL" ]; then
    pass "digest mismatch fails before installer execution"
else
    fail_case "digest mismatch fails before installer execution" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$DIGEST" \
    "FAKE_CURL_STATUS=302"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"HTTP 302"* ]] \
    && [ ! -e "$SENTINEL" ]; then
    pass "redirect is rejected as a different installer source"
else
    fail_case "redirect is rejected as a different installer source" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

set +e
LAST_OUTPUT="$(
    FAKE_CURL_LOG="$CURL_LOG" \
    FAKE_INSTALLER_PAYLOAD="$PAYLOAD" \
    "$FAKE_BIN/curl" --location https://example.com --output "$TMP/fake-out" 2>&1
)"
LAST_STATUS=$?
set -e
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"redirect-following flags are not supported"* ]]; then
    pass "fake curl rejects --location before any installer bytes are copied"
else
    fail_case "fake curl rejects --location before any installer bytes are copied" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

NO_SHA_BIN="$TMP/no-sha-bin"
mkdir -p "$NO_SHA_BIN"
ln -s /bin/bash "$NO_SHA_BIN/bash"
ln -s /bin/cp "$NO_SHA_BIN/cp"
ln -s /bin/rm "$NO_SHA_BIN/rm"
ln -s "$(command -v mktemp)" "$NO_SHA_BIN/mktemp"
cp "$FAKE_BIN/curl" "$NO_SHA_BIN/curl"
run_remote "$NO_SHA_BIN" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$DIGEST"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"sha256sum or shasum is required"* ]] \
    && [ ! -e "$SENTINEL" ]; then
    pass "absence of a SHA-256 tool fails closed"
else
    fail_case "absence of a SHA-256 tool fails closed" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf '\n=== Results: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
