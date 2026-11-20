//! Canonical framework template-target fact family (#8937).
//!
//! [`TemplateTargetFact`] is the transport-neutral canonical record for
//! statically observed framework template-target references (Dancer2
//! `template 'name'` style DSL calls). It follows the [`crate::HookFact`]
//! precedent: a payload fact wrapping the canonical [`SemanticFactEnvelope`]
//! with the envelope kind forced to [`SemanticFactKind::TemplateTarget`].
//!
//! The family is framework-neutral. Framework-specific minting (activation
//! gating, views-root resolution) lives in concrete adapters such as
//! `framework_adapters::dancer2_templates`; this module only defines what a
//! template-target reference must preserve: identity by source order, the
//! logical template name with its exact source range, and a separate
//! filesystem-target slot that stays unresolved unless a canonical
//! configuration authority proves one exact candidate.
//!
//! Exactness contract:
//!
//! - a fact is [`SemanticFactStatus::Exact`] when the envelope is exact **and**
//!   the name operand is a static literal. An unresolved filesystem target
//!   does **not** degrade the logical reference — the issue keeps the logical
//!   name exact while filesystem resolution stays bounded;
//! - a computed/interpolated/escaped name operand keeps the fact usable but
//!   degraded — a typed dynamic boundary, never a guessed name;
//! - two calls of the same logical name remain distinct template entities
//!   through `declaration_index` (source identity/order).

use crate::framework::AdapterId;
use crate::{
    BoundaryDisposition, BoundaryKind, Confidence, EntityId, FactId, Provenance,
    SemanticConfidence, SemanticFactEnvelope, SemanticFactKind, SemanticFactStatus,
    SemanticFreshness, SemanticProducer, SemanticProvenance, SemanticReasonCode, SourceAnchor,
    SourceGeneration,
};
use serde::{Deserialize, Serialize};

/// One logical template name with its exact source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateName {
    /// Normalized logical template name (unquoted source spelling).
    pub logical: String,
    /// Source range of the name operand (exact tokens, including quotes).
    pub anchor: SourceAnchor,
}

/// Template-name slot of one template-target call.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateNameSelection {
    /// Static literal template name.
    Literal(TemplateName),
    /// Computed, interpolated, or escaped name operand — an explicit boundary.
    Dynamic {
        /// Bounded explanation.
        reason: String,
        /// Source range of the dynamic name operand.
        anchor: SourceAnchor,
    },
}

impl TemplateNameSelection {
    /// Whether the name slot is a typed boundary.
    #[must_use]
    pub fn is_boundary(&self) -> bool {
        matches!(self, Self::Dynamic { .. })
    }

    /// Logical name when the operand is a static literal.
    #[must_use]
    pub fn logical(&self) -> Option<&str> {
        match self {
            Self::Literal(name) => Some(name.logical.as_str()),
            Self::Dynamic { .. } => None,
        }
    }
}

/// Filesystem resolution of one logical template name.
///
/// Kept as a distinct slot so the logical reference can stay exact while
/// filesystem identity remains bounded. This slice never invents a views-root
/// path: canonical project configuration (#7900) is the only authority that
/// may later prove one exact candidate.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateFilesystemTarget {
    /// No exact views root / naming rule is proven.
    Unresolved {
        /// Bounded explanation.
        reason: String,
    },
}

impl TemplateFilesystemTarget {
    /// Default unresolved slot for this slice: no second config parser.
    #[must_use]
    pub fn views_root_unresolved() -> Self {
        Self::Unresolved { reason: VIEWS_ROOT_UNRESOLVED_REASON.to_string() }
    }
}

/// Stable reason for the first-slice unresolved filesystem slot.
pub const VIEWS_ROOT_UNRESOLVED_REASON: &str = "filesystem template resolution requires one exact \
     views root from canonical project configuration (#7900); this slice retains the logical name \
     only";

/// Canonical payload of one template-target call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateReference {
    /// Source-order identity within the owning file. Distinct calls of the
    /// same logical name stay distinct template entities.
    pub declaration_index: u32,
    /// Template keyword token (`template`).
    pub keyword: String,
    /// Source range of the template keyword token.
    pub keyword_anchor: SourceAnchor,
    /// Template-name slot.
    pub name: TemplateNameSelection,
    /// Filesystem-target slot, independent of logical-name exactness.
    pub filesystem: TemplateFilesystemTarget,
}

