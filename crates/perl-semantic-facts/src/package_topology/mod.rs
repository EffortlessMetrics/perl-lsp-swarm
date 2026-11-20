//! Canonical package identity, declaration contribution, and resolution
//! states (#8913).
//!
//! This module is the schema foundation of #8197: it names what a package
//! *is*, which source contribution declared it, and how a package/module
//! request resolved. It materializes no graph, walks no AST, and changes no
//! provider or live package-graph index behavior.
//!
//! # Identity law
//!
//! [`PackageId`] is the logical package in one project and one resolution
//! domain. It is project identity + resolution-domain/root identity +
//! canonical qualified name. Package spelling alone is not a [`PackageId`].
//! URI/path hashes, byte offsets, traversal order, and a zero
//! [`crate::EntityId`] cannot mint package identity.
//!
//! [`PackageDeclarationId`] is one source-local declaration contribution.
//! Reopened declarations of one package share a [`PackageId`] and keep
//! distinct declaration identities. The same spelling in two roots is two
//! [`PackageId`]s. `main` is scoped by an explicit project/source domain.
//!
//! [`PackageResolution`] keeps workspace, resolved-dependency, missing,
//! ambiguous, dynamic, unsupported, and stale states distinct. Unresolved
//! and dynamic targets are typed states, not fictional package nodes, and
//! they do not collapse into [`crate::PackageKind::External`].
//!
//! Existing [`crate::PackageNode`] values and the workspace-core
//! file/offset `PackageId` remain separate propositions: a content/file/offset
//! hash or a graph node keyed by spelling is not this [`PackageId`].
//!
//! # Transport trust boundary
//!
//! `Serialize`/`Deserialize` is a wire shape, not an invariant guard:
//! deserializing untrusted JSON can produce records that `new()` would have
//! rejected. Any consumer accepting these types from a transport must call
//! `validate()` before reuse, and must treat a fingerprint match as a
//! candidate confirmed by structural equality.
//!
//! # Ownership fence
//!
//! This module owns no LSP protocol type, parser type, provider policy,
//! workspace storage, topology materialization, or module resolver. The
//! `architecture_fence` test enforces the import boundary mechanically.
//!
//! # Non-goals
//!
//! No live package-graph replacement, no relation migration, no inheritance
//! semantics, no LSP types, and no provider cutover. #8916 consumes these
//! types as relationship subjects; it does not redefine them.

mod contribution;
mod identity;
mod resolution;

#[cfg(test)]
mod tests;

pub use contribution::PackageDeclarationContribution;
pub use identity::{
    PackageDeclarationAnchor, PackageDeclarationForm, PackageDeclarationId, PackageEditAuthority,
    PackageId, PackageNameAnchor, PackageSourceTier,
};
pub use resolution::{PackageResolution, PackageResolutionRequest};

use serde::{Deserialize, Serialize};

use crate::semantic_identity::SemanticIdentityFingerprint;
use crate::{EntityKind, SourceAnchor};

/// Shorthand for the shared deterministic fingerprint accumulator.
type Fingerprint = SemanticIdentityFingerprint;

/// `package_topology.v1` schema version.
pub const PACKAGE_TOPOLOGY_SCHEMA_VERSION: u32 = 1;

/// Stable schema tag folded into every fingerprint in this contract.
pub const PACKAGE_TOPOLOGY_SCHEMA_TAG: &str = "package-topology.v1";

