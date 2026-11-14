//! Package, declaration, and source-tier identity types.

use super::{
    Fingerprint, PACKAGE_TOPOLOGY_SCHEMA_TAG, PackageTopologyContractError, PackageTopologySchema,
    fold_optional_span, require_identity, validate_span,
};
use crate::SourceAnchor;

use serde::{Deserialize, Serialize};

/// Canonical logical package identity in one project and one resolution domain.
///
/// Package spelling alone is not a [`PackageId`]. The same qualified name in
/// two roots, or in a workspace root versus a resolved-dependency domain, is
/// two identities. `main` is scoped by the same three slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    schema: PackageTopologySchema,
    project_identity: String,
    resolution_domain: String,
    qualified_name: String,
}

impl PackageId {
    /// Construct a package identity from project, resolution domain, and name.
    ///
    /// # Errors
    /// Returns [`PackageTopologyContractError::EmptyIdentityField`] when any
    /// slot is empty or whitespace.
    pub fn new(
        project_identity: impl Into<String>,
        resolution_domain: impl Into<String>,
        qualified_name: impl Into<String>,
    ) -> Result<Self, PackageTopologyContractError> {
        let identity = Self {
            schema: PackageTopologySchema::V1,
            project_identity: require_identity("PackageId.project_identity", project_identity)?,
            resolution_domain: require_identity("PackageId.resolution_domain", resolution_domain)?,
            qualified_name: require_identity("PackageId.qualified_name", qualified_name)?,
        };
        identity.validate()?;
        Ok(identity)
    }

    /// Validate identity slots after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        require_identity("PackageId.project_identity", self.project_identity.as_str())?;
        require_identity("PackageId.resolution_domain", self.resolution_domain.as_str())?;
        require_identity("PackageId.qualified_name", self.qualified_name.as_str())?;
        Ok(())
    }

    /// Project or workspace identity.
    #[must_use]
    pub fn project_identity(&self) -> &str {
        &self.project_identity
    }

    /// Resolution domain or root identity.
    #[must_use]
    pub fn resolution_domain(&self) -> &str {
        &self.resolution_domain
    }

    /// Canonical qualified package name.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Deterministic fingerprint of this package identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.fold(Fingerprint::new(self.schema.tag())).finish()
    }

    pub(super) fn fold(&self, fp: Fingerprint) -> Fingerprint {
        fp.discriminant("schema", self.schema.tag())
            .field("project", &self.project_identity)
            .field("domain", &self.resolution_domain)
            .field("name", &self.qualified_name)
    }
}

/// How a package, class, or role was written at the declaration site.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageDeclarationForm {
    /// `package Foo;` introducing source-order context.
    PackageStatement,
    /// `package Foo { ... }` owning a lexical region.
    PackageBlock,
    /// Native or object-system `class` declaration.
    Class,
    /// Native or object-system `role` declaration.
    Role,
}

impl PackageDeclarationForm {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::PackageStatement => "package-statement",
            Self::PackageBlock => "package-block",
            Self::Class => "class",
            Self::Role => "role",
        }
    }

    /// Entity kind this form may declare.
    #[must_use]
    pub const fn entity_kind_tag(self) -> &'static str {
        match self {
            Self::PackageStatement | Self::PackageBlock => "package",
            Self::Class => "class",
            Self::Role => "role",
        }
    }
}

/// Source tier owning a package declaration or resolved dependency.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageSourceTier {
    /// Declared in the current workspace.
    Workspace,
    /// Resolved from an accepted dependency source.
    ResolvedDependency,
    /// Vendored or bundled with the project.
    Bundled,
    /// Produced by a generator rather than authored source.
    Generated,
}

impl PackageSourceTier {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::ResolvedDependency => "resolved-dependency",
            Self::Bundled => "bundled",
            Self::Generated => "generated",
        }
    }

    /// Whether this tier can carry editable workspace authority.
    #[must_use]
    pub const fn allows_editable_authority(self) -> bool {
        matches!(self, Self::Workspace)
    }
}

/// Edit authority for one declaration contribution.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageEditAuthority {
    /// The contribution is in editable workspace source.
    Editable,
    /// The contribution is readable but not an edit subject.
    ReadOnly,
    /// The contribution is generated and not an edit subject.
    NotEditable,
}

