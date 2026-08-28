# ADR-0046: Loaded-Module Reload Semantics (R01 Contract)

- **Status**: Accepted
- **Date**: 2026-08-24
- **Decides**: #10097 (research: loaded-module reload eligibility, protocol, and runtime-generation semantics)
- **Constrains**: #10138 (custom family registration), #10098 (mechanism execution), #10102 (composition and reconciliation), #10104 (exact public proof)
- **Parent**: #10095 (reload programme controller)
- **Related**: [ADR-0019](0019-security-first-dap.md), [ADR-0027](0027-dap-bridge-native.md), [ADR-0036](0036-marker-framed-debugger-queries.md), [ADR-0042](0042-module-provenance-detection.md), [ADR-0043](0043-module-provenance-detection.md), [ADR-0045](0045-noncurrent-frame-mutation-disposition.md)

## Context

The #10095 programme must earn a bounded manual reload transaction before
any editor automation. Today the repository has no owner for changing code
inside a live Perl debuggee, and every adjacent authority observes but
never mutates: loaded modules and sources come from a framed `%INC` dump
(`crates/perl-dap/src/debug_adapter/output.rs:318`, `query_inc_entries`,
parsed by the regex at `debug_adapter/patterns.rs:310`) consumed by
`loadedSources` (`output.rs:361`) and `modules`
(`modules_from_inc_entries`, `output.rs:428`); suspension truth is the
session's only generation (`stopped_generation`,
`debug_adapter/session.rs:22-24`, advanced fail-closed by
`current_stopped_frame_id` with the `SCOPE_FRAME_ID_MAX = 99_999` ceiling,
`debug_adapter/process.rs:29-45`); path authority is
`security::validate_path` (`crates/perl-dap/src/security/mod.rs:86`) used
by `handle_source` (`output.rs:196`) through `validate_source_path`
(`debug_adapter/mod.rs:325`); capabilities derive only from the generated
catalog (`handle_initialize`, `debug_adapter/process.rs:49`;
`features_sot.toml` `dap.modules`/`dap.loaded_sources`); and the request
surface is a closed 37-command list with typed refusal for unknown
commands (`debug_adapter/dispatch.rs:6-44`, `:136-143`).

A naive `delete $INC{...}; require ...` cannot establish package
replacement, symbol removal, object/closure migration, active-frame
safety, rollback, or breakpoint/source currentness. Worse, the current
read-only observation path already demonstrates the exact failure shape a
mutation transaction must refuse: a framed-query timeout maps the unknown
outcome to an empty list (`output.rs:333-338`, `unwrap_or_default()`).

This ADR freezes the R01 contract. It changes no production runtime
behavior: the typed module lives at `crates/perl-dap/src/reload/` as a
pure semantic layer with no dispatch, no debugger I/O, no capability
change, and no wire format. The machine-checkable corpus is
`.spec/10097-loaded-module-reload-contract/`, bound to the module by
fixture-driven tests.

## Decision Drivers

- #10095 invariants: compilation/source validation alone is not reload
  success; a possibly-applied transaction advances runtime-module identity
  even if read-back later fails; old frames, scopes, variables, evaluate
  results, loaded-source facts, and applied breakpoint identities cannot
  remain exact after a possibly-applied reload; failed eligibility or
  preflight changes no runtime state; the client cannot authorize reload
  through path/package/raw-command reconstruction; protocol negotiation
  never upgrades eligibility or standard capabilities.
