//! Registry-backed Mojo::Base activation and profile facts (#9681).
//!
//! This adapter is built directly on the checked framework SDK
//! ([`crate::framework"]). It is a shadow adapter: its output is
//! comparison/receipt material only and cannot become publication authority
//! until the registry-dispatch and shard-publication issues land. Exact
//! activation requires a resolved `Mojo::Base` module identity whose observed
//! version satisfies the reviewed constraint plus one reviewed static import
//! form (`-base`, or a literal parent, optionally with `-signatures`) — a
//! module merely *named* `Mojo::Base` (name-only evidence, unresolved
//! identity, an unsupported version, or a non-import reference such as a
//! `has` call) is not exact activation.
//!
//! Nested `Mojo::Base::*` modules are intentionally not selectors of this
//! descriptor: `use Mojo::Base::_RoleBase` and internal helpers never
//! activate, mirroring the single-exact-selector containment of the Dancer2
//! adapter at the same seam.
//!
//! This adapter implements detection/profile identity only. It does not
//! interpret `has`, emit accessor or parent-edge facts, or change any
//! provider surface (#9681 non-goals).

use crate::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    AdapterId, DetectionAbsenceReason, DetectionOutcome, ModuleActivationIdentity,
    ModuleSelectorEvaluation, ModuleSelectorOutcome, UnavailableReason,
};
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

/// Build the Mojo::Base adapter descriptor.
///
/// Shadow disposition: this adapter's facts are comparison-only and cannot
/// become publication authority (the SDK's authority validator refuses
/// non-production output by design).
#[must_use]
pub fn mojo_base_descriptor() -> AdapterDescriptor {
    AdapterDescriptor::new(
        MOJO_BASE_ADAPTER_ID,
        "mojo-base",
        MOJO_BASE_FRAMEWORK_NAME,
        Some(MOJO_BASE_VERSION_CONSTRAINT.to_string()),
        MOJO_BASE_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    )
}

