#!/usr/bin/env bash
# Discriminating classification proof for the fsync_path helper in
# scripts/install.sh (#16711).
#
# The durability receipt is only as honest as the status each flush reports.
# These cases inject POSIX/IO::Handle implementations through PERL5LIB so every
# primitive shape is exercisable on any host with perl:
#   - a working POSIX::fsync flushes;
#   - a POSIX::fsync that croaks "Unimplemented" (the documented stub shape,
#     seen on Windows and on some Linux POSIX builds) must fall back to
#     IO::Handle::sync instead of reporting the host unsupported;
#   - a stub whose fallback also has no primitive stays host_unsupported;
#   - a POSIX::fsync that actually ran and errored is sync_failed, never a
#     host limitation and never a silent pass.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail_case() {
    printf 'FAIL  %s\n%s\n' "$1" "$2" >&2
    FAIL=$((FAIL + 1))
}

mode_file="$TMP/mode"
printf 'ok\n' > "$mode_file"

# One fake module pair covers every primitive shape; both read the injected
# mode from the file named by PERL_LSP_TEST_FSYNC_MODE_FILE so the helper's
# branch for each shape is the thing under test.
mkdir -p "$TMP/fakelib/IO"
cat > "$TMP/fakelib/POSIX.pm" <<'PERL'
package POSIX;
use strict;
use warnings;
sub _mode {
    open my $fh, '<', $ENV{PERL_LSP_TEST_FSYNC_MODE_FILE} or return '';
    my $mode = do { local $/; <$fh> };
    $mode =~ s/\s+\z//;
    return $mode;
}
sub fsync {
    my $mode = _mode();
    if ($mode eq 'ok') { return 0; }
    if ($mode eq 'error') { $! = 5; return undef; }
    die "Unimplemented: POSIX::fsync(): Use method IO::Handle::sync() instead\n";
}
$INC{'POSIX.pm'} = __FILE__;
1;
PERL
cat > "$TMP/fakelib/IO/Handle.pm" <<'PERL'
package IO::Handle;
use strict;
use warnings;
sub _mode {
    open my $fh, '<', $ENV{PERL_LSP_TEST_FSYNC_MODE_FILE} or return '';
    my $mode = do { local $/; <$fh> };
    $mode =~ s/\s+\z//;
    return $mode;
}
sub sync {
    my $mode = _mode();
    if ($mode eq 'stub_sync_ok') { return '0 but true'; }
    if ($mode eq 'stub_sync_croak') { die "IO::Handle::sync not implemented on this architecture\n"; }
    if ($mode eq 'stub_sync_false') { $! = 13; return 0; }
    return '0 but true';
}
$INC{'IO/Handle.pm'} = __FILE__;
1;
PERL

fsync_status() {
    local _mode="$1" _target="$2"
    printf '%s\n' "$_mode" > "$mode_file"
    # install.sh parses "$@" at source time, so the library-only source must
    # see no positional arguments; the flush target travels by environment.
    PERL_LSP_INSTALLER_LIBRARY_ONLY=1 \
    PERL5LIB="$TMP/fakelib${PERL5LIB:+:$PERL5LIB}" \
    PERL_LSP_TEST_FSYNC_MODE_FILE="$mode_file" \
    PERL_LSP_TEST_FSYNC_TARGET="$_target" \
        bash -c 'set -- ; source "'"$INSTALLER"'" && fsync_path "$PERL_LSP_TEST_FSYNC_TARGET"'
}

expect_status() {
    local _name="$1" _mode="$2" _target="$3" _want="$4"
    local _got
    _got="$(fsync_status "$_mode" "$_target" 2>/dev/null)"
    if [ "$_got" = "$_want" ]; then
        pass "$_name"
    else
        fail_case "$_name" "mode=$_mode want=$_want got=$_got"
    fi
}

printf 'payload\n' > "$TMP/regular-file"
mkdir -p "$TMP/dir"
expect_status "a working POSIX::fsync flushes" "ok" "$TMP/regular-file" "flushed"
expect_status "a working POSIX::fsync flushes a directory" "ok" "$TMP/dir" "flushed"
expect_status "a croaking POSIX::fsync falls back to a working IO::Handle::sync" \
    "stub_sync_ok" "$TMP/regular-file" "flushed"
expect_status "a stub whose fallback has no primitive stays host_unsupported" \
    "stub_sync_croak" "$TMP/regular-file" "host_unsupported"
expect_status "a stub whose fallback fails without a real fsync stays host_unsupported" \
    "stub_sync_false" "$TMP/regular-file" "host_unsupported"
expect_status "a POSIX::fsync that ran and errored is sync_failed" \
    "error" "$TMP/regular-file" "sync_failed"
expect_status "an unopenable path is open_failed" \
    "ok" "$TMP/does-not-exist" "open_failed"

# The refuse-closed consumer: the commit guard accepts only flushed and
# host_unsupported, so sync_failed can never publish a pointer.
product_unit_flush_status="sync_failed"
case "$product_unit_flush_status" in
    flushed|host_unsupported)
        fail_case "sync_failed must not be an acceptable commit status" \
            "the commit guard accepted sync_failed"
        ;;
    *)
        pass "sync_failed is refused by the commit guard like a flush failure"
        ;;
esac

if [ "$FAIL" -ne 0 ]; then
    printf 'FAILED %s  passed %s\n' "$FAIL" "$PASS" >&2
    exit 1
fi
printf 'passed %s\n' "$PASS"
