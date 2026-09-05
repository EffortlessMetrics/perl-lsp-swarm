# Context: #10858 — shared typed train edge and claim-profile contracts

This is a checked, declarative contract with deterministic validation. It adds
no universal train manifest, node vocabulary, frontier, live observer, support
evaluator, scheduler, work database, current-tree probe, packet generation, or
release authority. Programme manifests embed or adapt the contract and may add
stricter local fields without changing the shared meaning.

## Problem

Programme trains repeatedly need to distinguish materially different
relationships: implementation prerequisites, behavior required only for a
stronger claim, conditionally selected branches, optional evidence, fan-in
composition, external preparation/submission/acceptance/public-availability
stages, and non-blocking sidecars. An unqualified `depends_on` flattens those
distinctions, producing false serialization, misleading frontiers, and cheap
agents waiting unnecessarily or promoting claims from the wrong evidence.

## Scope ruling

- This contract owns the **shared semantic base only**: the closed edge
  vocabulary, the external checkpoint stages, the base reason traits, the
  minimal claim-profile shape, deterministic validation invariants, canonical
  semantics, and the declared class-to-kind adaptations binding landed
  programme manifests.
- Every programme retains its own node IDs and roles, implementation and
  evidence propositions, domain-specific refinements, claim profiles and
  accepted limitations, current-tree proof anchors, frontier and packet
  projection, live candidate observer, and support/publication semantics.
- #10554 remains the only shared-mechanics extraction owner; a programme may
  consume this contract before #10554 lands by using its own checked adapter.
  No programme is required to migrate merely because this contract exists.
- #10872 (agent packet schema) and #10881 (review/finding/closure contracts)
  remain separate claims; this contract neither defines nor gates them.

## Landed consumers this contract must express without rewriting

| Manifest | Schema | Edges | Classes used |
|---|---|---|---|
| `.spec/10918-emacs-train-graph` | `emacs_train.v1` | 124 | hard 104, evidence 19, optional 1 |
| `.spec/11764-controller-train-graph` | `issue_controller_train.v1` | 155 | hard 138, evidence 11, optional 5, external 1 |
| `.spec/11625-module-train-graph` | `module_train.v1` | 197 | hard 96, evidence 101 |

Dependency targets reference either node IDs or declared external authority
IDs; the contract therefore resolves edge targets against propositions plus
declared external subjects. The declared adaptation table
(`adaptations.json`) binds every landed class to exactly one shared kind and
fails closed on any manifest class without a declared row.

## Shared artifacts

- `schemas/train_edge_contract.v1.schema.json` — the versioned closed contract
  (edge kinds, external stages, reason traits, derives-from provenance,
  claim-profile shape, four independent projection tracks).
- `fixtures/train_edge_contract/` — programme-neutral fixtures encoding the
  ten required fixtures of #10858, including the shuffled canonical-semantics
  control and the invalid set with expected reason codes.
- `.spec/10858-train-edge-contract/adaptations.json` — the declared
  class-to-kind adaptation table for the landed manifests.
- `cargo xtask check-train-edge-contract` — deterministic validation of the
  closed vocabulary, the fixtures, the canonical control, and the landed
  manifest adaptations.

## Integration

- `#10554` may later extract common graph mechanics; this contract supplies
  the vocabulary such an extraction would consume.
- Programme trains (Emacs E06/T07 packet adapters, module train frontier
  tooling, Coc profiles) consume these semantics through the adaptation table
  or direct embedding; they do not copy them into local schemas.
