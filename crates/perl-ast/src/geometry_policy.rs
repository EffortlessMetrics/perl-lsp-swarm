//! Field-level source-geometry authority for AST payloads.
//!
//! [`crate::invariant_policy`] answers questions about a whole `NodeKind`:
//! its structural role, its range policy, and which *classes* of source-derived
//! payload it carries. That granularity is not sufficient for a consumer that
//! must move an entire tree into new coordinates, because the unit of work
//! there is one field, not one variant.
//!
//! This module is that finer denominator. It registers every **independent
//! source-geometry payload field**: a payload field that carries byte offsets
//! of its own, separate from [`crate::Node::location`]. Structural children are
//! not registered here — they keep coming from
//! [`crate::Node::try_for_each_child_with_field`] and
//! [`crate::kind_schema::NODE_KIND_STRUCTURAL_REGISTRY`]. This is deliberately
//! not a second child traversal.
//!
//! # Why a field-level registry
//!
//! A variant-level payload policy cannot say whether
//! `Subroutine { name_span, .. }` must be remapped when the tree moves, nor
//! that `Error { found, .. }` holds a token whose byte width was established
//! against its text at construction and therefore may not be rescaled like an
//! ordinary range. Without that, a field can silently remain in an old source
//! generation while an API still reports success.
//!
//! # Token width is a carried invariant, not an immutable fact
//!
//! `Token::new_checked` enforces `text.len() == end - start` when the token is
//! built, but `Token::text` is a public field: later code holding `&mut Token`
//! can replace the text without the span following it. So a mapping consumer
//! must **preserve the recorded width** when moving a token's start, and must
//! never recompute the width from `text` — recomputing would silently adopt a
//! drifted length as truth. [`AstGeometryMapping::MapStartPreserveWidth`] is
//! named for that obligation.
//!
//! # Drift resistance
//!
//! [`observe_geometry_fields`] is the observation authority, and it is
//! **compile-exhaustive over fields, not just over variants**. Every arm
//! destructures every field of its variant by name; no arm uses `..`. Adding a
//! field to an existing variant therefore fails to compile here until an author
//! classifies it, which is the mutation that a variant-only guard misses.
//!
//! Registration alone is not the guard. [`reconcile_geometry_rows`] compares
//! what a node actually carries against what the registry claims, so a stale
//! row, an unregistered field, a wrong shape, or a rescalable token is a typed
//! failure rather than a silent default.

use crate::{Node, NodeKind};

/// Shape of one independent source-geometry payload field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstGeometryShape {
    /// Exactly one always-present span, e.g. `Package { name_span }`.
    Direct,
    /// Zero or one span, e.g. `Subroutine { name_span: Option<_> }`.
    Optional,
    /// A flat collection of spans held directly by the field.
    ///
    /// Reserved: no current variant carries this shape. It exists so the first
    /// variant that does is a deliberate registration rather than a shape the
    /// vocabulary cannot express. [`geometry_shapes_in_use`] is the honest
    /// current denominator.
    Repeated,
    /// Spans reached through a nested record inside a collection, e.g.
    /// `Try { catch_blocks: Vec<(Option<(String, SourceLocation)>, _)> }`.
    Nested,
    /// A recovery token whose span width is established by its text at construction.
    ///
    /// A token is not a freely resizable range: its byte width was validated
    /// against its text when constructed, so only its start may move and the
    /// recorded width must be carried across rather than recomputed.
    Token,
}

impl AstGeometryShape {
    /// Stable token used by deterministic serialization and failure messages.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Optional => "optional",
            Self::Repeated => "repeated",
            Self::Nested => "nested",
            Self::Token => "token",
        }
    }
}

/// How a coordinate-mapping consumer must transform one geometry field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstGeometryMapping {
    /// Map both endpoints as an ordinary range.
    MapRange,
    /// Map the start and preserve the validated byte width.
    ///
    /// Required for [`AstGeometryShape::Token`], whose width is fixed by its
    /// own text at construction time.
    MapStartPreserveWidth,
    /// The boundary is owned by the caller's policy, not by the AST.
    ///
    /// Reserved for anchoring decisions (such as program/root extent) that are
    /// an incremental-strategy policy rather than a payload-field rule.
    CallerOwnedBoundary,
}

