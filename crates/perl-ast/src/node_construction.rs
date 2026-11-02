//! Checked local construction for public [`crate::Node`] builders.
//!
//! [`Node::try_new`] and [`Node::try_new_in_source`] validate span ordering,
//! registry-derived direct-child cardinality, and variant-local payload rules
//! without walking descendants for containment or source-payload agreement.
//! Whole-tree validation remains [`crate::validate_ast`]; parser production
//! continues to use unchecked [`Node::new`].

use crate::invariant_policy::{AstEmptyRangePolicy, AstNodeClassification, ast_node_policy_of};
use crate::kind_schema::{FieldCardinality, structural_row};
use crate::{FieldId, Node, NodeKind, SourceLocation};
use std::fmt;
use std::ops::ControlFlow;

/// Stable class for span failures at the checked construction boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeSpanInvariantCode {
    /// `location.start` is greater than `location.end`.
    ReversedRange,
    /// The location extends past the supplied source length.
    RangeOutOfBounds,
    /// The location endpoints are not UTF-8 char boundaries in the source.
    NonUtf8Boundary,
    /// A zero-width range is not admitted for this node kind.
    UnexpectedEmptyRange,
}

impl NodeSpanInvariantCode {
    /// Stable machine token for routing and tests.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReversedRange => "reversed_range",
            Self::RangeOutOfBounds => "range_out_of_bounds",
            Self::NonUtf8Boundary => "non_utf8_boundary",
            Self::UnexpectedEmptyRange => "unexpected_empty_range",
        }
    }
}

/// Stable class for variant-local invariant failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NodeLocalInvariantCode {
    /// A registry-required direct child field is absent on this instance.
    RequiredFieldMissing,
    /// A fixed or optional field exceeds its allowed emission count.
    FixedCardinalityMismatch,
    /// Parallel collections on the variant disagree in length.
    ParallelLengthMismatch,
    /// Mutually exclusive local payload fields are both set or contradictory.
    MutuallyExclusiveState,
    /// A required scalar or local payload field is missing.
    RequiredLocalPayloadMissing,
    /// A local discriminator or enum payload is invalid for the variant.
    InvalidLocalDiscriminator,
    /// A recovery-only allowance was requested on an ordinary node kind.
    RecoveryExceptionNotAdmitted,
}

impl NodeLocalInvariantCode {
    /// Stable machine token for routing and tests.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequiredFieldMissing => "required_field_missing",
            Self::FixedCardinalityMismatch => "fixed_cardinality_mismatch",
            Self::ParallelLengthMismatch => "parallel_length_mismatch",
            Self::MutuallyExclusiveState => "mutually_exclusive_state",
            Self::RequiredLocalPayloadMissing => "required_local_payload_missing",
            Self::InvalidLocalDiscriminator => "invalid_local_discriminator",
            Self::RecoveryExceptionNotAdmitted => "recovery_exception_not_admitted",
        }
    }
}

/// One variant-local invariant failure with structured context.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NodeLocalInvariantError {
    /// Stable failure class.
    pub code: NodeLocalInvariantCode,
    /// [`NodeKind::kind_name`] of the failing node.
    pub kind_name: &'static str,
    /// Registry field or local payload id when applicable.
    pub field: Option<FieldId>,
    /// Expected relationship or cardinality token.
    pub expected: &'static str,
    /// Observed bounded summary (counts, lengths).
    pub observed: String,
}

/// Failure from [`Node::try_new`] or [`Node::try_new_in_source`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeInvariantError {
    /// Span ordering or source-boundary failure on the node location.
    Span {
        /// Stable span failure class.
        code: NodeSpanInvariantCode,
        /// Location that failed validation.
        location: SourceLocation,
    },
    /// Variant-local structural or payload failure.
    Local(NodeLocalInvariantError),
}

