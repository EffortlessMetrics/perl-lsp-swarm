# Formal-methods maintenance

Formal verification remains useful only when claims, tools, production bindings, receipts, and documentation stay current. This document defines the maintenance contract owned by #19311 and the execution infrastructure in #19308–#19310.

## Current candidate tool baseline

> **Planned candidates, not yet machine-qualified on `main`.** #19309 owns immutable installation and compatibility qualification.

| Component | Candidate identity | Maintenance note |
| --- | --- | --- |
| Product Rust compiler | Rust 1.95 | Repository `rust-version`; proof tools may embed or require another compiler, which must be recorded separately. |
| TLA+ tools / TLC | 1.7.4 | Bind exact `tla2tools.jar`, Java runtime/distribution, workers, liveness mode, and checksum. |
| Kani | 0.68.0 | Bind exact Kani, actual CBMC, embedded compiler, target/features/cfg, and structured export. |
| Loom | 0.7.3 | Bind exact locked crate checksum, Rust toolchain, shared model source, and builder/exploration settings. |

Do not automatically update these through a mutable `latest` reference or dependency bot regeneration.

## Event-driven maintenance

Re-evaluate affected claims whenever any of the following changes:

- production kernel, wrapper, constructor, consumer, or final enforcement boundary;
- feature, cfg, target, platform, dependency resolution, build script, proc macro, or generated source;
- model, harness, property, configuration, assumption, stub, abstraction, or negative control;
- symbolic domain, fairness, unwind, recursion, branch, scheduling, resource, or artifact bound;
- claim registry, schema, semantic-closure algorithm, route policy, adapter, or status projection;
- verifier, solver, compiler, runtime, standard-library/model, runner, or host capability;
- ownership, public support claim, architecture decision, retirement, or supersession state;
- release candidate subject.

The impact router should make the previous receipt stale automatically. Human memory is not an invalidation mechanism.

## Periodic maintenance

Use the repository’s existing scheduled governance rather than adding a redundant formal-only workflow.

A scheduled/full audit should check:

1. claim IDs, owner issues, source paths, and implementation bindings still exist;
2. active tool pins and checksums remain available and reproducible;
3. every current receipt still matches its semantic closure;
4. all active required claims run in the full profile on supported hosts/targets;
5. all wrong-model/harness and wrong-production controls still discriminate;
6. counterexamples and `NOT_PROVEN` rows have owners and current next actions;
7. unsupported-host rows have an admitted executor or an explicit limitation;
8. runtime, memory, disk, artifact, and queue costs remain inside budgets;
9. model/harness files and shared kernels are not orphaned or bypassed;
10. unsafe/FFI/platform and generated-code obligations remain reconciled with their canonical owners;
11. generated current status matches the underlying receipts;
12. documentation and command examples match current executable behavior.

A periodic audit may confirm that nothing changed. It may not renew stale evidence without recomputing identity and currentness.

## Tool upgrade procedure

Every verifier, solver, compiler, runtime, or result-format upgrade is a focused change.

1. Open an issue naming old and new exact identities, reason, affected claims, known semantic/output changes, and rollback.
2. Read upstream release notes and primary documentation.
3. Verify immutable downloads, checksums, source revisions, crate lock resolution, and platform mapping.
4. Capture old and new version output and native positive/failure reports.
5. Run adapter parser controls against retained old and new captures.
6. Re-run every infrastructure failure control: zero work, timeout, malformed/unknown output, unsupported host, missing controls, process settlement, and source/report mismatch.
7. Re-run all wrong-model/harness controls.
8. Re-run all wrong-production correspondence controls.
9. Re-run every active claim/profile affected by the tool or trusted model.
10. Compare native outcomes, state/property/execution counts, counterexamples, runtime, memory, disk, and artifact size.
11. Investigate every changed result or material exploration/state-count difference.
12. Update trust boundaries and known limitations.
13. Version receipt, closure, or status semantics when backward interpretation is not proven equivalent.
14. Preserve old receipts and artifacts; mark currentness honestly.
15. Land through normal protected integration with an explicit rollback path.

A tool upgrade is not complete because the new binary installs or a smoke example passes.

## Claim review and ownership

Every active claim has:

- one owning issue;
- one accountable architecture/product owner;
- one method and exact proposition;
- explicit implementation binding;
- smoke/full profiles and resource budgets;
- negative-control owners;
- a retirement or supersession condition;
- an update trigger for its documentation.

When ownership changes, update the claim registry and generated status in the same change. An orphaned current pass becomes `NOT_PROVEN` or `STALE` until ownership and maintenance are restored.

## Freshness policy

A receipt is current only when every identity-bearing semantic input matches.

Changes that normally invalidate a result include:

