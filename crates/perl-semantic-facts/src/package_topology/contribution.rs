//! Package declaration contribution records.

use super::identity::{
    PackageDeclarationAnchor, PackageDeclarationForm, PackageDeclarationId, PackageEditAuthority,
    PackageId, PackageNameAnchor, PackageSourceTier,
};
use super::{
    Fingerprint, PACKAGE_TOPOLOGY_SCHEMA_TAG, PackageTopologyContractError, declaration_kind_tag,
};
use crate::semantic_identity::{SemanticDependencyIdentity, SemanticSubjectStatus};
use crate::{EntityId, EntityKind, SemanticConfidence, SemanticProducer, SemanticProvenance};

use serde::{Deserialize, Serialize};

/// One source declaration contribution of a package, class, or role.
///
/// A package node is the later join of every accepted contribution with the
/// same [`PackageId`]. This record does not select a sole owner.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageDeclarationContribution {
    package_id: PackageId,
    declaration_id: PackageDeclarationId,
    entity_id: EntityId,
    kind: EntityKind,
    declaration_anchor: Option<PackageDeclarationAnchor>,
    name_anchor: Option<PackageNameAnchor>,
    source_tier: PackageSourceTier,
    edit_authority: PackageEditAuthority,
    producer: SemanticProducer,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    completeness: SemanticSubjectStatus,
    limitations: Vec<String>,
    dependencies: Vec<SemanticDependencyIdentity>,
}

