#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
populate_script="${POPULATE_BOOK_SCRIPT:-$repo_root/scripts/populate-book.sh}"

mapfile -t roadmap_lines < <(grep -F 'ROADMAP\.md' "$populate_script")

if [[ "${#roadmap_lines[@]}" -ne 1 ]]; then
    printf 'expected one ROADMAP rewrite in %s, found %s\n' \
        "$populate_script" "${#roadmap_lines[@]}" >&2
    exit 1
fi

roadmap_substitution="${roadmap_lines[0]#*\'}"
roadmap_substitution="${roadmap_substitution%\'*}"

actual="$(printf '%s\n' \
    '[Roadmap](../project/ROADMAP.md)' \
    '[Near miss](ab/project/ROADMAP.md)' \
    '[Parser roadmap](../project/PARSER_EDGE_CASE_ROADMAP.md)' \
    '[Absolute](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/ROADMAP.md)' \
    | sed -e "$roadmap_substitution")"

expected="$(printf '%s\n' \
    '[Roadmap](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/ROADMAP.md)' \
    '[Near miss](ab/project/ROADMAP.md)' \
    '[Parser roadmap](../project/PARSER_EDGE_CASE_ROADMAP.md)' \
    '[Absolute](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/ROADMAP.md)')"

if [[ "$actual" != "$expected" ]]; then
    printf 'ROADMAP rewrite mismatch\n--- expected ---\n%s\n--- actual ---\n%s\n' \
        "$expected" "$actual" >&2
    exit 1
fi

printf 'ROADMAP rewrite fixture passed\n'
