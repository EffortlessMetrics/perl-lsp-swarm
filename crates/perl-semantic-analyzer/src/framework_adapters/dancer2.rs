//! Registry-backed Dancer2 activation, application identity, and scoped DSL
//! import facts (#8914).
//!
//! This adapter is built directly on the checked framework SDK
//! ([`perl_semantic_facts::framework`]). It is a shadow adapter: its output is
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

use perl_semantic_facts::Confidence;
#[cfg(test)]
use perl_semantic_facts::SourceGeneration;
use perl_semantic_facts::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    UnavailableReason,
};
use perl_semantic_facts::framework_adapters::dancer2::*;

const DANCER2_DSL_1_0_CONTRACT_VERSION: &str = "dancer2-dsl.1-0.v3";
const DANCER2_URI_FOR_ROUTE_VERSION_CONSTRAINT: &str = ">=1.1.0,<2.0.0";

fn supports_uri_for_route(framework_version: &str) -> bool {
    matches!(
        perl_semantic_facts::framework::version_constraint_matches(
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
                    match perl_semantic_facts::framework::version_constraint_matches(
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

        _ => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: "unrecognized module observation outcome".to_string(),
            },
        ),
    }
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

        Some(_) => AppNameSelection::Dynamic {
            reason: "unrecognized application-name selection".to_string(),
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
            return Dancer2ActivationFacts::new(
                Dancer2ActivationState::NotActivated {
                    reason: format!("detection outcome is {outcome:?}"),
                },
                dsl,
                DANCER2_DSL_CONTRACT_VERSION,
                Vec::new(),
                unknown_exclusions(evidence, None),
            );
        }
    };

    let dsl_contract_version = dsl_contract_version_for(&framework_version);
    let appname = dancer2_application_identity(package_name, evidence.appname.as_ref());
    let boundary_reason = dynamic_boundary_reason(&appname, &dsl, evidence);
    if let Some(reason) = boundary_reason {
        return Dancer2ActivationFacts::new(
            Dancer2ActivationState::DynamicBoundary { reason },
            dsl,
            dsl_contract_version,
            Vec::new(),
            unknown_exclusions(evidence, Some(&framework_version)),
        );
    }

    let application_name = match &appname {
        AppNameSelection::Literal(name) => name.clone(),
        _ => package_name.map(ToOwned::to_owned).unwrap_or_default(),
    };
    let keywords = default_keyword_facts(evidence, &framework_version);
    let unknown_exclusions = unknown_exclusions(evidence, Some(&framework_version));

    Dancer2ActivationFacts::new(
        Dancer2ActivationState::Exact { application_name, framework_version, source_generation },
        dsl,
        dsl_contract_version,
        keywords,
        unknown_exclusions,
    )
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

        _ => Some("unrecognized DSL selection".to_string()),
    }
}

fn default_keyword_facts(
    evidence: &Dancer2ImportEvidence,
    framework_version: &str,
) -> Vec<Dancer2KeywordImportFact> {
    DANCER2_DSL_KEYWORDS
        .iter()
        .filter(|keyword| keyword_is_available_in_version(keyword, framework_version))
        .map(|keyword| {
            Dancer2KeywordImportFact::new(
                keyword.name.to_string(),
                keyword.scope,
                if evidence.excluded_keywords.iter().any(|excluded| excluded == keyword.name) {
                    Dancer2KeywordState::Excluded
                } else {
                    Dancer2KeywordState::Imported
                },
                keyword.deprecated,
            )
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
    use perl_test_must::must_some_with;

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
    fn reviewed_keyword_exclusion_is_known_and_preserved() {
        let evidence = parse_dancer2_import_args(&["!uri_for_route".to_string()]);
        let facts = default_keyword_facts(&evidence, "1.1.1");
        let uri_for_route = must_some_with(
            facts.iter().find(|fact| fact.keyword == "uri_for_route"),
            "reviewed keyword fact",
        );
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
    fn inactive_exclusions_use_the_reviewed_union_without_importing_keywords() -> Result<(), String>
    {
        let evidence =
            parse_dancer2_import_args(&["!uri_for_route".to_string(), "!before".to_string()]);
        let detection = AdapterDetectionResult::new(
            dancer2_descriptor(),
            SourceGeneration::known("inactive-exclusion-test"),
            DetectionOutcome::Unsupported { reason: "framework version was not observed".into() },
        );
        let facts = dancer2_activation_facts(&detection, Some("App"), &evidence);

        // Without an admitted version, exclusion metadata recognizes the reviewed
        // union. This does not assert that any union keyword was imported.
        if !matches!(facts.state, Dancer2ActivationState::NotActivated { .. })
            || facts.is_exact()
            || !facts.keywords.is_empty()
        {
            return Err(format!("inactive exclusion evidence published keyword facts: {facts:?}"));
        }
        if facts.unknown_exclusions != vec!["before".to_string()] {
            return Err(format!(
                "expected only the non-keyword hook name to remain unknown: {:?}",
                facts.unknown_exclusions
            ));
        }
        Ok(())
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