/// Run the registry-backed Mojo::Base detection over one checked input.
///
/// Only the descriptor-owned `Mojo::Base` selector participates; nested
/// `Mojo::Base::*` modules never activate this adapter. The input descriptor
/// must be the canonical [`mojo_base_descriptor`] value: a foreign
/// descriptor's selectors are never adopted as Mojo::Base evidence. A
/// pre-cancelled admission snapshot fails closed to
/// `DetectionOutcome::Cancelled` before any module evidence is evaluated,
/// and duplicate or contradictory rows for one owned selector stay a
/// conflict instead of becoming first-wins evidence.
#[must_use]
pub fn detect_mojo_base(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    if input.descriptor != mojo_base_descriptor() {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: "input descriptor does not match the canonical Mojo::Base adapter \
                                                 descriptor"
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
                        "selector `{}` carries {} terminal evaluations; completeness requires \
                         exactly one",
                        descriptor.required_module_selectors[0],
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
            let identity_confidence = evidence_class.confidence_ceiling();
            if identity_confidence != Confidence::High {
                // A module named Mojo::Base without resolved supported
                // identity is not exact activation (#9681).
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "Mojo::Base selector matched with {identity_confidence:?} identity \
                             evidence; exact activation requires resolved module identity"
                        ),
                    },
                );
            }
            match &activation.observed_version {
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: "Mojo::Base activation lacks observed version evidence; the \
                                 reviewed version constraint cannot be checked"
                            .to_string(),
                    },
                ),
                Some(version) => {
                    match crate::framework::version_constraint_matches(
                        MOJO_BASE_VERSION_CONSTRAINT,
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
                                    "observed Mojo::Base version `{}` is not comparable with the \
                                     reviewed constraint `{MOJO_BASE_VERSION_CONSTRAINT}`",
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
    Dynamic { reason: String },
    /// Recovered or contradictory import spelling — the source could not be
    /// interpreted as one reviewed activation form.
    Malformed { reason: String },
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

/// Parse `use Mojo::Base` import arguments (parser token strings) into
/// evidence.
///
/// Reviewed literal forms: `-base`, `'Parent'`/`"Parent"` (or an equivalent
/// bareword module spelling), each optionally followed by `-signatures`.
/// `-strict` or an empty argument list is a strict/warnings-only import, not
/// an activation. Computed parents become explicit dynamic selections.
/// Unterminated quotes and contradictory base+parent spellings become
/// explicit malformed selections instead of being normalized into a profile.
#[must_use]
pub fn parse_mojo_base_import_args(args: &[String]) -> MojoBaseImportEvidence {
    let mut evidence = MojoBaseImportEvidence::default();
    let tokens = normalize_import_tokens(args);
    for token in &tokens {
        if token == "-signatures" {
            if evidence.parent == MojoBaseParentSelection::None {
                // `Mojo::Base::import` binds the first argument to the
                // base/parent slot, so a leading `-signatures` is not a
                // reviewed activation form: it would become the parent
                // spelling itself. Classify it malformed, fail-closed.
                evidence.parent = MojoBaseParentSelection::Malformed {
                    reason: "flag `-signatures` occupies the base/parent slot".to_string(),
                };
            }
            // The reviewed import option; valid in any flag position.
            evidence.signatures = true;
            continue;
        }
        if evidence.parent == MojoBaseParentSelection::None {
            evidence.parent = classify_base_slot(token);
            continue;
        }
        classify_option_slot(token, &mut evidence);
    }
    if evidence.parent == MojoBaseParentSelection::None {
        // `use Mojo::Base;` with no arguments imports strict/warnings only.
        evidence.parent = MojoBaseParentSelection::StrictOnly;
    }
    evidence
}

/// Classify the first positional argument: the base/parent slot of
/// `Mojo::Base::import`.
fn classify_base_slot(token: &str) -> MojoBaseParentSelection {
    match token {
        "-base" => MojoBaseParentSelection::Base,
        "-strict" => MojoBaseParentSelection::StrictOnly,
        _ => classify_parent_value(token),
    }
}

/// Classify later arguments. `Mojo::Base::import` only greps its flag list
/// for `-signatures`; every other extra argument is ignored at runtime and
/// therefore stays an unreviewed option here instead of silently widening
/// the profile.
fn classify_option_slot(token: &str, evidence: &mut MojoBaseImportEvidence) {
    evidence.unmodeled_options.push(token.to_string());
}

fn classify_parent_value(token: &str) -> MojoBaseParentSelection {
    if let Some(reason) = malformed_reason(token) {
        return MojoBaseParentSelection::Malformed { reason };
    }
    if let Some((style, inner)) = unquote_styled(token) {
        // `Mojo::Base::import` treats a falsy base (`''`/`'0'`) as no parent:
        // the import degrades to strict/warnings only.
        if inner.is_empty() || inner == "0" {
            return MojoBaseParentSelection::StrictOnly;
        }
        // Double-quoted spellings interpolate in Perl; any interpolation
        // sigil makes the parent computed, not a static literal.
        if style == QuoteStyle::Double && (inner.contains('$') || inner.contains('@')) {
            return MojoBaseParentSelection::Dynamic {
                reason: format!("double-quoted parent `{token}` interpolates at runtime"),
            };
        }
        return MojoBaseParentSelection::Literal(inner);
    }
    let dynamic = token.starts_with('$')
        || token.starts_with('@')
        || token.starts_with('%')
        || token.starts_with('\\')
        || token.contains('(');
    if dynamic {
        return MojoBaseParentSelection::Dynamic {
            reason: format!("parent expression `{token}` is computed at runtime"),
        };
    }
    // Bareword parents are runtime-equivalent to quoted spellings.
    MojoBaseParentSelection::Literal(token.to_string())
}

/// Quote style of a delimited import token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteStyle {
    Single,
    Double,
}

fn unquote_styled(token: &str) -> Option<(QuoteStyle, String)> {
    let (style, close): (QuoteStyle, char) = if token.starts_with('\'') {
        (QuoteStyle::Single, '\'')
    } else if token.starts_with('"') {
        (QuoteStyle::Double, '"')
    } else {
        return None;
    };
    if !token.ends_with(close) || token.len() < 2 {
        return None;
    }
    Some((style, token[1..token.len() - 1].to_string()))
}

/// A quote-delimited token whose closing quote is missing — the parser
/// recovered the source; the spelling is not an exact literal.
fn malformed_reason(token: &str) -> Option<String> {
    let first = token.chars().next()?;
    if (first == '\'' || first == '"') && !token.ends_with(first) {
        return Some(format!("unterminated quote in import argument `{token}`"));
    }
    if token == "'" || token == "\"" {
        return Some(format!("empty quote fragment in import argument `{token}`"));
    }
    None
}

fn normalize_import_tokens(args: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for arg in args {
        let token = arg.trim();
        if token.is_empty() || token == "," || token == "=>" || token == "(" || token == ")" {
            continue;
        }
        tokens.push(token.to_string());
    }
    tokens
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
    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.outcome.is_exact()
    }
}

