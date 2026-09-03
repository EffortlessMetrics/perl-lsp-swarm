//! External JSON round-trip proof for the structural access contract (#13619).
//!
//! The crate's own guidance requires round-trip coverage for records that cross
//! process and cache boundaries, and requires it from outside the crate: these
//! chains are meant to be persisted and read back by another process, so the
//! proof has to exercise the public API a consumer actually has rather than the
//! module's private internals.
//!
//! Every variant of every enum in the contract appears at least once below. A
//! field that cannot survive serde, or an enum arm that loses its payload,
//! fails here rather than in the consumer that persisted it.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): matches the crate's other suites.

use std::error::Error;

use perl_semantic_facts::structural_access::{
    STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION, StructuralAccessAggregate, StructuralAccessBudget,
    StructuralAccessChain, StructuralAccessHop, StructuralAccessLimitation,
    StructuralAccessOperator, StructuralAccessSelector, StructuralAccessSpelling,
    StructuralAccessSubject, StructuralAggregateCompleteness, StructuralAggregateDisposition,
    StructuralHopCertainty, StructuralHopOutcome,
};
use perl_semantic_facts::{
    AnchorId, BoundaryDisposition, BoundaryKind, BoundaryLink, Confidence, FactId, FileId,
    Provenance, SemanticConfidence, SemanticProducer, SemanticProvenance, SemanticReasonCode,
    SourceAnchor, SourceGeneration, ValueShape,
};

const DOCUMENT: FileId = FileId(11);

fn anchor(start: u32, end: u32) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(1)), DOCUMENT, start, end)
}

fn boundary(kind: BoundaryKind, disposition: BoundaryDisposition) -> BoundaryLink {
    BoundaryLink::new(Some(FactId(9)), kind, disposition, SemanticReasonCode::DynamicValue)
}

/// Build one hop, leaving every honesty field to the caller so the fixtures can
/// reach states the convenience helpers in the unit suite do not.
#[allow(clippy::too_many_arguments)]
fn hop(
    ordinal: u32,
    aggregate: StructuralAccessAggregate,
    operator: StructuralAccessOperator,
    selector: StructuralAccessSelector,
    text: &str,
    outcome: StructuralHopOutcome,
    certainty: StructuralHopCertainty,
    completeness: StructuralAggregateCompleteness,
    disposition: StructuralAggregateDisposition,
    limitations: Vec<StructuralAccessLimitation>,
) -> Result<StructuralAccessHop, Box<dyn Error>> {
    let start = ordinal * 20;
    Ok(StructuralAccessHop::new(
        ordinal,
        aggregate,
        operator,
        selector,
        StructuralAccessSpelling::new(text, anchor(start, start + text.len() as u32))?,
        outcome,
        certainty,
        completeness,
        disposition,
        SemanticProducer::SemanticAnalyzer,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticReasonCode::ExactSource,
        StructuralAccessBudget::new(500 - ordinal, 499 - ordinal)?,
        limitations,
    )?)
}

/// Assert a chain survives a JSON round trip byte-for-byte, still validates,
/// and keeps its fingerprint.
fn assert_round_trips(chain: &StructuralAccessChain) -> Result<(), Box<dyn Error>> {
    let serialized = serde_json::to_string(chain)?;
    let decoded: StructuralAccessChain = serde_json::from_str(&serialized)?;
    assert_eq!(&decoded, chain, "a round trip must preserve the whole chain");
    decoded.validate()?;
    assert_eq!(decoded.fingerprint(), chain.fingerprint());
    assert_eq!(serde_json::to_string(&decoded)?, serialized, "re-encoding must be stable");
    assert_eq!(decoded.schema_version(), STRUCTURAL_ACCESS_CHAIN_SCHEMA_VERSION);
    Ok(())
}

