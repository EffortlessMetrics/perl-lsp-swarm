#!/usr/bin/env bash
# Canonical scripts/install.sh must use the root wrapper's fixed-slot
# positional grammar (#16310 item 4).
#
# Slot 1 is always VERSION and slot 2 is always INSTALL_DIR. An environment
# value wins its matching slot and does not consume a later positional.
# A third leading positional fails closed. Flags are not positionals: a
# leftover non-flag after --print-target is still unknown.
#
# This instrument sources the installer with PERL_LSP_INSTALLER_LIBRARY_ONLY=1
# so argument binding is observable without a download. expect_bind always
# takes an explicit stdout needle (or empty) so bash cannot be rebound as a
# grep pattern (#16343).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
WRAPPER="$ROOT/install.sh"
DOCS="$ROOT/docs/how-to/INSTALLATION.md"
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

# $1 name
# $2 expected status
# $3 VERSION env (empty means unset)
# $4 INSTALL_DIR env (empty means unset)
# remaining: installer argv
probe_canonical() {
    local _name="$1" _want_status="$2" _ver_env="$3" _dir_env="$4"
    shift 4
    local _env=(
        PERL_LSP_INSTALLER_LIBRARY_ONLY=1
    )
    if [ -n "$_ver_env" ]; then
        _env+=("VERSION=$_ver_env")
    else
        _env+=("VERSION=")
    fi
    if [ -n "$_dir_env" ]; then
        _env+=("INSTALL_DIR=$_dir_env")
    else
        _env+=("INSTALL_DIR=")
    fi

    set +e
    LAST_OUTPUT="$(
        env -u VERSION -u INSTALL_DIR "${_env[@]}" bash -c '
            source "$0"
            printf "VERSION=%s\nINSTALL_DIR=%s\nPRINT_TARGET=%s\n" \
                "$VERSION" "$INSTALL_DIR" "$PRINT_TARGET"
        ' "$INSTALLER" "$@" 2>&1
    )"
    LAST_STATUS=$?
    set -e
}

expect_bind() {
    local _name="$1" _want_status="$2" _want_version="$3" _want_dir="$4" _want_print="$5"
    shift 5
    probe_canonical "$_name" "$_want_status" "$@"
    local _ok=1
    if [ "$LAST_STATUS" -ne "$_want_status" ]; then
        _ok=0
    fi
    if [ "$_want_status" -eq 0 ]; then
        printf '%s\n' "$LAST_OUTPUT" | grep -qx "VERSION=${_want_version}" || _ok=0
        printf '%s\n' "$LAST_OUTPUT" | grep -qx "INSTALL_DIR=${_want_dir}" || _ok=0
        printf '%s\n' "$LAST_OUTPUT" | grep -qx "PRINT_TARGET=${_want_print}" || _ok=0
    fi
    if [ "$_ok" -eq 1 ]; then
        pass "$_name"
    else
        fail_case "$_name" "want status=$_want_status VERSION=$_want_version INSTALL_DIR=$_want_dir PRINT_TARGET=$_want_print; got status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
}

expect_fail_needle() {
    local _name="$1" _needle="$2"
    shift 2
    probe_canonical "$_name" 1 "$@"
    if [ "$LAST_STATUS" -ne 0 ] && [[ "$LAST_OUTPUT" == *"$_needle"* ]]; then
        pass "$_name"
    else
        fail_case "$_name" "want non-zero containing '$_needle'; got status=$LAST_STATUS output=$LAST_OUTPUT"
    fi
}

# ── Canonical fixed-slot grammar ────────────────────────────────────────────

expect_bind "two positionals fill VERSION then INSTALL_DIR" \
    0 "1.2.3" "/tmp/arg-surface-bin" "1" \
    "" "" \
    1.2.3 /tmp/arg-surface-bin --print-target

expect_bind "VERSION env wins slot 1; first positional does not become INSTALL_DIR" \
    0 "v9.9.9" "/tmp/arg-surface-bin" "1" \
    "v9.9.9" "" \
    ignored /tmp/arg-surface-bin --print-target

expect_bind "INSTALL_DIR env wins slot 2; second positional is not a third error" \
    0 "1.2.3" "/tmp/from-env" "1" \
    "" "/tmp/from-env" \
    1.2.3 ignored --print-target

expect_bind "both env values win; both positionals occupy their slots" \
    0 "v9.9.9" "/tmp/from-env" "1" \
    "v9.9.9" "/tmp/from-env" \
    ignored also-ignored --print-target

