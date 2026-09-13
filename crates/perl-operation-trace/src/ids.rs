//! Ephemeral, non-semantic operation identity.
//!
//! See the crate-level docs for the full ephemeral-vs-durable contrast with
//! `perl-source-identity`. In short: an [`OperationId`] is minted from a
//! caller-supplied [`crate::SessionId`] and a per-session monotonic counter,
//! carries no content, and is meaningful only for the lifetime of one
//! recording session. It must never be folded into a durable digest,
//! fingerprint, or stable entity id. What is actually proven, and no more:
//! this crate's own dependency closure contains no hash function
//! (`tests/dependency_contract.rs` forbids `sha2`, fail-closed), and no API
//! in this crate folds an `OperationId` into a digest or fingerprint. A
//! downstream caller can still compute a hash in ordinary safe Rust over
//! [`OperationId::as_wire`]'s exposed string — this crate's dependency
//! closure cannot prevent that, and does not claim to. Keeping ephemeral
//! identity out of a durable digest is therefore a contract obligation this
//! crate places on its consumers, not a property this crate enforces for
//! them.
//!
//! [`OperationId::as_wire`] is also the boundary where [`crate::SessionId`]
//! leaves this crate's three-tier privacy model entirely: the session
//! component is embedded verbatim, with no redaction tier applied (see
//! `crate::session`'s module docs).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::session::SessionId;

const OPERATION_ID_PREFIX: &str = "op:";

/// Ephemeral, session-scoped correlation identity for one operation.
///
/// # Wire format
///
/// `op:<session>:<sequence>`. The session component may itself contain `:`;
/// parsing splits on the *last* colon, which is safe because the sequence
/// component is always the canonical unsigned-decimal rendering of a `u64`
/// and therefore never contains one. A non-canonical sequence rendering
/// (a leading zero, a `+` sign, whitespace) is rejected, not normalized, so
/// one id has exactly one wire spelling — the same rule
/// `perl-source-identity` applies to its `sha256:` bodies.
///
/// # This is not durable identity
///
/// Deliberately no `sha256:`-prefixed wire form, so a reader cannot mistake
/// this for `perl-source-identity`'s durable identity. Two different runs of
/// the same logical operation should be free to share every other property
/// and still mint different, unrelated `OperationId`s.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperationId {
    session: SessionId,
    sequence: u64,
}

impl OperationId {
    /// Construct an operation id directly from its parts.
    ///
    /// Prefer [`OperationIdAllocator`] in ordinary use; this constructor
    /// exists for tests and for callers reconstructing an id from parts they
    /// already know are valid.
    #[must_use]
    pub fn new(session: SessionId, sequence: u64) -> Self {
        Self { session, sequence }
    }

    /// The session this operation was minted under.
    #[must_use]
    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// The per-session sequence number.
    #[must_use]
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// The wire representation, e.g. `op:s1:0`.
    ///
    /// The session component is [`crate::SessionId`]'s label embedded
    /// **verbatim, with no privacy tier applied** — this is the boundary
    /// where a `SessionId` leaves this crate's three-tier privacy model
    /// entirely (see `crate::session`'s module docs). `SessionId::new`
    /// rejects blank, control-character-containing, and over-long labels for
    /// hygiene, but that validation carries no redaction guarantee: whatever
    /// string a caller supplies as a session label appears in this wire form
    /// unchanged, and this method is also `OperationId`'s `Serialize` impl.
    #[must_use]
    pub fn as_wire(&self) -> String {
        format!("{OPERATION_ID_PREFIX}{}:{}", self.session.as_str(), self.sequence)
    }

    /// Parse an operation id from its wire representation.
    ///
    /// Returns `None` for a missing `op:` prefix, a blank session component,
    /// a non-numeric sequence component, or a sequence component that is not
    /// the canonical decimal rendering of its own value.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        let rest = s.strip_prefix(OPERATION_ID_PREFIX)?;
        let (session_part, sequence_part) = rest.rsplit_once(':')?;
        let sequence: u64 = sequence_part.parse().ok()?;
        if sequence_part != sequence.to_string() {
            return None;
        }
        let session = SessionId::new(session_part).ok()?;
        Some(Self { session, sequence })
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_wire())
    }
}

impl Serialize for OperationId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_wire())
    }
}

impl<'de> Deserialize<'de> for OperationId {
    /// Validating deserialization: an ill-formed operation id is rejected at
    /// the serde boundary rather than carried into the type system unchecked.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_wire(&raw).ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"an operation id of the form `op:<session>:<sequence>`",
            )
        })
    }
}

