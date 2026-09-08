//! Contract tests for the compiler lexical cut-line cases manifest (#12156).
//!
//! The canonical manifest is the valid fixture; each negative test corrupts it
//! in memory along one intended falsifier axis and proves the validator fails
//! for that reason. Static invalid fixtures cover top-level shape checks.

use serde_json::Value;
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::compiler_lexical_cutline::{self as cutline, CutlineValidationError, MANIFEST_PATH};

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

fn validation_error(result: Result<cutline::ValidationStats, CutlineValidationError>) -> String {
    match result {
        Ok(_) => "manifest unexpectedly validated".to_string(),
        Err(error) => error.to_string(),
    }
}

fn expect_violation(manifest: &Value, needle: &str) -> TestResult {
    let error = validation_error(cutline::validate_manifest_value(manifest));
    assert!(error.contains(needle), "expected violation containing `{needle}`, got:\n{error}");
    Ok(())
}

fn case_mut<'a>(manifest: &'a mut Value, case_id: &str) -> Option<&'a mut Value> {
    manifest
        .get_mut("cases")?
        .as_array_mut()?
        .iter_mut()
        .find(|case| case.get("case_id").and_then(Value::as_str) == Some(case_id))
}

fn mutation_mut<'a>(manifest: &'a mut Value, mutation_id: &str) -> Option<&'a mut Value> {
    manifest
        .get_mut("mutations")?
        .as_array_mut()?
        .iter_mut()
        .find(|mutation| mutation.get("mutation_id").and_then(Value::as_str) == Some(mutation_id))
}

fn invariant_mut<'a>(manifest: &'a mut Value, invariant_id: &str) -> Option<&'a mut Value> {
    manifest
        .get_mut("work_invariants")?
        .as_array_mut()?
        .iter_mut()
        .find(|invariant| invariant.get("id").and_then(Value::as_str) == Some(invariant_id))
}

#[test]
fn canonical_manifest_validates() -> TestResult {
    let stats = cutline::validate_manifest_file(&repo_root())?;
    assert_eq!(stats.cases, 43);
    assert_eq!(stats.mutations, 37);
    assert_eq!(stats.work_invariants, 18);
    assert_eq!(stats.fixtures, 16);
    Ok(())
}

#[test]
fn list_and_explain_cover_the_manifest() -> TestResult {
    let manifest = cutline::load_manifest(&repo_root())?;
    let ids = cutline::list_case_ids(&manifest);
    assert_eq!(ids.len(), 43);
    assert!(ids.iter().any(|id| id == "LX-POS-001"));
    let explained =
        cutline::explain_case(&manifest, "LX-POS-001").ok_or("LX-POS-001 missing from explain")?;
    assert!(explained.contains("for-loop-decl-read"));
    assert!(cutline::explain_case(&manifest, "LX-POS-999").is_none());
    Ok(())
}

#[test]
fn rejects_duplicate_case_id() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-002").ok_or("missing case")?;
    case["case_id"] = Value::from("LX-POS-001");
    expect_violation(&manifest, "duplicate case id")
}

#[test]
fn rejects_mutation_without_existing_row() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let mutation = mutation_mut(&mut manifest, "LX-MUT-01").ok_or("missing mutation")?;
    mutation["fails_rows"] = serde_json::json!(["LX-POS-999"]);
    expect_violation(&manifest, "unknown case `LX-POS-999`")
}

#[test]
fn rejects_case_listing_unknown_mutation() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-001").ok_or("missing case")?;
    case["mutations"] = serde_json::json!(["LX-MUT-99"]);
    expect_violation(&manifest, "lists unknown mutation `LX-MUT-99`")
}

#[test]
fn rejects_one_directional_mutation_mapping() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let mutation = mutation_mut(&mut manifest, "LX-MUT-02").ok_or("missing mutation")?;
    mutation["fails_rows"] = serde_json::json!(["LX-POS-003"]);
    expect_violation(&manifest, "does not list it")
}

#[test]
fn rejects_forged_anchor_text() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-001").ok_or("missing case")?;
    case["expected"]["declaration_anchor"]["byte_start"] = Value::from(8);
    expect_violation(&manifest, "selects")
}

#[test]
fn rejects_utf16_position_drift() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-014").ok_or("missing case")?;
    case["expected"]["reference_locations"][0]["character_start"] = Value::from(22);
    expect_violation(&manifest, "recorded UTF-16 position does not match")
}

#[test]
fn rejects_plan_subset_of_authorized_ids() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-RN-001").ok_or("missing case")?;
    case["rename"]["plan_edit_ids"] = serde_json::json!(["lx-rn-001-1"]);
    expect_violation(&manifest, "must be identical on success")
}

#[test]
fn rejects_projection_superset_of_plan_ids() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-RN-001").ok_or("missing case")?;
    case["rename"]["projected_edit_ids"] =
        serde_json::json!(["lx-rn-001-1", "lx-rn-001-2", "lx-rn-001-3"]);
    expect_violation(&manifest, "must be identical on success")
}

