//! One ordered structural access hop and the laws it must satisfy.

use serde::{Deserialize, Serialize};

use super::{
    STRUCTURAL_ACCESS_SCHEMA_TAG, StructuralAccessAggregate, StructuralAccessBudget,
    StructuralAccessContractError, StructuralAccessLimitation, StructuralAccessOperator,
    StructuralAccessSelector, StructuralAccessSpelling, StructuralAggregateCompleteness,
    StructuralAggregateDisposition, StructuralHopCertainty, StructuralHopOutcome,
};
use crate::semantic_identity::SemanticIdentityFingerprint;
use crate::{
    SemanticConfidence, SemanticProducer, SemanticProvenance, SemanticReasonCode, ValueShape,
};

/// One local transition in a structural access chain.
///
/// A hop is ordered: its `ordinal` is its position in the owning chain, and
/// its `aggregate` must agree with that position. Everything the consumer
/// needs to explain the hop without re-deriving it is bound here — the
/// operator as written, the member selected, the value produced, the honesty
/// state of that answer, the generation it holds under, and the work spent.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralAccessHop {
    /// Zero-based position of this hop in its chain.
    ordinal: u32,
    /// What this hop selected out of.
    aggregate: StructuralAccessAggregate,
    /// The local operator exactly as written at this position.
    operator: StructuralAccessOperator,
    /// Canonical identity of the member selected.
    selector: StructuralAccessSelector,
    /// Source spelling and range. Evidence only; never folded into identity.
    spelling: StructuralAccessSpelling,
    /// What the hop produced, or why it produced nothing.
    outcome: StructuralHopOutcome,
    /// Whether the outcome holds on every path or only on some path.
    certainty: StructuralHopCertainty,
    /// Whether the aggregate's member set is closed.
    completeness: StructuralAggregateCompleteness,
    /// Whether the aggregate escaped or was mutated.
    disposition: StructuralAggregateDisposition,
    /// Producer subsystem that derived the hop.
    producer: SemanticProducer,
    /// How the hop was derived.
    provenance: SemanticProvenance,
    /// Confidence in the hop.
    confidence: SemanticConfidence,
    /// Why the hop is exact, degraded, or refused.
    reason_code: SemanticReasonCode,
    /// Work accounting across this hop.
    budget: StructuralAccessBudget,
    /// Canonical limitation view; sorted and de-duplicated by the constructor.
    limitations: Vec<StructuralAccessLimitation>,
}

impl StructuralAccessHop {
    /// Construct and validate one hop.
    ///
    /// Limitations are canonicalized (sorted, de-duplicated) so two producers
    /// that record the same limitations in different orders build equal hops.
    ///
    /// # Errors
    /// Returns a [`StructuralAccessContractError`] when the hop is
    /// structurally impossible. See [`Self::validate`] for the full law set.
    #[allow(clippy::too_many_arguments)] // the constructor mirrors the contract fields
    pub fn new(
        ordinal: u32,
        aggregate: StructuralAccessAggregate,
        operator: StructuralAccessOperator,
        selector: StructuralAccessSelector,
        spelling: StructuralAccessSpelling,
        outcome: StructuralHopOutcome,
        certainty: StructuralHopCertainty,
        completeness: StructuralAggregateCompleteness,
        disposition: StructuralAggregateDisposition,
        producer: SemanticProducer,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        reason_code: SemanticReasonCode,
        budget: StructuralAccessBudget,
        mut limitations: Vec<StructuralAccessLimitation>,
    ) -> Result<Self, StructuralAccessContractError> {
        limitations.sort();
        limitations.dedup();
        let hop = Self {
            ordinal,
            aggregate,
            operator,
            selector,
            spelling,
            outcome,
            certainty,
            completeness,
            disposition,
            producer,
            provenance,
            confidence,
            reason_code,
            budget,
            limitations,
        };
        hop.validate()?;
        Ok(hop)
    }

    /// Canonical limitation view.
    #[must_use]
    pub fn limitations(&self) -> &[StructuralAccessLimitation] {
        &self.limitations
    }