impl AstGeometryMapping {
    /// Stable token used by deterministic serialization and failure messages.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::MapRange => "map_range",
            Self::MapStartPreserveWidth => "map_start_preserve_width",
            Self::CallerOwnedBoundary => "caller_owned_boundary",
        }
    }
}

/// How one geometry field relates to source truth.
///
/// This is derived from the variant's [`crate::AstNodeClassification`] rather
/// than chosen per field, so a field cannot claim a friendlier disposition than
/// the node that owns it. [`geometry_disposition_for_classification`] is the
/// rule, and the reconciliation gate enforces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstGeometryDisposition {
    /// The span is an exact source anchor.
    SourceExact,
    /// The span anchors a specialized or opaque source region.
    SourceBoundary,
    /// The span belongs to synthetic or recovery material.
    Recovery,
}

impl AstGeometryDisposition {
    /// Stable token used by deterministic serialization and failure messages.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::SourceExact => "source_exact",
            Self::SourceBoundary => "source_boundary",
            Self::Recovery => "recovery",
        }
    }
}

/// One registered independent source-geometry payload field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AstGeometryField {
    /// Stable [`NodeKind::kind_name`] token that owns the field.
    pub kind_name: &'static str,
    /// Stable field identity, dotted for a field reached through a nested
    /// record (for example `catch_blocks.variable`).
    pub field: &'static str,
    /// Observed shape of the geometry.
    pub shape: AstGeometryShape,
    /// Transformation rule a mapping consumer must apply.
    pub mapping: AstGeometryMapping,
    /// Which of the owning variant's declared payload policies this field realizes.
    ///
    /// `None` when the owning variant declares no payload policy, in which case
    /// the disposition falls back to the variant's classification.
    ///
    /// This exists because a variant's classification is not always the right
    /// authority for one of its fields. `Format` is classified `SourceBoundary`
    /// and declares `[DeclarationNameAnchor, OpaqueSourceRegion]`: its `body` is
    /// the opaque region that earns the classification, but its `name_span` is a
    /// declaration-name anchor exactly like `Package.name_span`. Deriving from
    /// the classification alone would record the name as boundary geometry and
    /// discard a distinction the policy registry already makes.
    pub payload_role: Option<crate::AstPayloadPolicy>,
    /// Relationship to source truth.
    pub disposition: AstGeometryDisposition,
}

/// Version of the geometry-field contract.
pub const AST_GEOMETRY_SCHEMA_VERSION: u32 = 1;

/// Every independent source-geometry payload field in the primary AST.
///
/// Rows are in `NodeKind::ALL_KIND_NAMES` declaration order so a reviewer can
/// read this table against the enum. Structural children are **not** rows here.
pub const AST_NODE_GEOMETRY_FIELDS: &[AstGeometryField] = &[
    AstGeometryField {
        kind_name: "Heredoc",
        field: "body_span",
        shape: AstGeometryShape::Optional,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::OpaqueSourceRegion),
        disposition: AstGeometryDisposition::SourceBoundary,
    },
    AstGeometryField {
        kind_name: "Try",
        field: "catch_blocks.variable",
        shape: AstGeometryShape::Nested,
        mapping: AstGeometryMapping::MapRange,
        payload_role: None,
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "Subroutine",
        field: "name_span",
        shape: AstGeometryShape::Optional,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "Method",
        field: "name_span",
        shape: AstGeometryShape::Optional,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "Package",
        field: "name_span",
        shape: AstGeometryShape::Direct,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "PhaseBlock",
        field: "phase_span",
        shape: AstGeometryShape::Optional,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "Class",
        field: "name_span",
        shape: AstGeometryShape::Optional,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "Format",
        field: "name_span",
        shape: AstGeometryShape::Optional,
        mapping: AstGeometryMapping::MapRange,
        payload_role: Some(crate::AstPayloadPolicy::DeclarationNameAnchor),
        disposition: AstGeometryDisposition::SourceExact,
    },
    AstGeometryField {
        kind_name: "Error",
        field: "found",
        shape: AstGeometryShape::Token,
        mapping: AstGeometryMapping::MapStartPreserveWidth,
        payload_role: Some(crate::AstPayloadPolicy::RecoverySynthetic),
        disposition: AstGeometryDisposition::Recovery,
    },
];

