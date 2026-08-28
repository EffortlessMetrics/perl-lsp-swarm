//! Registry-backed Dancer2 activation, application identity, and scoped DSL
//! import facts (#8914).
//!
//! This adapter is built directly on the checked framework SDK
//! ([`crate::framework`]). It is a shadow adapter: its output is
//! comparison/receipt material only and cannot become publication authority
//! until the registry-dispatch and shard-publication issues land. Exact
//! activation requires a resolved `Dancer2` module identity whose observed
//! version satisfies the reviewed constraint — a module merely *named*
//! `Dancer2` (name-only evidence, unresolved identity, or an unsupported
//! version) is not exact activation.
//!
//! `Dancer2::Core` is intentionally not a selector of this descriptor: the
//! containment landed for #8910 (only an exact `use Dancer2` activates the
//! legacy analysis) is preserved here.

use crate::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    AdapterId, DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, UnavailableReason,
};
use crate::{Confidence, SourceGeneration};

/// Framework name handled by this adapter.
pub const DANCER2_FRAMEWORK_NAME: &str = "Dancer2";

/// Reviewed supported version range for the Dancer2 application DSL.
///
/// Covers the Dancer2 1.x series (the workspace fixture
/// `test_corpus/real_projects/dancer2_skeleton/lib/Dancer2.pm` carries
/// `1.1.1`). A Dancer v2.0+ release has not been reviewed.
pub const DANCER2_VERSION_CONSTRAINT: &str = ">=1.0.0,<2.0.0";

/// Provisional adapter identity.
///
/// The generic registry (#6821) owns final identity assignment; this stable
/// value is reserved for Dancer2 so shadow receipts remain comparable across
/// the registry extraction.
pub const DANCER2_ADAPTER_ID: AdapterId = AdapterId(0x0044_4E43);

/// Versioned identity of the reviewed default-DSL keyword contract.
///
/// The table is the reviewed Dancer2 1.x union: the v1.0.0 registry at
/// `PerlDancer/Dancer2@ab2a50a478f81b005d0a6f4f1ff2a1f6f9ce1aa5`
/// is the base, while `uri_for_route` was introduced by
/// `PerlDancer/Dancer2@1211dbf5e19ffbe46026ddf983c444ce1f5c0b6d`
/// and shipped in v1.1.0. Facts are filtered against the observed
/// framework version before publication. v3 also corrects `redirect`
/// and `cookie` scope, preserves the four upstream-deprecated exports,
/// and removes route/hook names that the reviewed DSL does not register
/// as keywords (#13089).
pub const DANCER2_DSL_CONTRACT_VERSION: &str = "dancer2-dsl.1-1.v3";
const DANCER2_DSL_1_0_CONTRACT_VERSION: &str = "dancer2-dsl.1-0.v3";
const DANCER2_URI_FOR_ROUTE_VERSION_CONSTRAINT: &str = ">=1.1.0,<2.0.0";

/// Reviewed versioned-descriptor schema revision for this adapter. Tracks
/// [`FRAMEWORK_ADAPTER_SCHEMA_VERSION`](crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION):
/// the descriptor travels on the adapter SDK wire, whose version 2 carries
/// the #8921 route-family fact kinds.
pub const DANCER2_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Reviewed default Dancer2 DSL keyword scope.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DslKeywordScope {
    /// Available in any package scope that activated the DSL.
    Global,
    /// Available only inside route handlers/hooks (request context).
    RouteHandlerOnly,
}

/// One reviewed default-DSL keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dancer2DslKeyword {
    /// DSL keyword name.
    pub name: &'static str,
    /// Availability scope in the reviewed contract.
    pub scope: DslKeywordScope,
    /// Whether the reviewed contract still exports the keyword but marks it
    /// deprecated (preserved as versioned metadata, never silently dropped).
    pub deprecated: bool,
}

const GLOBAL: DslKeywordScope = DslKeywordScope::Global;
const ROUTE: DslKeywordScope = DslKeywordScope::RouteHandlerOnly;
const fn kw(name: &'static str, scope: DslKeywordScope) -> Dancer2DslKeyword {
    Dancer2DslKeyword { name, scope, deprecated: false }
}
const fn deprecated_kw(name: &'static str, scope: DslKeywordScope) -> Dancer2DslKeyword {
    Dancer2DslKeyword { name, scope, deprecated: true }
}

