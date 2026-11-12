# Formal-methods architecture

This document defines the architecture that turns a model or proof harness into a current, bounded, reviewable repository claim. It describes the target system owned by #19297 and its child issues. Components not yet landed on `main` are marked **planned**.

## Design principles

1. **Property-level claims, never repository-level certification.** Each claim names one proposition, one owner, one method, one subject, explicit assumptions, and explicit non-claims.
2. **Production binding is separate from proof success.** A valid model can pass while production does something else.
3. **Every semantic input is identity-bearing.** A result is reusable only when its versioned semantic closure is unchanged.
4. **Native result admission is fail-closed.** Exit zero, familiar text, an evidence envelope, or a cached artifact cannot mint `PASS` by itself.
5. **Wrong-model and wrong-production controls are independent.** Each proves a different part of the evidence chain.
6. **Existing integration authority remains authoritative.** Formal verification consumes the repository’s test topology and queue; it does not create a parallel merge system.
7. **Historical evidence is immutable.** Current status may become stale, retired, or superseded, but prior receipts and counterexamples remain available.
8. **No scalar assurance score.** One high-consequence counterexample or stale claim cannot be averaged away by many passing low-risk rows.

## Independent state dimensions

A formal claim is not represented by one status flag.

```text
method class
implementation binding
rollout / execution authority
native result
freshness
production correspondence
profile / bounds
```

### Method class

```text
model_checked
bounded_proved
interleaving_checked
deductively_proved
```

The method describes what the verifier established, not what production consumes.

### Implementation binding

```text
model_only
substrate_only
live_consumer_bound
```

- `model_only`: the claim concerns a specification or abstract model with no current production correspondence.
- `substrate_only`: production and proof share a kernel or validator, but final consumer reachability is not established.
- `live_consumer_bound`: the receipt binds the real production constructor/call path and final mutation, publication, allocation, or linearization boundary.

A file path, public export, generated inventory row, or test-only constructor is not enough for `live_consumer_bound`.

### Rollout / execution authority

At minimum, distinguish:

```text
planned
runner_available
routed_advisory
selected_integration
required_consumption
retired
```

Source-marker presence or workflow-file existence proves neither selection nor required consumption.

### Native result

```text
pass
counterexample
not_proven
unsupported_host
```

Typed `not_proven` reasons include zero work, timeout, resource exhaustion, missing controls, malformed or unknown output, unsupported verifier constructs, process leaks, missing artifacts, and incomplete correspondence.

### Freshness

```text
current
stale
unknown
retired
superseded
```

Freshness is computed from the semantic closure, not from wall-clock age or a human assertion that an old run still applies.

### Correspondence

Correspondence records whether a production mutation/control demonstrates that the proof is sensitive to the real implementation. It remains distinct from proof-only malformed-model controls.

## Evidence flow

```text
changed source
  → impact and dependency routing
  → exact claim/profile selection
  → immutable tool and environment verification
  → native verifier execution under process custody
  → retained raw observations and counterexamples
  → tool-specific typed observations
  → receipt and semantic-closure validation
  → native result, freshness, and correspondence evaluation
  → exact selected integration aggregation
  → generated current-status projection
```

No later stage may repair or reinterpret missing earlier evidence as success.

## Claim registry

PR #19305 is the active first registry candidate. The accepted registry must eventually record, for each claim/profile:

- stable claim ID and owning issue;
- exact proposition and explicit non-claims;
- method and implementation-binding class;
- production closure and impact paths;
- model, harness, property, configuration, assumption, and control paths;
- exact tool/runner requirements;
- bounds, fairness, unwind, recursion, scheduler, target, feature, and resource assumptions;
- positive and negative obligation denominators;
- smoke/full profile membership;
- execution authority;
- retirement and supersession conditions.

The registry is the denominator, not proof. A valid registry row can still be `NOT_PROVEN`.

## Semantic implementation closure

#19308 owns the versioned closure algorithm. The closure includes every input that can change the proved semantics, where applicable:

- production kernel, wrappers, constructors, consumers, and final enforcement boundary;
- module inclusion and generated code;
- Cargo manifests, lock resolution, build scripts, proc macros, features, cfg, target, and dependency models;
- model, harness, properties, configuration, assumptions, stubs, abstractions, and controls;
- symbolic generators and bounds;
- verifier, solver, compiler, standard-library/model, runtime, and runner identity;
- environment assumptions that affect semantics.

