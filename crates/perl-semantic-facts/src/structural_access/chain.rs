//! The ordered chain of structural access hops.

use serde::{Deserialize, Serialize};

use super::{
    STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION, STRUCTURAL_ACCESS_SCHEMA_TAG,
    StructuralAccessContractError, StructuralAccessHop, StructuralAccessSubject,
    StructuralHopOutcome, shape_carries, shape_is_decisive,
};
use crate::semantic_identity::SemanticIdentityFingerprint;

/// One complete ordered structural access, e.g. `$config->{groups}{staff}[0]`.
///
/// Order is the contract. Hops are held privately and exposed as a slice so a
/// consumer cannot reorder or drop one after validation, and every hop's
/// aggregate names its immediate predecessor by ordinal, so a chain whose
/// order is internally inconsistent is mechanically detectable rather than
/// merely improbable.
///
/// # What the predecessor link does not prove
///
/// The ordinal link is a coherence check, not an integrity mechanism. It
/// rejects a chain that contradicts itself — a hop naming a predecessor that
/// is not `ordinal - 1`, ordinals that are not dense and ascending, a reorder
/// or deletion that leaves either inconsistent. It cannot reject a deletion
/// that renumbers everything after it, and no self-contained validator could:
/// `$config->{a}{b}{c}` with `{b}` removed and `{c}` renumbered *is*
/// `$config->{a}{c}`, which an honest producer emits for real source.
/// Rejecting it would reject that source.
///
/// Binding each hop to a predecessor *fingerprint* would not close this
/// either. These digests carry no secret and the folding is public, so anyone
/// able to renumber an ordinal can recompute a digest just as cheaply; it
/// would add a field to every hop and buy no guarantee against an edit.
///
/// What distinguishes a shortened chain is therefore identity, not validity:
/// [`Self::fingerprint`] folds the subject and every hop in written order, so
/// a consumer holding the expected digest sees the substitution. A consumer
/// that needs to trust a chain arriving from a transport must compare it
/// against an expected identity; validation alone establishes that a record is
/// a possible access, never that it is the access that was meant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAccessChain {
    /// Contract schema version.
    schema_version: u32,
    /// Source, document, project, and root generation the chain holds under.
    subject: StructuralAccessSubject,
    /// Ordered hops, first written first.
    hops: Vec<StructuralAccessHop>,
}

impl StructuralAccessChain {
    /// Construct and validate a chain.
    ///
    /// # Errors
    /// Returns a [`StructuralAccessContractError`] when any hop is invalid or
    /// the sequence is not a possible access. See [`Self::validate`].
    pub fn new(
        subject: StructuralAccessSubject,
        hops: Vec<StructuralAccessHop>,
    ) -> Result<Self, StructuralAccessContractError> {
        let chain = Self { schema_version: STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION, subject, hops };
        chain.validate()?;
        Ok(chain)
    }

    /// Contract schema version this chain was built under.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Source, document, project, and root generation the chain holds under.
    #[must_use]
    pub const fn subject(&self) -> &StructuralAccessSubject {
        &self.subject
    }

    /// Ordered hop view.
    #[must_use]
    pub fn hops(&self) -> &[StructuralAccessHop] {
        &self.hops
    }

    /// The value the whole chain selected, when it selected one.
    ///
    /// A chain whose last hop did not select has no result; the caller must
    /// read that hop's outcome rather than substituting an absence.
    #[must_use]
    pub fn selected(&self) -> Option<&StructuralHopOutcome> {
        self.hops.last().map(StructuralAccessHop::outcome).filter(|outcome| outcome.is_selecting())
    }