impl TemplateReference {
    /// Whether the *name* operand is a dynamic or bounded boundary.
    ///
    /// An unresolved filesystem target is a separate slot and does not count
    /// as a name-level boundary.
    #[must_use]
    pub fn has_name_boundary(&self) -> bool {
        self.name.is_boundary()
    }
}

/// Canonical framework template-target fact: envelope plus template payload
/// and framework identity.
///
/// Deserialization is checked: wire payloads are rebuilt through
/// [`TemplateTargetFact::new`], so every constructor-side invariant
/// (envelope kind, non-empty logical names) also holds for decoded facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateTargetFact {
    /// Canonical semantic envelope (kind forced to `TemplateTarget`).
    pub envelope: SemanticFactEnvelope,
    /// Framework name (e.g. `Dancer2`).
    pub framework_name: String,
    /// Producing adapter identity.
    pub adapter_id: AdapterId,
    /// Observed framework version the fact was minted against.
    pub framework_version: String,
    /// Owning application identity from the activating import.
    pub application_name: String,
    /// Template-target payload.
    pub template: TemplateReference,
}

#[derive(Deserialize)]
struct TemplateTargetFactWire {
    envelope: SemanticFactEnvelope,
    framework_name: String,
    adapter_id: AdapterId,
    framework_version: String,
    application_name: String,
    template: TemplateReference,
}

impl<'de> Deserialize<'de> for TemplateTargetFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TemplateTargetFactWire::deserialize(deserializer)?;
        Ok(TemplateTargetFact::new(
            wire.envelope,
            wire.framework_name,
            wire.adapter_id,
            wire.framework_version,
            wire.application_name,
            wire.template,
        ))
    }
}

impl TemplateTargetFact {
    /// Construct a template-target fact and canonicalize the payload.
    ///
    /// The envelope kind is forced to [`SemanticFactKind::TemplateTarget`].
    /// An empty logical name is repaired into a dynamic name boundary so no
    /// exact status can rest on a malformed payload.
    #[must_use]
    pub fn new(
        mut envelope: SemanticFactEnvelope,
        framework_name: impl Into<String>,
        adapter_id: AdapterId,
        framework_version: impl Into<String>,
        application_name: impl Into<String>,
        mut template: TemplateReference,
    ) -> Self {
        envelope.kind = SemanticFactKind::TemplateTarget;
        if let TemplateNameSelection::Literal(name) = &template.name
            && name.logical.trim().is_empty()
        {
            let anchor = name.anchor;
            template.name = TemplateNameSelection::Dynamic {
                reason: "empty template name stays an explicit boundary".to_string(),
                anchor,
            };
        }
        Self {
            envelope,
            framework_name: framework_name.into(),
            adapter_id,
            framework_version: framework_version.into(),
            application_name: application_name.into(),
            template,
        }
    }

    /// Classify the complete template-target fact for a provider decision.
    ///
    /// An exact envelope with a dynamic name stays degraded. An unresolved
    /// filesystem slot does not degrade a literal logical reference.
    #[must_use]
    pub fn status(&self) -> SemanticFactStatus {
        let envelope_status = self.envelope.status();
        if !matches!(envelope_status, SemanticFactStatus::Exact) {
            return envelope_status;
        }
        if self.template.has_name_boundary() {
            SemanticFactStatus::Degraded
        } else {
            SemanticFactStatus::Exact
        }
    }
}

/// Domain separator keeping template-target identities disjoint from route
/// and hook identities minted over the same (file, declaration, generation).
const TEMPLATE_IDENTITY_DOMAIN: u64 = 0x5445_4D50_4C54_3100;

/// Deterministic template-target fact identity for one
/// (file, declaration, generation).
#[must_use]
pub fn template_target_fact_identity(
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
    let fact = file ^ index ^ generation_digest ^ TEMPLATE_IDENTITY_DOMAIN;
    (FactId(fact), EntityId(fact.wrapping_add(1)))
}

