//! Falsifiers for the ordered structural access-hop contract (#13619).
//!
//! Each test names the wrong implementation it rejects. The suite is built so
//! that collapsing any distinction the issue requires — arrow versus plain,
//! dynamic key versus dynamic index, absence versus unknown, hop order — turns
//! at least one test red.

use std::error::Error;

use super::*;
use crate::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, FactId, FileId,
    SemanticConfidence, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration, ValueShape,
};

const DOCUMENT: FileId = FileId(7);

/// Assert a constructor rejected its input, returning the contract error.
///
/// Mirrors the reachability contract suite: a test must never panic on an
/// unexpected `Ok`, it must fail with the reason.
fn contract_error<T>(
    result: Result<T, StructuralAccessContractError>,
) -> Result<StructuralAccessContractError, Box<dyn Error>> {
    match result {
        Err(error) => Ok(error),
        Ok(_) => Err("expected a contract error, got success".into()),
    }
}

fn subject() -> Result<StructuralAccessSubject, StructuralAccessContractError> {
    StructuralAccessSubject::new(
        DOCUMENT,
        SourceGeneration::known("source-sha"),
        Some("file:///workspace".to_string()),
        Some(SourceGeneration::known("project-gen")),
    )
}

fn spelling(
    text: &str,
    start: u32,
    end: u32,
) -> Result<StructuralAccessSpelling, StructuralAccessContractError> {
    StructuralAccessSpelling::new(text, SourceAnchor::new(Some(AnchorId(1)), DOCUMENT, start, end))
}

fn base_variable() -> StructuralAccessAggregate {
    StructuralAccessAggregate::Variable { sigil: "$".to_string(), name: "config".to_string() }
}

fn dynamic_boundary(kind: BoundaryKind, reason: SemanticReasonCode) -> BoundaryLink {
    BoundaryLink::new(Some(FactId(3)), kind, BoundaryDisposition::Degrade, reason)
}

/// A selecting hop with every honesty field at its strongest setting.
#[allow(clippy::too_many_arguments)]
fn selecting_hop(
    ordinal: u32,
    aggregate: StructuralAccessAggregate,
    operator: StructuralAccessOperator,
    selector: StructuralAccessSelector,
    text: &str,
    shape: ValueShape,
) -> Result<StructuralAccessHop, StructuralAccessContractError> {
    StructuralAccessHop::new(
        ordinal,
        aggregate,
        operator,
        selector,
        spelling(text, ordinal * 10, ordinal * 10 + 5)?,
        StructuralHopOutcome::Selected {
            shape,
            value_fact: Some(FactId(100 + u64::from(ordinal))),
        },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(100 - ordinal, 99 - ordinal)?,
        Vec::new(),
    )
}

/// `$config->{groups}{staff}[0]` — the issue's worked example.
fn nested_chain() -> Result<StructuralAccessChain, StructuralAccessContractError> {
    let hops = vec![
        selecting_hop(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("groups".to_string()),
            "->{groups}",
            ValueShape::HashRef,
        )?,
        selecting_hop(
            1,
            StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("staff".to_string()),
            "{staff}",
            ValueShape::ArrayRef,
        )?,
        selecting_hop(
            2,
            StructuralAccessAggregate::PrecedingHop { ordinal: 1 },
            StructuralAccessOperator::ArrayIndex,
            StructuralAccessSelector::StaticIndex(0),
            "[0]",
            ValueShape::Object { package: "Staff".to_string(), confidence: Confidence::High },
        )?,
    ];
    StructuralAccessChain::new(subject()?, hops)
}

// ── Ordered round trips and deterministic fingerprints ────────────────────

#[test]
fn nested_chain_round_trips_and_preserves_written_operator_order() -> Result<(), Box<dyn Error>> {
    let chain = nested_chain()?;
    let operators: Vec<_> = chain.hops().iter().map(|hop| hop.operator).collect();
    assert_eq!(
        operators,
        [
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessOperator::HashSlot,
            StructuralAccessOperator::ArrayIndex,
        ],
        "an earlier arrow must not relabel a later local operator"
    );

    let serialized = serde_json::to_string(&chain)?;
    let decoded: StructuralAccessChain = serde_json::from_str(&serialized)?;
    assert_eq!(decoded, chain);
    decoded.validate()?;
    assert_eq!(decoded.fingerprint(), chain.fingerprint());
    assert_eq!(serde_json::to_string(&decoded)?, serialized);
    Ok(())
}