```text
production closure or live call chain
model/harness/property/configuration/assumptions/controls
bounds, fairness, unwind, recursion, pruning or resource ceilings
tool/solver/compiler/runtime/runner
features/cfg/target/dependency model
generated source or generator inputs
receipt/schema/closure interpretation
```

Unrelated repository changes may leave a receipt current when the versioned closure proves semantic equivalence. Whole-repository SHA inequality alone is not sufficient stale logic, and whole-repository SHA equality cannot hide dirty evaluated content.

`unknown` closure or compatibility is fail-closed.

## Retirement and supersession

Retire a claim when its subject is intentionally removed. Supersede it when a newer claim replaces the old proposition or architecture.

Required steps:

1. record the terminal reason and owning issue;
2. stop selecting the retired claim in current profiles;
3. retain its historical receipts, raw evidence, and counterexamples;
4. remove or archive only run surfaces no longer consumed;
5. update impact routing and generated current status;
6. prove that no live consumer still relies on the retired subject;
7. link the superseding claim where applicable.

Do not keep obsolete production behavior merely to preserve a passing model. #19147, for example, must retire or narrow when the target operation-ticket/snapshot protocol supersedes its current impure state path.

## Counterexample retention

Counterexamples are durable evidence.

- Retain the original raw native trace/report and exact identity.
- Retain minimized reproductions separately from the original.
- Preserve attribution and disposition history.
- Add ordinary regressions for production defects where possible.
- Never overwrite a failed receipt with a later pass.
- Never delete a counterexample because it is embarrassing, old, or expensive to replay.
- Redaction must be explicit and identity-bearing.

Retention duration should cover at least release audit and tool-upgrade comparison for active claims, subject to repository evidence/privacy policy.

## Performance and economics

Measure per claim/profile:

- cold and warm setup time;
- native compile/check/exploration time;
- peak memory and disk;
- artifact and counterexample size;
- cache hit/miss plus cache-verification cost;
- selection frequency and queue occupancy;
- failure/counterexample reproduction cost.

Each profile has an explicit budget and review trigger. A timeout or memory increase requires measured evidence and a new bound identity.

Prefer:

- small, high-discrimination smoke domains;
- split harnesses with attributable propositions;
- shared immutable tool caches with post-restore verification;
- scheduled expansion for expensive domains;
- proof-only dependencies that do not enter release artifacts.

Avoid:

- one opaque monolithic harness;
- all proofs on every PR;
- silently reduced domains to meet a budget;
- repeated verifier installation without identity/caching discipline;
- treating cache success as proof execution.

## Documentation maintenance

Documentation is part of the control plane when it defines claim vocabulary, commands, bounds, support, or status.

Checks should fail when:

- a documented current command does not exist;
- a planned command is presented as current;
- tool versions differ from the machine manifest;
- claim/result/status vocabulary drifts from code/schema;
- a claim ID, owner, or path is missing;
- generated status is hand-edited or stale;
- tests, fuzzing, Miri, or oracle agreement are described as formal proof;
- passing examples omit bounds or limitations;
- the repository, foreign code, or unsupported platform is called verified.

Prefer generated vocabulary/reference tables over duplicated constants.

## Release qualification

A release may claim only the formal properties whose exact release subject has current applicable receipts.

Before release:

1. resolve the final source commit/tree and semantic closures;
2. verify exact tool/runner identities;
3. run required full profiles on every claimed supported host/target;
4. run all required control families;
5. reject stale, foreign-head, copied, missing, duplicate, unsupported, timed-out, or zero-work evidence;
6. verify production correspondence for every live-consumer claim;
7. retain native/raw/counterexample artifacts;
8. generate and check current status;
9. copy only bounded claim language into release notes;
10. keep all remaining rows explicit `NOT_PROVEN`, `UNSUPPORTED_HOST`, or `STALE`.

Formal proof does not replace installed-artifact, platform, protocol, editor, real-process, release-integrity, or public-download acceptance.

## Maintenance checklist

```text
[ ] Owners and source paths current
[ ] Semantic closure complete
[ ] Tool pins immutable and verified
[ ] Positive obligation denominator nonzero
[ ] Wrong-model/harness controls discriminate
[ ] Wrong-production controls discriminate
[ ] Native result and raw artifacts retained
[ ] Process settlement proved
[ ] Freshness current
[ ] Binding/correspondence truthful
[ ] Smoke/full bounds explicit
[ ] Runtime/resource budgets current
[ ] Counterexamples owned and replayable
[ ] Generated status checked
[ ] Docs and commands current
[ ] Retirement/supersession trigger still correct
```

A claim failing this checklist remains visible and non-current; it is not removed from the status denominator.