//! Versioned, transport-neutral, immutable `workspace_lifecycle_status.v1`.
//!
//! Contract only (#16895). This module defines vocabulary, identity,
//! validation, bounds, privacy, and deterministic projection inputs. It
//! reads no live workspace state and changes no CLI, protocol, editor,
//! provider, reload, cache, or repair behavior.
//!
//! ```text
//! canonical domain observations
//! → workspace_lifecycle_status.v1 typed contract
//! → later assembler (#16896) and presentation leaves
//! ```
//!
//! Live domain authorities remain external: root lifecycle #7932,
//! configuration #6736/#7900, reload #7893, readiness/provider #3099,
//! storage #7892, hydration #7454, environment/trust #4832/#4905, reasons
//! and operations #4242/#4839. This module does not import those types and
//! does not define sibling status vocabularies for assembler, CLI, protocol,
//! VS Code, or support leaves.

mod envelope;
mod ids;
mod vocab;

pub use envelope::{
    ReadinessObservation, RepairProjection, StatusDimension, StatusMetadata,
    WorkspaceLifecycleStatusParts, WorkspaceLifecycleStatusRow, WorkspaceLifecycleStatusRowParts,
    WorkspaceLifecycleStatusV1,
};
pub use ids::{
    DimensionProvenance, LogicalToken, ProducerIdentity, StatusFingerprint,
    WorkspaceLifecycleSubject,
};
pub use vocab::{
    Completeness, ConfigurationState, HydrationState, InstrumentState, LocalDiagnosticAction,
    PrimaryActionKind, PrimaryActionPriorityPolicy, PrivacyClass, ProviderPosture, ReadinessState,
    ResponsibleOwnerRole, RootLifecycleState, StorageState, WatchReloadState,
};

use serde::{Deserialize, Deserializer, Serialize};

/// Current schema version for the `workspace_lifecycle_status.v1` envelope.
pub const WORKSPACE_LIFECYCLE_STATUS_SCHEMA_VERSION_V1: u32 = 1;

/// Maximum status rows in one envelope.
pub const MAX_STATUS_ROWS: usize = 32;
/// Maximum reason codes per row.
pub const MAX_REASONS_PER_ROW: usize = 16;
/// Maximum limitation codes per row.
pub const MAX_LIMITATIONS_PER_ROW: usize = 16;
/// Maximum operation/evidence links per row.
pub const MAX_OPERATION_LINKS: usize = 16;
/// Maximum logical-token bytes.
pub const MAX_LOGICAL_TOKEN_BYTES: usize = 128;
/// Maximum retained history entries (count only; bodies are never stored).
pub const MAX_HISTORY_ENTRIES: u16 = 8;

const FINGERPRINT_DOMAIN: &[u8] = b"perl-lsp:workspace-lifecycle-status-fingerprint:v1";

/// A versioned schema marker for `workspace_lifecycle_status.v1`.
///
/// Deserialization rejects any version this build does not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct WorkspaceLifecycleStatusSchemaVersion(
    /// Raw integer schema version. Only `1` is supported.
    pub u32,
);

impl WorkspaceLifecycleStatusSchemaVersion {
    /// The current `workspace_lifecycle_status.v1` schema version.
    pub const V1: Self = Self(WORKSPACE_LIFECYCLE_STATUS_SCHEMA_VERSION_V1);

    /// Returns `true` if this version is one the current runtime recognizes.
    #[must_use]
    pub const fn is_supported(self) -> bool {
        self.0 == WORKSPACE_LIFECYCLE_STATUS_SCHEMA_VERSION_V1
    }

    /// Unwrap the raw integer version.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for WorkspaceLifecycleStatusSchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "workspace_lifecycle_status.v{}", self.0)
    }
}

impl<'de> Deserialize<'de> for WorkspaceLifecycleStatusSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u32::deserialize(deserializer)?;
        let version = Self(raw);
        if version.is_supported() {
            Ok(version)
        } else {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Unsigned(u64::from(raw)),
                &"a supported workspace_lifecycle_status schema version (currently 1)",
            ))
        }
    }
}

