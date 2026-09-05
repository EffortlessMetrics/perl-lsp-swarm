//! Canonical framework hook fact family (#8924).
//!
//! [`HookFact`] is the transport-neutral canonical record for statically
//! observed framework hook declarations (Dancer2 `hook 'name' => $code` style
//! DSL hooks). It follows the [`crate::RouteFact`] precedent: a payload fact
//! wrapping the canonical [`SemanticFactEnvelope`] with the envelope kind
//! forced to [`SemanticFactKind::Hook`], so providers can classify a hook fact
//! without framework internals.
//!
//! The family is framework-neutral. Framework-specific minting (activation
//! gating, reviewed alias contracts) lives in concrete adapters such as
//! `framework_adapters::dancer2_hooks`; this module only defines what a hook
//! declaration must preserve: identity by source order, the literal source
//! hook name with its version-authoritative normalization state, the handler
//! relation (shared [`crate::handler`] contract), and the exact
//! declaration/keyword/name/handler ranges.
//!
//! Exactness contract:
//!
//! - a hook fact is [`SemanticFactStatus::Exact`] only when the envelope is
//!   exact **and** the payload carries no bounded member: the name operand is
//!   a static literal that the reviewed contract normalizes to a canonical
//!   hook name, and the handler relation is an exact inline sub or a resolved
//!   static coderef;
//! - a computed name operand, a hook name outside the reviewed contract
//!   (possible plugin/engine/runtime alias), or a bounded handler keeps the
//!   fact usable but degraded — typed boundaries, never false exact fields;
//! - two declarations of the same hook name remain distinct hook entities
//!   through `declaration_index` (source declaration identity).

use crate::framework::AdapterId;
use crate::handler::FrameworkHandler;
use crate::{
    BoundaryDisposition, BoundaryKind, Confidence, EntityId, FactId, Provenance,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};
use serde::{Deserialize, Serialize};

/// Provenance of one hook-name normalization.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookNameNormalization {
    /// The literal name is already a reviewed canonical hook name; no alias
    /// was applied.
    Canonical,
    /// The literal name is a reviewed alias; `canonical` is the
    /// version-authoritative canonical hook name it normalizes to.
    Alias {
        /// Canonical hook name after reviewed alias resolution.
        canonical: String,
    },
    /// The literal name is not in the reviewed contract: no canonical hook
    /// identity is claimed (a possible plugin/engine/runtime-registered alias
    /// or an unsupported name stays a typed boundary).
    Unresolved {
        /// Bounded explanation.
        reason: String,
    },
}

/// One literal hook name with its exact source anchor and normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookName {
    /// Literal source hook name (unquoted).
    pub literal: String,
    /// Source range of the name operand (exact tokens).
    pub anchor: SourceAnchor,
    /// Reviewed normalization state of the literal.
    pub normalization: HookNameNormalization,
}

impl HookName {
    /// Canonical hook name, when the reviewed contract resolves one.
    #[must_use]
    pub fn canonical(&self) -> Option<&str> {
        match &self.normalization {
            HookNameNormalization::Canonical => Some(self.literal.as_str()),
            HookNameNormalization::Alias { canonical } => Some(canonical.as_str()),
            HookNameNormalization::Unresolved { .. } => None,
        }
    }

    /// Whether the name normalization is a typed boundary.
    #[must_use]
    pub fn is_boundary(&self) -> bool {
        matches!(self.normalization, HookNameNormalization::Unresolved { .. })
    }
}

/// Hook-name slot of one hook declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookNameSelection {
    /// Static literal hook name with reviewed normalization.
    Literal(HookName),
    /// Computed name operand — an explicit boundary.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic name operand.
        anchor: SourceAnchor,
    },
}

/// Canonical payload of one hook declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDeclaration {
    /// Source-order declaration identity within the owning file. Distinct
    /// declarations of the same hook name stay distinct hook entities.
    pub declaration_index: u32,
    /// Hook keyword token (`hook`).
    pub keyword: String,
    /// Source range of the hook keyword token.
    pub keyword_anchor: SourceAnchor,
    /// Hook-name slot.
    pub name: HookNameSelection,
    /// Handler relation (shared handler contract: inline sub, resolved static
    /// coderef, or typed boundary).
    pub handler: FrameworkHandler,
}