/// One geometry field observed on an actual node instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedGeometryField {
    /// Stable field identity, matching [`AstGeometryField::field`].
    pub field: &'static str,
    /// Shape the field carries by construction.
    pub shape: AstGeometryShape,
    /// How many spans this instance actually holds for the field.
    ///
    /// Zero is legitimate for an absent [`AstGeometryShape::Optional`] value;
    /// the fully populated fixture bank is what proves a field can be observed
    /// at all.
    pub occurrences: usize,
}

/// A typed disagreement between the registry and an observed node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstGeometryDrift {
    /// A node carries geometry that no registry row claims.
    UnregisteredField {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Unregistered field identity.
        field: String,
    },
    /// A registry row names geometry the node does not carry.
    StaleRow {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Stale field identity.
        field: String,
    },
    /// A registry row disagrees with the observed shape.
    ShapeMismatch {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
        /// Shape claimed by the registry.
        registered: AstGeometryShape,
        /// Shape the node actually carries.
        observed: AstGeometryShape,
    },
    /// Two rows claim the same field of the same kind.
    DuplicateRow {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Duplicated field identity.
        field: String,
    },
    /// A token row does not preserve its validated byte width.
    ///
    /// `Token::new_checked` establishes `text.len() == end - start` at
    /// construction, so the recorded width is the authority a remap must carry
    /// across. Treating the token as a freely resizable range would let a remap
    /// invent bytes.
    TokenIsNotResizable {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
        /// Mapping rule that was registered instead.
        mapping: AstGeometryMapping,
    },
    /// A non-token row claims the width-preserving token rule.
    WidthPreservationRequiresToken {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
        /// Shape that was registered.
        shape: AstGeometryShape,
    },
    /// A registered payload field claims the caller-owned boundary rule.
    ///
    /// [`AstGeometryMapping::CallerOwnedBoundary`] is reserved for anchoring
    /// decisions that are not payload fields at all. A payload row claiming it
    /// would let a mapping consumer legitimately skip a real span.
    CallerOwnedMappingOnPayloadRow {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
    },
    /// A registered geometry row has no owning invariant-policy row.
    MissingPolicyRow {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
    },
    /// A geometry row names a payload role its owning variant does not declare.
    ///
    /// The role must come from the variant's own `payload_policies`, so a row
    /// cannot invent a friendlier role to obtain a friendlier disposition.
    PayloadRoleNotDeclaredByVariant {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
        /// Role the row claimed.
        role: crate::AstPayloadPolicy,
    },
    /// A geometry row omits its payload role although the owning variant declares one.
    ///
    /// `payload_role` is `None` only for a variant that declares no payload
    /// policy at all. Allowing `None` elsewhere would let a row silently drop
    /// its field semantics and fall back to classification-derived disposition,
    /// which for a declaration name on a `ChildBearing` node happens to produce
    /// the same answer — so the omission would validate clean while the registry
    /// stopped recording *why* the disposition is what it is.
    MissingPayloadRole {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
        /// Roles the owning variant declares, one of which the row must name.
        declared: Vec<crate::AstPayloadPolicy>,
    },
    /// A row's disposition disagrees with its owning variant's classification.
    DispositionMismatch {
        /// Owning `NodeKind`.
        kind_name: String,
        /// Field identity.
        field: String,
        /// Disposition the row registers.
        registered: AstGeometryDisposition,
        /// Disposition the owning classification requires.
        required: AstGeometryDisposition,
    },
}

