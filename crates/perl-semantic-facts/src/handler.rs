//! Canonical framework handler relation (#8924).
//!
//! [`FrameworkHandler`] is the framework-neutral handler operand shared by the
//! canonical route (#8918) and hook (#8924) fact families: the relation a
//! framework declaration (`get '/x' => ...`, `hook 'before' => ...`) retains
//! for its callback operand. It replaces the route-local `RouteHandler` arm
//! (kept as a re-export alias) so both families carry one handler contract.
//!
//! Exactness contract:
//!
//! - [`FrameworkHandler::InlineSub`] is an exact source-backed anchor (the
//!   `sub { ... }` operand tokens);
//! - [`FrameworkHandler::StaticCoderef`] is an exact handler relation **only**
//!   after the named subroutine target resolved to an in-file package-scoped
//!   declaration (`\&handler` with `sub handler { ... }` — or a `sub handler;`
//!   stub — in the same file and package, including forward declarations).
//!   The promotion from typed boundary is #8924: before it, every static
//!   coderef stayed bounded because the canonical callable fact layer did not
//!   prove named-subroutine targets;
//! - [`FrameworkHandler::Bounded`] keeps the declaration usable but degraded:
//!   a coderef whose target is not statically resolvable in-file (dynamic,
//!   cross-file, undefined), a string handler, or a computed expression. Typed
//!   boundaries, never fictional targets.

use crate::{EntityId, FactId, FileId, SourceAnchor, SourceGeneration};
use serde::{Deserialize, Serialize};

/// Why a handler operand is not an exact subroutine target.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FrameworkHandlerBoundary {
    /// Static coderef expression (e.g. `\&handler`): anchored, but no in-file
    /// package-scoped declaration of the named subroutine was found, so the
    /// target is not proven.
    StaticCoderef,
    /// String handler (Dancer v1 style action name): not a coderef target.
    String,
    /// Computed handler expression.
    Computed,
}

/// An in-file named subroutine declaration a static coderef resolved to.
///
/// Identity is `(file, package, name)`-scoped and carries exact anchors: the
/// declaration-name token (navigation identity), the full declaration span,
/// and the body span when the declaration carries one (a `sub name;` forward
/// stub has no body). Never a fictional body: every anchor comes from the
/// resolved declaration node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubroutineTarget {
    /// Declared subroutine name.
    pub name: String,
    /// Package the declaration appears in (Perl package-scoped subs).
    pub package: String,
    /// Exact anchor of the declaration name token.
    pub name_anchor: SourceAnchor,
    /// Exact anchor of the whole declaration (`sub name ... { ... }` / stub).
    pub declaration_anchor: SourceAnchor,
    /// Exact anchor of the declaration body, when the declaration has one.
    pub body_anchor: Option<SourceAnchor>,
}

impl SubroutineTarget {
    /// Deterministic entity identity of the resolved declaration.
    ///
    /// Mirrors the family identity scheme: file, name, package, and minting
    /// generation. Re-resolving the same generation reproduces the identity;
    /// declarations of different roots/edits never collide. The name and
    /// package components are length-framed before hashing so no `(name,
    /// package)` pair boundary can alias another (`("a", "bc")` and
    /// `("ab", "c")` never share an identity, including with separator-like
    /// bytes inside either component).
    #[must_use]
    pub fn entity_id(&self, file_id: FileId, generation: &SourceGeneration) -> EntityId {
        let generation_digest = match generation {
            SourceGeneration::Known(value) => {
                value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
                })
            }
            SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
        };
        let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut name_digest = 0x8422_2325_cbf2_9ce4_u64;
        let mut fold = |part: &str| {
            for byte in part.len().to_be_bytes().into_iter().chain(part.bytes()) {
                name_digest = (name_digest ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3);
            }
        };
        fold(&self.name);
        fold(&self.package);
        EntityId(file ^ name_digest ^ generation_digest)
    }

    /// Deterministic fact identity of the resolved-declaration receipt.
    #[must_use]
    pub fn fact_id(&self, file_id: FileId, generation: &SourceGeneration) -> FactId {
        let entity = self.entity_id(file_id, generation);
        FactId(entity.0 ^ 0xD3C1_B07A_5EED_1DE9)
    }
}

/// Handler relation of one framework declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameworkHandler {
    /// Inline anonymous sub: exact source-backed handler anchor.
    InlineSub {
        /// Source range of the `sub { ... }` operand (exact tokens).
        anchor: SourceAnchor,
    },
    /// Static coderef (`\&handler`, `\&Package::handler`) whose named target
    /// resolved to an in-file package-scoped declaration: exact handler
    /// relation with the declaration identity.
    StaticCoderef {
        /// Written target name of the coderef operand (as spelled in source,
        /// qualification included).
        name: String,
        /// Source range of the `\&name` operand (exact tokens).
        anchor: SourceAnchor,
        /// Resolved in-file declaration of the named subroutine.
        target: SubroutineTarget,
    },
    /// Bounded handler relation: the declaration fact is retained, the
    /// handler is not an exact framework subroutine target.
    Bounded {
        /// Boundary classification.
        boundary: FrameworkHandlerBoundary,
        /// Source range of the handler operand, when anchored.
        anchor: Option<SourceAnchor>,
        /// Bounded explanation.
        reason: String,
    },
}

