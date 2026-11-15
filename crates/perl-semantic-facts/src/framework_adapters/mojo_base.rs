//! Shared mojo base contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::mojo_base`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::framework::{AdapterId, ModuleActivationIdentity};
use crate::{Confidence, SourceGeneration};

/// Framework name handled by this adapter.
pub const MOJO_BASE_FRAMEWORK_NAME: &str = "Mojo::Base";

/// Reviewed supported version range for the Mojo::Base activation profile.
///
/// Covers the Mojolicious 8.x and 9.x series, where the reviewed import forms
/// (`-base`, literal parent, `-signatures`) are stable. The workspace fixture
/// `test_corpus/real_projects/mojolicious_skeleton` carries the dist version
/// `9.34` (`lib/Mojolicious.pm`). A Mojolicious 10.x release has not been
/// reviewed.
pub const MOJO_BASE_VERSION_CONSTRAINT: &str = ">=8.0.0,<10.0.0";

/// Provisional adapter identity.
///
/// The generic registry (#6821) owns final identity assignment; this stable
/// value is reserved for Mojo::Base so shadow receipts remain comparable
/// across the registry extraction.
pub const MOJO_BASE_ADAPTER_ID: AdapterId = AdapterId(0x004D_4F42);

/// Versioned identity of the reviewed Mojo::Base activation profile.
///
/// The profile covers the reviewed static import forms — `-base`, a literal
/// parent (quoted or bareword module spelling), and the `-signatures` import
/// option — as established by the `Mojo::Base::import` contract mirrored in
/// the workspace skeleton fixture.
pub const MOJO_BASE_PROFILE_VERSION: &str = "mojo-base.profile.1.v1";

/// Reviewed versioned-descriptor schema revision for this adapter. Tracks
/// [`FRAMEWORK_ADAPTER_SCHEMA_VERSION`](crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION):
/// the descriptor travels on the adapter SDK wire, whose version 2 carries
/// the #8921 route-family fact kinds.
pub const MOJO_BASE_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Parent selection from the activating `use Mojo::Base ...;` import.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MojoBaseParentSelection {
    /// No parent selection was made yet.
    #[default]
    None,
    /// `use Mojo::Base -base;` — the caller inherits from `Mojo::Base`.
    Base,
    /// `use Mojo::Base 'Parent';` — a literal parent spelling.
    Literal(String),
    /// `-strict` (or no arguments): strict/warnings-only import that does not
    /// activate inheritance or `has`.
    StrictOnly,
    /// Computed parent expression — an explicit dynamic boundary.
    Dynamic {
        /// Why the parent expression is dynamic.
        reason: String,
    },
    /// Recovered or contradictory import spelling — the source could not be
    /// interpreted as one reviewed activation form.
    Malformed {
        /// Why the import spelling could not be interpreted.
        reason: String,
    },
}

/// Import evidence extracted from the activating `use Mojo::Base ...;`
/// argument list, in parser token form.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MojoBaseImportEvidence {
    /// Parent selection (`-base`, literal parent, strict-only, dynamic,
    /// malformed).
    pub parent: MojoBaseParentSelection,
    /// Whether the reviewed `-signatures` import option is present.
    pub signatures: bool,
    /// Import options this profile does not review; the activation carries an
    /// explicit unsupported-profile state for them.
    pub unmodeled_options: Vec<String>,
}

/// Load-bearing site identity for one `use Mojo::Base ...;` activation site.
///
/// Carries the owning package, the import statement's source interval, the
/// literal parent's source range when present, and the source generation the
/// site was extracted from — the fields #9681 requires to stay load-bearing
/// on the activation profile.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseSiteAnchor {
    /// Caller package at the activating import (activation scope).
    pub package: Option<String>,
    /// Import statement source interval, in bytes.
    pub span_start_byte: u32,
    /// End of the import statement interval, in bytes.
    pub span_end_byte: u32,
    /// Literal parent spelling's source range (start, end in bytes), when the
    /// parent is a literal and the range was located in source.
    pub parent_range: Option<(u32, u32)>,
    /// Source generation the site was extracted from; exact activation
    /// requires the detection generation to match it.
    pub source_generation: SourceGeneration,
}

