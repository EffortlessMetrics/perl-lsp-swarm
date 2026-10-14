#!/usr/bin/env bash
# Cheap grep-gate for #17689: fails if stale product names reappear in docs.
# - perl-lsp.perlcritic.* must only appear as deprecated in VS Code setup docs.
# - perl-lsp (hyphenated, not the shipped `perllsp` binary) must not appear
#   in examples/README.md, where users are told how to run the server.
# Usage: scripts/check-stale-doc-names.sh [repo-root]
set -euo pipefail

ROOT="${1:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)}"
VS_CODE="$ROOT/docs/EDITORS/VS_CODE_SETUP.md"
EXAMPLES="$ROOT/examples/README.md"
fail=0

# Current critic settings must be documented.
if ! grep -q 'perl-lsp\.critic\.enabled' "$VS_CODE"; then
  echo "FAIL: perl-lsp.critic.enabled missing from $VS_CODE" >&2
  fail=1
fi

# Every perlcritic.* row must carry "(deprecated" in its default/status
# column (the 4th pipe-delimited field), not merely in its description.
while IFS= read -r line; do
  default_col="$(printf '%s' "$line" | awk -F'|' '{print $4}')"
  if ! printf '%s' "$default_col" | grep -qi '(deprecated'; then
    echo "FAIL: perlcritic setting presented as current in $VS_CODE: $line" >&2
    fail=1
  fi
done < <(grep 'perl-lsp\.perlcritic\.' "$VS_CODE" || true)

# The shipped binary is perllsp; the hyphenated name is not runnable.
if grep -n 'perl-lsp' "$EXAMPLES" >/dev/null 2>&1; then
  echo "FAIL: 'perl-lsp' (not 'perllsp') found in $EXAMPLES:" >&2
  grep -n 'perl-lsp' "$EXAMPLES" >&2 || true
  fail=1
fi
if ! grep -q 'perllsp' "$EXAMPLES"; then
  echo "FAIL: shipped binary name 'perllsp' missing from $EXAMPLES" >&2
  fail=1
fi

# The runnable example client must launch the shipped binary too.
CLIENT="$ROOT/examples/lsp_client.rs"
if [ -f "$CLIENT" ] && grep -n 'Command::new("perl-lsp")' "$CLIENT" >/dev/null 2>&1; then
  echo "FAIL: examples/lsp_client.rs launches 'perl-lsp', not 'perllsp'" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "OK: stale doc names gate passed"