const SHADOW_LIMITATIONS: &[&str] = &[
    "shadow adapter: comparison-only output; not publication authority",
    "has/accessor and parent-edge semantics are intentionally not modeled (#9681 non-goals)",
];

/// Build typed activation/profile facts from one detection result plus the
/// site anchor and import evidence.
///
/// Ordering is fail-closed: instrument failures and module availability
/// first, then source-level classification (malformed, dynamic, strict-only),
/// then evidence completeness (contributing module, version evidence, and a
/// reconciled current input identity), generation staleness with
/// known-generation requirements, and the reviewed-profile check. Only a
/// `Detected` outcome carrying contributing module and version evidence
/// under a reconciled input identity with current known site, module, and
/// version generations and no unreviewed import options yields an exact
/// activation.
#[must_use]
pub fn mojo_base_activation_facts(
    detection: &AdapterDetectionResult,
    anchor: &MojoBaseSiteAnchor,
    evidence: &MojoBaseImportEvidence,
) -> MojoBaseActivationFacts {
    let mut facts = MojoBaseActivationFacts {
        outcome: MojoBaseActivationOutcome::AbsentWithCompleteEvidence { reason: String::new() },
        profile_version: MOJO_BASE_PROFILE_VERSION,
        package: anchor.package.clone(),
        source_interval: (anchor.span_start_byte, anchor.span_end_byte),
        parent_range: anchor.parent_range,
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
        limitations: SHADOW_LIMITATIONS.iter().map(ToString::to_string).collect(),
    };

    // 1. Instrument state: nothing is knowable about activation.
    if let Some(reason) = instrument_failure_reason(&detection.outcome) {
        facts.outcome = MojoBaseActivationOutcome::InstrumentFailure { reason };
        return facts;
    }

    // 2. Module availability and identity.
    match &detection.outcome {
        DetectionOutcome::Unavailable { reason } => {
            facts.outcome = MojoBaseActivationOutcome::MissingOrUnavailableModule {
                reason: format!("{reason:?}"),
            };
            return facts;
        }
        DetectionOutcome::Conflicting { conflict_descriptions } => {
            facts.outcome = MojoBaseActivationOutcome::AmbiguousOrConflictingModule {
                reason: conflict_descriptions.join("; "),
            };
            return facts;
        }
        _ => {}
    }

    // 3. Source-level classification of the import site.
    match &evidence.parent {
        MojoBaseParentSelection::Malformed { reason } => {
            facts.outcome =
                MojoBaseActivationOutcome::RecoveredOrMalformedSource { reason: reason.clone() };
            return facts;
        }
        MojoBaseParentSelection::Dynamic { reason } => {
            facts.outcome =
                MojoBaseActivationOutcome::DynamicOrUnmodeledParent { reason: reason.clone() };
            return facts;
        }
        _ => {}
    }

    // 4. Detection-level version/profile support.
    match &detection.outcome {
        DetectionOutcome::Unsupported { reason } => {
            facts.outcome =
                MojoBaseActivationOutcome::UnsupportedVersionOrProfile { reason: reason.clone() };
            return facts;
        }
        DetectionOutcome::Absent {
            reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
        } => {
            facts.outcome = MojoBaseActivationOutcome::UnsupportedVersionOrProfile {
                reason: format!(
                    "observed version does not satisfy the reviewed constraint \
                     `{MOJO_BASE_VERSION_CONSTRAINT}`"
                ),
            };
            return facts;
        }
        DetectionOutcome::Absent { reason } => {
            facts.outcome = MojoBaseActivationOutcome::AbsentWithCompleteEvidence {
                reason: format!("{reason:?}"),
            };
            return facts;
        }
        _ => {}
    }

    // 5. Strict-only imports never activate inheritance or `has`.
    if evidence.parent == MojoBaseParentSelection::StrictOnly {
        facts.outcome = MojoBaseActivationOutcome::AbsentWithCompleteEvidence {
            reason: "strict/warnings-only import (`-strict` or no arguments) does not \
                     activate Mojo::Base inheritance"
                .to_string(),
        };
        return facts;
    }

    let DetectionOutcome::Detected { confidence, framework_version } = &detection.outcome else {
        // Unreachable for SDK outcomes not matched above; stays bounded.
        facts.outcome = MojoBaseActivationOutcome::StaleOrIncompleteInput {
            reason: format!("unhandled detection outcome {:?}", detection.outcome),
        };
        return facts;
    };
    facts.confidence = *confidence;
    facts.framework_version = framework_version.clone().unwrap_or_default();

    // 6. Evidence completeness: a raw or deserialized `Detected` result does
    // not become exact activation without its contributing module identity,
    // version evidence, and a current input identity whose descriptor,
    // owned selector, module name, scope, and generations reconcile with
    // this detection.
    if detection.contributing_modules.is_empty() || detection.version_evidence.is_none() {
        facts.outcome = MojoBaseActivationOutcome::StaleOrIncompleteInput {
            reason: "detected result lacks contributing module or version evidence; raw \
                     results cannot become exact activation"
                .to_string(),
        };
        return facts;
    }
    if let Some(reason) = identity_reconciliation_reason(detection) {
        facts.outcome = MojoBaseActivationOutcome::StaleOrIncompleteInput { reason };
        return facts;
    }

    // 7. Generation staleness: site, module, and version evidence must all be
    // known and current for the detection generation.
    if let Some(reason) = staleness_reason(detection, anchor) {
        facts.outcome = MojoBaseActivationOutcome::StaleOrIncompleteInput { reason };
        return facts;
    }
    facts.source_generation = anchor.source_generation.clone();

    // 8. The reviewed profile must cover every import option.
    if !evidence.unmodeled_options.is_empty() {
        facts.outcome = MojoBaseActivationOutcome::UnsupportedVersionOrProfile {
            reason: format!(
                "import carries options outside profile {MOJO_BASE_PROFILE_VERSION}: {}",
                evidence.unmodeled_options.join(", ")
            ),
        };
        return facts;
    }

    // 9. Exact activation under the reviewed profile. A literal parent
    // additionally requires its located source range to sit inside the
    // import interval (source anchors are load-bearing, not decorative).
    facts.outcome = match &evidence.parent {
        MojoBaseParentSelection::Base => MojoBaseActivationOutcome::ExactBaseActivation,
        MojoBaseParentSelection::Literal(parent) => {
            if let Some((range_start, range_end)) = anchor.parent_range
                && range_start >= anchor.span_start_byte
                && range_end <= anchor.span_end_byte
                && range_end > range_start
            {
                MojoBaseActivationOutcome::ExactLiteralParentActivation { parent: parent.clone() }
            } else {
                MojoBaseActivationOutcome::StaleOrIncompleteInput {
                    reason: "literal parent lacks a source range inside the import interval"
                        .to_string(),
                }
            }
        }
        other => MojoBaseActivationOutcome::AbsentWithCompleteEvidence {
            reason: format!("parent selection {other:?} does not carry an exact profile"),
        },
    };
    facts
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
/// the canonical descriptor, one terminal `Mojo::Base` selector evaluation
/// whose matched module is `Mojo::Base`, and an observation receipt from the
/// detection generation.
fn identity_reconciliation_reason(detection: &AdapterDetectionResult) -> Option<String> {
    let Some(identity) = &detection.input_identity else {
        return Some(
            "detected result carries no current input identity; fabricated evidence cannot \
             become exact activation"
                .to_string(),
        );
    };
    if identity.descriptor != mojo_base_descriptor() {
        return Some(
            "input identity belongs to a different adapter descriptor; it cannot support \
             Mojo::Base exact activation"
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
        .filter(|evaluation| evaluation.selector == MOJO_BASE_FRAMEWORK_NAME)
        .collect();
    let [evaluation] = owned.as_slice() else {
        return Some(format!(
            "input identity carries {} terminal evaluations for selector `{MOJO_BASE_FRAMEWORK_NAME}`; \
             exactly one is required",
            owned.len()
        ));
    };
    match &evaluation.outcome {
        ModuleSelectorOutcome::Matched { activation, .. }
            if activation.module_name == MOJO_BASE_FRAMEWORK_NAME
                && activation.generation == detection.project_generation =>
        {
            None
        }
        _ => Some(
            "the owned selector's terminal evaluation does not reconcile with the detection"
                .to_string(),
        ),
    }
}

fn staleness_reason(
    detection: &AdapterDetectionResult,
    anchor: &MojoBaseSiteAnchor,
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

    #[test]
    fn descriptor_is_mojo_base_selective_and_shadow() {
        let descriptor = mojo_base_descriptor();
        assert_eq!(descriptor.required_module_selectors, vec!["Mojo::Base"]);
        assert_eq!(descriptor.disposition, AdapterDisposition::Shadow);
        assert_eq!(
            descriptor.framework_version_constraint.as_deref(),
            Some(MOJO_BASE_VERSION_CONSTRAINT)
        );
    }

    #[test]
    fn nested_mojo_base_modules_are_not_selectors() {
        assert!(
            !mojo_base_descriptor()
                .required_module_selectors
                .iter()
                .any(|selector| selector.starts_with("Mojo::Base::"))
        );
    }

    #[test]
    fn adapter_ids_are_distinct_across_shadow_adapters() {
        assert_ne!(MOJO_BASE_ADAPTER_ID, crate::framework_adapters::dancer2::DANCER2_ADAPTER_ID);
    }

    fn parse(args: &[&str]) -> MojoBaseImportEvidence {
        let owned: Vec<String> = args.iter().map(ToString::to_string).collect();
        parse_mojo_base_import_args(&owned)
    }

    #[test]
    fn parses_base_form() {
        let evidence = parse(&["-base"]);
        assert_eq!(evidence.parent, MojoBaseParentSelection::Base);
        assert!(!evidence.signatures);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn parses_literal_parent_with_signatures() {
        let evidence = parse(&["'Mojo::EventEmitter'", "-signatures"]);
        assert_eq!(
            evidence.parent,
            MojoBaseParentSelection::Literal("Mojo::EventEmitter".to_string())
        );
        assert!(evidence.signatures);
    }

    #[test]
    fn parses_bareword_parent_as_equivalent_literal() {
        let evidence = parse(&["Parent"]);
        assert_eq!(evidence.parent, MojoBaseParentSelection::Literal("Parent".to_string()));
    }

    #[test]
    fn strict_only_import_is_not_an_activation() {
        assert_eq!(parse(&["-strict"]).parent, MojoBaseParentSelection::StrictOnly);
        assert_eq!(parse(&[]).parent, MojoBaseParentSelection::StrictOnly);
    }

    #[test]
    fn falsy_quoted_parent_is_strict_only() {
        // `Mojo::Base::import` treats a falsy base (`''`/`'0'`) as no parent.
        assert_eq!(parse(&["''"]).parent, MojoBaseParentSelection::StrictOnly);
        assert_eq!(parse(&["'0'"]).parent, MojoBaseParentSelection::StrictOnly);
        assert_eq!(parse(&["\"\""]).parent, MojoBaseParentSelection::StrictOnly);
    }

    #[test]
    fn leading_signatures_flag_is_malformed() {
        // `Mojo::Base::import` binds the first argument to the base/parent
        // slot; a leading `-signatures` is not a reviewed activation form.
        let evidence = parse(&["-signatures"]);
        assert!(matches!(evidence.parent, MojoBaseParentSelection::Malformed { .. }));
        assert!(evidence.signatures, "the flag is still recorded where it appears");
    }

    #[test]
    fn interpolated_double_quoted_parent_is_dynamic() {
        let evidence = parse(&["\"$parent\""]);
        assert!(
            matches!(evidence.parent, MojoBaseParentSelection::Dynamic { .. }),
            "double-quoted interpolation must not become a static literal"
        );
        let static_double = parse(&["\"Parent\""]);
        assert_eq!(static_double.parent, MojoBaseParentSelection::Literal("Parent".to_string()));
    }

    #[test]
    fn computed_parent_is_a_dynamic_boundary_not_a_literal() {
        let evidence = parse(&["$parent"]);
        assert!(
            matches!(evidence.parent, MojoBaseParentSelection::Dynamic { .. }),
            "computed parent must not silently become a literal"
        );
    }

    #[test]
    fn pre_cancelled_input_fails_closed() {
        let input = AdapterDetectionInput::new(
            mojo_base_descriptor(),
            crate::framework::ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                vec![ModuleSelectorEvaluation::new(
                    "Mojo::Base",
                    ModuleSelectorOutcome::Matched {
                        activation: ModuleActivationIdentity::new(
                            "Mojo::Base",
                            None,
                            SourceGeneration::known("gen-1"),
                        )
                        .with_observed_version(
                            crate::framework::ModuleVersionEvidence::new(
                                "9.34",
                                SourceGeneration::known("gen-1"),
                            ),
                        ),
                        evidence_class: crate::framework::DetectionEvidenceClass::ResolvedModule,
                    },
                )],
            ),
            None,
            crate::framework::AdapterCancellation::cancelled(),
        );
        assert_eq!(detect_mojo_base(&input).outcome, DetectionOutcome::Cancelled);
    }

    #[test]
    fn unterminated_quote_is_malformed_not_a_literal() {
        let evidence = parse(&["'Pare"]);
        assert!(
            matches!(evidence.parent, MojoBaseParentSelection::Malformed { .. }),
            "recovered quote fragments must not normalize into an exact profile"
        );
    }

    #[test]
    fn extra_base_slot_arguments_stay_unmodeled() {
        // `Mojo::Base::import` ignores extra arguments after the base slot;
        // they widen the reviewed profile instead of becoming a second
        // parent or a malformed spelling.
        let evidence = parse(&["-base", "'Parent'"]);
        assert_eq!(evidence.parent, MojoBaseParentSelection::Base);
        assert_eq!(evidence.unmodeled_options, vec!["'Parent'".to_string()]);
    }

    #[test]
    fn unknown_import_options_are_recorded_not_dropped() {
        let evidence = parse(&["-base", "-future_flag"]);
        assert_eq!(evidence.parent, MojoBaseParentSelection::Base);
        assert_eq!(evidence.unmodeled_options, vec!["-future_flag".to_string()]);
    }

    #[test]
    fn structural_commas_do_not_become_options() {
        let evidence = parse(&["$parent", ",", "-signatures"]);
        assert!(matches!(evidence.parent, MojoBaseParentSelection::Dynamic { .. }));
        assert!(evidence.signatures);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn detected_result_without_module_evidence_is_not_exact() {
        let detection = AdapterDetectionResult::new(
            mojo_base_descriptor(),
            SourceGeneration::known("gen-1"),
            DetectionOutcome::Detected {
                confidence: Confidence::High,
                framework_version: Some("9.34".to_string()),
            },
        );
        let anchor = MojoBaseSiteAnchor::new(
            Some("App".to_string()),
            0,
            1,
            None,
            SourceGeneration::known("gen-1"),
        );
        let evidence = parse_mojo_base_import_args(&["-base".to_string()]);
        let facts = mojo_base_activation_facts(&detection, &anchor, &evidence);
        assert!(
            matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
            "a raw Detected result without contributing evidence must not become exact"
        );
    }

    #[test]
    fn foreign_descriptor_inputs_are_rejected() {
        let mut foreign = mojo_base_descriptor();
        foreign.framework_name = "Foo".to_string();
        foreign.required_module_selectors = vec!["Foo".to_string()];
        let input = AdapterDetectionInput::new(
            foreign,
            crate::framework::ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                vec![ModuleSelectorEvaluation::new(
                    "Foo",
                    ModuleSelectorOutcome::Matched {
                        activation: ModuleActivationIdentity::new(
                            "Foo",
                            None,
                            SourceGeneration::known("gen-1"),
                        )
                        .with_observed_version(
                            crate::framework::ModuleVersionEvidence::new(
                                "9.34",
                                SourceGeneration::known("gen-1"),
                            ),
                        ),
                        evidence_class: crate::framework::DetectionEvidenceClass::ResolvedModule,
                    },
                )],
            ),
            None,
            crate::framework::AdapterCancellation::active(),
        );
        let detection = detect_mojo_base(&input);
        assert!(
            matches!(detection.outcome, DetectionOutcome::Unsupported { .. }),
            "a foreign descriptor's selectors must not become Mojo::Base evidence, got {:?}",
            detection.outcome
        );
    }

    #[test]
    fn duplicate_selector_rows_are_a_conflict() {
        let input = AdapterDetectionInput::new(
            mojo_base_descriptor(),
            crate::framework::ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                vec![
                    matched_mojo_base_row("gen-1"),
                    ModuleSelectorEvaluation::new("Mojo::Base", ModuleSelectorOutcome::Absent),
                ],
            ),
            None,
            crate::framework::AdapterCancellation::active(),
        );
        let detection = detect_mojo_base(&input);
        assert!(
            matches!(detection.outcome, DetectionOutcome::Conflicting { .. }),
            "duplicate terminal evaluations must not become first-wins evidence, got {:?}",
            detection.outcome
        );
    }

    fn matched_mojo_base_row(generation: &str) -> ModuleSelectorEvaluation {
        ModuleSelectorEvaluation::new(
            "Mojo::Base",
            ModuleSelectorOutcome::Matched {
                activation: ModuleActivationIdentity::new(
                    "Mojo::Base",
                    None,
                    SourceGeneration::known(generation),
                )
                .with_observed_version(
                    crate::framework::ModuleVersionEvidence::new(
                        "9.34",
                        SourceGeneration::known(generation),
                    ),
                ),
                evidence_class: crate::framework::DetectionEvidenceClass::ResolvedModule,
            },
        )
    }

    #[test]
    fn fabricated_evidence_without_input_identity_is_not_exact() {
        // Contributing module and version evidence attached to a raw result
        // with no input identity still cannot reach exact activation.
        let detection = AdapterDetectionResult::new(
            mojo_base_descriptor(),
            SourceGeneration::known("gen-1"),
            DetectionOutcome::Detected {
                confidence: Confidence::High,
                framework_version: Some("9.34".to_string()),
            },
        )
        .with_contributing_modules(vec![
            ModuleActivationIdentity::new("Mojo::Base", None, SourceGeneration::known("gen-1"))
                .with_observed_version(crate::framework::ModuleVersionEvidence::new(
                    "9.34",
                    SourceGeneration::known("gen-1"),
                )),
        ])
        .with_version_evidence(crate::framework::ModuleVersionEvidence::new(
            "9.34",
            SourceGeneration::known("gen-1"),
        ));
        let anchor = MojoBaseSiteAnchor::new(
            Some("App".to_string()),
            0,
            1,
            None,
            SourceGeneration::known("gen-1"),
        );
        let evidence = parse_mojo_base_import_args(&["-base".to_string()]);
        let facts = mojo_base_activation_facts(&detection, &anchor, &evidence);
        assert!(
            matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
            "fabricated evidence without a current input identity must not become exact"
        );
    }

    #[test]
    fn all_unknown_generations_cannot_be_exact() {
        let input = AdapterDetectionInput::new(
            mojo_base_descriptor(),
            crate::framework::ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::Unknown,
                "sha256:fixture-input",
                vec![ModuleSelectorEvaluation::new(
                    "Mojo::Base",
                    ModuleSelectorOutcome::Matched {
                        activation: ModuleActivationIdentity::new(
                            "Mojo::Base",
                            None,
                            SourceGeneration::Unknown,
                        )
                        .with_observed_version(
                            crate::framework::ModuleVersionEvidence::new(
                                "9.34",
                                SourceGeneration::Unknown,
                            ),
                        ),
                        evidence_class: crate::framework::DetectionEvidenceClass::ResolvedModule,
                    },
                )],
            ),
            None,
            crate::framework::AdapterCancellation::active(),
        );
        let detection = detect_mojo_base(&input);
        assert!(detection.is_detected());
        let anchor =
            MojoBaseSiteAnchor::new(Some("App".to_string()), 0, 1, None, SourceGeneration::Unknown);
        let evidence = parse_mojo_base_import_args(&["-base".to_string()]);
        let facts = mojo_base_activation_facts(&detection, &anchor, &evidence);
        assert!(
            matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
            "mutually-equal unknown generations identify nothing and cannot be exact, got {:?}",
            facts.outcome
        );
    }

    #[test]
    fn literal_parent_without_located_range_degrades() {
        let detection = detected_result_for_facts();
        let missing = MojoBaseSiteAnchor::new(
            Some("App".to_string()),
            13,
            34,
            None,
            SourceGeneration::known("gen-1"),
        );
        let outside = MojoBaseSiteAnchor::new(
            Some("App".to_string()),
            13,
            34,
            Some((900, 950)),
            SourceGeneration::known("gen-1"),
        );
        let evidence = parse_mojo_base_import_args(&["'Parent'".to_string()]);
        for anchor in [missing, outside] {
            let facts = mojo_base_activation_facts(&detection, &anchor, &evidence);
            assert!(
                matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
                "literal parent without a contained source range must degrade, got {:?}",
                facts.outcome
            );
        }
        let contained = MojoBaseSiteAnchor::new(
            Some("App".to_string()),
            13,
            34,
            Some((24, 32)),
            SourceGeneration::known("gen-1"),
        );
        let facts = mojo_base_activation_facts(&detection, &contained, &evidence);
        assert!(
            matches!(facts.outcome, MojoBaseActivationOutcome::ExactLiteralParentActivation { .. }),
            "a contained literal range stays exact"
        );
    }

    fn detected_result_for_facts() -> AdapterDetectionResult {
        let input = AdapterDetectionInput::new(
            mojo_base_descriptor(),
            crate::framework::ModuleObservationReceipt::new(
                "module-resolver.v1",
                "root:fixture",
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                vec![matched_mojo_base_row("gen-1")],
            ),
            None,
            crate::framework::AdapterCancellation::active(),
        );
        detect_mojo_base(&input)
    }
}