/// A distinct wrapper for "the id of this operation's parent".
///
/// A parent link is type-distinct from an operation's own [`OperationId`] so
/// the two cannot be mixed up at a call site — passing an operation's own id
/// where a parent was expected is a type error, not a runtime surprise.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParentOperationId(OperationId);

impl ParentOperationId {
    /// Wrap an operation id as a parent reference.
    #[must_use]
    pub fn new(id: OperationId) -> Self {
        Self(id)
    }

    /// Borrow the wrapped operation id.
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        &self.0
    }

    /// Unwrap into the underlying operation id.
    #[must_use]
    pub fn into_operation_id(self) -> OperationId {
        self.0
    }
}

/// Deterministic, session-scoped [`OperationId`] allocator.
///
/// Given the same [`SessionId`], an allocator yields the same sequence of
/// ids across processes and runs: there is no hidden global counter and no
/// ambient entropy. This is what makes `operation_trace.v1` fixtures
/// reproducible (#4853) — a test constructs an allocator with a fixed
/// session and gets a fixed id sequence, every time.
///
/// # Exhaustion
///
/// The per-session sequence space is a `u64`: [`Self::next`] returns `None`
/// once it is exhausted, rather than silently repeating the final id. A
/// silent repeat would hand out the *same* [`OperationId`] to two logically
/// distinct operations — exactly the collision this crate's identity model
/// exists to prevent — and would also manufacture spurious
/// `KindConflict`/`ParentConflict`/`DuplicateTerminal` errors against
/// whichever unrelated operation happened to share the repeated id. `2^64`
/// operations in one session is unreachable in ordinary use, but the fix is
/// a checked, not saturating, increment, so the failure mode is a defined
/// `None` rather than an undefined collision.
///
/// # Deliberately not `Clone`
///
/// Cloning an allocator would copy its current `next_sequence`, so the
/// original and the clone would then each mint the *same* next id —
/// silently manufacturing exactly the identity collision this type exists
/// to prevent, just via cloning instead of via overflow. Do not re-derive
/// `Clone` here; the absence of the derive is the actual guard, since it
/// cannot be unit-tested (a missing trait impl is a compile-time property,
/// not a runtime one).
#[derive(Debug)]
pub struct OperationIdAllocator {
    session: SessionId,
    next_sequence: Option<u64>,
}

