#!/usr/bin/env bash
# Termux detection and wrapper-ownership contract (#14605).
#
# Production truth on current main:
#   * root install.sh is the identity-bound bootstrap wrapper; it does not
#     detect Termux or select a libc/target.
#   * scripts/install.sh detects Termux via TERMUX_VERSION or the usr/bin
#     sentinel and, on Linux, selects source-build mode because there is no
#     Android/bionic release asset. It does not download a linux-musl archive
#     for Termux.
#   * musl remains selectable for non-Termux Linux via PERL_LSP_LINUX_LIBC.
#   * an explicit INSTALL_DIR (env or wrapper positional) beats the Termux
#     prefix default.
#
# Proof drives the real installer: --print-target for mode/target, and
# PERL_LSP_INSTALLER_LIBRARY_ONLY=1 for INSTALL_DIR resolution. A hardcoded
# snippet that reimplements detection is not evidence.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ROOT_INSTALLER="$ROOT/install.sh"
CANONICAL_INSTALLER="$ROOT/scripts/install.sh"
TERMUX_PREFIX="/data/data/com.termux/files/usr/bin"

PASS=0
FAIL=0
HARNESS_TMP=""

cleanup() {
    if [[ -n "${HARNESS_TMP:-}" && -d "$HARNESS_TMP" ]]; then
        rm -rf "$HARNESS_TMP"
    fi
}
trap cleanup EXIT

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail() {
    printf 'FAIL  %s\n' "$1"
    printf '      %s\n' "$2"
    FAIL=$((FAIL + 1))
}

code_lines() {
    sed -E 's/[[:space:]]*#.*$//' "$1"
}

host_linux_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64|x64) printf '%s\n' "x86_64" ;;
        aarch64|arm64) printf '%s\n' "aarch64" ;;
        *) printf '%s\n' "$arch" ;;
    esac
}

assert_file_exists() {
    local label="$1" path="$2"
    if [[ ! -f "$path" ]]; then
        fail "$label" "missing $path"
        return 1
    fi
    pass "$label"
}

assert_stdout() {
    local label="$1"
    local expected="$2"
    shift 2

    local output status
    set +e
    output="$("$@" 2>&1)"
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $output"
        return
    fi
    if [[ "$output" != "$expected" ]]; then
        fail "$label" "expected '$expected', got '$output'"
        return
    fi
    pass "$label"
}

assert_stdout_matches() {
    local label="$1"
    local pattern="$2"
    shift 2

    local output status
    set +e
    output="$("$@" 2>&1)"
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $output"
        return
    fi
    if [[ ! "$output" =~ $pattern ]]; then
        fail "$label" "expected output matching /$pattern/, got '$output'"
        return
    fi
    pass "$label"
}

canonical_install_dir() {
    # Caller supplies INSTALL_DIR / TERMUX_* in this function's environment.
    # `env` cannot invoke a shell function, so tests call this directly.
    (
        set --
        export PERL_LSP_INSTALLER_LIBRARY_ONLY=1
        # shellcheck disable=SC1090
        source "$CANONICAL_INSTALLER" >/dev/null
        printf '%s\n' "${INSTALL_DIR}"
    )
}

setup_wrapper_stub() {
    local wrap_root="$1"
    mkdir -p "$wrap_root/scripts"
    cp "$ROOT_INSTALLER" "$wrap_root/install.sh"
    cat > "$wrap_root/scripts/install.sh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'STUB argc=%s\n' "$#"
printf 'STUB VERSION=%s INSTALL_DIR=%s TERMUX_VERSION=%s\n' \
    "${VERSION:-}" "${INSTALL_DIR:-}" "${TERMUX_VERSION:-}"
printf 'STUB args=%s\n' "$*"
STUB
    chmod +x "$wrap_root/install.sh" "$wrap_root/scripts/install.sh"
}

# ── Wrapper ownership (retargeted from the three root-install.sh failures) ────

assert_file_exists "root install.sh exists" "$ROOT_INSTALLER"
assert_file_exists "canonical scripts/install.sh exists" "$CANONICAL_INSTALLER"

