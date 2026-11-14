# Authoring and reviewing formal claims

Use this guide when adding or materially changing a formal claim, model, harness, abstraction, bound, tool, or production binding.

Formal work starts from a product or architecture property. It does not start from a preferred prover.

## Before opening an implementation PR

1. Find the existing production, architecture, test-topology, receipt, and release owner.
2. Search for an existing claim or equivalent evidence surface.
3. File or update one focused issue with the exact proposition, subject, risks, controls, bounds, validation, and non-goals.
4. Confirm one active writer and one PR-sized change.
5. Decide whether the work is a model, shared substrate, or live production proof.
6. Identify the final mutation, publication, allocation, or linearization boundary that matters to users.
7. State what remains outside the claim.

Do not create a second lifecycle, parser, IR, receipt, unsafe, or CI authority merely because it is convenient for a verifier.

## Property template

Every claim should be reviewable in this form:

```text
Claim ID:
Owner issue:
Method:
Implementation binding:

For every <input/state/execution> in <explicit finite or deductive domain>,
under <assumptions/fairness/stubs/environment>,
<exact production or model subject>
satisfies <named safety/liveness/functional property>.

Bounds:
Trusted components:
Excluded behavior:
Positive obligations:
Wrong-model/harness controls:
Wrong-production controls:
Smoke profile:
Full profile:
Retirement/supersession trigger:
```

The proposition must be falsifiable. Avoid language such as “is safe,” “is correct,” “works for Unicode,” or “is verified” without a quantified subject and domain.

## Choose the method by property shape

### TLA+/TLC

Use finite model checking for protocols and state machines where ordering, cancellation, replacement, publication, fairness, and liveness matter more than implementation arithmetic.

Record:

- variables and state abstraction;
- constants and finite domains;
- initial states and actions;
- safety invariants;
- temporal/liveness properties;
- weak or strong fairness assumptions;
- symmetry sets and why they preserve each property;
- state/action constraints and what they exclude;
- deadlock policy;
- worker count, state-space/resource ceilings, and tool/runtime identity;
- refinement or production-correspondence status.

A bounded model can establish the model property. It does not prove that Rust implements the model unless correspondence is separately established.

### Kani

Use bounded proof for pure or narrowly extracted Rust kernels whose inputs and execution can be represented symbolically.

Record:

- exact production function/closure;
- package, target, features, cfg, and stubs;
- symbolic input representation and alphabet/domain;
- assumptions and why each is part of the public contract rather than a way to exclude failures;
- assertion/property denominator;
- positive `cover` or equivalent reachability obligations;
- unwind and recursion bounds;
- integer/overflow/panic policy;
- unsupported constructs;
- Kani, CBMC, embedded compiler, and result-export identity.

A Kani success is incomplete when required behavior is unreachable, the harness denominator is zero, cover evidence is absent or unsatisfied, unwinding is incomplete, or structured output is malformed.

### Loom

Use interleaving exploration for a small synchronization kernel shared by production and the model build.

Record:

- shared transition/kernel code;
- `std` and Loom primitive substitution boundary;
- modeled threads, values, and operation ordering;
- branch, preemption, permutation, pruning, checkpoint, timeout, and memory settings;
- panic/deadlock expectations;
- omitted Tokio, OS, timing, fairness, or foreign-runtime behavior;
- production wrapper/call-path correspondence.

Do not hand-reimplement a separate toy model that can drift independently from production.

### Deductive proof

Use Verus, TLAPS, or another theorem prover only after the kernel or protocol and its assumptions are stable enough to justify maintenance cost.

Record the theorem, axioms, trusted base, extraction/refinement boundary, and how code changes invalidate the proof.

### Complementary evidence

Property tests, fuzzing, Miri, sanitizers, differential/oracle checks, real-process tests, installed acceptance, static review, and receipts remain separate evidence classes. Formal PRs should preserve or improve them where the changed boundary requires it.

## Production binding

A formal subject is production-bound only when the claim records and tests:

1. the owning public or internal entry point;
2. actual constructor and call path;
3. wrappers, adapters, feature/cfg and target branches;
4. final state mutation, publication, allocation, response, or synchronization boundary;
5. every relevant bypass or alternative consumer;
6. the exact implementation closure used by the proof;
7. an independent wrong-production mutation/control.

A matching type name, source file, public export, test helper, or generated ledger row is not enough.

Where extraction is required for proof performance:

- extract a small pure/shared kernel;
- make production call it;
- keep one source of truth;
- add ordinary wrapper/correspondence tests;
- bind both kernel and wrappers into the semantic closure;
- retain behavior and support tier.

## Bounds and assumptions

Every bound and assumption must answer:

- What real behavior does it exclude?
- Why is that exclusion acceptable for this claim?
- Which profile owns the larger/deeper case?
- What happens when the bound is reached?
- How is the bound identity recorded and made stale on change?

