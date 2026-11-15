//! Shared mojolicious contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::mojolicious`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::framework::{AdapterId, ModuleActivationIdentity};

use crate::{Confidence, SourceGeneration};

/// Framework name handled by this adapter.
pub const MOJOLICIOUS_FRAMEWORK_NAME: &str = "Mojolicious";

/// Module selector owned by the Mojolicious::Lite profile.
pub const MOJOLICIOUS_LITE_MODULE: &str = "Mojolicious::Lite";

/// Literal `Mojo::Base` parent spelling that makes a package a full
/// Mojolicious application.
pub const MOJOLICIOUS_APPLICATION_PARENT: &str = "Mojolicious";

/// Literal `Mojo::Base` parent spelling that makes a package a Mojolicious
/// controller.
pub const MOJOLICIOUS_CONTROLLER_PARENT: &str = "Mojolicious::Controller";

/// Reviewed supported version range for the Mojolicious activation profile.
///
/// Matches the reviewed Mojo::Base range: `Mojo::Base`, `Mojolicious`, and
/// `Mojolicious::Lite` all ship in the one Mojolicious distribution and carry
/// the one dist version. The workspace fixture
/// `test_corpus/real_projects/mojolicious_skeleton` carries `9.34`
/// (`lib/Mojolicious.pm`). A Mojolicious 10.x release has not been reviewed.
pub const MOJOLICIOUS_VERSION_CONSTRAINT: &str = ">=8.0.0,<10.0.0";

/// Provisional adapter identity.
///
/// The generic registry (#6821) owns final identity assignment; this stable
/// value is reserved for Mojolicious so shadow receipts remain comparable
/// across the registry extraction.
pub const MOJOLICIOUS_ADAPTER_ID: AdapterId = AdapterId(0x004D_4F4A);

/// Versioned identity of the reviewed Mojolicious activation profile.
pub const MOJOLICIOUS_PROFILE_VERSION: &str = "mojolicious.profile.1.v1";

/// Reviewed versioned-descriptor schema revision for this adapter.
pub const MOJOLICIOUS_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Import selection from the activating `use Mojolicious::Lite ...;` import.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MojoliciousLiteImportSelection {
    /// `use Mojolicious::Lite;` — the reviewed default import form.
    #[default]
    Default,
    /// `use Mojolicious::Lite -signatures;` — the reviewed signatures form.
    Signatures,
    /// `use Mojolicious::Lite ();` — an explicit empty import list. Perl
    /// calls no `import`, so the Lite DSL is never installed and the package
    /// owns no Lite role even though the module is loaded.
    ImportSuppressed,
    /// Computed import argument — an explicit dynamic boundary.
    Dynamic {
        /// Bounded dynamic-boundary explanation.
        reason: String,
    },
    /// Recovered or contradictory import spelling.
    Malformed {
        /// Bounded recovery explanation.
        reason: String,
    },
}

/// The version requirement one `use Mojolicious::Lite ...;` statement carries,
/// recorded by where each part of it was recovered from.
///
/// The parser folds only the first numeric component of a `use MODULE VERSION`
/// requirement into the module name and emits any continuation as ordinary
/// argument tokens. `use Mojolicious::Lite 9.34.1;` and
/// `use Mojolicious::Lite 9.34 .1;` therefore arrive identically — module name
/// `Mojolicious::Lite 9.34`, tokens `.` and `1` — even though the first is one
/// three-part version and the second is a two-part version followed by a
/// separate import argument. Nothing in the token stream separates them, so
/// the statement's own source text has to.
///
/// Both fields come from the extractor, which holds that source text:
/// `folded_into_module` is the prefix the parser moved into the module name,
/// and `as_spelled` is the complete contiguous requirement as actually
/// written. A continuation token is a version component only while
/// `as_spelled` accounts for it; anything beyond that is an import argument
/// and is classified as one.
///
/// Both fields absent means the statement carries no numeric requirement — a
/// v-string requirement reaches the argument list whole instead.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MojoliciousLiteVersionRequirement {
    /// The requirement prefix the parser folded into the module name.
    pub folded_into_module: Option<String>,
    /// The complete contiguous requirement as spelled in the source statement.
    pub as_spelled: Option<String>,
}

