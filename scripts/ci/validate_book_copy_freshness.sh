#!/usr/bin/env bash
# Fail when committed book copies have drifted from what populate-book.sh
# currently generates from the canonical docs tree (#16544).
#
# Like the Helix fixture drift check, this runs against the committed tree
# BEFORE the real population step: it generates into a scratch directory and
# never mutates book/src, so the result reflects repository content only.
# Committed pointer stubs for generated chapters are excluded -- the committed
# tree intentionally carries a pointer there, and only publish-time generation
# writes the full chapter (same pattern as current-status.md).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Chapters whose committed file is a canonical-pointer stub by design.
EXCLUDED_STUBS=(
    "reference/status/index.md"
    "reference/status/lsp.md"
    "reference/status/tests.md"
    "reference/status/parser.md"
    "reference/status/quality.md"
    "reference/status/release.md"
    "reference/configuration-canonical.md"
    "reference/native-critic-rule-matrix.md"
    "reference/performance-slo.md"
    "reference/configuration-schema.md"
)

is_excluded() {
    local rel="$1"
    local entry
    for entry in "${EXCLUDED_STUBS[@]}"; do
        if [ "$rel" = "$entry" ]; then
            return 0
        fi
    done
    return 1
}

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

# Generate into the scratch tree; book/src must remain untouched so the diff
# below reflects the committed tree (PERL_LSP_BOOK_SRC is populate-book.sh's
# output override).
if ! PERL_LSP_BOOK_SRC="$SCRATCH" bash "$REPO_ROOT/scripts/populate-book.sh" > "$SCRATCH/populate.log" 2>&1; then
    cat "$SCRATCH/populate.log"
    echo "FAIL: populate-book.sh failed inside the freshness check" >&2
    exit 1
fi

drift=0
while IFS= read -r generated; do
    rel="${generated#"$SCRATCH"/}"
    if is_excluded "$rel"; then
        continue
    fi
    committed="$REPO_ROOT/book/src/$rel"
    if [ ! -f "$committed" ]; then
        echo "DRIFT: generated chapter is not committed: $rel"
        drift=1
    elif ! diff -q "$committed" "$generated" > /dev/null; then
        echo "DRIFT: committed copy is stale: $rel"
        drift=1
    fi
done < <(find "$SCRATCH" -type f -name '*.md' | sort)

if [ "$drift" -ne 0 ]; then
    echo ""
    echo "Committed book copies drifted from the canonical docs."
    echo "Re-run scripts/populate-book.sh and commit the refreshed copies."
    exit 1
fi

echo "Book copies are fresh: committed copies match docs sources."
