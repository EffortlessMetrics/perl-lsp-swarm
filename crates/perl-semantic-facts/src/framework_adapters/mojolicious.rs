//! Registry-backed Mojolicious application and controller identity facts
//! (#9688).
//!
//! This adapter answers one question for one activation site: *which
//! Mojolicious role, if any, does this package currently own?*
//!
//! ```text
//! use Mojolicious::Lite;                        -> MojoliciousRole::LiteApplication
//! use Mojolicious::Lite -signatures;            -> MojoliciousRole::LiteApplication
//! package MyApp;
//! use Mojo::Base 'Mojolicious';                 -> MojoliciousRole::Application
//! package MyApp::Controller::Users;
//! use Mojo::Base 'Mojolicious::Controller';     -> MojoliciousRole::Controller
//! ```
//!
//! The two profiles reach that answer through deliberately different
//! evidence, because the source constructs are different:
//!
//! - **Lite** is its own module import, so it carries its own module
//!   selector (`Mojolicious::Lite`) and its own registry detection.
//! - **Full application / controller** are ordinary `use Mojo::Base
//!   '<parent>';` imports. Their evidence is already minted exactly once by
//!   the Mojo::Base adapter (#9681/#9682), so this adapter *consumes*
//!   [`MojoBaseActivationFacts`] and classifies the proven literal parent.
//!   It never reparses `Mojo::Base` import semantics, and it never mints a
//!   second Mojo::Base activation.
//!
//! Like its neighbours this is a shadow adapter: its output is
//! comparison/receipt material only and cannot become publication authority
//! until the registry-dispatch and shard-publication issues land.
//!
//! ## Claim boundary
//!
//! An exact role here means the *activation and ownership identity* is
//! current — not that the parent class was independently resolved. For the
//! full-application and controller profiles the parent is a literal spelling
//! proven in source; `Mojolicious` and `Mojolicious::Controller` are not
//! themselves resolved as modules by this profile, and that limitation
//! travels on every derived fact. This adapter emits no route, helper,
//! stash, template, or accessor facts and changes no provider behavior
//! (#9688 non-goals).

use crate::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    AdapterId, DetectionAbsenceReason, DetectionOutcome, ModuleActivationIdentity,
    ModuleSelectorEvaluation, ModuleSelectorOutcome, UnavailableReason,
};
use crate::framework_adapters::mojo_base::{MojoBaseActivationFacts, MojoBaseActivationOutcome};
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

const SHADOW_LIMITATIONS: &[&str] = &[
    "shadow adapter: comparison-only output; not publication authority",
    "route, helper, stash, template, and accessor semantics are intentionally not modeled \
     (#9688 non-goals)",
    "static controller namespaces are not proven by this profile: no route or configuration \
     evidence is consumed",
];

/// Limitation carried by every role derived from a Mojo::Base literal parent.
const DERIVED_PARENT_LIMITATION: &str = "role derived from a literal `Mojo::Base` parent spelling proven in source; the parent \
     module itself was not independently resolved by this profile";

/// Build the Mojolicious::Lite adapter descriptor.
///
/// The descriptor owns exactly one module selector, `Mojolicious::Lite`.
/// `Mojolicious` and `Mojolicious::Controller` are deliberately *not*
/// selectors: those profiles are reached through the Mojo::Base adapter's
/// proven literal parent, not through their own module import.
#[must_use]
pub fn mojolicious_lite_descriptor() -> AdapterDescriptor {
    let mut descriptor = AdapterDescriptor::new(
        MOJOLICIOUS_ADAPTER_ID,
        "mojolicious-lite",
        MOJOLICIOUS_FRAMEWORK_NAME,
        Some(MOJOLICIOUS_VERSION_CONSTRAINT.to_string()),
        MOJOLICIOUS_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    );
    descriptor.required_module_selectors = vec![MOJOLICIOUS_LITE_MODULE.to_string()];
    descriptor
}

/// Run the registry-backed `Mojolicious::Lite` detection over one checked
/// input.
///
/// Only the descriptor-owned `Mojolicious::Lite` selector participates. The
/// input descriptor must be the canonical [`mojolicious_lite_descriptor`]
/// value: a foreign descriptor's selectors are never adopted as Mojolicious
/// evidence. A pre-cancelled admission snapshot fails closed to
/// [`DetectionOutcome::Cancelled`] before any module evidence is evaluated,
/// and duplicate or contradictory rows for the owned selector stay a
/// conflict instead of becoming first-wins evidence.
#[must_use]
pub fn detect_mojolicious_lite(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    if input.descriptor != mojolicious_lite_descriptor() {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: "input descriptor does not match the canonical Mojolicious::Lite \
                         adapter descriptor"
                    .to_string(),
            },
        );
    }
    if input.cancellation.is_cancelled {
        return AdapterDetectionResult::for_input(input, DetectionOutcome::Cancelled);
    }
    let descriptor = &input.descriptor;
    let owned: Vec<&ModuleSelectorEvaluation> = input
        .module_observation
        .evaluations
        .iter()
        .filter(|evaluation| {
            descriptor
                .required_module_selectors
                .iter()
                .any(|selector| selector == &evaluation.selector)
        })
        .collect();
    let [evaluation] = owned.as_slice() else {
        return if owned.is_empty() {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        } else {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Conflicting {
                    conflict_descriptions: vec![format!(
                        "selector `{MOJOLICIOUS_LITE_MODULE}` carries {} terminal evaluations; \
                         completeness requires exactly one",
                        owned.len()
                    )],
                },
            )
        };
    };
    let evaluation = *evaluation;
    match &evaluation.outcome {
        ModuleSelectorOutcome::Absent => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
        ),
        ModuleSelectorOutcome::Unresolved { .. } | ModuleSelectorOutcome::Unavailable { .. } => {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        }
        ModuleSelectorOutcome::Ambiguous { .. } => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Conflicting {
                conflict_descriptions: vec![format!(
                    "selector `{}` matched more than one module identity",
                    evaluation.selector
                )],
            },
        ),
        ModuleSelectorOutcome::Matched { activation, evidence_class } => {
            if activation.module_name != MOJOLICIOUS_LITE_MODULE {
                // The row is filed under the owned selector but resolves to a
                // different module. That contradiction is never Mojolicious
                // evidence, for this adapter or for a direct detector consumer.
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Conflicting {
                        conflict_descriptions: vec![format!(
                            "selector `{MOJOLICIOUS_LITE_MODULE}` matched module `{}`; the \
                             owned selector must resolve to its own module",
                            activation.module_name
                        )],
                    },
                );
            }
            let identity_confidence = evidence_class.confidence_ceiling();
            if identity_confidence != Confidence::High {
                // A module merely named Mojolicious::Lite without resolved
                // supported identity is not exact activation (#9688).
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "Mojolicious::Lite selector matched with {identity_confidence:?} \
                             identity evidence; exact activation requires resolved module identity"
                        ),
                    },
                );
            }
            match &activation.observed_version {
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: "Mojolicious::Lite activation lacks observed version evidence; \
                                 the reviewed version constraint cannot be checked"
                            .to_string(),
                    },
                ),
                Some(version) => {
                    match crate::framework::version_constraint_matches(
                        MOJOLICIOUS_VERSION_CONSTRAINT,
                        &version.version,
                    ) {
                        Some(true) => {
                            let mut result = AdapterDetectionResult::for_input(
                                input,
                                DetectionOutcome::Detected {
                                    confidence: Confidence::High,
                                    framework_version: Some(version.version.clone()),
                                },
                            );
                            result = result.with_contributing_modules(vec![activation.clone()]);
                            result.with_version_evidence(version.clone())
                        }
                        Some(false) => {
                            let result = AdapterDetectionResult::for_input(
                                input,
                                DetectionOutcome::Absent {
                                    reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
                                },
                            );
                            result.with_version_evidence(version.clone())
                        }
                        // The observed version cannot be compared against the
                        // reviewed constraint; it stays explicitly unsupported.
                        None => AdapterDetectionResult::for_input(
                            input,
                            DetectionOutcome::Unsupported {
                                reason: format!(
                                    "observed Mojolicious::Lite version `{}` is not comparable \
                                     with the reviewed constraint \
                                     `{MOJOLICIOUS_VERSION_CONSTRAINT}`",
                                    version.version
                                ),
                            },
                        ),
                    }
                }
            }
        }
    }
}

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

