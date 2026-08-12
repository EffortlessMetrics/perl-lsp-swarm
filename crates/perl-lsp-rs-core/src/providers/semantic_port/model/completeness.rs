/// Concrete denominator receipt retained for an exact-empty result.
///
/// The receipt is created only from a verified snapshot inside the semantic-port
/// control plane. Public provider implementations can inspect it but cannot
/// manufacture one from labels or exact-looking enums.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCompletenessAuthorityReceipt {
    capability: ProviderQueryCapability,
    project_identity: ProviderIdentity,
    root_identity: ProviderIdentity,
    document_generation: SourceGeneration,
    workspace_generation: SourceGeneration,
    producer: SemanticProducer,
    denominator_id: String,
    snapshot_id: String,
    covered_unit_count: u64,
}

impl ProviderCompletenessAuthorityReceipt {
    /// Query family whose supported denominator is complete.
    #[must_use]
    pub const fn capability(&self) -> ProviderQueryCapability {
        self.capability
    }

    /// Project identity bound to the denominator snapshot.
    #[must_use]
    pub const fn project_identity(&self) -> &ProviderIdentity {
        &self.project_identity
    }

    /// Workspace-root identity bound to the denominator snapshot.
    #[must_use]
    pub const fn root_identity(&self) -> &ProviderIdentity {
        &self.root_identity
    }

    /// Document generation bound to the denominator snapshot.
    #[must_use]
    pub const fn document_generation(&self) -> &SourceGeneration {
        &self.document_generation
    }

    /// Workspace/model generation bound to the denominator snapshot.
    #[must_use]
    pub const fn workspace_generation(&self) -> &SourceGeneration {
        &self.workspace_generation
    }

    /// Producer that owns the denominator snapshot.
    #[must_use]
    pub const fn producer(&self) -> SemanticProducer {
        self.producer
    }

    /// Stable denominator identity for the query family.
    #[must_use]
    pub fn denominator_id(&self) -> &str {
        &self.denominator_id
    }

    /// Stable snapshot identity for the authority input.
    #[must_use]
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// Number of concrete units covered by the denominator snapshot.
    #[must_use]
    pub const fn covered_unit_count(&self) -> u64 {
        self.covered_unit_count
    }
}

/// Test-only verified completeness snapshot. Production issuance is intentionally
/// absent until an owning adapter supplies a concrete denominator in #6817.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerifiedProviderCompletenessSnapshot {
    authority: ProviderCompletenessAuthorityReceipt,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
}

#[cfg(test)]
impl VerifiedProviderCompletenessSnapshot {
    /// Validate a concrete denominator snapshot before it can issue a grant.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_new(
        request: &ProviderQueryRequest,
        capability: ProviderQueryCapability,
        producer: SemanticProducer,
        denominator_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        covered_unit_count: u64,
        provenance: SemanticProvenance,
        confidence: SemanticConfidence,
        freshness: SemanticFreshness,
    ) -> Result<Self, ProviderQueryContractError> {
        let denominator_id = denominator_id.into();
        let snapshot_id = snapshot_id.into();
        if !request.is_well_formed()
            || !request.context.is_exact_ready()
            || capability != ProviderQueryCapability::from_query(&request.kind)
            || producer == SemanticProducer::Unknown
            || denominator_id.trim().is_empty()
            || snapshot_id.trim().is_empty()
            || covered_unit_count == 0
            || !semantic_provenance_is_exact(provenance)
            || confidence != SemanticConfidence::Known(Confidence::High)
            || freshness != SemanticFreshness::Fresh
        {
            return Err(ProviderQueryContractError::InvalidCompletenessGrant);
        }
        Ok(Self {
            authority: ProviderCompletenessAuthorityReceipt {
                capability,
                project_identity: request.context.project_identity.clone(),
                root_identity: request.context.root_identity.clone(),
                document_generation: request.context.document_generation.clone(),
                workspace_generation: request.context.workspace_generation.clone(),
                producer,
                denominator_id,
                snapshot_id,
                covered_unit_count,
            },
            provenance,
            confidence,
            freshness,
        })
    }
}

/// Exact supported-denominator authority for one request family.
///
/// The type is public so checked results can expose its evidence, but it has no
/// public constructor. This PR exposes no production issuer: #6817 must add a
/// crate-owned adapter from a concrete producer denominator snapshot.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderCompletenessGrant {
    authority: ProviderCompletenessAuthorityReceipt,
    provenance: SemanticProvenance,
    confidence: SemanticConfidence,
    freshness: SemanticFreshness,
}

impl ProviderCompletenessGrant {
    #[cfg(test)]
    pub(super) fn from_verified_snapshot(snapshot: VerifiedProviderCompletenessSnapshot) -> Self {
        Self {
            authority: snapshot.authority,
            provenance: snapshot.provenance,
            confidence: snapshot.confidence,
            freshness: snapshot.freshness,
        }
    }

    pub(crate) fn matches(&self, request: &ProviderQueryRequest) -> bool {
        self.authority.capability == ProviderQueryCapability::from_query(&request.kind)
            && self.authority.project_identity == request.context.project_identity
            && self.authority.root_identity == request.context.root_identity
            && self.authority.document_generation == request.context.document_generation
            && self.authority.workspace_generation == request.context.workspace_generation
            && self.authority.producer != SemanticProducer::Unknown
            && !self.authority.denominator_id.trim().is_empty()
            && !self.authority.snapshot_id.trim().is_empty()
            && self.authority.covered_unit_count > 0
            && semantic_provenance_is_exact(self.provenance)
            && self.confidence == SemanticConfidence::Known(Confidence::High)
            && self.freshness == SemanticFreshness::Fresh
    }

    pub(crate) const fn authority(&self) -> &ProviderCompletenessAuthorityReceipt {
        &self.authority
    }

    pub(crate) const fn provenance(&self) -> SemanticProvenance {
        self.provenance
    }

    pub(crate) const fn confidence(&self) -> SemanticConfidence {
        self.confidence
    }

    pub(crate) const fn freshness(&self) -> SemanticFreshness {
        self.freshness
    }
}
