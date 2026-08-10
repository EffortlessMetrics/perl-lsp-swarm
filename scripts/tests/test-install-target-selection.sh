#!/usr/bin/env bash
# Self-test for Linux installer target selection.
#
# The installer is intentionally shell because it is the curl-pipe/bootstrap
# entrypoint. This test locks the target-selection contract without downloading
# release artifacts by using --print-target.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ROOT_INSTALLER="$ROOT/install.sh"
CANONICAL_INSTALLER="$ROOT/scripts/install.sh"

PASS=0
FAIL=0
SKIP=0
TMPDIR_BASE=""

cleanup() {
    if [[ -n "${TMPDIR_BASE:-}" && -d "$TMPDIR_BASE" ]]; then
        rm -rf "$TMPDIR_BASE"
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

assert_bad_override_fails() {
    local output status
    set +e
    output="$(env PERL_LSP_LINUX_LIBC=bad bash "$CANONICAL_INSTALLER" --print-target 2>&1)"
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
        fail "bad PERL_LSP_LINUX_LIBC override fails" "expected non-zero exit, got 0"
        return
    fi

    if [[ "$output" != *"invalid PERL_LSP_LINUX_LIBC=bad; expected auto, gnu, glibc, or musl"* ]]; then
        fail "bad PERL_LSP_LINUX_LIBC override explains valid values" "unexpected output: $output"
        return
    fi

    pass "bad PERL_LSP_LINUX_LIBC override fails clearly"
}

# Windows target contract (#5007).
#
# The release matrix is the only authority for which target triples exist as
# published assets. An install surface naming a Windows target outside it
# builds a download URL that always 404s — which is exactly how ARM64 Windows
# users were routed to an asset that is never produced, and told only "Failed
# to download". PowerShell cannot be executed on the Linux CI host, so this
# asserts the contract statically against the release matrix rather than
# hardcoding the expected triple, which would go stale the day ARM64 ships.
#
# Comment lines are stripped so the surfaces can still name the unbuilt target
# when explaining why they do not request it.
strip_comments() {
    sed -E 's://.*$::; s:^[[:space:]]*\*.*$::; s:^[[:space:]]*#.*$::' "$1"
}

built_windows_targets() {
    grep -Eo '(x86_64|aarch64|arm64|i686|armv7)-pc-windows-[a-z]+' \
        "$ROOT/.github/workflows/release.yml" | sort -u
}

assert_only_built_windows_targets() {
    local label="$1" file="$2"

    if [[ ! -f "$file" ]]; then
        fail "$label" "missing $file"
        return
    fi

    local built referenced offenders
    built="$(built_windows_targets)"

    if [[ -z "$built" ]]; then
        fail "$label" "release.yml names no Windows target; cannot derive the contract"
        return
    fi

    referenced="$(strip_comments "$file" \
        | grep -Eo '(x86_64|aarch64|arm64|i686|armv7)-pc-windows-[a-z]+' | sort -u || true)"

    # An empty result must fail, not pass. The original defect assigned only the
    # arch ($Arch = "aarch64") and appended the suffix separately, so no whole
    # triple appeared in the source and a membership-only check saw nothing to
    # object to — it would have certified the very bug it exists to catch. Each
    # surface must name at least one target literally so there is something to
    # check.
    if [[ -z "$referenced" ]]; then
        fail "$label" "names no Windows target literally; the contract cannot be checked (assemble no triple from a variable)"
        return
    fi

    offenders="$(comm -23 <(printf '%s\n' "$referenced") <(printf '%s\n' "$built"))"

    if [[ -n "$offenders" ]]; then
        fail "$label" "requests Windows target(s) the release matrix never builds: $(printf '%s' "$offenders" | tr '\n' ' ')"
        return
    fi

    pass "$label"
}

reachable_windows_targets() {
    local file

    for file in "$@"; do
        case "$file" in
            *.ps1)
                strip_comments "$file" \
                    | grep -E '^[[:space:]]*\$(NativeTarget|Target)[[:space:]]*=' \
                    | grep -Eo '(x86_64|aarch64|arm64|i686|armv7)-pc-windows-[a-z]+' || true
                ;;
            *.ts)
                strip_comments "$file" \
                    | grep -E '^[[:space:]]*(export[[:space:]]+)?const[[:space:]]+[A-Z0-9_]*TARGET[[:space:]]*=' \
                    | grep -Eo '(x86_64|aarch64|arm64|i686|armv7)-pc-windows-[a-z]+' || true
                ;;
            *)
                fail "Windows target reachability" "unsupported install surface type: $file"
                return
                ;;
        esac
    done
}