#[test]
fn every_operator_and_static_selector_round_trips() -> Result<(), Box<dyn Error>> {
    // `$config->{groups}{staff}->[0][-1]` reaches all four operators and both
    // static selector kinds, including a negative index.
    let chain = StructuralAccessChain::new(
        StructuralAccessSubject::new(
            DOCUMENT,
            SourceGeneration::known("source-sha"),
            Some("file:///workspace".to_string()),
            Some(SourceGeneration::known("project-gen")),
        )?,
        vec![
            hop(
                0,
                StructuralAccessAggregate::Variable {
                    sigil: "$".to_string(),
                    name: "config".to_string(),
                },
                StructuralAccessOperator::HashRefSlot,
                StructuralAccessSelector::StaticKey("groups".to_string()),
                "->{groups}",
                StructuralHopOutcome::Selected { shape: ValueShape::HashRef, value_fact: None },
                StructuralHopCertainty::Definite,
                StructuralAggregateCompleteness::Closed,
                StructuralAggregateDisposition::Stable,
                Vec::new(),
            )?,
            hop(
                1,
                StructuralAccessAggregate::PrecedingHop { ordinal: 0 },
                StructuralAccessOperator::HashSlot,
                StructuralAccessSelector::StaticKey("staff".to_string()),
                "{staff}",
                StructuralHopOutcome::Selected {
                    shape: ValueShape::ArrayRef,
                    value_fact: Some(FactId(4)),
                },
                StructuralHopCertainty::Definite,
                StructuralAggregateCompleteness::Closed,
                StructuralAggregateDisposition::Stable,
                Vec::new(),
            )?,
            hop(
                2,
                StructuralAccessAggregate::PrecedingHop { ordinal: 1 },
                StructuralAccessOperator::ArrayRefIndex,
                StructuralAccessSelector::StaticIndex(0),
                "->[0]",
                StructuralHopOutcome::Selected { shape: ValueShape::ArrayRef, value_fact: None },
                StructuralHopCertainty::Possible,
                StructuralAggregateCompleteness::Open,
                StructuralAggregateDisposition::Mutated,
                vec![StructuralAccessLimitation::MutatedAggregate],
            )?,
            hop(
                3,
                StructuralAccessAggregate::PrecedingHop { ordinal: 2 },
                StructuralAccessOperator::ArrayIndex,
                StructuralAccessSelector::StaticIndex(-1),
                "[-1]",
                StructuralHopOutcome::Selected {
                    shape: ValueShape::Object {
                        package: "Staff".to_string(),
                        confidence: Confidence::Medium,
                    },
                    value_fact: Some(FactId(5)),
                },
                StructuralHopCertainty::Possible,
                StructuralAggregateCompleteness::Open,
                StructuralAggregateDisposition::EscapedAndMutated,
                vec![
                    StructuralAccessLimitation::EscapedAggregate,
                    StructuralAccessLimitation::MutatedAggregate,
                    StructuralAccessLimitation::OpenAggregate,
                ],
            )?,
        ],
    )?;

    assert_round_trips(&chain)?;
    assert!(chain.selected().is_some(), "the chain selects a value");
    assert_eq!(chain.hops().len(), 4);
    assert_eq!(chain.subject().document, DOCUMENT);
    Ok(())
}

#[test]
fn every_dynamic_selector_and_aggregate_kind_round_trips() -> Result<(), Box<dyn Error>> {
    for (aggregate, operator, selector, text) in [
        (
            StructuralAccessAggregate::Fact(FactId(42)),
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::DynamicKey(boundary(
                BoundaryKind::DynamicValue,
                BoundaryDisposition::Degrade,
            )),
            "->{$key}",
        ),
        (
            StructuralAccessAggregate::DynamicBoundary(boundary(
                BoundaryKind::SymbolicReference,
                BoundaryDisposition::Refuse,
            )),
            StructuralAccessOperator::ArrayIndex,
            StructuralAccessSelector::DynamicIndex(boundary(
                BoundaryKind::DynamicValue,
                BoundaryDisposition::Refuse,
            )),
            "[$i]",
        ),
    ] {
        let chain = StructuralAccessChain::new(
            StructuralAccessSubject::new(DOCUMENT, SourceGeneration::Unknown, None, None)?,
            vec![hop(
                0,
                aggregate,
                operator,
                selector,
                text,
                StructuralHopOutcome::Boundary(boundary(
                    BoundaryKind::DynamicValue,
                    BoundaryDisposition::Degrade,
                )),
                StructuralHopCertainty::Possible,
                StructuralAggregateCompleteness::Open,
                StructuralAggregateDisposition::Escaped,
                vec![
                    StructuralAccessLimitation::DynamicSelector,
                    StructuralAccessLimitation::EscapedAggregate,
                ],
            )?],
        )?;
        assert_round_trips(&chain)?;
        assert!(chain.selected().is_none(), "a boundary chain selects nothing");
    }
    Ok(())
}

