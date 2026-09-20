# Acceptance Criteria: #10858 — shared typed train edge and claim-profile contracts

This is a checked, declarative contract plus deterministic validation. It
implements no programme manifest, current-tree probe, frontier, live observer,
packet generation, scheduling, support projection, or external action.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| A relationship's semantics are questioned | Exactly one of the seven closed kinds applies: `requires_implementation`, `requires_behavior_for_claim`, `conditional_release_gate`, `consumes_if_available`, `fan_in`, `external_checkpoint`, `nonblocking_sidecar`; unknown kinds fail with `unknown_edge_kind` | Closed enum pinned in the schema and cross-checked by the validator |
| A conditional gate is evaluated | Selecting authority, selected value, selection subject, active predecessor, and invalidation rule are all present; missing fields fail (`conditional_gate_missing_fields`); undeclared authorities fail (`unknown_selecting_authority`); two active alternatives for one authority+subject fail (`contradictory_conditional_selection`) | Fixtures 2, 8, 9 |
| Optional evidence is absent | `consumes_if_available` targets never enter any profile requirement set (`optional_edge_in_required_set`); profile eligibility is unaffected | Fixtures 1, 3 |
| Fan-in is evaluated | Only independently terminal child propositions satisfy it; `satisfaction_source` is closed; terminal states derived from closed issues, merged PRs, workflow runs, or file state fail (`manufactured_child_success`) | Fixtures 7 + invalid control |
| An external stage is queried | The four stages stay distinct; a stage is satisfied only by `external_observation`; internal packet or merged local PR state fails (`internal_state_cannot_satisfy_external_stage`); a prepared internal packet reaches no external stage | Fixture 5 + invalid control |
| A sidecar is evaluated | The sidecar proposition (the `nonblocking_sidecar` edge **source**) never enters a core requirement set (`sidecar_in_core_requirement_set`); the core target it points at stays a legitimate requirement; sidecars never affect core profile eligibility | Fixture 6 + invalid control |
| Terminal proposition success is declared | A terminal proposition track must carry `derives_from: verified_proposition`; undeclared provenance fails (`missing_derives_from`) and never passes evaluation — missing evidence stays `not_proven`; implementation/evidence/external currentness tracks stay independent without provenance | Validator + evaluator invariants |
| Projection identities duplicate | Duplicate `proposition_states` ids or `external_stage_states` subjects fail (`duplicate_projection_state`, `duplicate_external_stage_state`); lookups are never first-match order-significant | Validator invariants |
| Root-level fields are unknown or mutable | Root unknown fields fail (`unknown_field`); root-level mutable GitHub/check/receipt state fails (`mutable_state_embedded`) | Validator invariants |
| A claim profile is defined | ID, version, required propositions, allowed terminal limitation states, and claim ceiling are present; conditional profiles declare authority and subject together; duplicate requirements and unknown propositions fail | Schema + validator invariants |
| Stage and proposition separation is queried | Implementation, evidence, proposition, and external tracks stay independent; non-terminal tracks carry a closed base reason class (`missing_reason_class`, `unknown_reason_class`); no universal status enum exists | Validator invariants |
| Mutable GitHub/check/receipt state is embedded | Validation fails (`mutable_state_embedded`) | Negative control fixture |
| Input order varies | Canonical semantics are identical: sorted propositions/edges/profiles/states with stable key order (BTreeMap semantics) | Fixture 10 (shuffled twin) |
| A landed programme manifest is adapted | Every dependency class resolves through the declared adaptation table; targets and provenance are preserved; unknown classes fail closed; the adapted document validates against the shared contract | Emacs (124), controller (155), module (197) edges; adaptability checks in the validator |

## §Required fixtures

1. Full-document v0.18 does not require atomic-ranged actual-host proof
   (`neovim_bounded_core` excludes `P_atomic_ranged_actual_host_proof`).
2. Atomic-incremental does require its selected branch (active conditional
   gate inserts the atomic-ranged proof into `neovim_atomic_incremental`).
3. Parser/race evidence can remain absent for a bounded core profile
   (`consumes_if_available` never blocks).
4. One platform or install channel can pass while siblings remain
   `not_proven` (nightly/stable, linux/macos stay independent).
5. A prepared packet is not submitted, accepted, released, or publicly
   installed (`external_stages.v1.json` reaches no external stage).
6. A DAP sidecar cannot block or satisfy LSP core (`nonblocking_sidecar`).
7. Fan-in cannot pass from closed issues or merged helpers
   (`manufactured_child_success`).
8. Unknown edge kind or selecting authority fails (closed vocabulary).
9. Two active conditional alternatives are invalid
   (`contradictory_conditional_selection`).
10. Shuffled input produces identical canonical semantics.

## §Negative controls

Fails when: every edge becomes an unconditional implementation dependency
(requirement-set inflation is observable and rejected), a programme-specific
state or field is normalized away, `consumes_if_available` blocks unrelated
work, a fan-in manufactures a child result, an external checkpoint is
satisfied by internal state, a sidecar enters a core spine, mutable
GitHub/check/receipt state is embedded, or this contract introduces a
universal manifest, frontier, scheduler, live observer, support registry, or
release authority.

## §Boundaries

- No product, support, live GitHub, release, or external mutation occurs.
- No runtime state is persisted by the contract; schemas, validator, and
  fixtures are durable artifacts.
- #10554 remains the only shared-mechanics extraction owner.
