#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! `perl-operation-trace` — the versioned `operation_trace.v1` contract:
//! ephemeral operation identity, a closed operation-kind vocabulary, a
//! context that threads that identity across call/async/background
//! boundaries, a bounded structured-event model with explicit privacy
//! classification, and an in-memory recorder enforcing terminal-state,
//! ordering, and budget invariants.
//!
//! # Why this crate exists
//!
//! Five independent, uncoordinated "operation identity" vocabularies already
//! exist on `main` and cannot be correlated with each other:
//!
//! - `OperationId` in `perl-subprocess-runtime` — a `String` correlation
//!   label, produced by a `correlation_id!` macro, paired with a closed
//!   `OwnerDomain` enum and a `PrivateBytes` redaction wrapper.
//! - `ParserOperationId` in `perl-parser-core` — a process-local `AtomicU64`
//!   counter.
//! - `WorkspaceRuntimeOperationId` in `perl-workspace`.
//! - `ReachabilityOperationKind` in `perl-semantic-facts`.
//! - The wire `operationId` field in `perl-dap`.
//!
//! This crate does **not** migrate or retire any of them, and it
//! instruments no subsystem itself. It defines the vocabulary they can later
//! be correlated through.
//!
//! # Ephemeral, not durable — the critical contrast
//!
//! `perl-source-identity` mints **durable, content-addressed, semantic**
//! identity: fixed inputs always produce a byte-identical `sha256:`-prefixed
//! id meant to survive across machines, checkouts, and time.
//!
//! This crate mints the **opposite**: [`OperationId`] is ephemeral,
//! non-semantic, session-local correlation identity. It carries no content,
//! is meaningful only for the lifetime of one recording session, and its
//! wire form deliberately never uses a `sha256:` prefix, so a reader cannot
//! mistake it for durable identity. An `OperationId` must **never** enter a
//! durable digest, fingerprint, or stable entity id. What this crate
//! actually proves, and no more: its own dependency closure contains no hash
//! function (see `tests/dependency_contract.rs`, which forbids `sha2`,
//! fail-closed), and no API in this crate folds an `OperationId` into a
//! digest or fingerprint. A downstream caller can still compute a hash over
//! `OperationId::as_wire`'s exposed string in ordinary safe Rust — no
//! dependency closure can prevent that — so keeping ephemeral identity out
//! of a durable digest is a contract obligation on consumers, not something
//! this crate enforces for them. See `crate::privacy` for the full
//! accounting.
//!
//! # Deterministic by construction
//!
//! [`SessionId`] is always caller-supplied — there is no
//! `SessionId::new_random()`. [`OperationIdAllocator`] mints a strictly
//! deterministic sequence from a given session: no hidden global counter, no
//! ambient entropy (wall-clock time, PID, randomness). A fixture that
//! supplies a fixed session gets a fixed id sequence every time, which is
//! what makes `operation_trace.v1` fixtures reproducible (#4853).
//!
//! # Context propagation
//!
//! [`OperationContext`] is the one value a caller threads across a call,
//! async, or background boundary instead of a loose `(OperationId,
//! Option<ParentOperationId>)` pair passed by hand: it bundles an
//! operation's own id, its optional parent, its [`OperationKind`]
//! classification, and any attached, opaque [`IdentityRef`]s from other
//! owning crates. [`OperationRecorder::record`] takes a `&OperationContext`,
//! not the loose triple, so `OperationKind` is not exported decoration — it
//! is what every recorded operation actually carries.
//!
//! # Privacy
//!
//! [`EventFieldValue`] carries three privacy tiers modeled on
//! `perl-subprocess-runtime`'s process-identity module: public, private
//! ([`PrivateValue`] — redacted, byte length disclosed) and secret
//! ([`SecretField`] — redacted, nothing disclosed, not even a length). This
//! crate does not fold either non-public tier into a digest itself — see
//! `crate::privacy` for exactly what is and is not proven about that.
//!
//! **This three-tier model covers [`EventFieldValue`] only.** [`SessionId`]
//! is validated for hygiene (non-blank, no ASCII control character, bounded
//! length) but carries no redaction guarantee whatsoever: it is embedded
//! verbatim into [`OperationId::as_wire`] (that type's `Serialize` impl) and
//! into several [`RecordError`] `Display` variants. See `crate::session`'s
//! module docs before ever constructing a `SessionId` from a value that
//! would need `Private`- or `Secret`-tier treatment if it were an event
//! field.
//!
//! # No subsystem is instrumented here
//!
//! This crate is a self-contained contract substrate. No existing crate's
//! behavior changes when this crate is added to the workspace.
//!
//! # Quick start
//!
//! ```
//! use perl_operation_trace::{
//!     OperationContext, OperationEvent, OperationEventKind, OperationIdAllocator,
//!     OperationKind, OperationOutcome, OperationRecorder, RecorderBounds, SessionId,
//!     TerminalState,
//! };
//!
//! let session = SessionId::new("fixture-session").expect("non-blank literal");
//! let mut allocator = OperationIdAllocator::new(session);
//! let context = OperationContext::root(
//!     allocator.next().expect("fresh allocator has plenty of remaining sequence numbers"),
//!     OperationKind::TestRun,
//! );
//!
//! let mut recorder = OperationRecorder::new(RecorderBounds::new(64, 4096, 100));
//! recorder
//!     .record(&context, OperationEvent::new(OperationEventKind::Admitted))
//!     .expect("admitted event is valid");
//! recorder
//!     .record(&context, OperationEvent::terminal(OperationOutcome::Completed))
//!     .expect("terminal event is valid");
//!
//! assert_eq!(
//!     recorder.terminal_state(context.operation()),
//!     TerminalState::Proven(OperationOutcome::Completed)
//! );
//! ```

mod context;
mod event;
mod ids;
mod kind;
mod privacy;
mod recorder;
mod registry;
mod schema;
mod session;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use context::{ContextError, IdentityRef, IdentityRefError, IdentityRefKind, OperationContext};

pub use context::MAX_ATTACHED_IDENTITY_REFS;

pub use event::{
    EventFieldValue, OUTCOME_FIELD, OperationEvent, OperationEventKind, OperationOutcome,
};

pub use ids::{OperationId, OperationIdAllocator, ParentOperationId};

pub use kind::OperationKind;

pub use privacy::{FieldPrivacy, PrivateValue, SecretField};

pub use recorder::{
    OperationRecorder, OperationTraceSnapshot, RecordError, RecordOutcome, RecordedEventEntry,
    RecordedOperation, RecorderBounds, TerminalState, TruncationReason,
};

pub use registry::{EventRegistry, RegistryError};

pub use schema::{OperationTraceSchemaVersion, SCHEMA_VERSION_V1};

pub use session::{MAX_SESSION_ID_BYTES, SessionId, SessionIdError};