impl PackageEditAuthority {
    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Editable => "editable",
            Self::ReadOnly => "read-only",
            Self::NotEditable => "not-editable",
        }
    }
}

/// Exact name-token identity for a package declaration.
///
/// Identity is the canonical name digest, not a byte offset, path, or
/// traversal order. The optional span is evidence and is excluded from
/// [`PackageDeclarationId`] fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageNameAnchor {
    name_spelling: String,
    name_digest: String,
    span: Option<SourceAnchor>,
}

impl PackageNameAnchor {
    /// Construct a name anchor from spelling, digest, and optional span evidence.
    ///
    /// # Errors
    /// Returns a contract error when spelling or digest is empty, or when the
    /// span is inverted.
    pub fn new(
        name_spelling: impl Into<String>,
        name_digest: impl Into<String>,
        span: Option<SourceAnchor>,
    ) -> Result<Self, PackageTopologyContractError> {
        let anchor = Self {
            name_spelling: require_identity("PackageNameAnchor.name_spelling", name_spelling)?,
            name_digest: require_identity("PackageNameAnchor.name_digest", name_digest)?,
            span,
        };
        anchor.validate()?;
        Ok(anchor)
    }

    /// Validate spelling, digest, and optional span after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        require_identity("PackageNameAnchor.name_spelling", self.name_spelling.as_str())?;
        require_identity("PackageNameAnchor.name_digest", self.name_digest.as_str())?;
        if let Some(span) = &self.span {
            validate_span(span)?;
        }
        Ok(())
    }

    /// Source spelling of the package name token.
    #[must_use]
    pub fn name_spelling(&self) -> &str {
        &self.name_spelling
    }

    /// Canonical digest of the name token text.
    #[must_use]
    pub fn name_digest(&self) -> &str {
        &self.name_digest
    }

    /// Exact name span evidence, when recovered.
    #[must_use]
    pub fn span(&self) -> Option<SourceAnchor> {
        self.span
    }

    pub(super) fn fold_identity(&self, fp: Fingerprint) -> Fingerprint {
        fp.field("name-spelling", &self.name_spelling).field("name-digest", &self.name_digest)
    }
}

/// Full declaration-region identity for one contribution.
///
/// The digest is identity (header/subtree text). The optional span is
/// evidence and is excluded from [`PackageDeclarationId`] fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageDeclarationAnchor {
    digest: String,
    span: Option<SourceAnchor>,
}

impl PackageDeclarationAnchor {
    /// Construct a declaration-region anchor.
    ///
    /// # Errors
    /// Returns a contract error when the digest is empty or the span is inverted.
    pub fn new(
        digest: impl Into<String>,
        span: Option<SourceAnchor>,
    ) -> Result<Self, PackageTopologyContractError> {
        let anchor =
            Self { digest: require_identity("PackageDeclarationAnchor.digest", digest)?, span };
        anchor.validate()?;
        Ok(anchor)
    }

    /// Validate digest and optional span after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        require_identity("PackageDeclarationAnchor.digest", self.digest.as_str())?;
        if let Some(span) = &self.span {
            validate_span(span)?;
        }
        Ok(())
    }

    /// Canonical digest of the declaration region.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Exact declaration span evidence, when recovered.
    #[must_use]
    pub fn span(&self) -> Option<SourceAnchor> {
        self.span
    }

    pub(super) fn fold_identity(&self, fp: Fingerprint) -> Fingerprint {
        fp.field("declaration-digest", &self.digest)
    }
}

/// Source-local identity of one package declaration contribution.
///
/// Distinct from [`PackageId`]: reopened declarations share a package identity
/// and keep distinct declaration identities. Logical source, content revision,
/// and generation participate; byte offsets do not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageDeclarationId {
    schema: PackageTopologySchema,
    logical_source_id: String,
    content_revision: String,
    source_generation: String,
    declaration_form: PackageDeclarationForm,
    name_digest: Option<String>,
    contribution_ordinal: u32,
}

