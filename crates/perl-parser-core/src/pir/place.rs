//! PIR-A place identities and structural place kinds.
//!
//! A place names storage that a later PIR operation may read, write, modify,
//! alias, localize, or reference. This module defines identity and geometry only:
//! it does not attach one global access mode to a place, evaluate Perl, or change
//! provider behavior. The contract follows PLSP-SPEC-0032 and is embedded into
//! `PirGraph` by the follow-up tracked in #13593.

use super::model::{LexicalName, PirId, PirSourceAnchor, SymbolName};
use crate::hir::{HirBodyId, HirExprId, HirScopeId};

/// Stable identifier for a place within one PIR graph.
///
/// Place IDs deliberately use a distinct type from [`PirId`]. A node computes or
/// uses a place; it is not itself the place identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct PirPlaceId {
    index: u32,
}

impl PirPlaceId {
    /// Create an identifier from a zero-based place-registry index.
    #[inline]
    #[must_use]
    pub const fn from_index(index: u32) -> Self {
        Self { index }
    }

    /// Return the zero-based place-registry index.
    #[inline]
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }
}

/// Slot selected from a Perl typeglob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PirGlobSlotKind {
    /// Scalar slot.
    Scalar,
    /// Array slot.
    Array,
    /// Hash slot.
    Hash,
    /// Code/subroutine slot.
    Code,
    /// Filehandle/I/O slot.
    Io,
    /// Format slot.
    Format,
    /// Whole glob identity when no narrower slot is proven.
    Glob,
}

impl PirGlobSlotKind {
    /// Stable name used by receipts and snapshots.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Scalar => "Scalar",
            Self::Array => "Array",
            Self::Hash => "Hash",
            Self::Code => "Code",
            Self::Io => "Io",
            Self::Format => "Format",
            Self::Glob => "Glob",
        }
    }
}

/// Canonical HIR origin for a place when the producing path can name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PirPlaceOrigin {
    /// Place came from the flat HIR-item path; the item is retained by the
    /// [`PirSourceAnchor`].
    FlatHirItem,
    /// Place came from one expression in one canonical HIR body arena.
    BodyExpression {
        /// Body containing the expression.
        body: HirBodyId,
        /// Expression whose geometry produced the place.
        expression: HirExprId,
    },
    /// The producing path cannot yet name a canonical HIR owner.
    Unknown,
}

impl PirPlaceOrigin {
    /// Stable origin-family name used by receipts and snapshots.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FlatHirItem => "FlatHirItem",
            Self::BodyExpression { .. } => "BodyExpression",
            Self::Unknown => "Unknown",
        }
    }
}

/// Structural identity of one Perl storage place.
///
/// Element selectors are PIR value-node IDs evaluated once. Nested element
/// bases are place IDs, so read, write, and modify operations can share one
/// evaluated location without flattening the path to source text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PirPlaceKind {
    /// Lexical storage such as a `my` or `state` variable.
    Lexical {
        /// Lexical name.
        name: LexicalName,
        /// Defining scope when canonical scope resolution is available.
        scope: Option<HirScopeId>,
    },
    /// Package/stash storage slot.
    PackageSlot {
        /// Package-qualified symbol identity when known.
        symbol: SymbolName,
    },
    /// One array element. `index` is evaluated exactly once.
    ArrayElement {
        /// Place containing the array element.
        base: PirPlaceId,
        /// PIR node computing the element index.
        index: PirId,
    },
    /// One hash element. `key` is evaluated exactly once.
    HashElement {
        /// Place containing the hash element.
        base: PirPlaceId,
        /// PIR node computing the element key.
        key: PirId,
    },
    /// One statically selected typeglob slot.
    GlobSlot {
        /// Glob symbol identity.
        symbol: SymbolName,
        /// Selected slot.
        slot: PirGlobSlotKind,
    },
    /// Storage reached through one evaluated reference value.
    Dereferenced {
        /// PIR node computing the reference value.
        reference: PirId,
    },
    /// Storage identity cannot be proven statically.
    Dynamic {
        /// Stable, human-readable explanation retained with the place.
        reason: String,
    },
}

impl PirPlaceKind {
    /// Stable kind-family name used by receipts and snapshots.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Lexical { .. } => "Lexical",
            Self::PackageSlot { .. } => "PackageSlot",
            Self::ArrayElement { .. } => "ArrayElement",
            Self::HashElement { .. } => "HashElement",
            Self::GlobSlot { .. } => "GlobSlot",
            Self::Dereferenced { .. } => "Dereferenced",
            Self::Dynamic { .. } => "Dynamic",
        }
    }
}

