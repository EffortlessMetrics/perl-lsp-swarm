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

/// `%config` — the aggregate a plain `{}` subscript actually addresses.
fn hash_variable() -> StructuralAccessAggregate {
    StructuralAccessAggregate::Variable { sigil: "%".to_string(), name: "config".to_string() }
}

/// `@config` — the aggregate a plain `[]` subscript actually addresses.
fn array_variable() -> StructuralAccessAggregate {
    StructuralAccessAggregate::Variable { sigil: "@".to_string(), name: "config".to_string() }
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
    let operators: Vec<_> = chain.hops().iter().map(StructuralAccessHop::operator).collect();
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
                hash_variable(),
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
    // class": hops that differ only in written text and position are the same
    // access, and a hop whose text *looks* like an arrow is still classified
    // by its operator field alone.
    let build = |text: &str, start: u32, end: u32| {
        StructuralAccessHop::new(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("groups".to_string()),
            spelling(text, start, end)?,
            StructuralHopOutcome::Selected { shape: ValueShape::HashRef, value_fact: None },
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    let original = build("{groups}", 0, 8)?;

    // Reformatted and relocated: same access.
    let moved = build("  {groups}  ", 4096, 4108)?;
    assert_eq!(
        moved.fingerprint(),
        original.fingerprint(),
        "reformatting or moving a hop must not change what it is"
    );

    // Text containing an earlier arrow cannot make this a hashref slot.
    let arrow_text = build("->{outer}{groups}", 0, 17)?;
    assert_eq!(arrow_text.operator(), StructuralAccessOperator::HashSlot);
    assert!(!arrow_text.operator().dereferences());
    assert_eq!(arrow_text.fingerprint(), original.fingerprint());
    Ok(())
}

#[test]
fn a_dynamic_key_hop_never_digests_as_a_dynamic_index_hop() -> Result<(), Box<dyn Error>> {
    // Law 1 forbids pairing a dynamic index with a keyed operator, so the
    // realistic pair varies operator and selector together. The selector-kind
    // discriminant must still keep them apart on its own: dropping it would
    // leave both folding the identical boundary text.
    let boundary = dynamic_boundary(BoundaryKind::DynamicValue, SemanticReasonCode::DynamicValue);
    let build = |operator: StructuralAccessOperator, selector| {
        // The aggregate has to move with the operator: a plain `{}` addresses
        // `%config` and a plain `[]` addresses `@config`.
        let aggregate = if operator.is_keyed() { hash_variable() } else { array_variable() };
        StructuralAccessHop::new(
            0,
            aggregate,
            operator,
            selector,
            spelling("[$i]", 0, 4)?,
            StructuralHopOutcome::Boundary(boundary.clone()),
            StructuralHopCertainty::Possible,
            StructuralAggregateCompleteness::Open,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::DynamicBoundary),
            SemanticConfidence::Known(Confidence::Low),
            SemanticReasonCode::DynamicValue,
            StructuralAccessBudget::new(10, 9)?,
            vec![StructuralAccessLimitation::DynamicSelector],
        )
    };
    let keyed = build(
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::DynamicKey(boundary.clone()),
    )?;
    let indexed = build(
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::DynamicIndex(boundary.clone()),
    )?;
    assert_ne!(keyed.fingerprint(), indexed.fingerprint());
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
        array_variable(),
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
        hash_variable(),
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
    assert_ne!(by_fact.aggregate().tag(), by_boundary.aggregate().tag());
    Ok(())
}

#[test]
fn negative_static_indices_stay_exact() -> Result<(), Box<dyn Error>> {
    let last = selecting_hop(
        0,
        StructuralAccessAggregate::Variable { sigil: "@".to_string(), name: "rows".to_string() },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(-1),
        "[-1]",
        ValueShape::Scalar,
    )?;
    let first = selecting_hop(
        0,
        StructuralAccessAggregate::Variable { sigil: "@".to_string(), name: "rows".to_string() },
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

// ── Fingerprint field-boundary soundness ──────────────────────────────────

#[test]
fn a_sigil_name_boundary_shift_does_not_collide() -> Result<(), Box<dyn Error>> {
    // `$` + `ab` and `$a` + `b` concatenate to the same text. Folding the two
    // components as separate labelled fields keeps them distinct.
    //
    // This drives `fold` directly rather than building a hop, because the
    // canonical-sigil law now rejects `$a` at construction. That law removes
    // this collision class for *this* field by construction — a one-character
    // sigil from a closed set leaves no boundary to shift — but `fold` is
    // shared machinery, so the property it provides is still worth pinning
    // where an impossible record cannot mask it.
    let digest = |sigil: &str, name: &str| {
        StructuralAccessAggregate::Variable { sigil: sigil.to_string(), name: name.to_string() }
            .fold(SemanticIdentityFingerprint::new(STRUCTURAL_ACCESS_SCHEMA_TAG))
            .finish()
    };
    assert_ne!(digest("$", "ab"), digest("$a", "b"));
    Ok(())
}

#[test]
fn a_non_canonical_sigil_cannot_name_a_variable() -> Result<(), Box<dyn Error>> {
    // Perl has exactly five sigils. A free-form string here would let a
    // rendered label stand in for a typed identity — `receiver_facts.rs`
    // degrading a nested base to the AST kind name `Binary` is the exact
    // failure #13619 exists to remove, and `Binary` is non-blank.
    // Driven through an arrow operator on purpose: `->{}` dereferences
    // whatever the base holds, so it fixes no sigil and leaves this test
    // measuring only the canonical-sigil law. A plain `{}` would additionally
    // require `%`, and every control but one would fail for the wrong reason.
    let build = |sigil: &str| {
        selecting_hop(
            0,
            StructuralAccessAggregate::Variable {
                sigil: sigil.to_string(),
                name: "config".to_string(),
            },
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "->{k}",
            ValueShape::Scalar,
        )
    };
    for rejected in ["Binary", "$a", "->", "$$", " $ "] {
        contract_error(build(rejected))?;
    }
    // Negative control: every sigil Perl actually has must still build.
    // Every sigil Perl actually has must still build, each paired with an
    // operator it is legitimately used with: `%` and `@` take a plain
    // subscript of their own container, while `$`, `&` and `*` dereference
    // through an arrow. Pairing them any other way would be rejected by the
    // container laws rather than by this one.
    for (accepted, operator, selector, text) in [
        (
            "%",
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "{k}",
        ),
        (
            "@",
            StructuralAccessOperator::ArrayIndex,
            StructuralAccessSelector::StaticIndex(0),
            "[0]",
        ),
        (
            "$",
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "->{k}",
        ),
        (
            "&",
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "->{k}",
        ),
        (
            "*",
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "->{k}",
        ),
    ] {
        selecting_hop(
            0,
            StructuralAccessAggregate::Variable {
                sigil: accepted.to_string(),
                name: "config".to_string(),
            },
            operator,
            selector,
            text,
            ValueShape::Scalar,
        )?;
    }
    Ok(())
}

#[test]
fn a_non_canonical_sigil_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["aggregate"]["Variable"]["sigil"] = serde_json::json!("Binary");
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(decoded.validate().is_err(), "a rendered label must not reach the contract as a sigil");
    Ok(())
}

#[test]
fn a_subject_field_boundary_shift_does_not_collide() -> Result<(), Box<dyn Error>> {
    // A delimiter inside a workspace root or a generation must not shift
    // content across a field boundary.
    let build = |generation: &str, root: &str| -> Result<String, Box<dyn Error>> {
        let chain = StructuralAccessChain::new(
            StructuralAccessSubject::new(
                DOCUMENT,
                SourceGeneration::known(generation),
                Some(root.to_string()),
                None,
            )?,
            vec![selecting_hop(
                0,
                hash_variable(),
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("k".to_string()),
                "{k}",
                ValueShape::Scalar,
            )?],
        )?;
        Ok(chain.fingerprint())
    };
    assert_ne!(build("a|b", "c")?, build("a", "b|c")?);
    Ok(())
}

#[test]
fn an_unknown_generation_is_not_a_known_empty_generation() -> Result<(), Box<dyn Error>> {
    let build = |generation: SourceGeneration| -> Result<String, Box<dyn Error>> {
        let chain = StructuralAccessChain::new(
            StructuralAccessSubject::new(DOCUMENT, generation, None, None)?,
            vec![selecting_hop(
                0,
                hash_variable(),
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("k".to_string()),
                "{k}",
                ValueShape::Scalar,
            )?],
        )?;
        Ok(chain.fingerprint())
    };
    assert_ne!(build(SourceGeneration::Unknown)?, build(SourceGeneration::known(""))?);
    Ok(())
}

#[test]
fn an_absent_project_generation_is_not_an_unknown_one() -> Result<(), Box<dyn Error>> {
    let build = |project: Option<SourceGeneration>| -> Result<String, Box<dyn Error>> {
        let chain = StructuralAccessChain::new(
            StructuralAccessSubject::new(DOCUMENT, SourceGeneration::known("gen"), None, project)?,
            vec![selecting_hop(
                0,
                hash_variable(),
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("k".to_string()),
                "{k}",
                ValueShape::Scalar,
            )?],
        )?;
        Ok(chain.fingerprint())
    };
    assert_ne!(build(None)?, build(Some(SourceGeneration::Unknown))?);
    Ok(())
}

#[test]
fn an_absent_workspace_root_is_not_an_empty_one() -> Result<(), Box<dyn Error>> {
    // A blank root is rejected outright, so the only reachable comparison is
    // absent versus a real root; the presence discriminant keeps them apart
    // even if a future producer is allowed to record an empty one.
    let absent = StructuralAccessSubject::new(DOCUMENT, SourceGeneration::known("g"), None, None)?;
    let present = StructuralAccessSubject::new(
        DOCUMENT,
        SourceGeneration::known("g"),
        Some("root".to_string()),
        None,
    )?;
    assert_ne!(absent, present);
    Ok(())
}

#[test]
fn a_selected_value_fact_is_not_its_absence() -> Result<(), Box<dyn Error>> {
    let build = |value_fact: Option<FactId>| {
        StructuralAccessHop::new(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            spelling("{k}", 0, 3)?,
            StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact },
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    assert_ne!(build(None)?.fingerprint(), build(Some(FactId(0)))?.fingerprint());
    Ok(())
}

#[test]
fn a_static_key_never_digests_as_the_same_static_index() -> Result<(), Box<dyn Error>> {
    let keyed = selecting_hop(
        0,
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("5".to_string()),
        "{5}",
        ValueShape::Scalar,
    )?;
    let indexed = selecting_hop(
        0,
        array_variable(),
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(5),
        "[5]",
        ValueShape::Scalar,
    )?;
    assert_ne!(keyed.fingerprint(), indexed.fingerprint());
    Ok(())
}

#[test]
fn a_package_name_shape_never_digests_as_an_object_shape() -> Result<(), Box<dyn Error>> {
    let build = |shape: ValueShape| {
        selecting_hop(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "{k}",
            shape,
        )
    };
    let package = build(ValueShape::PackageName { package: "Foo:High".to_string() })?;
    let object =
        build(ValueShape::Object { package: "Foo".to_string(), confidence: Confidence::High })?;
    assert_ne!(package.fingerprint(), object.fingerprint());
    Ok(())
}

#[test]
fn spending_the_last_unit_is_not_itself_a_defect() -> Result<(), Box<dyn Error>> {
    // A producer that budgeted exactly enough spends its last unit and still
    // gets a definite answer. Nothing about that is dishonest.
    StructuralAccessHop::new(
        0,
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("k".to_string()),
        spelling("{k}", 0, 3)?,
        StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(1, 0)?,
        Vec::new(),
    )?;
    Ok(())
}

#[test]
fn a_hop_anchored_in_another_document_cannot_join_the_chain() -> Result<(), Box<dyn Error>> {
    // A chain is one access in one document; a hop anchored elsewhere is not
    // part of it.
    let foreign = StructuralAccessHop::new(
        0,
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("k".to_string()),
        StructuralAccessSpelling::new("{k}", SourceAnchor::new(None, FileId(999), 0, 3))?,
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
    )?;
    assert!(
        StructuralAccessChain::new(subject()?, vec![foreign]).is_err(),
        "a hop anchored in another document must not join this chain"
    );
    Ok(())
}

#[test]
fn a_blank_workspace_root_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["subject"]["workspace_root"] = serde_json::json!("   ");
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a blank workspace root must not survive the transport boundary"
    );
    Ok(())
}

#[test]
fn an_impossible_budget_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["budget"]["units_after"] = serde_json::json!(u32::MAX);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a hop that gained units must not survive the transport boundary"
    );
    Ok(())
}

