# PLSP-SPEC-0039: Semantic contract packet E (parser-to-semantic and unsupported facts)

Status: draft
Owner: [#6599](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6599)
Linked specs:
- [PLSP-SPEC-0030](PLSP-SPEC-0030-compile-state-layers.md) — compile-state layer stack and parser-to-HIR transitions
- [PLSP-SPEC-0038](PLSP-SPEC-0038-unsupported-fact-catalog.md) — unsupported-fact class catalog
- [PLSP-SPEC-0017](PLSP-SPEC-0017-fact-provenance-and-source-backing.md) — provenance classes consumed at semantic boundaries
Linked policy:
- `policy/semantic/packet-e-issue-routing.v1.toml` — machine-readable routing for included issue IDs
Status impact: semantic contract documentation, harness compatibility expectations, issue routing only

## Purpose

Packet E makes two inference seams explicit and test-backed:

1. how parser and compile-state output becomes neutral semantic facts; and
2. how unsupported or partially modeled constructs must surface through the
   unsupported-fact catalog and envelope status rules rather than silent exact
   promotion.

This spec does not implement catalog migration, provider cutover, or harness
ledger generation. It documents the contract and routes residual work to three
bounded follow-up owners.

## Contract — Unsupported facts

Unsupported or partially modeled compiler behavior must not be promoted to
exact semantic proof without a catalog class or an explicit boundary record.

Normative surfaces:

- Class vocabulary and entry fields:
  [PLSP-SPEC-0038](PLSP-SPEC-0038-unsupported-fact-catalog.md) and
  `policy/compiler/unsupported-facts.v1.toml`.
- Envelope status rules (`Exact`, `Degraded`, `Refused`, `Stale`) in
  `crates/perl-semantic-facts/README.md` and `SemanticFactEnvelope::status()`.
- Provenance classes that forbid silent exact promotion:
  [PLSP-SPEC-0017](PLSP-SPEC-0017-fact-provenance-and-source-backing.md).

A parser, HIR, semantic-analyzer, or provider surface that recognizes unsupported
behavior must project to a stable catalog class ID or an explicit boundary /
`Refused` envelope outcome. Prose-only reasons are not receipt-compatible
identity.

## Contract — Parser-to-fact transitions

The compile-state parser-to-semantic path is layered; no stage may skip a lower
layer or infer facts from stale or foreign layer state.

| Stage | Owner | Output | Semantic fact entry |
| --- | --- | --- | --- |
| Parse | `perl-parser-core` | AST | not a semantic fact surface |
| Lower | `perl-parser-core::hir::lower_ast` | `HirFile` L0–L6 compile-state facts | projected via compile-state layers ([PLSP-SPEC-0030](PLSP-SPEC-0030-compile-state-layers.md)) |
| Analyze | `perl-semantic-analyzer` | synthesized facts, summaries, framework adapters | typed records in `perl-semantic-facts` |
| Transport | `perl-semantic-facts` | `SemanticFactEnvelope` and versioned contract modules | provider/workspace consumers |

**Direct AST import extraction (workspace index).** Some analyzer facts are
extracted straight from parser AST without `lower_ast` or `HirFile` compile-state
layers. `ImportExtractor::extract` walks the AST and emits `ImportSpec` records
in `perl-semantic-facts`; the live workspace index uses this path for `use` /
`require` discovery. That path still obeys analyzer provenance and envelope
classification rules, but it is not a compile-state-layer projection.

Transition rules (compile-state path and direct analyzer paths):

- Parser AST nodes are not neutral semantic facts; only lowered compile-state
  projections and analyzer-emitted typed facts enter `perl-semantic-facts`.
- Each emitted fact must carry provenance and confidence metadata sufficient
  for envelope classification; unknown metadata fails closed to non-exact status.
- Unsupported lowering or analyzer gaps must link a catalog class or boundary
  identity; they must not mint exact facts with `SemanticReasonCode::ExactSource`.
- Cross-file or dynamic behavior stays boundary-qualified until the owning
  harness or profile receipt proves otherwise.

The module `perl_semantic_facts::parser_to_semantic_contract` restates this
transition note for crate-local discovery; this spec remains normative.

## Contract — Harness and platform-sensitive compatibility

Bootstrap and platform-sensitive upstream behavior is indexed by
`perl-core-harness` target matrices and invocation traces; it is not implied
by Linux-only semantic tests alone.

Normative expectations:

- Platform-only upstream directories (`t/os2`, `t/win32`, …) appear in the
  checked-in target matrix with explicit capability predicates and a
  `platform_unavailable` disposition on the Linux-authoritative programme.
- Optional high-resource populations (`benchmark`, `bigmem`, …) stay out of the
  default denominator with documented claim boundaries.
- TestInit / bootstrap classes in invocation traces remain typed; bootstrap
  dispositions are proven through harness receipts, not inferred from parse
  success alone.

Focused proof lives in `crates/perl-core-harness/tests/semantic_contract_packet_e_compat.rs`.

## Packet E issue routing

Every issue ID listed in [#6599](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6599)
routes to exactly one bounded follow-up owner:

| Follow-up | Owner issue | Scope |
| --- | --- | --- |
| Compatibility ledger | [#4736](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4736) | SHA-bound receipts, semantic boundaries, bootstrap/platform harness proof |
| Fact taxonomy | [#2543](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2543) | Unsupported-fact taxonomy, catalog, compile-state / PIR fact classes |
| Capability backlog | [#2559](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2559) | Compiler programme capabilities, spec reconciliation, EIR entry, closed wave carryover |

The authoritative routing table is `policy/semantic/packet-e-issue-routing.v1.toml`.
Agents must not open parallel routing prose that contradicts that file.

## Acceptance mapping (#6599)

| #6599 criterion | Satisfied by |
| --- | --- |
| Explicit contract note (unsupported facts + parser-to-fact transitions) | This spec + `parser_to_semantic_contract` module |
| Harness/compatibility tests (bootstrap + platform-sensitive) | `semantic_contract_packet_e_compat.rs` |
| Route included issue IDs to three follow-ups | `packet-e-issue-routing.v1.toml` + xtask contract test |

## Non-goals

- No provider behavior change.
- No catalog seed expansion beyond existing PLSP-SPEC-0038 vocabulary.
- No automatic issue closure from routing alone.
