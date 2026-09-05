//! Stable semantic scope identity and its subject generation.

use super::SemanticDeclarationKey;
use super::{SemanticIdentityContractError, SemanticIdentityFingerprint, SemanticIdentitySchema};

use serde::{Deserialize, Serialize};

/// Kind of a semantic scope.
///
/// The closed kind set distinguishes every scope class required by the scope
/// identity law: file/global scope, package statement versus package-block
/// context, named and anonymous subs/methods, lexical blocks, loops and
/// branches, eval and phase scopes, class/role scopes, and recovered or
/// synthetic regions.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticScopeKind {
    /// File/global scope.
    File,
    /// Package statement (`package Foo;`) introducing source-order context.
    PackageStatement,
    /// Package block (`package Foo { ... }`) owning a lexical region.
    PackageBlock,
    /// Named subroutine.
    NamedSubroutine,
    /// Anonymous subroutine.
    AnonymousSubroutine,
    /// Named method within a class/role.
    Method,
    /// Ordinary lexical block.
    LexicalBlock,
    /// Loop body scope.
    Loop,
    /// Conditional body scope.
    Conditional,
    /// `eval` body scope.
    Eval,
    /// Phase block (`BEGIN`/`END`/... ) where represented.
    Phase,
    /// Class scope where represented.
    Class,
    /// Role scope where represented.
    Role,
    /// Region reconstructed by recovery.
    RecoveredRegion,
    /// Scope synthesized without a direct source construct.
    Synthetic,
}

impl SemanticScopeKind {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::PackageStatement => "package-statement",
            Self::PackageBlock => "package-block",
            Self::NamedSubroutine => "named-sub",
            Self::AnonymousSubroutine => "anonymous-sub",
            Self::Method => "method",
            Self::LexicalBlock => "lexical-block",
            Self::Loop => "loop",
            Self::Conditional => "conditional",
            Self::Eval => "eval",
            Self::Phase => "phase",
            Self::Class => "class",
            Self::Role => "role",
            Self::RecoveredRegion => "recovered-region",
            Self::Synthetic => "synthetic",
        }
    }
}

/// Recovery/synthesis/ambiguity disposition of a scope.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticScopeRecovery {
    /// Constructed from an exact, accepted parse region.
    Exact,
    /// Constructed from a recovered region.
    Recovered,
    /// Synthesized without a direct source construct.
    Synthetic,
    /// Ambiguous among candidate interpretations.
    Ambiguous,
}

impl SemanticScopeRecovery {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Recovered => "recovered",
            Self::Synthetic => "synthetic",
            Self::Ambiguous => "ambiguous",
        }
    }

    /// Whether this disposition can support an exact complete claim.
    #[must_use]
    pub fn supports_complete_claim(self) -> bool {
        matches!(self, Self::Exact)
    }
}

/// Role a source anchor plays in scope identity.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticAnchorRole {
    /// Scope header text (e.g. signature line, package header).
    Header,
    /// Canonical subtree digest of the scope body.
    Subtree,
    /// Owning declaration name plus its declaration-form digest.
    DeclarationName,
    /// Context marker (e.g. phase keyword, pragma marker) where represented.
    ContextMarker,
}

impl SemanticAnchorRole {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Subtree => "subtree",
            Self::DeclarationName => "declaration-name",
            Self::ContextMarker => "context-marker",
        }
    }
}

/// Logical source anchor for one scope.
///
/// The anchor is a digest over anchor-role-specific canonical source text
/// (header text, subtree digest, declaration form, or context marker), plus an
/// ordinal that disambiguates same-anchor siblings within one parent. It is
/// deliberately not a raw byte offset, line number, path, or display name
/// alone. Unrelated earlier insertions with different anchors therefore do not
/// disturb this identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticSourceAnchor {
    anchor_role: SemanticAnchorRole,
    anchor_digest: String,
    sibling_ordinal: u32,
}

impl SemanticSourceAnchor {
    /// Construct an anchor from a canonical digest.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when the
    /// digest is empty.
    pub fn new(
        anchor_role: SemanticAnchorRole,
        anchor_digest: impl Into<String>,
        sibling_ordinal: u32,
    ) -> Result<Self, SemanticIdentityContractError> {
        let anchor_digest = anchor_digest.into();
        if anchor_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSourceAnchor.anchor_digest",
            ));
        }
        Ok(Self { anchor_role, anchor_digest, sibling_ordinal })
    }

    /// Anchor role.
    #[must_use]
    pub fn anchor_role(&self) -> SemanticAnchorRole {
        self.anchor_role
    }

    /// Canonical anchor digest text.
    #[must_use]
    pub fn anchor_digest(&self) -> &str {
        &self.anchor_digest
    }

    /// Ordinal disambiguating same-anchor siblings within one parent.
    #[must_use]
    pub fn sibling_ordinal(&self) -> u32 {
        self.sibling_ordinal
    }
}