// ── Laws found by review ──────────────────────────────────────────────────

#[test]
fn empty_and_blank_hash_keys_are_real_members() -> Result<(), Box<dyn Error>> {
    // `$h{""}` and `$h{" "}` are legal Perl and name distinct members —
    // verified against the interpreter, not assumed. An empty key is an
    // identity here, unlike an empty aggregate name.
    let build = |key: &str| {
        selecting_hop(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey(key.to_string()),
            "{...}",
            ValueShape::Scalar,
        )
    };
    let empty = build("")?;
    let blank = build(" ")?;
    let named = build("x")?;
    assert_ne!(empty.fingerprint(), blank.fingerprint());
    assert_ne!(empty.fingerprint(), named.fingerprint());
    Ok(())
}

#[test]
fn a_member_missing_from_a_closed_aggregate_is_absent_not_unknown() -> Result<(), Box<dyn Error>> {
    let build = |outcome, completeness| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("missing".to_string()),
            spelling("->{missing}", 0, 11)?,
            outcome,
            StructuralHopCertainty::Possible,
            completeness,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::Medium),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    // Both honest pairings build.
    build(StructuralHopOutcome::AbsentMember, StructuralAggregateCompleteness::Closed)?;
    build(StructuralHopOutcome::UnknownMember, StructuralAggregateCompleteness::Open)?;
    // Neither crossed pairing does.
    let error = contract_error(build(
        StructuralHopOutcome::UnknownMember,
        StructuralAggregateCompleteness::Closed,
    ))?;
    assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));
    Ok(())
}

#[test]
fn non_canonical_limitations_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["limitations"] = serde_json::json!(["OpenAggregate", "OpenAggregate"]);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "duplicate limitations must not survive the transport boundary"
    );

    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["limitations"] = serde_json::json!(["OpenAggregate", "DynamicSelector"]);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "unsorted limitations must not survive the transport boundary"
    );
    Ok(())
}

