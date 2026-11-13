# Target workspace protocol model

This is the **model-only** first slice of [#19298](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/19298),
under the formal program [#19296](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/19296).
It specifies the intended operation protocol from #10791, #10799 and #10811.
It does not assert that current Rust implements this protocol.

## Subject and bounds

The general `positive.cfg` profile models one logical root, two never-reused ticket IDs,
at most one active operation, two root incarnations, and independently changing
document and configuration versions (0 and 1). A ticket captures the complete
modeled subject when it registers. A snapshot is an immutable opaque content token
identified by the ticket that published it; the model contains no parser or actual
snapshot bytes. There is one waiter, observing ticket 1 and its captured subject.
These are finite bounds, not a cutoff theorem for larger systems.

`positive-capacity.cfg` separately enumerates **three** never-reused tickets at
capacity one, with document/configuration fixed at zero and one root incarnation.
It covers accept A, register B, reject C, and retain A. Two tickets cannot reach
that state, so the general profile alone cannot establish rejection preservation
with a retained snapshot. This bounded composition profile is not a claim about
three tickets combined with all general-profile version/interleaving dimensions.

No symmetry reduction, state constraint, action constraint, probabilistic search,
or early success bound is used. The positive configuration enumerates the reachable
graph and checks both safety and liveness. Each wrong configuration changes exactly
one named fault parameter and checks its named detector.

The `Finish` action models the final currentness comparison and snapshot swap as
**one atomic action**. That is a requirement on the eventual publication sink,
not a fact established about a pre-publication eligibility check. Root replacement
and document/configuration changes interleave with registration, work, publication,
and waiting, but cannot interleave inside that single modeled action.

`Availability` deliberately differs from `Ready`: a retained snapshot may remain
`stale_available` after document/configuration invalidation. A snapshot from a
removed/reincarnated root is not available. Failed replacement preserves the prior
token; it does not restore the old subject's exactness.

## Properties and controls

| Wrong configuration | Required failure | Defect |
| --- | --- | --- |
| `wrong-doubleSettlement.cfg` | `ExactlyOnce` | Settle a terminal ticket twice |
| `wrong-stalePublication.cfg` | `NoStalePublication` | Publish after source/root/config movement |
| `wrong-dropRetained.cfg` | `PreserveAccepted` | Drop prior snapshot on a failed replacement |
| `wrong-consumingObservation.cfg` | `ObservationPurity` | Observation consumes terminal outcomes |
| `wrong-lostWakeup.cfg` | `EventuallyObserved` | Skip the post-registration terminal recheck |
| `wrong-reuseRoot.cfg` | `ReincarnationIsolation` | Reuse generation at the same logical root |
| `wrong-revokedPublication.cfg` | `NoRevokedPublication` | Publish a cancelled/superseded ticket |
| `wrong-readyWithoutSnapshot.cfg` | `ReadinessSoundness` | Claim Ready without any snapshot |
| `wrong-ignoreDocument.cfg` | `ReadinessSoundness` | Ignore document movement with root/config fixed |
| `wrong-phantomBuilding.cfg` | `BuildingSoundness` | Claim Building without registered work |
| `wrong-rejectionAccounting.cfg` | `CapacityAccounting` | Count rejected admission as active work |
| `wrong-stuckOperation.cfg` | `EventuallySettled` | Make registered work unable to advance |
| `wrong-wrongWaitSubject.cfg` | `WaiterSafety` | Observe terminal work under another captured subject |
| `wrong-failureRestoresCurrent.cfg` | `ReadinessSoundness` | Promote retained stale data to Ready after failure |
| `wrong-rejectionDropsPrior.cfg` | `PreserveAccepted` | Three-ticket rejection destroys accepted A |
| `wrong-rejectedPublication.cfg` | `AcceptedSnapshot` | Install a resource-rejected ticket as snapshot |
| `wrong-alwaysUnavailable.cfg` | `RetainedAvailability` | Report every retained snapshot unavailable |
| `wrong-failureDropsAvailability.cfg` | `PreserveAvailability` | Failure erases stale-available admission |
| `wrong-detachedAvailabilityLeak.cfg` | `DetachedUnavailable` | Expose prior data after root removal |
| `wrong-reincarnatedAvailabilityLeak.cfg` | `ReincarnatedUnavailable` | Expose prior data after same-root re-add |

The positive configurations also check types, terminal-state consistency, waiter
subject identity, and eventual terminal settlement. Action properties independently
compare pre/post state for failed-replacement preservation and observation purity.
Publication and preservation predicates inspect actual snapshot/settlement changes,
not self-reported event names. Availability is checked separately from the retained
token: exactness, non-loss, detach exclusion and reincarnation exclusion each have
explicit predicates. Only observation purity uses its named observation event.
They are not tests that simply call an implementation's own predicate.

Liveness assumes weak fairness of each ticket's `Advance` action and of `WaitStep`.
Registered work can start and then terminate, or terminate without starting. Failure,
cancellation and supersession remain permitted terminal outcomes: fairness does not
promise successful indexing. Version/incarnation advances are finite under the
listed bounds. Neither admission nor successful publication is assumed fair.
The model proves no real-time deadline, OS/Tokio fairness, unbounded progress, or
guarantee that permanently failing work eventually succeeds. Current accepted
snapshots are immediately Ready by projection; this is not a performance claim.
Session, trust, watch-set, host-profile and other unmodeled inputs are held fixed.
There is no multi-root isolation, arbitrary document-set or session-restart claim.
TLC's recorded finite-state fingerprint-collision estimates remain part of the
run's limitations; no mathematical hash-collision impossibility is asserted.

## Reproduce with the native tool

Use the official `tla2tools.jar` from TLA+ **1.7.4**, not a moving latest URL:

<https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4>

The downloaded release artifact used for local qualification has SHA-256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.
Its native banner is `TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)`.
The distribution version and native banner are different version labels.
Verify the artifact bytes before execution. Java distribution/version and executable
digest, model/config digests, commands, limits and raw output belong to each run's
evidence; the artifact hash alone is not a tool-soundness proof.

From this directory, using an absolute path to that verified jar:

```text
java -Xmx512m -XX:+UseParallelGC -cp <verified-tla2tools.jar> tlc2.TLC -workers 1 -fp 0 -metadir <fresh-output-directory> -config positive.cfg WorkspaceProtocol
```

Run each wrong configuration with the same command, replacing the config and using
a fresh output directory. Require normal native completion for the positive model,
and the exact property failure named in the table for each negative control.
A parser error, missing config, tool exception, timeout, memory failure, unrelated
property failure, or generic nonzero exit is **not** a successful negative control.
Retain the full counterexample, including the liveness cycle for lost wakeup.
Use an outer wall deadline in addition to the JVM heap bound. The local qualification
budget is 120 seconds per invocation; exceeding it is NOT_PROVEN, not permission to
raise the bound or substitute a smaller successful model.
Run both positive profiles. Give every invocation a private Java temporary directory
with `-Djava.io.tmpdir=<fresh-temp-directory>` as well as its own TLC output directory.

This directory introduces no new verifier launcher, result-admission authority,
formal receipt registry, ordinary-PR workflow or merge gate. Common tool custody,
native result parsing and formal receipts remain with #19315/#19316/#19313; route,
execution and protected-queue consumption remain with #19319/#19320/#19322.
Native model results cannot satisfy those absent joins by themselves.

## Production correspondence and integration ceiling

Source inspection at `1c50353d8e2a7110bc774258da2b6d8a9fde554e` identifies:

* `src/workspace/runtime_generation.rs` and its `core.rs` as root-generation,
  task-registration and publication-eligibility substrate. The public
  `accept_publication` method returns eligibility, not a snapshot-store transaction.
* `src/workspace/workspace_index.rs` as the existing coarse index lifecycle owner.
* `crates/perl-lsp-rs/src/runtime/scheduler.rs` and `serving.rs` as live scheduling
  and construction surfaces that must be traced when implementation binding lands.

Those observations do **not** establish a live constructor/call chain or final
compare-and-publish sink corresponding to this whole model. Exactly-once operation
settlement, pure observations, immutable accepted snapshots and the modeled waiter
protocol still require implementation-facing, mutation-sensitive correspondence
under their existing owners. Wrong-model controls are not wrong-production controls.
Production correspondence, refinement and queue-required acceptance are NOT_PROVEN.

#19147 remains the separate finite **as-is** resource-rejection pilot. This model
does not inherit its production tests or receipts and does not close its lifecycle
successors. Retire or narrow that pilot only when the ticket/snapshot cutover replaces
its live path and an explicitly mapped successor retains its preservation,
nonpublication and partial-routing obligations. Preserve historical evidence.

Rollback removes this model/configuration slice and its narrow file-policy entry;
it changes no product behavior, permissions, release surface or required checks.