impl OperationIdAllocator {
    /// Start an allocator for the given session, minting from sequence `0`.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self { session, next_sequence: Some(0) }
    }

    /// The session this allocator mints ids under.
    #[must_use]
    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// Mint the next operation id in sequence.
    ///
    /// Returns `None` once this allocator's `u64` sequence space is
    /// exhausted (see "Exhaustion" above), and returns `None` on every call
    /// thereafter — it does not resume, wrap around, or panic.
    // Not an `Iterator`, even though this signature now matches
    // `Iterator::next` exactly: this allocator is a side-effecting minting
    // source keyed by one session, not a sequence meant to be composed with
    // iterator adapters, and the settled `operation_trace.v1` contract names
    // this method `next()` specifically. Its `None` means "sequence space
    // exhausted," not "ordinary end of sequence."
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<OperationId> {
        let sequence = self.next_sequence?;
        self.next_sequence = sequence.checked_add(1);
        Some(OperationId::new(self.session.clone(), sequence))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn session(label: &str) -> SessionId {
        SessionId::new(label).unwrap()
    }

    // ── Allocator determinism ─────────────────────────────────────────────

    #[test]
    fn allocator_yields_distinct_ids_within_a_session() {
        let mut allocator = OperationIdAllocator::new(session("s1"));
        let a = allocator.next().unwrap();
        let b = allocator.next().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.sequence(), 0);
        assert_eq!(b.sequence(), 1);
    }

    #[test]
    fn same_session_and_sequence_are_equal() {
        let a = OperationId::new(session("s1"), 3);
        let b = OperationId::new(session("s1"), 3);
        assert_eq!(a, b);
    }

    #[test]
    fn different_sessions_never_collide() {
        let mut alloc_a = OperationIdAllocator::new(session("s1"));
        let mut alloc_b = OperationIdAllocator::new(session("s2"));
        // Same sequence position, different session.
        let a = alloc_a.next().unwrap();
        let b = alloc_b.next().unwrap();
        assert_ne!(a, b, "same sequence under different sessions must not collide");
        assert_eq!(a.sequence(), b.sequence(), "sanity: both are sequence 0");
    }

    #[test]
    fn allocator_is_deterministic_given_the_same_session() {
        let mut a = OperationIdAllocator::new(session("fixture"));
        let mut b = OperationIdAllocator::new(session("fixture"));
        for _ in 0..5 {
            assert_eq!(a.next(), b.next());
        }
    }

    /// Constructing an allocator already at `u64::MAX` (a private-field
    /// struct literal is available here because this test module is a child
    /// of `ids`, not through any widened public API) and taking its final id
    /// must exhaust the sequence space with a defined `None`, not a
    /// `saturating_add`-style silent repeat of the last id.
    #[test]
    fn allocator_returns_none_once_the_sequence_space_is_exhausted() {
        let mut allocator =
            OperationIdAllocator { session: session("s1"), next_sequence: Some(u64::MAX) };
        let last = allocator.next();
        assert_eq!(last, Some(OperationId::new(session("s1"), u64::MAX)));
        assert_eq!(
            allocator.next(),
            None,
            "sequence space exhausted must yield None, not repeat the final id"
        );
        assert_eq!(allocator.next(), None, "must stay None on every subsequent call, not resume");
    }

    // ── Wire format ────────────────────────────────────────────────────────

    #[test]
    fn wire_round_trips() {
        let id = OperationId::new(session("s1"), 42);
        let wire = id.as_wire();
        assert_eq!(wire, "op:s1:42");
        assert_eq!(OperationId::from_wire(&wire), Some(id));
    }

    #[test]
    fn wire_allows_colons_inside_the_session_component() {
        let id = OperationId::new(session("sess:with:colons"), 5);
        let wire = id.as_wire();
        assert_eq!(wire, "op:sess:with:colons:5");
        assert_eq!(OperationId::from_wire(&wire), Some(id));
    }

    #[test]
    fn wire_rejects_malformed_input() {
        assert!(OperationId::from_wire("").is_none(), "empty");
        assert!(OperationId::from_wire("op:").is_none(), "no session/sequence body");
        assert!(OperationId::from_wire("op::5").is_none(), "blank session");
        assert!(OperationId::from_wire("op:s1:").is_none(), "empty sequence");
        assert!(OperationId::from_wire("op:s1:x").is_none(), "non-numeric sequence");
        assert!(OperationId::from_wire("op:s1:05").is_none(), "non-canonical leading zero");
        assert!(OperationId::from_wire("op:s1:+5").is_none(), "non-canonical sign");
        assert!(OperationId::from_wire("s1:5").is_none(), "missing op: prefix");
        assert!(OperationId::from_wire("op:s1").is_none(), "no sequence at all");
    }

    #[test]
    fn wire_never_uses_the_sha256_prefix() {
        let id = OperationId::new(session("s1"), 1);
        assert!(
            !id.as_wire().starts_with("sha256:"),
            "an OperationId wire form must never look like durable identity"
        );
        assert!(id.as_wire().starts_with("op:"));
    }

    #[test]
    fn display_matches_wire() {
        let id = OperationId::new(session("s1"), 7);
        assert_eq!(format!("{id}"), id.as_wire());
    }

    #[test]
    fn deserialization_is_validating() {
        let id = OperationId::new(session("s1"), 9);
        let json = serde_json::to_string(&id).expect("serialize");
        let back: OperationId = serde_json::from_str(&json).expect("valid wire must parse");
        assert_eq!(id, back);

        for bad in ["\"\"", "\"op:\"", "\"op::5\"", "\"op:s1:05\"", "\"not-an-id\""] {
            assert!(serde_json::from_str::<OperationId>(bad).is_err(), "must reject {bad}");
        }
    }

    // ── ParentOperationId ─────────────────────────────────────────────────

    #[test]
    fn parent_operation_id_round_trips() {
        let id = OperationId::new(session("s1"), 2);
        let parent = ParentOperationId::new(id.clone());
        assert_eq!(parent.operation_id(), &id);
        assert_eq!(parent.into_operation_id(), id);
    }

    #[test]
    fn parent_operation_id_is_type_distinct_from_operation_id() {
        // This is a compile-time property: `ParentOperationId` is a distinct
        // newtype, not a type alias, so this line only compiles because the
        // two are different types with an explicit conversion between them.
        let id = OperationId::new(session("s1"), 0);
        let parent: ParentOperationId = ParentOperationId::new(id.clone());
        let _back: OperationId = parent.into_operation_id();
    }
}
