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

- Ephemeral identity: `SessionId` (caller-supplied; non-blank, no ASCII
  control character, at most `MAX_SESSION_ID_BYTES` bytes), `OperationId`
  (`{session, sequence}`, wire form `op:<session>:<sequence>`),
  `ParentOperationId` (a type-distinct parent link), and
  `OperationIdAllocator` (deterministic, session-scoped minting; `next()`
  returns `Option<OperationId>`, `None` once the per-session `u64` sequence
  space is exhausted rather than silently repeating the final id; not
  `Clone`, deliberately, since cloning would copy `next_sequence` and hand
  two allocators the same next id).
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
  field, a field supplied at the wrong tier, or the same declared field name
  supplied more than once.
- `OperationRecorder`: in-memory, bounded (`RecorderBounds`: `max_events`
  and `max_payload_bytes` per operation, `max_operations` across the whole
  recorder), taking a `&OperationContext` per call and enforcing
  exactly-one-terminal, no-event-after-terminal, no self-parent, no parent
  cycle, one kind and one parent per operation, and explicit truncation
  markers on a per-operation bound violation, or a typed rejection
  (`RecordError::TooManyOperations`) — never silent eviction — on the
  recorder-wide operation cap. Also a `disabled()` no-op mode. A recorded
  snapshot (`RecordedOperation`) carries the operation's established
  `OperationKind` alongside its id, parent, and events; the snapshot also
  carries `operations_rejected_by_cap`, the recorder-wide counterpart to a
  per-operation truncation marker.
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
- Fingerprinting/digesting private or secret values *from within this
  crate's own API surface*. This crate's dependency closure has no hash
  function, and no API here folds a value into a digest — but that only
  proves this crate's own code does not fingerprint; it does not make
  fingerprinting unreachable in any absolute sense, since a caller can
  implement a digest in safe Rust with no dependency at all, or hash
  `OperationId::as_wire`'s deliberately exposed identity string. A
  fingerprinted private tier, if a producer needs one, is that producer's
  concern and that producer's obligation to keep ephemeral/redacted identity
  out of it.
- A redaction guarantee for `SessionId`. The three-tier privacy model
  ([`FieldPrivacy`]) applies to `EventFieldValue` only; `SessionId` is
  validated for hygiene (non-blank, no control character, bounded length),
  not privacy, and appears verbatim in `OperationId::as_wire` and in several
  `RecordError::Display` messages.
- Persistence: `OperationRecorder` is in-memory only, per process.

## Invariants

- `OperationId`'s wire form (`op:<session>:<sequence>`) never uses the
  `sha256:` prefix `perl-source-identity` uses for durable identity. What is
  actually proven, and no more: this crate's own dependency closure contains
  no hash function (`tests/dependency_contract.rs` forbids `sha2`,
  fail-closed), and no API in this crate folds an `OperationId` into a
  digest or fingerprint. Absence of `sha2` does not make fingerprinting
  unreachable in any absolute sense — a caller can hash
  `OperationId::as_wire`'s exposed string in safe Rust with no dependency at
  all — so keeping ephemeral identity out of a durable digest remains a
  contract obligation on consumers, not something this crate enforces.
- `SessionId::new` rejects a blank label, a label containing an ASCII
  control character (`SessionIdError::ContainsControlCharacter` — a
  log/output-injection hazard, since the label is embedded verbatim into
  `OperationId::as_wire` and into several `RecordError::Display` messages),
  and a label over `MAX_SESSION_ID_BYTES` (128) bytes
  (`SessionIdError::TooLong`). This is hygiene validation, not a privacy
  tier: a `SessionId` carries no redaction guarantee at all, unlike
  `EventFieldValue`'s three tiers below.
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
- `EventRegistry::validate` rejects the same declared field name supplied
  more than once (`RegistryError::DuplicateField`). This is what keeps
  `RecorderBounds::max_payload_bytes` — an approximate budget over field
  names plus `approx_payload_len()`, not a hard cap on serialized bytes, see
  below — from being gamed: 500 repetitions of one cheap field previously
  passed the budget while the actual serialized trace overshot it by 4.57x,
  because serde's externally tagged encoding adds a per-value wrapper this
  accounting never counts.
- `OperationRecorder` enforces exactly one `Terminal` event per operation,
  ever; a second, or any event after it, is a typed `RecordError`. An
  operation with no `Terminal` event reports `TerminalState::NotProven` from
  a query — never success, never a panic.
