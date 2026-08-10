#!/usr/bin/env bash
# Self-test for the zero-argument installer wrapper path.
#
# Issue #5448 established the Bash 3.2/set -u compatibility contract. Issue
# #6097 changes the remote path's trust contract: stdin invocation remains
# zero-argument, but it must now carry an immutable/release-shaped ref plus the
# reviewed SHA-256 digest of the canonical installer.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ROOT_INSTALLER="$ROOT/install.sh"
CANONICAL_INSTALLER="$ROOT/scripts/install.sh"
LEGACY_BASH_IMAGE="${PERL_LSP_LEGACY_BASH_IMAGE:-bash:3.2}"

PASS=0
FAIL=0
SKIP=0
WORKDIR=""

cleanup() {
    if [[ -n "${WORKDIR:-}" && -d "$WORKDIR" ]]; then
        rm -rf "$WORKDIR"
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

skip() {
    printf 'SKIP  %s\n' "$1"
    SKIP=$((SKIP + 1))
}

sha256_file() {
    local path="$1" output
    if command -v sha256sum >/dev/null 2>&1; then
        output="$(sha256sum "$path")"
    elif command -v shasum >/dev/null 2>&1; then
        output="$(shasum -a 256 "$path")"
    else
        return 1
    fi
    printf '%s\n' "${output%% *}"
}

WORKDIR="$(mktemp -d)"

# A stub that stands in for scripts/install.sh. It reports how many arguments
# the wrapper forwarded, so the zero-argument path is observable without
# touching the network or release assets.
STUB="$WORKDIR/stub-install.sh"
cat > "$STUB" <<'STUB_EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'STUB argc=%s\n' "$#"
printf 'STUB VERSION=%s INSTALL_DIR=%s\n' "${VERSION:-}" "${INSTALL_DIR:-}"
STUB_EOF
chmod +x "$STUB"
STUB_SHA256="$(sha256_file "$STUB")" || {
    echo "No SHA-256 implementation is available for the installer self-test" >&2
    exit 1
}
TEST_REF="0123456789abcdef0123456789abcdef01234567"

# A stub curl that serves the stub installer, validates the exact immutable URL,
# and emits the HTTP status consumed by the wrapper.
FAKEBIN="$WORKDIR/bin"
mkdir -p "$FAKEBIN"
cat > "$FAKEBIN/curl" <<'CURL_EOF'
#!/usr/bin/env bash
set -euo pipefail
out=""
url=""
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --output) out="$2"; shift 2 ;;
        --proto|--write-out) shift 2 ;;
        --silent|--show-error) shift ;;
        *) url="$1"; shift ;;
    esac
done
if [[ -z "$out" ]]; then
    echo "fake curl expected --output <path>" >&2
    exit 2
fi
expected_url="https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/${PERL_LSP_INSTALLER_REF}/scripts/install.sh"
if [[ "$url" != "$expected_url" ]]; then
    echo "unexpected URL: $url" >&2
    exit 3
fi
cp "$PERL_LSP_TEST_STUB_INSTALLER" "$out"
printf '200'
CURL_EOF
chmod +x "$FAKEBIN/curl"

# A checkout-shaped tree whose sibling scripts/install.sh is the stub.
CHECKOUT="$WORKDIR/checkout"
mkdir -p "$CHECKOUT/scripts"
cp "$ROOT_INSTALLER" "$CHECKOUT/install.sh"
cp "$STUB" "$CHECKOUT/scripts/install.sh"

# 1. stdin invocation with zero arguments and an explicit installer identity.
test_identity_bound_pipe_zero_args() {
    local label="identity-bound curl-pipe zero-arg bootstrap survives set -u"
    local out="$WORKDIR/out-pipe.txt"
    local status=0

    set +e
    env \
        PATH="$FAKEBIN:$PATH" \
        PERL_LSP_TEST_STUB_INSTALLER="$STUB" \
        PERL_LSP_INSTALLER_REF="$TEST_REF" \
        PERL_LSP_INSTALLER_SHA256="$STUB_SHA256" \
        bash < "$ROOT_INSTALLER" >"$out" 2>&1
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $(cat "$out")"
        return
    fi

    if ! grep -q 'STUB argc=0' "$out"; then
        fail "$label" "expected the wrapper to forward zero arguments; output: $(cat "$out")"
        return
    fi

    pass "$label"
}

# 2. stdin invocation without identity must fail before curl or installer logic.
test_unbound_pipe_fails_closed() {
    local label="unbound curl-pipe bootstrap fails before network access"
    local out="$WORKDIR/out-unbound.txt"
    local curl_log="$WORKDIR/unbound-curl.log"
    local status=0

    rm -f "$curl_log"
    set +e
    env \
        PATH="$FAKEBIN:$PATH" \
        PERL_LSP_TEST_STUB_INSTALLER="$STUB" \
        PERL_LSP_TEST_CURL_LOG="$curl_log" \
        bash < "$ROOT_INSTALLER" >"$out" 2>&1
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
        fail "$label" "expected non-zero exit"
        return
    fi
    if ! grep -q 'requires PERL_LSP_INSTALLER_REF' "$out"; then
        fail "$label" "missing actionable identity error; output: $(cat "$out")"
        return
    fi
    pass "$label"
}

