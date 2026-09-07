//! Live native-critic execution against rule-proof fixtures.

use perl_lsp_rs_core::tooling::perl_critic::{
    CriticConfig, CriticContext, CriticFinding, CriticTextEdit, NativeCriticRegistry,
};
use perl_parser::Parser;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::error::ProofError;
use super::mapping::{native_profile, proof_remediation, proof_severity};
use super::model::{
    CaseRecord, ExpectedFinding, FixRoundTrip, ParseExpectation, ProofRemediation,
    RuleProofManifest, resolve_fixture_path,
};

/// Crate prefixes whose live behavior can change rule-proof findings.
/// The advisory lane must dispatch when these owners change (#14560).
pub const EXECUTE_LIVE_OWNER_PATHS: &[&str] = &[
    "crates/perl-parser/**",
    "crates/perl-parser-core/**",
    "crates/perl-lexer/**",
    "crates/perl-ast/**",
    "crates/perl-semantic-analyzer/**",
    "crates/perl-pragma/**",
];

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
        prove_suppression_targets_governed_rule(
            &source,
            &case.rule_id,
            selector,
            &config,
            &registry,
        )?;
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
        excerpt_at(source, expected.start_byte, expected.end_byte)?;
        let mut matched = None;
        for (index, finding) in unmatched.iter().enumerate() {
            if finding_matches(source, expected, finding)? {
                matched = Some(index);
                break;
            }
        }
        let Some(index) = matched else {
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

fn finding_matches(
    source: &str,
    expected: &ExpectedFinding,
    finding: &CriticFinding,
) -> Result<bool, ProofError> {
    if finding.rule_id != expected.rule_id {
        return Ok(false);
    }
    if finding.range.start.byte != expected.start_byte
        || finding.range.end.byte != expected.end_byte
    {
        return Ok(false);
    }
    let excerpt = excerpt_at(source, expected.start_byte, expected.end_byte)?;
    if excerpt != expected.excerpt {
        return Ok(false);
    }
    if proof_severity(finding.severity) != expected.severity {
        return Ok(false);
    }
    if proof_remediation(finding.remediation_eligibility()) != expected.remediation_eligibility {
        return Ok(false);
    }
    let actual_title = finding.fix.as_ref().map(|fix| fix.title.as_str());
    match (expected.fix_title.as_deref(), actual_title) {
        (Some(expected_title), Some(actual_title)) => Ok(expected_title == actual_title),
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

pub(crate) fn excerpt_at(source: &str, start: usize, end: usize) -> Result<&str, ProofError> {
    if start > end || end > source.len() {
        return Err(ProofError::new(format!(
            "expected excerpt range {start}..{end} is outside source of length {}",
            source.len()
        )));
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return Err(ProofError::new(format!(
            "expected excerpt range {start}..{end} is not on a UTF-8 character boundary"
        )));
    }
    source.get(start..end).ok_or_else(|| {
        ProofError::new(format!("expected excerpt range {start}..{end} is not a valid slice"))
    })
}

fn prove_suppression_targets_governed_rule(
    source: &str,
    rule_id: &str,
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
    if !findings.iter().any(|finding| finding.rule_id == rule_id) {
        return Err(ProofError::new(format!(
            "stripping suppression did not restore finding `{rule_id}`"
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
                let governed: BTreeSet<&str> = case.include.iter().map(String::as_str).collect();
                let new_rules = newly_introduced_governed(
                    findings.iter().map(|finding| finding.rule_id.as_str()),
                    after.iter().map(|finding| finding.rule_id.as_str()),
                    &governed,
                );
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

/// Governed rule IDs present after a fix that were not present before.
/// Pre-existing other diagnostics may remain; only newly introduced include-listed
/// rules fail `expect_no_new_governed`.
pub(crate) fn newly_introduced_governed<'a>(
    before: impl IntoIterator<Item = &'a str>,
    after: impl IntoIterator<Item = &'a str>,
    governed: &BTreeSet<&str>,
) -> Vec<&'a str> {
    let before: BTreeSet<&str> = before.into_iter().filter(|id| governed.contains(id)).collect();
    let mut seen = BTreeSet::new();
    let mut introduced = Vec::new();
    for id in after {
        if governed.contains(id) && !before.contains(id) && seen.insert(id) {
            introduced.push(id);
        }
    }
    introduced
}

pub(crate) fn apply_edits(source: &str, edits: &[CriticTextEdit]) -> Result<String, ProofError> {
    let mut spans: Vec<(usize, usize, usize, &str)> = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
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
        spans.push((start, end, index, edit.new_text.as_str()));
    }
    // Later source positions first; same-position inserts keep list order by
    // applying later-in-list edits first.
    spans.sort_by(|left, right| {
        right.0.cmp(&left.0).then(right.1.cmp(&left.1)).then(right.2.cmp(&left.2))
    });
    for window in spans.windows(2) {
        if window[1].1 > window[0].0 {
            return Err(ProofError::new("fix edits overlap"));
        }
    }
    let mut output = source.to_string();
    for (start, end, _, text) in spans {
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

#[cfg(test)]
mod tests {
    use super::{
        apply_edits, excerpt_at, newly_introduced_governed, prove_suppression_targets_governed_rule,
    };
    use perl_lsp_rs_core::tooling::perl_critic::{
        CriticConfig, CriticTextEdit, NativeCriticProfile, NativeCriticRegistry,
    };
    use perl_parser_core::position::{Position, Range};
    use std::collections::BTreeSet;

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
    fn apply_edits_same_position_inserts_keep_list_order() {
        let applied =
            apply_edits("X", &[edit(0, 0, "A"), edit(0, 0, "B")]).expect("same-position inserts");
        assert_eq!(applied, "ABX");
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

    #[test]
    fn excerpt_at_rejects_range_past_source_end() {
        let error = excerpt_at("abc", 4, 4).expect_err("oob").to_string();
        assert!(error.contains("outside source"), "{error}");
    }

    #[test]
    fn excerpt_at_rejects_mid_codepoint_range() {
        let error = excerpt_at("café", 4, 4).expect_err("utf-8").to_string();
        assert!(error.contains("UTF-8 character boundary"), "{error}");
    }

    #[test]
    fn expect_no_new_governed_allows_preexisting_other_diagnostics() {
        let governed: BTreeSet<&str> =
            ["native.testing.require_use_strict", "native.common.assignment_in_condition"]
                .into_iter()
                .collect();
        let introduced = newly_introduced_governed(
            ["native.testing.require_use_strict", "native.common.assignment_in_condition"],
            ["native.common.assignment_in_condition"],
            &governed,
        );
        assert!(introduced.is_empty(), "{introduced:?}");
    }

    #[test]
    fn expect_no_new_governed_rejects_newly_introduced_include_rule() {
        let governed: BTreeSet<&str> =
            ["native.testing.require_use_strict", "native.common.assignment_in_condition"]
                .into_iter()
                .collect();
        let introduced = newly_introduced_governed(
            ["native.testing.require_use_strict"],
            ["native.common.assignment_in_condition"],
            &governed,
        );
        assert_eq!(introduced, ["native.common.assignment_in_condition"]);
    }

    fn strict_registry() -> (CriticConfig, NativeCriticRegistry) {
        let config = CriticConfig {
            include: vec!["native.testing.require_use_strict".to_string()],
            ..CriticConfig::default()
        };
        let registry = NativeCriticRegistry::for_profile_with_config(
            NativeCriticProfile::Recommended,
            &config,
        );
        (config, registry)
    }

    #[test]
    fn suppression_strip_on_already_clean_source_fails_counterfactual() {
        let (config, registry) = strict_registry();
        let source = "## no critic native.testing.require_use_strict\nuse strict;\nmy $x = 1;\n";
        let error = prove_suppression_targets_governed_rule(
            source,
            "native.testing.require_use_strict",
            "native.testing.require_use_strict",
            &config,
            &registry,
        )
        .expect_err("vacuous suppression")
        .to_string();
        assert!(error.contains("stripping suppression did not restore"), "{error}");
    }

    #[test]
    fn ineffective_suppression_comment_does_not_count_as_selector() {
        let (config, registry) = strict_registry();
        let source = "my $x = 1;\n# native.testing.require_use_strict\n";
        let error = prove_suppression_targets_governed_rule(
            source,
            "native.testing.require_use_strict",
            "native.testing.require_use_strict",
            &config,
            &registry,
        )
        .expect_err("non-directive")
        .to_string();
        assert!(error.contains("does not name selector"), "{error}");
    }

    #[test]
    fn suppression_strip_restores_governed_finding() {
        let (config, registry) = strict_registry();
        let source =
            "## no critic native.testing.require_use_strict -- generated bootstrap\nmy $x = 1;\n";
        prove_suppression_targets_governed_rule(
            source,
            "native.testing.require_use_strict",
            "native.testing.require_use_strict",
            &config,
            &registry,
        )
        .expect("effective suppression counterfactual");
    }
}