impl PackageDeclarationContribution {
    /// Construct a validated declaration contribution.
    ///
    /// # Errors
    /// Returns a contract error when identity slots are empty, a zero entity
    /// identity is used, kind/form disagree, a name spelling disagrees with
    /// the package identity, complete status lacks a nonempty in-declaration
    /// name span, a nested dependency has a blank target, or
    /// source-tier/edit-authority contradict.
    #[expect(
        clippy::too_many_arguments,
        reason = "a declaration contribution is one validated record; splitting would duplicate the identity law"
    )]
    pub fn new(
        package_id: PackageId,
        declaration_id: PackageDeclarationId,
        entity_id: EntityId,
        kind: EntityKind,
        declaration_anchor: Option<PackageDeclarationAnchor>,
        name_anchor: Option<PackageNameAnchor>,
        source_tier: PackageSourceTier,
        edit_authority: PackageEditAuthority,
        producer: SemanticProducer,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        completeness: SemanticSubjectStatus,
        limitations: Vec<String>,
        dependencies: Vec<SemanticDependencyIdentity>,
    ) -> Result<Self, PackageTopologyContractError> {
        let mut limitations = limitations;
        limitations.sort();
        let mut dependencies = dependencies;
        dependencies.sort_by_key(|dependency| {
            (dependency.kind().tag(), dependency.target_fingerprint().to_owned())
        });
        let contribution = Self {
            package_id,
            declaration_id,
            entity_id,
            kind,
            declaration_anchor,
            name_anchor,
            source_tier,
            edit_authority,
            producer,
            provenance,
            confidence,
            completeness,
            limitations,
            dependencies,
        };
        contribution.validate()?;
        Ok(contribution)
    }

    /// Validate a contribution after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        self.package_id.validate()?;
        self.declaration_id.validate()?;
        if self.entity_id.0 == 0 {
            return Err(PackageTopologyContractError::SentinelEntityId);
        }
        let kind_tag = declaration_kind_tag(self.kind)?;
        if kind_tag != self.declaration_id.declaration_form().entity_kind_tag() {
            return Err(PackageTopologyContractError::KindFormMismatch {
                kind: kind_tag,
                form: self.declaration_id.declaration_form().tag(),
            });
        }
        if let Some(anchor) = &self.declaration_anchor {
            anchor.validate()?;
        }
        if let Some(anchor) = &self.name_anchor {
            anchor.validate()?;
            if anchor.name_spelling() != self.package_id.qualified_name() {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "name anchor spelling must match the package identity qualified name",
                ));
            }
            if let Some(declared) = self.declaration_id.name_digest()
                && declared != anchor.name_digest()
            {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "declaration identity name digest must match the name anchor",
                ));
            }
        } else if self.declaration_id.name_digest().is_some() {
            return Err(PackageTopologyContractError::ContradictoryStatus(
                "a name digest in the declaration identity requires a name anchor",
            ));
        }
        if self.completeness.is_complete() {
            let Some(name_span) = self.name_anchor.as_ref().and_then(PackageNameAnchor::span)
            else {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "a complete declaration requires an exact name span",
                ));
            };
            let Some(declaration_span) =
                self.declaration_anchor.as_ref().and_then(PackageDeclarationAnchor::span)
            else {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "a complete declaration requires an exact declaration span",
                ));
            };
            if name_span.start_byte == name_span.end_byte
                || declaration_span.start_byte == declaration_span.end_byte
            {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "a complete declaration requires nonempty name and declaration spans",
                ));
            }
            if name_span.file_id != declaration_span.file_id
                || name_span.start_byte < declaration_span.start_byte
                || name_span.end_byte > declaration_span.end_byte
            {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "a complete declaration's name span must occupy bytes inside the declaration span",
                ));
            }
            if !self.limitations.is_empty() {
                return Err(PackageTopologyContractError::ContradictoryStatus(
                    "a complete declaration must record no limitations",
                ));
            }
        }
        if matches!(self.edit_authority, PackageEditAuthority::Editable)
            && !self.source_tier.allows_editable_authority()
        {
            return Err(PackageTopologyContractError::ContradictoryStatus(
                "editable authority is only valid for workspace source",
            ));
        }
        if self.limitations.iter().any(|item| item.trim().is_empty()) {
            return Err(PackageTopologyContractError::EmptyIdentityField(
                "PackageDeclarationContribution.limitations",
            ));
        }
        if self.limitations.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(PackageTopologyContractError::ContradictoryStatus(
                "limitations must be distinct",
            ));
        }
        let mut expected_limitations = self.limitations.clone();
        expected_limitations.sort();
        if expected_limitations != self.limitations {
            return Err(PackageTopologyContractError::ContradictoryStatus(
                "limitations must be sorted",
            ));
        }
        let mut expected_dependencies = self.dependencies.clone();
        expected_dependencies.sort_by_key(|dependency| {
            (dependency.kind().tag(), dependency.target_fingerprint().to_owned())
        });
        if expected_dependencies != self.dependencies {
            return Err(PackageTopologyContractError::ContradictoryStatus(
                "dependencies must be sorted",
            ));
        }
        if expected_dependencies.windows(2).any(|pair| {
            pair[0].kind() == pair[1].kind()
                && pair[0].target_fingerprint() == pair[1].target_fingerprint()
        }) {
            return Err(PackageTopologyContractError::ContradictoryStatus(
                "dependencies must be distinct",
            ));
        }
        if self
            .dependencies
            .iter()
            .any(|dependency| dependency.target_fingerprint().trim().is_empty())
        {
            return Err(PackageTopologyContractError::EmptyIdentityField(
                "PackageDeclarationContribution.dependencies.target_fingerprint",
            ));
        }
        Ok(())
    }

    /// Logical package this contribution declares.
    #[must_use]
    pub fn package_id(&self) -> &PackageId {
        &self.package_id
    }

    /// Source-local declaration identity.
    #[must_use]
    pub fn declaration_id(&self) -> &PackageDeclarationId {
        &self.declaration_id
    }

    /// Entity identity of this declaration. Never the zero sentinel.
    #[must_use]
    pub fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    /// Package, class, or role kind.
    #[must_use]
    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    /// Declaration form written at the site.
    #[must_use]
    pub fn declaration_form(&self) -> PackageDeclarationForm {
        self.declaration_id.declaration_form()
    }

    /// Logical source instance owning this contribution.
    #[must_use]
    pub fn logical_source_id(&self) -> &str {
        self.declaration_id.logical_source_id()
    }

    /// Content revision of the logical source.
    #[must_use]
    pub fn content_revision(&self) -> &str {
        self.declaration_id.content_revision()
    }

    /// Accepted source generation of this contribution.
    #[must_use]
    pub fn source_generation(&self) -> &str {
        self.declaration_id.source_generation()
    }

    /// Full declaration-region anchor, when recovered.
    #[must_use]
    pub fn declaration_anchor(&self) -> Option<&PackageDeclarationAnchor> {
        self.declaration_anchor.as_ref()
    }

    /// Exact name-token anchor, when recovered.
    #[must_use]
    pub fn name_anchor(&self) -> Option<&PackageNameAnchor> {
        self.name_anchor.as_ref()
    }

    /// Source tier of this contribution.
    #[must_use]
    pub fn source_tier(&self) -> PackageSourceTier {
        self.source_tier
    }

    /// Edit authority of this contribution.
    #[must_use]
    pub fn edit_authority(&self) -> PackageEditAuthority {
        self.edit_authority
    }

    /// Producer that supplied the contribution.
    #[must_use]
    pub fn producer(&self) -> SemanticProducer {
        self.producer
    }

    /// Provenance of the contribution.
    #[must_use]
    pub fn provenance(&self) -> SemanticProvenance {
        self.provenance
    }

    /// Confidence ceiling of the contribution.
    #[must_use]
    pub fn confidence(&self) -> SemanticConfidence {
        self.confidence
    }

    /// Completeness of the contribution.
    #[must_use]
    pub fn completeness(&self) -> SemanticSubjectStatus {
        self.completeness
    }

    /// Recorded limitations.
    #[must_use]
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Dependency identities folded into the fingerprint.
    #[must_use]
    pub fn dependencies(&self) -> &[SemanticDependencyIdentity] {
        &self.dependencies
    }

    /// Deterministic fingerprint of this contribution.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.fold(Fingerprint::new(PACKAGE_TOPOLOGY_SCHEMA_TAG)).finish()
    }

    fn fold(&self, fp: Fingerprint) -> Fingerprint {
        let fp = self
            .package_id
            .fold(fp)
            .field("entity", &self.entity_id.0.to_string())
            .discriminant("kind", declaration_kind_tag(self.kind).unwrap_or("invalid"));
        let fp = self.declaration_id.fold(fp);
        let fp = match &self.declaration_anchor {
            Some(anchor) => anchor.fold_evidence(fp.discriminant("declaration-anchor", "some")),
            None => fp.discriminant("declaration-anchor", "none"),
        };
        let fp = match &self.name_anchor {
            Some(anchor) => anchor.fold_evidence(fp.discriminant("name-anchor", "some")),
            None => fp.discriminant("name-anchor", "none"),
        };
        let fp = self.edit_authority.fold(self.source_tier.fold(fp));
        let fp = fp
            .discriminant("producer", producer_tag(self.producer))
            .discriminant("provenance", provenance_tag(self.provenance))
            .discriminant("confidence", confidence_tag(self.confidence))
            .discriminant("completeness", self.completeness.tag())
            .field("limitation-count", &self.limitations.len().to_string());
        let fp = self.limitations.iter().enumerate().fold(fp, |fp, (index, limitation)| {
            fp.field(&format!("limitation-{index}"), limitation)
        });
        self.dependencies.iter().enumerate().fold(fp, |fp, (index, dependency)| {
            fp.field(&format!("dependency-{index}-kind"), dependency.kind().tag())
                .field(&format!("dependency-{index}-target"), dependency.target_fingerprint())
        })
    }
}