# 3. file invocation, zero arguments, local canonical installer present.
test_file_zero_args() {
    local label="file zero-arg invocation forwards no arguments"
    local out="$WORKDIR/out-file.txt"
    local status=0

    set +e
    bash "$CHECKOUT/install.sh" >"$out" 2>&1
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $(cat "$out")"
        return
    fi

    if ! grep -q 'STUB argc=0' "$out"; then
        fail "$label" "expected argc=0; output: $(cat "$out")"
        return
    fi

    pass "$label"
}

# 4. positional arguments still map to VERSION / INSTALL_DIR.
test_positional_args_still_work() {
    local label="positional version and install dir still become env vars"
    local out="$WORKDIR/out-args.txt"
    local status=0

    set +e
    bash "$CHECKOUT/install.sh" 0.17.0 /tmp/perl-lsp-selftest-bin >"$out" 2>&1
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $(cat "$out")"
        return
    fi

    if ! grep -q 'STUB VERSION=0.17.0 INSTALL_DIR=/tmp/perl-lsp-selftest-bin' "$out"; then
        fail "$label" "positional mapping regressed; output: $(cat "$out")"
        return
    fi

    if ! grep -q 'STUB argc=0' "$out"; then
        fail "$label" "consumed positionals must not be forwarded; output: $(cat "$out")"
        return
    fi

    pass "$label"
}

# 5. flag arguments are forwarded verbatim.
test_flag_args_forwarded() {
    local label="flag arguments are forwarded to the canonical installer"
    local out="$WORKDIR/out-flag.txt"
    local status=0

    set +e
    bash "$CHECKOUT/install.sh" --print-target >"$out" 2>&1
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $(cat "$out")"
        return
    fi

    if ! grep -q 'STUB argc=1' "$out"; then
        fail "$label" "expected argc=1; output: $(cat "$out")"
        return
    fi

    pass "$label"
}

# 6. Static guard for Bash 3.2: no bare "${arr[@]}" expansion.
test_no_unguarded_array_expansion() {
    local label="installers use \${arr[@]+\"\${arr[@]}\"} (bash 3.2 set -u safety)"
    local offenders=""

    local file
    for file in "$ROOT_INSTALLER" "$CANONICAL_INSTALLER"; do
        local hits
        hits="$(sed -e 's/[[:space:]]*#.*$//' \
                    -e 's/\${\([A-Za-z_][A-Za-z_0-9]*\)\[@\]+"\${\1\[@\]}"}/<GUARDED>/g' \
                    "$file" \
            | grep -n '"\${[A-Za-z_][A-Za-z_0-9]*\[@\]}"' || true)"
        if [[ -n "$hits" ]]; then
            offenders+="${file}:
${hits}
"
        fi
    done

    if [[ -n "$offenders" ]]; then
        fail "$label" "bare array expansions found (use \${arr[@]+\"\${arr[@]}\"}):
$offenders"
        return
    fi

    pass "$label"
}

# 7. Static guard: stdin invocation must not dereference an absent BASH_SOURCE.
test_bash_source_guarded() {
    local label="wrapper guards \${BASH_SOURCE[0]} for stdin invocation"

    local hits
    hits="$(sed 's/[[:space:]]*#.*$//' "$ROOT_INSTALLER" \
        | grep -n 'BASH_SOURCE\[0\]}' | grep -v 'BASH_SOURCE\[0\]:-' || true)"

    if [[ -n "$hits" ]]; then
        fail "$label" "unguarded BASH_SOURCE[0] expansion:
$hits"
        return
    fi

    pass "$label"
}

# 8. Real Bash 3.2 clone-local execution when Docker is available.
test_legacy_bash_container() {
    local label="zero-arg clone-local wrapper runs under bash 3.2 ($LEGACY_BASH_IMAGE)"

    if ! command -v docker >/dev/null 2>&1; then
        skip "$label (docker not installed)"
        return
    fi

    if ! docker info >/dev/null 2>&1; then
        skip "$label (docker daemon unavailable)"
        return
    fi

    if ! docker pull "$LEGACY_BASH_IMAGE" >/dev/null 2>&1; then
        skip "$label (could not pull $LEGACY_BASH_IMAGE)"
        return
    fi

    local out="$WORKDIR/out-legacy.txt"
    local status=0

    set +e
    docker run --rm -v "$CHECKOUT:/checkout:ro" -w /checkout \
        --entrypoint bash "$LEGACY_BASH_IMAGE" \
        -c 'echo "bash=$BASH_VERSION"; bash /checkout/install.sh' >"$out" 2>&1
    status=$?
    set -e

    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0 under legacy bash, got $status; output: $(cat "$out")"
        return
    fi

    if ! grep -q 'STUB argc=0' "$out"; then
        fail "$label" "expected argc=0 under legacy bash; output: $(cat "$out")"
        return
    fi

    printf '      %s\n' "$(grep '^bash=' "$out" || true)"
    pass "$label"
}

if [[ ! -f "$ROOT_INSTALLER" ]]; then
    fail "root installer exists" "missing $ROOT_INSTALLER"
elif [[ ! -f "$CANONICAL_INSTALLER" ]]; then
    fail "canonical installer exists" "missing $CANONICAL_INSTALLER"
else
    test_identity_bound_pipe_zero_args
    test_unbound_pipe_fails_closed
    test_file_zero_args
    test_positional_args_still_work
    test_flag_args_forwarded
    test_no_unguarded_array_expansion
    test_bash_source_guarded
    test_legacy_bash_container
fi

echo
echo "-- Summary --"
echo "Passed: $PASS"
echo "Failed: $FAIL"
echo "Skipped: $SKIP"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
