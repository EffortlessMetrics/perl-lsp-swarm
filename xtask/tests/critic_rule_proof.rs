//! Discriminating tests for the critic rule-proof manifest and checker (#6973).

use perl_lsp_rs_core::tooling::perl_critic::{NativeCriticProfile, NativeCriticRegistry};
use serde_json::{Value, json};
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::critic_rule_proof::{
    self as proof, MANIFEST_PATH, STATUS_PATH, resolve_fixture_path, validate_manifest_value,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn canonical_manifest() -> TestResult<Value> {
    let bytes = std::fs::read(repo_root().join(MANIFEST_PATH))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn expect_violation(value: &Value, needle: &str) -> TestResult {
    let error = match validate_manifest_value(&repo_root(), value) {
        Ok(_) => "manifest unexpectedly validated".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains(needle), "expected `{needle}` in:\n{error}");
    Ok(())
}

fn case_mut<'a>(manifest: &'a mut Value, case_id: &str) -> Option<&'a mut Value> {
    manifest
        .get_mut("cases")?
        .as_array_mut()?
        .iter_mut()
        .find(|case| case.get("case_id").and_then(Value::as_str) == Some(case_id))
}

fn rule_mut<'a>(manifest: &'a mut Value, rule_id: &str) -> Option<&'a mut Value> {
    manifest
        .get_mut("rules")?
        .as_array_mut()?
        .iter_mut()
        .find(|rule| rule.get("rule_id").and_then(Value::as_str) == Some(rule_id))
}

#[test]
fn committed_manifest_and_live_critic_proof_pass() -> TestResult {
    let manifest = proof::check(&repo_root()).map_err(|error| error.to_string())?;
    assert_eq!(manifest.rules.len(), 4);
    assert_eq!(manifest.cases.len(), 20);
    assert!(
        manifest
            .cases
            .iter()
            .any(|case| { case.case_id == "CRP-STRICT-POS-001" && case.fix_round_trip.is_some() })
    );
    Ok(())
}

#[test]
fn status_render_is_byte_deterministic() -> TestResult {
    let first = proof::load_and_validate(&repo_root()).map_err(|error| error.to_string())?;
    let second = proof::load_and_validate(&repo_root()).map_err(|error| error.to_string())?;
    let rendered = proof::render_status(&first);
    assert_eq!(rendered, proof::render_status(&second));
    assert!(rendered.contains("MISSING"));
    assert!(rendered.contains("n/a"));
    let catalog = NativeCriticRegistry::for_profile(NativeCriticProfile::Strict).len();
    let remainder = catalog.saturating_sub(first.rules.len());
    assert!(
        rendered.contains(&format!("{remainder} catalog native rules")),
        "status remainder must follow the live strict catalog, not a hardcoded 4-rule total:\n{rendered}"
    );
    assert!(
        rendered.contains("`write-status`"),
        "status must name the real write-status subcommand:\n{rendered}"
    );
    assert!(
        !rendered.contains("--write-status"),
        "status must not recommend a flag the CLI does not accept:\n{rendered}"
    );
    assert!(
        !rendered.contains("4-rule recommended/strict totals"),
        "status must not inherit proof from recommended/strict rule-count totals:\n{rendered}"
    );
    Ok(())
}

#[test]
fn duplicate_case_id_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let duplicate = case_mut(&mut manifest, "CRP-STRICT-NEAR-001").expect("near miss").clone();
    manifest["cases"].as_array_mut().expect("cases").push(duplicate);
    expect_violation(&manifest, "duplicate case_id")
}

#[test]
fn duplicate_rule_id_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let duplicate =
        rule_mut(&mut manifest, "native.security.string_eval").expect("string eval").clone();
    manifest["rules"].as_array_mut().expect("rules").push(duplicate);
    expect_violation(&manifest, "duplicate rule_id")
}

