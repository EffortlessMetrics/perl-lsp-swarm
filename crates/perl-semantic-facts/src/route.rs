//! Canonical framework route fact family (#8918).
//!
//! [`RouteFact`] is the transport-neutral canonical record for statically
//! observed framework route declarations (Dancer2 `get`/`post`/`any`/... style
//! DSL routes). It follows the [`crate::CallableResultFact`] precedent: a
//! payload fact wrapping the canonical [`SemanticFactEnvelope`] with the
//! envelope kind forced to [`SemanticFactKind::Route`], so providers can
//! classify a route fact without framework internals.
//!
//! The family is framework-neutral. Framework-specific minting (verb tables,
//! method normalization, activation gating) lives in concrete adapters such as
//! `framework_adapters::dancer2_routes`; this module only defines what a route
//! declaration must preserve: identity by source order, route name separate
//! from pattern, normalized method set, pattern kind, static options, handler
//! anchor, and the full declaration/keyword ranges. The handler relation
//! itself is the shared [`crate::handler`] contract (#8924 promoted the
//! static-coderef arm from a typed boundary to a resolved declaration fact).
//!
//! Exactness contract:
//!
//! - a route fact is [`SemanticFactStatus::Exact`] only when the envelope is
//!   exact **and** the payload carries no dynamic or bounded member (literal
//!   pattern, exact method set, exact handler relation, literal options);
//! - dynamic/computed/bounded members keep the fact usable but degraded —
//!   typed boundaries, never false exact fields;
//! - two declarations with the same methods and pattern remain distinct route
//!   entities through `declaration_index` (source declaration identity).

use crate::framework::AdapterId;
use crate::{
    BoundaryDisposition, BoundaryKind, Confidence, EntityId, FactId, Provenance,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};
use serde::{Deserialize, Serialize};

// Shared handler-relation aliases (#8924): the route family keeps its public
// `RouteHandler` spelling while the arms live in the shared `handler` module
// so the hook family carries the identical contract.
pub use crate::handler::FrameworkHandler as RouteHandler;
pub use crate::handler::FrameworkHandlerBoundary as RouteHandlerBoundary;
pub use crate::handler::SubroutineTarget as RouteSubroutineTarget;

/// Kind of a route pattern operand.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RoutePatternKind {
    /// Literal string route pattern (e.g. `'/users/:id'`).
    Literal,
    /// Regex route pattern with an exact source anchor (e.g. `qr{^/re/(\d+)$}`).
    Regex,
    /// Dynamic/computed pattern — a boundary, not an exact pattern.
    Dynamic,
}

/// One route pattern with its exact source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePattern {
    /// Pattern classification.
    pub kind: RoutePatternKind,
    /// Exact source text of the pattern operand. `None` only for
    /// [`RoutePatternKind::Dynamic`].
    pub value: Option<String>,
    /// Source range of the pattern operand (exact tokens).
    pub anchor: SourceAnchor,
}

/// One literal route name with its exact source anchor.
///
/// A route name is optional in every supported grammar and is never a
/// substitute for the route pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteName {
    /// Literal route name value (unquoted).
    pub value: String,
    /// Source range of the name operand (exact tokens).
    pub anchor: SourceAnchor,
}

/// Route-name slot of one route declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteNameSelection {
    /// Literal route name.
    Literal(RouteName),
    /// Computed name operand — an explicit boundary.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic name operand.
        anchor: SourceAnchor,
    },
    /// No name operand in the declaration.
    Absent,
}

/// Value of one static matching option.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteOptionValue {
    /// Static literal option value (unquoted).
    Literal(String),
    /// Computed option value — an explicit limitation, not a literal.
    Dynamic {
        /// Bounded explanation.
        reason: String,
    },
}

/// One static matching option entry with source ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteOption {
    /// Option key (unquoted literal).
    pub key: String,
    /// Source range of the key operand.
    pub key_anchor: SourceAnchor,
    /// Option value.
    pub value: RouteOptionValue,
    /// Source range of the value operand.
    pub value_anchor: SourceAnchor,
}