impl HookDeclaration {
    /// Whether any payload member is a dynamic or bounded boundary.
    #[must_use]
    pub fn has_boundary(&self) -> bool {
        let name_boundary = match &self.name {
            HookNameSelection::Literal(name) => name.is_boundary(),
            HookNameSelection::Dynamic { .. } => true,
        };
        name_boundary || !self.handler.is_exact()
    }
}

/// Canonical framework hook fact: envelope plus hook payload and framework
/// identity.
///
/// Deserialization is checked: wire payloads are rebuilt through
/// [`HookFact::new`], so every constructor-side invariant (envelope kind,
/// coherent alias normalization, non-empty literal names) also holds for
/// decoded facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookFact {
    /// Canonical semantic envelope (kind forced to `Hook`).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Hook payload.
    pub hook: HookDeclaration,
}

#[derive(Deserialize)]
struct HookFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    hook: HookDeclaration,
}

impl<'de> Deserialize<'de> for HookFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = HookFactWire::deserialize(deserializer)?;
        Ok(HookFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.hook,
        ))
    }
}

impl HookFact {
    /// Construct a hook fact and canonicalize the payload.
    ///
    /// The envelope kind is forced to [`SemanticFactKind::Hook`]. Incoherent
    /// normalizations are repaired into explicit boundaries so no exact
    /// status can rest on a malformed payload (covers unchecked wire
    /// payloads): an alias without a canonical target or with an empty one
    /// becomes [`HookNameNormalization::Unresolved`], and an empty literal
    /// becomes an unresolved name.
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        mut hook: HookDeclaration,
    ) -> Self {
        envelope.kind = SemanticFactKind::Hook;
        if let HookNameSelection::Literal(name) = &mut hook.name {
            let incoherent_alias = match &name.normalization {
                HookNameNormalization::Alias { canonical } => canonical.trim().is_empty(),
                _ => false,
            };
            if name.literal.trim().is_empty() || incoherent_alias {
                name.normalization = HookNameNormalization::Unresolved {
                    reason: "empty or incoherent hook name stays an explicit boundary".to_string(),
                };
            }
        }
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            hook,
        }
    }

    /// Classify the complete hook fact for a provider decision.
    ///
    /// An exact envelope with a bounded payload member stays degraded: hooks
    /// with dynamic names, unresolved names, or bounded handlers are usable
    /// but never exact.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if self.hook.has_boundary() {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Domain separator keeping hook fact identities disjoint from route fact
/// identities minted over the same (file, declaration order, generation).
const HOOK_IDENTITY_DOMAIN: u64 = 0x600D_5100_0000_0001;

