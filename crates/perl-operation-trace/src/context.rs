//! `OperationContext`: the one value a caller threads across a call, async,
//! or background boundary instead of a loose `(OperationId,
//! Option<ParentOperationId>)` pair passed by hand.
//!
//! A context bundles an operation's own id, its optional parent link, its
//! [`crate::OperationKind`] classification, and any attached, opaque
//! identity references from other owning crates. It is the thing
//! [`crate::OperationRecorder::record`] actually takes.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ids::{OperationId, ParentOperationId};
use crate::kind::OperationKind;

/// Closed vocabulary of what an attached [`IdentityRef`] refers to.
///
/// This crate cannot depend on the crates that own these identities (that
/// would reintroduce exactly the upward dependencies
/// `tests/dependency_contract.rs` forbids), so an attached reference is
/// stored as an opaque, validated string tagged with which owning domain it
/// belongs to. This crate asserts nothing about what the string denotes,
/// whether it is well-formed for its owning domain, or whether it is still
/// current — the owning producer (e.g. `perl-source-identity` for
/// [`Self::Source`]) remains the sole authority for that. A closed enum
/// here only prevents a typo'd domain tag from silently becoming a new,
/// unreviewed domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IdentityRefKind {
    /// A `perl-source-identity`-owned source/project/root/revision reference.
    Source,
    /// An environment/configuration snapshot reference.
    Environment,
    /// A compiler-facts or semantic-fact reference.
    Fact,
    /// A `perl-subprocess-runtime`-owned process/plan reference.
    Process,
    /// A receipt reference.
    Receipt,
    /// A freshness/generation cursor reference.
    Generation,
}

impl fmt::Display for IdentityRefKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Source => "source",
            Self::Environment => "environment",
            Self::Fact => "fact",
            Self::Process => "process",
            Self::Receipt => "receipt",
            Self::Generation => "generation",
        };
        f.write_str(s)
    }
}

/// An opaque, validated reference to identity owned by another crate.
///
/// This crate stores the value verbatim and interprets nothing about it
/// beyond "not blank" — no parsing, no wire-prefix validation, no
/// authority. It is deliberately not the real `perl-source-identity` id (or
/// any other owner's real id) type: taking a real dependency on that type
/// would pull this crate's upward-dependency closure right back open, which
/// is exactly what [`IdentityRefKind`] plus this opaque wrapper avoids.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct IdentityRef(String);

/// Why a candidate [`IdentityRef`] was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityRefError {
    /// The reference was empty or contained only whitespace.
    Blank,
}

impl fmt::Display for IdentityRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blank => f.write_str("identity reference must not be blank"),
        }
    }
}

impl std::error::Error for IdentityRefError {}

impl IdentityRef {
    /// Construct an identity reference from a caller-supplied opaque value.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityRefError::Blank`] if `value` is empty or
    /// whitespace-only.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityRefError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(IdentityRefError::Blank);
        }
        Ok(Self(value))
    }

    /// Borrow the opaque reference text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdentityRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for IdentityRef {
    /// Validating deserialization: a blank reference is rejected at the
    /// serde boundary rather than carried into the type system unchecked.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

/// The maximum number of [`IdentityRef`]s one [`OperationContext`] may carry.
///
/// A bound exists here for the same reason [`crate::RecorderBounds`] exists
/// on the recorder: an unbounded attachment list on a single context would
/// give a caller (or a bug) an unbounded-memory vector with no observable
/// signal, which is exactly the silent-growth failure mode this crate's
/// bounds are meant to make impossible.
pub const MAX_ATTACHED_IDENTITY_REFS: usize = 8;

/// A rejected [`OperationContext`] construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    /// [`OperationContext::attach`] was called after
    /// [`MAX_ATTACHED_IDENTITY_REFS`] references were already attached.
    TooManyAttachedIdentityRefs {
        /// The bound that was reached.
        max: usize,
    },
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyAttachedIdentityRefs { max } => {
                write!(f, "cannot attach more than {max} identity references to one context")
            }
        }
    }
}

