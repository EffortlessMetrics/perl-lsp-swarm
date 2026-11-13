//! Synthetic emitter-shaped controls, not captures from an executed verifier.
//! Shape and status partition are pinned to upstream Kani 0.68.0 schema_utils.rs.

use super::*;
use serde_json::json;

type TestResult = Result<()>;

fn expected() -> ExpectedRun {
    ExpectedRun {
        target: "x86_64-unknown-linux-gnu".to_owned(),
        tools: [
            ("kani", "0.68.0"),
            ("rustc", "fixture rustc"),
            ("cbmc", "fixture cbmc"),
            ("goto_cc", "fixture goto-cc"),
            ("goto_instrument", "fixture goto-instrument"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
        cbmc_version: "fixture cbmc metadata".to_owned(),
        solver: "cadical".to_owned(),
        object_bits: 16,
        property_categories: [
            "assertion",
            "cover",
            "overflow",
            "unwind",
            "recursion",
            "unsupported_construct",
            "missing_definition",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        harnesses: [(
            "h".to_owned(),
            ExpectedHarness {
                goal: Goal::Positive,
                source: "proof.rs".to_owned(),
                assertions: vec!["FV_ASSERT".to_owned()],
                covers: vec!["FV_COVER".to_owned()],
            },
        )]
        .into_iter()
        .collect(),
    }
}

fn fixture() -> Value {
    json!({
        "metadata": {"version": "1.0", "kani_version": "0.68.0", "target": "x86_64-unknown-linux-gnu"},
        "tools": {"kani": "0.68.0", "rustc": "fixture rustc", "cbmc": "fixture cbmc",
            "goto_cc": "fixture goto-cc", "goto_instrument": "fixture goto-instrument",
            "solvers": [{"name": "cadical", "version": null}]},
        "coverage": {"enabled": false},
        "harness_metadata": [{"pretty_name": "h", "source": {"file": "proof.rs"},
            "attributes": {"kind": "Proof", "should_panic": false},
            "is_automatically_generated": false, "is_ctor_based": false, "is_bounded": false, "has_loop_contracts": false,
            "contract": {"contracted_function_name": null, "recursion_tracker": null}}],
        "verification_results": {
            "summary": {"total_harnesses": 1, "executed": 1, "successful": 1,
                "failed": 0, "duration_ms": 10, "status": "completed"},
            "results": [{"harness_id": "h", "status": "Success", "duration_ms": 10,
                "checks": [
                    {"id": 1, "category": "assertion", "description": "FV_ASSERT", "status": "Success"},
                    {"id": 2, "category": "cover", "description": "FV_COVER", "status": "Satisfied"},
                    {"id": 3, "category": "overflow", "description": "unreachable ordinary check", "status": "Unreachable"}
                ]}]
        },
        "property_details": [{"harness_id": "h", "property_details": {
            "total_properties": 3, "passed": 1, "failed": 0, "unreachable": 1,
            "undetermined": 0, "solver_error": 0, "satisfied": 1, "unsatisfiable": 0,
            "covered": 0, "uncovered": 0}}],
        "error_details": [{"harness_id": "h", "has_errors": false}],
        "cbmc": [{"harness_id": "h", "cbmc_metadata": {"version": "fixture cbmc metadata"},
            "configuration": {"solver": "cadical", "object_bits": 16}}]
    })
}

fn inspect_value(value: &Value) -> Result<Inspection> {
    inspect(&serde_json::to_vec(value)?, &expected())
}

fn rejects<T>(result: Result<T>, fragment: &str) -> TestResult {
    match result {
        Ok(_) => bail!("incorrectly accepted negative control: {fragment}"),
        Err(error) => {
            ensure!(
                format!("{error:#}").contains(fragment),
                "wrong refusal: {error:#}; expected {fragment}"
            );
            Ok(())
        }
    }
}

#[test]
fn complete_positive_report_is_not_a_proof_receipt() -> TestResult {
    let observation = inspect_value(&fixture())?;
    assert_eq!(observation.kind, ObservationKind::PositiveReport);
    assert!(!observation.proof_accepted);
    assert_eq!(observation.evidence_class, "untrusted_artifact_inspection");
    assert_eq!(observation.harnesses["h"].satisfied_covers, 1);
    assert_eq!(observation.harnesses["h"].required_assertions, 1);
    assert_eq!(observation.input_sha256.len(), 64);
    Ok(())
}

#[test]
fn duplicate_json_keys_cannot_overwrite_native_failure() -> TestResult {
    for bytes in [
        br#"{"status":"Failure","status":"Success"}"#.as_slice(),
        br#"{"nested":{"failed":1,"failed":0}}"#.as_slice(),
        br#"{"a":[{"x":1,"x":2}]}"#.as_slice(),
    ] {
        rejects(unique_json::parse(bytes, MAX_INPUT_BYTES), "duplicate JSON key")?;
    }
    Ok(())
}

#[test]
fn malformed_empty_oversized_and_trailing_json_are_rejected() -> TestResult {
    assert!(unique_json::parse(b"", 4).is_err());
    assert!(unique_json::parse(b"{}", 1).is_err());
    assert!(unique_json::parse(b"{", 4).is_err());
    assert!(unique_json::parse(b"{}{}", 8).is_err());
    assert!(inspect_value(&Value::Null).is_err());
    Ok(())
}

#[test]
fn tool_version_target_and_schema_mismatches_are_rejected() -> TestResult {
    for path in [
        "/metadata/version",
        "/metadata/kani_version",
        "/metadata/target",
        "/tools/kani",
        "/tools/rustc",
        "/tools/cbmc",
        "/tools/goto_cc",
        "/tools/goto_instrument",
        "/cbmc/0/cbmc_metadata/version",
        "/cbmc/0/configuration/solver",
    ] {
        let mut value = fixture();
        *value.pointer_mut(path).context("fixture path")? = json!("foreign");
        rejects(inspect_value(&value), "mismatch")?;
    }
    Ok(())
}

#[test]
fn unknown_or_null_used_tool_identity_is_not_accepted() -> TestResult {
    for tool in TOOLS {
        let mut value = fixture();
        value["tools"][tool] = Value::Null;
        rejects(inspect_value(&value), "expected JSON string")?;
    }
    let mut value = fixture();
    value["tools"]["unverified_tool"] = json!("1");
    rejects(inspect_value(&value), "unexpected semantic field")
}

#[test]
fn every_harness_array_rejects_missing_duplicate_and_foreign_rows() -> TestResult {
    for (path, id) in [
        ("/harness_metadata", "pretty_name"),
        ("/verification_results/results", "harness_id"),
        ("/property_details", "harness_id"),
        ("/error_details", "harness_id"),
        ("/cbmc", "harness_id"),
    ] {
        let mut value = fixture();
        *value.pointer_mut(path).context("fixture path")? = json!([]);
        rejects(inspect_value(&value), "denominator differs")?;
        let mut value = fixture();
        let rows = value.pointer_mut(path).and_then(Value::as_array_mut).context("fixture rows")?;
        rows.push(rows[0].clone());
        rejects(inspect_value(&value), "denominator differs")?;
        let mut value = fixture();
        value.pointer_mut(path).context("fixture path")?[0][id] = json!("foreign");
        rejects(inspect_value(&value), "foreign native harness")?;
    }
    Ok(())
}

fn two_harnesses() -> Result<(ExpectedRun, Value)> {
    let mut expected = expected();
    let second = expected.harnesses.get("h").context("h")?.clone();
    expected.harnesses.insert("second".to_owned(), second);
    let mut value = fixture();
    for (path, id) in [
        ("/harness_metadata", "pretty_name"),
        ("/verification_results/results", "harness_id"),
        ("/property_details", "harness_id"),
        ("/error_details", "harness_id"),
        ("/cbmc", "harness_id"),
    ] {
        let rows = value.pointer_mut(path).and_then(Value::as_array_mut).context("fixture rows")?;
        let mut second = rows[0].clone();
        second[id] = json!("second");
        rows.push(second);
    }
    value["verification_results"]["summary"] = json!({"total_harnesses": 2, "executed": 2,
        "successful": 2, "failed": 0, "duration_ms": 20, "status": "completed"});
    Ok((expected, value))
}

#[test]
fn arrays_join_by_harness_identity_not_position() -> TestResult {
    let (expected, mut value) = two_harnesses()?;
    for key in ["property_details", "error_details", "cbmc"] {
        value[key].as_array_mut().context("fixture rows")?.reverse();
    }
    assert_eq!(inspect(&serde_json::to_vec(&value)?, &expected)?.harnesses.len(), 2);
    value["cbmc"][0]["harness_id"] = json!("h");
    rejects(inspect(&serde_json::to_vec(&value)?, &expected), "duplicate native harness")
}

#[test]
fn summary_counts_cannot_hide_missing_or_failed_work() -> TestResult {
    for key in ["total_harnesses", "executed", "successful", "failed", "duration_ms"] {
        let mut value = fixture();
        value["verification_results"]["summary"][key] = json!(99);
        assert!(inspect_value(&value).is_err(), "accepted contradictory {key}");
    }
    let mut value = fixture();
    value["verification_results"]["summary"]["status"] = json!("completed_with_fail_fast");
    rejects(inspect_value(&value), "completion mismatch")
}

#[test]
fn zero_work_and_vacuous_expectations_are_rejected() -> TestResult {
    let value = serde_json::to_vec(&fixture())?;
    let mut expected = expected();
    expected.harnesses.clear();
    rejects(inspect(&value, &expected), "expected harness denominator")?;
    let mut expected = super::tests::expected();
    expected.harnesses.get_mut("h").context("h")?.covers.clear();
    rejects(inspect(&value, &expected), "empty cover denominator")?;
    let mut expected = super::tests::expected();
    expected.harnesses.get_mut("h").context("h")?.assertions.clear();
    rejects(inspect(&value, &expected), "empty assertion denominator")
}

#[test]
fn each_property_count_is_recomputed_including_cover_statuses() -> TestResult {
    for key in COUNTS.into_iter().chain(["total_properties"]) {
        let mut value = fixture();
        value["property_details"][0]["property_details"][key] = json!(99);
        rejects(inspect_value(&value), "contradict")?;
    }
    Ok(())
}

#[test]
fn missing_foreign_and_duplicate_covers_are_rejected() -> TestResult {
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][1]["category"] = json!("assertion");
    value["verification_results"]["results"][0]["checks"][1]["status"] = json!("Success");
    rejects(inspect_value(&value), "cover set incomplete")?;
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][1]["description"] = json!("foreign");
    rejects(inspect_value(&value), "foreign cover")?;
    let mut value = fixture();
    let checks =
        value["verification_results"]["results"][0]["checks"].as_array_mut().context("checks")?;
    checks[2] = checks[1].clone();
    checks[2]["id"] = json!(3);
    rejects(inspect_value(&value), "duplicate cover")
}

#[test]
fn unsatisfied_and_unreachable_covers_defeat_aggregate_success() -> TestResult {
    for (status, counter) in [("Unsatisfiable", "unsatisfiable"), ("Unreachable", "unreachable")] {
        let mut value = fixture();
        value["verification_results"]["results"][0]["checks"][1]["status"] = json!(status);
        value["property_details"][0]["property_details"]["satisfied"] = json!(0);
        value["property_details"][0]["property_details"][counter] =
            json!(if counter == "unreachable" { 2 } else { 1 });
        assert_eq!(inspect_value(&value)?.kind, ObservationKind::NotProven);
    }
    Ok(())
}

#[test]
fn unreachable_required_assertion_is_not_vacuously_positive() -> TestResult {
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][0]["status"] = json!("Unreachable");
    value["property_details"][0]["property_details"]["passed"] = json!(0);
    value["property_details"][0]["property_details"]["unreachable"] = json!(2);
    assert_eq!(inspect_value(&value)?.kind, ObservationKind::NotProven);
    Ok(())
}

