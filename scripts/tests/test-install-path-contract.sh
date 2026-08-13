#!/usr/bin/env bash
# Static cross-platform contract test for installer PATH behavior (#7832).
# Windows PowerShell cannot run on the Linux gate host, so source invariants are
# checked here; actual fresh-process Windows proof remains in #5903/#7746.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WINDOWS_INSTALLER="$ROOT/install.ps1"
POSIX_INSTALLER="$ROOT/scripts/install.sh"

fail() {
    printf 'FAIL  %s\n' "$1" >&2
    exit 1
}

pass() {
    printf 'PASS  %s\n' "$1"
}

[[ -f "$WINDOWS_INSTALLER" ]] || fail "missing install.ps1"
[[ -f "$POSIX_INSTALLER" ]] || fail "missing scripts/install.sh"

# Windows: default user-local installation owns persistent User PATH, with an
# explicit opt-out. Never copy the merged process/system/user PATH back into
# User scope.
grep -Fq '[switch]$NoModifyPath' "$WINDOWS_INSTALLER" \
    || fail "Windows installer must expose -NoModifyPath"
grep -Fq '[Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")' "$WINDOWS_INSTALLER" \
    || fail "Windows installer must persist only the constructed User PATH"
if grep -Eq 'SetEnvironmentVariable\([^\n]*\$env:Path[^\n]*"User"' "$WINDOWS_INSTALLER"; then
    fail "Windows installer must not copy merged process PATH into User PATH"
fi
grep -Fq 'manual_path_action_required' "$WINDOWS_INSTALLER" \
    || fail "Windows installer must expose an explicit manual PATH disposition"
grep -Fq 'persisted_user_path_restart_required' "$WINDOWS_INSTALLER" \
    || fail "Windows installer must expose the persisted/restart disposition"
grep -Fq 'PATH status:' "$WINDOWS_INSTALLER" \
    || fail "Windows installer must render the PATH disposition"
pass "Windows installer owns a bounded persistent user-PATH contract"

# POSIX remains intentionally non-invasive for now. If the user-local fallback
# is not already on PATH, the installer must keep saying a manual action is
# needed rather than allowing an easy-install receipt to infer zero setup.
grep -Fq '$HOME/.local/bin' "$POSIX_INSTALLER" \
    || fail "POSIX installer no longer exposes the user-local fallback"
grep -Fq 'is not in PATH' "$POSIX_INSTALLER" \
    || fail "POSIX installer must diagnose a user-local directory outside PATH"
grep -Fq 'Add it by appending one of these lines' "$POSIX_INSTALLER" \
    || fail "POSIX installer must keep the manual PATH remediation explicit"
pass "POSIX installer remains explicitly manual when the fallback is outside PATH"

printf 'Installer PATH contract self-test passed.\n'