if grep -q 'TERMUX_VERSION' "$ROOT_INSTALLER"; then
    fail "install.sh: wrapper does not own Termux detection" \
        "root install.sh contains TERMUX_VERSION; Termux detection belongs in scripts/install.sh"
else
    pass "install.sh: wrapper does not own Termux detection"
fi

if grep -qE 'linux-musl|_libc=.*musl|unknown-linux-' "$ROOT_INSTALLER"; then
    fail "install.sh: wrapper does not select targets" \
        "root install.sh appears to select a Linux target; that belongs in scripts/install.sh"
else
    pass "install.sh: wrapper does not select targets"
fi

if ! grep -q 'scripts/install.sh' "$ROOT_INSTALLER"; then
    fail "install.sh: delegates to canonical installer" \
        "root install.sh does not name scripts/install.sh"
else
    pass "install.sh: delegates to canonical installer"
fi

# Positional INSTALL_DIR is the second non-flag argument after VERSION is
# shifted into $1. The old ${2} regex was a stale wrapper shape.
HARNESS_TMP="$(mktemp -d)"
WRAP_ROOT="$HARNESS_TMP/wrap"
setup_wrapper_stub "$WRAP_ROOT"

wrap_out=""
wrap_status=0
set +e
wrap_out="$(bash "$WRAP_ROOT/install.sh" 0.17.0 /tmp/perl-lsp-termux-selftest-bin --print-target 2>&1)"
wrap_status=$?
set -e
if [[ "$wrap_status" -ne 0 ]]; then
    fail "install.sh: positional INSTALL_DIR is exported before exec" \
        "expected exit 0, got $wrap_status; output: $wrap_out"
elif [[ "$wrap_out" != *"INSTALL_DIR=/tmp/perl-lsp-termux-selftest-bin"* ]]; then
    fail "install.sh: positional INSTALL_DIR is exported before exec" \
        "positional mapping missing; output: $wrap_out"
elif [[ "$wrap_out" != *"STUB args=--print-target"* ]]; then
    fail "install.sh: positional INSTALL_DIR is exported before exec" \
        "consumed positionals must not be forwarded; output: $wrap_out"
else
    pass "install.sh: positional INSTALL_DIR is exported before exec"
fi

set +e
wrap_out="$(
    env INSTALL_DIR=/tmp/env-wins TERMUX_VERSION=0.118 \
        bash "$WRAP_ROOT/install.sh" 0.17.0 /tmp/positional-loses 2>&1
)"
wrap_status=$?
set -e
if [[ "$wrap_status" -ne 0 ]]; then
    fail "install.sh: env INSTALL_DIR beats positional and Termux default" \
        "expected exit 0, got $wrap_status; output: $wrap_out"
elif [[ "$wrap_out" != *"INSTALL_DIR=/tmp/env-wins"* ]]; then
    fail "install.sh: env INSTALL_DIR beats positional and Termux default" \
        "env override lost; output: $wrap_out"
elif [[ "$wrap_out" == *"INSTALL_DIR=/tmp/positional-loses"* ]]; then
    fail "install.sh: env INSTALL_DIR beats positional and Termux default" \
        "positional overwrote env; output: $wrap_out"
else
    pass "install.sh: env INSTALL_DIR beats positional and Termux default"
fi

set +e
wrap_out="$(env TERMUX_VERSION=0.118 bash "$WRAP_ROOT/install.sh" 0.17.0 2>&1)"
wrap_status=$?
set -e
if [[ "$wrap_status" -ne 0 ]]; then
    fail "install.sh: wrapper does not invent a Termux INSTALL_DIR" \
        "expected exit 0, got $wrap_status; output: $wrap_out"
elif [[ "$wrap_out" == *"INSTALL_DIR=$TERMUX_PREFIX"* ]]; then
    fail "install.sh: wrapper does not invent a Termux INSTALL_DIR" \
        "wrapper assigned the Termux prefix; output: $wrap_out"
