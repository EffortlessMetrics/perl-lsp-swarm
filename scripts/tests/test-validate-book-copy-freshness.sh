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

# A missing mapped source must stop the complete freshness check, rather than
# leave a stale committed chapter outside its generated-file enumeration.
missing_sources="$(mktemp -d)"
if PERL_LSP_DOCS_DIR="$missing_sources" bash "$CHECKER" > /dev/null 2>&1; then
    echo "FAIL: checker accepted a missing canonical source" >&2
    exit 1
fi
rmdir "$missing_sources"

# Published status chapters replace committed stubs. Check the generated
# content, including parent-relative, directory, and non-Markdown links.
generated="$(mktemp -d)"
cleanup_generated() {
    if [ ! -d "$generated" ]; then
        return
    fi
    local generated_real temp_parent
    generated_real="$(realpath "$generated")"
    temp_parent="$(realpath "$(dirname "$generated")")"
    case "$generated_real" in
        "$temp_parent"/tmp.*) rm -r "$generated_real" ;;
        *) echo "FAIL: unexpected generated scratch path: $generated_real" >&2; return 1 ;;
    esac
}
trap cleanup_generated EXIT
PERL_LSP_BOOK_SRC="$generated" bash "$REPO_ROOT/scripts/populate-book.sh" > /dev/null
status_index="$generated/reference/status/index.md"
status_quality="$generated/reference/status/quality.md"
grep -Fq 'https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/COMPILER_BACKED_LSP_ROADMAP.md' "$status_index"
grep -Fq 'https://github.com/EffortlessMetrics/perl-lsp-swarm/tree/main/docs/reference/archive/' "$status_index"
grep -Fq 'https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/features.toml' "$status_index"
grep -Fq 'https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/editor_ux.json' "$status_quality"
grep -Fq 'https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/editor_ux.schema.json' "$status_quality"
cleanup_generated

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
    cleanup_generated
}
trap restore EXIT

printf '\nArtificial drift for the freshness-check self-test.\n' >> "$target"
if bash "$CHECKER" > /dev/null 2>&1; then
    echo "FAIL: checker passed despite artificial drift" >&2
    exit 1
fi

echo "validate_book_copy_freshness: clean pass and drift detection both hold."