impl fmt::Display for NodeInvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Span { code, location } => {
                write!(f, "{} at {}..{}", code.as_str(), location.start, location.end)
            }
            Self::Local(err) => write!(
                f,
                "{} on {} (field={}, expected={}, observed={})",
                err.code.as_str(),
                err.kind_name,
                err.field.map_or("-", FieldId::name),
                err.expected,
                err.observed
            ),
        }
    }
}

impl std::error::Error for NodeInvariantError {}

impl Node {
    /// Construct a node after validating local span and structural invariants.
    ///
    /// This does not consult source bytes. Span ordering is checked with
    /// `start <= end` only; whole-tree containment and source-payload checks
    /// remain with [`crate::validate_ast`].
    ///
    /// Unchecked construction for parser and compatibility paths remains
    /// [`Node::new`].
    pub fn try_new(kind: NodeKind, location: SourceLocation) -> Result<Self, NodeInvariantError> {
        validate_node_construction(&kind, location, None)?;
        Ok(Node::new(kind, location))
    }

    /// Like [`Self::try_new`], but also requires the location to fit the
    /// supplied source and end on UTF-8 boundaries.
    ///
    /// Success proves only that the location was valid for these bytes at
    /// construction time; the returned node stores no source revision.
    pub fn try_new_in_source(
        kind: NodeKind,
        location: SourceLocation,
        source: &str,
    ) -> Result<Self, NodeInvariantError> {
        validate_node_construction(&kind, location, Some(source))?;
        Ok(Node::new(kind, location))
    }
}

/// Validate local invariants for an assembled node without constructing it.
pub fn validate_node_construction(
    kind: &NodeKind,
    location: SourceLocation,
    source: Option<&str>,
) -> Result<(), NodeInvariantError> {
    validate_location(kind, location, source)?;
    let node = Node::new(kind.clone(), location);
    validate_registry_cardinality(&node)?;
    if let Some(err) = validate_variant_local(kind) {
        return Err(NodeInvariantError::Local(err));
    }
    Ok(())
}

fn validate_location(
    kind: &NodeKind,
    location: SourceLocation,
    source: Option<&str>,
) -> Result<(), NodeInvariantError> {
    if location.start > location.end {
        return Err(NodeInvariantError::Span {
            code: NodeSpanInvariantCode::ReversedRange,
            location,
        });
    }
    if let Some(source) = source {
        if location.start > source.len() || location.end > source.len() {
            return Err(NodeInvariantError::Span {
                code: NodeSpanInvariantCode::RangeOutOfBounds,
                location,
            });
        }
        if !source.is_char_boundary(location.start) || !source.is_char_boundary(location.end) {
            return Err(NodeInvariantError::Span {
                code: NodeSpanInvariantCode::NonUtf8Boundary,
                location,
            });
        }
    }
    if location.start == location.end && !allows_empty_range(kind) {
        return Err(NodeInvariantError::Span {
            code: NodeSpanInvariantCode::UnexpectedEmptyRange,
            location,
        });
    }
    Ok(())
}

fn allows_empty_range(kind: &NodeKind) -> bool {
    let kind_name = NodeKind::kind_name(kind);
    if structural_row(kind_name).is_some_and(|row| row.recovery) {
        return true;
    }
    let Some(policy) = ast_node_policy_of(kind) else {
        return false;
    };
    matches!(
        policy.classification,
        AstNodeClassification::Recovery | AstNodeClassification::SourceBoundary
    ) || policy.empty_range == AstEmptyRangePolicy::SyntheticAllowed
}

fn field_emission_counts(node: &Node) -> std::collections::BTreeMap<&'static str, usize> {
    let mut counts = std::collections::BTreeMap::new();
    let _ = node.try_for_each_child_with_field(|field, _child| {
        if let Some(field) = field {
            let name = FieldId::name(field);
            *counts.entry(name).or_default() += 1;
        }
        ControlFlow::<(), ()>::Continue(())
    });
    counts
}

