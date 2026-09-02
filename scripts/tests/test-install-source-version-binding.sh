#!/usr/bin/env bash
# Self-test for the source-mode version binding (#8367).
#
# scripts/install.sh accepts VERSION=vX.Y.Z, but its Cargo source path used to
# run a bare `cargo install perllsp --locked`, so a pinned request silently
# resolved a different (usually latest) crates.io subject. This test locks the
# repaired contract without touching the network or building the workspace:
#
#   * a fake `cargo` shim records its argv, so the test can prove the installer
#     passes `--version <requested>` for an exact request and no selector for
#     an explicit `latest` request;
#   * the shim stages a fake `perllsp` whose `--version` output is
#     attacker-controlled via FAKE_CARGO_RESOLVED, so the identity-mismatch
#     gate is proven against exactly the historical defect shape (cargo
#     ignoring the pin and resolving something else);
#   * failure modes are discriminated by message: exact-version absence, plain
#     build failure, and identity mismatch must be distinct reasons;
#   * VERSION values that could pose as cargo argv (`--target`) or that are
#     not full X.Y.Z semver must be rejected before cargo runs at all.
#
# The installer is sourced with PERL_LSP_INSTALLER_LIBRARY_ONLY=1 (its
# internal proof seam) and `build_from_source` is invoked in a subshell, so a
# refusing installer cannot kill the harness.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CANONICAL_INSTALLER="$ROOT/scripts/install.sh"

PASS=0
FAIL=0

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

HARNESS_TMP="$(mktemp -d)"
FAKEBIN="$HARNESS_TMP/bin"
FAKE_CARGO_LOG="$HARNESS_TMP/cargo-argv.log"
FAKE_CARGO_INSTALL_LOG="$HARNESS_TMP/cargo-install-argv.log"
mkdir -p "$FAKEBIN"

cat > "$FAKEBIN/cargo" <<'SHIM'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\0' "$@" >> "${FAKE_CARGO_LOG:?}"

if [[ "${1:-}" == "--version" ]]; then
    echo "cargo 1.95.0 (fake-shim)"
    exit 0
fi

# Only install invocations land in the install log, so the toolchain guard's
# bare `cargo --version` probe cannot be mistaken for a version selector.
if [[ "${1:-}" == "install" ]]; then
    printf '%s\0' "$@" >> "${FAKE_CARGO_INSTALL_LOG:?}"
fi

wanted=""
root=""
prev=""
for a in "$@"; do
    case "$prev" in
        --version) wanted="$a" ;;
        --root) root="$a" ;;
    esac
    prev="$a"
done

mode="${FAKE_CARGO_MODE:-ok}"
if [[ "$mode" == "fail-version" && -n "$wanted" ]]; then
    echo "error: failed to select a version for \`perllsp $wanted\`; no matching version found" >&2
    exit 101
fi
if [[ "$mode" == "fail-build" ]]; then
    echo "error: could not compile \`perllsp\` (simulated toolchain failure)" >&2
    exit 102
fi

: "${root:?shim expected --root}"
: "${FAKE_CARGO_RESOLVED:?shim requires the resolved version to stage}"
mkdir -p "$root/bin"
cat > "$root/bin/perllsp" <<BIN
#!/usr/bin/env bash
if [[ "\${1:-}" == "--version" ]]; then
    echo "${FAKE_CARGO_IDENTITY_PREFIX:-perllsp} ${FAKE_CARGO_RESOLVED}"
    exit 0
fi
exit 0
BIN
chmod +x "$root/bin/perllsp"
exit 0
SHIM
chmod +x "$FAKEBIN/cargo"

cat > "$FAKEBIN/rustc" <<'SHIM'
#!/usr/bin/env bash
if [[ "${1:-}" == "--version" ]]; then
    echo "rustc 1.95.0 (fake-shim)"
    exit 0
fi
exit 0
SHIM
chmod +x "$FAKEBIN/rustc"

