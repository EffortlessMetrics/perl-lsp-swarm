# Proposed EIR entry and runtime boundary

Status: proposed; not an accepted ADR and not implementation authorization.
Decision owner: #17510. Programme: #17509 / #2559.
Source baseline: `2f33ac783b5bb0da7c8bc12644bc47d565c21c7c` (2026-10-10).

## Decision requested

Approve a separate, profile-bounded execution model and reference runtime, with
one explicit adapter from the canonical compiler facts. Keep PIR-A an analysis
representation and ordinary editor requests unable to execute project Perl.

This proposal prepares the dedicated decision required by
[ADR-0005](../adr/PLSP-ADR-0005-hir-body-pir-eir-boundaries.md). It does not amend
that accepted decision by its presence. Before implementation starts, #17510
must obtain architecture acceptance, record the adopted entry conditions and
resolve their exact evidence. Open conditions below are not a readiness pass.
The draft [SPEC-0035](../specs/PLSP-SPEC-0035-executable-profile-and-eir.md) remains
a design input, not an already implemented runtime contract.

## Why the current graph is not executable input

The inspected [body model](../../crates/perl-parser-core/src/hir/body.rs) has typed
variables, operators, control regions, calls and several specialized expressions,
but no typed scalar-literal leaf. Its `Assign` carries `lhs`, `rhs` and a coarse
`AssignMode`; the complete operator handoff therefore needs an explicit audit,
not an assumption that this node alone is sufficient.

The [PIR model](../../crates/perl-parser-core/src/pir/model.rs) has
`Literal { kind: PirLiteralKind }`, without the literal value on that operation.
Flat-HIR facts can retain literal payloads. The gap is a qualified body/PIR
handoff, not an assertion that the entire compiler has lost every literal.
\#17538, under #6658, owns the first scalar-literal repair and payload relation.

Source anchoring does not resolve this: a byte range identifies a location, not
its runtime value or evaluation rule. The adapter must not reparse source,
inspect `Opaque` spelling, manufacture constants or execute the analysis graph.

The same body model documents four binding limitations under #14173: an
initializer reading the newly declared rather than outer binding, same-scope
redeclaration visibility, foreach iterator scope, and package-scope descent.
Those exact cases must be fixed or excluded before their executable admission.
The standalone `lower_body` route is not a substitute for production evidence.

## Proposed ownership and dependency direction

Use two package boundaries for the initial execution lane, subject to review of
repository topology at implementation start:

- `perl-eir`: execution model, verifier, canonical runtime operations/state and
  reference evaluator in internal modules. #4775/#4777/#4779, #2413/#2354 and the
  evaluator owners implement their slices here rather than separate per-operation
  crates. This package imports neither parser/HIR/PIR nor LSP/provider types.
- `perl-eir-lowering`: the narrow analysis-to-execution adapter. It depends on
  parser-core's canonical facts and `perl-eir`, not the other way around. #17512
  owns general lowering here; the first effect vertical remains #2447.

These names are proposals, not additions to the workspace in this change. One
adapter package is justified by the dependency direction; no backend/plugin
framework, duplicate oracle or new readiness store follows from the split.
If the accepted topology can enforce the same dependency boundary within an
existing compiler adapter package, reuse that placement instead.

The explicit offline runner consumes both packages. Keep process supervision
with the existing harness owners, not inside the evaluator. Native backend
placement is decided by #17515 after the semantic ABI is known. Interpreter-only
builds must not require a native code generator.

```text
source -> Parser -> lower_ast -> HirFile::bodies + canonical side facts
       -> verified PIR-A + exact semantic payload relations
       -> explicit adapter -> verified profile-stamped EIR
                              |-> reference evaluator
                              |-> optional native adapter
                                   -> canonical runtime helpers
```

No edge runs from parser-core, semantic providers or ordinary LSP requests into
execution. Dependency tests and an editor-path negative control must prove this
boundary; architecture prose alone cannot do so.

## Execution model proposed for the first slice

An immutable EIR module owns a versioned constant pool, functions, ordered blocks,
explicit operands and typed terminators. Instruction order determines evaluation
order within a block. Branch targets and edge operands determine control flow;
container iteration or HIR traversal order does not.