#[test]
fn missing_fixture_digest_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["fixtures"]["require_use_strict/positive.pl"] = json!({});
    expect_violation(&manifest, "digest")
}

#[test]
fn stale_fixture_digest_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["fixtures"]["require_use_strict/positive.pl"]["digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");
    expect_violation(&manifest, "digest is stale")
}

#[test]
fn unknown_evidence_class_fails_schema() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-NEAR-001").expect("near miss")["evidence_classes"] =
        json!(["not_a_real_class"]);
    expect_violation(&manifest, "schema")
}

#[test]
fn suggested_edit_cannot_be_marked_automatic() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "CRP-ASSIGN-POS-001").expect("assign pos");
    let classes = case["evidence_classes"].as_array_mut().expect("classes");
    classes.push(json!("automatic_fix_round_trip"));
    case["fix_round_trip"] = json!({
        "apply": "automatic",
        "expect_reparse": "ok",
        "expect_target_removed": true,
        "expect_no_new_governed": true,
        "expected_edits": [{"start_byte": 0, "end_byte": 0, "new_text": "use strict;\n"}]
    });
    expect_violation(&manifest, "automatic_fix_round_trip is impossible")
}

#[test]
fn diagnostic_only_cannot_be_represented_as_automatic_success() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-EVAL-POS-001").expect("eval pos")["expected_findings"][0]["remediation_eligibility"] =
        json!("automatic_candidate");
    expect_violation(&manifest, "cannot be represented as automatic success")
}

#[test]
fn deleting_a_required_negative_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let cases = manifest["cases"].as_array_mut().expect("cases");
    cases.retain(|case| case.get("case_id").and_then(Value::as_str) != Some("CRP-STRICT-NEAR-001"));
    expect_violation(&manifest, "missing required evidence class `near_miss_negative`")
}

#[test]
fn suppression_of_a_different_logical_rule_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-SUPP-001").expect("supp")["suppression_selector"] =
        json!("native.security.string_eval");
    expect_violation(&manifest, "suppression_selector")
}

#[test]
fn unknown_rule_id_fails_authority_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    rule_mut(&mut manifest, "native.testing.require_use_strict").expect("strict")["rule_id"] =
        json!("native.testing.not_a_rule");
    expect_violation(&manifest, "unknown native rule id")
}

#[test]
fn canonical_id_must_match_identity_registry() -> TestResult {
    let mut manifest = canonical_manifest()?;
    rule_mut(&mut manifest, "native.testing.require_use_strict").expect("strict")["canonical_id"] =
        json!("critic.testing.not_canonical");
    expect_violation(&manifest, "canonical_id")
}

#[test]
fn deleting_an_identity_alias_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    rule_mut(&mut manifest, "native.testing.require_use_strict").expect("strict")
        ["identity_aliases"]
        .as_array_mut()
        .expect("aliases")
        .pop();
    expect_violation(&manifest, "identity_aliases do not match")
}