impl std::fmt::Display for AstGeometryDrift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnregisteredField { kind_name, field } => write!(
                f,
                "{kind_name}.{field} carries source geometry with no registry row; register it in \
                 AST_NODE_GEOMETRY_FIELDS so coordinate-mapping consumers cannot leave it in an \
                 old source generation"
            ),
            Self::StaleRow { kind_name, field } => write!(
                f,
                "AST_NODE_GEOMETRY_FIELDS registers {kind_name}.{field}, but the variant no longer \
                 carries that geometry"
            ),
            Self::ShapeMismatch { kind_name, field, registered, observed } => write!(
                f,
                "{kind_name}.{field} is registered as {} but is observed as {}",
                registered.token(),
                observed.token()
            ),
            Self::DuplicateRow { kind_name, field } => {
                write!(f, "AST_NODE_GEOMETRY_FIELDS registers {kind_name}.{field} more than once")
            }
            Self::TokenIsNotResizable { kind_name, field, mapping } => write!(
                f,
                "{kind_name}.{field} is a token but registers mapping {}; a token's byte width is \
                 established at construction and must use {}",
                mapping.token(),
                AstGeometryMapping::MapStartPreserveWidth.token()
            ),
            Self::WidthPreservationRequiresToken { kind_name, field, shape } => write!(
                f,
                "{kind_name}.{field} is registered as {} but claims the token width-preserving \
                 mapping rule",
                shape.token()
            ),
            Self::CallerOwnedMappingOnPayloadRow { kind_name, field } => write!(
                f,
                "{kind_name}.{field} is a payload geometry field but registers mapping {}; that \
                 rule is reserved for anchoring decisions the AST does not own, and a payload row \
                 claiming it would let a remap skip a real span",
                AstGeometryMapping::CallerOwnedBoundary.token()
            ),
            Self::MissingPolicyRow { kind_name, field } => write!(
                f,
                "{kind_name}.{field} registers geometry but {kind_name} has no invariant-policy \
                 row, so its disposition cannot be derived"
            ),
            Self::PayloadRoleNotDeclaredByVariant { kind_name, field, role } => write!(
                f,
                "{kind_name}.{field} claims payload role {role:?}, which {kind_name} does not \
                 declare in its invariant policy"
            ),
            Self::MissingPayloadRole { kind_name, field, declared } => write!(
                f,
                "{kind_name}.{field} declares no payload role, but {kind_name} declares \
                 {declared:?}; a geometry row on such a variant must name the role it realizes"
            ),
            Self::DispositionMismatch { kind_name, field, registered, required } => write!(
                f,
                "{kind_name}.{field} registers disposition {} but its variant classification \
                 requires {}",
                registered.token(),
                required.token()
            ),
        }
    }
}

impl std::error::Error for AstGeometryDrift {}

/// Registered geometry rows for one stable `NodeKind` name.
///
/// An unknown name fails closed with an empty slice view rather than inheriting
/// a permissive default; [`reconcile_geometry_rows`] then reports any geometry
/// the node actually carries as [`AstGeometryDrift::UnregisteredField`].
///
/// # Empty is not an answer about existence
///
/// An empty result means "this name has no geometry rows". It does **not**
/// distinguish a real geometry-free variant such as `Number` from a misspelled
/// or retired name, because this function is not the kind inventory and must not
/// become a second one. A caller that needs to know whether a name is live
/// should check [`NodeKind::ALL_KIND_NAMES`] — a typo is otherwise
/// indistinguishable from a legitimately geometry-free node, and silently reads
/// as "nothing to remap".
///
/// This is safe for the reconciliation path, which always derives the name from
/// a real node rather than from caller-supplied text.
#[must_use]
pub fn geometry_fields_for(kind_name: &str) -> Vec<&'static AstGeometryField> {
    AST_NODE_GEOMETRY_FIELDS.iter().filter(|row| row.kind_name == kind_name).collect()
}

/// The geometry shapes that currently have at least one registered row.
///
/// This is the honest current denominator: [`AstGeometryShape::Repeated`] is
/// vocabulary, not coverage.
#[must_use]
pub fn geometry_shapes_in_use() -> Vec<AstGeometryShape> {
    let mut shapes: Vec<AstGeometryShape> = Vec::new();
    for row in AST_NODE_GEOMETRY_FIELDS {
        if !shapes.contains(&row.shape) {
            shapes.push(row.shape);
        }
    }
    shapes
}

