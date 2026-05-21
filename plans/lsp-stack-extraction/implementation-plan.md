# LSP Stack Extraction Implementation Plan

Status: Proposed  
Last updated: 2026-05-21  
Scope owner: perl-lsp maintainers

## Objective

Extract a reusable LSP 3.18-oriented stack from perl-lsp in narrow, reversible phases while preserving perl-lsp behavior and release stability.

## Preconditions

1. 0.14.1 release stabilization completed.
2. Angelo/LSP4IJ submission work completed against the current perl-lsp layout.
3. [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction-governance.md) accepted.
4. [PLSP-SPEC-0026](../../docs/specs/PLSP-SPEC-0026-lsp-stack-extraction-boundary.md) present and current.

## Phased execution

### Phase 0 — Control-plane rails (docs only)

Deliverables:
- ADR for extraction governance.
- Spec for extraction boundary + proof contract.
- This implementation plan.

Exit criteria:
- No production code movement.
- Reviewers can validate scope and proof expectations from docs alone.

### Phase 1 — Boundary traits in-place

Deliverables:
- Internal boundary module in `perl-lsp-rs-core` for language adapter, capability descriptors, handler and routing traits, transport reader/writer traits.

Exit criteria:
- Existing dispatch path remains active.
- No externally visible behavior changes.

Proof commands:
- `./scripts/cargo-safe test -p perl-lsp-rs-core --profile agent --locked`
- `./scripts/cargo-safe test -p perl-lsp-rs --profile agent --locked`

### Phase 2 — Protocol extraction

Deliverables:
- New `crates/lsp-stack` created.
- JSON-RPC types/errors/method constants moved to `lsp-stack`.
- Compatibility re-exports retained in `perl-lsp-rs-core`.
- Method constants split into `standard`, `proposed_3_18`, `experimental`, and `vendor` namespaces.

Exit criteria:
- Existing imports continue compiling via shims.
- Standard method constants remain behavior-equivalent.

### Phase 3 — Feature/capability extraction

Deliverables:
- Feature profile/flag and capability builder machinery moved to `lsp-stack`.
- Raw JSON patching promoted to first-class API.
- Perl-specific values supplied through a Perl adapter.

Exit criteria:
- Capability snapshots for `ga_lock`, `production`, `all` are parity-equivalent before/after.

### Phase 4 — Transport extraction

Deliverables:
- Content-Length framing and reader/writer helpers moved to `lsp-stack`.
- Stdio transport default; optional TCP/Tokio features where justified.

Exit criteria:
- Existing framing tests pass in new location.
- perl-lsp consumes transport via `lsp-stack` paths.

### Phase 5 — Inline completion protocol extraction

Deliverables:
- Generic inline completion protocol types and backend traits in `lsp-stack`.
- Perl deterministic logic and vendor streaming extension remain in perl-lsp.

Exit criteria:
- One-shot inline completion behavior unchanged.
- Generic module has no Perl parser/semantic dependency.

### Phase 6 — Consumer wiring and hardening

Deliverables:
- perl-lsp crates consume extracted modules as normal dependency.
- Optional compatibility shims retained for one minor cycle if needed.

Exit criteria:
- No 0.14.x behavior regressions.
- Dependency hygiene checks pass.

### Phase 7 — Standalone credibility and publish prep

Deliverables:
- Non-Perl examples, docs, and conformance-oriented tests in `lsp-stack`.
- Packaging and publish dry-run.

Exit criteria:
- docs.rs-ready crate without overclaiming ecosystem status.

## Guardrails

- One concern per PR.
- No DAP extraction in initial `lsp-stack` milestone.
- Keep root `features.toml` canonical during migration.
- Add/maintain capability parity snapshots for every extraction phase that changes capability generation.
- Enforce dependency hygiene (`cargo tree -p lsp-stack`) against Perl-only and DAP crates.

## Rollback strategy

If any phase causes unacceptable behavior drift:
1. Re-enable compatibility re-exports/path shims.
2. Revert phase-specific movement PR only (no cross-phase coupling).
3. Re-run parity snapshot and scoped test proofs.
4. Re-attempt with narrower extraction slice.