#[test]
fn positive_finding_at_the_wrong_range_fails_live_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-ASSIGN-POS-001").expect("assign pos")["expected_findings"][0]["start_byte"] =
        json!(0);
    case_mut(&mut manifest, "CRP-ASSIGN-POS-001").expect("assign pos")["expected_findings"][0]["end_byte"] =
        json!(6);
    case_mut(&mut manifest, "CRP-ASSIGN-POS-001").expect("assign pos")["expected_findings"][0]["excerpt"] =
        json!("use st");
    let typed =
        validate_manifest_value(&repo_root(), &manifest).map_err(|error| error.to_string())?;
    let error = proof::execute_manifest(&repo_root(), &typed)
        .expect_err("wrong range must fail live critic")
        .to_string();
    assert!(
        error.contains("missing expected finding") || error.contains("unexpected extra finding"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn relabeling_a_near_miss_as_a_positive_fails_live_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "CRP-ASSIGN-NEAR-001").expect("near");
    case["evidence_classes"] = json!(["near_miss_negative", "positive_finding"]);
    case["expected_findings"] = json!([{
        "rule_id": "native.common.assignment_in_condition",
        "start_byte": 0,
        "end_byte": 0,
        "excerpt": "",
        "severity": "stern",
        "remediation_eligibility": "preview_candidate"
    }]);
    let typed =
        validate_manifest_value(&repo_root(), &manifest).map_err(|error| error.to_string())?;
    let error = proof::execute_manifest(&repo_root(), &typed)
        .expect_err("near miss must not satisfy a positive")
        .to_string();
    assert!(
        error.contains("missing expected finding") || error.contains("unexpected extra finding"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn unused_fixture_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["fixtures"]["orphan.pl"] = json!({
        "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    });
    expect_violation(&manifest, "unused fixture identity")
}

#[test]
fn case_profile_mismatch_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["profile"] = json!("strict");
    expect_violation(&manifest, "does not match the governed rule profile")
}

#[test]
fn absolute_fixture_path_fails_schema() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["fixture"] =
        json!("/etc/passwd");
    expect_violation(&manifest, "schema:")
}

#[test]
fn traversal_fixture_path_fails_schema() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["fixture"] =
        json!("../../Cargo.toml");
    expect_violation(&manifest, "schema:")
}

#[test]
fn fixture_paths_may_not_escape_the_fixture_root() -> TestResult {
    let base = std::env::temp_dir().join(format!("crp-fixture-escape-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("fixtures/critic-rule-proof/inner"))?;
    std::fs::write(base.join("fixtures/critic-rule-proof/inner/real.pl"), "1;\n")?;
    std::fs::write(base.join("outside.pl"), "2;\n")?;

    assert!(resolve_fixture_path(&base, "/etc/passwd").is_err(), "absolute path must fail");
    assert!(resolve_fixture_path(&base, "../outside.pl").is_err(), "traversal must fail");
    assert!(
        resolve_fixture_path(&base, "inner\\real.pl").is_err(),
        "backslash separator must fail"
    );
    assert!(
        resolve_fixture_path(&base, "missing/real.pl").is_err(),
        "unresolvable fixture must fail"
    );
    assert!(resolve_fixture_path(&base, "inner/real.pl").is_ok(), "contained fixture must resolve");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            base.join("outside.pl"),
            base.join("fixtures/critic-rule-proof/inner/link.pl"),
        )?;
        assert!(
            resolve_fixture_path(&base, "inner/link.pl").is_err(),
            "symlinked escape must fail"
        );
    }

    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}

#[test]
fn include_missing_governed_rule_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["include"] =
        json!(["native.security.string_eval"]);
    expect_violation(&manifest, "include must contain the governed rule")
}

#[test]
fn include_unknown_rule_fails_profile_binding() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["include"] =
        json!(["native.testing.require_use_strict", "native.typo"]);
    expect_violation(&manifest, "include rule `native.typo` is not in the `recommended` profile roster")
}

#[test]
fn strict_only_rule_cannot_be_included_in_recommended_case() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["include"] =
        json!(["native.testing.require_use_strict", "native.regex.capture_without_match"]);
    expect_violation(&manifest, "is not in the `recommended` profile roster")
}

#[test]
fn automatic_edit_identity_is_required() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["cases"][0]["fix_round_trip"]["expected_edits"] = json!([
        {"start_byte": 0, "end_byte": 0, "new_text": "use warnings;\n"}
    ]);
    let typed = validate_manifest_value(&repo_root(), &manifest)?;
    let error = proof::execute_manifest(&repo_root(), &typed)
        .expect_err("wrong automatic edit must fail")
        .to_string();
    assert!(error.contains("automatic edit set does not match the manifest exactly"));
    Ok(())
}

#[test]
fn automatic_edit_set_cannot_be_omitted() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["cases"][0]["fix_round_trip"]["expected_edits"] = json!([]);
    expect_violation(&manifest, "expected_edits")
}