# Run build_from_source in a subshell with the shim PATH. stdout+stderr and
# the resulting status are captured by the caller; cargo's argv lands in
# FAKE_CARGO_LOG.
run_source_build() {
    local requested_version="$1"   # "latest" or vX.Y.Z / X.Y.Z
    local resolved_version="$2"    # what the shim-staged binary reports
    local mode="${3:-ok}"          # ok | fail-version | fail-build
    local identity_prefix="${4:-perllsp}"

    : > "$FAKE_CARGO_LOG"
    : > "$FAKE_CARGO_INSTALL_LOG"

    (
        export PATH="$FAKEBIN:$PATH"
        export FAKE_CARGO_LOG
        export FAKE_CARGO_INSTALL_LOG
        export FAKE_CARGO_RESOLVED="$resolved_version"
        export FAKE_CARGO_IDENTITY_PREFIX="$identity_prefix"
        export FAKE_CARGO_MODE="$mode"
        export PERL_LSP_INSTALLER_LIBRARY_ONLY=1
        unset TARGET
        VERSION="$requested_version"
        case "$VERSION" in
            v*) VERSION_NUM="${VERSION#v}" ;;
            latest) VERSION_NUM="" ;;
            *) VERSION_NUM="$VERSION" ;;
        esac
        TMPDIR="$HARNESS_TMP/stage"
        rm -rf "$TMPDIR"
        mkdir -p "$TMPDIR"
        # The harness function's own arguments must not leak into the
        # installer's CLI argument parser.
        set --
        # shellcheck disable=SC1090
        . "$CANONICAL_INSTALLER"
        build_from_source
    )
}

# Whole-argument membership check over the NUL-separated install argv log.
# The install log excludes the toolchain guard's bare `cargo --version` probe,
# so a selector check can never be satisfied by the guard call.
cargo_argv_contains() {
    tr -d '\n' < "$FAKE_CARGO_INSTALL_LOG" | grep -qz -- "$1"
}

cargo_argv_has_exact_pair() {
    python3 - "$FAKE_CARGO_INSTALL_LOG" "$1" "$2" <<'PY'
import sys

with open(sys.argv[1], "rb") as handle:
    argv = handle.read().split(b"\0")[:-1]
flag, value = sys.argv[2].encode(), sys.argv[3].encode()
if any(argv[index:index + 2] == [flag, value] for index in range(len(argv) - 1)):
    raise SystemExit(0)
raise SystemExit(1)
PY
}

expect_success() {
    local label="$1"
    shift
    local output status
    set +e
    output="$(run_source_build "$@" 2>&1)"
    status=$?
    set -e
    if [[ "$status" -ne 0 ]]; then
        fail "$label" "expected exit 0, got $status; output: $output"
    else
        pass "$label"
    fi
}

expect_failure_with() {
    local label="$1" expected_fragment="$2"
    shift 2
    local output status
    set +e
    output="$(run_source_build "$@" 2>&1)"
    status=$?
    set -e
    if [[ "$status" -eq 0 ]]; then
        fail "$label" "expected non-zero exit, got 0"
        return
    fi
    if [[ "$output" != *"$expected_fragment"* ]]; then
        fail "$label" "expected output to contain '$expected_fragment'; got: $output"
        return
    fi
    pass "$label"
}

# ---------------------------------------------------------------------------
# The core #8367 contract
# ---------------------------------------------------------------------------

expect_success "exact VERSION=v0.12.0 pins the cargo subject" "v0.12.0" "0.12.0" ok
if cargo_argv_has_exact_pair --version 0.12.0; then
    pass "exact request passes --version 0.12.0 to cargo"
else
    fail "exact request passes --version 0.12.0 to cargo" \
        "cargo argv log did not contain the requested version: $(tr '\0' ' ' < "$FAKE_CARGO_LOG")"
fi

if python3 - "$FAKE_CARGO_INSTALL_LOG" <<'PY'
import sys

with open(sys.argv[1], "rb") as handle:
    argv = handle.read().split(b"\0")[:-1]
if argv[:3] != [b"install", b"perllsp", b"--locked"]:
    raise SystemExit(f"unexpected cargo install prefix: {argv!r}")
PY
then
    pass "cargo source install keeps the locked selector in exact argv order"
else
    fail "cargo source install keeps the locked selector in exact argv order" \
        "cargo install argv did not begin with install perllsp --locked"
fi

# The historical defect: cargo resolves a DIFFERENT version than requested
# (the pin is ignored). The staged binary reports 9.9.9, so the identity gate
# must refuse promotion instead of silently installing another subject.
expect_failure_with \
    "identity mismatch refuses promotion of a different registry subject" \
    "identity mismatch" \
    "v0.12.0" "9.9.9" ok

expect_failure_with \
    "an unrelated binary identity cannot satisfy an exact version request" \
    "no parseable perllsp version token" \
    "v0.12.0" "0.12.0" ok "other-tool"

