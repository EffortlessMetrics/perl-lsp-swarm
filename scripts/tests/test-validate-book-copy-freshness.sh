#!/usr/bin/env bash
# Self-test for scripts/ci/validate_book_copy_freshness.sh (#16544):
#   1. the checker passes on a fresh tree;
#   2. the checker fails when a committed copy or canonical source drifts.
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

# 2. A stale committed projection fails.
book_target="$REPO_ROOT/book/src/reference/editor-setup-canonical.md"
# 3. A non-link canonical edit fails until the projection is regenerated.
source_target="$REPO_ROOT/docs/how-to/EDITOR_SETUP.md"
book_backup=""
source_backup=""
book_saved=0
source_saved=0
restore() {
    local result=0
    if [ "$book_saved" -eq 1 ]; then
        cp "$book_backup" "$book_target" || result=1
    fi
    if [ "$source_saved" -eq 1 ]; then
        cp "$source_backup" "$source_target" || result=1
    fi
    if [ -n "$book_backup" ]; then rm -f "$book_backup" || result=1; fi
    if [ -n "$source_backup" ]; then rm -f "$source_backup" || result=1; fi
    cleanup_generated || result=1
    return "$result"
}
trap restore EXIT

book_backup="$(mktemp)" || { echo "FAIL: cannot create book backup" >&2; exit 1; }
cp "$book_target" "$book_backup"
book_saved=1
source_backup="$(mktemp)" || { echo "FAIL: cannot create canonical backup" >&2; exit 1; }
cp "$source_target" "$source_backup"
source_saved=1

printf '\nArtificial drift for the freshness-check self-test.\n' >> "$book_target"
if bash "$CHECKER" > /dev/null 2>&1; then
    echo "FAIL: checker passed despite committed projection drift" >&2
    exit 1
fi
cp "$book_backup" "$book_target"

printf '\nArtificial canonical drift for the freshness-check self-test.\n' >> "$source_target"
if bash "$CHECKER" > /dev/null 2>&1; then
    echo "FAIL: checker passed despite canonical source drift" >&2
    exit 1
fi

echo "validate_book_copy_freshness: clean pass and drift detection both hold."