Start with the exact literals, places, operations and return forms admitted by
\#4773 and #17536. Add branches/loops under #2350 and direct calls under #17513
only when those families have independent reference proof. A type capable of
representing a call or branch does not admit that operation to an earlier profile.

An execution place names canonical storage/alias intent, distinct from a value.
The adapter remaps compiler identities to EIR-local identities and preserves a
same-generation provenance relation. Runtime #2413/#2354 allocate cells and frames
from that verified plan; source positions and variable display names are not
runtime cell identities. Literal tokens preserve reviewed interpretation facts
until canonical runtime construction; Rust parsing/formatting must not silently
define Perl numeric or string semantics.

Keep scalar/list/void value context distinct from truth/definedness demand and
read/write/RMW/alias access. Calls carry exact callee identity and argument plans,
including alias versus temporary ownership and caller return context. Dynamic
callee lookup, methods, closures, XS and module loading remain separate families.

### Verification and admission

Only the verifier can construct the public verified-module wrapper. It checks
schema/profile identity, complete operand/payload references, function and block
ownership, defined-before-use/control-flow rules, place/access consistency,
terminators, call closure and admitted capabilities. Future exceptional edges
must be explicit before exception handling is admitted. Unknown or malformed
operations cannot become an empty successful programme.

Bind the module to source and compiler generation, lowering/profile versions and
all load-bearing dependency facts. Validate external entry/input/context/state
limits separately before execution. Do not deserialize a purported verified bit
and bypass verification. A native artifact additionally binds ABI, backend,
target, CPU features, codegen options and dependency generations.

Source wrappers may supply inputs and observe results, but cannot replace the
subject computation. Loading only a selected entry requires a reviewed policy
for the rest of the source unit; silently dropping unsupported bodies is not one.

## Values, state, failure and code lifetime

\#2413 owns scalar/aggregate construction, coercion and alias-visible cells.
\#2354 owns frame/pad/stash/localization/handler lifetime. Both reference and native
execution use these semantic operations rather than maintaining parallel value
models. Begin with bounded runtime-owned handles and non-moving storage; a moving
collector, concurrent interpreter or unsafe `Send`/`Sync` assertion is not needed
for the first execution proof.

Handles validate owner/generation and remain rooted across allocating helpers.
Bound steps, allocations/bytes, frames, aggregate depth, output and trace size.
Budgets count semantic work under a shared reference/native policy, not machine
instruction count. Check loop backedges and other bounded safe points once those
families are admitted. A native compiler timeout is a separate host-containment
question, not a promise that a cooperative flag can stop arbitrary compilation.

Resolve #17534's commit/cleanup contract as follows, subject to review:

| Event | Required treatment |
|---|---|
| Admission fails before execution | No programme effects or half-published frame |
| Completed ordinary store precedes a language exception | Preserve the language-defined committed effect |
| Scope with `local` exits | Restore the localized binding/value exactly once |
| Frame or temporary lifetime ends | Release its owned resources without undoing unrelated committed stores |
| Budget/cancellation stops execution | Typed host/profile outcome with explicit observable-state and session-reuse policy |
| Native compilation refuses before entry | Reference fallback only for already EIR-supported code |
| Native side exit follows effects | Resume exact state or propagate failure; never replay the entire function |
| Internal invariant/panic failure | Terminal internal failure, not a successful fallback or Perl exception |

Thus `no partial writes` protects operation admission and coherent state, not a
fiction that Perl exceptions roll back an entire evaluation. PR #17537 supplies
stock-Perl counterexamples distinguishing ordinary writes from localization.
It does not implement runtime transactions or prove the Rust/native cleanup path.

Native #17516 owns the reviewed fixed-layout ABI and typed status returns;
\#17517 owns closed imports, write/execute policy, finalization, callable handles,
retirement and quiescent reclamation. Active/retained calls pin code and required
runtime dependencies. No unowned function pointer is a safe API. In-process native
execution is not a security sandbox, even when it uses these controls.

