//! Contribution ownership, fact families, dependencies, and shared status.

use super::{
    SemanticIdentityContractError, SemanticIdentityFingerprint, SemanticIdentitySchema,
    SemanticSourceOrderIdentity, SemanticSubjectGeneration,
};

use serde::{Deserialize, Serialize};

/// Fact family a semantic contribution belongs to.
///
/// Families keep scope-local facts, source-order effects, class/generated
/// facts, and boundary/limitation facts distinct so later invalidation
/// (#12122) can key off the family rather than off byte ranges.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticFactFamily {
    /// Scope-local declaration.
    ScopeLocalDeclaration,
    /// Scope-local reference.
    ScopeLocalReference,
    /// Scope-local semantic token.
    ScopeLocalToken,
    /// Hover source fact owned by a scope.
    HoverFact,
    /// Package transition fact.
    PackageFact,
    /// Export metadata fact.
    ExportFact,
    /// Pragma effect fact.
    PragmaFact,
    /// Import fact.
    ImportFact,
    /// Prototype fact.
    PrototypeFact,
    /// Feature-flag fact.
    FeatureFact,
    /// Class/inheritance fact.
    ClassInheritanceFact,
    /// Generated-member fact.
    GeneratedMemberFact,
    /// Data-section fact.
    DataSectionFact,
    /// Source-boundary fact.
    SourceBoundaryFact,
    /// Dynamic-construct limitation.
    DynamicLimitation,
    /// Recovery limitation.
    RecoveryLimitation,
}

impl SemanticFactFamily {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::ScopeLocalDeclaration => "scope-local-declaration",
            Self::ScopeLocalReference => "scope-local-reference",
            Self::ScopeLocalToken => "scope-local-token",
            Self::HoverFact => "hover-fact",
            Self::PackageFact => "package-fact",
            Self::ExportFact => "export-fact",
            Self::PragmaFact => "pragma-fact",
            Self::ImportFact => "import-fact",
            Self::PrototypeFact => "prototype-fact",
            Self::FeatureFact => "feature-fact",
            Self::ClassInheritanceFact => "class-inheritance-fact",
            Self::GeneratedMemberFact => "generated-member-fact",
            Self::DataSectionFact => "data-section-fact",
            Self::SourceBoundaryFact => "source-boundary-fact",
            Self::DynamicLimitation => "dynamic-limitation",
            Self::RecoveryLimitation => "recovery-limitation",
        }
    }

    /// Whether this family is naturally owned by a lexical scope.
    #[must_use]
    pub fn is_scope_local(self) -> bool {
        matches!(
            self,
            Self::ScopeLocalDeclaration
                | Self::ScopeLocalReference
                | Self::ScopeLocalToken
                | Self::HoverFact
        )
    }
}

/// Typed owner disposition of one semantic contribution.
///
/// Every contribution has exactly one owner kind; package transitions,
/// imports, prototypes, class facts, and following-source context get their
/// own owner kinds instead of being forced into a lexical-scope bucket.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticOwnershipDisposition {
    /// Owned by one semantic scope.
    ScopeOwned {
        /// Logical fingerprint of the owning scope identity.
        scope_fingerprint: String,
    },
    /// Owned by the file/global subject.
    FileGlobalOwned,
    /// Owned by a source-order context.
    SourceOrderContextOwned {
        /// The owning source-order context identity.
        context: SemanticSourceOrderIdentity,
    },
    /// Owned by an external canonical producer reference.
    ExternalCanonicalProducer {
        /// Durable producer identifier (e.g. parser boundary authority).
        producer_id: String,
        /// Producer-local subject digest.
        subject_digest: String,
    },
    /// Explicit compatibility projection with a recorded exit.
    CompatibilityProjection {
        /// The compatibility rule/exit reference.
        exit_reference: String,
    },
    /// Unsupported or not proven; carries no incremental identity.
    UnsupportedNotProven {
        /// Typed reason this ownership cannot be established.
        reason: String,
    },
}

impl SemanticOwnershipDisposition {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            Self::ScopeOwned { .. } => "scope-owned",
            Self::FileGlobalOwned => "file-global-owned",
            Self::SourceOrderContextOwned { .. } => "source-order-context-owned",
            Self::ExternalCanonicalProducer { .. } => "external-canonical-producer",
            Self::CompatibilityProjection { .. } => "compatibility-projection",
            Self::UnsupportedNotProven { .. } => "unsupported-not-proven",
        }
    }

    /// Mix the disposition payload as independently framed, labeled fields.
    ///
    /// Payload components are never flattened with separators, so a component
    /// containing any separator-like content cannot shift across a field
    /// boundary.
    fn mix_into(&self, fp: SemanticIdentityFingerprint) -> SemanticIdentityFingerprint {
        match self {
            Self::ScopeOwned { scope_fingerprint } => {
                fp.field("owner-scope-fingerprint", scope_fingerprint)
            }
            Self::FileGlobalOwned => fp,
            Self::SourceOrderContextOwned { context } => fp
                .field("context-ordinal", &context.context_ordinal().to_string())
                .field("context-digest", context.context_digest()),
            Self::ExternalCanonicalProducer { producer_id, subject_digest } => fp
                .field("producer-id", producer_id)
                .field("producer-subject-digest", subject_digest),
            Self::CompatibilityProjection { exit_reference } => {
                fp.field("exit-reference", exit_reference)
            }
            Self::UnsupportedNotProven { reason } => fp.field("reason", reason),
        }
    }
}