    /// Zero-based position of this hop in its chain.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// What this hop selected out of.
    #[must_use]
    pub const fn aggregate(&self) -> &StructuralAccessAggregate {
        &self.aggregate
    }

    /// The local operator exactly as written at this position.
    #[must_use]
    pub const fn operator(&self) -> StructuralAccessOperator {
        self.operator
    }

    /// Canonical identity of the member selected.
    #[must_use]
    pub const fn selector(&self) -> &StructuralAccessSelector {
        &self.selector
    }

    /// Source spelling and range. Evidence only; never part of identity.
    #[must_use]
    pub const fn spelling(&self) -> &StructuralAccessSpelling {
        &self.spelling
    }

    /// What the hop produced, or why it produced nothing.
    #[must_use]
    pub const fn outcome(&self) -> &StructuralHopOutcome {
        &self.outcome
    }

    /// Whether the outcome holds on every path or only on some path.
    #[must_use]
    pub const fn certainty(&self) -> StructuralHopCertainty {
        self.certainty
    }

    /// Whether the aggregate's member set is closed.
    #[must_use]
    pub const fn completeness(&self) -> StructuralAggregateCompleteness {
        self.completeness
    }

    /// Whether the aggregate escaped or was mutated.
    #[must_use]
    pub const fn disposition(&self) -> StructuralAggregateDisposition {
        self.disposition
    }

    /// Producer subsystem that derived the hop.
    #[must_use]
    pub const fn producer(&self) -> SemanticProducer {
        self.producer
    }

    /// How the hop was derived.
    #[must_use]
    pub const fn provenance(&self) -> SemanticProvenance {
        self.provenance
    }

    /// Confidence in the hop.
    #[must_use]
    pub const fn confidence(&self) -> SemanticConfidence {
        self.confidence
    }

    /// Why the hop is exact, degraded, or refused.
    #[must_use]
    pub const fn reason_code(&self) -> SemanticReasonCode {
        self.reason_code
    }

    /// Work accounting across this hop.
    #[must_use]
    pub const fn budget(&self) -> StructuralAccessBudget {
        self.budget
    }