/// Reviewed default Dancer2 1.x DSL keyword union.
///
/// Entries intentionally follow the v1.1.1 upstream `dsl_keywords`
/// registration order so review can compare the two tables without
/// reconstructing a second grouping. Per-activation facts filter additions
/// that were unavailable at the observed framework version. Hook names such
/// as `before` and `after` are operands to `hook`, not imported DSL keywords.
pub const DANCER2_DSL_KEYWORDS: &[Dancer2DslKeyword] = &[
    kw("any", GLOBAL),
    kw("app", GLOBAL),
    kw("captures", ROUTE),
    kw("config", GLOBAL),
    kw("content", ROUTE),
    kw("content_type", ROUTE),
    deprecated_kw("context", ROUTE),
    kw("cookie", ROUTE),
    kw("cookies", ROUTE),
    kw("dance", GLOBAL),
    kw("dancer_app", GLOBAL),
    kw("dancer_version", GLOBAL),
    kw("dancer_major_version", GLOBAL),
    kw("debug", GLOBAL),
    kw("decode_json", GLOBAL),
    kw("del", GLOBAL),
    kw("delayed", ROUTE),
    kw("dirname", GLOBAL),
    kw("done", ROUTE),
    kw("dsl", GLOBAL),
    kw("encode_json", GLOBAL),
    kw("engine", GLOBAL),
    kw("error", GLOBAL),
    kw("false", GLOBAL),
    kw("flush", ROUTE),
    kw("forward", ROUTE),
    kw("from_dumper", GLOBAL),
    kw("from_json", GLOBAL),
    kw("from_yaml", GLOBAL),
    kw("get", GLOBAL),
    kw("halt", ROUTE),
    deprecated_kw("header", ROUTE),
    deprecated_kw("headers", ROUTE),
    kw("hook", GLOBAL),
    kw("info", GLOBAL),
    kw("log", GLOBAL),
    kw("mime", GLOBAL),
    kw("options", GLOBAL),
    kw("param", ROUTE),
    kw("params", ROUTE),
    kw("query_parameters", ROUTE),
    kw("body_parameters", ROUTE),
    kw("route_parameters", ROUTE),
    kw("pass", ROUTE),
    kw("patch", GLOBAL),
    kw("path", GLOBAL),
    kw("post", GLOBAL),
    kw("prefix", GLOBAL),
    kw("prepare_app", GLOBAL),
    kw("psgi_app", GLOBAL),
    deprecated_kw("push_header", ROUTE),
    kw("push_response_header", ROUTE),
    kw("put", GLOBAL),
    kw("redirect", ROUTE),
    kw("request", ROUTE),
    kw("request_data", ROUTE),
    kw("request_header", ROUTE),
    kw("response", ROUTE),
    kw("response_header", ROUTE),
    kw("response_headers", ROUTE),
    kw("runner", GLOBAL),
    kw("send_as", ROUTE),
    kw("send_error", ROUTE),
    kw("send_file", ROUTE),
    kw("session", ROUTE),
    kw("set", GLOBAL),
    kw("setting", GLOBAL),
    kw("splat", ROUTE),
    kw("start", GLOBAL),
    kw("status", ROUTE),
    kw("template", GLOBAL),
    kw("to_app", GLOBAL),
    kw("to_dumper", GLOBAL),
    kw("to_json", GLOBAL),
    kw("to_yaml", GLOBAL),
    kw("true", GLOBAL),
    kw("upload", ROUTE),
    kw("uri_for", ROUTE),
    kw("uri_for_route", ROUTE),
    kw("var", ROUTE),
    kw("vars", ROUTE),
    kw("warning", GLOBAL),
];

fn supports_uri_for_route(framework_version: &str) -> bool {
    matches!(
        crate::framework::version_constraint_matches(
            DANCER2_URI_FOR_ROUTE_VERSION_CONSTRAINT,
            framework_version,
        ),
        Some(true)
    )
}

fn keyword_is_available_in_version(keyword: &Dancer2DslKeyword, framework_version: &str) -> bool {
    keyword.name != "uri_for_route" || supports_uri_for_route(framework_version)
}

fn dsl_contract_version_for(framework_version: &str) -> &'static str {
    if supports_uri_for_route(framework_version) {
        DANCER2_DSL_CONTRACT_VERSION
    } else {
        DANCER2_DSL_1_0_CONTRACT_VERSION
    }
}

