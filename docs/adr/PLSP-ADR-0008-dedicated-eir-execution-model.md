# PLSP-ADR-0008: Dedicated EIR execution model and offline entry

Status: proposed; acceptance is maintainer merge of the candidate PR, not the
existence of this file.
Date: 2026-10-12
Owner: #17510 (dedicated EIR architecture decision)
Linked proposal: [PLSP-PROP-0004](../proposals/PLSP-PROP-0004-eir-entry-and-runtime-boundary.md)
Linked specs:
- [PLSP-SPEC-0035](../specs/PLSP-SPEC-0035-executable-profile-and-eir.md)
- [PLSP-SPEC-0025](../specs/PLSP-SPEC-0025-pir-v0.md)
Linked ADR: [PLSP-ADR-0005](PLSP-ADR-0005-hir-body-pir-eir-boundaries.md)
  (reconciled, not weakened, by this decision)
Linked issues: #17509 (programme), #4770, #4773, #4775, #4777, #4779,
  #17512, #17514, #17534, #17535, #7422

## Context

[PLSP-ADR-0005](PLSP-ADR-0005-hir-body-pir-eir-boundaries.md) forbids any EIR
crate, type, lowering, or EIR-shaped PR until a dedicated EIR ADR is
accepted, and requires that ADR to name the crate, data model, dependency
contract, and claim limits. Its "Preconditions for EIR Start" list names only
documented PIR-A coverage plus this ADR; its branch-condition prose
additionally demands a real-workspace provider receipt proving tooling
substrate stability. Those two statements are in tension for an execution
lane that provably cannot affect editor requests.

[PLSP-PROP-0004](../proposals/PLSP-PROP-0004-eir-entry-and-runtime-boundary.md)
supplies the reviewed starting shape and a narrow amendment for the isolated
offline lane. The #17510 review rechecked every entry-condition row against
current main and returned: adopt the amendment for the offline lane, keep
readiness closed until the outstanding contracts and family proofs land.
Point-in-time row evidence lives in the #17510 review comments, not here.

## Decision

### 1. Offline-lane amendment to the provider-receipt prerequisite

For an explicitly offline execution lane, profile-scoped canonical frontend
and verifier proof satisfies the semantic start condition without waiting
for an unrelated provider cutover. The lane must carry
dependency/reachability proof that execution cannot affect ordinary editor
requests. Real-workspace evidence remains mandatory for any provider
promotion or editor-visible change.

This replaces the provider-receipt prerequisite **only for the isolated
execution lane**. It does not waive documented family coverage, the
#4770/#4773 contracts, or verifier scope. Until this ADR is accepted, the
existing provider-receipt gate remains in force everywhere.

A second, narrower scoping amendment: ADR-0005's "EIR work" start
precondition is scoped to lowering and evaluation work. Type-and-
construction-only work (#4775: model types plus total constructors, no
evaluation, no lowering) is admitted once this ADR is accepted and
#4770/#4773 land, because inert types carry no lowering semantics and
demanding lowering coverage before the model inverts the build order.
Every lowering and evaluation slice still needs its family's documented
coverage at admission. This is an explicit amendment of the start
precondition for type-only work, not a silent bypass.

Falsifiers: a qualifying provider receipt surfacing (moots the amendment);
any execution effect on ordinary editor requests (voids the lane's
isolation premise); any attempt to stretch this amendment to provider
cutover (out of scope by construction).

### 2. Crate placement

- `perl-eir`: execution model, verifier, canonical runtime operations/state,
  and reference evaluator in internal modules. Implements #4775/#4777/#4779,
  #2413/#2354, and evaluator slices. Imports neither parser/HIR/PIR nor
  LSP/provider types.
- `perl-eir-lowering`: the narrow analysis-to-execution adapter. Depends on
  parser-core canonical facts and `perl-eir`, never the reverse. #17512 owns
  general lowering here.