#[test]
fn boundaries_differing_only_in_disposition_do_not_collide() -> Result<(), Box<dyn Error>> {
    let build = |disposition, boundary_id| {
        let boundary = BoundaryLink::new(
            boundary_id,
            BoundaryKind::DynamicValue,
            disposition,
            SemanticReasonCode::DynamicValue,
        );
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::DynamicKey(boundary.clone()),
            spelling("->{$k}", 0, 6)?,
            StructuralHopOutcome::Boundary(boundary),
            StructuralHopCertainty::Possible,
            StructuralAggregateCompleteness::Open,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::DynamicBoundary),
            SemanticConfidence::Known(Confidence::Low),
            SemanticReasonCode::DynamicValue,
            StructuralAccessBudget::new(10, 9)?,
            vec![StructuralAccessLimitation::DynamicSelector],
        )
    };
    // A boundary that degrades is not one that refuses.
    let degrades = build(BoundaryDisposition::Degrade, Some(FactId(3)))?;
    let refuses = build(BoundaryDisposition::Refuse, Some(FactId(3)))?;
    assert_ne!(degrades.fingerprint(), refuses.fingerprint());

    // Nor is an identified boundary the same as an anonymous one.
    let anonymous = build(BoundaryDisposition::Degrade, None)?;
    assert_ne!(degrades.fingerprint(), anonymous.fingerprint());
    Ok(())
}

#[test]
fn an_operator_cannot_select_through_a_shape_that_cannot_carry_it() -> Result<(), Box<dyn Error>> {
    // `$a->{b}` selecting a hash reference, then indexed as an array.
    let head = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "->{b}",
        ValueShape::HashRef,
    )?;
    let indexed = selecting_hop(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(0),
        "[0]",
        ValueShape::Scalar,
    )?;
    let error =
        contract_error(StructuralAccessChain::new(subject()?, vec![head.clone(), indexed]))?;
    assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));

    // The honest record for the same source is a shape mismatch, and it builds.
    let mismatch = StructuralAccessHop::new(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(0),
        spelling("[0]", 10, 13)?,
        StructuralHopOutcome::ShapeMismatch { observed: ValueShape::HashRef },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(99, 98)?,
        Vec::new(),
    )?;
    StructuralAccessChain::new(subject()?, vec![head, mismatch])?;
    Ok(())
}

#[test]
fn every_outcome_kind_reaches_the_hop_fingerprint() -> Result<(), Box<dyn Error>> {
    // Tag distinctness alone does not prove the fingerprint separates these:
    // an implementation that dropped the `outcome-kind` discriminant would
    // still pass a tag comparison. These five outcomes are legal under one
    // identical set of companion fields, so any difference in their digests
    // can only come from the outcome itself.
    let build = |outcome| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            spelling("->{k}", 0, 5)?,
            outcome,
            StructuralHopCertainty::Possible,
            StructuralAggregateCompleteness::Open,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::Low),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    let fingerprints = [
        build(StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None })?,
        build(StructuralHopOutcome::UnknownMember)?,
        build(StructuralHopOutcome::ShapeMismatch { observed: ValueShape::ArrayRef })?,
        build(StructuralHopOutcome::StaleGeneration)?,
        build(StructuralHopOutcome::Boundary(dynamic_boundary(
            BoundaryKind::DynamicValue,
            SemanticReasonCode::DynamicValue,
        )))?,
    ]
    .iter()
    .map(StructuralAccessHop::fingerprint)
    .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(fingerprints.len(), 5, "each outcome kind must reach the digest");
    Ok(())
}

#[test]
fn workspace_root_presence_reaches_the_chain_fingerprint() -> Result<(), Box<dyn Error>> {
    // An absent root and a real root already differ by their field value.
    let with_root = StructuralAccessChain::new(
        StructuralAccessSubject::new(
            DOCUMENT,
            SourceGeneration::known("g"),
            Some("root".to_string()),
            None,
        )?,
        vec![selecting_hop(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "{k}",
            ValueShape::Scalar,
        )?],
    )?;
    let without_root = StructuralAccessChain::new(
        StructuralAccessSubject::new(DOCUMENT, SourceGeneration::known("g"), None, None)?,
        vec![selecting_hop(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "{k}",
            ValueShape::Scalar,
        )?],
    )?;
    assert_ne!(with_root.fingerprint(), without_root.fingerprint());

    // The presence discriminant earns its place only on the unvalidated
    // transport path. A blank root cannot be constructed, but it can be
    // deserialised, and `fingerprint()` does not validate — so without the
    // discriminant a blank root would digest identically to an absent one.
    let mut value = serde_json::to_value(&without_root)?;
    value["subject"]["workspace_root"] = serde_json::json!("");
    let blank_root: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(blank_root.validate().is_err(), "a blank root is still invalid");
    assert_ne!(
        blank_root.fingerprint(),
        without_root.fingerprint(),
        "a blank root must not digest as an absent one"
    );
    Ok(())
}

#[test]
fn a_shape_that_can_never_be_subscripted_cannot_carry_a_selection() -> Result<(), Box<dyn Error>> {
    // A code reference and a package name are defined values that cannot
    // become something else, so subscripting either is an error rather than an
    // access. Verified against the interpreter: `$coderef->{k}` is `Not a HASH
    // reference`, and `"Foo"->{k}` is `Can't use string ("Foo") as a HASH ref
    // while "strict refs" in use`. Any apparent selection through one is a
    // symbolic dereference, which has its own boundary and must be recorded
    // as one.
    //
    // `Scalar` is deliberately absent from this list: it does not distinguish
    // `undef`, which Perl autovivifies. See the autovivification test below.
    for shape in [ValueShape::CodeRef, ValueShape::PackageName { package: "Foo".to_string() }] {
        let head = selecting_hop(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("b".to_string()),
            "->{b}",
            shape.clone(),
        )?;
        let follower = selecting_hop(
            1,
            StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("c".to_string()),
            "{c}",
            ValueShape::Scalar,
        )?;
        let error = contract_error(StructuralAccessChain::new(subject()?, vec![head, follower]))?;
        assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));
    }
    Ok(())
}

#[test]
fn an_object_or_unknown_shape_constrains_nothing() -> Result<(), Box<dyn Error>> {
    // Negative control for the law above: a blessed reference may be a blessed
    // hash or a blessed array, and `Unknown` asserts nothing. Both must still
    // admit either operator, or the law would reject honest records.
    for shape in [
        ValueShape::Object { package: "Foo".to_string(), confidence: Confidence::High },
        ValueShape::Unknown,
    ] {
        for (operator, selector, text) in [
            (
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("c".to_string()),
                "{c}",
            ),
            (StructuralAccessOperator::ArrayIndex, StructuralAccessSelector::StaticIndex(0), "[0]"),
        ] {
            let head = selecting_hop(
                0,
                base_variable(),
                StructuralAccessOperator::HashRefSlot,
                StructuralAccessSelector::StaticKey("b".to_string()),
                "->{b}",
                shape.clone(),
            )?;
            let follower = selecting_hop(
                1,
                StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
                operator,
                selector,
                text,
                ValueShape::Scalar,
            )?;
            StructuralAccessChain::new(subject()?, vec![head, follower])?;
        }
    }
    Ok(())
}