/// Build the Dancer2 adapter descriptor.
///
/// Shadow disposition: this adapter's facts are comparison-only and cannot
/// become publication authority (the SDK's authority validator refuses
/// non-production output by design).
#[must_use]
pub fn dancer2_descriptor() -> AdapterDescriptor {
    AdapterDescriptor::new(
        DANCER2_ADAPTER_ID,
        "dancer2",
        DANCER2_FRAMEWORK_NAME,
        Some(DANCER2_VERSION_CONSTRAINT.to_string()),
        DANCER2_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    )
}

/// Run the registry-backed Dancer2 detection over one checked input.
///
/// Only the descriptor-owned `Dancer2` selector participates; a resolved
/// `Dancer2::Core` module never activates this adapter.
#[must_use]
pub fn detect_dancer2(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    let descriptor = &input.descriptor;
    let Some(evaluation) = input.module_observation.evaluations.iter().find(
        |evaluation: &&ModuleSelectorEvaluation| {
            descriptor
                .required_module_selectors
                .iter()
                .any(|selector| selector == &evaluation.selector)
        },
    ) else {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
        );
    };
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
                // A module named Dancer2 without resolved supported identity is
                // not exact activation (#8914).
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "Dancer2 selector matched with {identity_confidence:?} identity \
                             evidence; exact activation requires resolved module identity"
                        ),
                    },
                );
            }
            match &activation.observed_version {
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: "Dancer2 activation lacks observed version evidence; the \
                                 reviewed version constraint cannot be checked"
                            .to_string(),
                    },
                ),
                Some(version) => {
                    match crate::framework::version_constraint_matches(
                        DANCER2_VERSION_CONSTRAINT,
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
                                    "observed Dancer2 version `{}` is not comparable with the \
                                     reviewed constraint `{DANCER2_VERSION_CONSTRAINT}`",
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

/// Application-name selection from the activating import.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppNameSelection {
    /// Application identity derives from the caller package.
    Default,
    /// Unambiguous literal `appname => 'Name'`.
    Literal(String),
    /// Computed or unsupported app identity — an explicit dynamic boundary.
    Dynamic { reason: String },
}

/// DSL selection from the activating import.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslSelection {
    /// The reviewed default Dancer2 DSL contract.
    Default,
    /// Literal `dsl => 'Some::DSL'` with exact source/module evidence.
    CustomLiteral(String),
    /// Computed/configured DSL selection — an explicit dynamic boundary.
    Dynamic { reason: String },
}

/// Import evidence extracted from the activating `use Dancer2 ...;` argument
/// list, in parser token form.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dancer2ImportEvidence {
    /// Application-name selection.
    pub appname: Option<AppNameSelection>,
    /// DSL selection.
    pub dsl: Option<DslSelection>,
    /// `!keyword` exclusions in source order.
    pub excluded_keywords: Vec<String>,
    /// Import options this adapter does not model; the activation carries an
    /// explicit boundary for them.
    pub unmodeled_options: Vec<String>,
}

/// Parse `use Dancer2` import arguments (parser token strings) into evidence.
///
/// Recognized literal forms: `appname => 'Name'`, `dsl => 'Some::DSL'`,
/// `!keyword` (bare, quoted, or `qw(!keyword ...)` pieces). Computed values
/// become explicit dynamic selections, not defaults.
#[must_use]
pub fn parse_dancer2_import_args(args: &[String]) -> Dancer2ImportEvidence {
    let mut evidence = Dancer2ImportEvidence::default();
    let tokens = normalize_import_tokens(args);
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "!" && index + 1 < tokens.len() {
            let keyword = tokens[index + 1].clone();
            push_exclusion(&mut evidence, keyword);
            index += 2;
            continue;
        }
        if let Some(keyword) = token.strip_prefix('!')
            && !keyword.is_empty()
        {
            push_exclusion(&mut evidence, keyword.to_string());
            index += 1;
            continue;
        }
        if (token == "appname" || token == "dsl") && index + 1 < tokens.len() {
            let value = &tokens[index + 1];
            let selection = literal_option_value(value);
            match token.as_str() {
                "appname" => evidence.appname = Some(selection.map_name()),
                _ => evidence.dsl = Some(selection.map_dsl()),
            }
            index += 2;
            continue;
        }
        if !is_structural_token(token) {
            evidence.unmodeled_options.push(token.clone());
        }
        index += 1;
    }
    evidence
}

