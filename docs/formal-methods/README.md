# Formal methods in Perl LSP

This directory is the contributor entry point for property-level formal verification in `perl-lsp`.

> **Current status:** the formal-methods control plane is under construction. No TLA+/TLC, Kani, Loom, or deductive claim is currently qualified as a current, production-bound formal `PASS` on `main`. Issue #19296 owns the programme; #19297 owns the infrastructure; PR #19305 is the active claim-inventory candidate. The existing #19147 TLA+/TLC source candidate remains `NOT_PROVEN` until its native checker, production correspondence, and required controls run successfully.

The programme proves selected, high-consequence properties under explicit assumptions and bounds. It does **not** claim that the repository, the Perl language, the parser, the LSP server, or foreign dependencies are globally “formally verified.”

## What formal evidence means here

Every current formal claim has independent dimensions:

| Dimension | Question |
| --- | --- |
| Method | Was the result produced by finite model checking, bounded Rust proof, interleaving exploration, or deductive proof? |
| Binding | Is the subject a model only, a shared production substrate, or a proven live consumer path? |
| Rollout | Does a runner merely exist, or is the exact command selected by the canonical integration route? |
| Native result | Did the exact pinned tool produce `PASS`, a counterexample, or a typed non-pass? |
| Freshness | Does that result still bind every current semantic input? |
| Correspondence | Was sensitivity to the production implementation proved independently from model/harness controls? |
| Profile | Which exact bounds, hosts, targets, features, fairness, unwind, or exploration settings were checked? |

A current product claim requires all applicable dimensions. For example, `MODEL_CHECKED + model_only` is not implementation correctness, and `PASS + stale` is not a current pass.

## Formal claim classes

| Class | Meaning |
| --- | --- |
| `MODEL_CHECKED` | A finite TLA+/TLC model satisfied named safety or liveness properties under recorded constants, fairness, constraints, tool identity, and resource limits. |
| `BOUNDED_PROVED` | Kani proved named Rust properties for the recorded harness, symbolic domain, assumptions, implementation closure, and unwind/recursion bounds. |
| `INTERLEAVING_CHECKED` | Loom explored the recorded shared implementation abstraction under explicit thread, branch, preemption, pruning, and resource bounds. |
| `DEDUCTIVELY_PROVED` | A theorem prover discharged a named theorem from explicit assumptions against a stable kernel or model. |

These are distinct from `TESTED`, `PROPERTY_TESTED`, `FUZZED`, `MIRI_CHECKED`, `ORACLE_AGREEMENT`, and `RECEIPT_VALIDATED`. Those evidence classes remain necessary and must never be relabelled as formal proof.

## Result vocabulary

| Status | Meaning |
| --- | --- |
| `PASS` | The exact native obligation passed and every required control/evidence condition was satisfied for the bound inputs. |
| `COUNTEREXAMPLE` | The native tool produced a reproducible property violation for the bound subject. Attribution to model, harness, abstraction, tool, or product remains a separate investigation. |
| `NOT_PROVEN` | Required work did not establish the property. This includes timeout, resource exhaustion, skip, zero work, malformed or unknown output, missing controls, unsupported constructs, process-settlement failure, and incomplete correspondence. |
| `UNSUPPORTED_HOST` | The current executor cannot run a required obligation. This is visible non-pass evidence, not success. |
| `STALE` | A previously valid result no longer binds the current semantic closure, toolchain, assumptions, bounds, or production path. |
| `RETIRED` | The claim no longer applies because its subject was deliberately removed. |
| `SUPERSEDED` | A newer claim replaces the current claim without rewriting its historical receipts. |

## Programme map

### Control plane

- #19305 — typed claim inventory and rollout scaffold; active candidate, not native proof execution.
- #19308 — proof receipts, semantic implementation closure, native result, freshness, and correspondence.
- #19309 — immutable TLC/Kani/Loom tool identity, process custody, native result adapters, and #19147 migration.
- #19310 — impact routing, smoke/full profiles, artifacts, queue integration, and generated current status.
- #19311 — these contributor, operations, trust, upgrade, and maintenance documents.

### Initial proof domains

- #19147 — bounded current-behaviour workspace resource-rejection TLA+/TLC pilot.
- #19298 — target operation-ticket/currentness/accepted-snapshot protocol.
- #19299 — Loom initialization ownership and workspace settlement/publication races.
- #19300 — first Kani position slice: byte-column APIs and strict reverse UTF-16 conversion.
- #19301 — HIR/PIR IDs, anchors, graph structure, determinism, and fact integrity.
- #19302 — transport framing, progress, bounds, and compatibility-reader separation.
- #19303 — source identity and evidence-envelope fingerprint semantics.
- #19304 — operation-trace allocation, bounds, terminal states, and privacy projection.
- #19306 — parser progress, typed termination, recovery, and span integrity.
- #19307 — unsafe/FFI obligation ownership and evidence reconciliation.

## Documentation map

- [Architecture](ARCHITECTURE.md) — control-plane components, evidence flow, trust boundaries, and claim composition.
- [Authoring and review](AUTHORING.md) — how to define a property, choose a method, bind production, add controls, and review a proof PR.
- [Operations](OPERATIONS.md) — current versus planned commands, execution, triage, replay, and counterexample handling.
- [Maintenance](MAINTENANCE.md) — tool upgrades, freshness audits, ownership, performance budgets, retirement, and release qualification.

The repository’s existing [layered verification protocol](../project/protocols/layered-verification.md) remains the broader review-lens policy. Formal methods add specialized lenses; they do not replace semantic review, reference-oracle checks, real-process tests, installed acceptance, or ordinary CI.

## First integrated milestone

The first end-to-end milestone is deliberately narrow:

```text
current typed claim row
+ versioned semantic closure
+ exact pinned native tool
+ nonzero positive and control denominators
+ retained raw native evidence
+ fail-closed typed result
+ currentness and production-correspondence evaluation
+ conservative impact route
+ generated current status
```

The initial demonstration uses two methods:

1. TLC qualification of #19147 without widening its current-behaviour claim.
2. Kani qualification of #19300’s narrow first position-conversion slice.

All other rows remain explicit `NOT_PROVEN` until their exact obligations are qualified.