#[test]
fn ordinary_unreachable_safety_check_is_preserved_separately() -> TestResult {
    let observation = inspect_value(&fixture())?;
    assert_eq!(observation.kind, ObservationKind::PositiveReport);
    assert_eq!(observation.harnesses["h"].findings[0].status, "Unreachable");
    assert_eq!(observation.harnesses["h"].findings[0].category, "overflow");
    Ok(())
}

#[test]
fn wrong_status_vocabularies_and_cover_classifications_are_rejected() -> TestResult {
    for status in ["PASS", "SUCCESS", "new_status", "Covered", "Uncovered"] {
        let mut value = fixture();
        value["verification_results"]["results"][0]["checks"][0]["status"] = json!(status);
        assert!(inspect_value(&value).is_err());
    }
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][1]["status"] = json!("Success");
    rejects(inspect_value(&value), "invalid status for Kani cover")
}

#[test]
fn should_panic_automatic_and_loop_contract_semantics_are_not_reinterpreted() -> TestResult {
    for path in [
        "/harness_metadata/0/attributes/should_panic",
        "/harness_metadata/0/is_automatically_generated",
        "/harness_metadata/0/has_loop_contracts",
        "/harness_metadata/0/is_ctor_based",
        "/harness_metadata/0/is_bounded",
    ] {
        let mut value = fixture();
        *value.pointer_mut(path).context("path")? = json!(true);
        assert!(inspect_value(&value).is_err());
    }
    Ok(())
}

