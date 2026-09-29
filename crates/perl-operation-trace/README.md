# perl-operation-trace

The versioned `operation_trace.v1` contract: ephemeral operation identity, a
closed operation-kind vocabulary, a context that threads that identity
across call/async/background boundaries, a bounded structured-event model
with explicit privacy classification, and an in-memory recorder that
enforces terminal-state, ordering, and budget invariants.

**No subsystem is instrumented by this crate.** It defines the vocabulary;
nothing wires it into the LSP, DAP, workspace, or subprocess runtime yet.

## Why this crate exists

Five independent, uncoordinated "operation identity" vocabularies already
exist in this repository and cannot be correlated with each other:

- `OperationId` in `perl-subprocess-runtime` — a `String` correlation label.
- `ParserOperationId` in `perl-parser-core` — a process-local `AtomicU64`.
- `WorkspaceRuntimeOperationId` in `perl-workspace`.
- `ReachabilityOperationKind` in `perl-semantic-facts`.
- The wire `operationId` field in `perl-dap`.

This crate does **not** migrate or retire any of them. It defines the
vocabulary they can later be correlated through.

## Ephemeral, not durable — the critical contrast

[`perl-source-identity`](../perl-source-identity) mints **durable,
content-addressed, semantic** identity: a `sha256:`-prefixed wire form where
fixed inputs always produce a byte-identical id, meant to survive across
machines, checkouts, and time.

This crate mints the **opposite**: `OperationId` is ephemeral, non-semantic,
session-local correlation identity. It carries no content, is meaningful
only for the lifetime of one recording session, and its wire form
deliberately never uses the `sha256:` prefix, so a reader cannot mistake it
for durable identity.

An `OperationId` must **never** enter a durable digest, fingerprint, or
stable entity id. What is actually proven, and no more: this crate's own
dependency closure contains no hash function (see
`tests/dependency_contract.rs`, which forbids `sha2`, fail-closed), and no
API in this crate folds an `OperationId` into a digest or fingerprint. A
downstream caller can still compute a hash in ordinary safe Rust over
`OperationId::as_wire`'s exposed string — this crate's dependency closure
does not, and cannot, prevent that. Keeping ephemeral identity out of a
durable digest is therefore a contract obligation this crate places on its
consumers, not a property this crate enforces for them.

## Deterministic by construction