#[test]
fn deserialized_garbage_is_rejected_by_validate() -> Result<(), Box<dyn Error>> {
    // The transport trust boundary: serde reconstructs a shape the constructor
    // would never have produced, and only `validate` catches it.
    let mut value = serde_json::to_value(nested_chain()?)?;
    let hops = value["hops"].as_array_mut().ok_or("hops must be an array")?;
    hops.swap(0, 1);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a reordered chain must not survive the transport boundary"
    );
    Ok(())
}

#[test]
fn limitations_are_canonicalized_so_producer_order_does_not_change_equality()
-> Result<(), Box<dyn Error>> {
    let build = |limitations: Vec<StructuralAccessLimitation>| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("groups".to_string()),
            spelling("->{groups}", 0, 10)?,
            StructuralHopOutcome::UnknownMember,
            StructuralHopCertainty::Possible,
            StructuralAggregateCompleteness::Open,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::Medium),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            limitations,
        )
    };
    let ascending = build(vec![
        StructuralAccessLimitation::DynamicSelector,
        StructuralAccessLimitation::OpenAggregate,
    ])?;
    let descending = build(vec![
        StructuralAccessLimitation::OpenAggregate,
        StructuralAccessLimitation::DynamicSelector,
        StructuralAccessLimitation::OpenAggregate,
    ])?;
    assert_eq!(ascending, descending);
    assert_eq!(ascending.limitations().len(), 2);
    Ok(())
}

// ── The distinctions the contract exists to keep ──────────────────────────

#[test]
fn arrow_and_plain_first_hops_do_not_collide() -> Result<(), Box<dyn Error>> {
    // `$a->{b}[0]` versus `$a{b}->[0]`: the same selected shape, different
    // written operators at both hops. A model that collapsed arrow and plain
    // forms would make these two chains identical.
    let arrow_first = StructuralAccessChain::new(
        subject()?,
        vec![
            selecting_hop(
                0,
                base_variable(),
                StructuralAccessOperator::HashRefSlot,
                StructuralAccessSelector::StaticKey("b".to_string()),
                "->{b}",
                ValueShape::ArrayRef,
            )?,
            selecting_hop(
                1,
                StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
                StructuralAccessOperator::ArrayIndex,
                StructuralAccessSelector::StaticIndex(0),
                "[0]",
                ValueShape::Scalar,
            )?,
        ],
    )?;
    let plain_first = StructuralAccessChain::new(
        subject()?,
        vec![
            selecting_hop(
                0,
                base_variable(),
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("b".to_string()),
                "{b}",
                ValueShape::ArrayRef,
            )?,
            selecting_hop(
                1,
                StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
                StructuralAccessOperator::ArrayRefIndex,
                StructuralAccessSelector::StaticIndex(0),
                "->[0]",
                ValueShape::Scalar,
            )?,
        ],
    )?;
    assert_ne!(arrow_first, plain_first);
    assert_ne!(arrow_first.fingerprint(), plain_first.fingerprint());
    Ok(())
}

#[test]
fn changing_one_local_operator_changes_only_that_hop_identity() -> Result<(), Box<dyn Error>> {
    let chain = nested_chain()?;
    let mut hops = chain.hops().to_vec();
    // Rewrite only the middle hop's operator; `{staff}` becomes `->{staff}`.
    hops[1] = selecting_hop(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("staff".to_string()),
        "->{staff}",
        ValueShape::ArrayRef,
    )?;
    let rewritten = StructuralAccessChain::new(subject()?, hops)?;

    assert_eq!(chain.hops()[0].fingerprint(), rewritten.hops()[0].fingerprint());
    assert_ne!(chain.hops()[1].fingerprint(), rewritten.hops()[1].fingerprint());
    assert_eq!(chain.hops()[2].fingerprint(), rewritten.hops()[2].fingerprint());
    assert_ne!(chain.fingerprint(), rewritten.fingerprint());
    Ok(())
}