fn push_exclusion(evidence: &mut Dancer2ImportEvidence, keyword: String) {
    if !keyword.is_empty() && !evidence.excluded_keywords.contains(&keyword) {
        evidence.excluded_keywords.push(keyword);
    }
}

enum OptionValue {
    Literal(String),
    Dynamic,
}

impl OptionValue {
    fn map_name(self) -> AppNameSelection {
        match self {
            OptionValue::Literal(value) => AppNameSelection::Literal(value),
            OptionValue::Dynamic => AppNameSelection::Dynamic {
                reason: "appname value is computed at runtime".to_string(),
            },
        }
    }

    fn map_dsl(self) -> DslSelection {
        match self {
            OptionValue::Literal(value) => DslSelection::CustomLiteral(value),
            OptionValue::Dynamic => {
                DslSelection::Dynamic { reason: "dsl value is computed at runtime".to_string() }
            }
        }
    }
}

fn literal_option_value(token: &str) -> OptionValue {
    let unquoted = token
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| token.strip_prefix('"').and_then(|value| value.strip_suffix('"')));
    if let Some(value) = unquoted {
        return OptionValue::Literal(value.to_string());
    }
    let dynamic = token.starts_with('$')
        || token.starts_with('@')
        || token.starts_with('%')
        || token.starts_with('\\')
        || token.contains('(');
    if dynamic {
        return OptionValue::Dynamic;
    }
    // Bareword values are literal in the reviewed import forms.
    OptionValue::Literal(token.to_string())
}

fn is_structural_token(token: &str) -> bool {
    matches!(token, "," | "(" | ")" | ";") || token == "=>"
}

fn normalize_import_tokens(args: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for arg in args {
        let token = arg.trim();
        if token.is_empty() || token == "," || token == "=>" || token == "(" || token == ")" {
            continue;
        }
        // `qw(a b c)` arrives as one parser token; expand it in place.
        if let Some(stripped) = token.strip_prefix("qw").map(str::trim) {
            let inner = stripped
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
                .or_else(|| stripped.strip_prefix('{').and_then(|value| value.strip_suffix('}')));
            if let Some(words) = inner {
                tokens.extend(
                    words
                        .split_whitespace()
                        .filter(|word| !word.is_empty())
                        .map(ToString::to_string),
                );
                continue;
            }
            if stripped.is_empty() {
                // Bare `qw` marker: following tokens are the word list.
                continue;
            }
        }
        // Unwrap one level of quoting around `!keyword` pieces.
        let unquoted = token
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
            .or_else(|| token.strip_prefix('"').and_then(|value| value.strip_suffix('"')));
        if let Some(inner) = unquoted {
            tokens.push(inner.to_string());
        } else {
            tokens.push(token.to_string());
        }
    }
    tokens
}

/// Availability state of one keyword in the activating import.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dancer2KeywordState {
    /// Keyword is imported by this activation.
    Imported,
    /// Keyword is excluded by `!keyword`.
    Excluded,
}

/// One typed Dancer2 DSL keyword import fact.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2KeywordImportFact {
    /// DSL keyword name.
    pub keyword: String,
    /// Reviewed availability scope.
    pub scope: DslKeywordScope,
    /// Imported/excluded state for this activation.
    pub state: Dancer2KeywordState,
    /// Deprecated keywords are preserved as versioned metadata.
    pub deprecated: bool,
}

/// Final activation state of one Dancer2 activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dancer2ActivationState {
    /// Exact registry-backed activation.
    Exact {
        /// Application identity derived from caller package or literal appname.
        application_name: String,
        /// Observed supported framework version.
        framework_version: String,
        /// Source generation that produced the activation evidence.
        source_generation: SourceGeneration,
    },
    /// The site is an explicit dynamic boundary, not exact activation.
    DynamicBoundary {
        /// Bounded boundary explanation.
        reason: String,
    },
    /// The site did not activate under the registry contract.
    NotActivated {
        /// Bounded non-activation explanation.
        reason: String,
    },
}

/// Typed registry-backed Dancer2 activation facts for one activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2ActivationFacts {
    /// Activation state.
    pub state: Dancer2ActivationState,
    /// Effective DSL selection.
    pub dsl: DslSelection,
    /// Versioned identity of the keyword contract that produced `keywords`.
    pub dsl_contract_version: &'static str,
    /// Keyword import facts. Empty when a custom/dynamic DSL owns the keyword
    /// vocabulary (the default contract must not be inherited).
    pub keywords: Vec<Dancer2KeywordImportFact>,
    /// `!keyword` exclusions that do not name a reviewed DSL keyword.
    pub unknown_exclusions: Vec<String>,
}