#[test]
fn a_recorded_shape_mismatch_must_be_a_real_one() -> Result<(), Box<dyn Error>> {
    let head = |shape: ValueShape| {
        selecting_hop(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("b".to_string()),
            "->{b}",
            shape,
        )
    };
    let mismatch = |operator, selector, text: &str, observed: ValueShape| {
        StructuralAccessHop::new(
            1,
            StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
            operator,
            selector,
            spelling(text, 10, 13)?,
            StructuralHopOutcome::ShapeMismatch { observed },
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(99, 98)?,
            Vec::new(),
        )
    };

    // Honest: a hash reference, array-indexed, observing the hash reference.
    StructuralAccessChain::new(
        subject()?,
        vec![
            head(ValueShape::HashRef)?,
            mismatch(
                StructuralAccessOperator::ArrayIndex,
                StructuralAccessSelector::StaticIndex(0),
                "[0]",
                ValueShape::HashRef,
            )?,
        ],
    )?;

    // A mismatch claimed for an operator the shape does carry did not happen.
    let error = contract_error(StructuralAccessChain::new(
        subject()?,
        vec![
            head(ValueShape::HashRef)?,
            mismatch(
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("c".to_string()),
                "{c}",
                ValueShape::HashRef,
            )?,
        ],
    ))?;
    assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));

    // A mismatch observing a shape the predecessor did not select describes a
    // different aggregate.
    let error = contract_error(StructuralAccessChain::new(
        subject()?,
        vec![
            head(ValueShape::HashRef)?,
            mismatch(
                StructuralAccessOperator::ArrayIndex,
                StructuralAccessSelector::StaticIndex(0),
                "[0]",
                ValueShape::ArrayRef,
            )?,
        ],
    ))?;
    assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));
    Ok(())
}

#[test]
fn fingerprints_do_not_follow_rust_debug_names() -> Result<(), Box<dyn Error>> {
    // Borrowed vocabulary is folded through explicit tags, not `Debug`, so a
    // variant rename cannot silently change a persisted digest under an
    // unchanged schema version. Pinning one digest catches an accidental
    // return to `format!("{:?}")`, which would spell these differently.
    let hop = StructuralAccessHop::new(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::DynamicKey(BoundaryLink::new(
            None,
            BoundaryKind::SymbolicReference,
            BoundaryDisposition::Refuse,
            SemanticReasonCode::UnsupportedEffect,
        )),
        spelling("->{$k}", 0, 6)?,
        StructuralHopOutcome::Selected {
            shape: ValueShape::Object { package: "Foo".to_string(), confidence: Confidence::High },
            value_fact: None,
        },
        StructuralHopCertainty::Possible,
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::DynamicBoundary),
        SemanticConfidence::Known(Confidence::Low),
        SemanticReasonCode::DynamicValue,
        StructuralAccessBudget::new(10, 9)?,
        Vec::new(),
    )?;
    assert_eq!(
        hop.fingerprint(),
        "ad0f13dad1270b7f4ba3f7d1b9228275",
        "schema v1 hop digests are stable; bump the schema tag to change them"
    );
    Ok(())
}

#[test]
fn an_outcome_independent_of_the_aggregate_stays_definite_when_it_moves()
-> Result<(), Box<dyn Error>> {
    // A budget definitely ran out, a generation is definitely stale, and a
    // boundary definitely stopped the hop — none of which depends on what the
    // aggregate turned out to hold, so escape or mutation cannot undermine
    // them. Requiring stability here would reject honest records.
    let build = |outcome: StructuralHopOutcome, disposition, budget| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            spelling("->{k}", 0, 5)?,
            outcome,
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Open,
            disposition,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::Low),
            SemanticReasonCode::ExactSource,
            budget,
            Vec::new(),
        )
    };
    let nominal = StructuralAccessBudget::new(10, 9)?;
    let spent = StructuralAccessBudget::new(1, 0)?;
    for disposition in [
        StructuralAggregateDisposition::Escaped,
        StructuralAggregateDisposition::Mutated,
        StructuralAggregateDisposition::EscapedAndMutated,
    ] {
        build(StructuralHopOutcome::StaleGeneration, disposition, nominal)?;
        build(StructuralHopOutcome::BudgetExhausted, disposition, spent)?;
        build(
            StructuralHopOutcome::Boundary(dynamic_boundary(
                BoundaryKind::DynamicValue,
                SemanticReasonCode::DynamicValue,
            )),
            disposition,
            nominal,
        )?;

        // The content-dependent outcomes stay constrained.
        let error =
            contract_error(build(StructuralHopOutcome::UnknownMember, disposition, nominal))?;
        assert!(matches!(error, StructuralAccessContractError::ContradictoryStatus(_)));
    }
    Ok(())
}

// ── Package identity in outcome shapes (found by Devin review) ────────────

#[test]
fn an_empty_object_package_is_rejected_at_construction() -> Result<(), Box<dyn Error>> {
    // `bless $ref, ""` is accepted by the interpreter, but it warns and the
    // resulting class is `main`, not the empty string. A record claiming an
    // object blessed into "" therefore names something that cannot exist.
    let build = |package: &str| {
        selecting_hop(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "{k}",
            ValueShape::Object { package: package.to_string(), confidence: Confidence::High },
        )
    };
    let error = contract_error(build(""))?;
    assert!(
        matches!(error, StructuralAccessContractError::EmptyIdentityField(field)
            if field == "ValueShape::Object.package"),
        "an empty object package must be rejected, got {error:?}"
    );
    // Negative control: the class the interpreter actually produces for that
    // blessing, and an ordinary one, both remain constructible.
    build("main")?;
    build("Staff")?;
    Ok(())
}

#[test]
fn a_whitespace_object_package_is_a_real_class_and_is_admitted() -> Result<(), Box<dyn Error>> {
    // `bless {}, "  "` yields an object whose `ref` is "  ", and method
    // dispatch through it resolves against the "  " symbol-table entry. It is
    // perverse, but it is a real package, and this law cannot tell a real class
    // from a blank one by trimming — so it must not try. Rejecting this is the
    // over-strict failure the crate guide warns about.
    selecting_hop(
        0,
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("k".to_string()),
        "{k}",
        ValueShape::Object { package: "  ".to_string(), confidence: Confidence::High },
    )?;
    Ok(())
}

#[test]
fn an_empty_package_name_shape_is_rejected_at_construction() -> Result<(), Box<dyn Error>> {
    // `""->method` dies with `Can't call method "..." without a package or
    // object reference`, so the empty string is not a usable class name.
    let build = |package: &str| {
        selecting_hop(
            0,
            hash_variable(),
            StructuralAccessOperator::HashSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            "{k}",
            ValueShape::PackageName { package: package.to_string() },
        )
    };
    let error = contract_error(build(""))?;
    assert!(
        matches!(error, StructuralAccessContractError::EmptyIdentityField(field)
            if field == "ValueShape::PackageName.package"),
        "an empty package name must be rejected, got {error:?}"
    );
    build("Foo::Bar")?;
    // Negative control on the value side: `"  "->method` dispatches, so a
    // whitespace package name is as real here as it is for a blessed object.
    build("  ")?;
    Ok(())
}

#[test]
fn an_empty_package_in_a_shape_mismatch_is_rejected() -> Result<(), Box<dyn Error>> {
    // The mismatch arm carries an identity for the same reason the selected
    // arm does: it names the shape the aggregate actually had.
    // The mismatch is recorded over an *arrow* subscript on a scalar, which is
    // where a mismatch can honestly occur. A plain `%config` subscript cannot
    // mismatch its own container (law 12), so building this fixture that way
    // would make the test pass on the wrong law and its control unbuildable.
    let build = |package: &str| {
        StructuralAccessHop::new(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            spelling("->{k}", 0, 5)?,
            StructuralHopOutcome::ShapeMismatch {
                observed: ValueShape::PackageName { package: package.to_string() },
            },
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(10, 9)?,
            Vec::new(),
        )
    };
    contract_error(build(""))?;
    build("Foo")?;
    Ok(())
}

