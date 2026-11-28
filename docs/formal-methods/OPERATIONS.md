# Formal-methods operations

This runbook describes how formal claims are expected to be executed, inspected, triaged, and replayed. It distinguishes current repository behavior from the command surface planned by #19308–#19310.

## Current executable state

As of the programme baseline recorded by #19296:

- `main` has no qualified repository-wide formal runner or generated current-status page;
- no TLC, Kani, or Loom claim has a current production-bound formal `PASS`;
- PR #19305 is the active claim-inventory candidate and is not proof execution;
- #19147 contains an existing workspace TLA+/TLC source candidate, but native TLC, all process-owner controls, and current production correspondence are not qualified;
- ordinary unit, property, fuzz, Miri, oracle, process, installed, and receipt checks retain their existing commands and meanings.

Do not copy planned commands from this document into required automation until their owning issue has landed and the command is present on `main`.

## Planned command family

> **PLANNED — not yet available on `main`.** Exact spelling may change while #19308–#19310 are implemented.

```bash
cargo xtask formal claims check
cargo xtask formal tool verify
cargo xtask formal route --base <sha> --head <sha> --profile smoke --check
cargo xtask formal run --claim <id> --profile smoke
cargo xtask formal receipt validate <path>
cargo xtask formal status --check
```

The accepted implementation must expose one coherent command family. Independent scripts with different result semantics are not a supported interface.

## Preconditions for a native run

Before running a claim:

1. Start from the exact source subject named by the route/claim.
2. Record whether the tree is clean, dirty, or unknown. Dirty evaluated content must be bound directly; it cannot inherit clean `HEAD` identity.
3. Verify the claim registry, schema, tool manifest, closure algorithm, and route receipt.
4. Verify the immutable native tool, runtime, solver/compiler, target, features, and cfg.
5. Confirm the host has every required capability. Otherwise route to a capable admitted executor or retain `UNSUPPORTED_HOST`.
6. Confirm the positive and control denominators are nonzero and match the claim/profile.
7. Use an owned isolated work/output directory.
8. Record time, memory, disk, process, and artifact ceilings.
9. Ensure previous output/caches cannot be mistaken for current evidence.
10. Do not change bounds or assumptions after seeing a failure without creating a new claim/profile identity.

## Expected run outputs

A selected claim should produce:

```text
route / selection receipt
verified tool and environment identity
native stdout and stderr
structured native report, where supported
state / property / execution summaries
counterexample or replay packet, where applicable
process-settlement result
runtime and resource measurements
formal proof receipt
freshness and correspondence evaluation
generated current-status update
```

Artifact names should include claim ID, profile, and receipt identity. Missing required artifacts or upload failure is a typed evidence/publication failure, not a native pass.

## Interpreting outcomes

### `PASS`

Use only when:

- every expected positive obligation ran and passed;
- every required wrong-model/harness control produced its expected failure;
- every required wrong-production control/correspondence obligation passed;
- reachability/cover and unwind/exploration conditions are complete;
- the exact tool/process/artifact identities are valid;
- process descendants settled;
- the final receipt is current for the exact semantic closure.

The claim applies only to its recorded profile and non-claims.

### `COUNTEREXAMPLE`

A counterexample proves that the bound formal subject violates the stated property. It does not by itself identify a production defect.

Classify the first cause as one of:

```text
property/specification defect
model/harness defect
abstraction/correspondence defect
native tool or adapter defect
execution infrastructure defect
production implementation defect
```

Retain the original evidence before minimizing or changing anything.

### `NOT_PROVEN`

Common reasons:

```text
zero selected work
missing positive/control obligation
skip or filtered harness
unwind/recursion shortfall
state/branch/exploration exhaustion
timeout or resource limit
unsupported verifier construct
native tool crash or unknown output
malformed/truncated/missing report
process descendant leak
missing raw artifact
stale or incomplete semantic closure
unproved production correspondence
```

Do not restate `NOT_PROVEN` as “probably okay,” “infrastructure-only pass,” or “source reviewed.”

### `UNSUPPORTED_HOST`

Record the missing capability and the required executor class. Do not substitute a different target, tool, backend, or architecture silently.

### `STALE`

A prior native result remains historical evidence but cannot satisfy current status. Recompute the semantic closure and rerun the affected claim/profile.

## Counterexample handling

Use this sequence:

1. **Freeze evidence.** Retain raw trace/report, exact source/model/tool/profile identity, and process outcome.
2. **Reproduce.** Replay with the same immutable inputs and tool before simplifying.
3. **Check control semantics.** Verify that the failure is not an expected negative control.
4. **Classify attribution.** Property, model, abstraction, adapter, tool, infrastructure, or production.
5. **Minimize carefully.** Preserve the violated proposition and all identity-bearing inputs.
6. **Cross-check production.** Where possible, translate a production-attributed trace into an ordinary deterministic regression.
7. **Route to the accountable owner.** Do not patch a second subsystem or branch silently.
8. **Repair and rerun.** Run the positive claim, all negative controls, the production correspondence control, and complementary evidence.
9. **Preserve history.** Keep the failed receipt/counterexample; publish a new current result rather than rewriting the old one.

