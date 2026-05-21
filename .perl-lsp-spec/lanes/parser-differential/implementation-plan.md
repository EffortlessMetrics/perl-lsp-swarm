# Parser Differential Lane — Implementation Plan

## Objective

Make parser claims fair, current, measurable, and receipt-backed.

## Sequence

1. Rails/docs truth phase
   - Add `.perl-lsp-spec` lane artifacts.
   - Reframe historical tree-sitter docs with explicit claim boundaries.
2. Harness generalization phase
   - Introduce parser target registry.
   - Generalize corpus walker to N targets.
   - Add parser target CLI selection and availability messaging.
3. Current-upstream integration phase
   - Add `ts-upstream-crate` target.
   - Add `ts-upstream-c` target.
4. Evidence hardening phase
   - Fixture bank + expectations.
   - Parser gap ledger policy and checker.
   - JSON/Markdown receipts and advisory performance receipts.
5. Advisory automation phase
   - CI advisory receipt publication.

## Invariants

- No parser behavior changes are implied by rails/docs work.
- `.codex/`, `.spec/`, `.claude/`, `.jules/` remain awareness-only.
- Claims about current upstream parser behavior require current-target receipts.