#[test]
fn the_same_key_under_different_aggregates_does_not_collide() -> Result<(), Box<dyn Error>> {
    let from_config = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "->{groups}",
        ValueShape::HashRef,
    )?;
    let from_other = selecting_hop(
        0,
        StructuralAccessAggregate::Variable { sigil: "$".to_string(), name: "other".to_string() },
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "->{groups}",
        ValueShape::HashRef,
    )?;
    assert_ne!(from_config.fingerprint(), from_other.fingerprint());
    Ok(())
}

#[test]
fn dynamic_key_and_dynamic_index_remain_distinct_boundaries() -> Result<(), Box<dyn Error>> {
    let boundary = dynamic_boundary(BoundaryKind::DynamicValue, SemanticReasonCode::DynamicValue);
    let dynamic_key = StructuralAccessSelector::DynamicKey(boundary.clone());
    let dynamic_index = StructuralAccessSelector::DynamicIndex(boundary);
    assert_ne!(dynamic_key, dynamic_index);
    assert_ne!(dynamic_key.tag(), dynamic_index.tag());
    assert!(dynamic_key.is_keyed());
    assert!(!dynamic_index.is_keyed());
    Ok(())
}

#[test]
fn two_dynamic_selectors_with_different_boundaries_stay_distinguishable()
-> Result<(), Box<dyn Error>> {
    let build = |boundary: BoundaryLink| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::DynamicKey(boundary.clone()),
            spelling("->{$key}", 0, 8)?,
            StructuralHopOutcome::Boundary(boundary),
            StructuralHopCertainty::Possible,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::DynamicBoundary),
            SemanticConfidence::Known(Confidence::Low),
            SemanticReasonCode::DynamicValue,
            StructuralAccessBudget::new(10, 9)?,
            vec![StructuralAccessLimitation::DynamicSelector],
        )
    };
    let computed =
        build(dynamic_boundary(BoundaryKind::DynamicValue, SemanticReasonCode::DynamicValue))?;
    let symbolic = build(dynamic_boundary(
        BoundaryKind::SymbolicReference,
        SemanticReasonCode::UnsupportedEffect,
    ))?;
    assert_ne!(computed.fingerprint(), symbolic.fingerprint());
    Ok(())
}

#[test]
fn absence_unknown_mismatch_stale_and_exhaustion_are_all_distinct() -> Result<(), Box<dyn Error>> {
    let outcomes = [
        StructuralHopOutcome::AbsentMember,
        StructuralHopOutcome::UnknownMember,
        StructuralHopOutcome::ShapeMismatch { observed: ValueShape::ArrayRef },
        StructuralHopOutcome::StaleGeneration,
        StructuralHopOutcome::BudgetExhausted,
        StructuralHopOutcome::Boundary(dynamic_boundary(
            BoundaryKind::DynamicValue,
            SemanticReasonCode::DynamicValue,
        )),
        StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None },
    ];
    let mut tags: Vec<_> = outcomes.iter().map(StructuralHopOutcome::tag).collect();
    tags.sort_unstable();
    let distinct = tags.len();
    tags.dedup();
    assert_eq!(tags.len(), distinct, "every non-selecting state must stay distinct");

    // Only `Selected` may be selected out of.
    for outcome in &outcomes {
        assert_eq!(
            outcome.is_selecting(),
            matches!(outcome, StructuralHopOutcome::Selected { .. })
        );
    }
    Ok(())
}

