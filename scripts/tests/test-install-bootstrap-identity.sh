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

# #16542/#16715: every identity-bound piped-bootstrap abort must keep its typed
# first line and then name both facts a stale curl|bash user needs: the
# ref+digest packet is unpublished at release closeout, and the POSIX manual
# archive works today. The destination must be the Linux/macOS procedure, not
# the identity-bound wrapper heading or the Windows zip section. A check for
# the words "manual archive" alone would pass that dead-end pointer.
POSIX_MANUAL_ARCHIVE_HEADING='## macOS and Linux manual archive'
POSIX_MANUAL_ARCHIVE_ANCHOR='macos-and-linux-manual-archive'

has_unpublished_packet_pointer() {
    local text="$1"
    [[ "$text" == *"release closeout"* ]] \
        && [[ "$text" == *"manual archive"* ]] \
        && [[ "$text" == *"docs/how-to/INSTALLATION.md"* ]] \
        && [[ "$text" == *"$POSIX_MANUAL_ARCHIVE_ANCHOR"* ]] \
        && {
            [[ "$text" == *"not yet published"* ]] \
                || [[ "$text" == *"has not been published"* ]]
        }
}

posix_manual_archive_section() {
    awk -v heading="$POSIX_MANUAL_ARCHIVE_HEADING" '
        $0 == heading { on = 1 }
        on && /^## / && $0 != heading { exit }
        on { print }
    ' "$ROOT/docs/how-to/INSTALLATION.md"
}

assert_identity_dead_end_is_actionable() {
    local name="$1" needle="$2"
    if [ "$LAST_STATUS" -ne 0 ] \
        && [[ "$LAST_OUTPUT" == *"$needle"* ]] \
        && has_unpublished_packet_pointer "$LAST_OUTPUT"; then
        pass "$name"
    else
        fail_case "$name" "expected failure containing '$needle' plus unpublished-packet/manual-archive pointer; status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
}

assert_lacks_unpublished_packet_pointer() {
    local name="$1"
    if [[ "$LAST_OUTPUT" == *"docs/how-to/INSTALLATION.md"* ]] \
        || [[ "$LAST_OUTPUT" == *"release closeout"* ]] \
        || [[ "$LAST_OUTPUT" == *"manual archive"* ]]; then
        fail_case "$name" "local/success path must not print the unpublished-packet pointer: $LAST_OUTPUT"
    else
        pass "$name"
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

printf '=== installer bootstrap identity contract (#6097, #16542) ===\n'

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
assert_lacks_unpublished_packet_pointer "clone-local success does not print the unpublished-packet pointer"

NO_CURL_BIN="$TMP/no-curl-bin"
mkdir -p "$NO_CURL_BIN"
ln -s /bin/bash "$NO_CURL_BIN/bash"
run_remote "$NO_CURL_BIN"
assert_identity_dead_end_is_actionable "missing curl is a piped-bootstrap dead end" "curl is required"
if [ ! -e "$CURL_LOG" ] && [ ! -e "$SENTINEL" ]; then
    pass "missing curl fails before network or execution"
else
    fail_case "missing curl fails before network or execution" "curl or installer was reached"
fi

run_remote "$FAKE_BIN:$PATH"
assert_identity_dead_end_is_actionable "remote bootstrap requires an explicit ref" "requires PERL_LSP_INSTALLER_REF"
if [ ! -e "$CURL_LOG" ] && [ ! -e "$SENTINEL" ]; then
    pass "missing ref fails before network or execution"
else
    fail_case "missing ref fails before network or execution" "curl or installer was reached"
fi

SHORT_REF="${COMMIT_REF%?}"
UPPER_REF="${COMMIT_REF^^}"
for bad_ref in main master HEAD refs/heads/main 'feature/test' '$(touch boom)' $'v0.18.0\nnext' "$TAG_REF" v1.2.3 "$SHORT_REF" "$UPPER_REF"; do
    run_remote "$FAKE_BIN:$PATH" \
        "PERL_LSP_INSTALLER_REF=$bad_ref" \
        "PERL_LSP_INSTALLER_SHA256=$DIGEST"
    if [ "$LAST_STATUS" -ne 0 ] \
        && [[ "$LAST_OUTPUT" == *"must be a full lowercase commit SHA"* ]] \
        && has_unpublished_packet_pointer "$LAST_OUTPUT" \
        && [ ! -e "$CURL_LOG" ] \
        && [ ! -e "$SENTINEL" ]; then
        pass "rejects mutable or shell-shaped ref: ${bad_ref//$'\n'/\\n}"
    else
        fail_case "rejects mutable or shell-shaped ref: ${bad_ref//$'\n'/\\n}" "status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
done

run_remote "$FAKE_BIN:$PATH" "PERL_LSP_INSTALLER_REF=$COMMIT_REF"
assert_identity_dead_end_is_actionable "remote bootstrap requires an exact digest" "must be exactly 64 lowercase hexadecimal characters"

run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=${DIGEST^^}"
assert_identity_dead_end_is_actionable "uppercase digest is rejected" "must be exactly 64 lowercase hexadecimal characters"

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
assert_lacks_unpublished_packet_pointer "verified success does not print the unpublished-packet pointer"

BAD_DIGEST="${DIGEST%?}0"
if [ "$BAD_DIGEST" = "$DIGEST" ]; then
    BAD_DIGEST="${DIGEST%?}1"
fi
run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$BAD_DIGEST"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"SHA-256 mismatch"* ]] \
    && has_unpublished_packet_pointer "$LAST_OUTPUT" \
    && [ ! -e "$SENTINEL" ]; then
    pass "digest mismatch fails before installer execution"
else
    fail_case "digest mismatch fails before installer execution" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

FAILING_CURL_BIN="$TMP/failing-curl-bin"
mkdir -p "$FAILING_CURL_BIN"
cat > "$FAILING_CURL_BIN/curl" <<'CURL'
#!/bin/bash
echo "fake curl: transport failed" >&2
exit 22
CURL
chmod +x "$FAILING_CURL_BIN/curl"
run_remote "$FAILING_CURL_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$DIGEST"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"failed to fetch the canonical installer"* ]] \
    && has_unpublished_packet_pointer "$LAST_OUTPUT" \
    && [ ! -e "$SENTINEL" ]; then
    pass "curl transport failure keeps the unpublished-packet pointer"