impl std::error::Error for ContextError {}

/// The one value a caller threads across a call, async, or background
/// boundary: an operation's own id, its optional parent, its
/// [`OperationKind`], and any attached opaque identity references.
///
/// # Construction
///
/// [`Self::root`] starts a context with no parent. [`Self::child`] derives a
/// new context whose parent is *exactly* the context it was called on — the
/// parent link is never a caller-supplied, independently chosen
/// [`OperationId`], so a caller cannot accidentally attach an unrelated
/// parent by passing the wrong value to some `parent: OperationId`
/// parameter. (A caller can still construct a genuine self-parent by
/// passing a child context's own future id back into `child` for another
/// operation with that same id; [`crate::OperationRecorder`] still checks
/// for this at record time — see its `SelfParent`/`ParentCycle` errors —
/// because a type-level guard here cannot see the recorder's whole graph.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationContext {
    operation: OperationId,
    parent: Option<ParentOperationId>,
    kind: OperationKind,
    attached: Vec<(IdentityRefKind, IdentityRef)>,
}

impl OperationContext {
    /// Start a context for a root (parentless) operation.
    #[must_use]
    pub fn root(operation: OperationId, kind: OperationKind) -> Self {
        Self { operation, parent: None, kind, attached: Vec::new() }
    }

    /// Derive a context for a child operation of this one.
    ///
    /// The child's parent is always this context's own operation id — there
    /// is no way to pass a different, unrelated parent through this
    /// constructor. The child starts with no attached identity references
    /// (attachments are per-operation, not inherited).
    #[must_use]
    pub fn child(&self, operation: OperationId, kind: OperationKind) -> Self {
        Self {
            parent: Some(ParentOperationId::new(self.operation.clone())),
            operation,
            kind,
            attached: Vec::new(),
        }
    }

    /// Attach an opaque identity reference, builder-style.
    ///
    /// # Errors
    ///
    /// Returns [`ContextError::TooManyAttachedIdentityRefs`] once
    /// [`MAX_ATTACHED_IDENTITY_REFS`] references are already attached.
    pub fn attach(
        mut self,
        kind: IdentityRefKind,
        reference: IdentityRef,
    ) -> Result<Self, ContextError> {
        if self.attached.len() >= MAX_ATTACHED_IDENTITY_REFS {
            return Err(ContextError::TooManyAttachedIdentityRefs {
                max: MAX_ATTACHED_IDENTITY_REFS,
            });
        }
        self.attached.push((kind, reference));
        Ok(self)
    }

    /// This context's own operation id.
    #[must_use]
    pub fn operation(&self) -> &OperationId {
        &self.operation
    }

    /// This context's parent, if any.
    #[must_use]
    pub fn parent(&self) -> Option<&ParentOperationId> {
        self.parent.as_ref()
    }

    /// This context's operation-kind classification.
    #[must_use]
    pub fn kind(&self) -> OperationKind {
        self.kind
    }

