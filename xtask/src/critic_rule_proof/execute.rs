//! Live native-critic execution against rule-proof fixtures.

use perl_lsp_rs_core::tooling::perl_critic::{
    CriticConfig, CriticContext, CriticFinding, CriticRemediationEligibility, CriticTextEdit,
    NativeCriticProfile, NativeCriticRegistry, Severity,
};
use perl_parser::Parser;
use std::fs;
use std::path::Path;

use super::error::ProofError;
use super::model::{
    CaseRecord, ExpectedFinding, FixRoundTrip, ParseExpectation, ProofProfile, ProofRemediation,
    ProofSeverity, RuleProofManifest, resolve_fixture_path,
};

/// Execute every case against the live native critic.
pub fn execute_manifest(root: &Path, manifest: &RuleProofManifest) -> Result<(), ProofError> {
    let mut violations = Vec::new();
    for case in &manifest.cases {
        if let Err(error) = execute_case(root, case) {
            violations.push(format!("case `{}`: {error}", case.case_id));
        }
    }
    ProofError::from_violations(violations)
}

fn execute_case(root: &Path, case: &CaseRecord) -> Result<(), ProofError> {
    let path = resolve_fixture_path(root, &case.fixture)
        .map_err(|error| ProofError::new(format!("case fixture: {error}")))?;
    let source = fs::read_to_string(&path).map_err(|error| {
        ProofError::new(format!("cannot read fixture `{}`: {error}", case.fixture))
    })?;
    let parsed = Parser::new(&source).parse();
    if matches!(case.parse_expectation, ParseExpectation::Error) {
        if parsed.is_ok() {
            return Err(ProofError::new("expected a parse error boundary, but the fixture parsed"));
        }
        if !case.expected_findings.is_empty() {
            return Err(ProofError::new("malformed parse boundaries cannot inherit findings"));
        }
        return Ok(());
    }
    let ast = parsed.map_err(|_| ProofError::new("fixture failed to parse"))?;
    let config = critic_config(case);
    let registry =
        NativeCriticRegistry::for_profile_with_config(native_profile(case.profile), &config);
    let ctx = CriticContext::new(&source, &ast, &config);
    let findings = registry.check(&ctx);
    match_findings(&source, case, &findings)?;
    if let Some(selector) = case.suppression_selector.as_deref() {
        prove_suppression_targets_governed_rule(&source, case, selector, &config, &registry)?;
    }
    if let Some(round_trip) = &case.fix_round_trip {
        prove_automatic_round_trip(&source, case, &findings, round_trip, &config, &registry)?;
    }
    Ok(())
}