fn validate_registry_cardinality(node: &Node) -> Result<(), NodeInvariantError> {
    let kind_name = node.kind.kind_name();
    let Some(row) = structural_row(kind_name) else {
        return Ok(());
    };
    if row.is_leaf() {
        let counts = field_emission_counts(node);
        if counts.values().any(|count| *count > 0) {
            return Err(local_error(
                NodeLocalInvariantCode::FixedCardinalityMismatch,
                kind_name,
                None,
                "leaf",
                format!("observed {} direct child emissions", counts.values().sum::<usize>()),
            ));
        }
        return Ok(());
    }

    let counts = field_emission_counts(node);
    for child in row.children {
        let count = counts.get(child.field.name()).copied().unwrap_or(0);
        match child.cardinality {
            FieldCardinality::Required if count == 0 => {
                return Err(local_error(
                    NodeLocalInvariantCode::RequiredFieldMissing,
                    kind_name,
                    Some(child.field),
                    "required",
                    "0 emissions".to_string(),
                ));
            }
            FieldCardinality::Optional if count > 1 => {
                return Err(local_error(
                    NodeLocalInvariantCode::FixedCardinalityMismatch,
                    kind_name,
                    Some(child.field),
                    "optional",
                    format!("{count} emissions"),
                ));
            }
            FieldCardinality::Required
            | FieldCardinality::Optional
            | FieldCardinality::Repeated => {}
        }
    }
    Ok(())
}

fn local_error(
    code: NodeLocalInvariantCode,
    kind_name: &'static str,
    field: Option<FieldId>,
    expected: &'static str,
    observed: String,
) -> NodeInvariantError {
    NodeInvariantError::Local(NodeLocalInvariantError {
        code,
        kind_name,
        field,
        expected,
        observed,
    })
}

fn validate_variant_local(kind: &NodeKind) -> Option<NodeLocalInvariantError> {
    match kind {
        NodeKind::ChainedComparison { operands, ops } => {
            if operands.len() < 2 {
                return Some(local_payload(
                    NodeLocalInvariantCode::FixedCardinalityMismatch,
                    "ChainedComparison",
                    Some(FieldId::ELEMENTS),
                    "at least 2 operands",
                    format!("{} operands", operands.len()),
                ));
            }
            let expected_ops = operands.len().saturating_sub(1);
            if ops.len() != expected_ops {
                return Some(local_payload(
                    NodeLocalInvariantCode::ParallelLengthMismatch,
                    "ChainedComparison",
                    Some(FieldId::ELEMENTS),
                    "ops.len() == operands.len() - 1",
                    format!("{} operands, {} ops", operands.len(), ops.len()),
                ));
            }
            None
        }
        NodeKind::NamedParameter {
            required,
            default_operator,
            default_operator_span,
            default_value,
            ..
        } => {
            if *required {
                if default_value.is_some()
                    || default_operator.is_some()
                    || default_operator_span.is_some()
                {
                    return Some(local_payload(
                        NodeLocalInvariantCode::MutuallyExclusiveState,
                        "NamedParameter",
                        None,
                        "required parameter has no default",
                        "default fields present on required parameter".to_string(),
                    ));
                }
            } else if default_value.is_some() && default_operator.is_none() {
                return Some(local_payload(
                    NodeLocalInvariantCode::RequiredLocalPayloadMissing,
                    "NamedParameter",
                    None,
                    "default_operator with default_value",
                    "default_value without operator".to_string(),
                ));
            } else if default_operator_span.is_some() && default_operator.is_none() {
                return Some(local_payload(
                    NodeLocalInvariantCode::RequiredLocalPayloadMissing,
                    "NamedParameter",
                    None,
                    "default_operator with default_operator_span",
                    "span without operator".to_string(),
                ));
            }
            None
        }
        NodeKind::TargetlessGoto {} => None,
        _ => None,
    }
}

fn local_payload(
    code: NodeLocalInvariantCode,
    kind_name: &'static str,
    field: Option<FieldId>,
    expected: &'static str,
    observed: String,
) -> NodeLocalInvariantError {
    NodeLocalInvariantError { code, kind_name, field, expected, observed }
}