#[test]
fn native_assertion_failure_is_reported_but_never_admitted() -> TestResult {
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][0]["status"] = json!("Failure");
    value["verification_results"]["results"][0]["status"] = json!("Failure");
    value["verification_results"]["summary"]["successful"] = json!(0);
    value["verification_results"]["summary"]["failed"] = json!(1);
    value["error_details"][0] = json!({"harness_id": "h", "has_errors": true,
        "error_type": "assertion_failure", "failed_properties_type": "PanicsOnly",
        "exit_status": "properties_failed"});
    value["property_details"][0]["property_details"]["passed"] = json!(0);
    value["property_details"][0]["property_details"]["failed"] = json!(1);
    let observation = inspect_value(&value)?;
    assert_eq!(observation.kind, ObservationKind::ReportedCounterexample);
    assert!(!observation.proof_accepted);
    assert!(
        observation.harnesses["h"]
            .findings
            .iter()
            .any(|f| f.description == "FV_ASSERT" && f.status == "Failure")
    );
    Ok(())
}

#[test]
fn undetermined_unknown_and_solver_errors_never_become_positive() -> TestResult {
    for status in ["Undetermined", "Unknown", "Error"] {
        let mut value = fixture();
        value["verification_results"]["results"][0]["checks"][0]["status"] = json!(status);
        value["property_details"][0]["property_details"]["passed"] = json!(0);
        let key = if status == "Error" { "solver_error" } else { "undetermined" };
        value["property_details"][0]["property_details"][key] = json!(1);
        // Kani 0.68 bases aggregate failure on Failure/Error, not Unknown.
        // Reconcile every count so the control fails for meaning, not arithmetic.
        if status == "Error" {
            value["verification_results"]["results"][0]["status"] = json!("Failure");
            value["verification_results"]["summary"]["successful"] = json!(0);
            value["verification_results"]["summary"]["failed"] = json!(1);
            value["error_details"][0] = json!({"harness_id": "h", "has_errors": true,
                "error_type": "property_error", "failed_properties_type": "Error",
                "exit_status": "properties_failed"});
        }
        let report = inspect_value(&value)?;
        assert_eq!(report.kind, ObservationKind::NotProven);
        assert!(!report.proof_accepted);
    }
    Ok(())
}

