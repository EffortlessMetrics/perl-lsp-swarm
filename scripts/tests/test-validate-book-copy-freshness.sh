#!/usr/bin/env bash
# Self-test for scripts/ci/validate_book_copy_freshness.sh (#16544):
#   1. the checker passes on a fresh tree;
#   2. the checker fails when a committed copy is artificially drifted.
# The drift mutation is a scratch edit restored before exit; it is never
# staged or committed.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CHECKER="$REPO_ROOT/scripts/ci/validate_book_copy_freshness.sh"

# 1. Fresh tree passes.
if ! bash "$CHECKER" > /dev/null; then
    echo "FAIL: checker reported drift on a fresh tree" >&2
    exit 1
fi

# 2. Artificial drift fails.
target="$REPO_ROOT/book/src/user-guides/debugging.md"
backup="$(mktemp)"
cp "$target" "$backup"
restore() {
    cp "$backup" "$target"
    rm -f "$backup"
}
trap restore EXIT

printf '\nArtificial drift for the freshness-check self-test.\n' >> "$target"
if bash "$CHECKER" > /dev/null 2>&1; then
    echo "FAIL: checker passed despite artificial drift" >&2
    exit 1
fi

echo "validate_book_copy_freshness: clean pass and drift detection both hold."