These names are adopted, not merely proposed. If implementation-start
topology review finds an existing compiler adapter package that enforces
the same one-way boundary, it may host the adapter instead, with the
substitution recorded. No backend/plugin framework, duplicate oracle, or
readiness store follows from the split. Interpreter-only builds must not
require a native code generator; native placement is decided by #17515.

### 3. Data model

The #4775 type sketch is canonized under SPEC-0035 C4-C6 with these
bindings:

- Versioned module envelope: EIR schema version, profile stamp/reference,
  source/compiler generation binding. Forward-versioned and canonicalizable.
- Stable handle identities: `EirFunctionId`, `EirBlockId`, `EirInstrId`,
  `ValueId`, `CellId`, optional frame IDs. Public runtime state uses stable
  handles, never `Rc<RefCell<...>>`. Runtime value identity is not a source
  range; source positions and display names are never cell identities.
- `EirFunction` with one entry block; `EirBasicBlock` with ordered
  instructions and exactly one terminator; instruction order determines
  evaluation order; branch targets and edge operands determine control flow.
- `EirTerminator`: fallthrough, branch, return, throw/die, loop transfer,
  unsupported boundary. Exceptional edges stay explicit; unknown or
  malformed operations cannot become an empty successful programme.
- Argument/alias plans represent multiple names referring to one `CellId`,
  with alias-versus-temporary ownership and caller return context carried
  exactly. Scalar/list/void context stays distinct from truth/definedness
  demand and read/write/RMW/alias access.
- `UnsupportedBoundary` carries a stable #4770 ID, profile identity,
  provenance when available, reason/claim boundary, and stop-versus-partial
  disposition. No fabricated fallback values.

Scalar coercion/value families may land after the model, but the public
shape above is fixed at acceptance.

### 4. Dependency contract

- PIR-A never imports EIR types. EIR never reuses `PirOperation` or PIR node
  IDs as its runtime instruction contract. PIR-A → EIR conversion lives
  behind the explicit lowering interface only.
- The adapter remaps compiler identities to EIR-local identities preserving
  a same-generation provenance relation. It must not reparse source, inspect
  `Opaque` spelling, manufacture constants, or execute the analysis graph.
- Literal tokens preserve reviewed interpretation facts until canonical
  runtime construction; Rust parsing/formatting must not silently define
  Perl numeric or string semantics.
- `provider_behavior_changed = false` holds for the lane; dependency tests
  plus the #7422 editor-path negative control prove the boundary.
  Architecture prose alone is not proof.
- Only the verifier constructs the public verified-module wrapper
  (schema/profile identity, complete references, ownership,
  defined-before-use/CFG, place/access consistency, terminators, call
  closure, admitted capabilities). Never deserialize a purported
  verified bit and skip verification. Module entry binds
  source/generation and lowering/profile versions; external
  entry/input/context/state limits validate separately.

### 5. Claim limits

- Execution is profile-stamped and offline only. A receipt cannot claim a
  construct absent from its stamped profile. No whole-Perl, no CPAN-scale,
  no provider-behavior claim follows from this ADR.
- Interpreter/reference evaluation first. Native artifacts additionally bind
  ABI, backend, target, CPU features, codegen options, and dependency
  generations; in-process native execution is not a security sandbox.
- Operation admission is fail-closed: `no partial writes` protects admission
  and coherent state, never a fiction that Perl exceptions roll back an
  evaluation. #17534 owns the commit/cleanup table; stock-Perl seeds
  distinguish ordinary writes from localization.
- Budgets count semantic work under a shared reference/native policy with
  explicit session-reuse outcomes; a native compiler timeout is host
  containment, not a cooperative-stop promise.

### 6. Entry conditions

Staged, with owners. The #17510 review comments carry the point-in-time
pass/fail per row; the rules below are durable:

1. This ADR accepted (maintainer merge). No EIR types, crates, or lowering
   before that event.
2. #4770 unsupported-fact catalog and #4773 profile manifest/validator
   landed. Static `compiler_profile.v1` and series YAML are not substitutes.