/// Parse `use Mojolicious::Lite` import arguments (parser token strings) into
/// evidence.
///
/// `Mojolicious::Lite::import` reviews exactly one option, `-signatures`;
/// every other literal argument widens the profile instead of being silently
/// accepted. Computed arguments become explicit dynamic selections, and
/// unterminated quotes become explicit malformed selections instead of being
/// normalized into a profile.
#[must_use]
pub fn parse_mojolicious_lite_import_args(args: &[String]) -> MojoliciousLiteImportEvidence {
    mojolicious_lite_import_evidence(args, false, None)
}

/// Build Lite import evidence from parser argument tokens plus the source-level
/// fact of whether the import carried an explicit empty list.
///
/// The parser reports `use Mojolicious::Lite;` and `use Mojolicious::Lite ();`
/// with the same empty argument vector, but Perl treats them differently: the
/// explicit empty list suppresses `import` entirely, so the Lite DSL is never
/// installed. `explicit_empty_import` carries that distinction from the source
/// side; it is only meaningful when `args` is empty.
///
/// A leading v-string (`use Mojolicious::Lite v9.34;`) is a version
/// requirement, not an import option: Perl still calls `import` with no
/// arguments, so it does not widen the reviewed profile. The numeric spelling
/// (`use Mojolicious::Lite 9.34;`) never reaches here — the parser folds it
/// into the module name instead.
#[must_use]
pub fn mojolicious_lite_import_evidence(
    args: &[String],
    explicit_empty_import: bool,
    module_version_requirement: Option<&str>,
) -> MojoliciousLiteImportEvidence {
    let mut evidence = MojoliciousLiteImportEvidence {
        source_version_requirement: module_version_requirement.map(normalize_version_requirement),
        ..MojoliciousLiteImportEvidence::default()
    };
    let mut tokens = normalize_import_tokens(args);
    // A v-string requirement reaches the argument list rather than the module
    // name; consume it before the import-option scan.
    if let Some(first) = tokens.first()
        && is_version_requirement(first)
    {
        evidence.source_version_requirement = Some(normalize_version_requirement(first));
        tokens.remove(0);
    }
    // `use Mojolicious::Lite VERSION ();` reaches here as the parenthesis
    // tokens; the source-level check is authoritative either way.
    let only_parens = !tokens.is_empty() && tokens.iter().all(|token| token == "(" || token == ")");
    if (tokens.is_empty() || only_parens) && explicit_empty_import {
        evidence.selection = MojoliciousLiteImportSelection::ImportSuppressed;
        return evidence;
    }
    for token in tokens {
        if token == "-signatures" {
            evidence.signatures = true;
            if evidence.selection == MojoliciousLiteImportSelection::Default {
                evidence.selection = MojoliciousLiteImportSelection::Signatures;
            }
            continue;
        }
        if let Some(reason) = malformed_token_reason(&token) {
            evidence.selection = MojoliciousLiteImportSelection::Malformed { reason };
            continue;
        }
        if let Some(reason) = dynamic_token_reason(&token) {
            // A dynamic argument never downgrades an already-malformed
            // classification: recovered source stays the stronger signal.
            if !matches!(evidence.selection, MojoliciousLiteImportSelection::Malformed { .. }) {
                evidence.selection = MojoliciousLiteImportSelection::Dynamic { reason };
            }
            continue;
        }
        evidence.unmodeled_options.push(token);
    }
    evidence
}

/// Drop empty argument slots, preserving each parser token whole.
///
/// The parser already separates distinct import tokens — a bare sigil arrives
/// as its own `%` token — and it keeps a quoted literal containing spaces in
/// one token. Splitting further would fragment `'foo bar'` into `'foo` and
/// `bar'`, which would then read as an unterminated quote and misreport a
/// well-formed unreviewed option as recovered source.
fn normalize_import_tokens(args: &[String]) -> Vec<String> {
    args.iter().filter(|arg| !arg.trim().is_empty()).map(|arg| arg.trim().to_string()).collect()
}

/// Recovered spellings the parser could not terminate.
fn malformed_token_reason(token: &str) -> Option<String> {
    let unterminated =
        |quote: char| token.starts_with(quote) && (token.len() < 2 || !token.ends_with(quote));
    if unterminated('\'') || unterminated('"') {
        return Some(format!("unterminated quoted import argument `{token}`"));
    }
    None
}

/// Computed argument spellings that cannot be read statically.
fn dynamic_token_reason(token: &str) -> Option<String> {
    let interpolates = token.starts_with('"')
        && token.ends_with('"')
        && token.len() >= 2
        && token[1..token.len() - 1].contains(['$', '@', '%']);
    if token.starts_with(['$', '@', '%', '\\']) || interpolates {
        return Some(format!("computed import argument `{token}` is a dynamic boundary"));
    }
    None
}

/// Strip a `v` prefix so numeric and v-string requirements compare alike.
fn normalize_version_requirement(required: &str) -> String {
    required.strip_prefix('v').unwrap_or(required).to_string()
}