#[test]
fn an_empty_object_package_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["outcome"]["Selected"]["shape"] =
        serde_json::json!({ "Object": { "package": "", "confidence": "High" } });
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "an empty object package must not survive the transport boundary"
    );
    Ok(())
}

#[test]
fn an_honest_object_package_still_survives_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    // Negative control for the law above: the same substitution with a real
    // package name must still validate, so the law rejects blankness rather
    // than the shape.
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["outcome"]["Selected"]["shape"] =
        serde_json::json!({ "Object": { "package": "Staff", "confidence": "High" } });
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    decoded.validate()?;
    Ok(())
}

// ── Member-level answers through an incompatible shape (found by Devin) ───

#[test]
fn an_operator_cannot_reach_a_member_through_a_shape_that_cannot_carry_it()
-> Result<(), Box<dyn Error>> {
    // `$a->{b}` selects a hash reference; the next hop indexes it as an array.
    // A claimed *selection* was already refused. A claimed absence is just as
    // dishonest: `$hashref->[0]` is not an array whose element 0 is missing,
    // so recording `AbsentMember` would collapse wrong-shape into legitimate
    // absence — a distinction #13619 explicitly requires be kept.
    let head = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "->{b}",
        ValueShape::HashRef,
    )?;
    let build = |outcome, completeness, budget| {
        StructuralAccessHop::new(
            1,
            StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
            StructuralAccessOperator::ArrayIndex,
            StructuralAccessSelector::StaticIndex(0),
            spelling("[0]", 10, 13)?,
            outcome,
            StructuralHopCertainty::Definite,
            completeness,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            budget,
            Vec::new(),
        )
    };
    let chain = |hop| StructuralAccessChain::new(subject()?, vec![head.clone(), hop]);
    let ordinary = StructuralAccessBudget::new(99, 98)?;

    // Rejected: both member-level answers.
    contract_error(chain(build(
        StructuralHopOutcome::AbsentMember,
        StructuralAggregateCompleteness::Closed,
        ordinary,
    )?))?;
    contract_error(chain(build(
        StructuralHopOutcome::UnknownMember,
        StructuralAggregateCompleteness::Open,
        ordinary,
    )?))?;

    // Accepted: the honest conflict record, and the three outcomes that
    // stopped before any member lookup could happen. Refusing these would
    // reject truthful records.
    chain(build(
        StructuralHopOutcome::ShapeMismatch { observed: ValueShape::HashRef },
        StructuralAggregateCompleteness::Closed,
        ordinary,
    )?)?;
    chain(build(
        StructuralHopOutcome::StaleGeneration,
        StructuralAggregateCompleteness::Closed,
        ordinary,
    )?)?;
    chain(build(
        StructuralHopOutcome::BudgetExhausted,
        StructuralAggregateCompleteness::Closed,
        StructuralAccessBudget::new(99, 0)?,
    )?)?;
    chain(build(
        StructuralHopOutcome::Boundary(dynamic_boundary(
            BoundaryKind::SymbolicReference,
            SemanticReasonCode::UnsupportedEffect,
        )),
        StructuralAggregateCompleteness::Closed,
        ordinary,
    )?)?;
    Ok(())
}

#[test]
fn a_permissive_shape_still_admits_a_member_level_answer() -> Result<(), Box<dyn Error>> {
    // Negative control for the law above: `Unknown` and `Object` assert too
    // little to contradict any operator, so an absence recorded after one of
    // them is honest and must still validate. Without this, the law could be
    // tightened into rejecting every non-selecting follow-on.
    for shape in [
        ValueShape::Unknown,
        ValueShape::Object { package: "Staff".to_string(), confidence: Confidence::High },
    ] {
        let head = selecting_hop(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("b".to_string()),
            "->{b}",
            shape,
        )?;
        let absent = StructuralAccessHop::new(
            1,
            StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
            StructuralAccessOperator::ArrayIndex,
            StructuralAccessSelector::StaticIndex(0),
            spelling("[0]", 10, 13)?,
            StructuralHopOutcome::AbsentMember,
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::SemanticAnalyzer,
            SemanticProvenance::Known(crate::Provenance::ExactAst),
            SemanticConfidence::Known(Confidence::High),
            SemanticReasonCode::ExactSource,
            StructuralAccessBudget::new(99, 98)?,
            Vec::new(),
        )?;
        StructuralAccessChain::new(subject()?, vec![head, absent])?;
    }
    Ok(())
}

#[test]
fn a_dishonest_absence_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    // The serialised chain is the honest two-hop record: a hash reference,
    // then an array index recorded as the mismatch it is. Only the final hop
    // is mutated, and only its outcome, so the "no member through an
    // incompatible shape" law is the single law left to reject it.
    //
    // This deliberately does not mutate `nested_chain`: that chain has three
    // hops, so turning a middle hop non-selecting is caught by the "only the
    // final hop may fail to select" law instead, and the test would pass
    // without the law it claims to cover.
    let head = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "->{b}",
        ValueShape::HashRef,
    )?;
    let mismatch = StructuralAccessHop::new(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(0),
        spelling("[0]", 10, 13)?,
        StructuralHopOutcome::ShapeMismatch { observed: ValueShape::HashRef },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(99, 98)?,
        Vec::new(),
    )?;
    let honest = StructuralAccessChain::new(subject()?, vec![head, mismatch])?;

    let mut value = serde_json::to_value(&honest)?;
    value["hops"][1]["outcome"] = serde_json::json!("AbsentMember");
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "an absence claimed through an incompatible shape must not survive transport"
    );
    // Control: the unmutated chain validates, so the rejection is the outcome
    // swap and not some incidental defect in the fixture.
    honest.validate()?;
    Ok(())
}

// ── Autovivification through an undef scalar (found by Devin) ─────────────

#[test]
fn an_undef_scalar_can_still_be_subscripted() -> Result<(), Box<dyn Error>> {
    // `ValueShape::Scalar` does not distinguish `undef` from a defined
    // non-reference, and `undef` is subscriptable: Perl autovivifies it.
    // Verified against the interpreter rather than reasoned about:
    //
    //   my $x; $x->{k} = 1;   # $x is now a HASH reference
    //   my $y; $y->[0] = 1;   # $y is now an ARRAY reference
    //   my $z; my $v = $z->{k};   # rvalue: succeeds, $z autovivifies to HASH
    //
    // Treating every `Scalar` as decisively non-subscriptable rejected that
    // honest chain, so `Scalar` is permissive and non-decisive. It asserts too
    // little to contradict an operator, exactly like `Object` and `Unknown`.
    for operator in [
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessOperator::ArrayRefIndex,
        StructuralAccessOperator::HashSlot,
        StructuralAccessOperator::ArrayIndex,
    ] {
        let head = selecting_hop(
            0,
            base_variable(),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("b".to_string()),
            "->{b}",
            ValueShape::Scalar,
        )?;
        let selector = if operator.is_keyed() {
            StructuralAccessSelector::StaticKey("c".to_string())
        } else {
            StructuralAccessSelector::StaticIndex(0)
        };
        let follower = selecting_hop(
            1,
            StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
            operator,
            selector,
            "{c}",
            ValueShape::Scalar,
        )?;
        StructuralAccessChain::new(subject()?, vec![head, follower])?;
    }
    Ok(())
}