/// Build the canonical envelope for one minted template-target fact.
#[allow(clippy::too_many_arguments)] // mirrors the envelope contract fields
#[must_use]
pub fn template_target_envelope(
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
        SemanticFactKind::TemplateTarget,
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

    fn literal_index(declaration_index: u32) -> TemplateReference {
        TemplateReference {
            declaration_index,
            keyword: "template".to_string(),
            keyword_anchor: anchor(0, 8),
            name: TemplateNameSelection::Literal(TemplateName {
                logical: "index".to_string(),
                anchor: anchor(9, 16),
            }),
            filesystem: TemplateFilesystemTarget::views_root_unresolved(),
        }
    }

    fn envelope_for(fact_id: FactId, entity_id: EntityId, exact: bool) -> SemanticFactEnvelope {
        template_target_envelope(
            fact_id,
            entity_id,
            Some("App"),
            anchor(0, 16),
            &SourceGeneration::known("gen-1"),
            vec![InvalidationDependency::new("source:1", SourceGeneration::known("gen-1"))],
            if exact { None } else { Some(BoundaryKind::DynamicValue) },
            SemanticReasonCode::DynamicValue,
            exact,
        )
    }

    fn fact(reference: TemplateReference, exact: bool) -> TemplateTargetFact {
        let (fact_id, entity_id) = template_target_fact_identity(
            FileId(1),
            reference.declaration_index,
            &SourceGeneration::known("gen-1"),
        );
        TemplateTargetFact::new(
            envelope_for(fact_id, entity_id, exact),
            "Dancer2",
            AdapterId(1),
            "1.1.1",
            "App",
            reference,
        )
    }

    #[test]
    fn literal_name_is_exact_even_when_filesystem_is_unresolved() {
        let target = fact(literal_index(0), true);
        assert_eq!(target.envelope.kind, SemanticFactKind::TemplateTarget);
        assert_eq!(target.status(), SemanticFactStatus::Exact);
        assert!(matches!(target.template.filesystem, TemplateFilesystemTarget::Unresolved { .. }));
        assert_eq!(target.template.name.logical(), Some("index"));
    }

    #[test]
    fn dynamic_name_degrades_the_fact() {
        let reference = TemplateReference {
            name: TemplateNameSelection::Dynamic {
                reason: "computed template name".to_string(),
                anchor: anchor(9, 14),
            },
            ..literal_index(0)
        };
        let target = fact(reference, false);
        assert!(target.template.has_name_boundary());
        assert_eq!(target.status(), SemanticFactStatus::Degraded);
        assert!(target.envelope.boundary.is_some());
    }

    #[test]
    fn constructor_repairs_empty_logical_names() {
        let mut reference = literal_index(0);
        if let TemplateNameSelection::Literal(name) = &mut reference.name {
            name.logical = "   ".to_string();
        }
        let target = fact(reference, true);
        assert!(target.template.has_name_boundary());
        assert_eq!(target.status(), SemanticFactStatus::Degraded);
    }

    #[test]
    fn identities_stay_disjoint_from_hook_and_route() {
        let generation = SourceGeneration::known("gen-1");
        let (template_fact_id, template_entity) =
            template_target_fact_identity(FileId(1), 0, &generation);
        let (hook_fact_id, hook_entity) =
            crate::hook::hook_fact_identity(FileId(1), 0, &generation);
        let (route_fact_id, route_entity) =
            crate::route::route_fact_identity(FileId(1), 0, &generation);
        assert_ne!(template_fact_id, hook_fact_id);
        assert_ne!(template_fact_id, route_fact_id);
        assert_ne!(template_entity, hook_entity);
        assert_ne!(template_entity, route_entity);
        assert_eq!(template_fact_id, template_target_fact_identity(FileId(1), 0, &generation).0);
        assert_ne!(template_fact_id, template_target_fact_identity(FileId(1), 1, &generation).0);
        assert_ne!(
            template_fact_id,
            template_target_fact_identity(FileId(1), 0, &SourceGeneration::known("gen-2")).0
        );
        assert_ne!(template_fact_id, template_target_fact_identity(FileId(2), 0, &generation).0);
    }

    #[test]
    fn fact_roundtrips_through_json() -> Result<(), serde_json::Error> {
        let target = fact(literal_index(0), true);
        let serialized = serde_json::to_string(&target)?;
        let decoded: TemplateTargetFact = serde_json::from_str(&serialized)?;
        assert_eq!(decoded, target);
        assert_eq!(decoded.status(), SemanticFactStatus::Exact);
        Ok(())
    }

    #[test]
    fn deserialization_reapplies_constructor_invariants() -> Result<(), serde_json::Error> {
        let target = fact(literal_index(0), true);
        let mut value = serde_json::to_value(&target)?;
        value["envelope"]["kind"] = serde_json::json!("Declaration");
        let decoded: TemplateTargetFact = serde_json::from_value(value)?;
        assert_eq!(decoded.envelope.kind, SemanticFactKind::TemplateTarget, "kind is forced");
        let logical = must_some(decoded.template.name.logical());
        assert_eq!(logical, "index");
        Ok(())
    }
}