expect_fail_needle "a third leading positional fails closed" \
    "unexpected argument" \
    "" "" \
    1.2.3 /tmp/arg-surface-bin extra

expect_fail_needle "unknown flag still fails closed" \
    "unknown argument" \
    "" "" \
    --nope

expect_fail_needle "a non-flag after --print-target is not a VERSION slot" \
    "unknown argument" \
    "" "" \
    --print-target 1.2.3

# Production path: positionals plus --print-target must reach detect_platform
# and exit 0 without downloading (#16310). A parser that still rejects the
# prefix never gets here.
set +e
LAST_OUTPUT="$(bash "$INSTALLER" 1.2.3 "$TMP/live-bin" --print-target 2>&1)"
LAST_STATUS=$?
set -e
if [ "$LAST_STATUS" -eq 0 ] && [ -n "$LAST_OUTPUT" ]; then
    pass "live --print-target accepts prefix positionals and prints a target"
else
    fail_case "live --print-target accepts prefix positionals and prints a target" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

# ── Help / docs surfaces ────────────────────────────────────────────────────

set +e
LAST_OUTPUT="$(bash "$INSTALLER" --help 2>&1)"
LAST_STATUS=$?
set -e
if [ "$LAST_STATUS" -eq 0 ] \
    && printf '%s\n' "$LAST_OUTPUT" | grep -qE '^Usage:.*\[VERSION\].*\[INSTALL_DIR\]'; then
    pass "--help Usage line documents positional VERSION and INSTALL_DIR"
else
    fail_case "--help Usage line documents positional VERSION and INSTALL_DIR" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

if grep -q 'takes flags only' "$DOCS"; then
    fail_case "INSTALLATION.md no longer claims canonical flags-only" \
        "docs still say canonical takes flags only"
else
    if grep -q 'fixed-slot' "$DOCS" && grep -q 'environment variables win' "$DOCS"; then
        pass "INSTALLATION.md documents the shared fixed-slot grammar"
    else
        fail_case "INSTALLATION.md documents the shared fixed-slot grammar" \
            "missing fixed-slot / env-wins wording"
    fi
fi

# ── Wrapper parity (clone-local stub; no network) ───────────────────────────

CHECKOUT="$TMP/checkout"
mkdir -p "$CHECKOUT/scripts"
cp "$WRAPPER" "$CHECKOUT/install.sh"
cat > "$CHECKOUT/scripts/install.sh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'STUB VERSION=%s INSTALL_DIR=%s argc=%s args=%s\n' \
    "${VERSION:-}" "${INSTALL_DIR:-}" "$#" "$*"
STUB
chmod +x "$CHECKOUT/scripts/install.sh"

# The wrapper peels leading positionals then execs canonical with remaining flags.
set +e
LAST_OUTPUT="$(bash "$CHECKOUT/install.sh" 1.2.3 /tmp/arg-surface-bin --print-target 2>&1)"
LAST_STATUS=$?
set -e
if [ "$LAST_STATUS" -eq 0 ] \
    && [[ "$LAST_OUTPUT" == *"STUB VERSION=1.2.3 INSTALL_DIR=/tmp/arg-surface-bin"* ]] \
    && [[ "$LAST_OUTPUT" == *"argc=1"* ]] \
    && [[ "$LAST_OUTPUT" == *"args=--print-target"* ]]; then
    pass "wrapper peels VERSION then INSTALL_DIR and forwards flags"
else
    fail_case "wrapper peels VERSION then INSTALL_DIR and forwards flags" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

set +e
LAST_OUTPUT="$(VERSION=v9.9.9 bash "$CHECKOUT/install.sh" ignored /tmp/arg-surface-bin --print-target 2>&1)"
LAST_STATUS=$?
set -e
if [ "$LAST_STATUS" -eq 0 ] \
    && [[ "$LAST_OUTPUT" == *"STUB VERSION=v9.9.9 INSTALL_DIR=/tmp/arg-surface-bin"* ]] \
    && [[ "$LAST_OUTPUT" == *"argc=1"* ]]; then
    pass "wrapper VERSION env wins slot 1 without shifting INSTALL_DIR"
else
    fail_case "wrapper VERSION env wins slot 1 without shifting INSTALL_DIR" \
        "status=$LAST_STATUS output=$LAST_OUTPUT"
fi

printf 'summary: PASS=%d FAIL=%d\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