/// Matching options of one route declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteOptions {
    /// Static literal option map (typed entries with ranges).
    Map(Vec<RouteOption>),
    /// Computed/unsupported option expression — an explicit boundary.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic options operand, when anchored.
        anchor: Option<SourceAnchor>,
    },
}

/// Normalized HTTP method set of one route declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteMethodSet {
    /// Exact normalized method names (uppercase, framework-normalized).
    Exact(Vec<String>),
    /// Computed method list — a boundary, never `ANY` exactness.
    Dynamic {
        /// Bounded explanation.
        reason: String,
    },
}

/// Canonical payload of one route declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDeclaration {
    /// Source-order declaration identity within the owning file. Distinct
    /// declarations of the same methods/pattern stay distinct routes.
    pub declaration_index: u32,
    /// Route keyword token (e.g. `get`, `any`).
    pub keyword: String,
    /// Source range of the route keyword token.
    pub keyword_anchor: SourceAnchor,
    /// Literal route name, when present.
    pub route_name: RouteNameSelection,
    /// Normalized HTTP method set.
    pub methods: RouteMethodSet,
    /// Local route pattern (prefix composition is owned by later issues).
    pub pattern: RoutePattern,
    /// Static matching options.
    pub options: RouteOptions,
    /// Handler relation.
    pub handler: RouteHandler,
}

impl RouteDeclaration {
    /// Literal route name value, when the name slot is literal.
    #[must_use]
    pub fn route_name_literal_value(&self) -> Option<&str> {
        match &self.route_name {
            RouteNameSelection::Literal(name) => Some(name.value.as_str()),
            RouteNameSelection::Dynamic { .. } | RouteNameSelection::Absent => None,
        }
    }

    /// Whether any payload member is a dynamic or bounded boundary.
    #[must_use]
    pub fn has_boundary(&self) -> bool {
        matches!(self.methods, RouteMethodSet::Dynamic { .. })
            || self.pattern.kind == RoutePatternKind::Dynamic
            || matches!(self.route_name, RouteNameSelection::Dynamic { .. })
            || matches!(self.options, RouteOptions::Dynamic { .. })
            || matches!(self.handler, RouteHandler::Bounded { .. })
            || matches!(&self.options, RouteOptions::Map(entries)
                if entries.iter().any(|entry| matches!(entry.value, RouteOptionValue::Dynamic { .. })))
    }
}

/// Canonical framework route fact: envelope plus route payload and framework
/// identity.
///
/// Deserialization is checked: wire payloads are rebuilt through
/// [`RouteFact::new`], so every constructor-side invariant (envelope kind,
/// canonicalized method sets and option maps, no valueless literal patterns)
/// also holds for decoded facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteFact {
    /// Canonical semantic envelope (kind forced to `Route`).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Route payload.
    pub route: RouteDeclaration,
}

#[derive(Deserialize)]
struct RouteFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    route: RouteDeclaration,
}

impl<'de> Deserialize<'de> for RouteFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RouteFactWire::deserialize(deserializer)?;
        Ok(RouteFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.route,
        ))
    }
}

