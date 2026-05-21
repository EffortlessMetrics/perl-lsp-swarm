# PLSP-SPEC-0026: LSP stack extraction boundary

Status: Draft
Owner: perl-lsp maintainers
Linked proposal: LSP 3.18 Stack Extraction Handoff (2026-05-21)
Linked ADRs: [PLSP-ADR-0004](../adr/PLSP-ADR-0004-lsp-stack-extraction-governance.md)
Linked plan: [plans/lsp-stack-extraction/implementation-plan.md](../../plans/lsp-stack-extraction/implementation-plan.md)
Status impact: Defines extraction scope and proof gates; does not alter shipped runtime behavior by itself.

## Contract

Extraction work must follow these constraints:

1. **Release sequencing**
   - Extraction starts only after 0.14.1 stabilization and Angelo submission work against current perl-lsp is complete.
2. **Allowed extraction surface**
   - Protocol JSON-RPC/message types and LSP method constants.
   - Content-Length transport framing and generic transport interfaces.
   - Feature catalog/profile/flag machinery and capability generation.
   - Generic route registration + handler trait surface.
   - Inline completion protocol types and backend traits.
3. **Disallowed movement in v1 extraction**
   - Perl parser/semantic/document intelligence.
   - Perl deterministic inline completion logic.
   - Perl vendor streaming completion method.
   - DAP protocol/runtime and debug feature surfaces.
4. **Compatibility requirement**
   - Existing perl-lsp behavior and advertised capability JSON remain parity-equivalent for `ga_lock`, `production`, and `all` profiles.
5. **Dependency hygiene requirement**
   - Extracted `lsp-stack` crate must not pull in Perl-only or DAP-only dependencies.

## Acceptance

A PR sequence implementing this spec is acceptable only when all are true:

- PR 0 (docs rails) ships ADR + spec + implementation plan with no production code movement.
- PR 1 introduces boundary traits/interfaces in-place without behavior change.
- Subsequent extraction PRs preserve public behavior via parity tests/snapshots and scoped tests.
- Root `features.toml` is treated as canonical feature catalog during extraction; stale duplicate catalog sources are resolved.
- README positioning for the new crate is factual and avoids unverifiable exclusivity claims.

## Proof Commands

Per extraction PR, run the scoped checks declared by that PR plus fast merge gate:

- `./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked`
- `./scripts/cargo-safe test -p perl-lsp-rs --profile agent --locked`
- `./scripts/cargo-safe check --all-targets -p perl-lsp-rs-core --profile agent --locked`
- `./scripts/cargo-safe xtask fmt`
- `just agent-pr-fast`

Additional per-phase commands (new crate tests, capability snapshots, dependency checks) are defined in the implementation plan.

## Non-goals

- Publishing `lsp-stack` in this docs-only phase.
- Replacing perl-lsp runtime internals in one large migration PR.
- Expanding first release scope to DAP.
- Claiming ecosystem exclusivity without broad, current survey evidence.

## Claim Boundaries

- This spec governs extraction boundaries and verification gates only.
- It does not assert release dates.
- It does not change current user-facing capability claims until corresponding code PRs land with proof.