/// The disposition a geometry field must carry, derived from its owning
/// variant's structural classification.
#[must_use]
pub const fn geometry_disposition_for_classification(
    classification: crate::AstNodeClassification,
) -> AstGeometryDisposition {
    match classification {
        crate::AstNodeClassification::Recovery => AstGeometryDisposition::Recovery,
        crate::AstNodeClassification::SourceBoundary => AstGeometryDisposition::SourceBoundary,
        crate::AstNodeClassification::Leaf
        | crate::AstNodeClassification::ChildBearing
        | crate::AstNodeClassification::Wrapper => AstGeometryDisposition::SourceExact,
    }
}

/// Required disposition for one geometry field, given its payload role.
///
/// The variant's classification remains the floor: a `Recovery` node's geometry
/// is recovery geometry whatever role the field plays. Above that floor the
/// field's own declared role decides, because a variant classification is a
/// statement about the *node*, and a node may carry fields with different
/// relationships to source.
///
/// `DeclarationNameAnchor` is the case that matters today: a declaration name is
/// exact source text even when it sits on a node whose body is opaque. Without
/// this, `Format.name_span` would be recorded as boundary geometry while the
/// identical `Package.name_span` is exact — a difference with no basis in what
/// either span actually anchors.
#[must_use]
pub const fn geometry_disposition_for_role(
    role: Option<crate::AstPayloadPolicy>,
    classification: crate::AstNodeClassification,
) -> AstGeometryDisposition {
    // Recovery is a property of the whole node and cannot be escaped per field.
    if matches!(classification, crate::AstNodeClassification::Recovery) {
        return AstGeometryDisposition::Recovery;
    }

    match role {
        Some(crate::AstPayloadPolicy::DeclarationNameAnchor) => AstGeometryDisposition::SourceExact,
        Some(crate::AstPayloadPolicy::RecoverySynthetic) => AstGeometryDisposition::Recovery,
        Some(crate::AstPayloadPolicy::OpaqueSourceRegion)
        | Some(crate::AstPayloadPolicy::HeredocLabelAndIndent) => {
            AstGeometryDisposition::SourceBoundary
        }
        _ => geometry_disposition_for_classification(classification),
    }
}

/// Reconcile registry rows against the geometry a node actually carries.
///
/// `registry` is a parameter rather than a constant so a negative control can
/// prove this checker discriminates: feeding it a mutated registry must produce
/// the corresponding typed drift. [`reconcile_node_geometry`] is the ordinary
/// entry point over [`AST_NODE_GEOMETRY_FIELDS`].
///
/// # Errors
///
/// Returns the first [`AstGeometryDrift`] found, in a deterministic order:
/// duplicate rows, then internally incoherent rows, then unregistered observed
/// fields, then stale rows, then shape mismatches.
pub fn reconcile_geometry_rows(
    kind_name: &str,
    registry: &[AstGeometryField],
    observed: &[ObservedGeometryField],
) -> Result<(), AstGeometryDrift> {
    let rows: Vec<&AstGeometryField> =
        registry.iter().filter(|row| row.kind_name == kind_name).collect();

    for (index, row) in rows.iter().enumerate() {
        if rows.iter().take(index).any(|earlier| earlier.field == row.field) {
            return Err(AstGeometryDrift::DuplicateRow {
                kind_name: kind_name.to_string(),
                field: row.field.to_string(),
            });
        }
    }

    for row in &rows {
        match (row.shape, row.mapping) {
            (AstGeometryShape::Token, AstGeometryMapping::MapStartPreserveWidth) => {}
            (AstGeometryShape::Token, mapping) => {
                return Err(AstGeometryDrift::TokenIsNotResizable {
                    kind_name: kind_name.to_string(),
                    field: row.field.to_string(),
                    mapping,
                });
            }
            (shape, AstGeometryMapping::MapStartPreserveWidth) => {
                return Err(AstGeometryDrift::WidthPreservationRequiresToken {
                    kind_name: kind_name.to_string(),
                    field: row.field.to_string(),
                    shape,
                });
            }
            (_, AstGeometryMapping::CallerOwnedBoundary) => {
                return Err(AstGeometryDrift::CallerOwnedMappingOnPayloadRow {
                    kind_name: kind_name.to_string(),
                    field: row.field.to_string(),
                });
            }
            (_, AstGeometryMapping::MapRange) => {}
        }
    }

    for entry in observed {
        if !rows.iter().any(|row| row.field == entry.field) {
            return Err(AstGeometryDrift::UnregisteredField {
                kind_name: kind_name.to_string(),
                field: entry.field.to_string(),
            });
        }
    }

    for row in &rows {
        if !observed.iter().any(|entry| entry.field == row.field) {
            return Err(AstGeometryDrift::StaleRow {
                kind_name: kind_name.to_string(),
                field: row.field.to_string(),
            });
        }
    }

    for row in &rows {
        for entry in observed.iter().filter(|entry| entry.field == row.field) {
            if entry.shape != row.shape {
                return Err(AstGeometryDrift::ShapeMismatch {
                    kind_name: kind_name.to_string(),
                    field: row.field.to_string(),
                    registered: row.shape,
                    observed: entry.shape,
                });
            }
        }
    }

    Ok(())
}