The repository commit and tree record provenance, but unrelated repository movement alone must not invalidate an unchanged semantic closure. Conversely, matching commit metadata cannot hide dirty or substituted evaluated content.

Unknown or incomplete closure fails closed.

## Receipts

#19308 owns a formal-proof receipt registered through the existing `.ci/receipts/registry.toml` authority. A receipt binds:

- schema and closure-algorithm versions;
- claim, method, binding, profile, and owner;
- source commit/tree and clean/dirty/unknown state;
- semantic closure members and digest;
- exact tool, solver, compiler, runner, target, features, and dependency identity;
- all proof bounds and resource ceilings;
- selected positive and control obligations with fixed nonzero denominators;
- raw stdout, stderr, reports, traces, counterexamples, and artifact identities;
- native observations and final typed result;
- production-correspondence evidence;
- retirement/supersession metadata.

The adapter re-derives result meaning from bound raw evidence. A supplied receipt cannot self-assert `PASS`.

## Native tool adapters

#19309 owns common process custody and tool-specific result admission.

### TLC

The adapter binds the model, configuration, constants, invariants, temporal properties, fairness, constraints, deadlock policy, workers, state-space limits, Java runtime, and exact `tla2tools.jar`. Safety-only completion cannot satisfy a liveness claim. Every required wrong-model configuration must produce its named violation.

### Kani

The adapter binds Kani, CBMC, embedded Rust compiler, target/features/cfg, selected harnesses, stubs, assumptions, unwind/recursion, and structured export. Native success text is insufficient: expected harnesses, properties, unwind checks, and required positive cover/reachability evidence must be complete and internally consistent.

### Loom

The adapter binds the exact shared production/model source, Loom and Rust versions, thread/value domain, branch/preemption/permutation/pruning settings, checkpoint behavior, and resource ceilings. Branch exhaustion or pruning-limit termination is not success unless the declared claim explicitly treats it as a non-pass bound.

## Controls

Every formal claim has both control families.

### Wrong model or harness

Examples:

- permit stale publication in a workspace model;
- count Unicode scalars instead of UTF-16 code units;
- omit a generation check in a Loom abstraction;
- create a dangling PIR edge.

These demonstrate that the verifier and property can reject a known-bad formal subject.

### Wrong production

A controlled production mutation or fixture must break the correspondence obligation while the model/harness remains unchanged. This demonstrates that the proof is bound to the real implementation rather than only to an internally consistent duplicate.

Neither family substitutes for the other.

## Impact routing and profiles

#19310 owns deterministic selection.

- Unknown or incomplete impact selects rather than skips.
- A formal-path or implementation-closure change that selects zero claims fails.
- Renames and deletions remain visible.
- Shared adapter, schema, tool, or closure changes widen to all affected claims.
- Unsupported hosts route to an admitted capable executor or remain non-pass.

`formal-smoke` uses small high-discrimination bounds for impacted integration work. `formal-full` runs all active required claims at larger explicit bounds for scheduled or release qualification. A smoke pass never implies the full profile.

## Current-status projection

The planned generated status shows one row per claim/profile with:

- method;
- binding;
- rollout authority;
- latest applicable native result;
- freshness;
- correspondence;
- bounds and host/target support;
- source/closure/tool/receipt identity;
- limitations and next missing obligation;
- retirement/supersession state.

It never aggregates these rows into a repository percentage or “verified” badge.

## Trust boundaries

Formal results remain bounded by trusted components and abstractions:

- verifier, solver, compiler, runtime, standard-library/model, and runner implementation;
- correctness of trusted stubs and abstractions;
- adequacy of the stated property;
- completeness of production binding;
- declared host, OS, target, foreign-code, and environment assumptions;
- finite state, symbolic, unwind, recursion, branch, or scheduling bounds.

A proof of a Rust wrapper does not verify Perl, tree-sitter C code, the OS, an editor, a network service, or another foreign implementation unless that boundary is explicitly modeled and included.

Miri, sanitizers, integration tests, real Perl oracles, protocol tests, and installed acceptance retain their own authority at those boundaries.

## Ownership and landing order

```text
#19305 claim inventory
  → #19308 receipts, closure, result, freshness
  → #19309 native tool adapters
  → first TLC and Kani pilots
  → #19310 routing, artifacts, status, queue integration
  → wider domain proof qualification
```

Documentation under #19311 lands alongside those slices. It must label planned commands and behavior until the corresponding implementation is present on `main`.