/// Whether one leading import token is a v-string version requirement.
///
/// `use Mojolicious::Lite v9.34;` requires a version and still imports the
/// DSL with no arguments; the token is not an import option.
fn is_version_requirement(token: &str) -> bool {
    let Some(digits) = token.strip_prefix('v') else {
        return false;
    };
    !digits.is_empty()
        && digits.starts_with(|c: char| c.is_ascii_digit())
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
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

fn shadow_limitations() -> Vec<String> {
    SHADOW_LIMITATIONS.iter().map(ToString::to_string).collect()
}

/// Build typed Lite activation facts from one `Mojolicious::Lite` detection
/// result plus the site anchor and import evidence.
///
/// Ordering is fail-closed and mirrors the Mojo::Base profile: instrument
/// failures and module availability first, then source-level classification
/// (malformed, dynamic), then detection-level version/profile support, then
/// evidence completeness (contributing module, version evidence, and a
/// reconciled current input identity), generation staleness with
/// known-generation requirements, and the reviewed-profile check. Only a
/// `Detected` outcome carrying contributing module and version evidence
/// under a reconciled input identity with current known site, module, and
/// version generations and no unreviewed import options yields an exact
/// Lite application.
#[must_use]
pub fn mojolicious_lite_activation_facts(
    detection: &AdapterDetectionResult,
    anchor: &MojoliciousSiteAnchor,
    evidence: &MojoliciousLiteImportEvidence,
) -> MojoliciousActivationFacts {
    let mut facts = MojoliciousActivationFacts {
        outcome: MojoliciousActivationOutcome::AbsentWithCompleteEvidence { reason: String::new() },
        profile_version: MOJOLICIOUS_PROFILE_VERSION,
        package: anchor.package.clone(),
        source_interval: (anchor.span_start_byte, anchor.span_end_byte),
        // A Lite import has no literal parent spelling.
        parent_range: None,
        scope_identity: detection
            .input_identity
            .as_ref()
            .map(|identity| identity.module_observation.scope_identity.clone()),
        environment_identity: detection
            .input_identity
            .as_ref()
            .map(|identity| identity.module_observation.environment_identity.clone()),
        resolved_module: detection.contributing_modules.first().cloned(),
        framework_version: String::new(),
        confidence: Confidence::Low,
        source_generation: detection.project_generation.clone(),
        signatures: evidence.signatures,
        unmodeled_options: evidence.unmodeled_options.clone(),
        limitations: shadow_limitations(),
    };

    // 1. Instrument state: nothing is knowable about activation.
    if let Some(reason) = instrument_failure_reason(&detection.outcome) {
        facts.outcome = MojoliciousActivationOutcome::InstrumentFailure { reason };
        return facts;
    }

    // 2. The site anchor must describe a real statement interval. An empty or
    // reversed range is not an extracted import, so it cannot anchor a role
    // however current its generation is.
    if anchor.span_end_byte <= anchor.span_start_byte {
        facts.outcome = MojoliciousActivationOutcome::StaleOrIncompleteInput {
            reason: format!(
                "site interval [{}, {}) is empty or reversed; an exact role requires a \
                 located import statement",
                anchor.span_start_byte, anchor.span_end_byte
            ),
        };
        return facts;
    }

    // 3. Module availability and identity.
    match &detection.outcome {
        DetectionOutcome::Unavailable { reason } => {
            facts.outcome = MojoliciousActivationOutcome::MissingOrUnavailableModule {
                reason: format!("{reason:?}"),
            };
            return facts;
        }
        DetectionOutcome::Conflicting { conflict_descriptions } => {
            facts.outcome = MojoliciousActivationOutcome::AmbiguousOrConflictingModule {
                reason: conflict_descriptions.join("; "),
            };
            return facts;
        }
        _ => {}
    }

    // 4. Source-level classification of the import site.
    match &evidence.selection {
        MojoliciousLiteImportSelection::Malformed { reason } => {
            facts.outcome =
                MojoliciousActivationOutcome::RecoveredOrMalformedSource { reason: reason.clone() };
            return facts;
        }
        MojoliciousLiteImportSelection::Dynamic { reason } => {
            facts.outcome =
                MojoliciousActivationOutcome::DynamicOrUnmodeledParent { reason: reason.clone() };
            return facts;
        }
        MojoliciousLiteImportSelection::ImportSuppressed => {
            facts.outcome = MojoliciousActivationOutcome::AbsentWithCompleteEvidence {
                reason: "`use Mojolicious::Lite ();` suppresses `import`, so the Lite DSL is \
                         never installed and the package owns no Lite role"
                    .to_string(),
            };
            return facts;
        }
        _ => {}
    }

    // 5. Detection-level version/profile support.
    match &detection.outcome {
        DetectionOutcome::Unsupported { reason } => {
            facts.outcome = MojoliciousActivationOutcome::UnsupportedVersionOrProfile {
                reason: reason.clone(),
            };
            return facts;
        }
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
        } => {
            facts.outcome = MojoliciousActivationOutcome::UnsupportedVersionOrProfile {
                reason: format!(
                    "observed version does not satisfy the reviewed constraint \
                     `{MOJOLICIOUS_VERSION_CONSTRAINT}`"
                ),
            };
            return facts;
        }
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing } => {
            // The module is absent from the environment. That is a missing
            // dependency, not complete evidence that the package owns no role.
            facts.outcome = MojoliciousActivationOutcome::MissingOrUnavailableModule {
                reason: format!("{:?}", DetectionAbsenceReason::RequiredModulesMissing),
            };
            return facts;
        }
        DetectionOutcome::Absent { reason } => {
            facts.outcome = MojoliciousActivationOutcome::AbsentWithCompleteEvidence {
                reason: format!("{reason:?}"),
            };
            return facts;
        }
        _ => {}
    }

    let DetectionOutcome::Detected { confidence, framework_version } = &detection.outcome else {
        // Unreachable for SDK outcomes not matched above; stays bounded.
        facts.outcome = MojoliciousActivationOutcome::StaleOrIncompleteInput {
            reason: format!("unhandled detection outcome {:?}", detection.outcome),
        };
        return facts;
    };
    // Detection-reported confidence and version stay local until the
    // evidence behind them is reconciled: a refused fact must never publish
    // an unverified framework version as if it had been observed.
    let reported_confidence = *confidence;
    let reported_version = framework_version.clone().unwrap_or_default();

    // 6. Evidence completeness: a raw or deserialized `Detected` result does
    // not become exact activation without its contributing module identity,
    // version evidence, and a current input identity that reconciles with
    // this detection.
    if detection.contributing_modules.is_empty() || detection.version_evidence.is_none() {
        facts.outcome = MojoliciousActivationOutcome::StaleOrIncompleteInput {
            reason: "detected result lacks contributing module or version evidence; raw \
                     results cannot become exact activation"
                .to_string(),
        };
        return facts;
    }
    if let Some(reason) = identity_reconciliation_reason(detection) {
        facts.outcome = MojoliciousActivationOutcome::StaleOrIncompleteInput { reason };
        return facts;
    }

    // 7. Generation staleness: site, module, and version evidence must all be
    // known and current for the detection generation.
    if let Some(reason) = staleness_reason(detection, anchor) {
        facts.outcome = MojoliciousActivationOutcome::StaleOrIncompleteInput { reason };
        return facts;
    }
    facts.source_generation = anchor.source_generation.clone();

    // 8. A reported confidence below High is never exact activation: an
    // exact fact that carries Low confidence contradicts itself.
    if reported_confidence != Confidence::High {
        facts.outcome = MojoliciousActivationOutcome::StaleOrIncompleteInput {
            reason: format!(
                "detection reported {reported_confidence:?} confidence; exact activation \
                 requires High"
            ),
        };
        return facts;
    }

    // 9. The reviewed profile must cover every import option.
    if !evidence.unmodeled_options.is_empty() {
        facts.outcome = MojoliciousActivationOutcome::UnsupportedVersionOrProfile {
            reason: format!(
                "import carries options outside profile {MOJOLICIOUS_PROFILE_VERSION}: {}",
                evidence.unmodeled_options.join(", ")
            ),
        };
        return facts;
    }

    // 10. The import's own version requirement must be satisfied by the
    // observed version. `use Mojolicious::Lite VERSION;` dies at compile time
    // when it is not, so such an import never activates in real Perl.
    if let Some(required) = &evidence.source_version_requirement {
        let satisfied = crate::framework::version_constraint_matches(
            &format!(">={required}"),
            &reported_version,
        );
        if satisfied != Some(true) {
            facts.outcome = MojoliciousActivationOutcome::UnsupportedVersionOrProfile {
                reason: format!(
                    "import requires version `{required}` but the observed version is \
                     `{reported_version}`"
                ),
            };
            return facts;
        }
    }

    // 11. Exact Lite activation under the reviewed profile. Only here, with
    // every generation current and the evidence reconciled, do the observed
    // confidence and framework version become published facts.
    facts.confidence = reported_confidence;
    facts.framework_version = reported_version;
    facts.outcome =
        MojoliciousActivationOutcome::ExactActivation { role: MojoliciousRole::LiteApplication };
    facts
}