/// Shared completeness/terminal status vocabulary.
///
/// `complete` means denominator-sufficient and exact. Producer identity never
/// upgrades a status, and empty collections never determine completeness.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticSubjectStatus {
    /// Complete and denominator-sufficient.
    Complete,
    /// Partial: recovered source regions participate.
    PartialRecovered,
    /// Partial: dynamic constructs bound the exact claim.
    PartialDynamic,
    /// Unsupported for this subject/profile.
    Unsupported,
    /// Unavailable (absent evidence, distinct from not proven).
    Unavailable,
    /// Stale relative to the accepted generation.
    Stale,
    /// Cancelled before completion.
    Cancelled,
    /// The measuring/validating instrument failed.
    InstrumentFailure,
    /// Evidence insufficient to prove any typed claim.
    NotProven,
}

impl SemanticSubjectStatus {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::PartialRecovered => "partial-recovered",
            Self::PartialDynamic => "partial-dynamic",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Stale => "stale",
            Self::Cancelled => "cancelled",
            Self::InstrumentFailure => "instrument-failure",
            Self::NotProven => "not-proven",
        }
    }

    /// Whether this status is exact-complete.
    #[must_use]
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// Logical key of the declaration owning a fact or scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticDeclarationKey {
    declaration_form: String,
    declared_name: String,
    declaration_digest: String,
}

impl SemanticDeclarationKey {
    /// Construct a declaration key from its form, name, and canonical digest.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when any
    /// component is empty.
    pub fn new(
        declaration_form: impl Into<String>,
        declared_name: impl Into<String>,
        declaration_digest: impl Into<String>,
    ) -> Result<Self, SemanticIdentityContractError> {
        let declaration_form = declaration_form.into();
        let declared_name = declared_name.into();
        let declaration_digest = declaration_digest.into();
        if declaration_form.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticDeclarationKey.declaration_form",
            ));
        }
        if declared_name.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticDeclarationKey.declared_name",
            ));
        }
        if declaration_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticDeclarationKey.declaration_digest",
            ));
        }
        Ok(Self { declaration_form, declared_name, declaration_digest })
    }

    /// Declaration form tag (e.g. `sub`, `method`, `package`).
    #[must_use]
    pub fn declaration_form(&self) -> &str {
        &self.declaration_form
    }

    /// Declared (possibly anonymous-synthesized) name.
    #[must_use]
    pub fn declared_name(&self) -> &str {
        &self.declared_name
    }

    /// Canonical digest of the declaration form.
    #[must_use]
    pub fn declaration_digest(&self) -> &str {
        &self.declaration_digest
    }

    /// Canonical labeled-field fingerprint contribution text.
    #[must_use]
    pub fn fingerprint_text(&self) -> String {
        SemanticIdentityFingerprint::new("declaration-key")
            .field("form", &self.declaration_form)
            .field("name", &self.declared_name)
            .field("digest", &self.declaration_digest)
            .finish()
    }
}

/// Kind of dependency one contribution records.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticDependencyKind {
    /// Parent semantic scope.
    ParentScope,
    /// A specific declaration.
    Declaration,
    /// Package state in effect.
    PackageState,
    /// Pragma state in effect.
    PragmaState,
    /// A named fact elsewhere.
    NamedFact,
}

impl SemanticDependencyKind {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::ParentScope => "parent-scope",
            Self::Declaration => "declaration",
            Self::PackageState => "package-state",
            Self::PragmaState => "pragma-state",
            Self::NamedFact => "named-fact",
        }
    }
}

/// Stable identity of one dependency edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticDependencyIdentity {
    kind: SemanticDependencyKind,
    target_fingerprint: String,
}