`SessionId` is always caller-supplied: there is no `SessionId::new_random()`.
`OperationIdAllocator` mints a strictly deterministic sequence given a
session — no hidden global counter, no ambient entropy (wall-clock time,
PID, randomness). A fixture that supplies a fixed session gets a fixed id
sequence every time, which is what makes `operation_trace.v1` fixtures
reproducible (#4853).

`OperationIdAllocator::next()` returns `Option<OperationId>`: `None` once
the per-session `u64` sequence space is exhausted, rather than silently
repeating the final id (a `2^64`-operation session is unreachable in
practice, but the fix is a checked increment either way — a defined `None`
beats an undefined collision). The allocator is also deliberately not
`Clone`: cloning would copy its current sequence position, so the original
and the clone would each mint the identical next id.

## Wire formats

| Type | Wire format |
|---|---|
| `OperationId` | `op:<session>:<sequence>` |

The session component may itself contain `:`; parsing splits on the *last*
colon, which is safe because the sequence component is always the canonical
unsigned-decimal rendering of a `u64` and therefore never contains one. A
non-canonical sequence (a leading zero, a `+` sign) is rejected, not
normalized — one id has exactly one wire spelling.

**`SessionId` is outside the privacy model below.** The session component
above is the `SessionId` label embedded verbatim, with no redaction tier
applied. `SessionId::new` validates for hygiene only — non-blank, no ASCII
control character, at most `MAX_SESSION_ID_BYTES` (128) bytes — and that
validation is a log/output-injection guard, not a privacy guarantee: the
label also appears verbatim in several `RecordError::Display` messages.
Never pass a value that would need `Private`- or `Secret`-tier treatment as
a session label.

`OperationTraceSchemaVersion` gates the whole snapshot: an unsupported
`operation_trace` schema version is a serde decode error, not a value a
consumer must remember to check.

## Privacy: three tiers, no fingerprinting

`EventFieldValue` carries three privacy tiers, modeled on
`perl-subprocess-runtime`'s process-identity module
(`PrivatePath`/`PrivateBytes` vs. `SecretValue`):

| Tier | Type | `Debug`/`Serialize` | Use for |
|---|---|---|---|
| Public | `Integer`, `Boolean`, `PublicString` | verbatim | non-sensitive data |
| Private | `PrivateValue` | `<redacted:N bytes>` | high-entropy content (host paths, source text) |
| Secret | `SecretField` | `<redacted>` (no length) | low-entropy secrets (tokens, passwords) |

`Private` discloses a byte length because distinguishing two different
high-entropy values by size is useful and safe — a host path is the working
example. A line of Perl source text is deliberately **not** offered as a
`Private` example: real source lines are frequently low-entropy (`}`, `1;`,
`);`, blank), and low entropy is exactly what a byte length alone can
identify. This crate's own registry classifies `source_line` as `Secret`,
not `Private`, for exactly that reason. `Secret` discloses nothing, not even
a length, because a low-entropy secret's byte count itself narrows a guess
(`<redacted:4 bytes>` all but announces a PIN).

**This crate does not fold either non-public tier into a digest itself, and
cannot enforce that a caller won't.** What is proven: this crate's own
dependency closure contains no hash function (`tests/dependency_contract.rs`
forbids `sha2`, fail-closed), no API here folds a value into a digest or
fingerprint, and `tests/negative_control.rs` proves recorded event payloads
are independent of which operation recorded them. What is *not* proven:
absence of `sha2` does not make hashing unreachable in any absolute sense —
a digest can be implemented in safe Rust with no dependency at all, and
`OperationId::as_wire` deliberately exposes the identity as a string a
caller can hash. `perl-subprocess-runtime`'s fingerprinted tier additionally
exposes a digest computed and owned by that crate; if a consumer here needs
a fingerprinted value, it owns computing and carrying that digest itself —
this crate's dependency closure only proves its own code does not.

**`SessionId` also has no privacy tier at all** (not "private" or "secret" —
none). See "Wire formats" above.

The redaction holds through `serde_json::to_string`, not only through
`Debug` — see `tests/redaction.rs`.

## Context propagation

`OperationContext` is the one value a caller threads across a call, async,
or background boundary: an operation's own id, its optional parent, its
`OperationKind`, and any attached, opaque identity references from other
owning crates (`IdentityRef`, tagged by the closed `IdentityRefKind`:
`Source`/`Environment`/`Fact`/`Process`/`Receipt`/`Generation`, capped at
`MAX_ATTACHED_IDENTITY_REFS`). `OperationContext::root` starts a parentless
context; `parent_ctx.child(operation, kind)` derives a child whose parent is
always the exact operation the method was called on — there is no
constructor that takes an independently chosen `OperationId` as "the
parent", so an unrelated parent cannot be passed by mistake through the
ergonomic API. `OperationRecorder::record` takes a `&OperationContext`, so
`OperationKind` is not exported decoration: it is what every recorded
operation actually carries, visible on `RecordedOperation::kind`.

This crate stores an attached `IdentityRef` opaquely and asserts nothing
about what it denotes — the owning producer (e.g. `perl-source-identity` for
`IdentityRefKind::Source`) remains the authority for that.

## Event registry

`EventRegistry` declares, per closed `OperationEventKind`, which fields are
required and which privacy tier each declared field must be supplied at.
Validation rejects a missing required field, an undeclared field, a declared
field supplied at the wrong privacy tier — in either direction, so a
`Secret`-declared field cannot be smuggled in as `Private` either (`Private`
would still leak a byte length) — and the same declared field name supplied
more than once (`RegistryError::DuplicateField`).

## Recorder invariants

`OperationRecorder` is an in-memory, per-process recorder that enforces:

- **exactly one `Terminal` event per operation** — a duplicate, or any event
  after `Terminal`, is a typed `RecordError`, never silently accepted;
- **honest non-proof** — an operation with no `Terminal` event reports
  `TerminalState::NotProven` from a query, never success and never a panic;
- **no self-parent, no parent cycle** — both are typed `RecordError`s,
  detected before either could be stored;
- **one kind and one parent per operation** — a later `OperationContext`
  disagreeing with either is a typed `RecordError` (`KindConflict` /
  `ParentConflict`), never a silent overwrite;
- **explicit truncation** — hitting `RecorderBounds::max_events` or
  `max_payload_bytes` records an observable truncation marker in place of
  the dropped event, rather than silently shortening the trace. A silently
  dropped event is indistinguishable from an event that never happened,
  which would make the trace lie about what occurred. `max_events` counts
  *admitted* events only, never the marker itself, so one operation's
  snapshot can hold up to `max_events + 1` entries; `max_events = 0` is
  well-defined (zero events admitted, the first call yields only the
  marker);
- **a whole-recorder operation cap** — `RecorderBounds::max_operations`
  bounds the number of *distinct* operations this recorder retains at all,
  not just one operation's own trace. Once reached, a **new** operation's
  first `record` call is rejected (`RecordError::TooManyOperations`) rather
  than evicting an existing operation — eviction would discard a complete,
  otherwise-valid trace, exactly the silent loss truncation markers exist to
  avoid. An operation established before the cap was hit keeps accepting
  events afterward. Rejections are counted in both
  `OperationRecorder::operations_rejected_by_cap` and
  `OperationTraceSnapshot::operations_rejected_by_cap` — the recorder-wide
  counterpart to a per-operation truncation marker, since a rejected new
  operation has no per-operation record to attach a marker to;
- **registry validation before operation creation** — a registry-invalid
  event is rejected before its operation is even looked up, so it never
  establishes a kind or parent, is never counted by `operation_count`, and
  never consumes `max_operations` capacity.

A disabled recorder (`OperationRecorder::disabled()`) accepts every call and
stores nothing, so callers do not need an `if enabled` branch around their
recording call sites.

**`max_payload_bytes` is an approximate retention budget, not a hard cap on
serialized byte size.** It sums each field's name length plus its
`approx_payload_len()` — what that value actually *serializes to*: a public
value's own byte length, or a `Private`/`Secret` value's *redacted*
serialized length, **never** the private or secret plaintext's own length.
(Charging a `Secret` value's plaintext length used to make whether an event
was admitted or truncated depend on the secret's length — a real length
side channel through the one tier designed to disclose no length at all;
fixed by charging every `Secret` a fixed constant and every `Private` its
rendered `<redacted:N bytes>` length instead.) This still does not match
`serde_json`'s actual externally tagged encoding (its per-value wrapper,
quoting, and punctuation are never counted). `EventRegistry` rejecting a
repeated declared field name (`RegistryError::DuplicateField`) is what keeps
this approximation from being gamed: before that rejection existed, 500
repetitions of one cheap field passed the budget while the actual serialized
trace overshot it by 4.57x.