    /// Validate every law this hop must satisfy on its own.
    ///
    /// Chain-position agreement beyond the hop's own ordinal, and cross-hop
    /// ordering, are validated by
    /// [`StructuralAccessChain`](super::StructuralAccessChain).
    ///
    /// The laws are:
    ///
    /// 1. A keyed operator takes a keyed selector and an indexed operator
    ///    takes an indexed selector. `->{}` can never carry an index and
    ///    `->[]` can never carry a key.
    /// 2. The first hop cannot select out of a preceding hop, and a later hop
    ///    must select out of its immediate predecessor.
    /// 3. Definite absence requires a closed aggregate. A member missing from
    ///    an open aggregate is [`StructuralHopOutcome::UnknownMember`].
    /// 4. A definite selection requires a stable aggregate: an escaped or
    ///    mutated aggregate cannot support a claim that holds on every path.
    /// 5. [`StructuralHopOutcome::BudgetExhausted`] requires the budget to be
    ///    actually exhausted. Spending the last unit is not itself a defect: a
    ///    producer that budgeted exactly enough may still answer definitely.
    /// 6. Identity fields that name something are non-empty and ranges are
    ///    not inverted. A static key is exempt: `$h{""}` is a real member.
    ///    A package-bearing outcome shape is *not* exempt: see
    ///    [`validate_shape_identity`].
    /// 7. Limitations are sorted and duplicate-free, as the constructor
    ///    leaves them.
    /// 9. An arrow subscript cannot reach a member through a named `@` or `%`
    ///    variable: Perl rejects an array or hash used as a reference. `$`,
    ///    `&` and `*` all dereference legitimately.
    /// 8. A boundary that refuses promotion cannot carry a promoted value
    ///    fact. Only the promotion is refused: a shape claim with no
    ///    `value_fact` stays legal, since refusing to evaluate a dynamic key
    ///    does not stop a producer knowing what shape any member has.
    ///
    /// # Errors
    /// Returns the first violated law as a [`StructuralAccessContractError`].
    pub fn validate(&self) -> Result<(), StructuralAccessContractError> {
        self.aggregate.validate()?;
        self.budget.validate()?;

        if self.spelling.text.trim().is_empty() {
            return Err(StructuralAccessContractError::EmptyIdentityField(
                "StructuralAccessSpelling.text",
            ));
        }
        if self.spelling.anchor.start_byte > self.spelling.anchor.end_byte {
            return Err(StructuralAccessContractError::MalformedRange {
                start_byte: self.spelling.anchor.start_byte,
                end_byte: self.spelling.anchor.end_byte,
            });
        }
        // Law 10: a plain subscript on a named variable addresses that
        // variable's own container, never a scalar that merely shares its name.
        //
        // `$config{groups}` reads `%config` and `$config[0]` reads `@config`;
        // the leading `$` is the sigil of the *element*, not of the aggregate.
        // All three of `$config`, `@config` and `%config` can coexist as
        // distinct variables, so recording the aggregate as `$config` names a
        // different variable than the one the access reads — the same defect
        // as labelling a base with an AST kind name, reached by a subtler
        // route. Verified against the interpreter.
        //
        // Only the plain forms are constrained. `->{}` and `->[]` dereference
        // whatever the base holds, so their aggregate's sigil is not fixed by
        // the operator and forcing one would reject honest records.
        //
        // A dereferenced base such as `$$ref{k}` is not a named variable at
        // all — the hash it subscripts has no name — so it belongs in
        // `Fact` or `DynamicBoundary`, which is why this law can be exact
        // about the `Variable` case without catching that one.
        if let StructuralAccessAggregate::Variable { sigil, .. } = &self.aggregate {
            let required = match self.operator {
                StructuralAccessOperator::HashSlot => Some("%"),
                StructuralAccessOperator::ArrayIndex => Some("@"),
                StructuralAccessOperator::HashRefSlot | StructuralAccessOperator::ArrayRefIndex => {
                    None
                }
            };
            if let Some(required) = required
                && sigil != required
            {
                return Err(StructuralAccessContractError::ContradictoryStatus(
                    "a plain subscript addresses the variable's own container sigil",
                ));
            }

            // Law 11: an arrow subscript dereferences what the base yields, and
            // an array or hash container is not a reference. Perl says so
            // outright — `@a->[0]` is "Can't use an array as a reference" and
            // `%h->{k}` is "Can't use a hash as a reference" — so no member can
            // be reached through either.
            //
            // Only `@` and `%` are excluded, and only those. Verified against
            // the interpreter rather than generalised from "non-scalar":
            //
            //   $r->{k}                       ok, the ordinary case
            //   &foo->{k}, foo returning a ref  ok, the arrow derefs the call's
            //                                   result rather than the sub
            //   *STDOUT->{IO}                 ok, a glob slot
            //
            // Unlike the plain-subscript law above, this one is gated on the
            // outcome. That law is about *identity* — naming `$config` when the
            // access reads `%config` is the wrong variable whatever happened
            // next. This one is about *reachability*: the access genuinely
            // fails, so `ShapeMismatch` and a typed boundary remain the honest
            // ways to record it, exactly as for the adjacent-shape law.
            if matches!(
                self.operator,
                StructuralAccessOperator::HashRefSlot | StructuralAccessOperator::ArrayRefIndex
            ) && self.outcome.claims_member_answer()
                && matches!(sigil.as_str(), "@" | "%")
            {
                return Err(StructuralAccessContractError::ContradictoryStatus(
                    "an array or hash container is not a reference an arrow can dereference",
                ));
            }
        }

        // Law 9: a boundary that refuses promotion cannot carry a promoted
        // value fact.
        //
        // `BoundaryDisposition::Refuse` is documented as refusing *promotion*,
        // and `Selected::value_fact` is documented as the canonical fact
        // identity "when promoted". Those are the same word for the same act,
        // so recording one through the other is a record contradicting itself.
        //
        // Only the promotion is refused. `Selected { value_fact: None }`
        // remains legal behind a refusing boundary, because a shape claim is
        // not a promotion and is honestly reachable: a producer that refuses
        // to evaluate a dynamic key may still know every value in the hash is
        // an array reference — "I will not say which member, I will say what
        // shape a member has". Refusing that too would reject a truthful
        // record, and `crate::SemanticFactEnvelope::status` already treats a
        // refusing boundary as a *status* (`Refused`) rather than as an
        // impossible record, which this contract should not contradict.
        if let StructuralHopOutcome::Selected { value_fact: Some(_), .. } = &self.outcome
            && (self.aggregate.refuses_promotion() || self.selector.refuses_promotion())
        {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "a boundary that refuses promotion cannot carry a promoted value fact",
            ));
        }

        // Law 6, for the outcome's own payload. The aggregate and spelling are
        // checked above; the shape a hop claims to have selected or observed
        // carries an identity too, and serde reaches it without a constructor.
        match &self.outcome {
            StructuralHopOutcome::Selected { shape, .. }
            | StructuralHopOutcome::ShapeMismatch { observed: shape } => {
                validate_shape_identity(shape)?;
            }
            StructuralHopOutcome::AbsentMember
            | StructuralHopOutcome::UnknownMember
            | StructuralHopOutcome::StaleGeneration
            | StructuralHopOutcome::BudgetExhausted
            | StructuralHopOutcome::Boundary(_) => {}
        }

        // A static key is deliberately *not* checked for emptiness. `$h{""}`
        // and `$h{" "}` are legal Perl accesses naming distinct members, so an
        // empty or blank key is a real identity here, unlike a blank aggregate
        // name. A producer that has no key must say so with a dynamic selector
        // or a boundary rather than an empty one.

        // Law 1: operator class and selector class must agree.
        if self.operator.is_keyed() != self.selector.is_keyed() {
            return Err(StructuralAccessContractError::SelectorOperatorMismatch {
                operator: self.operator.tag(),
                selector: self.selector.tag(),
            });
        }

        // Law 2: the aggregate must match this hop's position.
        match (&self.aggregate, self.ordinal) {
            (StructuralAccessAggregate::PrecedingHop { .. }, 0) => {
                return Err(StructuralAccessContractError::AggregateChainPosition {
                    ordinal: 0,
                    reason: "the first hop has no preceding hop to select out of",
                });
            }
            (StructuralAccessAggregate::PrecedingHop { ordinal }, position) => {
                if position.checked_sub(1) != Some(*ordinal) {
                    return Err(StructuralAccessContractError::AggregateChainPosition {
                        ordinal: position,
                        reason: "a hop must select out of its immediate predecessor",
                    });
                }
            }
            (_, 0) => {}
            (_, position) => {
                return Err(StructuralAccessContractError::AggregateChainPosition {
                    ordinal: position,
                    reason: "only the first hop may name an input aggregate directly",
                });
            }
        }

        // Law 3: absence and unknown-ness each require their own completeness.
        // The two halves are mirrors: a member missing from an open aggregate
        // is unknown rather than absent, and a member missing from a closed one
        // is absent rather than unknown. Accepting either pairing would let a
        // producer record uncertainty and definite absence interchangeably,
        // which is the exact collapse this contract exists to prevent.
        if matches!(self.outcome, StructuralHopOutcome::AbsentMember)
            && matches!(self.completeness, StructuralAggregateCompleteness::Open)
        {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "a member missing from an open aggregate is unknown, not absent",
            ));
        }
        if matches!(self.outcome, StructuralHopOutcome::UnknownMember)
            && matches!(self.completeness, StructuralAggregateCompleteness::Closed)
        {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "a member missing from a closed aggregate is absent, not unknown",
            ));
        }

        // Law 4: a definite claim *about the aggregate's contents* requires an
        // aggregate that did not move. An outcome whose truth does not depend
        // on those contents is exempt: a budget definitely ran out, a
        // generation is definitely stale, and a boundary definitely stopped the
        // hop, whatever later happened to the aggregate. Applying stability to
        // those would reject honest records.
        if matches!(self.certainty, StructuralHopCertainty::Definite)
            && self.outcome.depends_on_aggregate_contents()
            && !self.disposition.is_stable()
        {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "an escaped or mutated aggregate cannot support a definite claim about its contents",
            ));
        }

        // Law 5: an exhausted-budget outcome must be backed by the accounting.
        //
        // The converse is deliberately not a law. Spending the last unit is not
        // the same as being cut short: a producer that budgeted exactly enough
        // ends at zero having answered definitely, and only the
        // `BudgetExhausted` *outcome* says the work stopped early.
        if matches!(self.outcome, StructuralHopOutcome::BudgetExhausted)
            && !self.budget.is_exhausted()
        {
            return Err(StructuralAccessContractError::MalformedBudget(
                "a budget-exhausted outcome requires zero remaining units",
            ));
        }

        // Law 7: limitations must already be canonical. The constructor sorts
        // and de-duplicates them so two producers recording the same set build
        // equal hops; serde bypasses that, and a non-canonical vector would
        // compare unequal and serialize differently while meaning the same
        // thing, breaking the determinism this contract claims.
        if self.limitations.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(StructuralAccessContractError::ContradictoryStatus(
                "limitations must be sorted and free of duplicates",
            ));
        }

        Ok(())
    }

    /// Deterministic fingerprint of this hop's identity.
    ///
    /// The fingerprint folds the schema tag, ordinal, aggregate, operator,
    /// selector, outcome, and honesty state. It deliberately excludes the
    /// source spelling and anchor: reformatting a file must not change what a
    /// hop *is*. It likewise excludes producer, provenance, confidence,
    /// reason code, budget, and limitations — each records how the hop was
    /// obtained or how far it can be trusted, not which access it describes.
    ///
    /// Fingerprint equality is a candidate match to be confirmed by structural
    /// equality, never proof of identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let accumulator = SemanticIdentityFingerprint::new(STRUCTURAL_ACCESS_SCHEMA_TAG)
            .field("ordinal", &self.ordinal.to_string());
        let accumulator = self.aggregate.fold(accumulator);
        let accumulator = accumulator.discriminant("operator", self.operator.tag());
        let accumulator = self.selector.fold(accumulator);
        let accumulator = self.outcome.fold(accumulator);
        accumulator
            .discriminant("certainty", self.certainty.tag())
            .discriminant("completeness", self.completeness.tag())
            .discriminant("disposition", self.disposition.tag())
            .finish()
    }
}