impl MojoBaseSiteAnchor {
    /// Construct one site anchor from extraction evidence.
    #[must_use]
    pub fn new(
        package: Option<String>,
        span_start_byte: u32,
        span_end_byte: u32,
        parent_range: Option<(u32, u32)>,
        source_generation: SourceGeneration,
    ) -> Self {
        Self { package, span_start_byte, span_end_byte, parent_range, source_generation }
    }
}

/// Typed outcome of one Mojo::Base activation/profile evaluation (#9681).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MojoBaseActivationOutcome {
    /// Exact `-base` activation under the reviewed profile.
    ExactBaseActivation,
    /// Exact literal-parent activation under the reviewed profile.
    ExactLiteralParentActivation {
        /// Literal parent spelling.
        parent: String,
    },
    /// Complete evidence establishes that no activation form is present.
    AbsentWithCompleteEvidence {
        /// Bounded absence explanation.
        reason: String,
    },
    /// The observed module version or import profile is not reviewed.
    UnsupportedVersionOrProfile {
        /// Bounded unsupported explanation.
        reason: String,
    },
    /// The Mojo::Base module is missing, unresolved, or unavailable.
    MissingOrUnavailableModule {
        /// Bounded unavailability explanation.
        reason: String,
    },
    /// More than one module identity matched the selector.
    AmbiguousOrConflictingModule {
        /// Bounded conflict explanation.
        reason: String,
    },
    /// The detection or site evidence is stale or incomplete for reuse.
    StaleOrIncompleteInput {
        /// Bounded staleness explanation.
        reason: String,
    },
    /// The parent selection is computed or otherwise unmodeled.
    DynamicOrUnmodeledParent {
        /// Bounded dynamic-boundary explanation.
        reason: String,
    },
    /// The import source was recovered or malformed.
    RecoveredOrMalformedSource {
        /// Bounded recovery explanation.
        reason: String,
    },
    /// The detection instrument failed (cancellation, budget, internal).
    InstrumentFailure {
        /// Bounded failure explanation.
        reason: String,
    },
}

impl MojoBaseActivationOutcome {
    /// Whether this outcome is an exact activation.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self, Self::ExactBaseActivation | Self::ExactLiteralParentActivation { .. })
    }
}

/// Typed registry-backed Mojo::Base activation/profile facts for one
/// activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoBaseActivationFacts {
    /// Typed activation outcome.
    pub outcome: MojoBaseActivationOutcome,
    /// Versioned identity of the reviewed profile that produced these facts.
    pub profile_version: &'static str,
    /// Owning package at the activating import.
    pub package: Option<String>,
    /// Import statement source interval, in bytes.
    pub source_interval: (u32, u32),
    /// Literal parent spelling's source range when present.
    pub parent_range: Option<(u32, u32)>,
    /// Exact root/scope identity of the observation receipt, when the
    /// detection carries a current input identity.
    pub scope_identity: Option<String>,
    /// Exact project environment identity of the observation receipt, when
    /// the detection carries a current input identity.
    pub environment_identity: Option<String>,
    /// Resolved module/source identity that made the detection current.
    pub resolved_module: Option<ModuleActivationIdentity>,
    /// Observed supported framework version (exact activations only).
    pub framework_version: String,
    /// Detection confidence carried onto the profile.
    pub confidence: Confidence,
    /// Source generation that produced this activation evidence.
    pub source_generation: SourceGeneration,
    /// Reviewed `-signatures` import option state.
    pub signatures: bool,
    /// Import options outside the reviewed profile.
    pub unmodeled_options: Vec<String>,
    /// Bounded limitations of these facts.
    pub limitations: Vec<String>,
}

impl MojoBaseActivationFacts {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        outcome: MojoBaseActivationOutcome,
        profile_version: &'static str,
        package: Option<String>,
        source_interval: (u32, u32),
        parent_range: Option<(u32, u32)>,
        scope_identity: Option<String>,
        environment_identity: Option<String>,
        resolved_module: Option<ModuleActivationIdentity>,
        framework_version: String,
        confidence: Confidence,
        source_generation: SourceGeneration,
        signatures: bool,
        unmodeled_options: Vec<String>,
        limitations: Vec<String>,
    ) -> Self {
        Self {
            outcome,
            profile_version,
            package,
            source_interval,
            parent_range,
            scope_identity,
            environment_identity,
            resolved_module,
            framework_version,
            confidence,
            source_generation,
            signatures,
            unmodeled_options,
            limitations,
        }
    }
}

impl MojoBaseActivationFacts {
    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.outcome.is_exact()
    }
}