# The other half of the contract (#6196).
#
# assert_only_built_windows_targets checks containment: surfaces must not
# request a target the matrix does not build. That direction alone cannot see
# a surface that IGNORES a target the matrix does build, because requesting
# fewer targets than are built satisfies a subset check trivially.
#
# That is not hypothetical. aarch64-pc-windows-msvc was added to the matrix on
# 2026-08-03 (#5208). Both install surfaces went on mapping ARM64 Windows to
# the x64 build for five days, telling users no native ARM64 build was
# published and refusing Windows 10 ARM64 outright — and every gate stayed
# green the whole time, because ignoring a built target is invisible to
# containment. Together the two directions make the mapping a bijection per
# install surface: each surface must name every built Windows target, and no
# surface may request one that is not built. Merging targets across surfaces
# would let one broken path hide behind the other.
#
# Scoped to Windows because that is where the matrix and the surfaces are both
# enumerable statically; the POSIX targets are selected by uname at runtime.
assert_every_built_windows_target_is_reachable() {
    local label="$1"
    shift

    local built file reachable missing surface_label
    built="$(built_windows_targets)"

    if [[ -z "$built" ]]; then
        fail "$label" "release.yml names no Windows target; cannot derive the contract"
        return
    fi

    for file in "$@"; do
        if [[ ! -f "$file" ]]; then
            fail "$label" "missing $file"
            return
        fi

        surface_label="${label} ($(basename "$file"))"

        # Count only target-bearing assignments/constants. A target string in a
        # diagnostic or explanatory comment is not a requestable release asset and
        # must not make the reverse-direction contract pass.
        reachable="$(reachable_windows_targets "$file" | sort -u || true)"
        reachable="$(printf '%s' "$reachable" | grep -E '\S' | sort -u || true)"

        if [[ -z "$reachable" ]]; then
            fail "$surface_label" "names no Windows target literally; the contract cannot be checked"
            return
        fi

        missing="$(comm -23 <(printf '%s\n' "$built") <(printf '%s\n' "$reachable"))"

        if [[ -n "$missing" ]]; then
            fail "$surface_label" "the release matrix builds Windows target(s) this surface cannot request: $(printf '%s' "$missing" | tr '\n' ' ') -- wire them into this surface or stop building them"
            return
        fi

        pass "$surface_label"
    done
}

host_arch() {
    case "$(uname -m)" in
        x86_64|amd64|x64) printf '%s\n' "x86_64" ;;
        aarch64|arm64) printf '%s\n' "aarch64" ;;
        *) return 1 ;;
    esac
}

host_auto_libc() {
    if command -v ldd >/dev/null 2>&1; then
        local ldd_output
        ldd_output="$(ldd --version 2>&1 || true)"
        if printf '%s\n' "$ldd_output" | grep -qi musl; then
            printf '%s\n' "musl"
            return
        fi
        if printf '%s\n' "$ldd_output" | grep -Eqi 'glibc|gnu libc'; then
            printf '%s\n' "gnu"
            return
        fi
    fi

    if command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
        printf '%s\n' "gnu"
        return
    fi

    if [[ -f /etc/alpine-release ]]; then
        printf '%s\n' "musl"
        return
    fi

    printf '%s\n' "gnu"
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

test_root_wrapper_fetch_fallback() {
    local expected="$1"
    TMPDIR_BASE="$(mktemp -d)"
    local fakebin="$TMPDIR_BASE/bin"
    local isolated="$TMPDIR_BASE/isolated"
    local digest ref
    mkdir -p "$fakebin" "$isolated"

    digest="$(sha256_file "$CANONICAL_INSTALLER")" || {
        fail "root install.sh identity-bound fallback" "no SHA-256 implementation available"
        return
    }
    ref="0123456789abcdef0123456789abcdef01234567"

    cp "$ROOT_INSTALLER" "$isolated/install.sh"
    cat > "$fakebin/curl" <<'FAKE_CURL'
#!/usr/bin/env bash
set -euo pipefail

out=""
url=""
while [[ "$#" -gt 0 ]]; do
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
        *)
            url="$1"
            shift
            ;;
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

cp "$PERL_LSP_TEST_CANONICAL_INSTALLER" "$out"
printf '200'
FAKE_CURL
    chmod +x "$fakebin/curl"

    assert_stdout \
        "root install.sh identity-bound fallback fetches canonical installer outside checkout" \
        "$expected" \
        env \
        PATH="$fakebin:$PATH" \
        PERL_LSP_TEST_CANONICAL_INSTALLER="$CANONICAL_INSTALLER" \
        PERL_LSP_INSTALLER_REF="$ref" \
        PERL_LSP_INSTALLER_SHA256="$digest" \
        bash "$isolated/install.sh" --print-target
}