    /// This context's attached identity references, in attachment order.
    #[must_use]
    pub fn attached(&self) -> &[(IdentityRefKind, IdentityRef)] {
        &self.attached
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::session::SessionId;

    fn op(session: &str, sequence: u64) -> OperationId {
        OperationId::new(SessionId::new(session).unwrap(), sequence)
    }

    // ── IdentityRefKind ───────────────────────────────────────────────────

    #[test]
    fn identity_ref_kind_serde_round_trip_is_lossless() {
        let all = [
            IdentityRefKind::Source,
            IdentityRefKind::Environment,
            IdentityRefKind::Fact,
            IdentityRefKind::Process,
            IdentityRefKind::Receipt,
            IdentityRefKind::Generation,
        ];
        for kind in all {
            let json = serde_json::to_string(&kind).unwrap();
            let back: IdentityRefKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn identity_ref_kind_unknown_fails_closed() {
        for bad in ["\"NotAKind\"", "\"\"", "\"source\"", "0"] {
            assert!(
                serde_json::from_str::<IdentityRefKind>(bad).is_err(),
                "must reject {bad}, not silently decode a default variant"
            );
        }
    }

    // ── IdentityRef ───────────────────────────────────────────────────────

    #[test]
    fn identity_ref_rejects_blank() {
        assert!(IdentityRef::new("").is_err());
        assert!(IdentityRef::new("   ").is_err());
    }

    #[test]
    fn identity_ref_accepts_and_round_trips_non_blank() {
        let r = IdentityRef::new("src:sha256:abc").unwrap();
        assert_eq!(r.as_str(), "src:sha256:abc");
        let json = serde_json::to_string(&r).unwrap();
        let back: IdentityRef = serde_json::from_str(&json).expect("valid reference must parse");
        assert_eq!(r, back);
    }

    #[test]
    fn identity_ref_deserialization_rejects_blank() {
        assert!(serde_json::from_str::<IdentityRef>("\"\"").is_err());
        assert!(serde_json::from_str::<IdentityRef>("\"   \"").is_err());
    }

    // ── OperationContext construction ─────────────────────────────────────

    #[test]
    fn root_has_no_parent_and_the_given_kind() {
        let ctx = OperationContext::root(op("s1", 0), OperationKind::TestRun);
        assert_eq!(ctx.operation(), &op("s1", 0));
        assert_eq!(ctx.parent(), None);
        assert_eq!(ctx.kind(), OperationKind::TestRun);
        assert!(ctx.attached().is_empty());
    }

    #[test]
    fn child_parent_is_exactly_the_parent_contexts_operation() {
        let root = OperationContext::root(op("s1", 0), OperationKind::WorkspaceIndexing);
        let child = root.child(op("s1", 1), OperationKind::LspRequest);
        assert_eq!(child.operation(), &op("s1", 1));
        assert_eq!(child.parent().map(ParentOperationId::operation_id), Some(&op("s1", 0)));
        assert_eq!(child.kind(), OperationKind::LspRequest);
    }

    #[test]
    fn child_does_not_inherit_attached_references() {
        let root = OperationContext::root(op("s1", 0), OperationKind::TestRun)
            .attach(IdentityRefKind::Source, IdentityRef::new("src:1").unwrap())
            .unwrap();
        let child = root.child(op("s1", 1), OperationKind::TestRun);
        assert!(child.attached().is_empty());
    }

    // ── Attachment bound ──────────────────────────────────────────────────

    #[test]
    fn attach_up_to_the_bound_succeeds() {
        let mut ctx = OperationContext::root(op("s1", 0), OperationKind::TestRun);
        for i in 0..MAX_ATTACHED_IDENTITY_REFS {
            ctx = ctx
                .attach(IdentityRefKind::Fact, IdentityRef::new(format!("fact:{i}")).unwrap())
                .expect("within bound");
        }
        assert_eq!(ctx.attached().len(), MAX_ATTACHED_IDENTITY_REFS);
    }

    #[test]
    fn attach_beyond_the_bound_is_rejected() {
        let mut ctx = OperationContext::root(op("s1", 0), OperationKind::TestRun);
        for i in 0..MAX_ATTACHED_IDENTITY_REFS {
            ctx = ctx
                .attach(IdentityRefKind::Fact, IdentityRef::new(format!("fact:{i}")).unwrap())
                .expect("within bound");
        }
        let err = ctx.attach(IdentityRefKind::Fact, IdentityRef::new("one-too-many").unwrap());
        assert_eq!(
            err.unwrap_err(),
            ContextError::TooManyAttachedIdentityRefs { max: MAX_ATTACHED_IDENTITY_REFS }
        );
    }

    #[test]
    fn attach_preserves_order() {
        let ctx = OperationContext::root(op("s1", 0), OperationKind::TestRun)
            .attach(IdentityRefKind::Source, IdentityRef::new("a").unwrap())
            .unwrap()
            .attach(IdentityRefKind::Environment, IdentityRef::new("b").unwrap())
            .unwrap();
        let names: Vec<&str> = ctx.attached().iter().map(|(_, r)| r.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }
}