/// Validate the canonical registry as a whole.
///
/// [`reconcile_geometry_rows`] answers "does this node agree with its rows".
/// It deliberately cannot answer "is the row itself coherent with the variant
/// that owns it", because it takes a bare `kind_name` and no policy authority.
/// This entry point closes that: it is the production check that every row has
/// an owning [`crate::AstNodePolicy`] and carries the disposition that
/// classification requires, and that every variant's fully populated sample
/// reconciles.
///
/// Consumers should call this once before trusting the registry rather than
/// relying on a test to have run.
///
/// # Cost
///
/// This is **not** a cheap preflight. It materializes the entire fixture bank
/// via [`crate::node_kind_fixtures`] — one fully populated [`Node`] for every
/// `NodeKind`, each with boxed children and owned `String` payloads — and
/// reconciles all of them. Call it once at startup or from a test, never per
/// node, per remap, or inside any loop.
///
/// # Errors
///
/// Returns the first [`AstGeometryDrift`] found.
pub fn validate_geometry_registry() -> Result<(), AstGeometryDrift> {
    for row in AST_NODE_GEOMETRY_FIELDS {
        let Some(policy) = crate::ast_node_policy(row.kind_name) else {
            return Err(AstGeometryDrift::MissingPolicyRow {
                kind_name: row.kind_name.to_string(),
                field: row.field.to_string(),
            });
        };

        if row.payload_role.is_none() && !policy.payload_policies.is_empty() {
            return Err(AstGeometryDrift::MissingPayloadRole {
                kind_name: row.kind_name.to_string(),
                field: row.field.to_string(),
                declared: policy.payload_policies.to_vec(),
            });
        }

        if let Some(role) = row.payload_role
            && !policy.payload_policies.contains(&role)
        {
            return Err(AstGeometryDrift::PayloadRoleNotDeclaredByVariant {
                kind_name: row.kind_name.to_string(),
                field: row.field.to_string(),
                role,
            });
        }

        let required = geometry_disposition_for_role(row.payload_role, policy.classification);
        if row.disposition != required {
            return Err(AstGeometryDrift::DispositionMismatch {
                kind_name: row.kind_name.to_string(),
                field: row.field.to_string(),
                registered: row.disposition,
                required,
            });
        }
    }

    for fixture in crate::node_kind_fixtures() {
        reconcile_node_geometry(&fixture.sample)?;
    }

    Ok(())
}

/// Reconcile one node against the canonical registry.
///
/// # Errors
///
/// Returns the [`AstGeometryDrift`] described by [`reconcile_geometry_rows`].
pub fn reconcile_node_geometry(node: &Node) -> Result<(), AstGeometryDrift> {
    reconcile_geometry_rows(
        node.kind.kind_name(),
        AST_NODE_GEOMETRY_FIELDS,
        &observe_geometry_fields(&node.kind),
    )
}

