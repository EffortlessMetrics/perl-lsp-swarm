/// Role one canonical fact plays in a provider query result.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderQueryFactRole {
    /// Fact selects the target at the request subject but is not returned.
    Selector,
    /// Fact is returned as a semantic value.
    Value,
    /// Fact both selects the target and is returned.
    SelectorValue,
    /// Fact supports a degraded or no-value outcome.
    Supporting,
}

impl ProviderQueryFactRole {
    pub(crate) fn is_selector(self) -> bool {
        matches!(self, Self::Selector | Self::SelectorValue)
    }

    pub(crate) fn is_value(self) -> bool {
        matches!(self, Self::Value | Self::SelectorValue)
    }

    pub(crate) fn is_supporting(self) -> bool {
        self == Self::Supporting
    }
}

/// Request generation to which a fact is bound.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProviderFactGenerationScope {
    /// Fact is bound to the request's document generation.
    Document,
    /// Fact is bound to the request's workspace/model generation.
    Workspace,
}

/// One canonical semantic fact with only source-level symbol aliases supplied by an adapter.
///
/// Entity, file, package, scope, and source geometry are always derived from the
/// envelope and cannot be overridden through parallel match keys.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderQueryFact {
    role: ProviderQueryFactRole,
    generation_scope: ProviderFactGenerationScope,
    envelope: SemanticFactEnvelope,
    symbols: Vec<String>,
}

impl ProviderQueryFact {
    /// Construct and validate a query fact.
    pub fn try_new(
        role: ProviderQueryFactRole,
        generation_scope: ProviderFactGenerationScope,
        envelope: SemanticFactEnvelope,
        symbols: impl IntoIterator<Item = String>,
    ) -> Result<Self, ProviderQueryContractError> {
        validate_envelope_structure(&envelope)?;
        let mut symbols: Vec<_> = symbols.into_iter().collect();
        if symbols.iter().any(|symbol| symbol.trim().is_empty()) {
            return Err(ProviderQueryContractError::MalformedSymbolKey);
        }
        symbols.sort();
        symbols.dedup();
        Ok(Self {
            role,
            generation_scope,
            envelope,
            symbols,
        })
    }

    /// Construct a query fact without source-level symbol aliases.
    pub fn from_envelope(
        role: ProviderQueryFactRole,
        generation_scope: ProviderFactGenerationScope,
        envelope: SemanticFactEnvelope,
    ) -> Result<Self, ProviderQueryContractError> {
        Self::try_new(role, generation_scope, envelope, Vec::new())
    }

    /// Fact role.
    #[must_use]
    pub const fn role(&self) -> ProviderQueryFactRole {
        self.role
    }

    /// Generation binding.
    #[must_use]
    pub const fn generation_scope(&self) -> ProviderFactGenerationScope {
        self.generation_scope
    }

    /// Canonical semantic envelope.
    #[must_use]
    pub const fn envelope(&self) -> &SemanticFactEnvelope {
        &self.envelope
    }

    /// Canonical source-level symbol aliases.
    #[must_use]
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    pub(crate) fn matches_subject_directly(&self, subject: &ProviderQuerySubject) -> bool {
        match subject {
            ProviderQuerySubject::Entity(entity_id) => self.envelope.entity_id == Some(*entity_id),
            ProviderQuerySubject::File(file_id) => self.envelope.anchor.file_id == *file_id,
            ProviderQuerySubject::Position {
                file_id,
                byte_offset,
            } => {
                self.envelope.anchor.file_id == *file_id
                    && range_contains(&self.envelope, *byte_offset)
            }
            ProviderQuerySubject::Package(package) => {
                self.envelope.package.as_deref() == Some(package.as_str())
            }
            ProviderQuerySubject::Symbol(symbol) => self.symbols.binary_search(symbol).is_ok(),
            ProviderQuerySubject::Workspace => true,
        }
    }

    pub(crate) fn is_generation_current(&self, request: &ProviderQueryRequest) -> bool {
        let expected = match self.generation_scope {
            ProviderFactGenerationScope::Document => &request.context.document_generation,
            ProviderFactGenerationScope::Workspace => &request.context.workspace_generation,
        };
        generation_is_known(expected)
            && generation_is_known(&self.envelope.source_generation)
            && &self.envelope.source_generation == expected
    }
}