elif [[ "$wrap_out" != *"INSTALL_DIR= TERMUX_VERSION="* ]]; then
    fail "install.sh: wrapper does not invent a Termux INSTALL_DIR" \
        "expected empty INSTALL_DIR forwarded to the canonical installer; output: $wrap_out"
else
    pass "install.sh: wrapper does not invent a Termux INSTALL_DIR"
fi

# ── Canonical detection present (kept; not deleted) ───────────────────────────

if ! grep -q 'TERMUX_VERSION' "$CANONICAL_INSTALLER"; then
    fail "scripts/install.sh: TERMUX_VERSION check missing" \
        "canonical installer lost TERMUX_VERSION detection"
else
    pass "scripts/install.sh: Termux detection block present"
fi

if ! grep -q "$TERMUX_PREFIX" "$CANONICAL_INSTALLER"; then
    fail "scripts/install.sh: Termux usr/bin path check missing" \
        "canonical installer lost the Termux usr/bin sentinel"
else
    pass "scripts/install.sh: Termux usr/bin path check present"
fi

termux_version_hits="$(code_lines "$CANONICAL_INSTALLER" | grep -c 'TERMUX_VERSION' || true)"
if [[ "$termux_version_hits" -ne 1 ]]; then
    fail "scripts/install.sh: TERMUX_VERSION has one owner" \
        "expected one non-comment TERMUX_VERSION use (is_termux_environment), got $termux_version_hits"
else
    pass "scripts/install.sh: TERMUX_VERSION has one owner"
fi

prefix_hits="$(code_lines "$CANONICAL_INSTALLER" | grep -c "$TERMUX_PREFIX" || true)"
if [[ "$prefix_hits" -ne 1 ]]; then
    fail "scripts/install.sh: Termux usr/bin path has one spelling" \
        "expected one non-comment $TERMUX_PREFIX (termux_usr_bin), got $prefix_hits"
else
    pass "scripts/install.sh: Termux usr/bin path has one spelling"
fi

if ! grep -q 'is_termux_environment' "$CANONICAL_INSTALLER"; then
    fail "scripts/install.sh: is_termux_environment helper present" \
        "canonical installer has no shared Termux detector"
else
    pass "scripts/install.sh: is_termux_environment helper present"
fi

# ── Musl is selectable for non-Termux Linux (retargeted grep) ─────────────────

musl_target="$(host_linux_arch)-unknown-linux-musl"
assert_stdout \
    "scripts/install.sh: linux-musl target is selectable via PERL_LSP_LINUX_LIBC" \
    "$musl_target" \
    env -u TERMUX_VERSION -u TERMUX_USR_BIN_OVERRIDE PERL_LSP_LINUX_LIBC=musl \
    bash "$CANONICAL_INSTALLER" --print-target

assert_stdout_matches \
    "non-Termux host: --print-target is a Linux release triple, not source" \
    '^[a-z0-9_]+-unknown-linux-(gnu|musl)$' \
    env -u TERMUX_VERSION -u TERMUX_USR_BIN_OVERRIDE \
    bash "$CANONICAL_INSTALLER" --print-target

# ── Termux forces source mode (replaces vacuous musl-snippet simulation) ──────

assert_stdout \
    "scripts/install.sh: TERMUX_VERSION selects source mode, not musl" \
    "source" \
    env TERMUX_VERSION=0.118 bash "$CANONICAL_INSTALLER" --print-target

assert_stdout \
    "scripts/install.sh: TERMUX_VERSION forces source even when PREFER_GNU=1" \
    "source" \
    env TERMUX_VERSION=0.118 PREFER_GNU=1 bash "$CANONICAL_INSTALLER" --print-target

assert_stdout \
    "scripts/install.sh: TERMUX_VERSION forces source even when PERL_LSP_LINUX_LIBC=musl" \
    "source" \
    env TERMUX_VERSION=0.118 PERL_LSP_LINUX_LIBC=musl bash "$CANONICAL_INSTALLER" --print-target

assert_stdout \
    "scripts/install.sh: TERMUX_VERSION forces source even when PERL_LSP_LINUX_LIBC=gnu" \
    "source" \
    env TERMUX_VERSION=0.118 PERL_LSP_LINUX_LIBC=gnu bash "$CANONICAL_INSTALLER" --print-target