/// Observe every independent source-geometry payload field on one node kind.
///
/// This is the observation authority for [`reconcile_geometry_rows`], and it is
/// deliberately verbose: **every arm names every field of its variant, and no
/// arm uses `..`**. Adding a `SourceLocation`, optional or repeated span,
/// recovery `Token`, or nested geometry record to an existing variant therefore
/// breaks compilation here until it is classified. A guard that matched
/// `NodeKind::Subroutine { .. }` would accept the new field silently, which is
/// exactly the drift this registry exists to prevent.
///
/// Structural children are not observed here; they belong to the canonical
/// field-aware traversal.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn observe_geometry_fields(kind: &NodeKind) -> Vec<ObservedGeometryField> {
    /// A variant carrying no independent payload geometry.
    const NONE: Vec<ObservedGeometryField> = Vec::new();

    fn optional_span(field: &'static str, present: bool) -> Vec<ObservedGeometryField> {
        vec![ObservedGeometryField {
            field,
            shape: AstGeometryShape::Optional,
            occurrences: usize::from(present),
        }]
    }

    match kind {
        NodeKind::Program { statements: _ } => NONE,
        NodeKind::ExpressionStatement { expression: _ } => NONE,
        NodeKind::VariableDeclaration {
            declarator: _,
            variable: _,
            attributes: _,
            initializer: _,
        } => NONE,
        NodeKind::VariableListDeclaration {
            declarator: _,
            variables: _,
            attributes: _,
            initializer: _,
        } => NONE,
        NodeKind::NestedVariableList { items: _ } => NONE,
        NodeKind::Variable { sigil: _, name: _ } => NONE,
        NodeKind::VariableWithAttributes { variable: _, attributes: _ } => NONE,
        NodeKind::Assignment { lhs: _, rhs: _, op: _ } => NONE,
        NodeKind::Binary { op: _, left: _, right: _ } => NONE,
        NodeKind::ArraySlice { target: _, indices: _ } => NONE,
        NodeKind::HashSlice { target: _, keys: _ } => NONE,
        NodeKind::KeyValueSlice { target: _, keys: _ } => NONE,
        NodeKind::ChainedComparison { operands: _, ops: _ } => NONE,
        NodeKind::Ternary { condition: _, then_expr: _, else_expr: _ } => NONE,
        NodeKind::Unary { op: _, operand: _ } => NONE,
        NodeKind::Diamond | NodeKind::Ellipsis | NodeKind::Undef => NONE,
        NodeKind::Readline { filehandle: _ } => NONE,
        NodeKind::Glob { pattern: _ } => NONE,
        NodeKind::Typeglob { name: _ } => NONE,
        NodeKind::Number { value: _ } => NONE,
        NodeKind::String { value: _, interpolated: _ } => NONE,
        NodeKind::VString { value: _ } => NONE,
        NodeKind::Heredoc {
            delimiter: _,
            content: _,
            interpolated: _,
            indented: _,
            command: _,
            body_span,
        } => optional_span("body_span", body_span.is_some()),
        NodeKind::ArrayLiteral { elements: _ } => NONE,
        NodeKind::HashLiteral { pairs: _ } => NONE,
        NodeKind::Block { statements: _ } => NONE,
        NodeKind::Eval { block: _ } => NONE,
        NodeKind::Do { block: _ } => NONE,
        NodeKind::Defer { block: _ } => NONE,
        NodeKind::Try { body: _, catch_blocks, finally_block: _ } => {
            vec![ObservedGeometryField {
                field: "catch_blocks.variable",
                shape: AstGeometryShape::Nested,
                occurrences: catch_blocks.iter().filter(|(variable, _)| variable.is_some()).count(),
            }]
        }
        NodeKind::If {
            condition: _,
            then_branch: _,
            elsif_branches: _,
            else_branch: _,
            keyword: _,
        } => NONE,
        NodeKind::LabeledStatement { label: _, statement: _ } => NONE,
        NodeKind::While { condition: _, body: _, continue_block: _, keyword: _ } => NONE,
        NodeKind::Tie { variable: _, package: _, args: _ } => NONE,
        NodeKind::Untie { variable: _ } => NONE,
        NodeKind::For { init: _, condition: _, update: _, body: _, continue_block: _ } => NONE,
        NodeKind::Foreach { variable: _, list: _, body: _, continue_block: _ } => NONE,
        NodeKind::Given { expr: _, body: _ } => NONE,
        NodeKind::When { condition: _, body: _ } => NONE,
        NodeKind::Default { body: _ } => NONE,
        NodeKind::StatementModifier { statement: _, modifier: _, condition: _ } => NONE,
        NodeKind::Subroutine {
            name: _,
            name_span,
            declarator: _,
            prototype: _,
            signature: _,
            attributes: _,
            body: _,
        } => optional_span("name_span", name_span.is_some()),
        NodeKind::Prototype { content: _ } => NONE,
        NodeKind::Signature { parameters: _ } => NONE,
        NodeKind::MandatoryParameter { variable: _ } => NONE,
        NodeKind::OptionalParameter { variable: _, default_value: _ } => NONE,
        NodeKind::SlurpyParameter { variable: _ } => NONE,
        NodeKind::NamedParameter {
            variable: _,
            external_name: _,
            default_operator: _,
            default_value: _,
            required: _,
        } => NONE,
        NodeKind::Method { name: _, name_span, signature: _, attributes: _, body: _ } => {
            optional_span("name_span", name_span.is_some())
        }
        NodeKind::Return { value: _ } => NONE,
        NodeKind::LoopControl { op: _, label: _ } => NONE,
        NodeKind::Goto { target: _, form: _ } => NONE,
        NodeKind::MethodCall { object: _, method: _, args: _ } => NONE,
        NodeKind::FunctionCall { name: _, args: _ } => NONE,
        NodeKind::AmperCall { name: _, args: _ } => NONE,
        NodeKind::IndirectCall { method: _, object: _, args: _ } => NONE,
        NodeKind::Regex { pattern: _, replacement: _, modifiers: _, has_embedded_code: _ } => NONE,
        NodeKind::Match { expr: _, pattern: _, modifiers: _, has_embedded_code: _, negated: _ } => {
            NONE
        }
        NodeKind::Substitution {
            expr: _,
            pattern: _,
            replacement: _,
            modifiers: _,
            has_embedded_code: _,
            negated: _,
        } => NONE,
        NodeKind::Transliteration { expr: _, search: _, replace: _, modifiers: _, negated: _ } => {
            NONE
        }
        NodeKind::Package { name: _, name_span: _, block: _ } => {
            vec![ObservedGeometryField {
                field: "name_span",
                shape: AstGeometryShape::Direct,
                occurrences: 1,
            }]
        }
        NodeKind::Use { module: _, args: _, has_filter_risk: _ } => NONE,
        NodeKind::No { module: _, args: _, has_filter_risk: _ } => NONE,
        NodeKind::PhaseBlock { phase: _, phase_span, block: _ } => {
            optional_span("phase_span", phase_span.is_some())
        }
        NodeKind::DataSection { marker: _, body: _ } => NONE,
        NodeKind::Class { name: _, name_span, parents: _, body: _ } => {
            optional_span("name_span", name_span.is_some())
        }
        NodeKind::Format { name: _, name_span, body: _ } => {
            optional_span("name_span", name_span.is_some())
        }
        NodeKind::Identifier { name: _ } => NONE,
        NodeKind::Error { message: _, expected: _, found, partial: _ } => {
            vec![ObservedGeometryField {
                field: "found",
                shape: AstGeometryShape::Token,
                occurrences: usize::from(found.is_some()),
            }]
        }
        NodeKind::MissingExpression
        | NodeKind::MissingStatement
        | NodeKind::MissingIdentifier
        | NodeKind::MissingBlock
        | NodeKind::UnknownRest => NONE,
    }
}