impl RouteFact {
    /// Construct a route fact and canonicalize the payload.
    ///
    /// The envelope kind is forced to [`SemanticFactKind::Route`]. Method sets
    /// are sorted and deduplicated; option maps are sorted by key with the last
    /// occurrence of a duplicated key winning (hash-construction semantics).
    #[allow(clippy::too_many_arguments)] // mirrors the fact contract fields
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        route: RouteDeclaration,
    ) -> Self {
        envelope.kind = SemanticFactKind::Route;
        let mut route = route;
        // A literal/regex pattern without a value is incoherent as an exact
        // operand: coerce it to the dynamic boundary so no exact status can
        // rest on a missing value (covers unchecked wire payloads).
        if matches!(route.pattern.kind, RoutePatternKind::Literal | RoutePatternKind::Regex)
            && route.pattern.value.is_none()
        {
            route.pattern.kind = RoutePatternKind::Dynamic;
        }
        if let RouteMethodSet::Exact(methods) = &mut route.methods {
            methods.sort();
            methods.dedup();
        }
        if let RouteOptions::Map(entries) = &mut route.options {
            entries.sort_by(|left, right| left.key.cmp(&right.key));
            // Hash-construction semantics: the last occurrence of a
            // duplicated key wins.
            let mut deduplicated: Vec<crate::route::RouteOption> =
                Vec::with_capacity(entries.len());
            for entry in entries.drain(..) {
                if deduplicated.last().is_some_and(|last| last.key == entry.key) {
                    deduplicated.pop();
                }
                deduplicated.push(entry);
            }
            *entries = deduplicated;
        }
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            route,
        }
    }

    /// Classify the complete route fact for a provider decision.
    ///
    /// An exact envelope with a bounded payload member stays degraded: routes
    /// with dynamic/computed members are usable but never exact.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if self.route.has_boundary() {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Deterministic route fact identity for one (file, declaration, generation).
