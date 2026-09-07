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
stable entity id. This crate has no hash-function dependency at all (see
`tests/dependency_contract.rs`, which forbids `sha2`), so folding an
`OperationId` into a digest is not merely avoided here — it is unreachable.

## Deterministic by construction

`SessionId` is always caller-supplied: there is no `SessionId::new_random()`.
`OperationIdAllocator` mints a strictly deterministic sequence given a
session — no hidden global counter, no ambient entropy (wall-clock time,
PID, randomness). A fixture that supplies a fixed session gets a fixed id
sequence every time, which is what makes `operation_trace.v1` fixtures
reproducible (#4853).

## Wire formats

| Type | Wire format |
|---|---|
| `OperationId` | `op:<session>:<sequence>` |

The session component may itself contain `:`; parsing splits on the *last*
colon, which is safe because the sequence component is always the canonical
unsigned-decimal rendering of a `u64` and therefore never contains one. A
non-canonical sequence (a leading zero, a `+` sign) is rejected, not
normalized — one id has exactly one wire spelling.

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
high-entropy values by size is useful and safe. `Secret` discloses nothing,
not even a length, because a low-entropy secret's byte count itself narrows
a guess (`<redacted:4 bytes>` all but announces a PIN).

**This crate never computes or stores a digest of either non-public tier.**
`perl-subprocess-runtime`'s fingerprinted tier additionally exposes a digest
so two different high-entropy inputs stay distinguishable without exposure —
that requires a hash function and is a producer-owned concern that
deliberately stays out of this crate's dependency closure. If a consumer
needs a fingerprinted private value, it owns computing and carrying that
digest itself.

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
Validation rejects a missing required field, an undeclared field, and a
declared field supplied at the wrong privacy tier — in either direction, so
a `Secret`-declared field cannot be smuggled in as `Private` either (`Private`
would still leak a byte length).

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
  which would make the trace lie about what occurred.

A disabled recorder (`OperationRecorder::disabled()`) accepts every call and
stores nothing, so callers do not need an `if enabled` branch around their
recording call sites.

## Non-goals (deferred to future consumers)

- Migrating or retiring any of the five existing operation-identity
  namespaces.
- Instrumenting any subsystem (LSP, DAP, workspace, subprocess runtime).
- Persisting a trace anywhere other than in-memory.
- Fingerprinting private or secret field values.
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
let context = OperationContext::root(allocator.next(), OperationKind::TestRun);

let mut recorder = OperationRecorder::new(RecorderBounds::new(64, 4096));
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