## Entry conditions and evidence disposition

This table is a review packet, not a replacement for #6657/#6689 or an executable
profile registry. Recheck source and evidence when each family is admitted.

| ADR-0005 concern | Current evidence / missing proposition | Owner and proposed disposition |
|---|---|---|
| Canonical body representation | Literal leaf absent; two body routes need shared construction/parity | #17538 / #6658 / #5813; unresolved for ordinary literal execution |
| Binding identity | Four limitations documented in the current model | #14173 / #6659; fix or explicitly exclude affected forms |
| Branch/loop/return/read lowering | Typed source structures exist; this review did not run complete production-path proofs | #2017 / #2277 / #2350; require exact family evidence, not node counts |
| Context and access | Types are not proof of every projection/call/place composition | #2275 / #5242 / #17513; qualify selected rows |
| Literal/operator payload | Flat facts and category-only PIR do not prove the full handoff | #17538 / #6658 / #17512; typed same-generation relation required |
| Real-workspace provider stability | No new qualifying provider receipt established by this work | Remains unresolved under the accepted ADR; proposed amendment below |
| Dedicated EIR decision | This document is a proposal only | #17510 must obtain accepted architecture and publish exact entry disposition |
| Profile/unsupported contracts | Required independently of architecture acceptance | #4770 / #4773; no default-admit instruction families |
| Reference/native proof | Stock-Perl seed exists in PR #17537; EIR/native execution remains absent | #17514 / #17522 / #17535; mechanism labels stay separate |

### Proposed narrow amendment, not an enacted waiver

For an explicitly offline runtime family, allow profile-scoped canonical frontend
and verifier proof to satisfy the semantic start condition without waiting for an
unrelated provider cutover. Require dependency/reachability proof that execution
cannot affect ordinary editor requests, and retain real-workspace evidence for
any provider promotion or editor-visible change. This is a proposed replacement
for the provider-receipt prerequisite **only for the isolated execution lane**.

Review must accept this amendment explicitly in the dedicated ADR and reconcile
ADR-0005's text. Until then the existing provider-receipt gate remains in force.
If rejected, name the exact outstanding provider receipt and its owner; do not
reinterpret the old condition as already satisfied. Either decision still needs
the selected frontend families' real proof and #4770/#4773 contracts.

## Execution order and claim limits

The #2447 PackageSubTable first-effect vertical and its original-source migration
remain separately owned. Sharing EIR foundations does not replace that accepted
cut line or make wrapper-generated TAP into original-source execution evidence.

Prepare #17538's frontend slice, #17536's attributed workload/admission matrix and
this architecture review now. After accepted entry, implement the existing EIR,
value/state and reference-expression leaves. Native scalar lowering may begin
once its pure family is qualified; it need not wait for all loops/calls/closures.
Advance source/reference integration, native control/calls, then function-entry
JIT and multi-mode qualification through #17509's existing train.

\#17533 measures whether call-count hotness reaches useful work. OSR remains an
optional measured successor, not a concealed first-JIT requirement. #17527 owns
cold/warm/compile/memory/break-even proof; machine code existence is not a speedup.
\#17535 extends the existing result axes rather than adding another evidence store.

## Review, alternatives and rollback

Reject direct interpretation of PIR-A: an analysis boundary or unresolved fact is
not an executable instruction. Reject a second AST-walking interpreter, a parallel
scalar model in the backend, one crate per operation and hidden stock-Perl runtime
fallback. A single package containing parser and evaluator would reduce package
count but weaken the enforceable dependency boundary; topology review may choose
it only with an equally concrete separation mechanism.

Before acceptance, independently review the dependency direction, payload and
binding evidence, profile exclusions, operation commit points and native lifetime
handoff. Missing proof must stay missing. Move only the accepted decision into the
canonical ADR/spec/entry owners; do not mark #17510 complete merely by merging an
unaccepted proposal.

Rollback of this proposed document is documentary. Later implementations must
retain interpreter-only operation, reject invalid artifacts before entry and
retire native code without freeing active frames. No feature flag, EIR type,
workspace member, provider change or executable-runtime claim is added here.
