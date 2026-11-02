#!/usr/bin/env bash
# Contract test: post-publish smoke must exercise the root install.sh wrapper path
# (#16390). Pins the ref+digest generation from GETTING_STARTED Option 2 and a
# wrong-digest refusal, without running the full post-publish smoke matrix.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SMOKE="$ROOT/scripts/post-publish-smoke.sh"

PASS=0
FAIL=0

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail() {
    printf 'FAIL  %s\n' "$1"
    FAIL=$((FAIL + 1))
}

require_pattern() {
    local label="$1"
    local pattern="$2"
    if grep -Eq "$pattern" "$SMOKE"; then
        pass "$label"
    else
        fail "$label (missing pattern: $pattern)"
    fi
}

if [[ ! -f "$SMOKE" ]]; then
    printf 'FAIL  post-publish-smoke.sh not found at %s\n' "$SMOKE" >&2
    exit 1
fi

require_pattern 'section for root wrapper' 'Root install\.sh wrapper|install\.sh wrapper \(identity'
require_pattern 'tag-dereferenced ref' 'refs/tags/\$\{RELEASE_TAG\}\^\{\}'
require_pattern 'PERL_LSP_INSTALLER_REF export' 'PERL_LSP_INSTALLER_REF="\$INSTALLER_REF"'
require_pattern 'PERL_LSP_INSTALLER_SHA256 export' 'PERL_LSP_INSTALLER_SHA256="\$INSTALLER_SHA256"'
require_pattern 'isolated wrapper copy' 'ISOLATED_WRAPPER|WRAPPER_ONLY|wrapper.*mktemp'
require_pattern 'wrong-digest negative' 'SHA-256 mismatch|wrong digest|BAD_DIGEST|0000000000000000'

printf '\nResults: %d passed, %d failed\n' "$PASS" "$FAIL"
if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
