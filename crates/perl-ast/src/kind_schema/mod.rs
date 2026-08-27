//! Structural `NodeKind` registry: production authority for [`crate::FieldId`]
//! membership and field-aware child traversal.
//!
//! # Role
//!
//! This module is **#8424 / #8155 S2 cutover** plus the **#8429** derived
//! identity/inventory surface. The checked registry owns:
//!
//! - [`crate::FieldId`] set membership (public [`crate::FieldId::ALL`] order is
//!   the compatibility inventory)
//! - immutable [`crate::Node::try_for_each_child_with_field`]
//! - mutable [`crate::Node::try_for_each_child_mut_with_field`]
//! - deterministic structural schema identity and freshness-gated NodeKind inventory
//!
//! [`crate::Node::for_each_child_mut`] is a compatibility wrapper over the
//! mutable field-aware walker. Native debug S-expression rendering consumes
//! this visit table for child order; payload spelling and the one-root grammar
//! live in `ast::node_sexp`. Schema identity and freshness-gated NodeKind
//! inventory are derived from this registry (`identity`). They do not change
//! traversal, rendering, or parser behavior.
//!
//! `source_boundary` tags are recorded and serialized. They are **not**
//! production authority: they were not reconciled against a production
//! inventory in #8415, and this cutover does not promote them.
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
mod identity;
mod observe;
mod parity;
mod registry;
mod types;
mod visit;

pub use forms::{cardinality_forms, grammar_input_witnesses};
pub use identity::{
    AST_STRUCTURAL_SCHEMA_DIGEST_ALGORITHM, AST_STRUCTURAL_SCHEMA_IDENTITY_VERSION,
    AstStructuralSchemaIdentity, NodeKindInventory, SchemaChange, SchemaDiff, SchemaIdentityError,
    StatusFreshnessError, VariantInventoryRow, canonical_structural_subject,
    check_status_freshness, current_ast_structural_schema_identity, current_nodekind_inventory,
    diff_structural_registries, fingerprint_registry, inventory_from_registry,
    parse_schema_identity, render_checked_status_report,
};
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
pub use visit::{registered_child_fields, registry_field_id_set, structural_row};

use crate::{FieldId, Node, NodeKind, node_kind_fixtures};

/// Production identifier for FieldId membership and field-aware traversal.
///
/// Rendering and `source_boundary` classification are not covered by this mode.
/// Schema identity and NodeKind inventory are derived projections of the same
/// registry; they do not change this traversal mode token.
pub const KIND_SCHEMA_MODE: &str = "production-traversal";

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

/// Check the production registry against current AST facts.
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