3. PIR-A-side verification scope re-owned and executed for each admitted
   family (#2277 is closed `not_planned` and contributes zero scope).
4. Per-family coverage gates bind each lowering and evaluation slice:
   documented branch/loop/return/read lowering, binding fix-or-exclude
   at admission, context/access composition proof where the family needs
   it. Evaluation slices additionally need reference-evaluator
   conformance evidence over their families (differential against the
   admitted source matrix with fail-closed boundaries); evaluation never
   precedes its families' lowering coverage.
5. #4775 (types and construction only, no evaluation, no lowering)
   may proceed once (1) and (2) hold, under the §1 scoping amendment.
   #17512 and later lowering/evaluation slices additionally need (3)
   and (4) for their families.

## Non-goals

This ADR does not authorize: EIR evaluation of any Perl program; a native
backend or JIT tier; provider behavior, cutover, or editor-visible change;
whole-Perl or CPAN coverage claims; a second AST-walking interpreter, a
parallel scalar model in the backend, one crate per operation, or hidden
stock-Perl runtime fallback; claim movement in any generated status surface.

## Consequences

Positive:

- EIR has a canonical crate/model/dependency/limit record; #4775/#4777/#4779
  can converge instead of re-deriving the shape per slice.
- The provider-receipt tension is resolved by rule, not per-PR
  reinterpretation; the promotion gate stays intact.
- Family-gated entry keeps each lowering slice falsifiable against its own
  coverage instead of a whole-substrate readiness vote.

Tradeoffs:

- Two packages where one might do; the boundary is enforced by dependency
  tests, which is review load on every structural PR.
- The staged entry keeps #17512+ waiting on #4770/#4773/verifier work that
  has no implementation yet; impatience will press against condition (2).

## Alternatives Considered

### Interpret PIR-A directly as the execution substrate

Rejected. An analysis boundary or unresolved fact is not an executable
instruction; PIR-A stays source-anchored tooling.

### Single package for parser facts and evaluator

Rejected unless an equally concrete separation mechanism is shown. A single
package weakens the enforceable dependency boundary this ADR exists to hold.

### Require the provider receipt for the offline lane too

Rejected as a gate, retained as a promotion requirement. Waiting for an
unrelated cutover before any offline execution proof is the delay this
amendment exists to remove; isolation proof plus family coverage is the
stricter, more relevant bar.

### Canonize the #4775 sketch without SPEC-0035 reconciliation

Rejected. The sketch predates the profile charter; adopting it verbatim
would leave profile binding, handle discipline, and boundary shape
undecided.

## Follow-up Obligations

- Keep PLSP-SPEC-0025 the PIR-A data-model contract and PLSP-SPEC-0030 the
  L0-L6 contract; this ADR adds the EIR lane, not a parallel vocabulary.
- Land #4770/#4773 before #4775; re-own PIR-A verifier scope for admitted
  families.
- Prove the #7422 editor-path negative control before any execution-adjacent
  PR claims isolation.
- Record the accepted topology substitution, if any, at implementation
  start; do not silently drift placement.
- File family-gated lowering slices with their coverage evidence attached;
  missing proof stays missing.

## Status Links

- [PLSP-ADR-0005](PLSP-ADR-0005-hir-body-pir-eir-boundaries.md)
- [PLSP-PROP-0004](../proposals/PLSP-PROP-0004-eir-entry-and-runtime-boundary.md)
- [PLSP-SPEC-0035](../specs/PLSP-SPEC-0035-executable-profile-and-eir.md)
- [Compiler-backed LSP roadmap](../project/COMPILER_BACKED_LSP_ROADMAP.md)

## Why ADR-worthy

This is the dedicated EIR decision ADR-0005 demands. It fixes the
crate/model/dependency/limit record, resolves the provider-receipt tension
by rule, and stages EIR entry so each slice is falsifiable. Without it, EIR
work stays unauthorized and every lowering PR re-litigates the boundary.