- The plan is frozen (#10097 plan comment, 2026-08-24): closed 13-class
  vocabulary, possibly-applied boundary, per-process monotonic
  `RuntimeModuleGeneration` advanced by both `reloaded` and
  `indeterminate_possibly_applied`, per-object-kind invalidation composed
  with `stopped_generation`, namespaced-family protocol requirements only,
  and mechanism limitation statements.
- The wire format stays with #10138 (#4838/#6737 authority); this decision
  must not pre-empt or duplicate it.

## Evidence

Live-tree witnesses (current `main`, 2026-08-24), each naming what a naive
implementation would get wrong:

| ID | Fact | Source |
|----|------|--------|
| W1 | No stable runtime module identity: `modules` assigns `Module.id` by sorted position per query; the same package under two roots yields two entries with fresh positional ids. | `output.rs:428-441`, existing test at `:449-468` |
| W2 | Generationless observations: `%INC` queries are live dumps with no snapshot identity and no invalidation; `loadedSources` and `modules` share them. | `output.rs:318-358` |
| W3 | Ambiguity rendered as absence: a framed-query timeout maps to empty. Honest for read-only queries; forbidden for a mutation transaction, where the identical condition means `indeterminate_possibly_applied`. | `output.rs:333-338` (`unwrap_or_default`) |
| W4 | The adapter's source view is disk-only: `handle_source` validates then reads the saved file; there is no unsaved-buffer channel. The contract therefore defines the subject as saved disk source and requires the client-declared digest to match it; dirty-buffer refusal is a declared limitation until the R05A editor leaf. | `output.rs:196-260`, `debug_adapter/mod.rs:325` |
| W5 | Applied breakpoints have no per-source installation identity: engine installs are line-relative `b <line> [cond]` in the debugger's current file context; after a source-changing reload there is nothing to reconcile against — the seam #10102 owns (#8080/#1742 stay open). | `debug_adapter/breakpoints/line.rs:76-80` |
| W6 | `threadId` is an adapter-synthetic, session-scoped projection (at most one synthetic execution context; empty before any session), not a runtime fact a reload could invalidate. | `features_sot.toml:1405` (`dap.threads`, #12273) |
| W7 | The suspension authority (`stopped_generation`) is the proven fail-closed generation precedent: saturating advance, bounded wire encoding, sentinel past ceiling. | `session.rs:22-24`, `process.rs:29-45` |
| W8 | Bounded retained observations have a house precedent (opaque ids, ring cap 128). | `crates/perl-workspace/src/workspace/runtime_generation/core.rs` |
| W9 | Typed reference bands are the invalidation-style precedent for encoding identity into references unambiguously. | `debug_adapter/var_ref.rs` |
| W10 | Zed is a second concrete adapter consumer alongside VS Code (#12271), strengthening the protocol split: family identity and wire registration belong to #10138; this ADR decides only what the family must mean. | `debug_adapter_schemas/perl-dap.json`, #12271 |

## Decision

### 1. Exact subject identity

A reload subject is bound by **all** of: process/session generation,
current suspension generation, logical source identity (editor URI), the
runtime `%INC` key and resolved runtime path, the current loaded-source
observation generation, the saved source content digest/revision, the
selected Perl runtime identity, the launch/root authority, the module
classification, and the operation's correlation identity
(`LoadedModuleSubject` in `crates/perl-dap/src/reload/subject.rs`).
Path spelling, basename, `%INC` key alone, package name alone, or matching
source bytes is insufficient: `SubjectCandidate::bind` refuses incomplete
bindings with `insufficient_subject_identity`, and a bound subject that is
no longer current (session replaced, generations moved, digest changed)
must re-bind before admission.

### 2. Closed eligibility vocabulary and precedence

Exactly thirteen dispositions exist (`LoadedModuleReloadEligibility`,
`reload/eligibility.rs`): `eligible_source_backed_perl_module` and the
twelve refusal classes `not_loaded`, `source_not_exact_or_stale`,
`dirty_or_unsaved_source`, `active_frame_in_target`,
`main_program_not_module`, `xs_or_native_module`,
`source_filter_or_compile_hook_boundary`, `generated_or_eval_source`,
`ambiguous_runtime_mapping`, `outside_launch_authority`,
`unsupported_runtime`, `not_stopped_or_not_command_ready`. The initial
admitted cohort is `eligible_source_backed_perl_module` with **no active
target-module frame**; every other class fails closed. Classification is a
pure function of the observed admission facts under a frozen precedence
(readiness → runtime → presence → authority → mapping → classification →
dirty client source → binding completeness/currentness → active frame →
eligible), so the same observation always yields the same disposition.
Future leaves may add refusal classes; they may never widen the admitted
cohort silently (#10098's measured limits can only narrow).

### 3. Transaction phases and the possibly-applied boundary

Eight frozen phases: `admission`, `preflight`, `prepare`,
`runtime_mutation_begins`, `runtime_acknowledgement_read_back`,
`commit_generation`, `post_reload_reconciliation`, `terminal_projection`
(`reload/transaction.rs`). The boundary is entering
`runtime_mutation_begins`:

- a failure **before** the boundary is terminal
  `failed_before_mutation` — no generation advance, no invalidation;
- a timeout, transport loss, or ambiguous response **at or after** the
  boundary is terminal `indeterminate_possibly_applied` — generation
  advance, full invalidation, and an explicit non-clean projection.
  `project_unknown_after_mutation` is the only contract-valid projection
  of a post-boundary unknown; mapping it to empty/success (the W3 shape)
  is a contract violation.

The four terminal kinds (`reloaded`, `refused`, `failed_before_mutation`,
`indeterminate_possibly_applied`) and their phase validity are pinned by
`phase_permits_outcome`; only `reloaded` projects clean.

### 4. Runtime-module generation

`RuntimeModuleGeneration` (`reload/generation.rs`) is a per-debuggee-
process monotonic authority, the module-reload analogue of
`stopped_generation` (W7): it advances on **both** `reloaded` and
`indeterminate_possibly_applied`, never on refusals or pre-mutation
failures, saturates without rollover at exhaustion (everything observed
before exhaustion is stale), and resets only when the process/session is
replaced. The advancement law fails closed on malformed outcomes: a
`failed_before_mutation` outcome carrying a phase at or after the
mutation boundary violates the frozen phase/kind pairing, and the
generation authority treats it as an advance — an invalid construction
can never leave old references current. Retained per-generation observations are bounded at 128 (W8)
and fail closed (unknown or evicted ⇒ stale). It is **independent** of
`stopped_generation` — one advances for module mutations, the other for
suspensions — but composes with it in the invalidation table (#8703/#10102
seam). #10098/#10102 carry it on `DebugSession`; this ADR owns its
meaning, and this PR wires nothing.

### 5. Invalidation table

`ReloadInvalidationPlan` (`reload/invalidation.rs`) assigns a disposition
to every enumerated DAP object kind for both terminal mutation outcomes
(completeness is test-pinned), and nothing for refusals/pre-mutation
failures:

| Object kind | Disposition |
|---|---|
| frame / scope / variable / evaluate references | stale when **either** the runtime-module generation or the suspension generation advanced past the reference's bind point (OR composition) |
| exception / current-stop facts (where affected) | stale when either generation advanced |
| `modules` module ids | always stale (positional per query, W1) |
| loadedSources/`%INC` observations | always stale (re-query, W2) |
| source content reads | always stale for the affected source |
| applied breakpoint installations for the affected source | always stale (no installation identity exists today, W5; #10102 reconciles) |
| retained runtime query results that could observe old code | always stale |
| thread references | **adapter projection, re-projected** — never treated as runtime fact (W6) |
| durable desired client breakpoint configuration | **preserved**, reconciled later by #10102 |

`verify_invalidation_plan` enforces the table **exactly**: for a
mutating outcome a claimed plan must match the frozen disposition of
every enumerated kind — preserving positional module ids,
loaded-source observations, source reads, applied installations,
exception/stop facts, or retained query results is
`stale_identity_survives_possibly_applied`, invalidating durable
configuration is `durable_configuration_invalidated`, and treating
thread references as runtime facts is `thread_reference_not_projection`.
For non-mutating outcomes the only valid plan is empty.

### 6. Protocol requirements (not wire)

The request family must be a **namespaced, versioned** custom DAP family
with a correlation identity on every request/response pair; it must not
accept raw module paths, debugger commands, or Perl expressions (the
payload is the typed subject only); it must not collide with standard DAP
request names (checked against the adapter's single supported-command
authority, `dispatch.rs` `SUPPORTED_COMMANDS`) and must not invent a
standard DAP capability; and it stays **unadvertised/false until R04**
proof (`reload/surface.rs`). The wire format, negotiation, and generated
Rust/TypeScript contracts are #10138's alone (#4838/#6737), consuming
these requirements.

### 7. Mechanism record

Four mechanisms are compared by limitation statements only
(`reload/mechanism.rs`): `%INC` deletion + `require`, a `do`/`require`
helper, a workspace-owned runtime helper/observer, and Class::Refresh
strictly as a measured compatibility subject — never a bundled dependency
and never product authority by availability. Shared Perl truths recorded
as limits for every mechanism: re-`require` does not remove old symbols;
existing instances, closures, and lexical state keep old code; `@ISA`/mro
caches need an explicit `mro::method_changed_in`; active frames continue
on old code; source filters and compile hooks re-enter only under their
own conditions; and **compile success is never reload success**. Live
measurement is #10098's harness; this record states what each candidate
can and cannot prove today.

## Considered and rejected alternatives

### Mapping a post-boundary timeout to an empty/clean answer

Rejected. This is the W3 read-only pattern applied to a mutation: the
debuggee may run new code while the adapter reports nothing changed.
Refused by `project_unknown_after_mutation` and negative control 6.

### Admitting a wider initial cohort (active frames, XS, generated source)

Rejected. #10095's initial boundary admits only saved, exact,
source-backed ordinary modules with no active target frame; each widening
needs its own earned rule and proof (#10098 may narrow, never widen).

### Defining the wire format here

Rejected. Family identity, negotiation, and generated contracts are
#10138's claim under #4838/#6737; this ADR would create duplicate protocol
authority. Only the requirements the wire must satisfy are frozen here.

### Inventing a standard DAP capability for reload

Rejected. Standard DAP has no such capability; advertising one would be a
spec collision. The family stays namespaced and unadvertised until R04.

### Bundling Class::Refresh (or requiring its presence) as the mechanism

Rejected. Availability is not authority (#10095 non-goals); it is measured
as a compatibility subject only, with its own documented limits (XS
excluded, `%INC`-deletion core).

### A second module registry or debugger writer

Rejected. The contract reuses the existing `%INC` observation, path
authority, capability catalog, and command list as cited; it adds only
the missing semantic layer.

## Consequences

### Positive

- #10098, #10138, and #10102 consume one frozen typed contract instead of
  re-deriving semantics; the admitted cohort, the possibly-applied
  boundary, and the invalidation table are machine-checked.
- The indeterminate-as-clean mapping class is now contract-illegal with a
  named detector, closing the W3 hazard before any mutation code exists.
- Generation semantics get a second authority (runtime-module) with the
  proven fail-closed shape of the first (suspension), composed rather
  than conflated.

### Negative

- Users still cannot reload anything: this PR lands semantics only, and
  every runtime leaf remains unimplemented.
- The mechanism record states limits without measurements; #10098 must
  produce them before any positive mechanism claim.

### Neutral / Follow-up

- #10138 registers the family and owns the wire; #10098 executes and
  measures; #10102 wires the generation onto the session, applies
  invalidation, and reconciles durable breakpoints; #10104 proves the
  transaction through exact public stdio; R04-gated advertisement.
- If #8703's explicit session generations land, the composition seam
  already records their interaction (OR composition at the invalidation
  table).

## Implementation Notes

- Research/decision packet only (#10097 "One-PR result"): the typed module
  performs no live reload, sends no debugger command, advertises no
  capability, and defines no wire format.
- The `.spec/10097-loaded-module-reload-contract` schema is corpus
  description, not wire; a fixture-driven test keeps its enums in exact
  sync with the Rust vocabularies so neither drifts silently.
- Integration citations are current-`main` file:line references (basis
  `main@b86384bca`, 2026-08-24) and must be re-verified if the cited
  files move.
- The `pub(crate)` re-export of the supported-command list from
  `debug_adapter` exists so the surface collision check consumes the
  single existing command authority rather than a second registry.
