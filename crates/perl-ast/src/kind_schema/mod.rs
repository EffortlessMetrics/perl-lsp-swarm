//! Shadow `NodeKind` structural registry and check-mode parity checker.
//!
//! # Role
//!
//! This module is **S1 of #8155 / issue #8415**: a declarative structural
//! authority that is compared with current production surfaces and is not
//! itself production authority.
//!
//! Production still owns:
//!
//! - [`crate::Node::try_for_each_child_with_field`] / [`crate::Node::for_each_child_mut`]
//! - [`crate::FieldId`]
//! - S-expression / debug rendering
//! - generated status inventories
//!
//! Any mismatch is a failed check, not a silent fallback onto this table.
//!
//! # What a row records
//!
//! - stable variant / public name
//! - declaration order (slice order)
//! - child fields in canonical first-emission order
//! - required / optional / repeated cardinality
//! - leaf / child-bearing / recovery / source-boundary tags
//! - static grammar name or runtime-derived grammar-name inputs
//! - public schema compatibility disposition

mod forms;
mod observe;
mod parity;
mod registry;
mod types;

pub use forms::{cardinality_forms, grammar_input_witnesses};
pub use observe::{TraversalObservation, observe_kind_traversal};
pub use parity::{
    GrammarInputWitness, KindSchemaEvidence, KindSchemaMismatch, KindSchemaReport,
    check_kind_schema, serialize_kind_schema,
};
pub use registry::NODE_KIND_STRUCTURAL_REGISTRY;
pub use types::{
    ChildFieldSpec, FieldCardinality, GrammarNameSpec, KindBody, KindStructuralRow,
    SchemaCompatibility,
};

use crate::{FieldId, Node, NodeKind, node_kind_fixtures};

/// Check-mode identifier. This module does not cut over production consumers.
pub const KIND_SCHEMA_MODE: &str = "shadow-check";

/// Schema vocabulary version for deterministic serialization.
pub const KIND_SCHEMA_VERSION: u32 = 1;

/// Build check-mode evidence against the production registry and #7754 fixtures.
#[must_use]
pub fn current_kind_schema_evidence<'a>(
    registry: &'a [KindStructuralRow<'a>],
    representatives: &'a [Node],
    cardinality: &'a [Node],
    grammar_witnesses: &'a [GrammarInputWitness],
) -> KindSchemaEvidence<'a> {
    KindSchemaEvidence {
        registry,
        kind_names: NodeKind::ALL_KIND_NAMES,
        recovery_names: NodeKind::RECOVERY_KIND_NAMES,
        field_ids: FieldId::ALL,
        representatives,
        cardinality_forms: cardinality,
        grammar_witnesses,
    }
}

/// Representative nodes from the #7754 compile-exhaustive fixture bank.
#[must_use]
pub fn representative_nodes() -> Vec<Node> {
    node_kind_fixtures().into_iter().map(|fixture| fixture.sample).collect()
}

/// Check the production shadow registry against current AST facts.
#[must_use]
pub fn check_current_kind_schema() -> KindSchemaReport {
    let representatives = representative_nodes();
    let cardinality = cardinality_forms();
    let grammar_witnesses = grammar_input_witnesses();
    let evidence = current_kind_schema_evidence(
        NODE_KIND_STRUCTURAL_REGISTRY,
        &representatives,
        &cardinality,
        &grammar_witnesses,
    );
    check_kind_schema(&evidence)
}