impl Dancer2ActivationFacts {
    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self.state, Dancer2ActivationState::Exact { .. })
    }
}

/// Derive application identity for an admitted activation.
#[must_use]
pub fn dancer2_application_identity(
    package_name: Option<&str>,
    appname: Option<&AppNameSelection>,
) -> AppNameSelection {
    match appname {
        Some(selection @ AppNameSelection::Literal(_)) => selection.clone(),
        Some(selection @ AppNameSelection::Dynamic { .. }) => selection.clone(),
        Some(AppNameSelection::Default) | None => match package_name {
            Some(package) if !package.trim().is_empty() => AppNameSelection::Default,
            _ => AppNameSelection::Dynamic {
                reason: "activating package name is unavailable".to_string(),
            },
        },
    }
}

/// Build typed activation facts from one detection result plus import evidence.
///
/// `keywords` are emitted from the reviewed default contract only when the DSL
/// selection is the default; a custom or dynamic DSL cannot inherit
/// default-Dancer2 exact keyword facts.
#[must_use]
pub fn dancer2_activation_facts(
    detection: &AdapterDetectionResult,
    package_name: Option<&str>,
    evidence: &Dancer2ImportEvidence,
) -> Dancer2ActivationFacts {
    let dsl = evidence.dsl.clone().unwrap_or(DslSelection::Default);
    let (framework_version, source_generation) = match &detection.outcome {
        DetectionOutcome::Detected { framework_version, .. } => {
            (framework_version.clone().unwrap_or_default(), detection.project_generation.clone())
        }
        outcome => {
            return Dancer2ActivationFacts {
                state: Dancer2ActivationState::NotActivated {
                    reason: format!("detection outcome is {outcome:?}"),
                },
                dsl,
                dsl_contract_version: DANCER2_DSL_CONTRACT_VERSION,
                keywords: Vec::new(),
                unknown_exclusions: unknown_exclusions(evidence, None),
            };
        }
    };

    let dsl_contract_version = dsl_contract_version_for(&framework_version);
    let appname = dancer2_application_identity(package_name, evidence.appname.as_ref());
    let boundary_reason = dynamic_boundary_reason(&appname, &dsl, evidence);
    if let Some(reason) = boundary_reason {
        return Dancer2ActivationFacts {
            state: Dancer2ActivationState::DynamicBoundary { reason },
            dsl,
            dsl_contract_version,
            keywords: Vec::new(),
            unknown_exclusions: unknown_exclusions(evidence, Some(&framework_version)),
        };
    }

    let application_name = match &appname {
        AppNameSelection::Literal(name) => name.clone(),
        _ => package_name.map(ToOwned::to_owned).unwrap_or_default(),
    };
    let keywords = default_keyword_facts(evidence, &framework_version);
    let unknown_exclusions = unknown_exclusions(evidence, Some(&framework_version));

    Dancer2ActivationFacts {
        state: Dancer2ActivationState::Exact {
            application_name,
            framework_version,
            source_generation,
        },
        dsl,
        dsl_contract_version,
        keywords,
        unknown_exclusions,
    }
}

fn dynamic_boundary_reason(
    appname: &AppNameSelection,
    dsl: &DslSelection,
    evidence: &Dancer2ImportEvidence,
) -> Option<String> {
    if let AppNameSelection::Dynamic { reason } = appname {
        return Some(format!("application identity is a dynamic boundary: {reason}"));
    }
    match dsl {
        DslSelection::Dynamic { reason } => {
            Some(format!("DSL selection is a dynamic boundary: {reason}"))
        }
        DslSelection::CustomLiteral(name) => Some(format!(
            "custom DSL `{name}` owns its keyword vocabulary; the default Dancer2 contract \
             is not inherited"
        )),
        DslSelection::Default => {
            if evidence.unmodeled_options.is_empty() {
                None
            } else {
                Some(format!(
                    "import carries unmodeled options: {}",
                    evidence.unmodeled_options.join(", ")
                ))
            }
        }
    }
}