#[test]
fn spelling_is_evidence_and_never_participates_in_identity() -> Result<(), Box<dyn Error>> {
    // Negative control for "no source substring scan may decide the operator
    // class": two hops that differ only in written text and position are the
    // same access, and a hop whose text *looks* like an arrow is still
    // classified by its operator field alone.
    let mut moved = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "{groups}",
        ValueShape::HashRef,
    )?;
    let original_fingerprint = moved.fingerprint();

    moved.spelling = spelling("  {groups}  ", 4096, 4108)?;
    assert_eq!(
        moved.fingerprint(),
        original_fingerprint,
        "reformatting or moving a hop must not change what it is"
    );

    // Text containing an earlier arrow cannot make this a hashref slot.
    moved.spelling = spelling("->{outer}{groups}", 0, 17)?;
    moved.validate()?;
    assert_eq!(moved.operator, StructuralAccessOperator::HashSlot);
    assert!(!moved.operator.dereferences());
    assert_eq!(moved.fingerprint(), original_fingerprint);
    Ok(())
}

// ── Impossible records must not validate ──────────────────────────────────

#[test]
fn an_index_selector_cannot_ride_a_hash_operator() -> Result<(), Box<dyn Error>> {
    // a keyed operator must reject an index selector
    let error = contract_error(StructuralAccessHop::new(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticIndex(0),
        spelling("->{b}", 0, 5)?,
        StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(10, 9)?,
        Vec::new(),
    ))?;
    assert!(matches!(error, StructuralAccessContractError::SelectorOperatorMismatch { .. }));

    // an indexed operator must reject a key selector
    let error = contract_error(StructuralAccessHop::new(
        0,
        base_variable(),
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticKey("b".to_string()),
        spelling("[b]", 0, 3)?,
        StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(10, 9)?,
        Vec::new(),
    ))?;
    assert!(matches!(error, StructuralAccessContractError::SelectorOperatorMismatch { .. }));
    Ok(())
}

#[test]
fn a_first_hop_cannot_select_out_of_a_preceding_hop() -> Result<(), Box<dyn Error>> {
    // hop zero has no predecessor
    let error = contract_error(selecting_hop(
        0,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "{b}",
        ValueShape::Scalar,
    ))?;
    assert!(matches!(
        error,
        StructuralAccessContractError::AggregateChainPosition { ordinal: 0, .. }
    ));
    Ok(())
}

#[test]
fn a_later_hop_must_select_out_of_its_immediate_predecessor() -> Result<(), Box<dyn Error>> {
    // Skipping a hop: hop 2 claims hop 0 as its aggregate.
    // a hop may not skip its predecessor
    let error = contract_error(selecting_hop(
        2,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(0),
        "[0]",
        ValueShape::Scalar,
    ))?;
    assert!(matches!(error, StructuralAccessContractError::AggregateChainPosition { .. }));

    // Re-entering the chain from a fresh variable partway through.
    // only the first hop may name an input aggregate
    let error = contract_error(selecting_hop(
        1,
        base_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "{b}",
        ValueShape::Scalar,
    ))?;
    assert!(matches!(error, StructuralAccessContractError::AggregateChainPosition { .. }));
    Ok(())
}

#[test]
fn dropping_an_intermediate_hop_fails_chain_validation() -> Result<(), Box<dyn Error>> {
    let chain = nested_chain()?;
    let mut hops = chain.hops().to_vec();
    hops.remove(1);
    // a chain with a hole must not validate
    let error = contract_error(StructuralAccessChain::new(subject()?, hops))?;
    assert!(matches!(error, StructuralAccessContractError::AggregateChainPosition { .. }));
    Ok(())
}

#[test]
fn an_empty_chain_describes_no_access() -> Result<(), Box<dyn Error>> {
    // an empty chain must not validate
    let error = contract_error(StructuralAccessChain::new(subject()?, Vec::new()))?;
    assert!(matches!(error, StructuralAccessContractError::MalformedChain(_)));
    Ok(())
}

#[test]
fn nothing_can_be_selected_out_of_a_non_selecting_hop() -> Result<(), Box<dyn Error>> {
    let stalled = StructuralAccessHop::new(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        spelling("->{groups}", 0, 10)?,
        StructuralHopOutcome::UnknownMember,
        StructuralHopCertainty::Possible,
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::Low),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(100, 99)?,
        vec![StructuralAccessLimitation::OpenAggregate],
    )?;
    let follower = selecting_hop(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("staff".to_string()),
        "{staff}",
        ValueShape::ArrayRef,
    )?;

    // a hop that produced nothing cannot be selected out of
    let error =
        contract_error(StructuralAccessChain::new(subject()?, vec![stalled.clone(), follower]))?;
    assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));

    // The same hop is legitimate as the final hop of a chain.
    let terminal = StructuralAccessChain::new(subject()?, vec![stalled])?;
    assert!(terminal.selected().is_none(), "a terminal non-selecting chain has no result");
    Ok(())
}