#[test]
fn every_non_selecting_outcome_round_trips() -> Result<(), Box<dyn Error>> {
    for (outcome, completeness, budget_exhausted, limitation) in [
        (
            StructuralHopOutcome::AbsentMember,
            StructuralAggregateCompleteness::Closed,
            false,
            StructuralAccessLimitation::RecoveredSyntax,
        ),
        (
            StructuralHopOutcome::UnknownMember,
            StructuralAggregateCompleteness::Open,
            false,
            StructuralAccessLimitation::OpenAggregate,
        ),
        (
            StructuralHopOutcome::ShapeMismatch { observed: ValueShape::CodeRef },
            StructuralAggregateCompleteness::Closed,
            false,
            StructuralAccessLimitation::Unsupported,
        ),
        (
            StructuralHopOutcome::StaleGeneration,
            StructuralAggregateCompleteness::Open,
            false,
            StructuralAccessLimitation::StaleDependency,
        ),
        (
            StructuralHopOutcome::BudgetExhausted,
            StructuralAggregateCompleteness::Open,
            true,
            StructuralAccessLimitation::BudgetExhausted,
        ),
    ] {
        let budget = if budget_exhausted {
            StructuralAccessBudget::new(1, 0)?
        } else {
            StructuralAccessBudget::new(500, 499)?
        };
        // `$table->{missing}` rather than `$table{missing}`: one of these
        // outcomes is a shape mismatch, and a plain subscript cannot mismatch
        // the container its own sigil names. An arrow subscript on a scalar
        // has a genuinely open runtime shape and is honest for every outcome
        // in this table, so one hop shape still covers them all.
        let single = StructuralAccessHop::new(
            0,
            StructuralAccessAggregate::Variable {
                sigil: "$".to_string(),
                name: "table".to_string(),
            },
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("missing".to_string()),
            StructuralAccessSpelling::new("->{missing}", anchor(0, 11))?,
            outcome,
            StructuralHopCertainty::Possible,
            completeness,
            StructuralAggregateDisposition::Stable,
            SemanticProducer::PirA,
            SemanticProvenance::Unknown,
            SemanticConfidence::Known(Confidence::Low),
            SemanticReasonCode::DynamicValue,
            budget,
            vec![limitation],
        )?;
        let chain = StructuralAccessChain::new(
            StructuralAccessSubject::new(
                DOCUMENT,
                SourceGeneration::known(""),
                Some("root".to_string()),
                Some(SourceGeneration::Unknown),
            )?,
            vec![single],
        )?;
        assert_round_trips(&chain)?;
    }
    Ok(())
}