- A per-operation bound violation (`max_events`, `max_payload_bytes`)
  records an explicit, observable truncation marker in place of the dropped
  event. A silently dropped event is indistinguishable from one that never
  happened, which would make the trace lie about what occurred. This applies
  uniformly, including to a `Terminal` event that arrives once `max_events`
  is already reached: it is truncated like any other event, its outcome is
  discarded, and the operation reports `NotProven` forever afterward —
  deliberate, not accidental (see `record`'s doc comment and the dedicated
  test).
- `max_events` bounds *admitted events* only — a truncation marker is
  metadata about the recording, not an admitted event, so it is never
  charged against the bound. A snapshot for one operation can therefore hold
  up to `max_events + 1` entries: `max_events` admitted events plus at most
  one marker. `max_events = 0` is well-defined: zero events are ever
  admitted, and the first `record` call yields only the marker. (Fixed an
  off-by-one where the marker itself was counted against the bound.)
- `RecorderBounds::max_operations` bounds the recorder as a whole, not one
  operation: once that many *distinct* operations are established, a
  **new** operation's first `record` call is rejected
  (`RecordError::TooManyOperations`), counted in
  `OperationRecorder::operations_rejected_by_cap` and surfaced on
  `OperationTraceSnapshot::operations_rejected_by_cap`. An operation already
  established keeps accepting events afterward; the cap never evicts an
  existing operation to make room for a new one — eviction would discard a
  complete trace, exactly the silent loss the truncation-marker design
  exists to avoid elsewhere.
- `EventRegistry` validation runs *before* an operation is looked up or
  created at all: a registry-invalid event never establishes the
  operation's kind or parent, is not counted by `operation_count`, and does
  not consume `max_operations` capacity. (Fixed a bug where a
  registry-rejected first event still silently claimed the operation's kind,
  so a later, valid event with a *different* kind could spuriously hit
  `KindConflict`.)
- `EventFieldValue::approx_payload_len` (the basis of `max_payload_bytes`
  accounting) charges every variant the length of what it actually
  serializes to — a `Secret` value's *fixed* redacted length, a `Private`
  value's *rendered* `<redacted:N bytes>` length — **never** the private or
  secret plaintext's own length. Charging a `Secret` value's plaintext
  length used to make budget admission itself leak a predicate on the
  secret's length: a real side channel through the one tier designed to
  disclose no length at all.
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
  leak into recorded domain content: three cases, each varying exactly one
  of operation id / kind / parent while holding the other two fixed.
- `tests/redaction.rs` — the hostile-fixture proof for both non-public
  tiers.
- `tests/dependency_contract.rs` — the fail-closed `cargo tree --target all`
  allowlist.

## Focused validation

`cargo test -p perl-operation-trace --all-targets --locked`. The dependency
contract shells out to `cargo tree --target all` (unioning every target's
dependency graph, not just the host's) and fails closed when the instrument
cannot run. Coverage includes: allocator determinism and exhaustion,
wire-format rejection, closed-vocabulary fail-closed decoding, context
construction and attachment bounds, registry field/privacy/duplicate-field
validation, terminal/parent/kind/bounds/operation-cap invariants,
secret-budget-accounting independence from plaintext length, disabled-mode
parity (including registry-validation bypass), the negative control (id,
kind, and parent independence), and the hostile-fixture redaction proof.

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
- `OperationRecorder::record`'s ordering — registry validation first (before
  the operation is even looked up), then the `max_operations` cap (for a
  brand-new operation only), then parent check, then kind check, then
  terminal check, then truncation check, then `max_events`/`max_payload_bytes`
  — is load-bearing: reordering it can silently reopen "event after
  terminal", "event after truncation", a stored kind/parent conflict, a new
  operation evading the operation cap, or (the bug this ordering fixed) a
  registry-rejected event nonetheless establishing the operation's kind and
  parent for a later, unrelated call.
- `OperationContext::child` deriving the parent from `self` is what makes an
  *unrelated* parent unconstructible through the ergonomic API; do not add a
  `child`/`with_parent` overload that takes an independent `OperationId` for
  the parent without a strong reason, since that reintroduces exactly the
  hand-wired-triple mistake this type replaced.
- `SessionId` sits *outside* the three-tier privacy model, not inside it at
  a weaker tier. It is embedded verbatim into `OperationId::as_wire` and
  into several `RecordError::Display` variants. `SessionId::new`'s
  control-character/length validation is a log/output-injection hygiene
  guard, not a redaction guarantee — do not describe it as one, and do not
  route a value that needs `Private`/`Secret` treatment through `SessionId`
  instead of an `EventFieldValue`.