#[test]
fn definite_absence_requires_a_closed_aggregate() -> Result<(), Box<dyn Error>> {
    let build = |completeness| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("missing".to_string()),
            spelling("->{missing}", 0, 11)?,
            StructuralHopOutcome::AbsentMember,
            StructuralHopCertainty::Definite,
            completeness,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    build(StructuralAggregateCompleteness::Closed)?;
    // absence from an open aggregate is unknown, not absent
    let error = contract_error(build(StructuralAggregateCompleteness::Open))?;
    assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));
    Ok(())
}

#[test]
fn an_escaped_or_mutated_aggregate_cannot_support_a_definite_outcome() -> Result<(), Box<dyn Error>>
{
    let build = |disposition, certainty| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("groups".to_string()),
            spelling("->{groups}", 0, 10)?,
            StructuralHopOutcome::Selected { shape: ValueShape::HashRef, value_fact: None },
            certainty,
            StructuralAggregateCompleteness::Closed,
            disposition,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::Medium),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    for disposition in [
        StructuralAggregateDisposition::Escaped,
        StructuralAggregateDisposition::Mutated,
        StructuralAggregateDisposition::EscapedAndMutated,
    ] {
        // a moved aggregate cannot support a definite claim
        let error = contract_error(build(disposition, StructuralHopCertainty::Definite))?;
        assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));
        // The same hop is honest when it claims only possibility.
        build(disposition, StructuralHopCertainty::Possible)?;
    }
    Ok(())
}

#[test]
fn budget_accounting_must_be_monotone_and_back_its_own_outcome() -> Result<(), Box<dyn Error>> {
    // Remaining units cannot grow across a hop.
    let error = contract_error(StructuralAccessBudget::new(5, 6))?;
    assert!(matches!(error, StructuralAccessContractError::MalformedBudget(_)));

    // A budget-exhausted outcome with units left is a lie about the budget.
    let error = contract_error(StructuralAccessHop::new(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        spelling("->{groups}", 0, 10)?,
        StructuralHopOutcome::BudgetExhausted,
        StructuralHopCertainty::Possible,
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::Low),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(10, 9)?,
        vec![StructuralAccessLimitation::BudgetExhausted],
    ))?;
    assert!(matches!(error, StructuralAccessContractError::MalformedBudget(_)));

    // A chain cannot refill the budget between hops.
    let first = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "->{groups}",
        ValueShape::HashRef,
    )?;
    let refilled = StructuralAccessHop::new(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("staff".to_string()),
        spelling("{staff}", 10, 17)?,
        StructuralHopOutcome::Selected { shape: ValueShape::ArrayRef, value_fact: None },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(u32::MAX, u32::MAX - 1)?,
        Vec::new(),
    )?;
    // a chain cannot refill its budget between hops
    let error = contract_error(StructuralAccessChain::new(subject()?, vec![first, refilled]))?;
    assert!(matches!(error, StructuralAccessContractError::MalformedBudget(_)));
    Ok(())
}