///
/// Identity derives from the owning file, the source declaration order, and
/// the minting generation: re-mining the same generation reproduces the same
/// identities, two declarations of the same route stay distinct, and facts of
/// different roots/edits (different generations over root-local file ids)
/// never collide. Addition keeps the entity identity disjoint from the fact
/// identity.
#[must_use]
pub fn route_fact_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let generation_digest = match generation {
        // FNV-1a accumulation: order-sensitive and repetition-sensitive, so
        // distinct generation identities (including transposed or repeated
        // spellings like `"11"` vs `"22"`) never collide.
        SourceGeneration::Known(value) => {
            value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
            })
        }
        // An unknown generation is still a distinct minting context; it can
        // never produce an exact fact (the envelope degrades it separately).
        SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
    };
    let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let index = u64::from(declaration_index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let fact = file ^ index ^ generation_digest;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Build the canonical envelope for one minted route fact.
///
/// Shared by framework adapters so every route fact carries the same
/// producer/provenance/freshness contract: producer `FrameworkAdapter`,
/// AST-exact provenance, high confidence, fresh generation, and invalidation
/// dependencies over the owning source file plus the activating framework
/// module. The reason code and optional boundary link reflect the payload's
/// exactness.
#[allow(clippy::too_many_arguments)] // mirrors the envelope contract fields
#[must_use]
pub fn route_envelope(
    fact_id: FactId,
    entity_id: EntityId,
    package: Option<&str>,
    declaration_anchor: SourceAnchor,
    generation: &SourceGeneration,
    dependencies: Vec<crate::InvalidationDependency>,
    boundary: Option<BoundaryKind>,
    boundary_reason: crate::SemanticReasonCode,
    exact: bool,
) -> SemanticFactEnvelope {
    SemanticFactEnvelope::new(
        fact_id,
        Some(entity_id),
        SemanticFactKind::Route,
        declaration_anchor,
        generation.clone(),
        None,
        package.map(ToString::to_string),
        crate::LifecyclePhase::Runtime,
        SemanticProducer::FrameworkAdapter,
        SemanticProvenance::Known(Provenance::ExactAst),
        SemanticConfidence::Known(Confidence::High),
        SemanticFreshness::Fresh,
        boundary.map(|kind| {
            // No separate boundary fact is minted: the typed boundary lives in
            // the route payload, so the link carries no foreign boundary id.
            crate::BoundaryLink::new(None, kind, BoundaryDisposition::Degrade, boundary_reason)
        }),
        dependencies,
        if exact {
            SemanticReasonCode::ExactSource
        } else {
            SemanticReasonCode::GeneratedFromSource
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnchorId, FileId, InvalidationDependency};
    use perl_test_must::must_some;

    fn anchor(start: u32, end: u32) -> SourceAnchor {
        SourceAnchor::new(Some(AnchorId(start as u64)), FileId(1), start, end)
    }

    fn literal_route(declaration_index: u32) -> RouteDeclaration {
        RouteDeclaration {
            declaration_index,
            keyword: "get".to_string(),
            keyword_anchor: anchor(0, 3),
            route_name: RouteNameSelection::Absent,
            methods: RouteMethodSet::Exact(vec!["GET".to_string(), "HEAD".to_string()]),
            pattern: RoutePattern {
                kind: RoutePatternKind::Literal,
                value: Some("/x".to_string()),
                anchor: anchor(4, 8),
            },
            options: RouteOptions::Map(Vec::new()),
            handler: RouteHandler::InlineSub { anchor: anchor(12, 21) },
        }
    }

    fn envelope_for(fact_id: FactId, entity_id: EntityId, exact: bool) -> SemanticFactEnvelope {
        route_envelope(
            fact_id,
            entity_id,
            Some("App"),
            anchor(0, 21),
            &SourceGeneration::known("gen-1"),
            vec![InvalidationDependency::new("source:1", SourceGeneration::known("gen-1"))],
            if exact { None } else { Some(BoundaryKind::DynamicValue) },
            SemanticReasonCode::DynamicValue,
            exact,
        )
    }

    #[test]
    fn exact_literal_route_is_exact_and_forces_kind() {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            literal_route(0),
        );
        assert_eq!(fact.envelope.kind, SemanticFactKind::Route);
        assert_eq!(fact.status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn bounded_payload_members_degrade_the_fact() {
        for mutation in [
            RouteMutation::Pattern,
            RouteMutation::Methods,
            RouteMutation::Handler,
            RouteMutation::Options,
        ] {
            let (fact_id, entity_id) =
                route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
            let mut route = literal_route(0);
            mutation.apply(&mut route);
            assert!(route.has_boundary(), "{mutation:?} must be a boundary");
            // Bounded facts carry a boundary link on the envelope.
            let fact = RouteFact::new(
                envelope_for(fact_id, entity_id, false),
                "Dancer2",
                AdapterId(1),
                "1.1.1",
                "App",
                route,
            );
            assert_eq!(fact.status(), SemanticFactStatus::Degraded, "{mutation:?}");
        }
    }

    #[derive(Debug)]
    enum RouteMutation {
        Pattern,
        Methods,
        Handler,
        Options,
    }

    impl RouteMutation {
        fn apply(&self, route: &mut RouteDeclaration) {
            match self {
                RouteMutation::Pattern => {
                    route.pattern = RoutePattern {
                        kind: RoutePatternKind::Dynamic,
                        value: None,
                        anchor: anchor(4, 8),
                    };
                }
                RouteMutation::Methods => {
                    route.methods =
                        RouteMethodSet::Dynamic { reason: "computed method list".to_string() };
                }
                RouteMutation::Handler => {
                    route.handler = RouteHandler::Bounded {
                        boundary: RouteHandlerBoundary::String,
                        anchor: Some(anchor(12, 21)),
                        reason: "string handler".to_string(),
                    };
                }
                RouteMutation::Options => {
                    route.options = RouteOptions::Map(vec![RouteOption {
                        key: "agent".to_string(),
                        key_anchor: anchor(9, 15),
                        value: RouteOptionValue::Dynamic { reason: "computed".to_string() },
                        value_anchor: anchor(16, 20),
                    }]);
                }
            }
        }
    }

    #[test]
    fn method_set_and_option_map_are_canonicalized() {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let mut route = literal_route(1);
        route.methods =
            RouteMethodSet::Exact(vec!["POST".to_string(), "GET".to_string(), "POST".to_string()]);
        route.options = RouteOptions::Map(vec![
            RouteOption {
                key: "b".to_string(),
                key_anchor: anchor(1, 2),
                value: RouteOptionValue::Literal("first".to_string()),
                value_anchor: anchor(3, 4),
            },
            RouteOption {
                key: "a".to_string(),
                key_anchor: anchor(5, 6),
                value: RouteOptionValue::Literal("x".to_string()),
                value_anchor: anchor(7, 8),
            },
            RouteOption {
                key: "b".to_string(),
                key_anchor: anchor(9, 10),
                value: RouteOptionValue::Literal("last".to_string()),
                value_anchor: anchor(11, 12),
            },
        ]);
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            route,
        );
        let methods = match &fact.route.methods {
            RouteMethodSet::Exact(methods) => Some(methods.clone()),
            RouteMethodSet::Dynamic { .. } => None,
        };
        assert_eq!(
            must_some(methods),
            vec!["GET".to_string(), "POST".to_string()],
            "method sets are sorted and deduplicated"
        );
        let entries = match &fact.route.options {
            RouteOptions::Map(entries) => Some(entries),
            RouteOptions::Dynamic { .. } => None,
        };
        let entries = must_some(entries);
        assert_eq!(entries.len(), 2, "duplicate keys collapse (last wins)");
        assert_eq!(entries[0].key, "a");
        assert_eq!(entries[1].key, "b");
        assert_eq!(
            entries[1].value,
            RouteOptionValue::Literal("last".to_string()),
            "hash-construction keeps the last duplicate"
        );
    }

    #[test]
    fn identities_are_distinct_per_declaration_and_deterministic() {
        let (fact_a, entity_a) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let (fact_b, entity_b) =
            route_fact_identity(FileId(1), 1, &SourceGeneration::known("gen-1"));
        assert_ne!(fact_a, fact_b, "same route shape stays distinct by order");
        assert_ne!(entity_a, entity_b);
        assert_eq!(
            fact_a,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1")).0,
            "deterministic"
        );
        // Facts of a different root/edit (different generation) never collide,
        // even with root-local file ids.
        let other_root = route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-b"));
        assert_ne!(fact_a, other_root.0);
        let stale = route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-2"));
        assert_ne!(fact_a, stale.0);
    }

    #[test]
    fn generation_digest_is_order_and_repetition_sensitive() {
        // Regression: an order-insensitive fold made repeated-byte spellings
        // collide (both digests cancelled to the seed).
        assert_ne!(
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("11")).0,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("22")).0,
            "repeated-byte generations must not collide"
        );
        assert_ne!(
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-ab")).0,
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-ba")).0,
            "transposed generations must not collide"
        );
    }

    #[test]
    fn deserialization_reapplies_constructor_invariants() -> Result<(), serde_json::Error> {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            literal_route(0),
        );

        // A forged wire payload: wrong envelope kind, unsorted/duplicate
        // methods, and a valueless literal pattern.
        let mut value = serde_json::to_value(&fact)?;
        value["envelope"]["kind"] = serde_json::json!("Declaration");
        value["route"]["methods"] = serde_json::json!({ "Exact": ["POST", "GET", "POST"] });
        value["route"]["pattern"]["value"] = serde_json::json!(null);
        let decoded: RouteFact = serde_json::from_value(value)?;
        assert_eq!(decoded.envelope.kind, SemanticFactKind::Route, "kind is forced");
        let methods = must_some(match &decoded.route.methods {
            RouteMethodSet::Exact(methods) => Some(methods.clone()),
            _ => None,
        });
        assert_eq!(methods, vec!["GET".to_string(), "POST".to_string()]);
        assert_eq!(
            decoded.route.pattern.kind,
            RoutePatternKind::Dynamic,
            "a valueless literal pattern is coerced to the dynamic boundary"
        );
        assert_eq!(decoded.status(), SemanticFactStatus::Degraded);
        Ok(())
    }

    #[test]
    fn route_fact_roundtrips_through_json() -> Result<(), serde_json::Error> {
        let (fact_id, entity_id) =
            route_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-1"));
        let fact = RouteFact::new(
            envelope_for(fact_id, entity_id, true),
            "Dancer2",
            AdapterId(0x0044_4E43),
            "1.1.1",
            "MyApp",
            literal_route(0),
        );
        let serialized = serde_json::to_string(&fact)?;
        let decoded: RouteFact = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, fact);
        assert_eq!(decoded.status(), SemanticFactStatus::Exact);
        Ok(())
    }
}