/// Source-order context identity (package/source-order effects).
///
/// Package transitions, pragma effects, imports, prototypes, and features are
/// owned by source-order context rather than by a lexical scope bucket. This
/// identity names that context deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticSourceOrderIdentity {
    context_ordinal: u32,
    context_digest: String,
}

impl SemanticSourceOrderIdentity {
    /// Construct a source-order context identity.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when the
    /// digest is empty.
    pub fn new(
        context_ordinal: u32,
        context_digest: impl Into<String>,
    ) -> Result<Self, SemanticIdentityContractError> {
        let context_digest = context_digest.into();
        if context_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSourceOrderIdentity.context_digest",
            ));
        }
        Ok(Self { context_ordinal, context_digest })
    }

    /// Ordinal of this context in source order.
    #[must_use]
    pub fn context_ordinal(&self) -> u32 {
        self.context_ordinal
    }

    /// Canonical context digest.
    #[must_use]
    pub fn context_digest(&self) -> &str {
        &self.context_digest
    }
}

/// Exact accepted semantic-profile identity.
///
/// Profiles select which semantic fact families a producer attempts; identity
/// comparison must bind the profile, not merely its display name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticSemanticProfileIdentity {
    profile_id: String,
    profile_digest: String,
}

impl SemanticSemanticProfileIdentity {
    /// Construct a semantic profile identity.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when
    /// either component is empty.
    pub fn new(
        profile_id: impl Into<String>,
        profile_digest: impl Into<String>,
    ) -> Result<Self, SemanticIdentityContractError> {
        let profile_id = profile_id.into();
        let profile_digest = profile_digest.into();
        if profile_id.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSemanticProfileIdentity.profile_id",
            ));
        }
        if profile_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSemanticProfileIdentity.profile_digest",
            ));
        }
        Ok(Self { profile_id, profile_digest })
    }

    /// Durable profile identifier.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// Canonical profile digest (families/configuration).
    #[must_use]
    pub fn profile_digest(&self) -> &str {
        &self.profile_digest
    }
}

/// Exact subject generation binding for semantic identities.
///
/// Binds the logical document instance (root instance + document instance),
/// the accepted source generation, the accepted parser snapshot and
/// configuration identity, and the semantic profile. Source-identical later
/// generations, close/reopen instances, and the same relative path/content in
/// two roots remain distinct because all five slots participate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticSubjectGeneration {
    logical_source_id: String,
    source_generation: String,
    parser_snapshot_id: String,
    parser_configuration_id: String,
    profile: SemanticSemanticProfileIdentity,
}

impl SemanticSubjectGeneration {
    /// Construct a subject generation.
    ///
    /// # Errors
    /// Returns [`SemanticIdentityContractError::EmptyIdentityField`] when any
    /// identity slot is empty.
    pub fn new(
        logical_source_id: impl Into<String>,
        source_generation: impl Into<String>,
        parser_snapshot_id: impl Into<String>,
        parser_configuration_id: impl Into<String>,
        profile: SemanticSemanticProfileIdentity,
    ) -> Result<Self, SemanticIdentityContractError> {
        let logical_source_id = logical_source_id.into();
        let source_generation = source_generation.into();
        let parser_snapshot_id = parser_snapshot_id.into();
        let parser_configuration_id = parser_configuration_id.into();
        if logical_source_id.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSubjectGeneration.logical_source_id",
            ));
        }
        if source_generation.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSubjectGeneration.source_generation",
            ));
        }
        if parser_snapshot_id.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSubjectGeneration.parser_snapshot_id",
            ));
        }
        if parser_configuration_id.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticSubjectGeneration.parser_configuration_id",
            ));
        }
        Ok(Self {
            logical_source_id,
            source_generation,
            parser_snapshot_id,
            parser_configuration_id,
            profile,
        })
    }

    /// Logical document instance identity (root instance + document instance).
    #[must_use]
    pub fn logical_source_id(&self) -> &str {
        &self.logical_source_id
    }

    /// Accepted source generation.
    #[must_use]
    pub fn source_generation(&self) -> &str {
        &self.source_generation
    }

    /// Accepted parser snapshot identity.
    #[must_use]
    pub fn parser_snapshot_id(&self) -> &str {
        &self.parser_snapshot_id
    }

    /// Accepted parser configuration identity.
    #[must_use]
    pub fn parser_configuration_id(&self) -> &str {
        &self.parser_configuration_id
    }

    /// Semantic profile identity.
    #[must_use]
    pub fn profile(&self) -> &SemanticSemanticProfileIdentity {
        &self.profile
    }

    /// Mix each subject component as an independently framed, labeled field.
    ///
    /// Component values are never flattened with separators, so a component
    /// containing any separator-like content cannot shift across a field
    /// boundary.
    pub(super) fn mix_subject_fields(
        &self,
        fp: SemanticIdentityFingerprint,
    ) -> SemanticIdentityFingerprint {
        fp.field("logical-source", &self.logical_source_id)
            .field("source-generation", &self.source_generation)
            .field("parser-snapshot", &self.parser_snapshot_id)
            .field("parser-config", &self.parser_configuration_id)
            .field("profile-id", &self.profile.profile_id)
            .field("profile-digest", &self.profile.profile_digest)
    }
}