# Exact version absent from the registry: a distinct failure reason naming the
# requested subject, not a generic build failure.
expect_failure_with \
    "registry absence of the exact version is a distinct typed reason" \
    "perllsp 0.9.9" \
    "v0.9.9" "0.9.9" fail-version

# Plain build failure without a pin: the generic source-build reason.
expect_failure_with \
    "build failure keeps its own generic source-build reason" \
    "failed to build/install perllsp from source" \
    "latest" "0.99.0" fail-build

# Explicit latest: no version selector may reach cargo, and the resolved
# subject identity is surfaced.
expect_success "explicit latest resolves without a pin" "latest" "0.99.0" ok
if cargo_argv_contains --version; then
    fail "latest request passes no --version selector" \
        "cargo argv contained --version: $(tr '\0' ' ' < "$FAKE_CARGO_LOG")"
else
    pass "latest request passes no --version selector"
fi
latest_output="$(run_source_build latest 0.99.0 ok 2>&1)"
if [[ "$latest_output" == *"resolved registry subject: perllsp 0.99.0"* ]]; then
    pass "latest request surfaces the resolved registry subject"
else
    fail "latest request surfaces the resolved registry subject" \
        "resolved identity was not surfaced: $latest_output"
fi

# ---------------------------------------------------------------------------
# Argv-safety and semver-shape rejection before cargo runs
# ---------------------------------------------------------------------------

expect_failure_with \
    "a VERSION that poses as a cargo flag is rejected before cargo runs" \
    "invalid VERSION" \
    "--target" "--target" ok
if [[ -s "$FAKE_CARGO_INSTALL_LOG" ]]; then
    fail "a VERSION that poses as a cargo flag never reaches cargo" \
        "cargo install shim was invoked: $(tr '\0' ' ' < "$FAKE_CARGO_INSTALL_LOG")"
else
    pass "a VERSION that poses as a cargo flag never reaches cargo"
fi

expect_failure_with \
    "a two-component VERSION is rejected with a typed semver reason" \
    "expected a full X.Y.Z semver" \
    "v0.12" "0.12" ok
if [[ -s "$FAKE_CARGO_INSTALL_LOG" ]]; then
    fail "a malformed VERSION never reaches cargo" "cargo install shim was invoked"
else
    pass "a malformed VERSION never reaches cargo"
fi

expect_failure_with \
    "a leading-zero VERSION is rejected with a typed semver reason" \
    "expected a full X.Y.Z semver" \
    "v01.2.3" "1.2.3" ok
if [[ -s "$FAKE_CARGO_INSTALL_LOG" ]]; then
    fail "a leading-zero VERSION never reaches cargo" "cargo install shim was invoked"
else
    pass "a leading-zero VERSION never reaches cargo"
fi

expect_failure_with \
    "a leading-zero prerelease identifier is rejected" \
    "expected a full X.Y.Z semver" \
    "v0.12.0-01" "0.12.0-01" ok

expect_success \
    "a combined prerelease and build suffix is accepted" \
    "v0.12.0-alpha+build.7" "0.12.0-alpha+build.7" ok

expect_failure_with \
    "an underscore VERSION is rejected with a typed semver reason" \
    "expected a full X.Y.Z semver" \
    "v0.12.3_bad" "0.12.3_bad" ok
if [[ -s "$FAKE_CARGO_INSTALL_LOG" ]]; then
    fail "an underscore VERSION never reaches cargo" "cargo install shim was invoked"
else
    pass "an underscore VERSION never reaches cargo"
fi

# ---------------------------------------------------------------------------
# Negative control: the archive path stays the only release-mode path, and the
# source build remains behind the non-release branch of INSTALL_MODE.
# ---------------------------------------------------------------------------
release_line="$(grep -n 'INSTALL_MODE" = "release"' "$CANONICAL_INSTALLER" | head -n1 | cut -d: -f1 || true)"
source_line="$(grep -n '^[[:space:]]*build_from_source$' "$CANONICAL_INSTALLER" | tail -n1 | cut -d: -f1 || true)"
if [[ -n "$release_line" && -n "$source_line" && "$source_line" -gt "$release_line" ]]; then
    pass "source build stays behind the non-release branch of INSTALL_MODE"
else
    fail "source build stays behind the non-release branch of INSTALL_MODE" \
        "build_from_source call must follow the release-mode branch (release=$release_line source=$source_line)"
fi

echo
echo "-- Summary --"
echo "Passed: $PASS"
echo "Failed: $FAIL"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