/// Classify one proven Mojo::Base activation into its Mojolicious role.
///
/// This is the full-application and controller profile. It consumes
/// [`MojoBaseActivationFacts`] minted by the Mojo::Base adapter
/// (#9681/#9682) and never reparses `Mojo::Base` import semantics: the
/// parent spelling, source anchors, module identity, version, confidence,
/// and generation are all carried through from the proven activation.
///
/// Only an exact literal-parent activation whose parent is exactly
/// `Mojolicious` or `Mojolicious::Controller` yields a role. Every other
/// Mojo::Base state — including an exact `-base` activation, an exact
/// literal parent that is some other class, and each fail-closed state —
/// maps to the matching bounded Mojolicious state and yields no role.
#[must_use]
pub fn mojolicious_role_facts_from_mojo_base(
    mojo_base: &MojoBaseActivationFacts,
) -> MojoliciousActivationFacts {
    let mut limitations = shadow_limitations();
    limitations.push(DERIVED_PARENT_LIMITATION.to_string());

    let outcome = match &mojo_base.outcome {
        MojoBaseActivationOutcome::ExactLiteralParentActivation { parent } => {
            match parent.as_str() {
                MOJOLICIOUS_APPLICATION_PARENT => MojoliciousActivationOutcome::ExactActivation {
                    role: MojoliciousRole::Application,
                },
                MOJOLICIOUS_CONTROLLER_PARENT => MojoliciousActivationOutcome::ExactActivation {
                    role: MojoliciousRole::Controller,
                },
                other => MojoliciousActivationOutcome::AbsentWithCompleteEvidence {
                    reason: format!(
                        "literal parent `{other}` is neither \
                         `{MOJOLICIOUS_APPLICATION_PARENT}` nor \
                         `{MOJOLICIOUS_CONTROLLER_PARENT}`"
                    ),
                },
            }
        }
        MojoBaseActivationOutcome::ExactBaseActivation => {
            MojoliciousActivationOutcome::AbsentWithCompleteEvidence {
                reason: "`use Mojo::Base -base;` inherits from Mojo::Base, not from a \
                         Mojolicious application or controller"
                    .to_string(),
            }
        }
        MojoBaseActivationOutcome::AbsentWithCompleteEvidence { reason } => {
            MojoliciousActivationOutcome::AbsentWithCompleteEvidence { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::UnsupportedVersionOrProfile { reason } => {
            MojoliciousActivationOutcome::UnsupportedVersionOrProfile { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::MissingOrUnavailableModule { reason } => {
            MojoliciousActivationOutcome::MissingOrUnavailableModule { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::AmbiguousOrConflictingModule { reason } => {
            MojoliciousActivationOutcome::AmbiguousOrConflictingModule { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::StaleOrIncompleteInput { reason } => {
            MojoliciousActivationOutcome::StaleOrIncompleteInput { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::DynamicOrUnmodeledParent { reason } => {
            MojoliciousActivationOutcome::DynamicOrUnmodeledParent { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::RecoveredOrMalformedSource { reason } => {
            MojoliciousActivationOutcome::RecoveredOrMalformedSource { reason: reason.clone() }
        }
        MojoBaseActivationOutcome::InstrumentFailure { reason } => {
            MojoliciousActivationOutcome::InstrumentFailure { reason: reason.clone() }
        }
    };

    MojoliciousActivationFacts {
        outcome,
        profile_version: MOJOLICIOUS_PROFILE_VERSION,
        package: mojo_base.package.clone(),
        source_interval: mojo_base.source_interval,
        parent_range: mojo_base.parent_range,
        scope_identity: mojo_base.scope_identity.clone(),
        environment_identity: mojo_base.environment_identity.clone(),
        resolved_module: mojo_base.resolved_module.clone(),
        framework_version: mojo_base.framework_version.clone(),
        confidence: mojo_base.confidence,
        source_generation: mojo_base.source_generation.clone(),
        signatures: mojo_base.signatures,
        unmodeled_options: mojo_base.unmodeled_options.clone(),
        limitations,
    }
}

fn instrument_failure_reason(outcome: &DetectionOutcome) -> Option<String> {
    match outcome {
        DetectionOutcome::Cancelled => {
            Some("detection was cancelled before admission completed".to_string())
        }
        DetectionOutcome::BudgetExhausted => {
            Some("detection exhausted its resource budget".to_string())
        }
        DetectionOutcome::Unavailable { reason: UnavailableReason::InternalError } => {
            Some("detection reported an internal instrument error".to_string())
        }
        _ => None,
    }
}

/// Reconcile the detection's current input identity with this adapter before
/// its evidence may support exact activation: the identity must exist, carry
/// the canonical descriptor, one terminal `Mojolicious::Lite` selector
/// evaluation whose matched module is `Mojolicious::Lite`, and an
/// observation receipt from the detection generation.
fn identity_reconciliation_reason(detection: &AdapterDetectionResult) -> Option<String> {
    let Some(identity) = &detection.input_identity else {
        return Some(
            "detected result carries no current input identity; fabricated evidence cannot \
             become exact activation"
                .to_string(),
        );
    };
    if identity.descriptor != mojolicious_lite_descriptor() {
        return Some(
            "input identity belongs to a different adapter descriptor; it cannot support \
             Mojolicious::Lite exact activation"
                .to_string(),
        );
    }
    let observation = &identity.module_observation;
    if observation.generation != detection.project_generation {
        return Some(format!(
            "observation receipt generation {:?} does not match the detection generation {:?}",
            observation.generation, detection.project_generation
        ));
    }
    let owned: Vec<&ModuleSelectorEvaluation> = observation
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.selector == MOJOLICIOUS_LITE_MODULE)
        .collect();
    let [evaluation] = owned.as_slice() else {
        return Some(format!(
            "input identity carries {} terminal evaluations for selector \
             `{MOJOLICIOUS_LITE_MODULE}`; exactly one is required",
            owned.len()
        ));
    };
    let ModuleSelectorOutcome::Matched { activation, evidence_class } = &evaluation.outcome else {
        return Some(
            "the owned selector's terminal evaluation does not reconcile with the detection"
                .to_string(),
        );
    };
    // `detect_mojolicious_lite` refuses a non-resolved identity, but a
    // hand-built or deserialized result can reach this builder without ever
    // passing through the detector. The identity gate therefore belongs here
    // too, not only in the detector.
    if evidence_class.confidence_ceiling() != Confidence::High {
        return Some(format!(
            "the owned selector's evidence class {evidence_class:?} cannot support exact \
             activation; resolved module identity is required"
        ));
    }
    if activation.module_name != MOJOLICIOUS_LITE_MODULE
        || activation.generation != detection.project_generation
    {
        return Some(
            "the owned selector's terminal evaluation does not reconcile with the detection"
                .to_string(),
        );
    }
    // The result's own module and version evidence must be the input row's,
    // not merely present. A deserialized or mutated result can carry a valid
    // input identity beside fabricated contributing evidence; binding them
    // here is what stops that from becoming exact activation.
    if detection.contributing_modules.as_slice() != std::slice::from_ref(activation) {
        return Some(
            "contributing module evidence does not equal the owned selector's matched \
             activation; fabricated identities cannot become exact activation"
                .to_string(),
        );
    }
    if detection.version_evidence.as_ref() != activation.observed_version.as_ref() {
        return Some(
            "version evidence does not equal the matched activation's observed version".to_string(),
        );
    }
    if let DetectionOutcome::Detected { framework_version: Some(reported), .. } = &detection.outcome
        && activation.observed_version.as_ref().map(|evidence| &evidence.version) != Some(reported)
    {
        return Some(
            "reported framework version does not equal the matched activation's observed version"
                .to_string(),
        );
    }
    None
}

fn staleness_reason(
    detection: &AdapterDetectionResult,
    anchor: &MojoliciousSiteAnchor,
) -> Option<String> {
    // Exact current activation requires every load-bearing generation to be
    // known: all-`Unknown` generations compare equal but identify nothing.
    if !anchor.source_generation.is_known() {
        return Some("site source generation is unknown".to_string());
    }
    if !detection.project_generation.is_known() {
        return Some("detection generation is unknown".to_string());
    }
    if let Some(module) = detection.contributing_modules.first()
        && !module.generation.is_known()
    {
        return Some("module activation generation is unknown".to_string());
    }
    if let Some(version) = &detection.version_evidence
        && !version.generation.is_known()
    {
        return Some("version evidence generation is unknown".to_string());
    }
    if anchor.source_generation != detection.project_generation {
        return Some(format!(
            "site generation {:?} does not match detection generation {:?}",
            anchor.source_generation, detection.project_generation
        ));
    }
    if let Some(module) = detection.contributing_modules.first()
        && module.generation != detection.project_generation
    {
        return Some(format!(
            "module activation generation {:?} does not match detection generation {:?}",
            module.generation, detection.project_generation
        ));
    }
    if let Some(version) = &detection.version_evidence
        && version.generation != detection.project_generation
    {
        return Some(format!(
            "version evidence generation {:?} does not match detection generation {:?}",
            version.generation, detection.project_generation
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileId;
    use crate::framework::{
        AdapterCancellation, DetectionEvidenceClass, ModuleObservationReceipt,
        ModuleVersionEvidence,
    };
    use crate::framework_adapters::mojo_base::{
        MojoBaseSiteAnchor, detect_mojo_base, mojo_base_activation_facts, mojo_base_descriptor,
        parse_mojo_base_import_args,
    };

    // ---------------------------------------------------------------------
    // Descriptor identity and containment
    // ---------------------------------------------------------------------

    #[test]
    fn descriptor_owns_only_the_lite_selector() {
        let descriptor = mojolicious_lite_descriptor();
        assert_eq!(descriptor.required_module_selectors, vec![MOJOLICIOUS_LITE_MODULE.to_string()]);
        assert_eq!(descriptor.framework_name, MOJOLICIOUS_FRAMEWORK_NAME);
        assert_eq!(descriptor.disposition, AdapterDisposition::Shadow);
        assert_eq!(
            descriptor.framework_version_constraint.as_deref(),
            Some(MOJOLICIOUS_VERSION_CONSTRAINT)
        );
    }

    #[test]
    fn application_and_controller_parents_are_not_module_selectors() {
        // Those profiles are reached through the Mojo::Base adapter's proven
        // literal parent; claiming them as selectors here would mint a second
        // activation authority for the same source construct.
        let selectors = mojolicious_lite_descriptor().required_module_selectors;
        assert!(!selectors.iter().any(|s| s == MOJOLICIOUS_APPLICATION_PARENT));
        assert!(!selectors.iter().any(|s| s == MOJOLICIOUS_CONTROLLER_PARENT));
    }

    #[test]
    fn adapter_ids_are_distinct_across_shadow_adapters() {
        use crate::framework_adapters::{dancer2, dbix_class, mojo_base};
        assert_ne!(MOJOLICIOUS_ADAPTER_ID, mojo_base::MOJO_BASE_ADAPTER_ID);
        assert_ne!(MOJOLICIOUS_ADAPTER_ID, dancer2::DANCER2_ADAPTER_ID);
        assert_ne!(MOJOLICIOUS_ADAPTER_ID, dbix_class::DBIX_CLASS_ADAPTER_ID);
    }

    #[test]
    fn mojolicious_and_dancer2_remain_distinct_framework_profiles() {
        use crate::framework_adapters::dancer2;
        let mojo = mojolicious_lite_descriptor();
        let dancer = dancer2::dancer2_descriptor();
        assert_ne!(mojo.framework_name, dancer.framework_name);
        assert_ne!(mojo.required_module_selectors, dancer.required_module_selectors);
    }

    // ---------------------------------------------------------------------
    // Import argument parsing
    // ---------------------------------------------------------------------

    fn parse(args: &[&str]) -> MojoliciousLiteImportEvidence {
        let owned: Vec<String> = args.iter().map(ToString::to_string).collect();
        parse_mojolicious_lite_import_args(&owned)
    }

    #[test]
    fn bare_import_is_the_reviewed_default_form() {
        let evidence = parse(&[]);
        assert_eq!(evidence.selection, MojoliciousLiteImportSelection::Default);
        assert!(!evidence.signatures);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn signatures_option_is_the_reviewed_signatures_form() {
        let evidence = parse(&["-signatures"]);
        assert_eq!(evidence.selection, MojoliciousLiteImportSelection::Signatures);
        assert!(evidence.signatures);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn unreviewed_option_stays_unmodeled_instead_of_widening_the_profile() {
        let evidence = parse(&["-strict"]);
        assert_eq!(evidence.unmodeled_options, vec!["-strict".to_string()]);
        assert!(!evidence.signatures);
    }

    #[test]
    fn computed_import_argument_is_a_dynamic_boundary() {
        assert!(matches!(
            parse(&["$flag"]).selection,
            MojoliciousLiteImportSelection::Dynamic { .. }
        ));
        assert!(matches!(
            parse(&["\"$flag\""]).selection,
            MojoliciousLiteImportSelection::Dynamic { .. }
        ));
    }

    #[test]
    fn statically_quoted_option_is_not_dynamic() {
        // A double-quoted literal without interpolation is static evidence.
        let evidence = parse(&["\"-strict\""]);
        assert_eq!(evidence.selection, MojoliciousLiteImportSelection::Default);
        assert_eq!(evidence.unmodeled_options, vec!["\"-strict\"".to_string()]);
    }

    #[test]
    fn unterminated_quote_is_malformed_not_an_option() {
        assert!(matches!(
            parse(&["'-signa"]).selection,
            MojoliciousLiteImportSelection::Malformed { .. }
        ));
    }

    #[test]
    fn recovered_source_outranks_a_dynamic_argument() {
        // Both a malformed and a dynamic token are present; the recovered
        // spelling is the stronger signal and must not be overwritten.
        let evidence = parse(&["'-signa", "$flag"]);
        assert!(matches!(evidence.selection, MojoliciousLiteImportSelection::Malformed { .. }));
    }

    // ---------------------------------------------------------------------
    // Lite detection
    // ---------------------------------------------------------------------

    fn lite_evaluation(
        module: &str,
        version: Option<&str>,
        generation: &str,
        evidence_class: DetectionEvidenceClass,
    ) -> ModuleSelectorEvaluation {
        let mut activation = ModuleActivationIdentity::new(
            module,
            Some(FileId(11)),
            SourceGeneration::known(generation),
        );
        if let Some(version) = version {
            activation = activation.with_observed_version(ModuleVersionEvidence::new(
                version,
                SourceGeneration::known(generation),
            ));
        }
        ModuleSelectorEvaluation::new(
            MOJOLICIOUS_LITE_MODULE,
            ModuleSelectorOutcome::Matched { activation, evidence_class },
        )
    }

    fn receipt(
        generation: &str,
        evaluations: Vec<ModuleSelectorEvaluation>,
    ) -> ModuleObservationReceipt {
        ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known(generation),
            "sha256:fixture-input",
            evaluations,
        )
    }

    fn lite_input(generation: &str) -> AdapterDetectionInput {
        AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                generation,
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    generation,
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        )
    }

    #[test]
    fn resolved_supported_lite_module_is_detected() {
        let result = detect_mojolicious_lite(&lite_input("gen-1"));
        assert!(matches!(result.outcome, DetectionOutcome::Detected { .. }));
        assert_eq!(result.contributing_modules.len(), 1);
        assert!(result.version_evidence.is_some());
    }

    #[test]
    fn foreign_descriptor_is_never_adopted_as_mojolicious_evidence() {
        let input = AdapterDetectionInput::new(
            mojo_base_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn pre_cancelled_input_fails_closed() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::cancelled(),
        );
        assert_eq!(detect_mojolicious_lite(&input).outcome, DetectionOutcome::Cancelled);
    }

    #[test]
    fn name_only_evidence_is_not_exact_activation() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::NameOnly,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn version_outside_the_reviewed_range_does_not_activate() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("10.1"),
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::VersionConstraintNotSatisfied
            }
        ));
    }

    #[test]
    fn missing_version_evidence_cannot_be_checked_and_stays_unsupported() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    None,
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn duplicate_rows_for_the_owned_selector_stay_a_conflict() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![
                    lite_evaluation(
                        MOJOLICIOUS_LITE_MODULE,
                        Some("9.34"),
                        "gen-1",
                        DetectionEvidenceClass::ResolvedModule,
                    ),
                    lite_evaluation(
                        MOJOLICIOUS_LITE_MODULE,
                        Some("9.34"),
                        "gen-1",
                        DetectionEvidenceClass::ResolvedModule,
                    ),
                ],
            ),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Conflicting { .. }
        ));
    }

    #[test]
    fn absent_owned_selector_is_unavailable_not_absent_activation() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt("gen-1", vec![]),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Unavailable { .. }
        ));
    }

    // ---------------------------------------------------------------------
    // Lite activation facts
    // ---------------------------------------------------------------------

    fn lite_anchor(package: &str, generation: &str) -> MojoliciousSiteAnchor {
        MojoliciousSiteAnchor::new(
            Some(package.to_string()),
            0,
            25,
            SourceGeneration::known(generation),
        )
    }

    #[test]
    fn exact_lite_import_owns_the_lite_application_role() {
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(facts.role(), Some(MojoliciousRole::LiteApplication));
        assert_eq!(facts.package.as_deref(), Some("main"));
        assert_eq!(facts.framework_version, "9.34");
        assert_eq!(facts.confidence, Confidence::High);
        assert_eq!(facts.profile_version, MOJOLICIOUS_PROFILE_VERSION);
        assert_eq!(facts.scope_identity.as_deref(), Some("root:fixture"));
    }

    #[test]
    fn signatures_form_still_owns_the_lite_application_role() {
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&["-signatures"]),
        );
        assert_eq!(facts.role(), Some(MojoliciousRole::LiteApplication));
        assert!(facts.signatures);
    }

    #[test]
    fn unreviewed_import_option_refuses_the_role() {
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&["-strict"]),
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::UnsupportedVersionOrProfile { .. }
        ));
    }

    #[test]
    fn stale_site_generation_refuses_the_role() {
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-2")),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }
        ));
    }

    #[test]
    fn unknown_site_generation_refuses_the_role() {
        let anchor =
            MojoliciousSiteAnchor::new(Some("main".to_string()), 0, 25, SourceGeneration::Unknown);
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &anchor,
            &parse(&[]),
        );
        assert_eq!(facts.role(), None);
    }

    #[test]
    fn dynamic_import_argument_refuses_the_role() {
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&["$flag"]),
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::DynamicOrUnmodeledParent { .. }
        ));
    }

    #[test]
    fn malformed_import_refuses_the_role() {
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&["'-signa"]),
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::RecoveredOrMalformedSource { .. }
        ));
    }

    #[test]
    fn cancelled_detection_is_an_instrument_failure_not_an_absence() {
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::cancelled(),
        );
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&input),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert!(matches!(facts.outcome, MojoliciousActivationOutcome::InstrumentFailure { .. }));
    }

    // ---------------------------------------------------------------------
    // Application / controller roles derived from proven Mojo::Base facts
    // ---------------------------------------------------------------------

    /// Build genuine Mojo::Base activation facts through the Mojo::Base
    /// adapter's own pipeline, so the derived Mojolicious role is proven
    /// against real upstream authority rather than a fabricated record.
    fn mojo_base_facts(parent_token: &str, generation: &str) -> MojoBaseActivationFacts {
        let evaluation = {
            let activation = ModuleActivationIdentity::new(
                "Mojo::Base",
                Some(FileId(7)),
                SourceGeneration::known(generation),
            )
            .with_observed_version(ModuleVersionEvidence::new(
                "9.34",
                SourceGeneration::known(generation),
            ));
            ModuleSelectorEvaluation::new(
                "Mojo::Base",
                ModuleSelectorOutcome::Matched {
                    activation,
                    evidence_class: DetectionEvidenceClass::ResolvedModule,
                },
            )
        };
        let input = AdapterDetectionInput::new(
            mojo_base_descriptor(),
            receipt(generation, vec![evaluation]),
            None,
            AdapterCancellation::active(),
        );
        let detection = detect_mojo_base(&input);
        let evidence = parse_mojo_base_import_args(&[parent_token.to_string()]);
        // The literal parent's located range must sit inside the import
        // interval; model one real `use Mojo::Base '<parent>';` statement.
        let statement = format!("use Mojo::Base {parent_token};");
        let parent_range = statement
            .find(parent_token)
            .map(|offset| (offset as u32, (offset + parent_token.len()) as u32));
        let anchor = MojoBaseSiteAnchor::new(
            Some("MyApp".to_string()),
            0,
            statement.len() as u32,
            parent_range,
            SourceGeneration::known(generation),
        );
        mojo_base_activation_facts(&detection, &anchor, &evidence)
    }

    #[test]
    fn literal_mojolicious_parent_owns_the_application_role() {
        let base = mojo_base_facts("'Mojolicious'", "gen-1");
        assert!(base.is_exact(), "upstream Mojo::Base activation must itself be exact");
        let facts = mojolicious_role_facts_from_mojo_base(&base);
        assert_eq!(facts.role(), Some(MojoliciousRole::Application));
        assert_eq!(facts.package.as_deref(), Some("MyApp"));
        assert_eq!(facts.framework_version, "9.34");
        assert_eq!(facts.confidence, Confidence::High);
        assert_eq!(facts.source_generation, SourceGeneration::known("gen-1"));
        assert!(facts.parent_range.is_some(), "the parent spelling stays source-anchored");
    }

    #[test]
    fn literal_controller_parent_owns_the_controller_role() {
        let facts = mojolicious_role_facts_from_mojo_base(&mojo_base_facts(
            "'Mojolicious::Controller'",
            "gen-1",
        ));
        assert_eq!(facts.role(), Some(MojoliciousRole::Controller));
    }

    #[test]
    fn application_and_controller_roles_are_distinct() {
        let app = mojolicious_role_facts_from_mojo_base(&mojo_base_facts("'Mojolicious'", "gen-1"));
        let controller = mojolicious_role_facts_from_mojo_base(&mojo_base_facts(
            "'Mojolicious::Controller'",
            "gen-1",
        ));
        assert_ne!(app.role(), controller.role());
    }

    #[test]
    fn an_unrelated_literal_parent_owns_no_mojolicious_role() {
        // Negative control: an exact Mojo::Base activation that is simply not
        // a Mojolicious class must not become one.
        let base = mojo_base_facts("'Mojo::EventEmitter'", "gen-1");
        assert!(base.is_exact());
        let facts = mojolicious_role_facts_from_mojo_base(&base);
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::AbsentWithCompleteEvidence { .. }
        ));
    }

    #[test]
    fn a_controller_namespace_lookalike_parent_owns_no_role() {
        // Negative control: only the exact parent spellings activate; a
        // project's own controller base class does not.
        let facts =
            mojolicious_role_facts_from_mojo_base(&mojo_base_facts("'MyApp::Controller'", "gen-1"));
        assert_eq!(facts.role(), None);
    }

    #[test]
    fn a_parent_that_merely_starts_with_the_framework_name_owns_no_role() {
        // Discriminates exact equality from a prefix/substring match. A
        // classifier regressed to `starts_with` would agree with every other
        // negative control in this suite but pass these.
        for parent in ["'Mojolicious::ControllerFake'", "'MojoliciousApp'", "'Mojolicious2'"] {
            let facts = mojolicious_role_facts_from_mojo_base(&mojo_base_facts(parent, "gen-1"));
            assert_eq!(facts.role(), None, "parent {parent} must not own a role");
        }
    }

    #[test]
    fn a_parent_that_merely_ends_with_the_framework_name_owns_no_role() {
        // The mirror control, against a regression to `ends_with`/`contains`.
        for parent in ["'My::Mojolicious'", "'Vendor::Mojolicious::Controller'"] {
            let facts = mojolicious_role_facts_from_mojo_base(&mojo_base_facts(parent, "gen-1"));
            assert_eq!(facts.role(), None, "parent {parent} must not own a role");
        }
    }

    #[test]
    fn base_form_activation_owns_no_mojolicious_role() {
        // Negative control: `-base` inherits from Mojo::Base itself.
        let base = mojo_base_facts("-base", "gen-1");
        assert!(base.is_exact());
        assert_eq!(mojolicious_role_facts_from_mojo_base(&base).role(), None);
    }

    #[test]
    fn strict_only_import_owns_no_mojolicious_role() {
        let facts = mojolicious_role_facts_from_mojo_base(&mojo_base_facts("-strict", "gen-1"));
        assert_eq!(facts.role(), None);
    }

    #[test]
    fn a_dynamic_parent_cannot_become_a_mojolicious_role() {
        // Negative control: the parent could evaluate to `Mojolicious` at
        // runtime; a dynamic boundary is not exact ownership.
        let base = mojo_base_facts("$parent", "gen-1");
        let facts = mojolicious_role_facts_from_mojo_base(&base);
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::DynamicOrUnmodeledParent { .. }
        ));
    }

    #[test]
    fn a_recovered_parent_spelling_cannot_become_a_mojolicious_role() {
        let facts = mojolicious_role_facts_from_mojo_base(&mojo_base_facts("'Mojolic", "gen-1"));
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::RecoveredOrMalformedSource { .. }
        ));
    }

    #[test]
    fn a_stale_mojo_base_activation_cannot_become_a_mojolicious_role() {
        // Negative control: the strongest falsifier for a derived profile is
        // reusing an activation that is no longer current. Build an exact
        // parent spelling but bind the site to a different generation.
        let detection = {
            let activation = ModuleActivationIdentity::new(
                "Mojo::Base",
                Some(FileId(7)),
                SourceGeneration::known("gen-2"),
            )
            .with_observed_version(ModuleVersionEvidence::new(
                "9.34",
                SourceGeneration::known("gen-2"),
            ));
            detect_mojo_base(&AdapterDetectionInput::new(
                mojo_base_descriptor(),
                receipt(
                    "gen-2",
                    vec![ModuleSelectorEvaluation::new(
                        "Mojo::Base",
                        ModuleSelectorOutcome::Matched {
                            activation,
                            evidence_class: DetectionEvidenceClass::ResolvedModule,
                        },
                    )],
                ),
                None,
                AdapterCancellation::active(),
            ))
        };
        let statement = "use Mojo::Base 'Mojolicious';";
        let anchor = MojoBaseSiteAnchor::new(
            Some("MyApp".to_string()),
            0,
            statement.len() as u32,
            statement.find("'Mojolicious'").map(|o| (o as u32, (o + 13) as u32)),
            SourceGeneration::known("gen-1"),
        );
        let base = mojo_base_activation_facts(
            &detection,
            &anchor,
            &parse_mojo_base_import_args(&["'Mojolicious'".to_string()]),
        );
        assert!(!base.is_exact(), "the upstream activation is stale");
        let facts = mojolicious_role_facts_from_mojo_base(&base);
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }
        ));
    }

    #[test]
    fn derived_roles_declare_the_unresolved_parent_limitation() {
        // The parent is a literal spelling proven in source, not a resolved
        // module; the claim must travel with the facts.
        let facts =
            mojolicious_role_facts_from_mojo_base(&mojo_base_facts("'Mojolicious'", "gen-1"));
        assert!(
            facts.limitations.iter().any(|limitation| limitation == DERIVED_PARENT_LIMITATION),
            "derived facts must carry the unresolved-parent limitation: {:?}",
            facts.limitations
        );
    }

    #[test]
    fn every_profile_declares_the_shadow_and_non_goal_limitations() {
        let lite = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        let derived =
            mojolicious_role_facts_from_mojo_base(&mojo_base_facts("'Mojolicious'", "gen-1"));
        for expected in SHADOW_LIMITATIONS {
            assert!(lite.limitations.iter().any(|l| l == expected), "lite missing {expected}");
            assert!(
                derived.limitations.iter().any(|l| l == expected),
                "derived missing {expected}"
            );
        }
    }

    // -----------------------------------------------------------------
    // Evidence integrity (Devin review on PR #14266)
    // -----------------------------------------------------------------

    #[test]
    fn a_selector_row_resolving_to_another_module_is_a_conflict() {
        // The row is filed under `Mojolicious::Lite` but resolves to a
        // different module. A direct detector consumer must not receive a
        // Detected Mojolicious identity from it.
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    "Some::Other::Module",
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        assert!(matches!(
            detect_mojolicious_lite(&input).outcome,
            DetectionOutcome::Conflicting { .. }
        ));
    }

    #[test]
    fn fabricated_contributing_module_cannot_become_exact_activation() {
        // A result can be deserialized or mutated: it may carry a genuine
        // input identity beside contributing evidence that was never observed.
        let mut detection = detect_mojolicious_lite(&lite_input("gen-1"));
        assert!(matches!(detection.outcome, DetectionOutcome::Detected { .. }));
        detection.contributing_modules = vec![
            ModuleActivationIdentity::new(
                "Totally::Different",
                Some(FileId(99)),
                SourceGeneration::known("gen-1"),
            )
            .with_observed_version(ModuleVersionEvidence::new(
                "9.34",
                SourceGeneration::known("gen-1"),
            )),
        ];
        let facts = mojolicious_lite_activation_facts(
            &detection,
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }
        ));
    }

    #[test]
    fn fabricated_version_evidence_cannot_become_exact_activation() {
        let mut detection = detect_mojolicious_lite(&lite_input("gen-1"));
        detection.version_evidence =
            Some(ModuleVersionEvidence::new("8.11", SourceGeneration::known("gen-1")));
        let facts = mojolicious_lite_activation_facts(
            &detection,
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(facts.role(), None);
    }

    #[test]
    fn a_reported_framework_version_must_match_the_observed_one() {
        let mut detection = detect_mojolicious_lite(&lite_input("gen-1"));
        detection.outcome = DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("9.99".to_string()),
        };
        let facts = mojolicious_lite_activation_facts(
            &detection,
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(facts.role(), None);
        assert_ne!(
            facts.framework_version, "9.99",
            "a fabricated version must not be published as an exact fact"
        );
    }

    #[test]
    fn name_only_identity_cannot_reach_exact_activation_through_the_facts_builder() {
        // Hand-build a self-consistent result that bypasses
        // `detect_mojolicious_lite` entirely: every field agrees with the
        // input row, but the row's evidence class is name-only and the
        // reported confidence is Low. An "exact activation" whose own
        // confidence is Low is self-contradictory and must not be minted.
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::NameOnly,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        let activation = ModuleActivationIdentity::new(
            MOJOLICIOUS_LITE_MODULE,
            Some(FileId(11)),
            SourceGeneration::known("gen-1"),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            "9.34",
            SourceGeneration::known("gen-1"),
        ));
        let version = ModuleVersionEvidence::new("9.34", SourceGeneration::known("gen-1"));
        let forged = AdapterDetectionResult::for_input(
            &input,
            DetectionOutcome::Detected {
                confidence: Confidence::Low,
                framework_version: Some("9.34".to_string()),
            },
        )
        .with_contributing_modules(vec![activation])
        .with_version_evidence(version);

        let facts =
            mojolicious_lite_activation_facts(&forged, &lite_anchor("main", "gen-1"), &parse(&[]));
        assert_eq!(facts.role(), None, "name-only identity must not own a role");
        assert!(
            matches!(facts.outcome, MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }),
            "the refusal must be typed, not a silent absence: {:?}",
            facts.outcome
        );
        assert!(
            facts.framework_version.is_empty(),
            "a refused fact must publish no observed framework version"
        );
    }

    #[test]
    fn name_only_identity_is_refused_even_when_high_confidence_is_claimed() {
        // Isolates the evidence-class gate from the confidence gate: a forged
        // result can simply assert High confidence. Exactness must still turn
        // on the *resolved identity class* of the observed row, which a
        // forger does not get to restate.
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![lite_evaluation(
                    MOJOLICIOUS_LITE_MODULE,
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::NameOnly,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        let activation = ModuleActivationIdentity::new(
            MOJOLICIOUS_LITE_MODULE,
            Some(FileId(11)),
            SourceGeneration::known("gen-1"),
        )
        .with_observed_version(ModuleVersionEvidence::new(
            "9.34",
            SourceGeneration::known("gen-1"),
        ));
        let forged = AdapterDetectionResult::for_input(
            &input,
            DetectionOutcome::Detected {
                confidence: Confidence::High,
                framework_version: Some("9.34".to_string()),
            },
        )
        .with_contributing_modules(vec![activation])
        .with_version_evidence(ModuleVersionEvidence::new(
            "9.34",
            SourceGeneration::known("gen-1"),
        ));

        let facts =
            mojolicious_lite_activation_facts(&forged, &lite_anchor("main", "gen-1"), &parse(&[]));
        assert_eq!(facts.role(), None, "name-only identity must not own a role");
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }
        ));
    }

    // -----------------------------------------------------------------
    // Second review round on the repaired head
    // -----------------------------------------------------------------

    #[test]
    fn an_unsatisfied_source_version_requirement_refuses_the_role() {
        // `use Mojolicious::Lite 99.0;` dies at compile time against an
        // installed 9.34, so it must never own a role.
        let mut evidence = parse(&[]);
        evidence.source_version_requirement = Some("99.0".to_string());
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &evidence,
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::UnsupportedVersionOrProfile { .. }
        ));
    }

    #[test]
    fn a_satisfied_source_version_requirement_keeps_the_role() {
        // Positive control: equal and lower requirements both activate.
        for required in ["9.34", "8.0"] {
            let mut evidence = parse(&[]);
            evidence.source_version_requirement = Some(required.to_string());
            let facts = mojolicious_lite_activation_facts(
                &detect_mojolicious_lite(&lite_input("gen-1")),
                &lite_anchor("main", "gen-1"),
                &evidence,
            );
            assert_eq!(
                facts.role(),
                Some(MojoliciousRole::LiteApplication),
                "requirement {required} is satisfied by 9.34"
            );
        }
    }

    #[test]
    fn a_vstring_requirement_is_captured_not_discarded() {
        assert_eq!(parse(&["v9.34"]).source_version_requirement, Some("9.34".to_string()));
        assert_eq!(
            mojolicious_lite_import_evidence(&[], false, Some("9.34")).source_version_requirement,
            Some("9.34".to_string())
        );
    }

    #[test]
    fn an_empty_or_reversed_site_interval_refuses_the_role() {
        // A fabricated anchor is not an extracted import, however current its
        // generation is.
        for (start, end) in [(0u32, 0u32), (40u32, 10u32)] {
            let anchor = MojoliciousSiteAnchor::new(
                Some("main".to_string()),
                start,
                end,
                SourceGeneration::known("gen-1"),
            );
            let facts = mojolicious_lite_activation_facts(
                &detect_mojolicious_lite(&lite_input("gen-1")),
                &anchor,
                &parse(&[]),
            );
            assert_eq!(facts.role(), None, "interval [{start}, {end}) must not anchor a role");
            assert!(matches!(
                facts.outcome,
                MojoliciousActivationOutcome::StaleOrIncompleteInput { .. }
            ));
        }
    }

    #[test]
    fn a_missing_module_is_not_complete_evidence_of_absence() {
        // A dependency that is not installed is unavailable, not proof that
        // the package owns no role.
        let input = AdapterDetectionInput::new(
            mojolicious_lite_descriptor(),
            receipt(
                "gen-1",
                vec![ModuleSelectorEvaluation::new(
                    MOJOLICIOUS_LITE_MODULE,
                    ModuleSelectorOutcome::Absent,
                )],
            ),
            None,
            AdapterCancellation::active(),
        );
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&input),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(facts.role(), None);
        assert!(
            matches!(
                facts.outcome,
                MojoliciousActivationOutcome::MissingOrUnavailableModule { .. }
            ),
            "expected a missing-module state, got {:?}",
            facts.outcome
        );
    }

    #[test]
    fn a_quoted_option_containing_spaces_stays_one_unreviewed_option() {
        // The parser keeps `'foo bar'` whole. Fragmenting it would make a
        // well-formed unreviewed option read as unterminated recovered source.
        let evidence = parse(&["'foo bar'"]);
        assert_eq!(evidence.unmodeled_options, vec!["'foo bar'".to_string()]);
        assert_eq!(
            evidence.selection,
            MojoliciousLiteImportSelection::Default,
            "a quoted literal is not malformed source"
        );
    }

    #[test]
    fn an_exact_role_always_carries_high_confidence() {
        // The invariant behind the gate above: exactness and confidence can
        // never disagree on a published fact.
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert!(facts.is_exact());
        assert_eq!(facts.confidence, Confidence::High);
    }

    // -----------------------------------------------------------------
    // Import-form semantics (Devin review on PR #14266)
    // -----------------------------------------------------------------

    #[test]
    fn an_explicit_empty_import_list_suppresses_the_role() {
        // `use Mojolicious::Lite ();` loads the module but calls no `import`,
        // so the Lite DSL is never installed.
        let evidence = mojolicious_lite_import_evidence(&[], true, None);
        assert_eq!(evidence.selection, MojoliciousLiteImportSelection::ImportSuppressed);
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &evidence,
        );
        assert_eq!(facts.role(), None);
        assert!(matches!(
            facts.outcome,
            MojoliciousActivationOutcome::AbsentWithCompleteEvidence { .. }
        ));
    }

    #[test]
    fn a_bare_import_is_still_exact_beside_the_suppressed_form() {
        // Negative control for the suppression fix: the ordinary import must
        // keep its role.
        assert_eq!(
            mojolicious_lite_import_evidence(&[], false, None).selection,
            MojoliciousLiteImportSelection::Default
        );
    }

    #[test]
    fn a_hash_import_argument_is_a_dynamic_boundary() {
        // The parser emits `%options` as the tokens `%` and `options`; a
        // computed hash must not become an unmodeled literal option.
        assert!(matches!(
            parse(&["%", "options"]).selection,
            MojoliciousLiteImportSelection::Dynamic { .. }
        ));
        assert!(matches!(
            parse(&["%opts"]).selection,
            MojoliciousLiteImportSelection::Dynamic { .. }
        ));
    }

    #[test]
    fn a_vstring_version_requirement_is_not_an_import_option() {
        // `use Mojolicious::Lite v9.34;` requires a version and still imports
        // the DSL with no arguments.
        let evidence = parse(&["v9.34"]);
        assert_eq!(evidence.selection, MojoliciousLiteImportSelection::Default);
        assert!(evidence.unmodeled_options.is_empty());
        let facts = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &evidence,
        );
        assert_eq!(facts.role(), Some(MojoliciousRole::LiteApplication));
    }

    #[test]
    fn a_vstring_beside_signatures_keeps_both_facts() {
        let evidence = parse(&["v9.34", "-signatures"]);
        assert_eq!(evidence.selection, MojoliciousLiteImportSelection::Signatures);
        assert!(evidence.signatures);
    }

    #[test]
    fn a_vstring_shaped_option_in_a_later_slot_still_widens_the_profile() {
        // Only the leading token can be a version requirement; a later one is
        // an ordinary unreviewed option.
        let evidence = parse(&["-signatures", "v9.34"]);
        assert_eq!(evidence.unmodeled_options, vec!["v9.34".to_string()]);
    }

    #[test]
    fn a_bareword_starting_with_v_is_not_a_version_requirement() {
        assert!(!is_version_requirement("verbose"));
        assert!(!is_version_requirement("v"));
        assert!(is_version_requirement("v9.34"));
    }

    #[test]
    fn the_lite_profile_never_derives_an_application_or_controller_role() {
        // Containment: the three roles come from distinct evidence and must
        // not be interchangeable.
        let lite = mojolicious_lite_activation_facts(
            &detect_mojolicious_lite(&lite_input("gen-1")),
            &lite_anchor("main", "gen-1"),
            &parse(&[]),
        );
        assert_eq!(lite.role(), Some(MojoliciousRole::LiteApplication));
        assert_ne!(lite.role(), Some(MojoliciousRole::Application));
        assert_ne!(lite.role(), Some(MojoliciousRole::Controller));
    }
}