impl MojoliciousLiteVersionRequirement {
    /// A statement carrying no parser-folded numeric version requirement.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The requirement recovered from a statement, given the prefix the parser
    /// folded into the module name and the complete contiguous spelling.
    #[must_use]
    pub fn new(folded_into_module: Option<String>, as_spelled: Option<String>) -> Self {
        Self { folded_into_module, as_spelled }
    }
}

/// Import evidence extracted from the activating `use Mojolicious::Lite ...;`
/// argument list, in parser token form.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MojoliciousLiteImportEvidence {
    /// Reviewed import selection.
    pub selection: MojoliciousLiteImportSelection,
    /// Whether the reviewed `-signatures` import option is present.
    pub signatures: bool,
    /// Import options this profile does not review; the activation carries an
    /// explicit unsupported-profile state for them.
    pub unmodeled_options: Vec<String>,
    /// Version required by the import itself (`use Mojolicious::Lite 9.34;`),
    /// normalized without any `v` prefix.
    ///
    /// Perl calls `Mojolicious::Lite->VERSION(REQUIRED)` for this form, which
    /// dies unless the installed version satisfies it, so an import whose
    /// requirement the environment does not meet never activates at runtime
    /// and must not own a role here either.
    pub source_version_requirement: Option<String>,
}

/// Load-bearing site identity for one Mojolicious::Lite activation site.
///
/// Carries the owning package, the activating statement's source interval, and
/// the source generation the site was extracted from.
///
/// There is deliberately no parent-range field: a Lite import has no literal
/// parent spelling, and the parent-derived Application/Controller roles carry
/// their range through [`MojoBaseActivationFacts`] instead of through this
/// type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoliciousSiteAnchor {
    /// Caller package at the activating import (activation scope).
    pub package: Option<String>,
    /// Import statement source interval start, in bytes.
    pub span_start_byte: u32,
    /// Import statement source interval end, in bytes.
    pub span_end_byte: u32,
    /// Source generation the site was extracted from; exact activation
    /// requires the detection generation to match it.
    pub source_generation: SourceGeneration,
}

impl MojoliciousSiteAnchor {
    /// Construct one site anchor from extraction evidence.
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

/// The Mojolicious role a package currently owns.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MojoliciousRole {
    /// `use Mojolicious::Lite;` — a Lite application.
    LiteApplication,
    /// `use Mojo::Base 'Mojolicious';` — a full application class.
    Application,
    /// `use Mojo::Base 'Mojolicious::Controller';` — a controller class.
    Controller,
}

/// Typed outcome of one Mojolicious activation/ownership evaluation (#9688).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MojoliciousActivationOutcome {
    /// Exact activation under the reviewed profile.
    ExactActivation {
        /// The role the activating package owns.
        role: MojoliciousRole,
    },
    /// Complete evidence establishes that no Mojolicious role is present.
    AbsentWithCompleteEvidence {
        /// Bounded absence explanation.
        reason: String,
    },
    /// The observed module version or import profile is not reviewed.
    UnsupportedVersionOrProfile {
        /// Bounded unsupported explanation.
        reason: String,
    },
    /// The activating module is missing, unresolved, or unavailable.
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
    /// The activating import or parent is computed or otherwise unmodeled.
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

impl MojoliciousActivationOutcome {
    /// The exact role this outcome establishes, if any.
    #[must_use]
    pub fn role(&self) -> Option<MojoliciousRole> {
        match self {
            Self::ExactActivation { role } => Some(*role),
            _ => None,
        }
    }

    /// Whether this outcome is an exact activation.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.role().is_some()
    }
}

/// Typed registry-backed Mojolicious activation/ownership facts for one
/// activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MojoliciousActivationFacts {
    /// Typed activation outcome.
    pub outcome: MojoliciousActivationOutcome,
    /// Versioned identity of the reviewed profile that produced these facts.
    pub profile_version: &'static str,
    /// Owning package at the activating import.
    pub package: Option<String>,
    /// Activating statement source interval, in bytes.
    pub source_interval: (u32, u32),
    /// Literal parent spelling's source range for parent-derived roles.
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

impl MojoliciousActivationFacts {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        outcome: MojoliciousActivationOutcome,
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

impl MojoliciousActivationFacts {
    /// The exact role these facts establish, if any.
    #[must_use]
    pub fn role(&self) -> Option<MojoliciousRole> {
        self.outcome.role()
    }

    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.outcome.is_exact()
    }
}