const fn producer_tag(producer: SemanticProducer) -> &'static str {
    match producer {
        SemanticProducer::Parser => "parser",
        SemanticProducer::Hir => "hir",
        SemanticProducer::PirA => "pir-a",
        SemanticProducer::SemanticAnalyzer => "semantic-analyzer",
        SemanticProducer::WorkspaceIndex => "workspace-index",
        SemanticProducer::FrameworkAdapter => "framework-adapter",
        SemanticProducer::Unknown => "unknown",
    }
}

const fn provenance_tag(provenance: SemanticProvenance) -> &'static str {
    match provenance {
        SemanticProvenance::Known(inner) => match inner {
            crate::Provenance::ExactAst => "exact-ast",
            crate::Provenance::DesugaredAst => "desugared-ast",
            crate::Provenance::SemanticAnalyzer => "semantic-analyzer",
            crate::Provenance::FrameworkSynthesis => "framework-synthesis",
            crate::Provenance::ImportExportInference => "import-export-inference",
            crate::Provenance::PragmaInference => "pragma-inference",
            crate::Provenance::NameHeuristic => "name-heuristic",
            crate::Provenance::SearchFallback => "search-fallback",
            crate::Provenance::DynamicBoundary => "dynamic-boundary",
            crate::Provenance::LiteralRequireImport => "literal-require-import",
        },
        SemanticProvenance::Unknown => "unknown",
    }
}

const fn confidence_tag(confidence: SemanticConfidence) -> &'static str {
    match confidence {
        SemanticConfidence::Known(crate::Confidence::High) => "high",
        SemanticConfidence::Known(crate::Confidence::Medium) => "medium",
        SemanticConfidence::Known(crate::Confidence::Low) => "low",
        SemanticConfidence::Unknown => "unknown",
    }
}
