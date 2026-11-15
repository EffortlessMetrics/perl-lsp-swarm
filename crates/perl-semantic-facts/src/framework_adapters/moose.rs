//! Shared moose contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::moose`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::SourceGeneration;
use crate::framework::AdapterId;

/// Reviewed Moose module/version family.
///
/// The current reviewed primary documentation carries Moose 2.4000. Moose 3.x
/// is deliberately outside this profile until separately reviewed.
pub const MOOSE_VERSION_CONSTRAINT: &str = ">=2.0.0,<3.0.0";

/// Stable identity of the reviewed activation contract.
pub const MOOSE_ACTIVATION_PROFILE_VERSION: &str = "moose.activation.2.v1";

/// Provisional adapter identity for `use Moose`.
pub const MOOSE_CLASS_ADAPTER_ID: AdapterId = AdapterId(0x004D_4F53);

/// Provisional adapter identity for `use Moose::Role`.
pub const MOOSE_ROLE_ADAPTER_ID: AdapterId = AdapterId(0x004D_5253);

/// Current descriptor schema revision.
pub const MOOSE_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Exact activation kind established by the imported module.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MooseActivationKind {
    /// `use Moose` class activation.
    Class,
    /// `use Moose::Role` role activation.
    Role,
}

impl MooseActivationKind {
    /// Exact module selector owned by this activation kind.
    #[must_use]
    pub const fn module_name(self) -> &'static str {
        match self {
            Self::Class => "Moose",
            Self::Role => "Moose::Role",
        }
    }

    /// Stable human-readable adapter name.
    #[must_use]
    pub const fn adapter_name(self) -> &'static str {
        match self {
            Self::Class => "moose-class",
            Self::Role => "moose-role",
        }
    }

    /// Stable adapter identity associated with this activation kind.
    #[must_use]
    pub const fn adapter_id(self) -> AdapterId {
        match self {
            Self::Class => MOOSE_CLASS_ADAPTER_ID,
            Self::Role => MOOSE_ROLE_ADAPTER_ID,
        }
    }
}

/// Source-side import disposition retained with an activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MooseImportDisposition {
    /// Reviewed default import, optionally carrying a static version
    /// requirement.
    Exact,
    /// The exact module was imported with arguments outside this activation
    /// profile. The site remains inspectable but cannot establish exact
    /// activation. An empty `arguments` list is the explicit-empty import
    /// spelling (`use Moose ();`): the module loads without applying its
    /// activation sugar, so the spelling is retained as inspectable evidence
    /// rather than silently folded into [`MooseImportDisposition::Exact`].
    Unmodeled {
        /// Import arguments carried by the source spelling, normalized for
        /// inspection. An empty list records the explicit-empty import
        /// spelling (`use Moose ();`); the `<invalid-source-span>`
        /// placeholder records a site whose source text could not be
        /// extracted, so the real arguments were not captured.
        arguments: Vec<String>,
    },
}

impl MooseImportDisposition {
    /// Whether the import spelling is inside the reviewed activation profile.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact)
    }
}

/// Load-bearing source identity for one Moose import site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MooseSiteAnchor {
    /// Caller package at the activating import.
    pub package: Option<String>,
    /// First byte of the import statement.
    pub span_start_byte: u32,
    /// Byte immediately after the import statement.
    pub span_end_byte: u32,
    /// Source generation from which this site was extracted.
    pub source_generation: SourceGeneration,
}

impl MooseSiteAnchor {
    /// Construct a source anchor.
    #[must_use]
    pub fn new(
        package: Option<String>,
        span_start_byte: u32,
        span_end_byte: u32,
        source_generation: SourceGeneration,
    ) -> Self {
        Self { package, span_start_byte, span_end_byte, source_generation }
    }
}