## Non-goals (deferred to future consumers)

- Migrating or retiring any of the five existing operation-identity
  namespaces.
- Instrumenting any subsystem (LSP, DAP, workspace, subprocess runtime).
- Persisting a trace anywhere other than in-memory.
- Fingerprinting private or secret field values *within this crate's own
  API surface*. This crate's dependency closure has no hash function and no
  API here folds a value into a digest — that does not make fingerprinting
  unreachable in any absolute sense (a caller can hash a value it already
  holds, or `OperationId::as_wire`'s exposed string, in plain safe Rust);
  see "Privacy" above.
- A redaction guarantee for `SessionId` — see "Wire formats" above.
- Interpreting what an attached `IdentityRef` denotes — it is opaque to this
  crate; the owning producer remains the authority.

## Quick start

```rust
use perl_operation_trace::{
    OperationContext, OperationEvent, OperationEventKind, OperationIdAllocator, OperationKind,
    OperationOutcome, OperationRecorder, RecorderBounds, SessionId, TerminalState,
};

let session = SessionId::new("fixture-session").expect("non-blank literal");
let mut allocator = OperationIdAllocator::new(session);
let context = OperationContext::root(
    allocator.next().expect("fresh allocator has plenty of remaining sequence numbers"),
    OperationKind::TestRun,
);

let mut recorder = OperationRecorder::new(RecorderBounds::new(64, 4096, 100));
recorder
    .record(&context, OperationEvent::new(OperationEventKind::Admitted))
    .expect("admitted event is valid");
recorder
    .record(&context, OperationEvent::terminal(OperationOutcome::Completed))
    .expect("terminal event is valid");

assert_eq!(
    recorder.terminal_state(context.operation()),
    TerminalState::Proven(OperationOutcome::Completed)
);
```

## License

Licensed under either of Apache License, Version 2.0 or MIT License at your
option.