test_alpine_docker() {
    local arch="$1"

    if ! command -v docker >/dev/null 2>&1; then
        skip "Alpine Docker target selection (docker not installed)"
        return
    fi

    if ! docker info >/dev/null 2>&1; then
        skip "Alpine Docker target selection (docker daemon unavailable)"
        return
    fi

    if ! docker pull alpine:latest >/dev/null 2>&1; then
        fail "Alpine Docker target selection" "failed to pull alpine:latest"
        return
    fi

    assert_stdout \
        "Alpine Docker auto-selects musl" \
        "${arch}-unknown-linux-musl" \
        docker run --rm -v "$ROOT:/repo:ro" -w /repo alpine:latest \
            sh -lc 'apk add --no-cache bash >/dev/null && bash scripts/install.sh --print-target'
}

if [[ ! -f "$ROOT_INSTALLER" ]]; then
    fail "root installer exists" "missing $ROOT_INSTALLER"
fi

if [[ ! -f "$CANONICAL_INSTALLER" ]]; then
    fail "canonical installer exists" "missing $CANONICAL_INSTALLER"
fi

assert_only_built_windows_targets \
    "install.ps1 requests only built Windows targets" "$ROOT/install.ps1"
assert_only_built_windows_targets \
    "extension downloader requests only built Windows targets" \
    "$ROOT/vscode-extension/src/downloader.ts"

assert_every_built_windows_target_is_reachable \
    "every built Windows target is reachable from an install surface" \
    "$ROOT/install.ps1" \
    "$ROOT/vscode-extension/src/downloader.ts"

