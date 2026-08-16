# PLSP-PROP-0002: Repo-native compiler-program contracts

Status: proposed; orientation foundation complete
Owner: perl-lsp maintainers
Created: 2026-06-21
Target milestone: Compiler-program gate (tracker #2559)
Linked specs: #2563 (context), #2564 (HIR-body ADR), #2565 (PIR-A/EIR)
Linked ADRs: [PLSP-ADR-0005](../adr/PLSP-ADR-0005-hir-body-pir-eir-boundaries.md)
Linked plan: [plans/compiler-program/implementation-plan.md](../../plans/compiler-program/implementation-plan.md)
Support/status impact: compiler facts, HIR lowering coverage, PIR v0 substrate, provider cutover gating
Policy impact: HIR-body/PIR-A/EIR terminology becomes canonical; no generated status is altered here

## Problem

When this proposal landed, `perl-lsp` had a fixture-backed compiler substrate:
HIR items, scope/pad,
package/stash, compile environment, import/export, compile-time effects,
framework adapters, and PIR v0. The layers are contracted in
[PLSP-SPEC-0030](../specs/PLSP-SPEC-0030-compile-state-layers.md), but the
next level of the roadmap — compiler world, abstract compile engine, provider
bridge, and the distinction between tooling PIR and a future execution IR — had
no repo-native artifacts. The accepted ADR and implementation plan now carry
those durable boundaries.

The risk this proposal addressed was that PRs could drift between HIR body
expansion, PIR control-flow broadening,
and speculative execution-IR experiments without a durable boundary saying which
is which. That tension is already visible: PIR v0 is named "tooling IR" in one
place and "PIR" in another, and there is no canonical record clarifying whether
EIR (execution IR) is an evolution of PIR or a separate concern.

## Users and Surfaces

- Compiler-substrate engineers adding HIR body constructs or PIR operations
- Agents routing build work to HIR-body vs. PIR-A vs. EIR slices
- Reviewers deciding whether a PR is in-contract with the layer order
- Plan reviewers preserving expressions-before-dependent-control-flow order
- LSP provider engineers planning provider cutover sequences

## Current Evidence

Current facts live in generated or human-owned status docs. This proposal
links to those sources instead of duplicating their tables.

- [HIR lowering coverage](../project/status/hir_lowering.md) is the generated,
  registry-backed authority for current HIR construct counts and dispositions.
- [Compiler facts](../project/status/compiler_facts.md) records Tooling PIR as
  `fixture-backed`, including first-class `Branch`, `Loop`, and `Return`
  lowering. Current [PIR-A lowering](../../crates/perl-parser-core/src/pir/lower.rs)
  emits bounded read-side operations, while the current
  [body fixtures](../../crates/perl-parser-core/tests/pir_a_bodies_test.rs)
  exercise that path without independently discriminating exact `LexicalRead`
  from `StashRead`; the human status page does not yet record this narrower fact.
- [Provider cutover status](../project/status/provider_cutover.md) owns current
  provider state, support boundaries, and receipts. The former umbrella
  [#8197](https://github.com/EffortlessMetrics/perl-lsp/issues/8197) is completed
  historical evidence, not a live sole gate; #2559 and re-queried live GitHub
  subjects own residual work routing.
- [PLSP-SPEC-0025 (PIR v0)](../specs/PLSP-SPEC-0025-pir-v0.md) contracts the
  PIR-A data model and its operation families, but still describes read-side
  lowering as reserved future work. The code and fixture evidence above is
  narrower current implementation truth; this accepted-spec drift remains
  explicit rather than being silently attributed to the spec.
- [PLSP-SPEC-0030 (Compile state layers)](../specs/PLSP-SPEC-0030-compile-state-layers.md)
  contracts L0–L6 with no PIR-A or EIR layer defined; those belong in the
  next layer's contract, not in the PLSP-SPEC-0030 revision.

The compiler roadmap ([COMPILER_BACKED_LSP_ROADMAP.md](../project/COMPILER_BACKED_LSP_ROADMAP.md))
names "tooling IR" in the pipeline but does not define HIR-body expansion
ordering, SemanticSnapshot extension, compiler world, abstract compile engine,
or the EIR branch-off point. This proposal provides the missing orientation.

## Success Criteria

- A canonical ADR records the HIR-body / PIR-A / EIR terminology and boundary
  rules so any future IR-adjacent PR can be checked against it without chat context.
- A proposal records the lane motivation, user surfaces, and non-goals so
  reviewers know what the compiler-program lane is for.
- An implementation plan preserves expressions-before-dependent-control-flow
  ordering and the remaining cross-layer sequence without caching a live gate.
- Live [tracker #2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559)
  and its subordinate GitHub subjects route current work.
- No generated status is altered; no provider behavior is changed; no PIR
  operations are added; no EIR crate is created.

## Proposed Shape

The accepted orientation is organized around three durable repo-native artifacts
plus live GitHub routing:

**Proposal (this document)**: records why the lane exists, the user surfaces,
and the product motivation. Does not encode PR sequence or generated metrics.

**Boundary ADR (PLSP-ADR-0005)**: records the durable terminology decision —
PIR-A is the evolution of the existing tooling PIR (`crates/perl-parser-core/src/pir/`);
EIR is a future execution IR that branches off PIR-A later; HIR body items are
the HIR-layer shells for expressions and control-flow, not a new IR. Agents and
reviewers may cite this ADR when deciding whether a PIR PR is in-contract.

**Implementation plan (plans/compiler-program/)**: preserves the durable
expressions-before-dependent-control-flow ordering and remaining cross-layer
sequence. Current slices and their readiness come from live GitHub subjects,
not a phase gate cached in this document.

**GitHub routing**: tracker
[#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559) and
current subordinate issues, PRs, reviews, and checks own live work selection.
Tracked goal-manifest selection was retired by
[#5332](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5332), closing
[#5205](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5205).
Compatibility commands emit retirement receipts but validate nothing and select
no work.

## Alternatives Considered

### Extend PLSP-SPEC-0030 with a PIR-A and EIR layer definition

Rejected. PLSP-SPEC-0030 contracts the compile-state stack (L0–L6). PIR-A and
EIR sit above and downstream of that stack; adding them to PLSP-SPEC-0030
would conflate HIR-to-provider lowering with IR-for-execution concerns. The
correct place for PIR-A/EIR boundary rules is a focused ADR.

### Put all lane guidance in the compiler roadmap

Rejected. The roadmap is a prose design document, not a durable decision
record. Agents cannot verify whether a PIR PR is in-contract by reading
roadmap prose. A dedicated ADR and plan provide checkable, single-purpose
artifacts that the roadmap can link to.

### Defer all orientation documents until EIR work is imminent

Rejected. HIR-body and PIR-A expansion required the boundary before EIR work.
That orientation foundation is now complete, and fixture-backed
branch/loop/return/read-side lowering has since landed. Further semantic depth
still follows the accepted boundary rather than implying EIR or provider
completion.

## Non-goals

- No new HIR body shells (expressions, control-flow items)
- No new PIR operations (branch, loop, return, read-side)
- No EIR crate or EIR data model
- No compiler world or abstract compile engine implementation
- No provider cutover or live behavior change
- No generated status alteration
- No parser/corpus bucket movement
- No release-lineage sync claim

## Evidence Plan

Docs-only check:

```bash
git diff --check
cargo xtask ci-hygiene check-doc-paths docs/proposals
cargo xtask ci-hygiene check-doc-paths docs/adr
cargo xtask ci-hygiene check-doc-paths plans/compiler-program
```

## Exit Criteria

The lane can close when all of these are true:

- PLSP-ADR-0005 is accepted and merged with the HIR-body/PIR-A/EIR boundary rules
- Implementation plan preserves expressions-before-dependent-control-flow
  ordering without claiming a current phase gate
- Live tracker #2559 and subordinate GitHub subjects own current routing
- No generated status, provider behavior, or PR sequence is altered by the docs-only PR
- Agents and reviewers can cite ADR-0005 to decide whether a future IR PR is in-contract

## Claim Boundary

This proposal defines lane orientation and document locations. It does not add
HIR body shells, add PIR operations, create EIR, change provider behavior, alter
generated status, claim parser movement, or authorize provider cutover. Those
changes require their own specs, plans, receipts, and PR-sized proof.