/// Deterministic hook fact identity for one (file, declaration, generation).
///
/// Identity derives from the owning file, the source declaration order, and
/// the minting generation under a hook-specific domain separator, so hook and
/// route facts of the same file/order/generation never collide while keeping
/// the route family's identity scheme (#8918) untouched.
#[must_use]
pub fn hook_fact_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let generation_digest = match generation {
        SourceGeneration::Known(value) => {
            value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3)
            })
        }
        SourceGeneration::Unknown => 0x1a2b_3c4d_5e6f_7081_u64,
    };
    let file = file_id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let index = u64::from(declaration_index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let fact = file ^ index ^ generation_digest ^ HOOK_IDENTITY_DOMAIN;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Domain separator keeping hook *handler-context* identities disjoint from
/// both hook fact identities and route handler-context identities minted over
/// the same (file, declaration order, generation).
const HOOK_HANDLER_CONTEXT_IDENTITY_DOMAIN: u64 = 0x600D_5100_4841_4E44;

/// Deterministic hook handler-context identity for one (file, declaration,
/// generation).
///
/// The entity is the owning hook's entity, so the context and its hook are
/// two facts about one entity; the fact identity uses its own domain
/// separator so it can never collide with the hook fact or with a route
/// handler context of the same file/order/generation.
#[must_use]
pub fn hook_handler_context_identity(
    file_id: crate::FileId,
    declaration_index: u32,
    generation: &SourceGeneration,
) -> (FactId, EntityId) {
    let (fact_id, hook_entity) = hook_fact_identity(file_id, declaration_index, generation);
    (FactId(fact_id.0 ^ HOOK_HANDLER_CONTEXT_IDENTITY_DOMAIN), hook_entity)
}

/// Build the canonical envelope for one minted hook fact.
///
/// Shared by framework adapters so every hook fact carries the same
/// producer/provenance/freshness contract as the route family: producer
/// `FrameworkAdapter`, AST-exact provenance, high confidence, fresh
/// generation, and invalidation dependencies over the owning source file plus
/// the activating framework module. The reason code and optional boundary
/// link reflect the payload's exactness.
#[allow(clippy::too_many_arguments)] // mirrors the envelope contract fields
#[must_use]
pub fn hook_envelope(
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
        SemanticFactKind::Hook,
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
            // the hook payload, so the link carries no foreign boundary id.
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
    use crate::handler::SubroutineTarget;
    use crate::{AnchorId, FileId, InvalidationDependency, SemanticFactStatus};
    use perl_test_must::must_some;

    fn anchor(start: u32, end: u32) -> SourceAnchor {
        SourceAnchor::new(Some(AnchorId(start as u64)), FileId(1), start, end)
    }

    fn alias_before_declaration(declaration_index: u32) -> HookDeclaration {
        HookDeclaration {
            declaration_index,
            keyword: "hook".to_string(),
            keyword_anchor: anchor(0, 4),
            name: HookNameSelection::Literal(HookName {
                literal: "before".to_string(),
                anchor: anchor(5, 13),
                normalization: HookNameNormalization::Alias {
                    canonical: "core.app.before_request".to_string(),
                },
            }),
            handler: FrameworkHandler::InlineSub { anchor: anchor(16, 32) },
        }
    }

    fn envelope_for(fact_id: FactId, entity_id: EntityId, exact: bool) -> SemanticFactEnvelope {
        hook_envelope(
            fact_id,
            entity_id,
            Some("App"),
            anchor(0, 32),
            &SourceGeneration::known("gen-1"),
            vec![InvalidationDependency::new("source:1", SourceGeneration::known("gen-1"))],
            if exact { None } else { Some(BoundaryKind::DynamicValue) },
            SemanticReasonCode::DynamicValue,
            exact,
        )
    }

    fn fact(declaration: HookDeclaration, exact: bool) -> HookFact {
        let (fact_id, entity_id) = hook_fact_identity(
            FileId(1),
            declaration.declaration_index,
            &SourceGeneration::known("gen-1"),
        );
        HookFact::new(
            envelope_for(fact_id, entity_id, exact),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            declaration,
        )
    }

    #[test]
    fn resolved_alias_hook_is_exact_and_forces_kind() {
        let hook_fact = fact(alias_before_declaration(0), true);
        assert_eq!(hook_fact.envelope.kind, SemanticFactKind::Hook);
        assert_eq!(hook_fact.status(), SemanticFactStatus::Exact);
        let name = must_some(match &hook_fact.hook.name {
            HookNameSelection::Literal(name) => Some(name),
            HookNameSelection::Dynamic { .. } => None,
        });
        assert_eq!(name.literal, "before");
        assert_eq!(name.canonical(), Some("core.app.before_request"));
    }

    #[test]
    fn bounded_payload_members_degrade_the_fact() {
        let mutations: Vec<HookDeclaration> = vec![
            // Unresolved hook name: possible plugin/runtime alias.
            HookDeclaration {
                name: HookNameSelection::Literal(HookName {
                    literal: "plugin_only".to_string(),
                    anchor: anchor(5, 17),
                    normalization: HookNameNormalization::Unresolved {
                        reason: "not in the reviewed contract".to_string(),
                    },
                }),
                ..alias_before_declaration(0)
            },
            // Computed name operand.
            HookDeclaration {
                name: HookNameSelection::Dynamic {
                    reason: "computed hook name".to_string(),
                    anchor: anchor(5, 11),
                },
                ..alias_before_declaration(0)
            },
            // Bounded handler.
            HookDeclaration {
                handler: FrameworkHandler::Bounded {
                    boundary: crate::handler::FrameworkHandlerBoundary::Computed,
                    anchor: Some(anchor(16, 24)),
                    reason: "computed handler".to_string(),
                },
                ..alias_before_declaration(0)
            },
        ];
        for mutation in mutations {
            let hook_fact = fact(mutation, false);
            assert!(hook_fact.hook.has_boundary(), "every mutation must be a boundary");
            assert_eq!(hook_fact.status(), SemanticFactStatus::Degraded);
            assert!(hook_fact.envelope.boundary.is_some());
        }
    }

    #[test]
    fn resolved_static_coderef_handler_keeps_hook_exact() {
        let declaration = HookDeclaration {
            handler: FrameworkHandler::StaticCoderef {
                name: "on_request".to_string(),
                anchor: anchor(16, 27),
                target: SubroutineTarget {
                    name: "on_request".to_string(),
                    package: "App".to_string(),
                    name_anchor: anchor(40, 50),
                    declaration_anchor: anchor(35, 80),
                    body_anchor: Some(anchor(51, 80)),
                },
            },
            ..alias_before_declaration(0)
        };
        let hook_fact = fact(declaration, true);
        assert_eq!(hook_fact.status(), SemanticFactStatus::Exact);
    }

    #[test]
    fn constructor_repairs_incoherent_wire_payloads() {
        let mut declaration = alias_before_declaration(0);
        if let HookNameSelection::Literal(name) = &mut declaration.name {
            name.normalization = HookNameNormalization::Alias { canonical: String::new() };
        }
        let hook_fact = fact(declaration, true);
        let name = must_some(match &hook_fact.hook.name {
            HookNameSelection::Literal(name) => Some(name),
            HookNameSelection::Dynamic { .. } => None,
        });
        assert!(name.is_boundary(), "an empty alias target cannot claim canonical identity");
        assert_eq!(name.canonical(), None);
        assert_eq!(hook_fact.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn hook_identities_stay_disjoint_from_route_identities() {
        let generation = SourceGeneration::known("gen-1");
        let (hook_fact_id, hook_entity_id) = hook_fact_identity(FileId(1), 0, &generation);
        let (route_fact_id, route_entity_id) =
            crate::route::route_fact_identity(FileId(1), 0, &generation);
        assert_ne!(hook_fact_id, route_fact_id, "hook and route ids never collide");
        assert_ne!(hook_entity_id, route_entity_id);
        // Same determinism/scoping contract as the route family.
        assert_eq!(hook_fact_id, hook_fact_identity(FileId(1), 0, &generation).0);
        assert_ne!(
            hook_fact_id,
            hook_fact_identity(FileId(1), 1, &generation).0,
            "same-name hooks stay distinct by order"
        );
        assert_ne!(
            hook_fact_id,
            hook_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-2")).0,
            "generation-scoped"
        );
        assert_ne!(hook_fact_id, hook_fact_identity(FileId(2), 0, &generation).0, "file-scoped");
    }

    #[test]
    fn hook_fact_roundtrips_through_json() -> Result<(), serde_json::Error> {
        let hook_fact = fact(alias_before_declaration(0), true);
        let serialized = serde_json::to_string(&hook_fact)?;
        let decoded: HookFact = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, hook_fact);
        assert_eq!(decoded.status(), SemanticFactStatus::Exact);
        Ok(())
    }

    #[test]
    fn deserialization_reapplies_constructor_invariants() -> Result<(), serde_json::Error> {
        let hook_fact = fact(alias_before_declaration(0), true);
        let mut value = serde_json::to_value(&hook_fact)?;
        value["envelope"]["kind"] = serde_json::json!("Declaration");
        let decoded: HookFact = serde_json::from_value(value)?;
        assert_eq!(decoded.envelope.kind, SemanticFactKind::Hook, "kind is forced");
        Ok(())
    }
}
