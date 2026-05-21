# Rails Framework Adoption Burndown

> **Substrate (already built)**: rail scaffolding already exists via `docs/project/RAILS_INDEX.md`, `docs/project/RAIL_TEMPLATE.md`, and active rail docs using substrate + connector + upside framing.
> **Connector gap**: formalize a durable `.rails/` framework footprint (index, templates, schemas, validator, closeout shape) so all future rails share one operating contract.
> **0.14.0 upside**: contributors can add and close rails consistently, with less drift and less per-rail reinvention of templates, receipts, and claim boundaries.

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---|---|---|
| 1. Framework footprint decision (`.rails/` tree + ownership boundaries) | #0000 | yes | — | `rg "^# Rails Framework Adoption Burndown" docs/development/RAILS_FRAMEWORK_ADOPTION_ROLLOUT.md` |
| 2. `index.toml` + artifact ID conventions + lane metadata | #0000 | yes | — | `rg "rails-adoption" .rails/index.toml` |
| 3. Template set (proposal/spec/ADR/lane/closeout) | #0000 | yes | — | `rg "template" .rails -g "*.md"` |
| 4. Validator + schema checks + docs wiring | #0000 | no (depends on 2-3) | — | `just rails-validate` |
| 5. Migration plan from `docs/project/RAILS_INDEX.md` (if selected) | #0000 | no (depends on 1-4) | — | `rg "migration" docs/rails.md docs/contributing/rails.md` |

## Requirements

- Define the canonical `.rails/` footprint and which artifacts are source-of-truth.
- Keep external assistant spaces (`.codex/`, `.spec/`, `.claude/`, `.jules/`) awareness-only.
- Standardize artifact IDs, lane assignment fields, claim-boundary fields, and receipt metadata.
- Provide copy-forward templates for proposals, specs, ADRs, lanes, and closeouts.
- Add a validator contract so malformed rails cannot silently enter the index.

## PR sequence

1. Create this rollout rail doc and register it in `docs/project/RAILS_INDEX.md`.
2. Land minimal `.rails/` directory skeleton and `index.toml` contract.
3. Land template pack + schema docs + validator wiring.
4. Decide whether `docs/project/RAILS_INDEX.md` remains primary or is generated/mirrored.
5. Close rail with reproducible receipts and explicit claim boundary confirmation.

## Exit criteria

A rail is "closed" when ALL of:

- [ ] `.rails/` footprint exists with index, templates, and closeout scaffolding.
- [ ] Validator/schema checks are documented and runnable by contributors.
- [ ] Rails index contract is explicit (authoritative source + any mirrors).
- [ ] Claim boundary and do-not-combine constraints are documented for framework changes.

## Claim boundary

Proves: the repository has one documented, reproducible framework for rail metadata, templates, and validation, reducing process drift across future rails.

Does **NOT** prove: completion of any product rail (editor support, release artifacts, provider contracts, receipts quality), nor the correctness of claims inside individual rail docs.

## Receipts

- `git diff --check`
- `rg "RAILS_FRAMEWORK_ADOPTION_ROLLOUT" docs/project/RAILS_INDEX.md`
- `rg "^# Rails Framework Adoption Burndown" docs/development/RAILS_FRAMEWORK_ADOPTION_ROLLOUT.md`

## Related

- Umbrella issue: #0000
- Index: `docs/project/RAILS_INDEX.md`
- Template: `docs/project/RAIL_TEMPLATE.md`
- Adjacent rails: release artifact contract, configuration schema parity, editor support matrix, support tiers, receipts infrastructure

## Do not combine

- Do not bundle `.rails` framework PRs with implementation changes in LSP/DAP/editor crates.
- Do not mix framework adoption edits with unrelated rail-content burndowns.
- Do not merge multiple independent lane rollouts into the same PR when they touch shared index/template lines.

## Lane assignment

orchestrator for rollout docs and framework policy shape; codex/builder lanes may pick follow-up mechanical template or validator tasks once phases are marked builder-ready.
