#!/usr/bin/env bash
# Static cross-platform contract for Windows product-unit durability (#17155).
# PowerShell promotion proof runs on windows-latest; source invariants are
# checked here so Linux CI can refuse a durability=flushed claim without
# directory-entry flushes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WINDOWS_INSTALLER="$ROOT/install.ps1"

fail() {
    printf 'FAIL  %s\n' "$1" >&2
    exit 1
}

pass() {
    printf 'PASS  %s\n' "$1"
}

[[ -f "$WINDOWS_INSTALLER" ]] || fail "missing install.ps1"

if ! grep -Fq 'function Invoke-StandaloneCandidateDurability' "$WINDOWS_INSTALLER"; then
    pass "product-unit durability seam not present; directory contract not applicable"
    exit 0
fi

grep -Fq 'function Invoke-StandaloneFlushDirectory' "$WINDOWS_INSTALLER" \
    || fail "install.ps1 must flush directories via Invoke-StandaloneFlushDirectory"
grep -Fq '0x02000000' "$WINDOWS_INSTALLER" \
    || fail "install.ps1 directory flush must use FILE_FLAG_BACKUP_SEMANTICS (0x02000000)"
grep -Fq 'dir candidates-parent' "$WINDOWS_INSTALLER" \
    || fail "install.ps1 must flush the candidates parent directory entry"
grep -Fq 'Invoke-StandaloneFlushDirectory -Path $dirEntry.Path' "$WINDOWS_INSTALLER" \
    || fail "Invoke-StandaloneCandidateDurability must flush directory paths"
grep -Fq 'pointer_flush=$($script:StandaloneProductUnitPointerFlush)' "$WINDOWS_INSTALLER" \
    || fail "install.ps1 receipt must record pointer_flush separately from durability="
grep -Fq 'function Invoke-StandaloneFlushStoreDir' "$WINDOWS_INSTALLER" \
    || fail "install.ps1 must flush the store directory after the selector move"

pass "Windows product-unit durability contract requires directory-entry flushes"