impl PackageDeclarationId {
    /// Construct a declaration contribution identity.
    ///
    /// `name_digest` is omitted for a recovered declaration whose exact name
    /// token could not be established. `contribution_ordinal` disambiguates
    /// several declarations in one logical source.
    ///
    /// # Errors
    /// Returns [`PackageTopologyContractError::EmptyIdentityField`] when a
    /// required identity slot is empty.
    pub fn new(
        logical_source_id: impl Into<String>,
        content_revision: impl Into<String>,
        source_generation: impl Into<String>,
        declaration_form: PackageDeclarationForm,
        name_digest: Option<String>,
        contribution_ordinal: u32,
    ) -> Result<Self, PackageTopologyContractError> {
        let identity = Self {
            schema: PackageTopologySchema::V1,
            logical_source_id: require_identity(
                "PackageDeclarationId.logical_source_id",
                logical_source_id,
            )?,
            content_revision: require_identity(
                "PackageDeclarationId.content_revision",
                content_revision,
            )?,
            source_generation: require_identity(
                "PackageDeclarationId.source_generation",
                source_generation,
            )?,
            declaration_form,
            name_digest: match name_digest {
                Some(digest) => Some(require_identity("PackageDeclarationId.name_digest", digest)?),
                None => None,
            },
            contribution_ordinal,
        };
        identity.validate()?;
        Ok(identity)
    }

    /// Validate identity slots after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        require_identity(
            "PackageDeclarationId.logical_source_id",
            self.logical_source_id.as_str(),
        )?;
        require_identity("PackageDeclarationId.content_revision", self.content_revision.as_str())?;
        require_identity(
            "PackageDeclarationId.source_generation",
            self.source_generation.as_str(),
        )?;
        if let Some(digest) = &self.name_digest {
            require_identity("PackageDeclarationId.name_digest", digest.as_str())?;
        }
        Ok(())
    }

    /// Logical source instance owning this contribution.
    #[must_use]
    pub fn logical_source_id(&self) -> &str {
        &self.logical_source_id
    }

    /// Content revision of the logical source.
    #[must_use]
    pub fn content_revision(&self) -> &str {
        &self.content_revision
    }

    /// Accepted source generation of this contribution.
    #[must_use]
    pub fn source_generation(&self) -> &str {
        &self.source_generation
    }

    /// Declaration form written at the site.
    #[must_use]
    pub fn declaration_form(&self) -> PackageDeclarationForm {
        self.declaration_form
    }

    /// Canonical name-token digest, when the exact name was recovered.
    #[must_use]
    pub fn name_digest(&self) -> Option<&str> {
        self.name_digest.as_deref()
    }

    /// Source-local ordinal distinguishing several declarations in one file.
    #[must_use]
    pub fn contribution_ordinal(&self) -> u32 {
        self.contribution_ordinal
    }

    /// Deterministic fingerprint of this declaration identity.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.fold(Fingerprint::new(PACKAGE_TOPOLOGY_SCHEMA_TAG)).finish()
    }

    pub(super) fn fold(&self, fp: Fingerprint) -> Fingerprint {
        let fp = fp
            .discriminant("schema", self.schema.tag())
            .field("logical-source", &self.logical_source_id)
            .field("revision", &self.content_revision)
            .field("generation", &self.source_generation)
            .discriminant("form", self.declaration_form.tag())
            .field("ordinal", &self.contribution_ordinal.to_string());
        match &self.name_digest {
            Some(digest) => {
                fp.discriminant("name-digest", "some").field("name-digest-value", digest)
            }
            None => fp.discriminant("name-digest", "none"),
        }
    }
}

impl PackageSourceTier {
    pub(super) fn fold(self, fp: Fingerprint) -> Fingerprint {
        fp.discriminant("source-tier", self.tag())
    }
}

impl PackageEditAuthority {
    pub(super) fn fold(self, fp: Fingerprint) -> Fingerprint {
        fp.discriminant("edit-authority", self.tag())
    }
}

impl PackageNameAnchor {
    pub(super) fn fold_evidence(&self, fp: Fingerprint) -> Fingerprint {
        fold_optional_span(self.fold_identity(fp), "name-span", self.span.as_ref())
    }
}

impl PackageDeclarationAnchor {
    pub(super) fn fold_evidence(&self, fp: Fingerprint) -> Fingerprint {
        fold_optional_span(self.fold_identity(fp), "declaration-span", self.span.as_ref())
    }
}
