# CLAUDE.md (perl-operation-trace)

## Role

The versioned `operation_trace.v1` contract: ephemeral operation identity, a
closed operation-kind vocabulary, a context that threads that identity
across call/async/background boundaries, a bounded structured-event model
with explicit privacy classification, and an in-memory recorder enforcing
terminal-state, ordering, and budget invariants. A self-contained contract
substrate — `publish = false`, no external consumer yet. No subsystem is
instrumented by this crate.

## Owns

- Ephemeral identity: `SessionId` (caller-supplied, non-blank), `OperationId`
  (`{session, sequence}`, wire form `op:<session>:<sequence>`),
  `ParentOperationId` (a type-distinct parent link), and
  `OperationIdAllocator` (deterministic, session-scoped minting).
- Closed vocabularies: `OperationKind` (what kind of operation as a whole)
  and `OperationEventKind` (what kind of event on that operation), both
  rejecting unknown wire values rather than decoding a default variant.
- `OperationContext`: the one value a caller threads across a call, async, or
  background boundary — an operation's own id, its optional parent, its
  `OperationKind`, and any attached, opaque `IdentityRef`s (tagged by the
  closed `IdentityRefKind`: `Source`/`Environment`/`Fact`/`Process`/
  `Receipt`/`Generation`), bounded by `MAX_ATTACHED_IDENTITY_REFS`.
  `OperationContext::root`/`::child` are the safe constructors: `child`
  always derives its parent link from the context it is called on, so a
  caller cannot pass an unrelated `OperationId` as "the parent" by mistake.
- Privacy: `FieldPrivacy` (`Public`/`Private`/`Secret`), `PrivateValue`
  (redacted, byte length disclosed), `SecretField` (redacted, nothing
  disclosed).
- Events: `OperationEvent` (kind plus ordered `(name, EventFieldValue)`
  fields), `EventFieldValue` (integer/boolean/public string/private/secret),
  `OperationOutcome` (the closed terminal-outcome vocabulary).
- `EventRegistry`: per-event-kind required/allowed fields and their declared
  privacy tier, with typed rejection of a missing field, an undeclared
  field, or a field supplied at the wrong tier.
- `OperationRecorder`: in-memory, bounded (`RecorderBounds`), taking a
  `&OperationContext` per call and enforcing exactly-one-terminal,
  no-event-after-terminal, no self-parent, no parent cycle, one kind and one
  parent per operation, and explicit truncation markers on bound violation.
  Also a `disabled()` no-op mode. A recorded snapshot (`RecordedOperation`)
  carries the operation's established `OperationKind` alongside its id,
  parent, and events.
- `OperationTraceSchemaVersion` / `SCHEMA_VERSION_V1`, fail-closed on decode.

## Does not own

- Any of the five existing operation-identity namespaces this crate was
  written to eventually correlate — `OperationId` in
  `perl-subprocess-runtime`, `ParserOperationId` in `perl-parser-core`,
  `WorkspaceRuntimeOperationId` in `perl-workspace`,
  `ReachabilityOperationKind` in `perl-semantic-facts`, and the wire
  `operationId` field in `perl-dap`. This crate migrates and retires none of
  them.
- Instrumenting any subsystem: no LSP/DAP/workspace/subprocess-runtime code
  calls into this crate yet.
- Durable, content-addressed, semantic identity — that is
  `perl-source-identity`'s `source_identity.v1`, a deliberately different
  concern (see "Neighbors" below).
- Interpreting an attached `IdentityRef`'s meaning, format, or currentness.
  It is stored as an opaque, validated (non-blank) string tagged with an
  `IdentityRefKind`; the owning producer (e.g. `perl-source-identity` for
  `IdentityRefKind::Source`) remains the sole authority for what the value
  denotes.
- Fingerprinting/digesting private or secret values. This crate has no hash
  function dependency; a fingerprinted private tier, if a producer needs
  one, is that producer's concern.
- Persistence: `OperationRecorder` is in-memory only, per process.

## Invariants

- `OperationId`'s wire form (`op:<session>:<sequence>`) never uses the
  `sha256:` prefix `perl-source-identity` uses for durable identity, and
  this crate depends on no hash function at all — folding an `OperationId`
  into a digest is not just unused here, it is unreachable
  (`tests/dependency_contract.rs` forbids `sha2`).
- `SessionId` is always caller-supplied; there is no
  `SessionId::new_random()`. `OperationIdAllocator` is deterministic given
  the same session — no hidden global counter, no ambient entropy.
- `OperationKind`, `OperationEventKind`, and `IdentityRefKind` are closed,
  not `#[non_exhaustive]`: unknown wire input is a decode error, not a
  default variant.
- `OperationContext::child` always sets the child's parent to the exact
  operation id of the context `child` was called on. There is no
  constructor that takes an independently chosen `OperationId` as "the
  parent" — a caller can still hand-build a bad context (e.g. pass the same
  id for both the parent context and the new child), which is why
  `OperationRecorder::record` still checks for self-parent and cycles at
  record time regardless of how the context was constructed.
- `OperationContext::attach` rejects beyond `MAX_ATTACHED_IDENTITY_REFS`
  with a typed `ContextError` rather than growing the list unboundedly.
- A registry-declared `Secret` field must not be supplied as `Private`
  either — `Private` still discloses a byte length, which is exactly what
  `Secret` exists to withhold.
