# Compiler-Program Implementation Plan

Status: active routing map; orientation foundation complete
Owner: perl-lsp maintainers
Linked proposal: [PLSP-PROP-0002](../../docs/proposals/PLSP-PROP-0002-compiler-program.md)
Linked ADR: [PLSP-ADR-0005](../../docs/adr/PLSP-ADR-0005-hir-body-pir-eir-boundaries.md)
Linked specs:

- [PLSP-SPEC-0025](../../docs/specs/PLSP-SPEC-0025-pir-v0.md) — PIR-A data model
- [PLSP-SPEC-0030](../../docs/specs/PLSP-SPEC-0030-compile-state-layers.md) — compile state layers

Tracker: [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559)
References: #2563 (context), #2564 (HIR-body ADR), #2565 (PIR-A/EIR)

## Purpose and authority

This document preserves the durable compiler-program sequence and layer
boundaries. It does not own generated coverage counts, current work selection,
or provider-support claims:

- accepted specs and ADRs own architecture and layer contracts;
- generated [HIR lowering coverage](../../docs/project/status/hir_lowering.md)
  owns current registry-backed HIR counts;
- human-owned [compiler facts](../../docs/project/status/compiler_facts.md)
  records bounded layer status;
- [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559),
  its re-queried live subordinate issues and PRs own work routing.

## Current state and claim boundary

The orientation foundation landed through #2566: PLSP-PROP-0002,
PLSP-ADR-0005, this plan, and the then-current routing artifact established the
HIR-body/PIR-A/EIR vocabulary. Phase 1 is complete historical context, not an
active gate.

L0-L6 HIR side graphs and Tooling PIR are `fixture-backed`; see
[compiler facts](../../docs/project/status/compiler_facts.md). That human status
page records first-class `Branch`, `Loop`, and `Return` lowering. The canonical
[PIR-A lowering](../../crates/perl-parser-core/src/pir/lower.rs) and
[body fixtures](../../crates/perl-parser-core/tests/pir_a_bodies_test.rs)
show that lowering emits bounded `LexicalRead` and `StashRead` operations and
that fixtures exercise the read-side path without independently discriminating
the exact operation kind. Accepted
PLSP-SPEC-0025 still reserves read-side lowering for later, and the human status
page does not name it, so this implementation drift is explicit rather than
attributed to either authority. The fixtures do not establish full Perl
semantics, precise value merging, EIR, compiler-world completion, or broad
provider cutover.

Do not copy HIR totals into this plan. The generated
[HIR lowering coverage](../../docs/project/status/hir_lowering.md), sourced from
the disposition registry and checked by the completeness tests, is the current
authority for those totals.

Provider promotion remains independently staged. The former provider umbrella
[#8197](https://github.com/EffortlessMetrics/perl-lsp/issues/8197) is completed;
it is historical evidence, not a live sole gate. Use the current
[provider cutover status](../../docs/project/status/provider_cutover.md) for
provider state, support boundaries, and receipts. Use #2559 and re-queried live
GitHub subjects for residual work routing.

## Durable implementation sequence

Expressions-before-dependent-control-flow remains the architectural rule:
expression facts and anchors must exist before later control-flow semantics
depend on them. Landed fixture-backed control-flow operations do not waive this
dependency for semantic broadening.

1. **HIR body constructs and semantic depth** (L0, PLSP-SPEC-0030)
   - Follow the generated HIR disposition registry for missing constructs.
   - Add expression, call, receiver, and context facts with provenance and
     source anchors before downstream consumers rely on them.
2. **PIR-A semantic broadening** (PLSP-SPEC-0025)
   - Extend the landed read-side, assignment, branch, loop, and return families
     only from contracted HIR facts.
   - Keep unknown context, dynamic boundaries, evaluation order, CFG fidelity,
     and value merging explicit rather than inferring semantic completeness
     from operation presence.
3. **Compiler world**
   - Expose the L0-L6 fact graph through the contracted project/workspace owner.
   - Add query and invalidation behavior without creating a duplicate semantic
     authority.
4. **EIR** (PLSP-ADR-0005)
   - Remains a separate future execution concern; do not turn ordinary PIR-A
     expansion into an EIR implementation.
5. **Provider bridge and promotion**
   - Connect compiler-world facts to provider fact-source tracing.
   - Promote behavior only through the provider ledger's freshness,
     confidence, fallback, rollback, and live-path proof.

Each implementation slice remains a separate review-forward issue and PR with
its own claim boundary, files, proof, non-goals, and rollback path.

## Live work routing

Select current work from [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559)
and live subordinate GitHub subjects. Representative current owners include:

- [#7136](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7136)
  for a missing canonical body-HIR regex family;
- [#4239](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4239)
  for remaining LSP-useful PIR-A operation families and the semantic-fact
  adapter;
- [#4799](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4799)
  for the ProjectModel/PIR-A-to-provider semantic-fact port;
- [#5218](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5218)
  for the first lexical references/rename vertical;
- [#6657](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6657)
  for the cross-layer semantic coverage ledger.

Re-query those subjects before claiming or building them; this list describes
ownership seams, not a cached queue or priority order.

Tracked goal-manifest selection was retired by
[#5332](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5332), which
closed [#5205](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5205).
The compatibility command `cargo xtask check-active-goal-manifest` emits a stable
retirement receipt; it validates nothing, selects no work, and mutates nothing.
See the [swarm operating model](../../docs/swarm/operating-model.md).

## Plan verification

Use proof proportional to the selected slice. For source-truth reconciliation,
the discriminating checks are:

```bash
git diff --check
cargo xtask metrics hir-coverage --check
cargo test -p perl-parser-core --test hir_lowering_completeness_tests --locked
cargo test -p perl-parser-core --test pir_tests --locked
cargo test -p perl-parser-core --test pir_a_bodies_test --locked
cargo test -p perl-parser-core --test pir_a_branch_body_test --locked
cargo test -p perl-parser-core --test pir_a_loop_body_test --locked
cargo test -p perl-parser-core --test pir_a_return_body_test --locked
cargo test -p perl-parser-core --test pir_a_ternary_body_test --locked
cargo test -p xtask --test active_goal_manifest_cli --locked
cargo xtask ci-hygiene check-doc-paths plans/compiler-program
```

Generated status, code, specs, provider behavior, and support tiers are outside
this plan-reconciliation claim.