/// Error returned by package-topology contract validators.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageTopologyContractError {
    /// An identity field that must identify a subject is empty or whitespace.
    EmptyIdentityField(&'static str),
    /// A zero entity identity was used as if it identified a declaration.
    SentinelEntityId,
    /// The entity kind is not a package, class, or role declaration.
    UnsupportedEntityKind(&'static str),
    /// Declaration form and entity kind cannot be claimed together.
    KindFormMismatch {
        /// Entity kind tag.
        kind: &'static str,
        /// Declaration form tag.
        form: &'static str,
    },
    /// A typed status/tier/authority combination cannot be claimed together.
    ContradictoryStatus(&'static str),
    /// A source range is inverted or otherwise structurally impossible.
    MalformedRange {
        /// Inclusive start byte recorded on the anchor.
        start_byte: u32,
        /// Exclusive end byte recorded on the anchor.
        end_byte: u32,
    },
    /// A resolution state is missing required payload or carries a forbidden one.
    MalformedResolution(&'static str),
}

impl std::fmt::Display for PackageTopologyContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentityField(field) => {
                write!(f, "package topology field `{field}` must be a non-empty identity")
            }
            Self::SentinelEntityId => {
                write!(f, "a zero entity identity cannot mint package or declaration identity")
            }
            Self::UnsupportedEntityKind(kind) => {
                write!(f, "entity kind `{kind}` is not a package, class, or role declaration")
            }
            Self::KindFormMismatch { kind, form } => {
                write!(f, "entity kind `{kind}` cannot be declared as `{form}`")
            }
            Self::ContradictoryStatus(reason) => {
                write!(f, "contradictory package topology status: {reason}")
            }
            Self::MalformedRange { start_byte, end_byte } => {
                write!(f, "package topology range {start_byte}..{end_byte} is inverted")
            }
            Self::MalformedResolution(reason) => {
                write!(f, "malformed package resolution: {reason}")
            }
        }
    }
}

impl std::error::Error for PackageTopologyContractError {}

fn require_identity(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, PackageTopologyContractError> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(PackageTopologyContractError::EmptyIdentityField(field));
    }
    Ok(value)
}

fn validate_span(span: &SourceAnchor) -> Result<(), PackageTopologyContractError> {
    if span.start_byte > span.end_byte {
        return Err(PackageTopologyContractError::MalformedRange {
            start_byte: span.start_byte,
            end_byte: span.end_byte,
        });
    }
    Ok(())
}

fn fold_optional_span(fp: Fingerprint, label: &str, span: Option<&SourceAnchor>) -> Fingerprint {
    match span {
        None => fp.discriminant(label, "none"),
        Some(span) => fp
            .discriminant(label, "some")
            .field(&format!("{label}-file"), &span.file_id.0.to_string())
            .field(&format!("{label}-start"), &span.start_byte.to_string())
            .field(&format!("{label}-end"), &span.end_byte.to_string()),
    }
}

fn declaration_kind_tag(kind: EntityKind) -> Result<&'static str, PackageTopologyContractError> {
    match kind {
        EntityKind::Package => Ok("package"),
        EntityKind::Class => Ok("class"),
        EntityKind::Role => Ok("role"),
        EntityKind::Subroutine => {
            Err(PackageTopologyContractError::UnsupportedEntityKind("subroutine"))
        }
        EntityKind::Method => Err(PackageTopologyContractError::UnsupportedEntityKind("method")),
        EntityKind::Variable => {
            Err(PackageTopologyContractError::UnsupportedEntityKind("variable"))
        }
        EntityKind::Constant => {
            Err(PackageTopologyContractError::UnsupportedEntityKind("constant"))
        }
        EntityKind::Field => Err(PackageTopologyContractError::UnsupportedEntityKind("field")),
        EntityKind::Label => Err(PackageTopologyContractError::UnsupportedEntityKind("label")),
        EntityKind::Format => Err(PackageTopologyContractError::UnsupportedEntityKind("format")),
        EntityKind::Module => Err(PackageTopologyContractError::UnsupportedEntityKind("module")),
        EntityKind::GeneratedMember => {
            Err(PackageTopologyContractError::UnsupportedEntityKind("generated-member"))
        }
        EntityKind::ExternalSymbol => {
            Err(PackageTopologyContractError::UnsupportedEntityKind("external-symbol"))
        }
        EntityKind::Unknown => Err(PackageTopologyContractError::UnsupportedEntityKind("unknown")),
    }
}

/// Schema/producer version tag carried by every identity in this contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageTopologySchema {
    /// First version of the package topology identity contract (#8913).
    V1,
}

impl PackageTopologySchema {
    /// Stable textual tag used inside deterministic fingerprints.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::V1 => PACKAGE_TOPOLOGY_SCHEMA_TAG,
        }
    }
}