/// Stable logical identity of one semantic scope.
///
/// Composes the exact subject generation, scope kind, owning declaration key
/// text, parent logical fingerprint, logical source anchor, package/source-
/// order context, and recovery disposition. See the module documentation for
/// the identity law this type enforces.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticScopeIdentity {
    schema: SemanticIdentitySchema,
    subject: SemanticSubjectGeneration,
    kind: SemanticScopeKind,
    owning_declaration_key: Option<SemanticDeclarationKey>,
    parent_fingerprint: Option<String>,
    anchor: SemanticSourceAnchor,
    package_context: Option<SemanticSourceOrderIdentity>,
    recovery: SemanticScopeRecovery,
}

impl SemanticScopeIdentity {
    /// Construct a scope identity.
    ///
    /// # Errors
    /// Returns a contract error when the parent fingerprint is empty, a
    /// file/global scope carries a parent, or a non-file scope lacks one.
    pub fn new(
        subject: SemanticSubjectGeneration,
        kind: SemanticScopeKind,
        owning_declaration_key: Option<SemanticDeclarationKey>,
        parent_fingerprint: Option<String>,
        anchor: SemanticSourceAnchor,
        package_context: Option<SemanticSourceOrderIdentity>,
        recovery: SemanticScopeRecovery,
    ) -> Result<Self, SemanticIdentityContractError> {
        if parent_fingerprint.as_deref().is_some_and(|parent| parent.trim().is_empty()) {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticScopeIdentity.parent_fingerprint",
            ));
        }
        if owning_declaration_key.as_ref().is_some_and(|key| {
            key.declaration_form().trim().is_empty()
                || key.declared_name().trim().is_empty()
                || key.declaration_digest().trim().is_empty()
        }) {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticScopeIdentity.owning_declaration_key",
            ));
        }
        if matches!(kind, SemanticScopeKind::File) && parent_fingerprint.is_some() {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "file/global scope cannot have a parent scope",
            ));
        }
        if !matches!(kind, SemanticScopeKind::File) && parent_fingerprint.is_none() {
            return Err(SemanticIdentityContractError::MissingCompanion(
                "SemanticScopeIdentity.parent_fingerprint",
            ));
        }
        // Fail closed at construction: validity is not opt-in through a
        // later validate() call.
        if matches!(kind, SemanticScopeKind::PackageStatement | SemanticScopeKind::PackageBlock)
            && package_context.is_none()
        {
            return Err(SemanticIdentityContractError::MissingCompanion(
                "SemanticScopeIdentity.package_context",
            ));
        }
        if matches!(recovery, SemanticScopeRecovery::Recovered | SemanticScopeRecovery::Ambiguous)
            && matches!(kind, SemanticScopeKind::File)
        {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "file/global scope must be exact or explicitly synthesized, never recovered/ambiguous",
            ));
        }
        Ok(Self {
            schema: SemanticIdentitySchema::V1,
            subject,
            kind,
            owning_declaration_key,
            parent_fingerprint,
            anchor,
            package_context,
            recovery,
        })
    }

    /// Schema tag of this identity.
    #[must_use]
    pub fn schema(&self) -> SemanticIdentitySchema {
        self.schema
    }

    /// Exact subject generation this identity describes.
    #[must_use]
    pub fn subject(&self) -> &SemanticSubjectGeneration {
        &self.subject
    }

    /// Scope kind.
    #[must_use]
    pub fn kind(&self) -> SemanticScopeKind {
        self.kind
    }

    /// Typed owning declaration key, where a declaration owns the scope.
    #[must_use]
    pub fn owning_declaration_key(&self) -> Option<&SemanticDeclarationKey> {
        self.owning_declaration_key.as_ref()
    }

    /// Logical fingerprint of the parent scope, where present.
    #[must_use]
    pub fn parent_fingerprint(&self) -> Option<&str> {
        self.parent_fingerprint.as_deref()
    }

    /// Logical source anchor.
    #[must_use]
    pub fn anchor(&self) -> &SemanticSourceAnchor {
        &self.anchor
    }

    /// Package/source-order context, where this scope is context-owned.
    #[must_use]
    pub fn package_context(&self) -> Option<&SemanticSourceOrderIdentity> {
        self.package_context.as_ref()
    }

    /// Recovery disposition.
    #[must_use]
    pub fn recovery(&self) -> SemanticScopeRecovery {
        self.recovery
    }

    /// Deterministic logical fingerprint of this scope.
    ///
    /// Stable under unrelated earlier source movement because the parent
    /// contributes its own logical fingerprint (recursively stable) rather
    /// than an ordinal, and the anchor contributes its digest rather than an
    /// offset.
    ///
    /// # Parent-fingerprint law
    ///
    /// `parent_fingerprint` is opaque by design: it names the parent scope's
    /// logical fingerprint, and this type cannot prove from the string alone
    /// which subject generation produced it. Constructors and producers must
    /// only install a parent fingerprint obtained from a
    /// [`SemanticScopeIdentity`] of the same [`SemanticSubjectGeneration`]
    /// (same logical source, source generation, parser snapshot/config, and
    /// profile); a fingerprint from any other subject is an invalid
    /// construction. Downstream consumers confirm candidate fingerprint
    /// matches with structural scope identity before reuse.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let fp =
            self.subject.mix_subject_fields(SemanticIdentityFingerprint::new(self.schema.tag()));
        let fp = fp
            .discriminant("kind", self.kind.tag())
            .field("decl-present", &self.owning_declaration_key.is_some().to_string())
            .field(
                "decl",
                self.owning_declaration_key
                    .as_ref()
                    .map(SemanticDeclarationKey::fingerprint_text)
                    .as_deref()
                    .unwrap_or(""),
            );
        let mut fp = fp;
        if let Some(ctx) = self.package_context.as_ref() {
            fp = fp
                .field("package-context-ordinal", &ctx.context_ordinal.to_string())
                .field("package-context-digest", &ctx.context_digest);
        }
        fp.discriminant("anchor-role", self.anchor.anchor_role.tag())
            .field("anchor-digest", &self.anchor.anchor_digest)
            .field("anchor-ordinal", &self.anchor.sibling_ordinal.to_string())
            .field("parent-present", &self.parent_fingerprint.is_some().to_string())
            .field("parent", self.parent_fingerprint.as_deref().unwrap_or(""))
            .discriminant("recovery", self.recovery.tag())
            .finish()
    }

    /// Validate the identity contract.
    ///
    /// # Errors
    /// Returns a contract error when any identity slot is empty or a
    /// structural rule is violated.
    pub fn validate(&self) -> Result<(), SemanticIdentityContractError> {
        if matches!(
            self.recovery,
            SemanticScopeRecovery::Recovered | SemanticScopeRecovery::Ambiguous
        ) && matches!(self.kind, SemanticScopeKind::File)
        {
            return Err(SemanticIdentityContractError::ContradictoryStatus(
                "file/global scope must be exact or explicitly synthesized, never recovered/ambiguous",
            ));
        }
        if self.anchor.anchor_digest.trim().is_empty() {
            return Err(SemanticIdentityContractError::EmptyIdentityField(
                "SemanticScopeIdentity.anchor",
            ));
        }
        if matches!(
            self.kind,
            SemanticScopeKind::PackageStatement | SemanticScopeKind::PackageBlock
        ) && self.package_context.is_none()
        {
            return Err(SemanticIdentityContractError::MissingCompanion(
                "SemanticScopeIdentity.package_context",
            ));
        }
        Ok(())
    }
}