#[test]
fn identity_fields_and_ranges_must_be_well_formed() -> Result<(), Box<dyn Error>> {
    // blank spelling is not evidence
    let error = contract_error(StructuralAccessSpelling::new(
        "   ",
        SourceAnchor::new(None, DOCUMENT, 0, 4),
    ))?;
    assert!(matches!(error, StructuralAccessContractError::EmptyIdentityField(_)));

    // an inverted range is impossible
    let error = contract_error(StructuralAccessSpelling::new(
        "{b}",
        SourceAnchor::new(None, DOCUMENT, 20, 10),
    ))?;
    assert!(matches!(error, StructuralAccessContractError::MalformedRange { .. }));

    // an aggregate must be nameable or explicitly unknown
    let error = contract_error(selecting_hop(
        0,
        StructuralAccessAggregate::Variable { sigil: "$".to_string(), name: "  ".to_string() },
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "{b}",
        ValueShape::Scalar,
    ))?;
    assert!(matches!(error, StructuralAccessContractError::EmptyIdentityField(_)));

    // an empty static key is not an identity
    let error = contract_error(selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey(String::new()),
        "{}",
        ValueShape::Scalar,
    ))?;
    assert!(matches!(error, StructuralAccessContractError::EmptyIdentityField(_)));

    // a blank workspace root is not a root
    let error = contract_error(StructuralAccessSubject::new(
        DOCUMENT,
        SourceGeneration::known("source-sha"),
        Some("  ".to_string()),
        None,
    ))?;
    assert!(matches!(error, StructuralAccessContractError::EmptyIdentityField(_)));
    Ok(())
}

#[test]
fn an_unnameable_aggregate_uses_a_typed_identity_not_a_rendered_label() -> Result<(), Box<dyn Error>>
{
    // The defect this replaces: a nested base degrading to the AST kind name
    // `Binary`. Both typed escapes are available and stay distinct from each
    // other and from a variable that happens to be spelled the same way.
    let by_fact = selecting_hop(
        0,
        StructuralAccessAggregate::Fact(FactId(42)),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "->{groups}",
        ValueShape::HashRef,
    )?;
    let by_boundary = StructuralAccessHop::new(
        0,
        StructuralAccessAggregate::DynamicBoundary(dynamic_boundary(
            BoundaryKind::DynamicValue,
            SemanticReasonCode::DynamicValue,
        )),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        spelling("->{groups}", 0, 10)?,
        StructuralHopOutcome::Boundary(dynamic_boundary(
            BoundaryKind::DynamicValue,
            SemanticReasonCode::DynamicValue,
        )),
        StructuralHopCertainty::Possible,
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::DynamicBoundary),
        SemanticConfidence::Known(Confidence::Low),
        SemanticReasonCode::DynamicValue,
        StructuralAccessBudget::new(10, 9)?,
        vec![StructuralAccessLimitation::OpenAggregate],
    )?;
    assert_ne!(by_fact.fingerprint(), by_boundary.fingerprint());
    assert_ne!(by_fact.aggregate.tag(), by_boundary.aggregate.tag());
    Ok(())
}

#[test]
fn negative_static_indices_stay_exact() -> Result<(), Box<dyn Error>> {
    let last = selecting_hop(
        0,
        StructuralAccessAggregate::Variable { sigil: "$".to_string(), name: "rows".to_string() },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(-1),
        "[-1]",
        ValueShape::Scalar,
    )?;
    let first = selecting_hop(
        0,
        StructuralAccessAggregate::Variable { sigil: "$".to_string(), name: "rows".to_string() },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(1),
        "[1]",
        ValueShape::Scalar,
    )?;
    assert_ne!(last.fingerprint(), first.fingerprint());
    Ok(())
}

#[test]
fn a_selecting_chain_reports_its_result() -> Result<(), Box<dyn Error>> {
    let chain = nested_chain()?;
    let selected = chain.selected().ok_or("the nested chain selects a value")?;
    assert!(matches!(
        selected,
        StructuralHopOutcome::Selected {
            shape: ValueShape::Object { .. },
            value_fact: Some(FactId(102))
        }
    ));
    Ok(())
}

// ── Ownership fence ───────────────────────────────────────────────────────

#[test]
fn architecture_fence_forbids_provider_parser_and_workspace_ownership() {
    let sources = [include_str!("mod.rs"), include_str!("hop.rs"), include_str!("chain.rs")];
    for source in sources {
        for forbidden in [
            "std::time::",
            "std::thread::",
            "tower_lsp",
            "lsp_types",
            "perl_parser",
            "perl_lsp",
            "perl_ast",
            "perl_workspace",
            "perl_semantic_analyzer",
        ] {
            assert!(
                !source.contains(forbidden),
                "the structural access contract must not own {forbidden}"
            );
        }
    }
}
