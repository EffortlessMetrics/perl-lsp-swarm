//! Structural validation for the critic rule-proof manifest.

use perl_lsp_rs_core::tooling::perl_critic::{
    CriticFindingOrigin, CriticFindingShape, CriticIdentityRegistry, NativeCriticProfile,
    NativeCriticRegistry,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::digest::file_digest;
use super::error::ProofError;
use super::mapping::{native_profile, origin_name, shape_name};
use super::model::{
    EvidenceClass, FIXTURE_ROOT, FixApply, ISSUE, MANIFEST_NAME, MANIFEST_PATH, PILOT_RULES,
    ParseExpectation, ProofRemediation, RuleProofManifest, SCHEMA_PATH, SCHEMA_VERSION,
    resolve_fixture_path,
};

/// Load, schema-validate, and structurally check the committed manifest.
pub fn load_and_validate(root: &Path) -> Result<RuleProofManifest, ProofError> {
    let bytes = fs::read(root.join(MANIFEST_PATH))
        .map_err(|error| ProofError::new(format!("{MANIFEST_PATH}: cannot read: {error}")))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| ProofError::new(format!("{MANIFEST_PATH}: invalid JSON: {error}")))?;
    decode_and_validate(root, &value)
}

/// Validate an in-memory JSON document against schema plus structural rules.
pub fn validate_manifest_value(
    root: &Path,
    value: &Value,
) -> Result<RuleProofManifest, ProofError> {
    decode_and_validate(root, value)
}

fn decode_and_validate(root: &Path, value: &Value) -> Result<RuleProofManifest, ProofError> {
    let schema_text = read_text(root, SCHEMA_PATH)?;
    let schema: Value = serde_json::from_str(&schema_text)
        .map_err(|error| ProofError::new(format!("{SCHEMA_PATH}: invalid JSON: {error}")))?;
    validate_schema(&schema, value)?;
    let manifest: RuleProofManifest = serde_json::from_value(value.clone()).map_err(|error| {
        ProofError::new(format!("{MANIFEST_PATH}: typed decode failed: {error}"))
    })?;
    validate_manifest(root, &manifest)?;
    Ok(manifest)
}

fn validate_schema(schema: &Value, value: &Value) -> Result<(), ProofError> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| ProofError::new(format!("{SCHEMA_PATH}: invalid schema: {error}")))?;
    let violations: Vec<String> =
        validator.iter_errors(value).map(|error| format!("schema: {error}")).collect();
    ProofError::from_violations(violations)
}

pub(crate) fn validate_manifest(
    root: &Path,
    manifest: &RuleProofManifest,
) -> Result<(), ProofError> {
    let mut violations = Vec::new();
    if manifest.schema_version != SCHEMA_VERSION {
        violations.push(format!(
            "schema_version: expected `{SCHEMA_VERSION}`, found `{}`",
            manifest.schema_version
        ));
    }
    if manifest.manifest != MANIFEST_NAME {
        violations
            .push(format!("manifest: expected `{MANIFEST_NAME}`, found `{}`", manifest.manifest));
    }
    if manifest.issue != ISSUE {
        violations.push(format!("issue: expected {ISSUE}, found {}", manifest.issue));
    }
    if manifest.status != "pilot" {
        violations.push(format!("status: expected `pilot`, found `{}`", manifest.status));
    }

    let declared_classes: BTreeSet<_> = manifest.evidence_classes.iter().copied().collect();
    let required_classes: BTreeSet<_> = EvidenceClass::all().iter().copied().collect();
    if declared_classes != required_classes {
        violations.push(
            "evidence_classes: must list the closed nine-class vocabulary exactly once".to_string(),
        );
    }

    validate_fixtures(root, manifest, &mut violations);
    validate_rules(manifest, &mut violations);
    validate_cases(manifest, &mut violations);
    ProofError::from_violations(violations)
}