- Any wording claiming fingerprinting an `OperationId`/`PrivateValue`/
  `SecretField` is "unreachable" or "structurally impossible" overstates
  what `tests/dependency_contract.rs` proves: it proves this crate's own
  dependency *closure* has no hash function and no API here folds a value
  into a digest — it does not and cannot prove a downstream caller can't
  hash `OperationId::as_wire`'s exposed string in safe Rust with no
  dependency at all. State the narrower, actually-proven claim (see
  `src/privacy.rs`, `src/lib.rs`).
- `RecorderBounds::max_payload_bytes` is an approximate retention budget
  (field name length plus `approx_payload_len()`), not a hard cap on
  serialized byte size — `serde_json`'s externally tagged per-value wrapper
  is never counted. `EventRegistry`'s duplicate-field rejection
  (`RegistryError::DuplicateField`) is what stops that approximation from
  being gamed by many repetitions of one cheap field; do not remove it
  without an equivalent replacement.
- `EventFieldValue::approx_payload_len` must charge a `Private`/`Secret`
  value's *serialized redacted* length, never its plaintext length — the
  latter reopens a length side channel through `max_payload_bytes`
  admission/truncation. `PrivateValue::approx_serialized_len` and
  `SecretField::approx_serialized_len` are the single source of truth for
  this, sharing the exact string-building logic their `Serialize` impls use
  so the two cannot drift apart; do not reintroduce a second, independent
  length calculation.
- `OperationIdAllocator` is deliberately not `Clone`: cloning would copy
  `next_sequence`, so the original and the clone would mint the identical
  next id. Do not re-derive `Clone` on it.
- `RecorderBounds::max_operations` rejects a brand-new operation rather than
  evicting an existing one when the whole-recorder cap is reached; do not
  change this to an eviction strategy without a strong reason — silently
  discarding a complete, otherwise-valid trace is exactly what this crate's
  truncation-marker design exists to avoid.
- `tests/dependency_contract.rs` runs `cargo tree --target all`, not the
  host-default target; do not drop `--target all` when touching that test,
  since a target-specific dependency would otherwise evade the allowlist on
  every platform except the one it targets.

## Claim boundary

This crate makes the `operation_trace.v1` vocabulary exist, makes an
operation's identity/kind/parent one value a caller threads through a call
(`OperationContext`) instead of a hand-wired triple, and makes violations of
the contract (unknown kinds, bad privacy tiers, duplicate declared fields,
duplicate terminals, parent or kind conflicts, parent cycles, silent
truncation, leaked private/secret content, unbounded attached references)
detectable by construction and by test. It does **not** make any real
operation traceable: no subsystem calls into this crate, so today it
instruments nothing. It does not retire, migrate, or even reference the five
existing operation-identity namespaces it was written to eventually
correlate, and it does not interpret what an attached `IdentityRef` denotes
— that migration, and the semantics of any particular reference, belong to a
later, separate claim.

This crate's privacy claim is narrower than "cannot leak" in two specific,
disclosed ways. First, the three-tier redaction guarantee
(`FieldPrivacy`/`PrivateValue`/`SecretField`) covers `EventFieldValue` only;
`SessionId` is validated for hygiene, not privacy, and is embedded verbatim
into `OperationId::as_wire` and into `RecordError::Display` text — a caller
must never pass a sensitive value as a session label. Second, this crate
proves its own dependency closure contains no hash function and that no API
here folds a value into a digest — it does not, and cannot, prove that no
caller anywhere can compute a digest over an exposed identity string in safe
Rust; keeping ephemeral or redacted identity out of a durable digest remains
a consumer-side contract obligation.

"Bounded in-memory recorder" is now a claim about the whole recorder, not
only about one operation's own trace: `RecorderBounds::max_events` and
`max_payload_bytes` still bound one operation, but `max_operations` bounds
the recorder's operation table as a whole, rejecting (not evicting) a
brand-new operation once reached. `max_payload_bytes` remains an approximate
budget, not a byte-exact serialized-size guarantee (see "Invariants"); it is
now, additionally, charged against each value's *redacted serialized*
length rather than a `Private`/`Secret` value's plaintext length, closing a
length side channel the plaintext-based accounting previously opened
through `RecordOutcome` itself.