#[test]
fn an_undef_scalar_cannot_be_contradicted_by_a_shape_mismatch() -> Result<(), Box<dyn Error>> {
    // The other half of non-decisiveness. Law 8 refuses a `ShapeMismatch`
    // whose operator the predecessor's shape does carry — but only where the
    // shape is decisive. `Scalar` is not, so a producer that did observe a
    // mismatch against something it could only classify as a scalar must
    // still be able to record it.
    let head = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "->{b}",
        ValueShape::Scalar,
    )?;
    let mismatch = StructuralAccessHop::new(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(0),
        spelling("[0]", 10, 13)?,
        StructuralHopOutcome::ShapeMismatch { observed: ValueShape::Scalar },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(99, 98)?,
        Vec::new(),
    )?;
    StructuralAccessChain::new(subject()?, vec![head, mismatch])?;
    Ok(())
}

#[test]
fn an_autovivified_access_survives_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    // The permissive reading must hold across serde too, not only in the
    // constructor: a chain whose first hop selected an undef scalar and whose
    // second subscripts it validates after a round trip.
    let head = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("b".to_string()),
        "->{b}",
        ValueShape::Scalar,
    )?;
    let follower = selecting_hop(
        1,
        StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("c".to_string()),
        "{c}",
        ValueShape::Scalar,
    )?;
    let chain = StructuralAccessChain::new(subject()?, vec![head, follower])?;
    let decoded: StructuralAccessChain = serde_json::from_str(&serde_json::to_string(&chain)?)?;
    decoded.validate()?;
    assert_eq!(decoded, chain);
    Ok(())
}

// ── Refused promotion (found by Devin) ────────────────────────────────────

/// A hop whose selector or aggregate stands behind a chosen boundary.
fn hop_behind_boundary(
    disposition: BoundaryDisposition,
    on_selector: bool,
    value_fact: Option<FactId>,
) -> Result<StructuralAccessHop, StructuralAccessContractError> {
    let link = BoundaryLink::new(
        Some(FactId(3)),
        BoundaryKind::DynamicValue,
        disposition,
        SemanticReasonCode::DynamicValue,
    );
    let (aggregate, selector) = if on_selector {
        (base_variable(), StructuralAccessSelector::DynamicKey(link))
    } else {
        (
            StructuralAccessAggregate::DynamicBoundary(link),
            StructuralAccessSelector::StaticKey("k".to_string()),
        )
    };
    StructuralAccessHop::new(
        0,
        aggregate,
        StructuralAccessOperator::HashRefSlot,
        selector,
        spelling("->{$k}", 0, 6)?,
        StructuralHopOutcome::Selected { shape: ValueShape::ArrayRef, value_fact },
        StructuralHopCertainty::Possible,
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::DynamicBoundary),
        SemanticConfidence::Known(Confidence::Low),
        SemanticReasonCode::DynamicValue,
        StructuralAccessBudget::new(10, 9)?,
        Vec::new(),
    )
}

#[test]
fn a_refusing_boundary_cannot_carry_a_promoted_value_fact() -> Result<(), Box<dyn Error>> {
    // `BoundaryDisposition::Refuse` refuses *promotion*, and `value_fact` is
    // the canonical fact identity "when promoted". Recording one through the
    // other is a record contradicting itself, on either carrier.
    for on_selector in [true, false] {
        let error = contract_error(hop_behind_boundary(
            BoundaryDisposition::Refuse,
            on_selector,
            Some(FactId(42)),
        ))?;
        assert!(
            matches!(error, StructuralAccessContractError::ContradictoryStatus(_)),
            "a refused promotion must be rejected, got {error:?}"
        );
    }
    Ok(())
}

#[test]
fn a_refusing_boundary_still_admits_a_shape_without_promotion() -> Result<(), Box<dyn Error>> {
    // Negative control, and the reason this law is narrow. A shape claim is
    // not a promotion: a producer that refuses to evaluate a dynamic key may
    // still know every value in the hash is an array reference — "I will not
    // say which member, I will say what shape a member has". Refusing this
    // would reject an honest record, and `SemanticFactEnvelope::status`
    // already treats a refusing boundary as a status rather than as an
    // impossible record.
    for on_selector in [true, false] {
        hop_behind_boundary(BoundaryDisposition::Refuse, on_selector, None)?;
    }
    Ok(())
}

#[test]
fn a_degrading_boundary_still_admits_a_promoted_value_fact() -> Result<(), Box<dyn Error>> {
    // The other control: only `Refuse` refuses promotion. A degrading boundary
    // permits a degraded answer, promotion included, so the law must key on
    // the disposition and not merely on the presence of a boundary.
    for on_selector in [true, false] {
        hop_behind_boundary(BoundaryDisposition::Degrade, on_selector, Some(FactId(42)))?;
    }
    Ok(())
}

#[test]
fn a_refused_promotion_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let honest = hop_behind_boundary(BoundaryDisposition::Refuse, true, None)?;
    let chain = StructuralAccessChain::new(subject()?, vec![honest])?;
    let mut value = serde_json::to_value(&chain)?;
    value["hops"][0]["outcome"]["Selected"]["value_fact"] = serde_json::json!(42);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a promoted fact behind a refusing boundary must not survive transport"
    );
    chain.validate()?;
    Ok(())
}

// ── Plain subscripts address their own container (found by Devin) ─────────

#[test]
fn a_plain_subscript_must_name_its_own_container() -> Result<(), Box<dyn Error>> {
    // `$config{groups}` reads `%config` and `$config[0]` reads `@config`. The
    // leading `$` belongs to the element, not to the aggregate, and all three
    // of `$config`, `@config` and `%config` can coexist as distinct variables
    // — verified against the interpreter. Recording the aggregate as
    // `$config` therefore names a different variable than the access reads.
    let build = |sigil: &str, operator, text| {
        selecting_hop(
            0,
            StructuralAccessAggregate::Variable {
                sigil: sigil.to_string(),
                name: "config".to_string(),
            },
            operator,
            if operator == StructuralAccessOperator::HashSlot {
                StructuralAccessSelector::StaticKey("groups".to_string())
            } else {
                StructuralAccessSelector::StaticIndex(0)
            },
            text,
            ValueShape::Scalar,
        )
    };
    // A plain hash slot on anything but `%`, and a plain index on anything
    // but `@`, names the wrong variable.
    for wrong in ["$", "@", "&", "*"] {
        contract_error(build(wrong, StructuralAccessOperator::HashSlot, "{groups}"))?;
    }
    for wrong in ["$", "%", "&", "*"] {
        contract_error(build(wrong, StructuralAccessOperator::ArrayIndex, "[0]"))?;
    }
    // The honest records for the same source.
    build("%", StructuralAccessOperator::HashSlot, "{groups}")?;
    build("@", StructuralAccessOperator::ArrayIndex, "[0]")?;
    Ok(())
}

#[test]
fn an_arrow_subscript_does_not_fix_the_aggregate_sigil() -> Result<(), Box<dyn Error>> {
    // Negative control, and the boundary of the law above. `->{}` and `->[]`
    // dereference whatever the base holds, so the operator does not fix the
    // sigil the way a plain subscript does.
    //
    // `@` and `%` are excluded because Perl rejects an array or hash used as a
    // reference; that is law 11 and its own test. The three that remain all
    // dereference legitimately, verified against the interpreter:
    // `$r->{k}` ordinarily, `&foo->{k}` through the call's result, and
    // `*STDOUT->{IO}` as a glob slot.
    for sigil in ["$", "&", "*"] {
        for (operator, selector, text) in [
            (
                StructuralAccessOperator::HashRefSlot,
                StructuralAccessSelector::StaticKey("k".to_string()),
                "->{k}",
            ),
            (
                StructuralAccessOperator::ArrayRefIndex,
                StructuralAccessSelector::StaticIndex(0),
                "->[0]",
            ),
        ] {
            selecting_hop(
                0,
                StructuralAccessAggregate::Variable {
                    sigil: sigil.to_string(),
                    name: "config".to_string(),
                },
                operator,
                selector,
                text,
                ValueShape::Scalar,
            )?;
        }
    }
    Ok(())
}