fn validate_fixtures(root: &Path, manifest: &RuleProofManifest, violations: &mut Vec<String>) {
    let mut used = BTreeSet::new();
    for case in &manifest.cases {
        used.insert(case.fixture.as_str());
        if !manifest.fixtures.contains_key(&case.fixture) {
            violations.push(format!(
                "case `{}`: fixture `{}` is not listed in fixtures",
                case.case_id, case.fixture
            ));
        }
    }
    for (relative, record) in &manifest.fixtures {
        if !used.contains(relative.as_str()) {
            violations.push(format!("fixtures.`{relative}`: unused fixture identity"));
        }
        if !record.digest.starts_with("sha256:") || record.digest.len() != 71 {
            violations.push(format!("fixtures.`{relative}`: digest must be sha256:<64 hex chars>"));
            continue;
        }
        if !root.join(FIXTURE_ROOT).join(relative).is_file() {
            violations.push(format!("fixtures.`{relative}`: file does not exist"));
            continue;
        }
        let path = match resolve_fixture_path(root, relative) {
            Ok(path) => path,
            Err(error) => {
                violations.push(format!("fixtures.`{relative}`: {error}"));
                continue;
            }
        };
        match file_digest(&path) {
            Ok(actual) if actual != record.digest => {
                violations.push(format!(
                    "fixtures.`{relative}`: digest is stale (manifest {}, file {actual})",
                    record.digest
                ));
            }
            Ok(_) => {}
            Err(error) => violations.push(error.to_string()),
        }
    }
}

fn validate_rules(manifest: &RuleProofManifest, violations: &mut Vec<String>) {
    let catalog: BTreeSet<&str> = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict)
        .rule_ids()
        .into_iter()
        .collect();
    let mut seen_rules = BTreeSet::new();
    let mut seen_canonical = BTreeSet::new();
    for rule in &manifest.rules {
        if !seen_rules.insert(rule.rule_id.as_str()) {
            violations.push(format!("rules: duplicate rule_id `{}`", rule.rule_id));
        }
        if !seen_canonical.insert(rule.canonical_id.as_str()) {
            violations.push(format!("rules: duplicate canonical_id `{}`", rule.canonical_id));
        }
        if !PILOT_RULES.contains(&rule.rule_id.as_str()) {
            violations
                .push(format!("rules: `{}` is outside the closed PILOT_RULES set", rule.rule_id));
            continue;
        }
        if !catalog.contains(rule.rule_id.as_str()) {
            violations.push(format!("rule `{}`: unknown native rule id", rule.rule_id));
            continue;
        }
        let Some(entry) = CriticIdentityRegistry::by_canonical_id(&rule.canonical_id) else {
            violations.push(format!(
                "rule `{}`: canonical_id `{}` is not in the identity registry",
                rule.rule_id, rule.canonical_id
            ));
            continue;
        };
        let Some(resolved) = CriticIdentityRegistry::resolve_parts(
            CriticFindingOrigin::NativeCritic,
            &rule.rule_id,
            CriticFindingShape::General,
        ) else {
            violations.push(format!(
                "rule `{}`: native id does not resolve through the identity registry",
                rule.rule_id
            ));
            continue;
        };
        if resolved.canonical_id() != rule.canonical_id {
            violations.push(format!(
                "rule `{}`: native id resolves to `{}`, not `{}`",
                rule.rule_id,
                resolved.canonical_id(),
                rule.canonical_id
            ));
        }
        let expected = expected_alias_rows(entry.aliases());
        let actual: Vec<(String, String, String)> = rule
            .identity_aliases
            .iter()
            .map(|alias| (alias.origin.clone(), alias.code.clone(), alias.shape.clone()))
            .collect();
        if actual != expected {
            violations.push(format!(
                "rule `{}`: identity_aliases do not match the identity registry",
                rule.rule_id
            ));
        }
    }
    for required in PILOT_RULES {
        if !seen_rules.contains(required) {
            violations.push(format!("rules: missing pilot rule `{required}`"));
        }
    }
}

fn expected_alias_rows(
    aliases: &[perl_lsp_rs_core::tooling::perl_critic::CriticAlias],
) -> Vec<(String, String, String)> {
    aliases
        .iter()
        .map(|alias| {
            (
                origin_name(alias.origin()).to_string(),
                alias.code().to_string(),
                shape_name(alias.shape()).to_string(),
            )
        })
        .collect()
}

fn origin_name(origin: CriticFindingOrigin) -> &'static str {
    match origin {
        CriticFindingOrigin::BuiltInDiagnostic => "built_in_diagnostic",
        CriticFindingOrigin::NativeCritic => "native_critic",
        CriticFindingOrigin::LegacyPolicy => "legacy_policy",
        CriticFindingOrigin::ExternalPerlCritic => "external_perl_critic",
    }
}