#[test]
fn error_details_cannot_contradict_the_native_result() -> TestResult {
    let mut value = fixture();
    value["error_details"][0]["has_errors"] = json!(true);
    rejects(inspect_value(&value), "error/result contradiction")?;
    let mut value = fixture();
    value["error_details"][0]["exit_status"] = json!("timeout");
    rejects(inspect_value(&value), "success carries failure details")?;
    error_classes_cannot_disagree_with_observed_properties()
}

#[test]
fn missing_null_fractional_and_negative_property_counts_are_rejected() -> TestResult {
    for replacement in [Value::Null, json!(-1), json!(1.5), json!("1")] {
        let mut value = fixture();
        value["property_details"][0]["property_details"]["passed"] = replacement;
        rejects(inspect_value(&value), "expected nonnegative integer")?;
    }
    Ok(())
}

#[test]
fn duplicate_out_of_range_and_zero_check_ids_are_rejected() -> TestResult {
    for id in [0, 1, 99] {
        let mut value = fixture();
        value["verification_results"]["results"][0]["checks"][1]["id"] = json!(id);
        rejects(inspect_value(&value), "duplicate/invalid native check ID")?;
    }
    Ok(())
}

#[test]
fn assertion_identity_cannot_be_missing_duplicated_or_reclassified() -> TestResult {
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][0]["description"] =
        json!("not the assertion");
    rejects(inspect_value(&value), "assertion set incomplete")?;
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][0]["category"] = json!("overflow");
    rejects(inspect_value(&value), "assertion category mismatch")?;
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][2] =
        json!({"id": 3, "category": "assertion", "description": "FV_ASSERT", "status": "Success"});
    rejects(inspect_value(&value), "duplicate required assertion")
}

#[test]
fn source_identity_requires_the_exact_emitted_path() -> TestResult {
    let mut value = fixture();
    value["harness_metadata"][0]["source"]["file"] = json!("elsewhere/proof.rs");
    rejects(inspect_value(&value), "harness source mismatch")
}