#[test]
fn a_wrong_container_sigil_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>> {
    let honest = selecting_hop(
        0,
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "{groups}",
        ValueShape::Scalar,
    )?;
    let chain = StructuralAccessChain::new(subject()?, vec![honest])?;
    let mut value = serde_json::to_value(&chain)?;
    value["hops"][0]["aggregate"]["Variable"]["sigil"] = serde_json::json!("$");
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a plain subscript naming a scalar of the same name must not survive transport"
    );
    chain.validate()?;
    Ok(())
}

#[test]
fn an_arrow_cannot_dereference_an_array_or_hash_container() -> Result<(), Box<dyn Error>> {
    // `@a->[0]` is "Can't use an array as a reference" and `%h->{k}` is
    // "Can't use a hash as a reference" — verified against the interpreter.
    // No member is reachable through either, so a hop claiming one is
    // impossible.
    let build = |sigil: &str, outcome, completeness| {
        StructuralAccessHop::new(
            0,
            StructuralAccessAggregate::Variable {
                sigil: sigil.to_string(),
                name: "config".to_string(),
            },
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("k".to_string()),
            spelling("->{k}", 0, 5)?,
            outcome,
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
    for sigil in ["@", "%"] {
        for (outcome, completeness) in [
            (
                StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None },
                StructuralAggregateCompleteness::Closed,
            ),
            (StructuralHopOutcome::AbsentMember, StructuralAggregateCompleteness::Closed),
            (StructuralHopOutcome::UnknownMember, StructuralAggregateCompleteness::Open),
        ] {
            contract_error(build(sigil, outcome, completeness))?;
        }
        // Honest records for the same source: the access really did fail, and
        // saying so is exactly what these outcomes are for. Refusing them too
        // would leave no way to record what happened.
        build(
            sigil,
            StructuralHopOutcome::ShapeMismatch { observed: ValueShape::Unknown },
            StructuralAggregateCompleteness::Closed,
        )?;
        build(
            sigil,
            StructuralHopOutcome::Boundary(dynamic_boundary(
                BoundaryKind::Unsupported,
                SemanticReasonCode::UnsupportedEffect,
            )),
            StructuralAggregateCompleteness::Closed,
        )?;
    }
    // Control: the three sigils an arrow can dereference still select.
    for sigil in ["$", "&", "*"] {
        build(
            sigil,
            StructuralHopOutcome::Selected { shape: ValueShape::Scalar, value_fact: None },
            StructuralAggregateCompleteness::Closed,
        )?;
    }
    Ok(())
}

#[test]
fn an_arrow_through_an_array_container_cannot_survive_the_transport_boundary()
-> Result<(), Box<dyn Error>> {
    let honest = selecting_hop(
        0,
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("groups".to_string()),
        "->{groups}",
        ValueShape::Scalar,
    )?;
    let chain = StructuralAccessChain::new(subject()?, vec![honest])?;
    let mut value = serde_json::to_value(&chain)?;
    value["hops"][0]["aggregate"]["Variable"]["sigil"] = serde_json::json!("@");
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "an arrow dereference of an array container must not survive transport"
    );
    chain.validate()?;
    Ok(())
}

// ── The limit of the ordinal predecessor link (found by Devin review) ─────

#[test]
fn a_renumbered_middle_hop_deletion_still_validates() -> Result<(), Box<dyn Error>> {
    // The ordinal predecessor link catches an *incoherent* chain: a hop naming
    // a non-adjacent predecessor, a gap or duplicate in the ordinals, a reorder
    // that leaves the ordinals inconsistent. It cannot catch a deletion that
    // renumbers what follows it, and no self-contained validator could: the
    // shortened chain is a faithful description of a *different* real access.
    //
    // `$config->{a}{b}{c}` with `{b}` removed and `{c}` renumbered is exactly
    // `$config->{a}{c}`, which an honest producer may emit for real source.
    // Rejecting it would reject that source. This test pins the boundary so a
    // future change that claims to close it has something to flip.
    let build = |ordinal: u32, aggregate, key: &str| {
        selecting_hop(
            ordinal,
            aggregate,
            if ordinal == 0 {
                StructuralAccessOperator::HashRefSlot
            } else {
                StructuralAccessOperator::HashSlot
            },
            StructuralAccessSelector::StaticKey(key.to_string()),
            if ordinal == 0 { "->{a}" } else { "{k}" },
            ValueShape::HashRef,
        )
    };
    let full = StructuralAccessChain::new(
        subject()?,
        vec![
            build(0, base_variable(), "a")?,
            build(1, StructuralAccessAggregate::PrecedingHop { ordinal: 0 }, "b")?,
            build(2, StructuralAccessAggregate::PrecedingHop { ordinal: 1 }, "c")?,
        ],
    )?;

    // Delete the middle hop and renumber the survivor, as a tamperer would.
    let mut value = serde_json::to_value(&full)?;
    let hops = value["hops"].as_array_mut().ok_or("hops must be an array")?;
    hops.remove(1);
    hops[1]["ordinal"] = serde_json::json!(1);
    hops[1]["aggregate"] = serde_json::json!({ "PrecedingHop": { "ordinal": 0 } });
    let shortened: StructuralAccessChain = serde_json::from_value(value)?;

    // The boundary: validation accepts it, because it describes real source.
    shortened.validate()?;

    // What actually distinguishes it is identity, not validity. The chain
    // fingerprint folds every hop in order, so the shortened chain cannot be
    // mistaken for the original by a consumer that kept the original digest.
    assert_ne!(
        shortened.fingerprint(),
        full.fingerprint(),
        "a shortened chain must not share the original's identity"
    );
    assert_eq!(shortened.hops().len(), 2, "the deletion really happened");
    Ok(())
}

#[test]
fn a_deletion_without_renumbering_is_still_rejected() -> Result<(), Box<dyn Error>> {
    // Negative control for the boundary above: the ordinal link does its actual
    // job. Removing a hop without renumbering leaves the ordinals non-dense and
    // a predecessor reference that no longer names `ordinal - 1`, and that is
    // mechanically detected.
    let mut value = serde_json::to_value(nested_chain()?)?;
    let hops = value["hops"].as_array_mut().ok_or("hops must be an array")?;
    hops.remove(1);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a deletion that leaves the ordinals inconsistent must be rejected"
    );
    Ok(())
}

// ── Limitations restating a typed field (found by Devin review) ───────────

/// Build a hop carrying `limitations` against the given honesty fields.
///
/// The outcome is a parameter because completeness already constrains it under
/// law 3 — an open aggregate takes `UnknownMember` and a closed one
/// `AbsentMember` — and a fixture that ignored that would fail on law 3 while
/// appearing to exercise law 11.
fn hop_with_limitations(
    completeness: StructuralAggregateCompleteness,
    disposition: StructuralAggregateDisposition,
    outcome: StructuralHopOutcome,
    limitations: Vec<StructuralAccessLimitation>,
) -> Result<StructuralAccessHop, StructuralAccessContractError> {
    StructuralAccessHop::new(
        0,
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("k".to_string()),
        spelling("{k}", 0, 3)?,
        outcome,
        StructuralHopCertainty::Possible,
        completeness,
        disposition,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(10, 9)?,
        limitations,
    )
}

