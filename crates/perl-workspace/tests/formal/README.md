# Workspace-index resource-rejection pilot

This is a bounded **as-is preservation** model of the production
`perl_workspace::workspace::workspace_index::IndexCoordinator`. It does not
verify the separate `state_machine` implementation or claim that the target
index architecture has landed.

The [existing WSI ledger](../../../../policy/workspace-index-lifecycle-propositions.v1.tsv)
remains the ownership authority. The binding joins WSI-COORD-002 (observation),
WSI-COORD-004 (coarse query routing), WSI-RES-001 (resource admission), and
WSI-INSTR-001 (transition telemetry). No ownership or generated ledger projection
changes are needed for this preservation slice.

## Contract and boundary

The fixture accepts one file, uses `max_files = 1`, and explicitly transitions
the coordinator to Ready. The finite model explores arbitrary serial orderings
of one rejected new file, one update of the accepted file, and repeated state
observations/queries. Safety claims are:

- The accepted file remains admitted.
- The rejected file is not published.
- A query after rejection cannot choose the full/Ready branch.
- Once the pending rejection is consumed, quiescent observations cannot count
  another degradation transition.

This intentionally preserves the current `state()` side effect: it drains a
queued resource rejection and may transition to Degraded. **Observation purity
is still unmet.** Operation-ticket/root-generation settlement, immutable
snapshots, concurrency/linearizability, liveness/fairness, provider exactness and
the blocked #10791/#10799 cutovers are outside this model. The existing ledger's
target-identity gaps must not be marked complete because this pilot passes.

The production integration target is `workspace_index_resource_model`. It uses
the actual index, document store, query and instrumentation APIs. A full-query
control is asserted before rejection, both state-first and query-first
observations are exercised, and accepted-symbol preservation/update is checked.
These scenarios establish correspondence only for the exercised API cases; they
are not a proof that every production execution refines the TLA+ model.

## Check and execute

Adapter and binding controls require Python 3.11+:

```sh
python3 scripts/ci/workspace_index_model.py check
python3 scripts/ci/test_workspace_index_model.py
```

The check command reports native proof `NOT_PROVEN`. The unit controls test
refusal, invalidation, strict native-output interpretation and selection.
Process-instrument tests explicitly skip when the existing owned-child
discovery capability is unavailable; a skip is not process qualification.

The native model route uses the exact TLC 1.7.4 jar and SHA-256 in
`workspace_index_resource_rejection.toolchain.json`:

```sh
python3 scripts/ci/workspace_index_model.py model --jar /absolute/path/tla2tools.jar
```

The adapter verifies and copies the jar into a fresh private run directory,
then executes only that copy. It copies each unchanged model/configuration into
a separate empty input directory so local files cannot shadow standard modules.
It removes Java/TLA library injection variables. Each run has one worker, a
256 MiB heap, a 30-second whole-command deadline and a 1 MiB combined output cap.
The adapter reuses the existing `cargo_admitted.ClippyTree` process owner and
requires settled descendants. Missing capabilities, timeout, output overflow,
malformed frames or incomplete search leave the result `NOT_PROVEN`.

The main configuration enables the complete invariant denominator. Four
known-wrong configurations must fail for their specific property with a native
counterexample; an unrelated checker failure does not count:

| Control | Deliberate defect | Required violation |
| --- | --- | --- |
| bypass | Ignore the pending rejection | `NoFullQueryAfterRejection` |
| publish | Admit the rejected file anyway | `NoRejectedPublication` |
| repeat | Never clear the rejection | `SingleDrain` |
| evict | Drop the previously accepted file | `AcceptedPreserved` |

Configuration tokens are fixed for this pilot: removing invariants or controls,
adding a state constraint, changing bounds or weakening the search is rejected.
Native success requires TLC's complete start/success/statistics/finish frames,
non-empty state exploration, an empty frontier and no contradictory errors.

For local Rust evidence, use the repository's current cargo-admitted preflight
and qualification route for:

```text
cargo test -p perl-workspace --test workspace_index_resource_model --locked
cargo test -p xtask --locked --test workspace_index_lifecycle_ledger
```

The adapter's `conformance` mode is deliberately callable only from the existing
hosted qualification job. It captures Cargo's actual test executable, copies and
hashes it, executes that copy and requires both named cases with no ignored or
filtered tests. Compiler overrides and unowned external Cargo configurations
refuse; permitted build-environment values are hashed rather than exposed.
Present root Cargo configurations must be tracked regular files; their exact
bytes are retained in the environment preimage and rechecked after execution.
This does not authorize new CI triggers or bypass local Cargo admission.

The existing `check-all-targets` job calls adapter controls, a conservative
changed-input selector, and both native routes when affected. No trigger,
publication or contents-write permission is added. Unknown base, graph failure
or unclassified code changes select the pilot rather than silently skip it.
Known local dependency impact comes from Cargo's resolved graph, not a second
committed dependency registry. This source hook is not evidence of deployment
or a hosted run; use the currently authorized qualification route.

## Evidence and currentness

Fresh receipt directories retain raw native stdout/stderr, exact copied inputs,
native return/settlement results and source commit/tree. Model and implementation
claims are separate `proof.state-machine-evidence-binding.v1` objects.

The model digest binds the module plus the explicit Naturals/FiniteSets import
closure and the pinned jar carrying those standard modules. Extra imports or
INSTANCE use refuse. The implementation preimage retains the clean tracked Git
tree plus direct correspondence anchors, including Cargo.lock. This is a
conservative repository commitment, not a claim that dependency source outside
the checkout was inventoried. Runtime/toolchain identity, assumptions, complete
configuration scope, property and adapter bytes have separate digests.

Model-only evidence omits implementation/adapter digests. Changing only a
property, assumption, configuration, actual source tree, adapter or toolchain
cannot reuse a structurally matching receipt. The receipt adapter is not yet a
qualified cargo-proof provider-native validator: metadata agreement alone must
not promote it to admitted proof.

Known-wrong model controls validate the model/checker route. They do **not**
establish that the Rust test fails against a wrong production binary. That
red/green qualification remains a separate required experiment before claiming
production mutation sensitivity. No native run is implied by committed fixtures.

## Reusable practice

1. Discover candidate state transitions from current source, callers and runtime
   observations. Record source anchors and reachability; do not call an inferred
   diagram a semantic extraction proof.
2. Join the existing proposition/ownership ledger. Keep current behavior,
   intended behavior and blocked identity gaps distinct.
3. Select a bounded safety/liveness obligation and write its native model,
   explicit environment assumptions and a real production-facing scenario.
4. Bind model, enabled properties, scope, implementation, adapter and toolchain.
   Add specific known-wrong controls and require non-vacuous native results.
5. Extend the existing ledger ratchet and qualification surface. Recheck affected
   source/model/assumption changes; fail safe when impact is unknown.
6. Retain raw counterexamples and native evidence. Report separately what was
   modeled, tested, unproven and still absent from production.

Other WSI families remain at their current ledger dispositions. This pilot does
not create a universal FSM registry, claim automated semantic discovery is
solved, or close architecture work merely by adding a diagram.