#[test]
fn rejects_broken_postcondition() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-RN-001").ok_or("missing case")?;
    case["rename"]["postcondition_source"] = Value::from("for my $i (1 .. 3) { print $i }");
    expect_violation(&manifest, "postcondition")
}

#[test]
fn rejects_refusal_with_partial_edits() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-LC-004").ok_or("missing case")?;
    case["rename"]["plan_edit_ids"] = serde_json::json!(["partial-1"]);
    expect_violation(&manifest, "no edits and no partial ID sets")
}

#[test]
fn rejects_protocol_continuation_token() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["protocol_lifecycle"]["preparation_token"] = Value::from("opaque-id");
    expect_violation(&manifest, "unknown field `preparation_token`")
}

#[test]
fn rejects_prior_preparation_authorization() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-RN-002").ok_or("missing case")?;
    case["rename"]["authorization"] = Value::from("prior-preparation-observation");
    expect_violation(&manifest, "authorization")
}

#[test]
fn rejects_old_plan_reuse() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-LC-003").ok_or("missing case")?;
    case["preparation"]["old_plan_reuse"] = Value::from("allowed");
    expect_violation(&manifest, "old_plan_reuse")
}

#[test]
fn rejects_unknown_correlation_outcome() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-LC-003").ok_or("missing case")?;
    case["preparation"]["correlation_outcome"] = Value::from("stale_but_fine");
    expect_violation(&manifest, "correlation_outcome")
}

#[test]
fn rejects_zero_without_instrument() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let invariant = invariant_mut(&mut manifest, "WI-01").ok_or("missing invariant")?;
    invariant.as_object_mut().ok_or("invariant not object")?.remove("instrument");
    expect_violation(&manifest, "instrument")
}

#[test]
fn rejects_pending_without_4306_note() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let invariant = invariant_mut(&mut manifest, "WI-01").ok_or("missing invariant")?;
    invariant["status"] = Value::from("pending");
    expect_violation(&manifest, "pending assertion requires a note naming #4306")
}

#[test]
fn rejects_pending_nonzero_assertion() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let invariant = invariant_mut(&mut manifest, "WI-12").ok_or("missing invariant")?;
    invariant["status"] = Value::from("pending");
    invariant["note"] = Value::from("pending before #4306");
    expect_violation(&manifest, "only zero assertions may be pending")
}

#[test]
fn rejects_include_declaration_true_on_admitted_row() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-003").ok_or("missing case")?;
    case["request"]["include_declaration"] = Value::from(true);
    expect_violation(&manifest, "admitted references rows must use false")
}

#[test]
fn rejects_exact_empty_with_locations() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-003").ok_or("missing case")?;
    case["expected"]["result_class"] = Value::from("exact_empty");
    expect_violation(&manifest, "exact_empty rows must be empty")
}

#[test]
fn rejects_fallback_on_admitted_row() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-004").ok_or("missing case")?;
    case["expected"]["fallback_invoked"] = Value::from(true);
    expect_violation(&manifest, "admitted rows never invoke fallback")
}

#[test]
fn rejects_missing_positive_denominator_coverage() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let cases = manifest.get_mut("cases").and_then(Value::as_array_mut).ok_or("no cases")?;
    cases.retain(|case| case.get("case_id").and_then(Value::as_str) != Some("LX-POS-014"));
    expect_violation(&manifest, "admitted denominator missing coverage `unicode_astral_geometry`")
}

#[test]
fn rejects_missing_exclusion_denominator_coverage() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let cases = manifest.get_mut("cases").and_then(Value::as_array_mut).ok_or("no cases")?;
    cases.retain(|case| case.get("case_id").and_then(Value::as_str) != Some("LX-EXC-003"));
    expect_violation(&manifest, "exclusion denominator missing coverage `package_global`")
}

#[test]
fn rejects_missing_preparation_scenario() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let cases = manifest.get_mut("cases").and_then(Value::as_array_mut).ok_or("no cases")?;
    cases.retain(|case| case.get("case_id").and_then(Value::as_str) != Some("LX-LC-006"));
    expect_violation(&manifest, "preparation scenario `cache_miss_eviction` has no row")
}

#[test]
fn rejects_missing_mutation() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let mutations =
        manifest.get_mut("mutations").and_then(Value::as_array_mut).ok_or("no mutations")?;
    mutations.retain(|mutation| {
        mutation.get("mutation_id").and_then(Value::as_str) != Some("LX-MUT-37")
    });
    let error = validation_error(cutline::validate_manifest_value(&manifest));
    assert!(
        error.contains("expected exactly 37 controlled mutations"),
        "unexpected error:\n{error}"
    );
    Ok(())
}

#[test]
fn rejects_duplicate_occurrence_in_denominator() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-002").ok_or("missing case")?;
    let duplicated = case["expected"]["reference_locations"][0].clone();
    case["expected"]["reference_locations"]
        .as_array_mut()
        .ok_or("locations not an array")?
        .push(duplicated);
    expect_violation(&manifest, "duplicate occurrence range inflates the exact denominator")
}