Avoid hidden defaults. Record all of the following where applicable:

```text
input length and alphabet
items/nodes/edges/scopes/depth
threads and values
states and transitions
unwind and recursion
branches/preemptions/permutations
fairness and deadlock policy
time, memory, disk and artifact limits
host, target, features and cfg
foreign-code and environment assumptions
```

Timeout, branch exhaustion, state-space exhaustion, incomplete unwind, or unsupported behavior is `NOT_PROVEN`, not a smaller implicit proof.

## Controls

### Wrong-model or wrong-harness controls

Create one named control per property family. The control must fail for the intended reason and retain a replayable counterexample or assertion.

Examples:

- allow double ticket settlement;
- remove stale-generation publication guard;
- count Unicode scalar values instead of UTF-16 units;
- omit a wakeup recheck;
- allow a dangling IR edge;
- drop a framing size check.

The runner rejects a missing control, zero selected controls, or an unexpectedly passing control.

### Wrong-production controls

Independently mutate or fixture the real implementation/call path while leaving the formal subject/property fixed. Examples:

- bypass the production generation check;
- shift a converted edit endpoint;
- publish acceptance before the complete session value;
- route an LSP reader around the shared framing kernel;
- emit a fact referring to a missing IR entity.

A wrong model failing does not establish production sensitivity. A production test failing does not establish that the formal property is discriminating.

## Counterexample-ready design

Before the first positive run, decide how a counterexample will be retained and replayed:

- raw native trace/report;
- exact source/model/harness/tool/profile identity;
- deterministic replay command;
- reduced fixture or ordinary regression path;
- attribution categories: property, model, abstraction, adapter, tool, infrastructure, or product;
- owner and terminal disposition.

Never tune bounds or assumptions solely to remove a counterexample.

## PR boundaries

Prefer these focused boundaries:

1. shared validator/kernel or representation repair;
2. formal model/harness and controls;
3. production correspondence;
4. generic adapter or routing infrastructure;
5. execution authority activation;
6. documentation/status updates.

A single PR may combine adjacent steps only when the diff remains reviewable and no active owner is duplicated.

Do not mix broad product feature expansion into a proof PR merely to increase the number of proved cases.

## PR evidence template

```markdown
## Claim

- Claim ID:
- Owner:
- Method:
- Binding:
- Profile and exact bounds:
- Explicit non-claims:

## Implementation closure

- Production kernel:
- Wrappers/consumers:
- Final enforcement boundary:
- Manifests/features/cfg/target:
- Model/harness/config/assumptions/controls:
- Tool/compiler/solver/runner:

## Native evidence

- Exact source/tree/closure identity:
- Positive obligation denominator and result:
- Wrong-model/harness controls:
- Wrong-production controls:
- Raw artifacts/counterexamples:
- Runtime and resource use:

## Complementary evidence

- Unit/property/fuzz/Miri/oracle/process/installed checks:

## Currentness and rollout

- Receipt/freshness:
- Route/authority:
- Generated status change:
- Retirement/rollback:
```

## Review checklist

Review the following independently:

### Proposition

- Is the property useful and precise?
- Are safety, liveness, functional, reachability, and performance claims separated?
- Are non-claims explicit?
- Does the property accidentally require invalid behavior, such as all CFG nodes being reachable?

### Model or harness fidelity

- Do abstractions preserve the property?
- Are assumptions weaker than the property itself?
- Is the checker proving production code or a duplicate?
- Are fairness, symmetry, constraints, stubs, and recovery boundaries justified?

### Production binding

- Does the real consumer use the subject?
- Are wrappers, features, cfg, target branches, and bypasses included?
- Is the final state-changing boundary identified?
- Does the wrong-production control fail?

### Result admission

- Is the exact tool verified?
- Is the obligation denominator fixed and nonzero?
- Are reachability/cover and unwind/exploration results complete?
- Are timeout, unknown output, missing artifacts, unsupported host, and process settlement fail-closed?
- Are raw evidence and counterexamples retained?

### Currentness and maintenance

- Does the receipt bind every semantic input?
- Will relevant changes make it stale?
- Are smoke/full profiles and budgets explicit?
- Are current status, docs, ownership, retirement, and rollback updated?

One green native run is not substantive review.

## Common rejection cases

Reject or re-scope work that:

- says “formally verified” without a bounded property;
- proves only a hand-copied model but claims production correctness;
- uses `assume` to remove the failing public case;
- accepts exit zero or success text without structured result admission;
- has zero selected harnesses or controls;
- omits positive reachability/cover evidence;
- uses a mutable `latest` tool or ambient executable;
- treats unsupported host as pass;
- adds a second receipt, unsafe, lifecycle, parser, or merge authority;
- runs every proof on every PR without impact evidence;
- hides one counterexample behind an aggregate score;
- rewrites or deletes historical failed evidence;
- changes product support claims based solely on a proof harness.