/// Whether a shape that names a package actually names one.
///
/// [`ValueShape::PackageName`] and [`ValueShape::Object`] both carry a package
/// string that identifies what was selected. A blank one identifies nothing,
/// and no honest producer emits one:
///
/// - `package ;` is a syntax error, so a blank [`ValueShape::PackageName`]
///   cannot describe real source at all.
/// - `bless $ref, ""` is accepted by the interpreter, but it warns
///   (`Explicit blessing to '' (assuming package main)`) and the resulting
///   class is `main`, not the empty string. The honest record for that case is
///   `Object { package: "main" }`.
///
/// Verified against the interpreter rather than assumed, because the sibling
/// question — whether `$h{""}` is a real hash member — went the other way, and
/// a static key is deliberately exempt from this law for exactly that reason.
///
/// The remaining shapes carry no identity to check.
fn validate_shape_identity(shape: &ValueShape) -> Result<(), StructuralAccessContractError> {
    match shape {
        ValueShape::PackageName { package } if package.trim().is_empty() => Err(
            StructuralAccessContractError::EmptyIdentityField("ValueShape::PackageName.package"),
        ),
        ValueShape::Object { package, .. } if package.trim().is_empty() => {
            Err(StructuralAccessContractError::EmptyIdentityField("ValueShape::Object.package"))
        }
        ValueShape::PackageName { .. }
        | ValueShape::Object { .. }
        | ValueShape::Unknown
        | ValueShape::Scalar
        | ValueShape::ArrayRef
        | ValueShape::HashRef
        | ValueShape::CodeRef => Ok(()),
    }
}