#[test]
fn rejects_missing_test_target() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let targets =
        manifest.get_mut("test_targets").and_then(Value::as_array_mut).ok_or("no targets")?;
    targets.retain(|target| {
        target.get("target").and_then(Value::as_str) != Some("compiler_rename_stdio")
    });
    expect_violation(&manifest, "missing required target `compiler_rename_stdio`")
}

#[test]
fn rejects_subject_off_binding() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-001").ok_or("missing case")?;
    case["request"]["subject"] = serde_json::json!({"line": 0, "character": 0});
    expect_violation(&manifest, "does not land on the binding")
}

#[test]
fn rejects_fixture_digest_drift() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["fixtures"][0]["source"] = Value::from("for my $i (1 .. 3) { print $i }\n");
    expect_violation(&manifest, "digest mismatch")
}

#[test]
fn rejects_noncanonical_bytes() -> TestResult {
    let manifest = canonical_manifest()?;
    let noncanonical = format!("{:#?}", "");
    let _ = noncanonical;
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut text = String::from_utf8(bytes)?;
    text = text.replacen("  \"schema_version\"", "    \"schema_version\"", 1);
    let error = validation_error(cutline::validate_manifest_bytes(text.as_bytes()));
    assert!(error.contains("canonical"), "unexpected error:\n{error}");
    Ok(())
}

#[test]
fn static_invalid_fixture_wrong_schema_version() -> TestResult {
    let bytes = std::fs::read(
        repo_root()
            .join("xtask/tests/fixtures/compiler_lexical_cutline/invalid-schema-version.json"),
    )?;
    let value: Value = serde_json::from_slice(&bytes)?;
    expect_violation(&value, "schema_version")
}

#[test]
fn static_invalid_fixture_continuation_token() -> TestResult {
    let bytes = std::fs::read(
        repo_root()
            .join("xtask/tests/fixtures/compiler_lexical_cutline/invalid-continuation-token.json"),
    )?;
    let value: Value = serde_json::from_slice(&bytes)?;
    expect_violation(&value, "unknown field `preparation_continuation`")
}

#[test]
fn rejects_structural_drift_the_schema_owns() -> TestResult {
    // The schema is applied, not merely parsed: removing a schema-required
    // field the handwritten checks do not name must fail validation.
    let root = repo_root();
    let temp = tempfile::tempdir()?;
    let temp_root = temp.path();
    let schema_dest = temp_root.join(cutline::SCHEMA_PATH);
    let manifest_dest = temp_root.join(cutline::MANIFEST_PATH);
    std::fs::create_dir_all(schema_dest.parent().ok_or("schema parent")?)?;
    std::fs::create_dir_all(manifest_dest.parent().ok_or("manifest parent")?)?;
    std::fs::copy(root.join(cutline::SCHEMA_PATH), &schema_dest)?;
    let mut manifest = canonical_manifest()?;
    manifest.as_object_mut().ok_or("manifest object")?.remove("owner");
    let mut bytes = serde_json::to_string_pretty(&manifest)?;
    bytes.push('\n');
    std::fs::write(&manifest_dest, bytes)?;
    let error = validation_error(cutline::validate_manifest_file(temp_root));
    assert!(
        error.contains("schema violation"),
        "expected a schema violation for the removed `owner` field, got:\n{error}"
    );
    Ok(())
}

#[test]
fn rejects_positive_coverage_tag_on_excluded_row() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-EXC-001").ok_or("missing case")?;
    case["coverage"] = serde_json::json!(["unicode_astral_geometry"]);
    expect_violation(
        &manifest,
        "admitted-denominator tag `unicode_astral_geometry` sits on an excluded row",
    )
}

#[test]
fn rejects_case_listing_unrelated_existing_mutation() -> TestResult {
    // LX-MUT-02 fails only LX-POS-001; listing it on LX-POS-002 must fail
    // even though the mutation exists — global existence is not the pair.
    let mut manifest = canonical_manifest()?;
    let case = case_mut(&mut manifest, "LX-POS-002").ok_or("missing case")?;
    case["mutations"] = serde_json::json!(["LX-MUT-01", "LX-MUT-02", "LX-MUT-07", "LX-MUT-15"]);
    expect_violation(&manifest, "fails_rows does not name this row")
}

#[test]
fn fails_closed_when_the_schema_itself_is_invalid() -> TestResult {
    // An invalid schema must fail the validation command closed, never be
    // silently skipped back to handwritten-only checks.
    let root = repo_root();
    let temp = tempfile::tempdir()?;
    let temp_root = temp.path();
    let schema_dest = temp_root.join(cutline::SCHEMA_PATH);
    let manifest_dest = temp_root.join(cutline::MANIFEST_PATH);
    std::fs::create_dir_all(schema_dest.parent().ok_or("schema parent")?)?;
    std::fs::create_dir_all(manifest_dest.parent().ok_or("manifest parent")?)?;
    std::fs::write(&schema_dest, "{\"type\": \"not-a-real-json-schema-type\"}")?;
    std::fs::copy(root.join(cutline::MANIFEST_PATH), &manifest_dest)?;
    let error = validation_error(cutline::validate_manifest_file(temp_root));
    assert!(error.contains("invalid schema"), "expected an invalid-schema failure, got:\n{error}");
    Ok(())
}