impl SemanticDependencyIdentity {
    /// Construct a dependency identity.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when the
    /// target fingerprint is empty.
    pub fn new(
        kind: SemanticDependencyKind,
        target_fingerprint: impl Into<String>,
    ) -> Result<Self, SemanticIdentityContractError> {
        let target_fingerprint = target_fingerprint.into();
        if target_fingerprint.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticDependencyIdentity.target_fingerprint",
            ));
        }
        Ok(Self { kind, target_fingerprint })
    }

    /// Dependency kind.
    #[must_use]
    pub fn kind(&self) -> SemanticDependencyKind {
        self.kind
    }

    /// Logical fingerprint of the dependency target.
    #[must_use]
    pub fn target_fingerprint(&self) -> &str {
        &self.target_fingerprint
    }
}

/// Stable deterministic identity of one semantic contribution.
///
/// Composed from the owner, fact family, anchor digest, and a family-local
/// ordinal. Two facts with the same [`SemanticContributionId`] describe the
/// same logical contribution of the same owner at the same subject
/// generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticContributionId {
    schema: SemanticIdentitySchema,
    owner_fingerprint: String,
    fact_family: SemanticFactFamily,
    anchor_digest: String,
    family_ordinal: u32,
}

impl SemanticContributionId {
    /// Construct a contribution identity.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when the
    /// owner fingerprint or anchor digest is empty.
    pub fn new(
        owner_fingerprint: impl Into<String>,
        fact_family: SemanticFactFamily,
        anchor_digest: impl Into<String>,
        family_ordinal: u32,
    ) -> Result<Self, SemanticIdentityContractError> {
        let owner_fingerprint = owner_fingerprint.into();
        let anchor_digest = anchor_digest.into();
        if owner_fingerprint.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticContributionId.owner_fingerprint",
            ));
        }
        if anchor_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticContributionId.anchor_digest",
            ));
        }
        Ok(Self {
            schema: SemanticIdentitySchema::V1,
            owner_fingerprint,
            fact_family,
            anchor_digest,
            family_ordinal,
        })
    }

    /// Schema tag of this identity.
    #[must_use]
    pub fn schema(&self) -> SemanticIdentitySchema {
        self.schema
    }

    /// Fact family of the contribution.
    #[must_use]
    pub fn fact_family(&self) -> SemanticFactFamily {
        self.fact_family
    }

    /// Deterministic fingerprint of this contribution identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        SemanticIdentityFingerprint::new(self.schema.tag())
            .field("owner", &self.owner_fingerprint)
            .discriminant("family", self.fact_family.tag())
            .field("anchor", &self.anchor_digest)
            .field("ordinal", &self.family_ordinal.to_string())
            .finish()
    }
}

/// One typed owner record for a semantic contribution.
///
/// Records the exact subject generation, fact family, primary and related
/// source anchors, package/context identity, provenance/confidence/
/// completeness/limitations, dependencies, and derives a deterministic
/// contribution identity and fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticContributionOwner {
    schema: SemanticIdentitySchema,
    subject: SemanticSubjectGeneration,
    disposition: SemanticOwnershipDisposition,
    fact_family: SemanticFactFamily,
    primary_anchor_digest: String,
    related_anchor_digests: Vec<String>,
    status: SemanticSubjectStatus,
    dependencies: Vec<SemanticDependencyIdentity>,
    limitations: Vec<String>,
}