#[test]
fn every_limitation_round_trips_and_stays_canonical() -> Result<(), Box<dyn Error>> {
    // Supplied deliberately out of order and with a duplicate: the constructor
    // canonicalises, and the canonical order must survive the wire.
    //
    // The disposition is `EscapedAndMutated` and the completeness `Open`
    // because this hop carries the `EscapedAggregate`, `MutatedAggregate` and
    // `OpenAggregate` limitations, each of which restates one of those fields.
    // An earlier version of this fixture paired them with a `Stable`
    // aggregate, which made the all-variants coverage rest on a record the
    // contract should never have accepted.
    let chain = StructuralAccessChain::new(
        StructuralAccessSubject::new(DOCUMENT, SourceGeneration::known("g"), None, None)?,
        vec![hop(
            0,
            StructuralAccessAggregate::Variable {
                sigil: "@".to_string(),
                name: "rows".to_string(),
            },
            StructuralAccessOperator::ArrayIndex,
            // A dynamic index, because this hop carries the `DynamicSelector`
            // limitation, which restates exactly what this field says. A
            // static index here would make the all-variants coverage rest on
            // a record the contract rejects.
            StructuralAccessSelector::DynamicIndex(boundary(
                BoundaryKind::DynamicValue,
                BoundaryDisposition::Degrade,
            )),
            "[$i]",
            StructuralHopOutcome::UnknownMember,
            StructuralHopCertainty::Possible,
            StructuralAggregateCompleteness::Open,
            StructuralAggregateDisposition::EscapedAndMutated,
            vec![
                StructuralAccessLimitation::Unsupported,
                StructuralAccessLimitation::CompatibilityBridge,
                StructuralAccessLimitation::StaleDependency,
                StructuralAccessLimitation::BudgetExhausted,
                StructuralAccessLimitation::RecoveredSyntax,
                StructuralAccessLimitation::MutatedAggregate,
                StructuralAccessLimitation::EscapedAggregate,
                StructuralAccessLimitation::OpenAggregate,
                StructuralAccessLimitation::DynamicSelector,
                StructuralAccessLimitation::DynamicSelector,
            ],
        )?],
    )?;

    let limitations = chain.hops()[0].limitations();
    assert_eq!(limitations.len(), 9, "the duplicate is removed, all nine kinds remain");
    assert!(limitations.windows(2).all(|pair| pair[0] < pair[1]), "canonical order is ascending");
    assert_round_trips(&chain)?;
    Ok(())
}

#[test]
fn a_json_shape_the_constructor_would_reject_is_caught_by_validate() -> Result<(), Box<dyn Error>> {
    // The contract's own claim to consumers: a chain from a transport is safe
    // only once `validate()` has accepted it. Proven from outside the crate,
    // where a real consumer stands.
    let chain = StructuralAccessChain::new(
        StructuralAccessSubject::new(
            DOCUMENT,
            SourceGeneration::known("g"),
            Some("root".to_string()),
            None,
        )?,
        vec![hop(
            0,
            StructuralAccessAggregate::Variable {
                sigil: "$".to_string(),
                name: "config".to_string(),
            },
            StructuralAccessOperator::HashRefSlot,
            StructuralAccessSelector::StaticKey("groups".to_string()),
            "->{groups}",
            StructuralHopOutcome::Selected { shape: ValueShape::HashRef, value_fact: None },
            StructuralHopCertainty::Definite,
            StructuralAggregateCompleteness::Closed,
            StructuralAggregateDisposition::Stable,
            Vec::new(),
        )?],
    )?;

    for (label, mutate) in [
        (
            "blank workspace root",
            Box::new(|value: &mut serde_json::Value| {
                value["subject"]["workspace_root"] = serde_json::json!("  ");
            }) as Box<dyn Fn(&mut serde_json::Value)>,
        ),
        (
            "budget that grew across the hop",
            Box::new(|value: &mut serde_json::Value| {
                value["hops"][0]["budget"]["units_after"] = serde_json::json!(u32::MAX);
            }),
        ),
        (
            "duplicated limitations",
            Box::new(|value: &mut serde_json::Value| {
                value["hops"][0]["limitations"] =
                    serde_json::json!(["OpenAggregate", "OpenAggregate"]);
            }),
        ),
        (
            "a foreign document anchor",
            Box::new(|value: &mut serde_json::Value| {
                value["hops"][0]["spelling"]["anchor"]["file_id"] = serde_json::json!(999);
            }),
        ),
        (
            "an unrecognised schema version",
            Box::new(|value: &mut serde_json::Value| {
                value["schema_version"] = serde_json::json!(99);
            }),
        ),
    ] {
        let mut value = serde_json::to_value(&chain)?;
        mutate(&mut value);
        let decoded: StructuralAccessChain = serde_json::from_value(value)?;
        assert!(decoded.validate().is_err(), "{label} must not survive the transport boundary");
    }
    Ok(())
}