Never weaken a property, increase a timeout, reduce a domain, add an assumption, or remove a cover solely to make the counterexample disappear.

## Tool-specific triage

### TLC

Check separately:

- model/config/constants/property/fairness identity;
- parse/evaluation/model errors;
- invariant versus temporal-property failure;
- deadlock policy;
- state and distinct-state counts;
- search depth;
- worker count and liveness mode;
- state/action constraints and symmetry;
- timeout, memory, disk, and state-space ceilings;
- exact counterexample trace;
- whether the production correspondence, rather than the abstract model, failed.

A safety run does not satisfy a liveness claim.

### Kani

Check separately:

- exact selected harness count;
- property/assertion outcomes;
- `cover`/reachability denominator and positive satisfaction;
- unwinding and recursion checks;
- assumptions and stubs;
- target/features/cfg;
- unsupported/undetermined properties;
- Kani, CBMC, embedded compiler, and structured-report identity;
- source/report identity;
- wrong-production control.

Native success text with absent, zero, unreachable, unsatisfied, or inconsistent required cover evidence is `NOT_PROVEN`.

### Loom

Check separately:

- selected model/test denominator;
- shared production/model source identity;
- thread/value domain;
- branch/preemption/permutation/pruning/checkpoint settings;
- unexpected panic or deadlock;
- maximum-branches or search exhaustion;
- timeout/resource ceiling;
- primitive substitution and unsupported runtime behavior;
- wrong-interleaving and wrong-production controls.

A bounded Loom run does not prove arbitrary Tokio scheduling, OS timing, fairness, or every unbounded execution.

## Incident procedures

### Zero selected obligations

- Stop the run.
- Inspect route, claim ID, harness filter, feature/cfg, deleted/renamed paths, and denominator.
- Treat a formal-source or implementation-closure change with zero selected claims as a routing failure.
- Do not emit an empty success receipt.

### Tool identity mismatch

- Stop before execution if possible.
- Record expected and observed tool/runtime/solver/compiler identities.
- Do not trust an ambient PATH executable or cache entry.
- Reinstall from the immutable manifest or retain `NOT_PROVEN`.

### Timeout or resource exhaustion

- Preserve partial raw evidence and process-settlement result.
- Do not classify as property pass or counterexample unless the native tool produced a complete named violation before termination and the adapter can prove that fact.
- Investigate state-space/domain shape and performance.
- Budget increases require measured justification and a new bound identity.

### Native output changed or is unknown

- Preserve the unparsed output.
- Fail closed.
- Add retained old/new captures and parser controls under #19309.
- Review release notes and semantic changes before updating the adapter.

### Expected negative control passes

- Treat the claim as `NOT_PROVEN` even when the positive subject passes.
- Determine whether the property, control, selection, adapter, or tool changed.
- Do not delete or quarantine the control to restore green.

### Production mutation is not detected

- Downgrade correspondence.
- Trace the live consumer path and final enforcement boundary.
- Check whether the proof covers only a duplicate or bypassed substrate.
- Do not retain `live_consumer_bound` status.

### Artifact upload fails

- Preserve local evidence where policy permits.
- Record native result and publication/evidence failure separately.
- A missing required artifact cannot satisfy required integration.

### Generated status disagrees with receipts

- Stop publication.
- Recompute against exact registry, route, receipt, and semantic closure.
- Treat current status as invalid until the projection is deterministic and checked.

## Reproducing a reported result

A reproduction packet must include:

- claim/profile ID;
- source commit/tree and semantic closure identity;
- clean/dirty evaluated-source identity;
- immutable tool/runtime/solver/compiler identity;
- target/features/cfg/environment;
- exact command/argv and working directory;
- bounds and resource ceilings;
- positive/control denominator;
- raw artifacts and expected result;
- counterexample/replay input where applicable.

A reproduction that changes any identity-bearing input is a new run, not confirmation of the old result.

## Cleanup

Formal tools may create large outputs. Cleanup must be scoped to the run-owned directory or repository-declared cache owner.

- Never delete worktrees, shared Cargo targets, verifier caches, or evidence from a name/age guess.
- Never delete a counterexample or failed receipt because a later run passed.
- Verify process settlement before removing an output directory.
- Retain the artifacts required by repository policy and release audit.
- Use existing repository storage/reclamation authorities rather than adding a proof-specific destructive cleaner.

## Activation posture

Formal execution becomes required in phases:

1. local commands and deterministic fixtures;
2. advisory/shadow execution with explicit non-authority;
3. exact selected queue execution;
4. required consumption after result, staleness, artifact, and rollback controls pass;
5. scheduled/release full profiles.

A green advisory workflow, source marker, or command presence is not required integration.