#[test]
fn an_open_aggregate_limitation_cannot_annotate_a_closed_aggregate() -> Result<(), Box<dyn Error>> {
    // `OpenAggregate` says the member set is not closed; `Closed` says every
    // member is known. One record cannot say both.
    let error = contract_error(hop_with_limitations(
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        StructuralHopOutcome::AbsentMember,
        vec![StructuralAccessLimitation::OpenAggregate],
    ))?;
    assert!(
        matches!(error, StructuralAccessContractError::ContradictoryStatus(_)),
        "an open-aggregate limitation on a closed aggregate must be rejected, got {error:?}"
    );
    // Negative control: the same limitation on the aggregate it describes.
    hop_with_limitations(
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        StructuralHopOutcome::UnknownMember,
        vec![StructuralAccessLimitation::OpenAggregate],
    )?;
    Ok(())
}

#[test]
fn a_mutation_limitation_cannot_annotate_a_stable_aggregate() -> Result<(), Box<dyn Error>> {
    let error = contract_error(hop_with_limitations(
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Stable,
        StructuralHopOutcome::UnknownMember,
        vec![StructuralAccessLimitation::MutatedAggregate],
    ))?;
    assert!(
        matches!(error, StructuralAccessContractError::ContradictoryStatus(_)),
        "a mutation limitation on a stable aggregate must be rejected, got {error:?}"
    );
    // Both dispositions that genuinely include mutation must still validate.
    hop_with_limitations(
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Mutated,
        StructuralHopOutcome::UnknownMember,
        vec![StructuralAccessLimitation::MutatedAggregate],
    )?;
    hop_with_limitations(
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::EscapedAndMutated,
        StructuralHopOutcome::UnknownMember,
        vec![StructuralAccessLimitation::MutatedAggregate],
    )?;
    Ok(())
}

#[test]
fn an_escape_limitation_cannot_annotate_an_unescaped_aggregate() -> Result<(), Box<dyn Error>> {
    // `Mutated` is the discriminating case: it is not `Stable`, but it is not
    // escape either, so a law testing merely "not stable" would wrongly pass.
    let error = contract_error(hop_with_limitations(
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Mutated,
        StructuralHopOutcome::UnknownMember,
        vec![StructuralAccessLimitation::EscapedAggregate],
    ))?;
    assert!(
        matches!(error, StructuralAccessContractError::ContradictoryStatus(_)),
        "an escape limitation on a merely mutated aggregate must be rejected, got {error:?}"
    );
    hop_with_limitations(
        StructuralAggregateCompleteness::Open,
        StructuralAggregateDisposition::Escaped,
        StructuralHopOutcome::UnknownMember,
        vec![StructuralAccessLimitation::EscapedAggregate],
    )?;
    Ok(())
}

#[test]
fn limitations_that_restate_no_field_stay_unconstrained() -> Result<(), Box<dyn Error>> {
    // The law must not widen. `StaleDependency` is about a dependency's
    // generation, not this aggregate's, and `BudgetExhausted` as a limitation
    // is deliberately weaker than the outcome of the same name — only that
    // outcome forces zero remaining units. Both must annotate a stable, closed
    // aggregate with units left over.
    hop_with_limitations(
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        StructuralHopOutcome::AbsentMember,
        vec![
            StructuralAccessLimitation::DynamicSelector,
            StructuralAccessLimitation::RecoveredSyntax,
            StructuralAccessLimitation::BudgetExhausted,
            StructuralAccessLimitation::StaleDependency,
            StructuralAccessLimitation::CompatibilityBridge,
            StructuralAccessLimitation::Unsupported,
        ],
    )?;
    Ok(())
}

#[test]
fn a_contradictory_limitation_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>>
{
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][0]["limitations"] = serde_json::json!(["MutatedAggregate"]);
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a limitation contradicting the disposition must not survive transport"
    );
    Ok(())
}

// ── Plain subscripts cannot mismatch their own sigil (found by Devin) ─────

/// A hop whose outcome is a shape mismatch, over the given aggregate/operator.
fn mismatching_hop(
    aggregate: StructuralAccessAggregate,
    operator: StructuralAccessOperator,
    selector: StructuralAccessSelector,
    text: &str,
    observed: ValueShape,
) -> Result<StructuralAccessHop, StructuralAccessContractError> {
    StructuralAccessHop::new(
        0,
        aggregate,
        operator,
        selector,
        spelling(text, 0, 4)?,
        StructuralHopOutcome::ShapeMismatch { observed },
        StructuralHopCertainty::Definite,
        StructuralAggregateCompleteness::Closed,
        StructuralAggregateDisposition::Stable,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(crate::Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(10, 9)?,
        Vec::new(),
    )
}

#[test]
fn a_plain_hash_subscript_cannot_mismatch_its_own_hash() -> Result<(), Box<dyn Error>> {
    // `$config{k}` reads `%config`, which is a hash in every execution. A
    // recorded mismatch describes a conflict Perl cannot produce.
    let error = contract_error(mismatching_hop(
        hash_variable(),
        StructuralAccessOperator::HashSlot,
        StructuralAccessSelector::StaticKey("k".to_string()),
        "{k}",
        ValueShape::ArrayRef,
    ))?;
    assert!(
        matches!(error, StructuralAccessContractError::ContradictoryStatus(_)),
        "a plain hash subscript must not report a shape mismatch, got {error:?}"
    );
    Ok(())
}

#[test]
fn a_plain_array_subscript_cannot_mismatch_its_own_array() -> Result<(), Box<dyn Error>> {
    let error = contract_error(mismatching_hop(
        array_variable(),
        StructuralAccessOperator::ArrayIndex,
        StructuralAccessSelector::StaticIndex(0),
        "[0]",
        ValueShape::HashRef,
    ))?;
    assert!(
        matches!(error, StructuralAccessContractError::ContradictoryStatus(_)),
        "a plain array subscript must not report a shape mismatch, got {error:?}"
    );
    Ok(())
}

#[test]
fn an_arrow_subscript_on_a_scalar_still_reports_a_real_mismatch() -> Result<(), Box<dyn Error>> {
    // The discriminating control. `$ref->{k}` names a scalar whose runtime
    // shape is genuinely unknown, so a mismatch there is honest evidence and
    // the law must not reach it. A law keyed on "named variable" alone, rather
    // than on the plain operators, would wrongly reject this.
    mismatching_hop(
        base_variable(),
        StructuralAccessOperator::HashRefSlot,
        StructuralAccessSelector::StaticKey("k".to_string()),
        "->{k}",
        ValueShape::ArrayRef,
    )?;
    Ok(())
}

#[test]
fn a_plain_subscript_mismatch_cannot_survive_the_transport_boundary() -> Result<(), Box<dyn Error>>
{
    // `nested_chain`'s second hop is a plain `{staff}` over its predecessor.
    // Rewriting its aggregate to a named hash variable and its outcome to a
    // mismatch is the serialized form of the record law 12 forbids.
    let mut value = serde_json::to_value(nested_chain()?)?;
    value["hops"][1]["aggregate"] =
        serde_json::json!({ "Variable": { "sigil": "%", "name": "config" } });
    value["hops"][1]["outcome"] =
        serde_json::json!({ "ShapeMismatch": { "observed": "ArrayRef" } });
    let decoded: StructuralAccessChain = serde_json::from_value(value)?;
    assert!(
        decoded.validate().is_err(),
        "a fabricated plain-subscript mismatch must not survive transport"
    );
    Ok(())
}
