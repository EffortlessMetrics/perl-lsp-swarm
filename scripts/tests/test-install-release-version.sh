#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"

run_resolution() {
    local mode="$1" version="$2"
    PERL_LSP_INSTALLER_LIBRARY_ONLY=1 INSTALL_MODE="$mode" VERSION="$version" \
        bash -c 'script=$1; shift; source "$script"; resolve_version; if [ "$INSTALL_MODE" = source ]; then validate_source_version_spec "$VERSION_NUM"; fi; printf "resolved=%s/%s\n" "$TAG" "$VERSION_NUM"' \
        _ "$INSTALLER" 2>&1
}

expect_result() {
    local mode="$1" version="$2" status="$3" needle="$4" output actual
    actual=0
    output="$(run_resolution "$mode" "$version")" || actual=$?
    if [ "$actual" -ne "$status" ] || [[ "$output" != *"$needle"* ]]; then
        printf 'FAIL %s %q: status=%s, output=%s\n' "$mode" "$version" "$actual" "$output" >&2
        exit 1
    fi
}

expect_result release 0.17.0 0 'resolved=v0.17.0/0.17.0'
expect_result release v0.17.0 0 'resolved=v0.17.0/0.17.0'
expect_result release 0.17 1 'invalid VERSION=0.17 for release mode'
expect_result release 'not@a version' 1 'invalid VERSION=not@a version for release mode'
expect_result release vv0.17.0 1 'invalid VERSION=vv0.17.0 for release mode'
expect_result source 0.12 1 'invalid VERSION=0.12 for source mode'
expect_result source 0.17.0 0 'resolved=v0.17.0/0.17.0'

printf 'PASS release-mode VERSION validation and source-mode routing\n'