    /// Validate the chain and every hop in it.
    ///
    /// Beyond each hop's own laws, a chain must satisfy:
    ///
    /// 1. It has at least one hop — an empty chain describes no access.
    /// 2. Hop ordinals are exactly `0..n`, in order. A reordered or dropped
    ///    hop breaks this, and so does a duplicated one.
    /// 3. Only the last hop may fail to select. A hop that produced no value
    ///    cannot be selected out of, so any non-selecting hop before the end
    ///    describes an impossible access.
    /// 4. Work accounting is monotone across hops: a hop cannot begin with
    ///    more remaining units than its predecessor ended with.
    /// 5. Every hop is anchored in the subject's own document. A chain is one
    ///    access in one file; a hop anchored elsewhere is not part of it.
    /// 6. The subject itself validates. A chain reconstructed by serde never
    ///    ran the subject's constructor, so this is where that check lands.
    /// 7. A hop cannot claim any *member-level* answer through an operator the
    ///    predecessor's known shape cannot carry — a hash operator on a known
    ///    array reference, the reverse, or any subscript on a plain scalar, a
    ///    code reference or a package name. That covers a claimed selection,
    ///    but equally a claimed absence: `$scalar->{k}` is a strict-refs error,
    ///    not a hash whose member happens to be missing, so recording it as
    ///    `AbsentMember` would collapse wrong-shape into legitimate absence —
    ///    a distinction #13619 requires be kept. `ShapeMismatch` says it
    ///    honestly, and a symbolic dereference says so with its own boundary.
    /// 8. A recorded [`StructuralHopOutcome::ShapeMismatch`] must describe a
    ///    real conflict about the shape in hand: the operator must be one the
    ///    predecessor's known shape cannot carry, and the observed shape must
    ///    be the one the predecessor selected.
    ///
    /// # Errors
    /// Returns the first violated law as a [`StructuralAccessContractError`].
    pub fn validate(&self) -> Result<(), StructuralAccessContractError> {
        if self.schema_version != STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION {
            return Err(StructuralAccessContractError::MalformedChain(
                "chain schema version is not the version this contract validates",
            ));
        }
        self.subject.validate()?;
        if self.hops.is_empty() {
            return Err(StructuralAccessContractError::MalformedChain(
                "a structural access chain must contain at least one hop",
            ));
        }

        for (position, hop) in self.hops.iter().enumerate() {
            hop.validate()?;

            if hop.spelling().anchor.file_id != self.subject.document {
                return Err(StructuralAccessContractError::MalformedChain(
                    "every hop must be anchored in the chain subject's own document",
                ));
            }

            let Ok(expected) = u32::try_from(position) else {
                return Err(StructuralAccessContractError::MalformedChain(
                    "chain length exceeds the ordinal space",
                ));
            };
            if hop.ordinal() != expected {
                return Err(StructuralAccessContractError::AggregateChainPosition {
                    ordinal: hop.ordinal(),
                    reason: "hop ordinals must be a dense ascending sequence from zero",
                });
            }

            // The predecessor link needs no separate check here. A hop's own
            // `validate` proves a `PrecedingHop` aggregate names `ordinal - 1`,
            // and the dense-ordinal check above proves `ordinal == position`;
            // together those force the named predecessor to be the hop at
            // `position - 1`, which this loop already validated. An explicit
            // re-derivation would be unreachable code.
        }

        for window in self.hops.windows(2) {
            let (previous, next) = (&window[0], &window[1]);
            if !previous.outcome().is_selecting() {
                return Err(StructuralAccessContractError::ContradictoryStatus(
                    "only the final hop may fail to select; nothing can be selected out of nothing",
                ));
            }
            if next.budget().units_before > previous.budget().units_after {
                return Err(StructuralAccessContractError::MalformedBudget(
                    "a hop cannot begin with more remaining units than its predecessor left",
                ));
            }

            // Law 7: an operator cannot successfully select through a shape
            // that cannot carry it.
            //
            // A hash reference carries only keyed operators and an array
            // reference only indexed ones. A plain scalar and a code reference
            // carry neither: neither is a subscriptable reference, so any
            // apparent selection through one is a symbolic dereference, which
            // has its own typed boundary and must be recorded as one.
            //
            // Only `Object` and `Unknown` constrain nothing: an object is a
            // blessed reference that may be a blessed hash or a blessed array,
            // and `Unknown` asserts nothing at all. Constraining either would
            // reject honest records.
            //
            // `ShapeMismatch` remains available to record a genuine mismatch.
            if let StructuralHopOutcome::Selected { shape, .. } = previous.outcome() {
                let carries = shape_carries(shape, next.operator().is_keyed());
                if next.outcome().claims_member_answer() && !carries {
                    return Err(StructuralAccessContractError::ContradictoryStatus(
                        "an operator cannot reach a member through a shape that cannot carry it",
                    ));
                }

                // Law 8: a recorded mismatch must be a real one, about the
                // shape actually in hand. A `ShapeMismatch` whose operator the
                // predecessor's shape does carry claims a conflict that did not
                // happen, and one whose `observed` disagrees with what the
                // predecessor selected describes a different aggregate. Both
                // are only checkable when the predecessor's shape is known,
                // which is exactly when `shape_carries` is decisive.
                if let StructuralHopOutcome::ShapeMismatch { observed } = next.outcome()
                    && shape_is_decisive(shape)
                {
                    if carries {
                        return Err(StructuralAccessContractError::ContradictoryStatus(
                            "a shape mismatch cannot be recorded for an operator the shape carries",
                        ));
                    }
                    if observed != shape {
                        return Err(StructuralAccessContractError::ContradictoryStatus(
                            "a shape mismatch must observe the shape the predecessor selected",
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Deterministic fingerprint of the whole chain.
    ///
    /// The subject and every hop fingerprint are folded in written order, so
    /// two chains that differ only in hop order, or in one hop's operator,
    /// produce different fingerprints. Spelling and anchors are excluded for
    /// the same reason they are excluded per hop.
    ///
    /// Fingerprint equality is a candidate match to be confirmed by structural
    /// equality, never proof of identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let accumulator = SemanticIdentityFingerprint::new(STRUCTURAL_ACCESS_SCHEMA_TAG)
            .field("schema-version", &self.schema_version.to_string());
        let mut accumulator =
            self.subject.fold(accumulator).field("hop-count", &self.hops.len().to_string());
        for hop in &self.hops {
            accumulator = accumulator.field("hop", &hop.fingerprint());
        }
        accumulator.finish()
    }
}