/// Typed validation refusal for `workspace_lifecycle_status.v1`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValidationError {
    /// The schema version is not one this build supports.
    #[error("unsupported workspace_lifecycle_status schema version {version}")]
    UnsupportedSchemaVersion {
        /// Unsupported version value.
        version: u32,
    },
    /// A logical token was empty.
    #[error("logical token must be non-empty")]
    EmptyLogicalToken,
    /// A logical token exceeded the mechanical byte bound.
    #[error("logical token length {length} exceeds limit {limit}")]
    LogicalTokenTooLong {
        /// Observed length.
        length: usize,
        /// Mechanical limit.
        limit: usize,
    },
    /// A logical token used a character outside the admitted charset.
    #[error("logical token contains a character outside the admitted charset")]
    InvalidLogicalTokenCharset,
    /// Source, secrets, private paths, environment values, or logs entered public output.
    #[error("hostile path, source, secret, environment, or log content is refused")]
    HostilePublicContent,
    /// A bounded collection exceeded its mechanical limit.
    #[error("{what} length {length} exceeds limit {limit}")]
    BoundExceeded {
        /// Bound name.
        what: &'static str,
        /// Observed length.
        length: usize,
        /// Mechanical limit.
        limit: usize,
    },
    /// Reasons, limitations, or links were not canonically ordered.
    #[error("{what} must be canonically ordered and duplicate-free")]
    TokenOrder {
        /// Collection name.
        what: &'static str,
    },
    /// Envelope contained no rows.
    #[error("workspace_lifecycle_status envelope must contain at least one row")]
    EmptyEnvelope,
    /// Rows were not canonically ordered by root then runtime generation.
    #[error("status rows must be canonically ordered by root and runtime generation")]
    RowOrder,
    /// Two rows claimed the same root/runtime generation.
    #[error("duplicate root/runtime generation in one envelope")]
    DuplicateRootGeneration,
    /// Observations from different application sessions were joined.
    #[error("cross-session observations cannot join")]
    CrossSessionJoin,
    /// Observations from different roots were joined.
    #[error("cross-root observations cannot join")]
    CrossRootJoin,
    /// Current observations from mismatched generations were joined.
    #[error("cross-generation observations cannot join")]
    CrossGenerationJoin,
    /// A stale configuration observation did not actually mismatch generations.
    #[error("stale configuration requires a generation mismatch")]
    StaleRequiresGenerationMismatch,
    /// Completeness contradicted explicit not-proven or instrument-failed dimensions.
    #[error("completeness contradicts dimension observation state")]
    CompletenessContradiction,
    /// Instrument state contradicted explicit instrument-failed dimensions.
    #[error("instrument state contradicts dimension observation state")]
    InstrumentStateContradiction,
    /// Stored fingerprint was not the fingerprint of the payload.
    #[error("status fingerprint does not match its canonical identity parts")]
    FingerprintMismatch,
}

impl ValidationError {
    /// Stable error family token for tests and later projection leaves.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::UnsupportedSchemaVersion { .. } => "unsupported_schema_version",
            Self::EmptyLogicalToken => "empty_logical_token",
            Self::LogicalTokenTooLong { .. } => "logical_token_too_long",
            Self::InvalidLogicalTokenCharset => "invalid_logical_token_charset",
            Self::HostilePublicContent => "hostile_public_content",
            Self::BoundExceeded { .. } => "bound_exceeded",
            Self::TokenOrder { .. } => "token_order",
            Self::EmptyEnvelope => "empty_envelope",
            Self::RowOrder => "row_order",
            Self::DuplicateRootGeneration => "duplicate_root_generation",
            Self::CrossSessionJoin => "cross_session_join",
            Self::CrossRootJoin => "cross_root_join",
            Self::CrossGenerationJoin => "cross_generation_join",
            Self::StaleRequiresGenerationMismatch => "stale_requires_generation_mismatch",
            Self::CompletenessContradiction => "completeness_contradiction",
            Self::InstrumentStateContradiction => "instrument_state_contradiction",
            Self::FingerprintMismatch => "fingerprint_mismatch",
        }
    }
}

#[cfg(test)]
mod tests;
