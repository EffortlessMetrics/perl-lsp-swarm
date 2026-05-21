# Rails Framework Adoption Burndown

> **Substrate (already built)**: `docs/project/RAILS_INDEX.md` defines the rail model (substrate + connector + upside), builder-ready phases, receipts, claim boundaries, and do-not-combine discipline; `docs/project/RAIL_TEMPLATE.md` provides canonical rail-doc structure.
> **Connector gap**: establish a durable `.rails/` framework footprint (index, templates, schemas, receipts, closeouts, docs) so new rails stop re-inventing structure and proof shape.
> **0.14.0 upside**: every future rail lands with one predictable operating surface, reducing rail-doc drift and review friction.

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---|---|---|
| 1. Define framework footprint and artifact conventions (`.rails/` tree + `index.toml`) | (doc only — file umbrella before pickup) | yes | — | `git diff --check` |
| 2. Add templates/policy docs (`proposal`, `spec`, `ADR`, `lane`, `closeout`) and contributor guide wiring | (doc only — file umbrella before pickup) | yes | — | `git diff --check` |
| 3. Add validator + schema hooks for rail artifacts and receipt metadata | (doc only — file umbrella before pickup) | no (depends on 1-2) | — | `just agent-pr-fast` |
| 4. Migrate index authority from `docs/project/RAILS_INDEX.md` to `.rails/index.toml` (or keep mirrored with single owner) | (doc only — file umbrella before pickup) | no (depends on 1-3) | — | `git diff --check` |

## Requirements

- Keep this rail documentation-only until umbrella issue and ownership are explicit.
- Preserve existing index discipline during rollout: no rail row without a verifiable doc path or umbrella issue.
- Ensure every template encodes mandatory sections: substrate, connector gap, upside, status table, receipts, exit criteria, claim boundary, do-not-combine, lane assignment.
- Define artifact IDs and layout up front to avoid follow-on lane churn.

## PR sequence

1. Add this rail doc + index row (scaffolding only).
2. Create `.rails/` footprint and canonical `index.toml` with migration note.
3. Add templates and contributor docs (`docs/rails.md`, `docs/contributing/rails.md`).
4. Add validator/schemas and receipt conventions.
5. Close migration by selecting one source of truth for rail indexing.

## Exit criteria

A rail is "closed" when ALL of:

- [ ] `.rails/` framework footprint exists with documented ownership and artifact conventions.
- [ ] Template and policy docs are in place and referenced by contributor docs.
- [ ] Validator/schema checks enforce required rail fields.
- [ ] Index authority is explicit (`.rails/index.toml` or documented mirror strategy).
- [ ] Claim boundary is recorded.

## Claim boundary

Proves: perl-lsp has a single, durable rail framework footprint and contribution path for future rails. Does **not** prove: that any individual rail's product claims are complete, that editor-specific rails are implemented, or that release readiness criteria are satisfied.

## Receipts

- `git diff --check`
- `just agent-pr-fast` (after validator wiring lands)

## Related

- Index substrate: [`docs/project/RAILS_INDEX.md`](../project/RAILS_INDEX.md)
- Rail shape template: [`docs/project/RAIL_TEMPLATE.md`](../project/RAIL_TEMPLATE.md)
- Adjacent rails: release artifact contract, configuration schema parity, editor support matrix, support claims and tiers, receipts infrastructure

## Do not combine

- Do not mix this rail's scaffolding with editor-specific implementation rails (VS Code, LSP4IJ, Zed, Neovim latency, DAP).
- Do not combine framework-template migration with unrelated product code changes.
- Do not add unverifiable rows to `RAILS_INDEX.md` without doc path or umbrella issue.

## Lane assignment

orchestrator (rollout docs and framework policy shape); builder pickup begins once umbrella issues exist for executable phases.