fn default_keyword_facts(
    evidence: &Dancer2ImportEvidence,
    framework_version: &str,
) -> Vec<Dancer2KeywordImportFact> {
    DANCER2_DSL_KEYWORDS
        .iter()
        .filter(|keyword| keyword_is_available_in_version(keyword, framework_version))
        .map(|keyword| Dancer2KeywordImportFact {
            keyword: keyword.name.to_string(),
            scope: keyword.scope,
            state: if evidence.excluded_keywords.iter().any(|excluded| excluded == keyword.name) {
                Dancer2KeywordState::Excluded
            } else {
                Dancer2KeywordState::Imported
            },
            deprecated: keyword.deprecated,
        })
        .collect()
}

fn keyword_is_reviewed_for_version(
    keyword: &Dancer2DslKeyword,
    framework_version: Option<&str>,
) -> bool {
    match framework_version {
        Some(version) => keyword_is_available_in_version(keyword, version),
        None => true,
    }
}

fn unknown_exclusions(
    evidence: &Dancer2ImportEvidence,
    framework_version: Option<&str>,
) -> Vec<String> {
    evidence
        .excluded_keywords
        .iter()
        .filter(|excluded| {
            !DANCER2_DSL_KEYWORDS.iter().any(|keyword| {
                keyword.name == excluded.as_str()
                    && keyword_is_reviewed_for_version(keyword, framework_version)
            })
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_dancer2_selective_and_shadow() {
        let descriptor = dancer2_descriptor();
        assert_eq!(descriptor.required_module_selectors, vec!["Dancer2"]);
        assert_eq!(descriptor.disposition, AdapterDisposition::Shadow);
        assert_eq!(
            descriptor.framework_version_constraint.as_deref(),
            Some(DANCER2_VERSION_CONSTRAINT)
        );
    }

    #[test]
    fn core_module_is_not_a_selector() {
        assert!(
            !dancer2_descriptor()
                .required_module_selectors
                .iter()
                .any(|selector| selector.starts_with("Dancer2::"))
        );
    }

    #[test]
    fn keyword_table_matches_reviewed_v1_1_registration() {
        let expected: &[(&str, DslKeywordScope, bool)] = &[
            ("any", GLOBAL, false),
            ("app", GLOBAL, false),
            ("captures", ROUTE, false),
            ("config", GLOBAL, false),
            ("content", ROUTE, false),
            ("content_type", ROUTE, false),
            ("context", ROUTE, true),
            ("cookie", ROUTE, false),
            ("cookies", ROUTE, false),
            ("dance", GLOBAL, false),
            ("dancer_app", GLOBAL, false),
            ("dancer_version", GLOBAL, false),
            ("dancer_major_version", GLOBAL, false),
            ("debug", GLOBAL, false),
            ("decode_json", GLOBAL, false),
            ("del", GLOBAL, false),
            ("delayed", ROUTE, false),
            ("dirname", GLOBAL, false),
            ("done", ROUTE, false),
            ("dsl", GLOBAL, false),
            ("encode_json", GLOBAL, false),
            ("engine", GLOBAL, false),
            ("error", GLOBAL, false),
            ("false", GLOBAL, false),
            ("flush", ROUTE, false),
            ("forward", ROUTE, false),
            ("from_dumper", GLOBAL, false),
            ("from_json", GLOBAL, false),
            ("from_yaml", GLOBAL, false),
            ("get", GLOBAL, false),
            ("halt", ROUTE, false),
            ("header", ROUTE, true),
            ("headers", ROUTE, true),
            ("hook", GLOBAL, false),
            ("info", GLOBAL, false),
            ("log", GLOBAL, false),
            ("mime", GLOBAL, false),
            ("options", GLOBAL, false),
            ("param", ROUTE, false),
            ("params", ROUTE, false),
            ("query_parameters", ROUTE, false),
            ("body_parameters", ROUTE, false),
            ("route_parameters", ROUTE, false),
            ("pass", ROUTE, false),
            ("patch", GLOBAL, false),
            ("path", GLOBAL, false),
            ("post", GLOBAL, false),
            ("prefix", GLOBAL, false),
            ("prepare_app", GLOBAL, false),
            ("psgi_app", GLOBAL, false),
            ("push_header", ROUTE, true),
            ("push_response_header", ROUTE, false),
            ("put", GLOBAL, false),
            ("redirect", ROUTE, false),
            ("request", ROUTE, false),
            ("request_data", ROUTE, false),
            ("request_header", ROUTE, false),
            ("response", ROUTE, false),
            ("response_header", ROUTE, false),
            ("response_headers", ROUTE, false),
            ("runner", GLOBAL, false),
            ("send_as", ROUTE, false),
            ("send_error", ROUTE, false),
            ("send_file", ROUTE, false),
            ("session", ROUTE, false),
            ("set", GLOBAL, false),
            ("setting", GLOBAL, false),
            ("splat", ROUTE, false),
            ("start", GLOBAL, false),
            ("status", ROUTE, false),
            ("template", GLOBAL, false),
            ("to_app", GLOBAL, false),
            ("to_dumper", GLOBAL, false),
            ("to_json", GLOBAL, false),
            ("to_yaml", GLOBAL, false),
            ("true", GLOBAL, false),
            ("upload", ROUTE, false),
            ("uri_for", ROUTE, false),
            ("uri_for_route", ROUTE, false),
            ("var", ROUTE, false),
            ("vars", ROUTE, false),
            ("warning", GLOBAL, false),
        ];
        let observed: Vec<_> = DANCER2_DSL_KEYWORDS
            .iter()
            .map(|keyword| (keyword.name, keyword.scope, keyword.deprecated))
            .collect();
        assert_eq!(observed.as_slice(), expected);
    }

    #[test]
    fn reviewed_keyword_exclusion_is_known_and_preserved() {
        let evidence = parse_dancer2_import_args(&["!uri_for_route".to_string()]);
        let facts = default_keyword_facts(&evidence, "1.1.1");
        let uri_for_route = facts
            .iter()
            .find(|fact| fact.keyword == "uri_for_route")
            .expect("reviewed keyword fact");
        assert_eq!(uri_for_route.state, Dancer2KeywordState::Excluded);
        assert!(unknown_exclusions(&evidence, Some("1.1.1")).is_empty());
        assert_eq!(dsl_contract_version_for("1.1.1"), DANCER2_DSL_CONTRACT_VERSION);
    }

    #[test]
    fn uri_for_route_is_not_imported_before_v1_1() {
        let evidence = parse_dancer2_import_args(&["!uri_for_route".to_string()]);
        let facts = default_keyword_facts(&evidence, "1.0.0");
        assert_eq!(facts.len(), DANCER2_DSL_KEYWORDS.len() - 1);
        assert!(
            facts.iter().all(|fact| fact.keyword != "uri_for_route"),
            "Dancer2 1.0.x did not register `uri_for_route`"
        );
        assert_eq!(unknown_exclusions(&evidence, Some("1.0.0")), vec!["uri_for_route".to_string()]);
        assert_eq!(dsl_contract_version_for("1.0.0"), DANCER2_DSL_1_0_CONTRACT_VERSION);
    }

    #[test]
    fn non_keyword_hook_name_remains_an_unknown_exclusion() {
        let evidence = parse_dancer2_import_args(&["!before".to_string()]);
        assert_eq!(unknown_exclusions(&evidence, Some("1.1.1")), vec!["before".to_string()]);
    }

    #[test]
    fn parses_literal_appname_dsl_and_exclusions() {
        let args: Vec<String> = ["appname", "=>", "'MyApp'", ",", "'!get'", "!", "post"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let evidence = parse_dancer2_import_args(&args);
        assert_eq!(evidence.appname, Some(AppNameSelection::Literal("MyApp".to_string())));
        assert_eq!(evidence.excluded_keywords, vec!["get", "post"]);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn computed_appname_is_a_dynamic_boundary_not_a_default() {
        let args: Vec<String> =
            ["appname", "=>", "$name"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        assert!(
            matches!(evidence.appname, Some(AppNameSelection::Dynamic { .. })),
            "computed appname must not silently become Default"
        );
    }

    #[test]
    fn parses_custom_dsl_selection() {
        let args: Vec<String> =
            ["dsl", "=>", "'My::DSL'"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        assert_eq!(evidence.dsl, Some(DslSelection::CustomLiteral("My::DSL".to_string())));
    }

    #[test]
    fn qw_exclusion_list_is_expanded() {
        // The parser stores `qw(!get !post)` as one argument token.
        let args: Vec<String> = vec!["qw(!get !post)".to_string()];
        let evidence = parse_dancer2_import_args(&args);
        assert_eq!(evidence.excluded_keywords, vec!["get", "post"]);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn unknown_import_options_are_recorded_not_dropped() {
        let args: Vec<String> =
            ["appname", "=>", "'A'", "skip_check"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_import_args(&args);
        assert_eq!(evidence.unmodeled_options, vec!["skip_check".to_string()]);
    }
}