#[test]
fn coverage_external_solver_and_unknown_semantic_fields_fail_closed() -> TestResult {
    let mut value = fixture();
    value["coverage"]["enabled"] = json!(true);
    rejects(inspect_value(&value), "coverage mode unsupported")?;
    let mut expected = expected();
    expected.solver = "z3".to_owned();
    let mut value = fixture();
    value["tools"]["solvers"][0]["name"] = json!("z3");
    rejects(inspect(&serde_json::to_vec(&value)?, &expected), "external solver unsupported")?;
    let mut value = fixture();
    value["verification_results"]["summary"]["incomplete"] = json!(true);
    rejects(inspect_value(&value), "unexpected semantic field")
}

#[test]
fn expectation_files_have_closed_schema_and_no_duplicate_keys() -> TestResult {
    let bytes = serde_json::to_vec(&expected())?;
    assert_eq!(parse_expected(&bytes)?.harnesses.len(), 1);
    let mut value = serde_json::to_value(expected())?;
    value["pass"] = json!(true);
    rejects(parse_expected(&serde_json::to_vec(&value)?), "unknown field")?;
    rejects(parse_expected(br#"{"target":"x","target":"y"}"#), "duplicate JSON key")
}

#[test]
fn unknown_metadata_changes_the_retained_raw_identity() -> TestResult {
    let original = inspect_value(&fixture())?;
    let mut value = fixture();
    value["metadata"]["display_note"] = json!("preserved in original raw artifact");
    let changed = inspect_value(&value)?;
    assert_ne!(original.input_sha256, changed.input_sha256);
    assert_eq!(changed.kind, ObservationKind::PositiveReport);
    assert!(!changed.proof_accepted);
    Ok(())
}

fn negative_fixture() -> Result<(ExpectedRun, Value)> {
    let mut expected = expected();
    let harness = expected.harnesses.get_mut("h").context("h")?;
    harness.goal = Goal::Counterexample { assertion: "FV_ASSERT".to_owned() };
    harness.covers.clear();
    let mut value = fixture();
    let result = &mut value["verification_results"]["results"][0];
    result["status"] = json!("Failure");
    result["checks"] = json!([
        {"id": 1, "category": "assertion", "description": "FV_ASSERT", "status": "Failure"}
    ]);
    value["verification_results"]["summary"]["successful"] = json!(0);
    value["verification_results"]["summary"]["failed"] = json!(1);
    value["error_details"][0] = json!({"harness_id": "h", "has_errors": true,
        "error_type": "assertion_failure", "failed_properties_type": "PanicsOnly",
        "exit_status": "properties_failed"});
    value["property_details"][0]["property_details"] = json!({
        "total_properties": 1, "passed": 0, "failed": 1, "unreachable": 0,
        "undetermined": 0, "solver_error": 0, "satisfied": 0, "unsatisfiable": 0,
        "covered": 0, "uncovered": 0
    });
    Ok((expected, value))
}

#[test]
fn no_cover_counterexample_control_requires_its_named_assertion() -> TestResult {
    let (expected, value) = negative_fixture()?;
    let report = inspect(&serde_json::to_vec(&value)?, &expected)?;
    assert_eq!(report.kind, ObservationKind::ReportedCounterexample);
    assert!(report.harnesses["h"].expected_failure_observed);
    assert!(!report.proof_accepted);
    Ok(())
}

#[test]
fn unrelated_failure_does_not_satisfy_a_negative_control() -> TestResult {
    let (mut expected, value) = negative_fixture()?;
    let harness = expected.harnesses.get_mut("h").context("h")?;
    harness.goal = Goal::Counterexample { assertion: "FV_OTHER".to_owned() };
    harness.assertions.push("FV_OTHER".to_owned());
    let mut value = value;
    let check =
        json!({"id": 2, "category": "assertion", "description": "FV_OTHER", "status": "Success"});
    value["verification_results"]["results"][0]["checks"]
        .as_array_mut()
        .context("checks")?
        .push(check);
    value["property_details"][0]["property_details"]["total_properties"] = json!(2);
    value["property_details"][0]["property_details"]["passed"] = json!(1);
    let report = inspect(&serde_json::to_vec(&value)?, &expected)?;
    assert_eq!(report.kind, ObservationKind::NotProven);
    assert!(!report.harnesses["h"].expected_failure_observed);
    Ok(())
}

#[test]
fn timeout_or_crash_is_not_a_successful_negative_control() -> TestResult {
    let (expected, value) = negative_fixture()?;
    for exit in ["timeout", "out_of_memory", "exit_code_137"] {
        let mut value = value.clone();
        value["error_details"][0]["exit_status"] = json!(exit);
        rejects(inspect(&serde_json::to_vec(&value)?, &expected), "termination mismatch")?;
    }
    Ok(())
}

#[test]
fn inspection_binds_the_expectation_not_just_the_supplied_export() -> TestResult {
    let value = serde_json::to_vec(&fixture())?;
    let expected = expected();
    let report = inspect(&value, &expected)?;
    assert_eq!(
        report.expectation_sha256,
        format!("{:x}", Sha256::digest(serde_json::to_vec(&expected)?))
    );
    Ok(())
}

#[test]
fn unwind_and_unsupported_failures_are_not_production_counterexamples() -> TestResult {
    for category in ["unwind", "recursion", "unsupported_construct", "missing_definition"] {
        let mut value = fixture();
        let check = &mut value["verification_results"]["results"][0]["checks"][2];
        check["category"] = json!(category);
        check["status"] = json!("Failure");
        value["verification_results"]["results"][0]["status"] = json!("Failure");
        value["verification_results"]["summary"]["successful"] = json!(0);
        value["verification_results"]["summary"]["failed"] = json!(1);
        value["error_details"][0] = json!({
            "harness_id": "h", "has_errors": true,
            "error_type": "verification_failure", "failed_properties_type": "Other",
            "exit_status": "properties_failed"
        });
        value["property_details"][0]["property_details"]["unreachable"] = json!(0);
        value["property_details"][0]["property_details"]["failed"] = json!(1);
        let report = inspect_value(&value)?;
        assert_eq!(report.kind, ObservationKind::NotProven, "{category}");
        assert!(!report.proof_accepted);
    }
    Ok(())
}

#[test]
fn a_new_property_category_requires_explicit_profile_review() -> TestResult {
    let mut value = fixture();
    value["verification_results"]["results"][0]["checks"][2]["category"] = json!("new_safety_class");
    rejects(inspect_value(&value), "unrecognized property category")
}

#[test]
fn a_negative_control_cannot_hide_an_unsatisfied_required_cover() -> TestResult {
    let (mut expected, mut value) = negative_fixture()?;
    expected.harnesses.get_mut("h").context("h")?.covers.push("FV_COVER".to_owned());
    let check =
        json!({"id": 2, "category": "cover", "description": "FV_COVER", "status": "Unsatisfiable"});
    value["verification_results"]["results"][0]["checks"]
        .as_array_mut()
        .context("checks")?
        .push(check);
    value["property_details"][0]["property_details"]["total_properties"] = json!(2);
    value["property_details"][0]["property_details"]["unsatisfiable"] = json!(1);
    let report = inspect(&serde_json::to_vec(&value)?, &expected)?;
    assert!(report.harnesses["h"].expected_failure_observed);
    assert_eq!(report.kind, ObservationKind::NotProven);
    Ok(())
}

#[test]
fn object_bit_configuration_cannot_be_omitted_or_changed() -> TestResult {
    for replacement in [Value::Null, json!(8), json!("16")] {
        let mut value = fixture();
        value["cbmc"][0]["configuration"]["object_bits"] = replacement;
        assert!(inspect_value(&value).is_err());
    }
    let mut profile = expected();
    profile.object_bits = 0;
    rejects(inspect(&serde_json::to_vec(&fixture())?, &profile), "invalid expected object bits")
}

fn error_classes_cannot_disagree_with_observed_properties() -> TestResult {
    let (expected, value) = negative_fixture()?;
    for (field, replacement) in [
        ("error_type", "property_error"),
        ("failed_properties_type", "Other"),
        ("failed_properties_type", "unknown_future_class"),
    ] {
        let mut changed = value.clone();
        changed["error_details"][0][field] = json!(replacement);
        rejects(inspect(&serde_json::to_vec(&changed)?, &expected), "classification mismatch")?;
    }
    for field in ["error_type", "failed_properties_type"] {
        let mut changed = value.clone();
        changed["error_details"][0].as_object_mut().context("error object")?.remove(field);
        rejects(inspect(&serde_json::to_vec(&changed)?, &expected), "expected JSON string")?;
    }
    Ok(())
}