else
    fail_case "curl transport failure keeps the unpublished-packet pointer" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$DIGEST" \
    "FAKE_CURL_STATUS=404"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"HTTP 404"* ]] \
    && has_unpublished_packet_pointer "$LAST_OUTPUT" \
    && [ ! -e "$SENTINEL" ]; then
    pass "HTTP 404 is rejected with the unpublished-packet pointer"
else
    fail_case "HTTP 404 is rejected with the unpublished-packet pointer" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

run_remote "$FAKE_BIN:$PATH" \
    "PERL_LSP_INSTALLER_REF=$COMMIT_REF" \
    "PERL_LSP_INSTALLER_SHA256=$DIGEST" \
    "FAKE_CURL_STATUS=302"
if [ "$LAST_STATUS" -ne 0 ] \
    && [[ "$LAST_OUTPUT" == *"HTTP 302"* ]] \
    && has_unpublished_packet_pointer "$LAST_OUTPUT" \
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

# PATH contains no sha256 tool and no cat. The remedy must still print via
# shell builtins; a cat-heredoc pointer dies with 127 on this path.
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
    && has_unpublished_packet_pointer "$LAST_OUTPUT" \
    && [ ! -e "$SENTINEL" ]; then
    pass "absence of a SHA-256 tool fails closed"
else
    fail_case "absence of a SHA-256 tool fails closed" "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

# Pasting the INSTALLATION.md pointer at every fail() site would drift.
# A one-function owner is the only shape that keeps the four issue-named
# dead ends (missing ref, bad ref, HTTP 404, digest mismatch) on one text.
DOC_POINTER_HITS="$(grep -c 'docs/how-to/INSTALLATION.md' "$WRAPPER" || true)"
if [ "$DOC_POINTER_HITS" -eq 1 ]; then
    pass "unpublished-packet pointer is single-sourced in install.sh"
else
    fail_case "unpublished-packet pointer is single-sourced in install.sh" \
        "expected exactly one docs/how-to/INSTALLATION.md pointer, got $DOC_POINTER_HITS"
fi

WRAPPER_POINTER="$(grep 'docs/how-to/INSTALLATION.md' "$WRAPPER" || true)"
if [[ "$WRAPPER_POINTER" == *"$POSIX_MANUAL_ARCHIVE_ANCHOR"* ]] \
    && [[ "$WRAPPER_POINTER" != *"installer-script-macos-and-linux"* ]]; then
    pass "bootstrap pointer names the POSIX manual-archive heading"
else
    fail_case "bootstrap pointer names the POSIX manual-archive heading" \
        "expected $POSIX_MANUAL_ARCHIVE_ANCHOR without installer-script-macos-and-linux: $WRAPPER_POINTER"
fi

# A wrapper-section pointer would still contain INSTALLATION.md and "manual
# archive". The destination check must reject that dead end.
DEAD_END_POINTER=$'release closeout\nhas not been published\nThe manual archive install works today:\nsee docs/how-to/INSTALLATION.md (installer-script-macos-and-linux).'
if has_unpublished_packet_pointer "$DEAD_END_POINTER"; then
    fail_case "wrapper heading is not an actionable POSIX destination" \
        "has_unpublished_packet_pointer accepted installer-script-macos-and-linux"
