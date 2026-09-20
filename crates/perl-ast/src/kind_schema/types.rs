//! Structural row vocabulary for the `NodeKind` schema.
//!
//! These types describe primary-AST structure. [`FieldCardinality`] and
//! [`ChildFieldSpec`] are production facts for FieldId membership and
//! field-aware traversal. `source_boundary` is recorded only; it is not
//! production classification authority.

use crate::FieldId;

/// How many times a named child field may be emitted by canonical traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldCardinality {
    /// Emitted at least once on every instance. The same [`FieldId`] may still
    /// repeat (for example `If` reuses `condition` for each `elsif`).
    Required,
    /// Emitted zero or one time. Never more than once.
    Optional,
    /// Emitted zero or more times under one [`FieldId`].
    Repeated,
}

impl FieldCardinality {
    /// Stable token used by deterministic serialization.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Repeated => "repeated",
        }
    }
}

/// One named child relationship in canonical emission order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildFieldSpec {
    /// Canonical field identity shared with production [`FieldId`].
    pub field: FieldId,
    /// Required, optional, or repeated emission.
    pub cardinality: FieldCardinality,
}

/// Whether a variant owns structural AST children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KindBody {
    /// No structural AST children.
    Leaf,
    /// Owns one or more structural child fields.
    ChildBearing,
}

/// How the public grammar name is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarNameSpec<'a> {
    /// Grammar name is fixed by the variant.
    Static(&'a str),
    /// Grammar name is computed from named payload or child-field inputs.
    RuntimeDerived {
        /// Field names that can change the runtime grammar name.
        inputs: &'a [&'a str],
    },
}

/// Public schema compatibility ruling for this structural row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaCompatibility {
    /// Describes current-main public names and child identities without change.
    Current,
}

impl SchemaCompatibility {
    /// Stable token used by deterministic serialization.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Current => "current",
        }
    }
}

/// One checked structural row for a primary [`crate::NodeKind`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindStructuralRow<'a> {
    /// Stable [`crate::NodeKind::kind_name`] token.
    pub kind_name: &'a str,
    /// Child fields in canonical first-emission order.
    pub children: &'a [ChildFieldSpec],
    /// Leaf versus child-bearing body.
    pub body: KindBody,
    /// Whether this variant is a recovery/synthetic kind.
    pub recovery: bool,
    /// Whether this variant is a specialized source-boundary kind.
    ///
    /// Recorded and serialized only. This flag is not production authority:
    /// #8415 did not reconcile it against a production inventory, and #8424
    /// does not promote it.
    pub source_boundary: bool,
    /// Static grammar name or runtime-derived inputs.
    pub grammar: GrammarNameSpec<'a>,
    /// Compatibility disposition for later cutover work.
    pub compatibility: SchemaCompatibility,
}

impl KindStructuralRow<'_> {
    /// Whether this row claims no structural children.
    #[must_use]
    pub const fn is_leaf(self) -> bool {
        matches!(self.body, KindBody::Leaf)
    }

    /// Whether this row claims structural children.
    #[must_use]
    pub const fn is_child_bearing(self) -> bool {
        matches!(self.body, KindBody::ChildBearing)
    }
}