impl SemanticContributionOwner {
    /// Construct an owner record.
    ///
    /// # Errors
    /// Returns a contract error when identity slots are empty, related
    /// anchors/dependencies are duplicated, or a complete status is claimed
    /// for a scope-owned family without a scope owner.
    pub fn new(
        subject: SemanticSubjectGeneration,
        disposition: SemanticOwnershipDisposition,
        fact_family: SemanticFactFamily,
        primary_anchor_digest: impl Into<String>,
        related_anchor_digests: Vec<String>,
        status: SemanticSubjectStatus,
        dependencies: Vec<SemanticDependencyIdentity>,
        limitations: Vec<String>,
    ) -> Result<Self, SemanticIdentityContractError> {
        let primary_anchor_digest = primary_anchor_digest.into();
        if primary_anchor_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticContributionOwner.primary_anchor_digest",
            ));
        }
        if matches!(&disposition,
            SemanticOwnershipDisposition::ScopeOwned { scope_fingerprint }
                if scope_fingerprint.trim().is_empty())
        {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticOwnershipDisposition.scope_fingerprint",
            ));
        }
        if matches!(&disposition,
            SemanticOwnershipDisposition::UnsupportedNotProven { reason }
                if reason.trim().is_empty())
        {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticOwnershipDisposition.reason",
            ));
        }
        let mut sorted_related = related_anchor_digests;
        sorted_related.sort();
        if sorted_related.iter().any(|d| d.trim().is_empty()) {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticContributionOwner.related_anchor_digests",
            ));
        }
        if sorted_related.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "related anchor digests must be distinct",
            ));
        }
        // Deterministic dependency order: dependency identity is compared as a
        // whole, never by insertion order.
        let mut sorted_dependencies = dependencies;
        sorted_dependencies.sort_by(|a, b| {
            (a.kind.tag(), &a.target_fingerprint).cmp(&(b.kind.tag(), &b.target_fingerprint))
        });
        let dep_duplicates = sorted_dependencies.windows(2).any(|pair| {
            pair[0].kind == pair[1].kind && pair[0].target_fingerprint == pair[1].target_fingerprint
        });
        if dep_duplicates {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "owner dependencies must be distinct",
            ));
        }
        if fact_family.is_scope_local()
            && !matches!(disposition, SemanticOwnershipDisposition::ScopeOwned { .. })
        {
            return Err(SemanticIdentityContractError::MissingCompanion(
                "scope-local family requires a ScopeOwned disposition",
            ));
        }
        // Fail closed at construction: known-invalid owner records are never
        // minted, so validity is not opt-in through a later validate() call.
        if matches!(disposition, SemanticOwnershipDisposition::UnsupportedNotProven { .. })
            && status.is_complete()
        {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "an unsupported/not-proven owner can never claim complete status",
            ));
        }
        if status.is_complete() && !limitations.is_empty() {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "a complete contribution must record no limitations",
            ));
        }
        Ok(Self {
            schema: SemanticIdentitySchema::V1,
            subject,
            disposition,
            fact_family,
            primary_anchor_digest,
            related_anchor_digests: sorted_related,
            status,
            dependencies: sorted_dependencies,
            limitations,
        })
    }

    /// Exact subject generation this owner describes.
    #[must_use]
    pub fn subject(&self) -> &SemanticSubjectGeneration {
        &self.subject
    }

    /// Typed ownership disposition.
    #[must_use]
    pub fn disposition(&self) -> &SemanticOwnershipDisposition {
        &self.disposition
    }

    /// Fact family of the owned contribution.
    #[must_use]
    pub fn fact_family(&self) -> SemanticFactFamily {
        self.fact_family
    }

    /// Shared status of the owned contribution.
    #[must_use]
    pub fn status(&self) -> SemanticSubjectStatus {
        self.status
    }

    /// Recorded limitations, in canonical (sorted) order.
    #[must_use]
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Recorded dependencies, in canonical order.
    #[must_use]
    pub fn dependencies(&self) -> &[SemanticDependencyIdentity] {
        &self.dependencies
    }

    /// Derive the deterministic contribution identity of this owner.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::ContradictoryStatus`] for an
    /// unsupported/not-proven owner: that disposition carries no reusable
    /// incremental identity by definition. Otherwise propagates contract
    /// errors from [`SemanticContributionId::new`].
    pub fn contribution_id(
        &self,
        family_ordinal: u32,
    ) -> Result<SemanticContributionId, SemanticIdentityContractError> {
        if matches!(self.disposition, SemanticOwnershipDisposition::UnsupportedNotProven { .. }) {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "an unsupported/not-proven owner carries no reusable contribution identity",
            ));
        }
        SemanticContributionId::new(
            self.owner_fingerprint(),
            self.fact_family,
            &self.primary_anchor_digest,
            family_ordinal,
        )
    }

    /// Deterministic owner fingerprint (schema-tagged).
    ///
    /// Every multi-component contributor (subject, disposition payload,
    /// dependency edges) is mixed as independently framed labeled fields, so
    /// separator-like content in any component cannot shift a field boundary.
    #[must_use]
    pub fn owner_fingerprint(&self) -> String {
        let fp =
            self.subject.mix_subject_fields(SemanticIdentityFingerprint::new(self.schema.tag()));
        let fp = self.disposition.mix_into(fp.discriminant("disposition", self.disposition.tag()));
        let fp = fp
            .discriminant("family", self.fact_family.tag())
            .field("primary-anchor", &self.primary_anchor_digest);
        let fp = self
            .related_anchor_digests
            .iter()
            .fold(fp, |acc, digest| acc.field("related-anchor", digest));
        let fp = self.dependencies.iter().fold(fp, |acc, dep| {
            acc.discriminant("dep-kind", dep.kind.tag())
                .field("dep-target", &dep.target_fingerprint)
        });
        fp.finish()
    }

    /// Validate the owner contract.
    ///
    /// # Errors
    /// Returns a contract error when a complete status is claimed for an
    /// unsupported/not-proven owner, or when the recorded limitations
    /// contradict a complete claim.
    pub fn validate(&self) -> Result<(), SemanticIdentityContractError> {
        if matches!(self.disposition, SemanticOwnershipDisposition::UnsupportedNotProven { .. })
            && self.status.is_complete()
        {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "an unsupported/not-proven owner can never claim complete status",
            ));
        }
        if self.status.is_complete() && !self.limitations.is_empty() {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "a complete contribution must record no limitations",
            ));
        }
        Ok(())
    }
}