/// One source-anchored place record.
///
/// The two provenance fields are deliberately not redundant:
/// [`PirSourceAnchor`] records *source-text* provenance (which source text and
/// flat HIR item caused this place geometry), while [`PirPlaceOrigin`] records
/// *canonical body-arena* provenance (which [`HirBodyId`]/[`HirExprId`]
/// produced the place). Keep both until #13593 embeds places into `PirGraph`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PirPlace {
    /// Stable place ID within the containing graph.
    pub id: PirPlaceId,
    /// Structural storage identity.
    pub kind: PirPlaceKind,
    /// Source-text / flat-HIR-item provenance for the place geometry.
    pub source_anchor: PirSourceAnchor,
    /// Canonical HIR body-arena origin when available.
    pub origin: PirPlaceOrigin,
    /// Dynamic-boundary node that qualifies this place, when any.
    pub dynamic_boundary: Option<PirId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceLocation;
    use crate::hir::HirId;

    #[test]
    fn place_id_round_trips_without_becoming_a_node_id() {
        let place = PirPlaceId::from_index(7);
        let node = PirId::from_index(7);

        assert_eq!(place.index(), 7);
        assert_eq!(node.index(), 7);
        assert_ne!(format!("{place:?}"), format!("{node:?}"));
    }

    #[test]
    fn glob_slot_names_are_stable() {
        assert_eq!(PirGlobSlotKind::Scalar.name(), "Scalar");
        assert_eq!(PirGlobSlotKind::Array.name(), "Array");
        assert_eq!(PirGlobSlotKind::Hash.name(), "Hash");
        assert_eq!(PirGlobSlotKind::Code.name(), "Code");
        assert_eq!(PirGlobSlotKind::Io.name(), "Io");
        assert_eq!(PirGlobSlotKind::Format.name(), "Format");
        assert_eq!(PirGlobSlotKind::Glob.name(), "Glob");
    }

    #[test]
    fn place_kind_names_are_stable() {
        let lexical = PirPlaceKind::Lexical {
            name: LexicalName { sigil: "$".to_string(), name: "node".to_string() },
            scope: Some(HirScopeId::from_index(3)),
        };
        let package = PirPlaceKind::PackageSlot {
            symbol: SymbolName {
                sigil: "%".to_string(),
                name: "tree".to_string(),
                package: Some("RB".to_string()),
            },
        };
        let array = PirPlaceKind::ArrayElement {
            base: PirPlaceId::from_index(0),
            index: PirId::from_index(4),
        };
        let hash = PirPlaceKind::HashElement {
            base: PirPlaceId::from_index(0),
            key: PirId::from_index(5),
        };
        let glob = PirPlaceKind::GlobSlot {
            symbol: SymbolName { sigil: "*".to_string(), name: "entry".to_string(), package: None },
            slot: PirGlobSlotKind::Scalar,
        };
        let dereferenced = PirPlaceKind::Dereferenced { reference: PirId::from_index(6) };
        let dynamic = PirPlaceKind::Dynamic { reason: "runtime container".to_string() };

        assert_eq!(lexical.name(), "Lexical");
        assert_eq!(package.name(), "PackageSlot");
        assert_eq!(array.name(), "ArrayElement");
        assert_eq!(hash.name(), "HashElement");
        assert_eq!(glob.name(), "GlobSlot");
        assert_eq!(dereferenced.name(), "Dereferenced");
        assert_eq!(dynamic.name(), "Dynamic");
    }

    #[test]
    fn array_and_hash_elements_retain_distinct_base_and_selector_ids() {
        let base = PirPlaceId::from_index(2);
        let selector = PirId::from_index(9);

        // Round-trip equality: same base + same selector compare equal.
        assert_eq!(
            PirPlaceKind::ArrayElement { base, index: selector },
            PirPlaceKind::ArrayElement { base: PirPlaceId::from_index(2), index: selector }
        );
        // Distinct bases and distinct selectors both break identity, so bases
        // and selectors participate in place equality.
        assert_ne!(
            PirPlaceKind::ArrayElement { base, index: selector },
            PirPlaceKind::ArrayElement { base: PirPlaceId::from_index(3), index: selector }
        );
        assert_ne!(
            PirPlaceKind::ArrayElement { base, index: selector },
            PirPlaceKind::ArrayElement { base, index: PirId::from_index(10) }
        );
        // Variant distinction: same payload under a different kind is not equal.
        assert_ne!(
            PirPlaceKind::ArrayElement { base, index: selector },
            PirPlaceKind::HashElement { base, key: selector }
        );
        assert_ne!(
            PirPlaceKind::HashElement { base, key: selector },
            PirPlaceKind::HashElement { base: PirPlaceId::from_index(3), key: selector }
        );
        assert_ne!(
            PirPlaceKind::HashElement { base, key: selector },
            PirPlaceKind::HashElement { base, key: PirId::from_index(10) }
        );
    }

    #[test]
    fn body_origin_keeps_body_and_expression_identity_together() {
        let origin =
            PirPlaceOrigin::BodyExpression { body: HirBodyId(2), expression: HirExprId(11) };

        assert_eq!(origin.name(), "BodyExpression");
        assert_eq!(
            origin,
            PirPlaceOrigin::BodyExpression { body: HirBodyId(2), expression: HirExprId(11) }
        );
    }

    #[test]
    fn dynamic_place_retains_reason_source_and_boundary() {
        let range = SourceLocation::new(10, 28);
        let place = PirPlace {
            id: PirPlaceId::from_index(1),
            kind: PirPlaceKind::Dynamic { reason: "tied or magical container".to_string() },
            source_anchor: PirSourceAnchor::explicit(range, HirId::from_index(4)),
            origin: PirPlaceOrigin::Unknown,
            dynamic_boundary: Some(PirId::from_index(8)),
        };

        assert_eq!(place.id.index(), 1);
        assert_eq!(place.kind.name(), "Dynamic");
        assert_eq!(place.source_anchor.range, Some(range));
        assert_eq!(place.origin.name(), "Unknown");
        assert_eq!(place.dynamic_boundary, Some(PirId::from_index(8)));
    }
}