assert_stdout \
    "install.sh: wrapper TERMUX_VERSION --print-target follows canonical source mode" \
    "source" \
    env TERMUX_VERSION=0.118 bash "$ROOT_INSTALLER" --print-target

# Empty TERMUX_VERSION is unset-equivalent for `[ -n ... ]`.
assert_stdout_matches \
    "empty TERMUX_VERSION does not select Termux source mode" \
    '^[a-z0-9_]+-unknown-linux-(gnu|musl)$' \
    env TERMUX_VERSION= bash "$CANONICAL_INSTALLER" --print-target

# ── Directory probe via TERMUX_USR_BIN_OVERRIDE ───────────────────────────────

probe_dir="$HARNESS_TMP/termux-usr-bin"
mkdir -p "$probe_dir"

assert_stdout \
    "Termux usr/bin sentinel selects source without TERMUX_VERSION" \
    "source" \
    env -u TERMUX_VERSION TERMUX_USR_BIN_OVERRIDE="$probe_dir" \
    bash "$CANONICAL_INSTALLER" --print-target

assert_stdout_matches \
    "missing Termux usr/bin sentinel does not select source" \
    '^[a-z0-9_]+-unknown-linux-(gnu|musl)$' \
    env -u TERMUX_VERSION TERMUX_USR_BIN_OVERRIDE="$HARNESS_TMP/no-such-termux-usr-bin" \
    bash "$CANONICAL_INSTALLER" --print-target

assert_stdout \
    "TERMUX_VERSION still selects source when the usr/bin sentinel is absent" \
    "source" \
    env TERMUX_VERSION=0.118 TERMUX_USR_BIN_OVERRIDE="$HARNESS_TMP/no-such-termux-usr-bin" \
    bash "$CANONICAL_INSTALLER" --print-target

# ── INSTALL_DIR default vs caller override ────────────────────────────────────

assert_install_dir() {
    local label="$1"
    local expected="$2"
    local comparison="$3"
    local got_dir dir_status
    shift 3

    set +e
    got_dir="$(
        unset INSTALL_DIR TERMUX_VERSION TERMUX_USR_BIN_OVERRIDE 2>/dev/null || true
        "$@"
        canonical_install_dir
    )"
    dir_status=$?
    set -e
    if [[ "$dir_status" -ne 0 ]]; then
        fail "$label" "library-only source failed with status $dir_status; output: $got_dir"
        return
    fi
    if [[ "$comparison" == "eq" && "$got_dir" != "$expected" ]]; then
        fail "$label" "expected '$expected', got '$got_dir'"
        return
    fi
    if [[ "$comparison" == "ne" && "$got_dir" == "$expected" ]]; then
        fail "$label" "got unexpected '$got_dir'"
        return
    fi
    pass "$label"
}

# Each helper below is a tiny env setup evaluated in assert_install_dir's subshell.
termux_version_only() { export TERMUX_VERSION=0.118; }
termux_caller_override() { export TERMUX_VERSION=0.118 INSTALL_DIR=/tmp/caller-wins; }
termux_probe_only() { export TERMUX_USR_BIN_OVERRIDE="$probe_dir"; }
non_termux_host() { :; }

assert_install_dir \
    "scripts/install.sh: Termux default INSTALL_DIR is the Termux prefix" \
    "$TERMUX_PREFIX" eq termux_version_only

assert_install_dir \
    "scripts/install.sh: caller INSTALL_DIR override is honored before Termux default" \
    "/tmp/caller-wins" eq termux_caller_override

assert_install_dir \
    "scripts/install.sh: directory-probe Termux default uses the sentinel path" \
    "$probe_dir" eq termux_probe_only

assert_install_dir \
    "non-Termux host: INSTALL_DIR is not the Termux prefix" \
    "$TERMUX_PREFIX" ne non_termux_host

# ── Report ────────────────────────────────────────────────────────────────────

echo
echo "── Summary ──"
echo "Passed: $PASS"
echo "Failed: $FAIL"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
