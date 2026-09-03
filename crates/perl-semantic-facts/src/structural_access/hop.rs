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
    /// 8. A boundary that refuses promotion cannot carry a promoted value
    ///    fact. Only the promotion is refused: a shape claim with no
    ///    `value_fact` stays legal, since refusing to evaluate a dynamic key
    ///    does not stop a producer knowing what shape any member has.
    /// 9. A plain subscript on a named variable addresses that variable's own
    ///    container: `$config{groups}` reads `%config` and `$config[0]` reads
    ///    `@config`. This is an identity law and holds whatever the outcome.
    /// 10. An arrow subscript cannot reach a member through a named `@` or
    ///     `%` variable: Perl rejects an array or hash used as a reference.
    ///     `$`, `&` and `*` all dereference legitimately. This is a
    ///     reachability law, so it is gated on the outcome claiming a member
    ///     answer.
    /// 11. A limitation that restates a typed field must not contradict it.
    ///     `OpenAggregate`, `MutatedAggregate` and `EscapedAggregate` each
    ///     assert word for word what a completeness or disposition field on
    ///     this same record asserts, so carrying one binds that field. The
    ///     law is one-directional — omitting a limitation asserts nothing —
    ///     and covers only those three; the remaining limitations either
    ///     restate no field or are deliberately weaker than the outcome that
    ///     shares their name.
    /// 12. A plain subscript on a named variable cannot report
    ///     [`StructuralHopOutcome::ShapeMismatch`]. Law 9 has already bound
    ///     the operator to the variable's own sigil, so the shape is fixed in
    ///     the source and there is nothing left to conflict with. Arrow
    ///     operators and non-variable aggregates keep the outcome, because
    ///     their runtime shape is genuinely open.
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
        // Law 9: a plain subscript on a named variable addresses that
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

            // Law 10: an arrow subscript dereferences what the base yields, and
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

            // Law 12: a plain subscript on a named variable cannot mismatch the
            // shape it just named. Law 9 has already forced `{}` onto a `%`
            // variable and `[]` onto an `@` one, so by this point the sigil
            // *is* the shape declaration, fixed in the source rather than
            // discovered at run time: `%config` is a hash in every execution,
            // and `$config{k}` accesses it as one. There is no shape left to
            // conflict with, so a recorded `ShapeMismatch` describes a conflict
            // that cannot occur, and nothing else constrains its `observed`
            // payload on a first hop — chain law 8 only binds `observed` when a
            // predecessor selected a shape.
            //
            // The law stops exactly here. An arrow operator on a variable keeps
            // `ShapeMismatch`, because `$ref->{k}` names a scalar whose runtime
            // shape genuinely is unknown and may well be the wrong one, and so
            // does every non-`Variable` aggregate, whose shape this record does
            // not fix either.
            if matches!(
                self.operator,
                StructuralAccessOperator::HashSlot | StructuralAccessOperator::ArrayIndex
            ) && matches!(self.outcome, StructuralHopOutcome::ShapeMismatch { .. })
            {
                return Err(StructuralAccessContractError::ContradictoryStatus(
                    "a plain subscript cannot mismatch the container its own sigil names",
                ));
            }
        }

        // Law 8: a boundary that refuses promotion cannot carry a promoted
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

        // Law 11: a limitation that restates a typed field must not contradict
        // it. Three limitations assert the same proposition as a field on this
        // same record, word for word, so a record carrying both must agree:
        //
        // - `OpenAggregate` says "the member set is not closed", which is what
        //   `StructuralAggregateCompleteness::Open` says;
        // - `MutatedAggregate` says "written after construction", which is what
        //   `StructuralAggregateDisposition::Mutated` says;
        // - `EscapedAggregate` says "reached unanalyzed code", which is what
        //   `StructuralAggregateDisposition::Escaped` says.
        //
        // A consumer reading the limitation and a consumer reading the field
        // would otherwise reach opposite conclusions from one record, which is
        // the collapse this contract exists to prevent.
        //
        // The law is deliberately one-directional. Carrying the limitation
        // asserts the proposition and so binds the field; omitting it asserts
        // nothing, because limitations are the producer's notes rather than an
        // exhaustive projection, and requiring one for every `Open` aggregate
        // would reject honest records that simply did not annotate.
        //
        // The other limitations are *not* covered, and deliberately:
        // `StaleDependency` is about a dependency's generation while the
        // `StaleGeneration` outcome is about this aggregate against its own
        // subject — different claims that may hold independently — and
        // `BudgetExhausted` as a limitation is weaker than the outcome by the
        // design recorded above, where only the outcome forces zero remaining
        // units. `DynamicSelector`, `RecoveredSyntax`, `CompatibilityBridge`
        // and `Unsupported` restate no field at all.
        for limitation in &self.limitations {
            let contradiction = match limitation {
                StructuralAccessLimitation::OpenAggregate => {
                    matches!(self.completeness, StructuralAggregateCompleteness::Closed)
                        .then_some("an open-aggregate limitation contradicts a closed aggregate")
                }
                StructuralAccessLimitation::MutatedAggregate => (!matches!(
                    self.disposition,
                    StructuralAggregateDisposition::Mutated
                        | StructuralAggregateDisposition::EscapedAndMutated
                ))
                .then_some("a mutated-aggregate limitation contradicts an unmutated aggregate"),
                StructuralAccessLimitation::EscapedAggregate => (!matches!(
                    self.disposition,
                    StructuralAggregateDisposition::Escaped
                        | StructuralAggregateDisposition::EscapedAndMutated
                ))
                .then_some("an escaped-aggregate limitation contradicts an unescaped aggregate"),
                StructuralAccessLimitation::DynamicSelector
                | StructuralAccessLimitation::RecoveredSyntax
                | StructuralAccessLimitation::BudgetExhausted
                | StructuralAccessLimitation::StaleDependency
                | StructuralAccessLimitation::CompatibilityBridge
                | StructuralAccessLimitation::Unsupported => None,
            };
            if let Some(reason) = contradiction {
                return Err(StructuralAccessContractError::ContradictoryStatus(reason));
            }
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
/// string that identifies what was selected. The *empty* string identifies
/// nothing, in both directions, and no honest producer emits one:
///
/// - `bless $ref, ""` is accepted by the interpreter, but it warns
///   (`Explicit blessing to '' (assuming package main)`) and the resulting
///   class is `main`, not the empty string. The honest record for that case is
///   `Object { package: "main" }`.
/// - `""->method` dies with `Can't call method "..." without a package or
///   object reference`, so the empty string is not a usable class name either.
///
/// A *whitespace* package name is a different answer, and the law deliberately
/// admits it. `bless {}, "  "` yields an object whose `ref` is `"  "`, and
/// method dispatch through it resolves normally against the `"  "` symbol-table
/// entry — as does `"  "->method` on the value side. It is perverse, but it is
/// a real package, and a shape that cannot tell a real class from a blank one
/// must not reject it. This mirrors `$h{""}`, which is a real hash member and
/// is likewise exempt.
///
/// The distinction that decides this law is source token versus runtime value.
/// A blank check on spelling text, a sigil or a variable name stays a
/// whitespace check, because each records something *written*, and no Perl
/// program writes a whitespace-only variable token — `my $  ;` is a syntax
/// error. A package here is a runtime string, reachable through
/// `bless $ref, $name`, so it carries no such guarantee.
///
/// Verified against the interpreter rather than assumed, in both directions.
///
/// The remaining shapes carry no identity to check.
fn validate_shape_identity(shape: &ValueShape) -> Result<(), StructuralAccessContractError> {
    match shape {
        ValueShape::PackageName { package } if package.is_empty() => Err(
            StructuralAccessContractError::EmptyIdentityField("ValueShape::PackageName.package"),
        ),
        ValueShape::Object { package, .. } if package.is_empty() => {
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
