#!/usr/bin/env bash
# Self-test for installer argument-surface agreement and loud failure paths.
#
# Locks the acceptance edges from #16310 that live in the POSIX installer:
# the canonical installer accepts the same positional VERSION/INSTALL_DIR
# surface as the root wrapper, --help documents it, a release run without
# gzip is precondition-checked by name, and a binary that fails --version
# fails the install instead of printing success.
#
# Limitations: the gzip precondition is asserted structurally (the release
# branch is unreachable without network), and the verify_install failure path
# is exercised by extracting the real function into a stub harness.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ROOT_INSTALLER="$ROOT/install.sh"
CANONICAL_INSTALLER="$ROOT/scripts/install.sh"

PASS=0
FAIL=0
TMP_DIR=""

cleanup() {
    if [[ -n "${TMP_DIR:-}" && -d "$TMP_DIR" ]]; then
        rm -rf "$TMP_DIR"
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
    printf '      %s\n' "$2"
}

# Runs a command expecting a specific exit status; on match runs the given
# grep pattern against the combined output when one is supplied.
expect_status() {
    local label="$1" expected="$2" pattern="${3:-}"
    shift 3
    local output status
    set +e
    output="$("$@" 2>&1)"
    status=$?
    set -e
    if [[ "$status" -ne "$expected" ]]; then
        fail "$label" "expected exit $expected, got $status; output: $output"
        return 1
    fi
    if [[ -n "$pattern" ]] && ! grep -q "$pattern" <<<"$output"; then
        fail "$label" "output missing '$pattern'; output: $output"
        return 1
    fi
    pass "$label"
}

TMP_DIR="$(mktemp -d)"

# The canonical installer refuses unsupported hosts (e.g. Windows) before its
# target selection, so end-to-end accept probes skip here and are covered by
# the just recipe on supported hosts. Rejections handled by the argument loop
# and the structural checks below stay unconditional.
CANONICAL_SUPPORTS_HOST=1
if ! bash "$CANONICAL_INSTALLER" --print-target >/dev/null 2>&1; then
    CANONICAL_SUPPORTS_HOST=0
    skip "end-to-end positional probes" "host unsupported by scripts/install.sh"
fi

# 1. Canonical installer accepts positional VERSION and INSTALL_DIR, matching
#    the root wrapper (#16310 item 4). --print-target exits before any
#    download, so this needs no network.
if [[ "$CANONICAL_SUPPORTS_HOST" -eq 1 ]]; then
    expect_status "canonical accepts positional VERSION" 0 \
        bash "$CANONICAL_INSTALLER" v0.12.0 --print-target
    expect_status "canonical accepts positional VERSION and INSTALL_DIR" 0 \
        env HOME="$TMP_DIR" bash "$CANONICAL_INSTALLER" v0.12.0 "$TMP_DIR/bin" --print-target
fi

# A third positional still fails closed with a named reason.
expect_status "unexpected third positional rejected" 1 "unexpected argument" \
    bash "$CANONICAL_INSTALLER" a b c --print-target

# Unknown flags keep their named rejection.
expect_status "unknown flag rejected" 1 "unknown argument" \
    bash "$CANONICAL_INSTALLER" --bogus-flag

# 2. --help documents the positional surface.
help_text="$(bash "$CANONICAL_INSTALLER" --help 2>&1)"
if grep -q "\[VERSION\] \[INSTALL_DIR\]" <<<"$help_text"; then
    pass "--help documents positional arguments"
else
    fail "--help documents positional arguments" "usage line missing [VERSION] [INSTALL_DIR]"
fi

# 3. The root wrapper keeps accepting positionals (regression guard).
if [[ "$CANONICAL_SUPPORTS_HOST" -eq 1 ]]; then
    expect_status "wrapper accepts positional VERSION" 0 \
        bash "$ROOT_INSTALLER" v0.12.0 --print-target
fi

# 4. Release branch preconditions name gzip (#16310 item 3). Structural: the
#    release path cannot be driven offline.
if grep -q "^        need_cmd gzip$" "$CANONICAL_INSTALLER" &&
    grep -B1 "^        need_cmd gzip$" "$CANONICAL_INSTALLER" | grep -q "need_cmd od"; then
    pass "release branch preconditions gzip next to od"
else
    fail "release branch preconditions gzip next to od" \
        "scripts/install.sh must call need_cmd gzip right after need_cmd od in the release branch"
fi

# 5. verify_install fails loudly when the installed binary cannot run
#    (#16310 item 2): extract the real function and drive it with a stub
#    binary that exits non-zero.
verify_fn="$(sed -n '/^verify_install() {/,/^}/p' "$CANONICAL_INSTALLER")"
if [[ -z "$verify_fn" ]]; then
    fail "verify_install extracted" "could not extract verify_install from $CANONICAL_INSTALLER"
else
    printf '#!/usr/bin/env sh\nexit 3\n' >"$TMP_DIR/perllsp"
    chmod +x "$TMP_DIR/perllsp"
    cat >"$TMP_DIR/harness.sh" <<HARNESS
INSTALL_DIR="$TMP_DIR"
BIN_NAME="perllsp"
info() { printf '%s\n' "\$1"; }
err() { printf 'error: %s\n' "\$1" >&2; exit 1; }
$verify_fn
verify_install
HARNESS
    expect_status "failing --version aborts install" 1 "post-install verification failed" \
        bash "$TMP_DIR/harness.sh"
fi

# 6. The release-download URLs in the release process doc agree with the
#    packaged asset prefix (#16310 item 1).
doc="$ROOT/docs/RELEASE_PROCESS.md"
stale="$(grep -c 'download/v{VERSION}/perl-lsp-' "$doc" || true)"
fixed="$(grep -c 'download/v{VERSION}/perllsp-' "$doc" || true)"
if [[ "$stale" -eq 0 && "$fixed" -ge 7 ]]; then
    pass "release download URLs use the packaged perllsp- prefix"
else
    fail "release download URLs use the packaged perllsp- prefix" \
        "stale perl-lsp- URLs: $stale, perllsp- URLs: $fixed (expected 0 stale, >=7 fixed)"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[[ "$FAIL" -eq 0 ]]