assert_windows_arm64_native_preference() {
    local label="$1" file="$2"

    if [[ ! -f "$file" ]]; then
        fail "$label" "missing $file"
        return
    fi

    # Every capture below is `|| true`-guarded. Under `set -euo pipefail` an
    # unmatched grep inside a command substitution aborts the whole script, so
    # the previous version of this assertion killed the run with no diagnostic
    # at all the moment the installer stopped matching it -- a gate that fails
    # closed but silently, which is nearly as unhelpful as failing open.
    local native_line native_assignment_line probe_line absent_branch_line unknown_probe_error_line floor_line error_line fallback_line download_line

    # The native target must be named as a whole literal, for the same reason
    # the triples are: this file cannot be executed on the Linux CI host, so
    # the contract is checked by reading the source.
    native_line="$(grep -nE '^[[:space:]]*\$NativeTarget[[:space:]]*=[[:space:]]*"aarch64-pc-windows-msvc"' "$file" | head -n1 | cut -d: -f1 || true)"
    probe_line="$(grep -nE '^[[:space:]]*\$AssetProbe[[:space:]]*=[[:space:]]*Test-ReleaseAsset ' "$file" | head -n1 | cut -d: -f1 || true)"
    native_assignment_line="$(grep -nE '^[[:space:]]*\$Target[[:space:]]*=[[:space:]]*\$NativeTarget' "$file" | head -n1 | cut -d: -f1 || true)"
    floor_line="$(grep -nE '^[[:space:]]*if \(\$WindowsBuild -lt 22000\) \{' "$file" | head -n1 | cut -d: -f1 || true)"
    error_line="$(grep -nE 'Write-Error "[^"]*emulation requires Windows 11' "$file" | head -n1 | cut -d: -f1 || true)"
    absent_branch_line="$(grep -nE '\$AssetProbe\.State[[:space:]]+-eq[[:space:]]+"absent"' "$file" | head -n1 | cut -d: -f1 || true)"
    unknown_probe_error_line="$(grep -nE 'asset probe failed' "$file" | head -n1 | cut -d: -f1 || true)"
    fallback_line="$(grep -nE '^[[:space:]]*\$Target[[:space:]]*=[[:space:]]*"x86_64-pc-windows-msvc"' "$file" | head -n1 | cut -d: -f1 || true)"
    download_line="$(grep -nE '^[[:space:]]*Invoke-WebRequest -Uri \$Url' "$file" | head -n1 | cut -d: -f1 || true)"

    if [[ -z "$native_line" ]]; then
        fail "$label" "does not name aarch64-pc-windows-msvc as a preferred target literal"
        return
    fi

    if [[ -z "$probe_line" ]]; then
        fail "$label" "does not probe whether the release carries the native asset; preferring it unconditionally would 404 on every release predating that target"
        return
    fi

    if [[ -z "$native_assignment_line" ]]; then
        fail "$label" "does not assign the native target after probing it"
        return
    fi

    if [[ -z "$absent_branch_line" ]]; then
        fail "$label" "does not branch on a definitive \"absent\" probe result; an unknown probe failure would be treated as proven absence and would push Windows 10 ARM64 onto an unusable x64 build"
        return
    fi

    if [[ -z "$unknown_probe_error_line" ]]; then
        fail "$label" "does not keep a separate unknown-probe failure path; transport failures must not be reported as proven asset absence on Windows 10 ARM64"
        return
    fi

    if [[ -z "$floor_line" || -z "$error_line" || -z "$fallback_line" || -z "$download_line" ]]; then
        fail "$label" "must keep an executable Windows 11 build floor, an emulation-specific error, an x64 fallback assignment, and a download call"
        return
    fi

    # The whole point of #6196: the build-22000 floor is a property of x64
    # emulation, not of ARM64. It must sit *after* the native-asset probe, so a
    # release carrying the native build installs on Windows 10 ARM64 instead of
    # being refused.
    if (( probe_line >= floor_line )); then
        fail "$label" "the Windows 11 build floor must come after the native-asset probe, or it will refuse Windows 10 ARM64 installs that a native build would satisfy"
        return
    fi

    if (( native_line >= probe_line )); then
        fail "$label" "the native target must be named before it is probed"
        return
    fi

    if (( probe_line >= native_assignment_line )); then
        fail "$label" "the native target must be assigned only after the asset probe succeeds"
        return
    fi

    if (( floor_line >= download_line || fallback_line >= download_line )); then
        fail "$label" "target selection must complete before the download"
        return
    fi

    pass "$label"
}

assert_windows_arm64_native_preference \
    "PowerShell installer prefers the native ARM64 build and gates emulation only" \
    "$ROOT/install.ps1"

if [[ "$(uname -s)" != "Linux" ]]; then
    skip "Linux target-selection checks (host is $(uname -s))"
else
    if arch="$(host_arch)"; then
        auto_libc="$(host_auto_libc)"
        auto_target="${arch}-unknown-linux-${auto_libc}"
        gnu_target="${arch}-unknown-linux-gnu"
        musl_target="${arch}-unknown-linux-musl"

        assert_stdout "root install.sh --print-target follows canonical installer" \
            "$auto_target" bash "$ROOT_INSTALLER" --print-target
        assert_stdout "scripts/install.sh auto target" \
            "$auto_target" bash "$CANONICAL_INSTALLER" --print-target
        assert_stdout "PERL_LSP_LINUX_LIBC=gnu target" \
            "$gnu_target" env PERL_LSP_LINUX_LIBC=gnu bash "$CANONICAL_INSTALLER" --print-target
        assert_stdout "PERL_LSP_LINUX_LIBC=glibc target" \
            "$gnu_target" env PERL_LSP_LINUX_LIBC=glibc bash "$CANONICAL_INSTALLER" --print-target
        assert_stdout "PERL_LSP_LINUX_LIBC=musl target" \
            "$musl_target" env PERL_LSP_LINUX_LIBC=musl bash "$CANONICAL_INSTALLER" --print-target
        assert_bad_override_fails
        test_root_wrapper_fetch_fallback "$auto_target"
        test_alpine_docker "$arch"
    else
        skip "Linux target-selection checks (unsupported host arch: $(uname -m))"
    fi
fi

echo
echo "-- Summary --"
echo "Passed: $PASS"
echo "Failed: $FAIL"
echo "Skipped: $SKIP"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