- `OperationRecorder` enforces exactly one `Terminal` event per operation,
  ever; a second, or any event after it, is a typed `RecordError`. An
  operation with no `Terminal` event reports `TerminalState::NotProven` from
  a query — never success, never a panic.
- A bound violation (`max_events`, `max_payload_bytes`) records an explicit,
  observable truncation marker in place of the dropped event. A silently
  dropped event is indistinguishable from one that never happened, which
  would make the trace lie about what occurred. This applies uniformly,
  including to a `Terminal` event that arrives once `max_events` is already
  reached: it is truncated like any other event, its outcome is discarded,
  and the operation reports `NotProven` forever afterward — deliberate, not
  accidental (see `record`'s doc comment and the dedicated test).
- An operation's parent and kind are both fixed at first mention. A later
  `record` call may supply a context with no parent (a no-op) or the same
  parent/kind (also a no-op), but a genuinely different parent or kind is a
  typed `RecordError::ParentConflict`/`KindConflict`, not a silently
  accepted overwrite.
- A disabled recorder accepts every call and stores nothing; it does not
  perform partial validation against throwaway state.

## Neighbors

- Upstream: `serde` only; the full transitive closure is pinned by an exact
  allowlist test (see below). No `sha2`, `rand`, `tokio`, or time/PID
  sources.
- Sibling, not dependency: `perl-source-identity` (`source_identity.v1`) —
  durable/content-addressed/semantic identity, the opposite of this crate's
  ephemeral/non-semantic/session-local identity. Neither crate depends on
  the other; `IdentityRefKind::Source` names it by convention, not by import.
- Sibling idiom: `perl-subprocess-runtime`'s process-identity module
  (`src/process/identity.rs`) — the closest existing privacy-tier and
  correlation-id idiom in the repository. This crate is not a dependency of
  it and vice versa; the resemblance is deliberate convention-following, not
  code sharing.
- Downstream: none yet. The five existing operation-identity namespaces
  listed under "Does not own" are this crate's eventual correlation targets,
  not current consumers.

## Read first

- `src/lib.rs` — the crate-level contract: why it exists, the
  ephemeral-vs-durable contrast, determinism, context propagation, privacy,
  quick start.
- `src/context.rs` — `OperationContext`'s safe-construction shape and the
  attached-reference bound.
- `src/ids.rs` — `OperationId`'s wire parsing and the deterministic
  allocator.
- `src/privacy.rs` — the three-tier privacy model and why it does not
  fingerprint.
- `src/recorder.rs` — the terminal/ordering/parent/kind/bounds invariants.
- `tests/negative_control.rs` — the proof that ephemeral identity does not
  leak into recorded domain content, holding kind constant across the two
  compared operations.
- `tests/redaction.rs` — the hostile-fixture proof for both non-public
  tiers.
- `tests/dependency_contract.rs` — the fail-closed `cargo tree` allowlist.

## Focused validation

`cargo test -p perl-operation-trace --all-targets --locked`. The dependency
contract shells out to `cargo tree` and fails closed when the instrument
cannot run. Coverage includes: allocator determinism, wire-format rejection,
closed-vocabulary fail-closed decoding, context construction and attachment
bounds, registry field/privacy validation, terminal/parent/kind/bounds
invariants, disabled-mode parity, the negative control, and the
hostile-fixture redaction proof.

## Review hotspots

- Any new dependency fails `tests/dependency_contract.rs` until it is
  reviewed into the exact `PERMITTED` allowlist; a denylist alone would
  silently admit anything nobody thought to forbid.
- A new `EventFieldValue` variant or a new `OperationEventKind`/
  `OperationOutcome`/`IdentityRefKind` value is a vocabulary change: update
  the closed-decoding tests (and, for event kinds, `EventRegistry`'s
  declared fields) together, not separately.
- A change to `PrivateValue` or `SecretField`'s `Serialize`/`Debug` impls is
  the single highest-consequence edit in this crate — it is the only thing
  standing between a hostile fixture and a leaked trace. Any change here
  must keep (or extend) `tests/redaction.rs`'s three assertions: private
  content absent, secret content absent, and secret byte length absent.
- `OperationRecorder::record`'s ordering (parent check, then kind check,
  then terminal check, then truncation check, then registry validation,
  then bound checks) is load-bearing: reordering it can silently reopen
  "event after terminal", "event after truncation", or a stored
  kind/parent conflict as accepted paths.
- `OperationContext::child` deriving the parent from `self` is what makes an
  *unrelated* parent unconstructible through the ergonomic API; do not add a
  `child`/`with_parent` overload that takes an independent `OperationId` for
  the parent without a strong reason, since that reintroduces exactly the
  hand-wired-triple mistake this type replaced.

## Claim boundary

This crate makes the `operation_trace.v1` vocabulary exist, makes an
operation's identity/kind/parent one value a caller threads through a call
(`OperationContext`) instead of a hand-wired triple, and makes violations of
the contract (unknown kinds, bad privacy tiers, duplicate terminals, parent
or kind conflicts, parent cycles, silent truncation, leaked private/secret
content, unbounded attached references) detectable by construction and by
test. It does **not** make any real operation traceable: no subsystem calls
into this crate, so today it instruments nothing. It does not retire,
migrate, or even reference the five existing operation-identity namespaces
it was written to eventually correlate, and it does not interpret what an
attached `IdentityRef` denotes — that migration, and the semantics of any
particular reference, belong to a later, separate claim.