fn shape_name(shape: CriticFindingShape) -> &'static str {
    match shape {
        CriticFindingShape::General => "general",
        CriticFindingShape::LiteralUndefComparison => "literal_undef_comparison",
        CriticFindingShape::PotentiallyUndefComparison => "potentially_undef_comparison",
        CriticFindingShape::Backtick => "backtick",
        CriticFindingShape::Qx => "qx",
        CriticFindingShape::Readpipe => "readpipe",
        CriticFindingShape::SystemCall => "system_call",
        CriticFindingShape::ExecCall => "exec_call",
    }
}

fn validate_cases(manifest: &RuleProofManifest, violations: &mut Vec<String>) {
    let mut seen_cases = BTreeSet::new();
    let mut classes_by_rule: BTreeMap<&str, BTreeSet<EvidenceClass>> = BTreeMap::new();
    for case in &manifest.cases {
        if !seen_cases.insert(case.case_id.as_str()) {
            violations.push(format!("cases: duplicate case_id `{}`", case.case_id));
        }
        let Some(rule) = manifest.rule(&case.rule_id) else {
            violations.push(format!(
                "case `{}`: rule_id `{}` is not a governed rule row",
                case.case_id, case.rule_id
            ));
            continue;
        };
        if case.profile != rule.profile {
            violations.push(format!(
                "case `{}`: profile `{}` does not match the governed rule profile `{}`",
                case.case_id,
                case.profile.as_str(),
                rule.profile.as_str()
            ));
        }
        let classes = classes_by_rule.entry(case.rule_id.as_str()).or_default();
        for class in &case.evidence_classes {
            classes.insert(*class);
        }
        if case.include.is_empty() {
            violations
                .push(format!("case `{}`: include must name the governed rule", case.case_id));
        }
        if !case.include.iter().any(|id| id == &case.rule_id) {
            violations.push(format!(
                "case `{}`: include must contain the governed rule `{}`",
                case.case_id, case.rule_id
            ));
        }
        let profile_catalog: BTreeSet<&str> =
            NativeCriticRegistry::for_profile(native_profile(case.profile))
                .rule_ids()
                .into_iter()
                .collect();
        for included in &case.include {
            if !profile_catalog.contains(included.as_str()) {
                violations.push(format!(
                    "case `{}`: include rule `{included}` is not in the `{}` profile roster",
                    case.case_id,
                    case.profile.as_str()
                ));
            }
        }
        if matches!(case.parse_expectation, ParseExpectation::Error) {
            for class in &case.evidence_classes {
                if !class.allowed_on_parse_error() {
                    violations.push(format!(
                        "case `{}`: parse-error fixtures may only declare boundary evidence, not `{}`",
                        case.case_id,
                        class.as_str()
                    ));
                }
            }
            if !case.expected_findings.is_empty() {
                violations.push(format!(
                    "case `{}`: malformed parse boundaries cannot claim expected findings",
                    case.case_id
                ));
            }
            if case.suppression_selector.is_some() || case.fix_round_trip.is_some() {
                violations.push(format!(
                    "case `{}`: parse-error fixtures cannot claim suppression or automatic round-trip",
                    case.case_id
                ));
            }
        }
        for class in &case.evidence_classes {
            if class.requires_governed_expected_finding()
                && !case.expected_findings.iter().any(|finding| finding.rule_id == case.rule_id)
            {
                violations.push(format!(
                    "case `{}`: evidence class `{}` requires an expected finding for governed rule `{}`",
                    case.case_id,
                    class.as_str(),
                    case.rule_id
                ));
            }
            if class.requires_governed_non_finding()
                && !case.expected_non_findings.iter().any(|rule_id| rule_id == &case.rule_id)
            {
                violations.push(format!(
                    "case `{}`: evidence class `{}` requires expected_non_findings to name governed rule `{}`",
                    case.case_id,
                    class.as_str(),
                    case.rule_id
                ));
            }
        }
        if case.evidence_classes.contains(&EvidenceClass::Boundary)
            && matches!(case.parse_expectation, ParseExpectation::Ok)
        {
            let has_finding =
                case.expected_findings.iter().any(|finding| finding.rule_id == case.rule_id);
            let has_non_finding =
                case.expected_non_findings.iter().any(|rule_id| rule_id == &case.rule_id);
            if !has_finding && !has_non_finding {
                violations.push(format!(
                    "case `{}`: ordinary boundary evidence must name a governed finding or non-finding",
                    case.case_id
                ));
            }
        }
        if case.evidence_classes.contains(&EvidenceClass::FileLevelSuppression) {
            match case.suppression_selector.as_deref() {
                None => violations.push(format!(
                    "case `{}`: file-level suppression requires suppression_selector",
                    case.case_id
                )),
                Some(selector) if selector != case.rule_id && selector != rule.canonical_id => {
                    violations.push(format!(
                        "case `{}`: suppression_selector `{selector}` is not the governed rule or canonical identity",
                        case.case_id
                    ));
                }
                Some(_) => {}
            }
        }
        if case.evidence_classes.contains(&EvidenceClass::AutomaticFixRoundTrip) {
            match &case.fix_round_trip {
                None => violations.push(format!(
                    "case `{}`: automatic_fix_round_trip requires fix_round_trip",
                    case.case_id
                )),
                Some(round_trip) => {
                    if !rule.declared_remediation.automatic_round_trip_applicable() {
                        violations.push(format!(
                            "case `{}`: automatic_fix_round_trip is impossible for declared_remediation `{}`",
                            case.case_id,
                            rule.declared_remediation.as_str()
                        ));
                    }
                    if round_trip.apply != FixApply::Automatic {
                        violations.push(format!(
                            "case `{}`: automatic_fix_round_trip must apply automatic edits",
                            case.case_id
                        ));
                    }
                    if round_trip.expect_reparse != ParseExpectation::Ok
                        || !round_trip.expect_target_removed
                        || !round_trip.expect_no_new_governed
                    {
                        violations.push(format!(
                            "case `{}`: automatic success must require reparse ok, target removal, and no new governed diagnostic",
                            case.case_id
                        ));
                    }
                    if round_trip.expected_edits.is_empty() {
                        violations.push(format!(
                            "case `{}`: automatic success must record at least one expected edit",
                            case.case_id
                        ));
                    }
                }
            }
        } else if case.fix_round_trip.is_some() {
            violations.push(format!(
                "case `{}`: fix_round_trip is only valid with automatic_fix_round_trip",
                case.case_id
            ));
        }
        for finding in &case.expected_findings {
            if finding.rule_id != case.rule_id {
                continue;
            }
            if finding.remediation_eligibility == rule.declared_remediation {
                continue;
            }
            if matches!(
                rule.declared_remediation,
                ProofRemediation::None
                    | ProofRemediation::Manual
                    | ProofRemediation::PreviewCandidate
            ) && finding.remediation_eligibility == ProofRemediation::AutomaticCandidate
            {
                violations.push(format!(
                    "case `{}`: diagnostic-only or preview-only findings cannot be represented as automatic success",
                    case.case_id
                ));
            } else {
                violations.push(format!(
                    "case `{}`: expected finding remediation `{}` does not match declared_remediation `{}`",
                    case.case_id,
                    finding.remediation_eligibility.as_str(),
                    rule.declared_remediation.as_str()
                ));
            }
        }
    }
    for rule in &manifest.rules {
        let present = classes_by_rule.get(rule.rule_id.as_str()).cloned().unwrap_or_default();
        for class in EvidenceClass::all() {
            if !class.required_for_every_pilot_rule() {
                continue;
            }
            if !present.contains(class) {
                violations.push(format!(
                    "rule `{}`: missing required evidence class `{}`",
                    rule.rule_id,
                    class.as_str()
                ));
            }
        }
        if rule.declared_remediation.automatic_round_trip_applicable() {
            if !present.contains(&EvidenceClass::AutomaticFixRoundTrip) {
                violations.push(format!(
                    "rule `{}`: missing required evidence class `automatic_fix_round_trip`",
                    rule.rule_id
                ));
            }
        } else if present.contains(&EvidenceClass::AutomaticFixRoundTrip) {
            violations.push(format!(
                "rule `{}`: automatic_fix_round_trip is not applicable for `{}`",
                rule.rule_id,
                rule.declared_remediation.as_str()
            ));
        }
    }
}

fn read_text(root: &Path, rel: &str) -> Result<String, ProofError> {
    fs::read_to_string(root.join(rel))
        .map_err(|error| ProofError::new(format!("{rel}: cannot read: {error}")))
}