impl FrameworkHandler {
    /// Whether this handler relation is an exact source-backed target
    /// (inline sub or resolved static coderef).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        !matches!(self, FrameworkHandler::Bounded { .. })
    }

    /// The exact handler anchor, when the relation is exact.
    ///
    /// This is the operand range (`sub { ... }` or `\&name`), not the resolved
    /// declaration anchors carried by [`FrameworkHandler::StaticCoderef`].
    #[must_use]
    pub fn exact_anchor(&self) -> Option<SourceAnchor> {
        match self {
            FrameworkHandler::InlineSub { anchor }
            | FrameworkHandler::StaticCoderef { anchor, .. } => Some(*anchor),
            FrameworkHandler::Bounded { anchor, .. } => *anchor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnchorId, FileId};

    fn anchor(start: u32, end: u32) -> SourceAnchor {
        SourceAnchor::new(Some(AnchorId(start as u64)), FileId(1), start, end)
    }

    fn target(name: &str, package: &str) -> SubroutineTarget {
        SubroutineTarget {
            name: name.to_string(),
            package: package.to_string(),
            name_anchor: anchor(4, 11),
            declaration_anchor: anchor(0, 30),
            body_anchor: Some(anchor(12, 30)),
        }
    }

    #[test]
    fn resolved_coderef_is_exact_and_carries_target_anchors() {
        let handler = FrameworkHandler::StaticCoderef {
            name: "handler".to_string(),
            anchor: anchor(40, 49),
            target: target("handler", "App"),
        };
        assert!(handler.is_exact());
        assert_eq!(handler.exact_anchor(), Some(anchor(40, 49)), "operand range");
        let FrameworkHandler::StaticCoderef { target, .. } = &handler else {
            unreachable!("matched above");
        };
        assert_eq!(target.package, "App");
        assert!(target.body_anchor.is_some(), "full declarations carry a body");
    }

    #[test]
    fn bounded_kinds_are_not_exact() {
        for boundary in [
            FrameworkHandlerBoundary::StaticCoderef,
            FrameworkHandlerBoundary::String,
            FrameworkHandlerBoundary::Computed,
        ] {
            let handler = FrameworkHandler::Bounded {
                boundary,
                anchor: Some(anchor(0, 9)),
                reason: "not an exact target".to_string(),
            };
            assert!(!handler.is_exact());
        }
    }

    #[test]
    fn target_identities_are_deterministic_and_scoped() {
        let generation = SourceGeneration::known("gen-1");
        let first = target("handler", "App");
        let other_package = target("handler", "Other");
        let other_name = target("other", "App");
        let id = first.entity_id(FileId(1), &generation);
        assert_eq!(id, first.entity_id(FileId(1), &generation), "deterministic");
        assert_ne!(id, other_package.entity_id(FileId(1), &generation));
        assert_ne!(id, other_name.entity_id(FileId(1), &generation));
        assert_ne!(id, first.entity_id(FileId(2), &generation), "file-scoped");
        assert_ne!(
            id,
            first.entity_id(FileId(1), &SourceGeneration::known("gen-2")),
            "generation-scoped"
        );
        assert_ne!(
            first.fact_id(FileId(1), &generation).0,
            first.entity_id(FileId(1), &generation).0,
            "fact and entity identities stay disjoint"
        );
    }

    #[test]
    fn target_identities_frame_name_and_package_boundaries() {
        // Length framing: no `(name, package)` concatenation boundary may
        // alias another pair — including pairs whose components contain
        // separator-like bytes (NUL, colons) that a bare chained hash or a
        // single-separator hash would collide.
        let generation = SourceGeneration::known("gen-1");
        let pairs = [
            ("a", "bc"),
            ("ab", "c"),
            ("handler\u{0}x", "App"),
            ("handler", "\u{0}xApp"),
            ("App::x", "handler"),
            ("App", "::xhandler"),
        ];
        let ids: Vec<EntityId> = pairs
            .iter()
            .map(|(name, package)| target(name, package).entity_id(FileId(1), &generation))
            .collect();
        for (left, left_id) in pairs.iter().zip(&ids) {
            for (right, right_id) in pairs.iter().zip(&ids) {
                if left != right {
                    assert_ne!(
                        left_id, right_id,
                        "({left:?}) and ({right:?}) must never share an entity identity"
                    );
                }
            }
        }
    }
}
