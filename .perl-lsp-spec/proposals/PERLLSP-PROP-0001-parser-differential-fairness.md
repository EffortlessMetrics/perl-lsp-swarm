# PERLLSP-PROP-0001 — Parser Differential Fairness Lane

## Problem

perl-lsp has concrete parser-differential evidence, but the current comparison rails primarily measure historical internal targets (vendored tree-sitter C snapshot, legacy Pest, and v3 native). That is useful history, but incomplete for current-parser claims.

Veesh Goldman raised a fair question: what does **current** upstream Tree-sitter Perl do on the same fixtures and corpus conditions?

## Proposal

Create a repo-owned, receipt-driven parser differential lane under `.perl-lsp-spec/lanes/parser-differential/` with these durable requirements:

1. Future parser claims require current-target receipts.
2. The parser-comparison harness must expand from fixed v1/v2/v3 wiring to a parser-target registry.
3. The registry roadmap must include:
   - `ts-upstream-crate`
   - `ts-upstream-c`
4. Parser progress must be tracked via fixtures, gap ledgers, and performance budgets.
5. Tool-specific namespaces (`.codex/`, `.spec/`, `.claude/`, `.jules/`) remain awareness-only and are not durable owned lane state.

## Non-goals

- No parser behavior changes in this proposal PR.
- No parser ranking or support-tier promotion.
- No claim about current upstream parser outcomes yet.
