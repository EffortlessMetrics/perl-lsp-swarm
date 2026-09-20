# ADR-0045: Native Non-Current-Frame Variable Mutation Disposition

- **Status**: Accepted
- **Date**: 2026-08-24
- **Decides**: #11324 (research: native non-current-frame mutation disposition)
- **Constrains**: #11325 (admit exact non-current ordinary-frame scalar mutation), #11364
- **Related**: [ADR-0011](0011-dap-bridge-mode-architecture.md), [ADR-0019](0019-security-first-dap.md), [ADR-0027](0027-dap-bridge-native.md), [ADR-0036](0036-marker-framed-debugger-queries.md)

## Context

The native DAP adapter drives a stock `perl -d` (perl5db) child process through
marker-framed stdin commands (ADR-0036). `setVariable` is dispatched to
`handle_set_variable` (`crates/perl-dap/src/debug_adapter/dispatch.rs:112`), which
validates the name/value and then sends exactly two debugger commands — the
assignment and a read-back — that perl5db evaluates in its **current** (stopped)
frame:

```rust
// crates/perl-dap/src/debug_adapter/variables.rs:594
let commands = vec![format!("p {name} = {value}"), format!("p {name}")];
```

The request's `variablesReference` is checked only for being positive
(`crates/perl-dap/src/debug_adapter/variables.rs:494-504`); its encoded frame
identity is never decoded, so every writable path is a current-frame `p`
evaluation by construction. The *minting* side is already gated: since #10563
(merged as PR #11806), `scopes` serves only the exact current stopped frame and
returns an empty scope list for any other frame id
(`crates/perl-dap/src/debug_adapter/frames.rs:257-266`), `variables` refuses
scope references not bound to that frame plus all `Package`/`Globals` references
(`crates/perl-dap/src/debug_adapter/variables.rs:117-140`), and the code carries
the explicit note that the typed frame authority of #9045/#9046 will replace
this compatibility floor (`crates/perl-dap/src/debug_adapter/frames.rs:69-74`).
Issue #11324 therefore asks for the exact disposition before any implementation
leaf (#11325) is selected: supported, limited, unsupported, or not_proven with
the missing evidence named.

## Decision Drivers

- The mutation train must not promise caller-frame or recursive-frame editing
  merely because frames can be observed (#11324).
- Read-only acquisition and writable mutation are different capabilities; a DAP
  frame id is not a debugger pad index (#11324).
- A positive mechanism must prove exact frame identity, exact writable storage
  identity, serialized enter/assign/read-back/restore, acknowledgement distinct
  from write/prompt, and restoration under failure (#11324 "Required contract").
- No injected runtime helper, shim, XS module, or debuggee instrumentation
  project may be designed to obtain the capability (#11324 non-goals).
- This ADR is a research/decision packet: it changes no production mutation
  behavior, capability value, handler routing, or support claim.

## Evidence

Backend (perl5db `$VERSION = '1.82'` as shipped with Strawberry Perl 5.42.0,
`C:/Strawberry/perl/lib/perl5db.pl`, read 2026-08-24):

| ID | Fact | Source |
|----|------|--------|
| E1 | The complete perl5db command set contains no `up`, `down`, or `frame`-switching command. Commands are `- . = H S T W c f i l m n p q r s save source t w x y X/V enable disable R rerun` plus wrappers `a A b B e E h L M o O v w W`; the `%set` wrapper table only remaps pre-5.8.0 spellings of those same commands and adds none. | perl5db.pl:2804-2840 (`%cmd_lookup`), :4659-4683 (`%set`) |
| E2 | `f` switches the **file** view, not a frame: "The new f command switches filenames." | perl5db.pl:1885-1953 (quote at :1893) |
| E3 | `p`/DB::eval evaluates in the stopped frame's context only; usercontext is derived from the immediate `caller` of `DB::DB`. | perl5db.pl:2891 (`local $usercontext = _calc_usercontext($package)`) |
| E4 | The only shipped primitive that reaches **another frame's** lexicals is `y [levels]`: read-only display via `PadWalker::peek_my(level+2)`, requiring optional XS CPAN module PadWalker ≥ 0.08 and degrading to "PadWalker module not found - please install" when absent. | perl5db.pl:1954-2005 (require :1964, warning :1968, peek_my :1984) |
| E5 | perl5db's two PadWalker uses (`y` display, lexical completion) are both read-only; no write-through exists anywhere in perl5db. | perl5db.pl:1984, :9513 |
| E6 | PadWalker presence is environment-dependent, not core: present (PadWalker 2.5) on local Strawberry 5.42.0; absent on local cygwin-thread-multi 5.42.2 (local probe, 2026-08-24). | local interpreters |

Integration (current `main`):

| ID | Fact | Source |
|----|------|--------|
| E7 | Native sessions launch stock `perl -d`. | `crates/perl-dap/src/debug_adapter/process.rs:503` |
| E8 | `handle_set_variable` is frame-blind: after validating name/value it sends `p {name} = {value}` + `p {name}`; the scope reference's frame is never decoded, so every write is a current-frame `p` evaluation. | `crates/perl-dap/src/debug_adapter/variables.rs:473-681` (send :594; sole ref check :494-504) |
| E9 | Locals enumeration is a B walk of the **current** CV's padlist only (`$DB::sub` or `B::main_cv()`), with the pad selection hardcoded to the innermost pad (`frame = 0`); no frame-id-to-pad indexing remains. The helper is deliberately opaque about aggregate contents pending #7358. | `crates/perl-dap/src/debug_adapter/variables.rs:427-470` (comment :427-441, CV :446, pad-index template :450-451, `frame = 0` :468), invoked at :299 |
| E10 | `stackTrace` parses `T` text; frame ids are adapter-assigned, per-stop-rebound display indexes with no backend identity. | `crates/perl-dap/src/debug_adapter/frames.rs:20-104` (`T` at :104), `crates/perl-dap/src/debug_adapter/parsing.rs:87-120`, `crates/perl-dap/src/stack/parser.rs:184-191` (`starting_id: 1`) |
| E11 | Observation is already gated to the exact current stopped frame: `scopes` returns an empty scope list for any other frame id and serves only `Locals` (+ captured `Arguments`); `variables` returns honest-empty for scope references not bound to that frame and for `Package`/`Globals` kinds. Landed by #10563 (PR #11806). | `crates/perl-dap/src/debug_adapter/frames.rs:257-266`, `:289-309`; `crates/perl-dap/src/debug_adapter/variables.rs:117-140` |
| E12 | The canonical `DebugBackend` trait has **no** set-variable/mutation operation at all; mutation exists only in the DAP frontend's direct `p` writes. | `crates/perl-dap/src/backend/mod.rs` (`trait DebugBackend` method list) |
| E13 | The ptkdb external-peer path explicitly negotiates no variable setting ("ptkdb v1 does not set variables"). | `crates/perl-dap/src/backend/capabilities.rs:258` |
| E14 | The typed selected-frame observation substrate this ruling would build on is not merged: #9046 and #9045 are still OPEN; current main carries only the `exact_current_stopped_frame_id` compatibility floor, whose own comment defers to #9045/#9046. | GitHub issues #9046, #9045 (OPEN, verified 2026-08-24); `crates/perl-dap/src/debug_adapter/frames.rs:69-83` |

Protocol (Debug Adapter Protocol specification, `SetVariableRequest` /
`ScopesRequest` / `SetExpressionRequest`):

| ID | Fact | Source |
|----|------|--------|
| E15 | `setVariable` carries no `frameId`; the target frame is implied by the `variablesReference` provenance chain (`stackTrace` → `scopes(frameId)` → `variables(ref)`), and the reference "must have been obtained in the current suspended state". | DAP spec, `SetVariableRequest` |
| E16 | The protocol nowhere requires adapters to support setting variables in non-current frames; a `success: false` response whose message is surfaced to the user is the conformant refusal. | DAP spec, `SetVariableRequest`, `ErrorResponse`/`Message` |
| E17 | The protocol does anticipate frame-scoped writes generally (`setExpression` takes `frameId`: "evaluate … in the scope of this stack frame"), so a future supported/limited ruling is protocol-compatible if a backend mechanism is ever proven. | DAP spec, `SetExpressionRequest.frameId` |

Repository precedent:

| ID | Fact | Source |
|----|------|--------|
| E18 | The repo's established honest-refusal pattern: a request whose backend has no primitive (`restartFrame`, `terminateThreads` — "perl5db has no primitive for either") stays advertised = false with an explicit failure response. | `crates/perl-dap/CLAUDE.md`, "Capability advertising" |

## Decision

### Disposition: `not_proven` (primary ruling)

Non-current-frame mutation is **not admitted**. No production leaf may implement
or advertise it, and no capability cell may be promoted. This is the disposition
because exactly one mechanism class survives the negative screening below, and
that class has **zero executed proof** against the #11324 contract; the issue's
own rule applies: "If any proposition is missing, the corresponding cohort stays
unsupported/not-proven."

Ruling IDs (minted here as the durable outputs of #11324):

- `DAP-MUT-FRAME-CURRENT-01` — **supported (already shipped, unchanged)**:
  current-frame scalar assignment via perl5db `p {name} = {value}` with framed
  read-back (E8). Releasable independently of this ruling, per #11324.
- `DAP-MUT-FRAME-NONCURRENT-01` — **not_proven**: no shipped primitive can
  address a non-current frame; the one candidate mechanism class is unexecuted
  (see "Missing evidence"). Implementation guidance beyond the refusal path
  below is intentionally absent.
- `DAP-MUT-FRAME-RESTORE-01` — **vacuous under this ruling**: there is no
  context-enter primitive to restore. perl5db exposes no frame context that a
  mechanism could enter or corrupt (E1-E3); the candidate class below needs no
  debugger context switch at all, reducing any future "restore" obligation to
  ordinary transaction integrity (assignment/read-back ordering), not frame
  restoration.
- `DAP-MUT-FRAME-RESEARCH-01` — this ADR is the research/decision packet; the
  fixture matrix named below is the remaining evidence debt.

### Sub-ruling A: unsupported now — the perl5db frame-switch route is dead

An "existing perl5db command/context primitive that selects a specific stack
frame and can be restored deterministically" (#11324 candidate mechanism 1)
**does not exist**: the shipped command set has no frame selection at all (E1),
`f` switches files (E2), and `p` always evaluates in the stopped frame (E3).
This negative is decidable from the backend source and durable for every
perl5db that ships this command table; it cannot be re-litigated by a future
research PR, only superseded by a materially different backend.

### Sub-ruling B: unsupported now — every shipped integration primitive

- The B/PADLIST locals helper observes only the current CV's innermost pad, with
  the pad selection hardcoded to the innermost frame — it is observation-only
  for the current frame's sub and structurally cannot address a different
  caller sub (E9).
- The canonical backend model has no mutation operation (E12), and the ptkdb
  peer path negotiates no variable setting (E13).
- `T` is textual and read-only (E10).

A new B-based route mapping DAP frames to caller CVs, or any other newly
constructed introspection route, is **not** a shipped primitive; treating it as
candidate mechanism 2 under #11324 requires the full fixture matrix, exactly
like the candidate below.

### Sub-ruling C: the one surviving candidate class, and why it is not_proven

perl5db itself demonstrates the only mechanism class that can reach another
frame's exact storage without a new injected helper: **write-through a
caller-pad reference** obtained from `PadWalker::peek_my(level)` (E4). peek_my
returns references to the target frame's real lexical storage, so
`p ${ PadWalker::peek_my(N)->{'$x'} } = value` addresses the exact writable cell
by construction — no frame switch, no pad-index guessing. But:

1. PadWalker is an optional XS CPAN dependency, not core; perl5db degrades
   without it (E4), and local interpreters disagree about its presence (E6).
2. perl5db itself uses it only read-only (E5); no write-through exists anywhere
   in shipped software.
3. The `peek_my` context offset from inside a `p`/DB::eval frame is
   implementation-sensitive (perl5db calibrates `+2` for its own call depth,
   E4) and unverified for this integration's write path.
4. None of the #11324 contract obligations — serialized
   assign/read-back, engine acknowledgement distinct from write/prompt,
   same-name caller/callee and double-recursion discriminators, closure/eval/XS
   refusal cohort, timeout/cancel/disconnect at each stage, Perl-version and
   platform rows — has been executed. No fixture digests exist.

Missing evidence that would decide between `supported`/`expected_limited` and
final `unsupported`: the #11324 fixture matrix executed against a pinned
interpreter/PadWalker version table, namely (1) caller and callee with the same
lexical name and different sentinels; (2) two recursive invocations at the same
source line; (3) adjacent named subs and package transitions; (4) a
current-frame control; (5) refusal rows for closure/eval/XS/native frames;
(6) timeout/cancel/disconnect during assign/read-back; (7) a later stop at the
same line; (8) debugger-output interleaving — with PadWalker-present and
PadWalker-absent environments both recorded. Until that packet exists, the
cohort stays not_proven, per #11324: "Missing evidence remains not-proven rather
than implementation guidance."

### Honest refusal path (required interim behavior)

Current main already refuses non-current frames at the minting surface: scopes
and variables return honest-empty for anything not bound to the exact current
stopped frame (E11, landed by #10563), so no non-current-frame scope reference
can reach the write path today. The remaining obligation is forward-looking:
`handle_set_variable` itself never decodes the scope reference (E8), so the
current-frame-only write invariant currently holds by minting-side accident
rather than by handler contract. When the typed frame authority (#9045/#9046)
or any future selected-frame work mints scope references for non-current frames
again, `setVariable` through such a reference **must fail** with a descriptive
error message (E16), following the repo's `restartFrame`/`terminateThreads`
precedent (E18) — it must not silently evaluate in the current frame. This ADR
records the requirement; wiring the guard into the mutation train's
lowering/canonical target work (#10774/#10891) is a production change outside
this docs-only claim.

### Consequence for #11325

#11324 concludes **not_proven**, so under #11325's own start gate ("If #11324
concludes unsupported or not-proven, this node remains non-selectable/historical
until a new architecture issue changes the mechanism boundary"):

- #11325 is **not unlocked**; it stays non-selectable/historical (as does its
  public-proof twin #11364).
- No implementation leaf may be selected from the current mutation train for
  non-current frames.
- Reopening requires either (a) the fixture matrix above producing an accepted
  positive/limited ruling via a new research/ADR decision, or (b) a new
  architecture issue that changes the mechanism boundary (e.g. a decision to
  require PadWalker). Current-frame mutation (#11324's baseline) is unaffected.

## Considered and rejected alternatives

### Disposition `supported` or `expected_limited` now

Rejected. Would assert exact writable-storage identity, failure-safe
restoration, and version/platform rows with zero executed fixtures — precisely
the unsupported claim shapes #11324 forbids ("command write/prompt counts as
mutation success", "observation of a value is not writable storage proof").

### Disposition `unsupported` (flat, final)

Rejected as the primary ruling. The frame-switch route and all shipped
primitives are affirmatively unsupported (Sub-rulings A/B), but the write-through
caller-pad class is mechanistically live, demonstrated read-only by perl5db
itself, and has never been falsified by the required fixtures. A flat
`unsupported` would retire #11325 on evidence we do not have, and would misstate
the boundary the issue explicitly distinguishes ("Stop and rule
unsupported/not-proven rather than broadening" — both are honest; here the
missing evidence is exactly nameable, which is the `not_proven` shape).

### Designing a helper (injected runtime helper, shim, XS module, instrumentation)

Rejected. Forbidden by #11324 non-goals; any such route requires a separate
architecture/security programme.

## Consequences

### Positive

- The #11325 branch gets a definitive disposition today: not selectable, with
  the exact reopening condition named — no architectural rediscovery needed
  later.
- Two negative mechanism families (frame-switch commands, shipped primitives)
  are settled durably and cannot consume further research cycles.
- The current-frame-only write invariant is now recorded with its
  DAP-conformant refusal path, so the #10563 minting gate cannot be silently
  eroded when selected-frame work (#9045/#9046) lands.
- Current-frame mutation remains releasable and independent, per #11324's
  baseline requirement.

### Negative

- Non-current-frame editing remains unavailable to users of the native preview,
  and clients that offer it through advertised scopes will receive errors (once
  the refusal guard lands) rather than edits.
- The evidence debt is real: the deciding fixture matrix still has to be
  executed by a future research packet under a pinned interpreter/PadWalker
  table.

### Neutral / Follow-up

- A future research PR can execute the fixture matrix named in Sub-ruling C and
  supersede this ADR's primary ruling with `supported`/`expected_limited` for an
  exact cohort, or final `unsupported`.
- The refusal-path guard belongs in the mutation train's canonical
  target/lowering work (#10774/#10891) or a focused hardening leaf; this ADR is
  its authority, not its implementation.
- #9046 (exact selected-frame observation) remains the prerequisite seam for
  any future positive ruling (E14).

## Implementation Notes

This ADR is a research/decision packet only (#11324 "One-PR result": research,
normalized evidence, and an ADR/spec ruling). It changes no production mutation
behavior, capability values, handler routing, or public support claims.

- Backend citations E1-E5 are from perl5db 1.82 as shipped with Strawberry Perl
  5.42.0 (MSWin32-x64-multi-thread), `C:/Strawberry/perl/lib/perl5db.pl`, read
  2026-08-24; the PadWalker presence probe (E6) compared that interpreter with
  a cygwin-thread-multi 5.42.2 build. These are evidence rows, not support
  claims; no fixture digests exist yet (that is the not_proven gap).
- Integration citations E7-E13 are current-`main` file:line references and must
  be re-verified if the cited files move.
- The DAP citations E15-E17 are from the published Debug Adapter Protocol
  specification text for `setVariable`, `scopes`, and `setExpression`.