else
    pass "wrapper heading is not an actionable POSIX destination"
fi

SECTION="$(posix_manual_archive_section)"
if [[ "$SECTION" == "$POSIX_MANUAL_ARCHIVE_HEADING"$'\n'* ]] \
    && [[ "$SECTION" == *'perllsp-${VERSION}-${TARGET}.tar.gz'* ]] \
    && [[ "$SECTION" == *'SHA256SUMS'* ]] \
    && [[ "$SECTION" == *'ROW="$(grep -F "$ASSET" SHA256SUMS)" || exit 1'* ]] \
    && [[ "$SECTION" == *'sha256sum'* ]] \
    && [[ "$SECTION" == *'shasum'* ]] \
    && [[ "$SECTION" == *'tar -xzf'* ]] \
    && [[ "$SECTION" == *'perl-dap'* ]] \
    && [[ "$SECTION" != *'.zip'* ]] \
    && [[ "$SECTION" != *'.exe'* ]]; then
    pass "POSIX manual-archive section documents tar.gz plus SHA256SUMS"
else
    fail_case "POSIX manual-archive section documents tar.gz plus SHA256SUMS" \
        "heading or matching asset/checksum/extract steps missing: $SECTION"
fi

# Follow the documented verify-then-extract steps on a local posix_nested_v1
# fixture. No unpublished installer ref/digest and no GitHub download.
FIXTURE="$TMP/manual-archive"
mkdir -p "$FIXTURE/perllsp-0.17.0-x86_64-unknown-linux-gnu"
printf 'fixture-perllsp\n' > "$FIXTURE/perllsp-0.17.0-x86_64-unknown-linux-gnu/perllsp"
printf 'fixture-perl-dap\n' > "$FIXTURE/perllsp-0.17.0-x86_64-unknown-linux-gnu/perl-dap"
printf 'fixture-readme\n' > "$FIXTURE/perllsp-0.17.0-x86_64-unknown-linux-gnu/README.md"
ASSET="perllsp-0.17.0-x86_64-unknown-linux-gnu.tar.gz"
(
    cd "$FIXTURE"
    tar -czf "$ASSET" perllsp-0.17.0-x86_64-unknown-linux-gnu
    sha256sum "$ASSET" > SHA256SUMS
    printf 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  perllsp-0.17.0-x86_64-pc-windows-msvc.zip\n' >> SHA256SUMS
)
EXTRACT="$TMP/manual-archive-extract"
mkdir -p "$EXTRACT"
(
    cd "$EXTRACT"
    cp "$FIXTURE/$ASSET" "$FIXTURE/SHA256SUMS" .
    ROW="$(grep -F "$ASSET" SHA256SUMS)" || exit 1
    printf '%s\n' "$ROW" | sha256sum -c -
    tar -xzf "$ASSET"
)
if grep -qx 'fixture-perllsp' "$EXTRACT/perllsp-0.17.0-x86_64-unknown-linux-gnu/perllsp" \
    && grep -qx 'fixture-perl-dap' "$EXTRACT/perllsp-0.17.0-x86_64-unknown-linux-gnu/perl-dap"; then
    pass "documented POSIX archive steps extract a local fixture without installer identity"
else
    fail_case "documented POSIX archive steps extract a local fixture without installer identity" \
        "verify/extract did not produce the fixture binaries"
fi

MISSING_ROW="$TMP/manual-archive-missing-row"
mkdir -p "$MISSING_ROW"
cp "$FIXTURE/$ASSET" "$MISSING_ROW/"
printf 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  other.tar.gz\n' \
    > "$MISSING_ROW/SHA256SUMS"
set +e
MISSING_OUTPUT="$(
    cd "$MISSING_ROW"
    ROW="$(grep -F "$ASSET" SHA256SUMS)" || exit 1
    printf '%s\n' "$ROW" | sha256sum -c -
    tar -xzf "$ASSET"
    echo EXTRACTED
)"
MISSING_STATUS=$?
set -e
if [ "$MISSING_STATUS" -ne 0 ] \
    && [[ "$MISSING_OUTPUT" != *"EXTRACTED"* ]] \
    && [ ! -e "$MISSING_ROW/perllsp-0.17.0-x86_64-unknown-linux-gnu" ]; then
    pass "missing SHA256SUMS row fails closed before extract"
else
    fail_case "missing SHA256SUMS row fails closed before extract" \
        "status=$MISSING_STATUS output=$MISSING_OUTPUT"
fi

printf '\n=== Results: %d passed, %d failed ===\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