fn match_findings(
    source: &str,
    case: &CaseRecord,
    findings: &[CriticFinding],
) -> Result<(), ProofError> {
    let mut unmatched: Vec<&CriticFinding> = findings.iter().collect();
    for expected in &case.expected_findings {
        let Some(index) =
            unmatched.iter().position(|finding| finding_matches(source, expected, finding))
        else {
            return Err(ProofError::new(format!(
                "missing expected finding for `{}` at bytes {}..{} ({})",
                expected.rule_id, expected.start_byte, expected.end_byte, expected.excerpt
            )));
        };
        unmatched.swap_remove(index);
    }
    if !unmatched.is_empty() {
        let extra = unmatched
            .iter()
            .map(|finding| {
                format!(
                    "{}@{}..{}",
                    finding.rule_id, finding.range.start.byte, finding.range.end.byte
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ProofError::new(format!("unexpected extra finding(s): {extra}")));
    }
    for rule_id in &case.expected_non_findings {
        if findings.iter().any(|finding| finding.rule_id == *rule_id) {
            return Err(ProofError::new(format!(
                "near-miss or negative control produced finding `{rule_id}`"
            )));
        }
    }
    Ok(())
}

fn finding_matches(source: &str, expected: &ExpectedFinding, finding: &CriticFinding) -> bool {
    if finding.rule_id != expected.rule_id {
        return false;
    }
    if finding.range.start.byte != expected.start_byte
        || finding.range.end.byte != expected.end_byte
    {
        return false;
    }
    let excerpt = source.get(expected.start_byte..expected.end_byte).unwrap_or("");
    if excerpt != expected.excerpt {
        return false;
    }
    if proof_severity(finding.severity) != expected.severity {
        return false;
    }
    if proof_remediation(finding.remediation_eligibility()) != expected.remediation_eligibility {
        return false;
    }
    match (&expected.fix_title, finding.fix.as_ref()) {
        (Some(title), Some(fix)) => fix.title == *title,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn prove_suppression_targets_governed_rule(
    source: &str,
    case: &CaseRecord,
    selector: &str,
    config: &CriticConfig,
    registry: &NativeCriticRegistry,
) -> Result<(), ProofError> {
    if !source.lines().any(|line| {
        let trimmed = line.trim_start();
        (trimmed.starts_with("## no critic ") || trimmed.starts_with("## no perl-lsp-critic "))
            && trimmed.contains(selector)
    }) {
        return Err(ProofError::new(format!(
            "suppression fixture does not name selector `{selector}`"
        )));
    }
    let stripped = strip_native_suppression_lines(source);
    let ast = Parser::new(&stripped)
        .parse()
        .map_err(|_| ProofError::new("unsuppressed fixture failed to parse"))?;
    let ctx = CriticContext::new(&stripped, &ast, config);
    let findings = registry.check(&ctx);
    if !findings.iter().any(|finding| finding.rule_id == case.rule_id) {
        return Err(ProofError::new(format!(
            "stripping suppression did not restore finding `{}`",
            case.rule_id
        )));
    }
    Ok(())
}

fn prove_automatic_round_trip(
    source: &str,
    case: &CaseRecord,
    findings: &[CriticFinding],
    round_trip: &FixRoundTrip,
    config: &CriticConfig,
    registry: &NativeCriticRegistry,
) -> Result<(), ProofError> {
    let Some(finding) = findings.iter().find(|finding| finding.rule_id == case.rule_id) else {
        return Err(ProofError::new("automatic round trip needs a target finding"));
    };
    let Some(fix) = finding.fix.as_ref() else {
        return Err(ProofError::new("automatic round trip finding has no fix"));
    };
    if proof_remediation(fix.remediation_eligibility()) != ProofRemediation::AutomaticCandidate {
        return Err(ProofError::new("automatic round trip cannot apply a non-automatic edit"));
    }
    if fix.edits.len() != round_trip.expected_edits.len()
        || fix.edits.iter().zip(&round_trip.expected_edits).any(|(actual, expected)| {
            actual.range.start.byte != expected.start_byte
                || actual.range.end.byte != expected.end_byte
                || actual.new_text != expected.new_text
        })
    {
        return Err(ProofError::new("automatic edit set does not match the manifest exactly"));
    }
    let applied = apply_edits(source, &fix.edits)?;
    let reparsed = Parser::new(&applied).parse();
    match (round_trip.expect_reparse, reparsed) {
        (ParseExpectation::Ok, Err(_)) => {
            return Err(ProofError::new("automatic edit broke parsing"));
        }
        (ParseExpectation::Error, Ok(_)) => {
            return Err(ProofError::new("automatic edit was expected to fail reparse"));
        }
        (ParseExpectation::Error, Err(_)) => return Ok(()),
        (ParseExpectation::Ok, Ok(ast)) => {
            let ctx = CriticContext::new(&applied, &ast, config);
            let after = registry.check(&ctx);
            if round_trip.expect_target_removed
                && after.iter().any(|finding| finding.rule_id == case.rule_id)
            {
                return Err(ProofError::new("automatic edit did not remove the target diagnostic"));
            }
            if round_trip.expect_no_new_governed {
                let governed: std::collections::BTreeSet<&str> =
                    case.include.iter().map(String::as_str).collect();
                let new_rules: Vec<&str> = after
                    .iter()
                    .map(|finding| finding.rule_id.as_str())
                    .filter(|rule_id| governed.contains(rule_id) && *rule_id != case.rule_id)
                    .collect();
                if !new_rules.is_empty() {
                    return Err(ProofError::new(format!(
                        "automatic edit introduced governed diagnostic(s): {}",
                        new_rules.join(", ")
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn apply_edits(source: &str, edits: &[CriticTextEdit]) -> Result<String, ProofError> {
    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for edit in edits {
        let start = edit.range.start.byte;
        let end = edit.range.end.byte;
        if start > end || end > source.len() {
            return Err(ProofError::new(format!(
                "fix edit range {start}..{end} is outside source of length {}",
                source.len()
            )));
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return Err(ProofError::new(format!(
                "fix edit range {start}..{end} is not on a UTF-8 character boundary"
            )));
        }
        spans.push((start, end, edit.new_text.as_str()));
    }
    spans.sort_by(|left, right| right.0.cmp(&left.0).then(right.1.cmp(&left.1)));
    for window in spans.windows(2) {
        if window[1].1 > window[0].0 {
            return Err(ProofError::new("fix edits overlap"));
        }
    }
    let mut output = source.to_string();
    for (start, end, text) in spans {
        output.replace_range(start..end, text);
    }
    Ok(output)
}

fn strip_native_suppression_lines(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("## no critic ") || trimmed.starts_with("## no perl-lsp-critic ")
            {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn critic_config(case: &CaseRecord) -> CriticConfig {
    CriticConfig { include: case.include.clone(), ..CriticConfig::default() }
}

fn native_profile(profile: ProofProfile) -> NativeCriticProfile {
    match profile {
        ProofProfile::Recommended => NativeCriticProfile::Recommended,
        ProofProfile::Strict => NativeCriticProfile::Strict,
    }
}

fn proof_severity(severity: Severity) -> ProofSeverity {
    match severity {
        Severity::Gentle => ProofSeverity::Gentle,
        Severity::Stern => ProofSeverity::Stern,
        Severity::Harsh => ProofSeverity::Harsh,
        Severity::Cruel => ProofSeverity::Cruel,
        Severity::Brutal => ProofSeverity::Brutal,
    }
}

fn proof_remediation(eligibility: CriticRemediationEligibility) -> ProofRemediation {
    match eligibility {
        CriticRemediationEligibility::None => ProofRemediation::None,
        CriticRemediationEligibility::Manual => ProofRemediation::Manual,
        CriticRemediationEligibility::PreviewCandidate => ProofRemediation::PreviewCandidate,
        CriticRemediationEligibility::AutomaticCandidate => ProofRemediation::AutomaticCandidate,
    }
}

#[cfg(test)]
mod tests {
    use super::apply_edits;
    use perl_lsp_rs_core::tooling::perl_critic::CriticTextEdit;
    use perl_parser_core::position::{Position, Range};

    fn edit(start: usize, end: usize, new_text: &str) -> CriticTextEdit {
        CriticTextEdit {
            range: Range {
                start: Position { byte: start, line: 0, column: 0 },
                end: Position { byte: end, line: 0, column: 0 },
            },
            new_text: new_text.to_string(),
        }
    }

    #[test]
    fn apply_edits_inserts_from_the_end() {
        let applied = apply_edits("my $x = 1;\n", &[edit(0, 0, "use strict;\n")]).expect("apply");
        assert_eq!(applied, "use strict;\nmy $x = 1;\n");
    }

    #[test]
    fn apply_edits_rejects_overlapping_ranges() {
        let error = apply_edits("abcdef", &[edit(0, 4, "x"), edit(2, 6, "y")])
            .expect_err("overlap")
            .to_string();
        assert!(error.contains("overlap"), "{error}");
    }

    #[test]
    fn apply_edits_rejects_range_past_source_end() {
        let error = apply_edits("abc", &[edit(0, 4, "x")]).expect_err("oob").to_string();
        assert!(error.contains("outside source"), "{error}");
    }

    #[test]
    fn apply_edits_rejects_mid_codepoint_range() {
        // "café" — é is U+00E9 encoded as c3 a9, so byte 4 sits inside the codepoint.
        let error = apply_edits("café", &[edit(4, 4, "x")]).expect_err("utf-8").to_string();
        assert!(error.contains("UTF-8 character boundary"), "{error}");
    }
}