#[test]
fn parse_error_boundary_cannot_claim_findings() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-BOUND-001").expect("strict boundary")["parse_expectation"] =
        json!("error");
    expect_violation(&manifest, "malformed parse boundaries cannot claim expected findings")
}

#[test]
fn automatic_rule_missing_round_trip_class_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos");
    case["evidence_classes"] = json!([
        "positive_finding",
        "canonical_identity",
        "source_range_and_severity",
        "remediation_class"
    ]);
    expect_violation(&manifest, "missing required evidence class `automatic_fix_round_trip`")
}

#[test]
fn automatic_success_without_target_removal_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-STRICT-POS-001").expect("strict pos")["fix_round_trip"]["expect_target_removed"] =
        json!(false);
    expect_violation(&manifest, "target removal")
}

#[test]
fn unknown_alias_shape_fails_schema() -> TestResult {
    let mut manifest = canonical_manifest()?;
    rule_mut(&mut manifest, "native.testing.require_use_strict").expect("strict")["identity_aliases"]
        [0]["shape"] = json!("not_a_shape");
    expect_violation(&manifest, "schema")
}

#[test]
fn near_miss_fixture_that_starts_producing_a_finding_fails_live_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    case_mut(&mut manifest, "CRP-ASSIGN-NEAR-001").expect("near")["fixture"] =
        json!("assignment_in_condition/positive.pl");
    manifest["fixtures"]
        .as_object_mut()
        .expect("fixtures")
        .remove("assignment_in_condition/near_miss.pl");
    let typed =
        validate_manifest_value(&repo_root(), &manifest).map_err(|error| error.to_string())?;
    let error = proof::execute_manifest(&repo_root(), &typed)
        .expect_err("a near miss must fail when the fixture starts producing the finding")
        .to_string();
    assert!(
        error.contains("near-miss or negative control produced finding")
            || error.contains("unexpected extra finding"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn positive_with_no_recorded_range_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "CRP-ASSIGN-POS-001").expect("assign pos");
    case["expected_findings"] = json!([]);
    expect_violation(&manifest, "requires an expected finding for governed rule")
}

#[test]
fn parse_error_cannot_claim_non_boundary_evidence() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "CRP-STRICT-BOUND-001").expect("strict boundary");
    case["parse_expectation"] = json!("error");
    case["expected_findings"] = json!([]);
    case["evidence_classes"] = json!(["boundary", "positive_finding", "file_level_suppression"]);
    expect_violation(&manifest, "parse-error fixtures may only declare boundary evidence")
}

#[test]
fn positive_class_without_governed_finding_fails_check() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "CRP-ASSIGN-NEAR-001").expect("near");
    case["evidence_classes"] = json!(["near_miss_negative", "positive_finding"]);
    expect_violation(&manifest, "requires an expected finding for governed rule")
}

#[test]
fn declared_remediation_must_match_expected_finding() -> TestResult {
    let mut manifest = canonical_manifest()?;
    rule_mut(&mut manifest, "native.security.string_eval").expect("eval")["declared_remediation"] =
        json!("manual");
    expect_violation(&manifest, "does not match declared_remediation")
}

#[test]
fn generated_status_marks_missing_and_not_applicable_classes() -> TestResult {
    let manifest = proof::load_and_validate(&repo_root()).map_err(|error| error.to_string())?;
    let rendered = proof::render_status(&manifest);
    assert!(rendered.contains("`native.security.string_eval`"));
    assert!(rendered.contains("`native.regex.capture_without_match`"));
    assert!(
        rendered
            .contains("| `native.security.string_eval` | `critic.security.string_eval` | `none` |")
    );
    assert!(rendered.contains("n/a"));
    let status = std::fs::read_to_string(repo_root().join(STATUS_PATH))?;
    assert_eq!(status, rendered);
    Ok(())
}
