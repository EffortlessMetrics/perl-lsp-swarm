//! Contract tests for the standalone diagnostics registry (#11493).
//!
//! The committed registry is the valid fixture. Each negative test corrupts it
//! in memory along one intended falsifier axis and proves the validator rejects
//! it for that reason. The projection tests prove the load-bearing user-facing
//! distinctions: free text never selects a reason, PATH persistence is never
//! rendered as fresh-process visibility, a preserved current is never reported
//! without its known-good consequence, and no outcome degrades into generic
//! retry advice.

use serde_json::{Value, json};
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::standalone_diagnostics::{
    self as diagnostics, Combination, MANIFEST_PATH, StandaloneDiagnosticsError, ValidationStats,
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

fn validation_error(result: Result<ValidationStats, StandaloneDiagnosticsError>) -> String {
    match result {
        Ok(_) => "registry unexpectedly validated".to_string(),
        Err(error) => error.to_string(),
    }
}

fn expect_violation(manifest: &Value, needle: &str) -> TestResult {
    let error = validation_error(diagnostics::validate_manifest_value(manifest));
    assert!(error.contains(needle), "expected violation containing `{needle}`, got:\n{error}");
    Ok(())
}

fn reasons_mut<'a>(manifest: &'a mut Value, key: &str) -> Option<&'a mut Vec<Value>> {
    manifest.get_mut(key)?.as_array_mut()
}

fn reason_mut<'a>(manifest: &'a mut Value, key: &str, reason_id: &str) -> Option<&'a mut Value> {
    reasons_mut(manifest, key)?
        .iter_mut()
        .find(|reason| reason.get("reason_id").and_then(Value::as_str) == Some(reason_id))
}

fn position_of(manifest: &Value, key: &str, reason_id: &str) -> Option<usize> {
    manifest
        .get(key)?
        .as_array()?
        .iter()
        .position(|reason| reason.get("reason_id").and_then(Value::as_str) == Some(reason_id))
}

fn combination(
    operation: &str,
    disposition: &str,
    product_units: &str,
    cleanup: &str,
    process_startup: &str,
    path_persistence: &str,
) -> Combination {
    Combination {
        operation: operation.to_string(),
        disposition: disposition.to_string(),
        product_units: product_units.to_string(),
        cleanup: cleanup.to_string(),
        process_startup: process_startup.to_string(),
        path_persistence: path_persistence.to_string(),
    }
}

fn packet(disposition: &str, dimensions: Value, bounded_reason: &str) -> Value {
    json!({
        "schema_version": "standalone_install_transition.v1",
        "route_mode": "first_party_posix",
        "operation": "install",
        "transaction_id": "tx-1",
        "attempt_id": "att-1",
        "disposition": disposition,
        "candidate_id": null,
        "prior_current_candidate_id": null,
        "outcome_dimensions": dimensions,
        "bounded_reason": bounded_reason
    })
}

fn action_ids(projection: &Value) -> Vec<String> {
    projection
        .get("allowed_actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| action.get("action_id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Positive control
// ---------------------------------------------------------------------------

#[test]
fn committed_registry_validates() -> TestResult {
    let stats = diagnostics::validate_manifest_file(&repo_root())?;
    assert_eq!(stats.actions, 11);
    assert_eq!(stats.summary_templates, 18);
    assert_eq!(stats.primary_reasons, 29);
    assert_eq!(stats.additional_reasons, 4);
    assert_eq!(stats.deferred_reason_domains, 7);
    // The closed cross-product of the typed transition contract.
    assert_eq!(stats.combinations, 4 * 8 * 7 * 5 * 4 * 4);
    assert_eq!(stats.combinations, 17_920);
    Ok(())
}

#[test]
fn every_typed_combination_projects() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for combination in diagnostics::all_combinations() {
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        assert!(
            projection.get("primary_reason").and_then(Value::as_str).is_some(),
            "no primary reason for {combination:?}"
        );
        assert!(!action_ids(&projection).is_empty(), "no action offered for {combination:?}");
        assert!(
            projection.get("claim_ceiling").and_then(Value::as_str).is_some(),
            "no claim ceiling for {combination:?}"
        );
    }
    Ok(())
}

#[test]
fn list_and_explain_cover_the_registry() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let ids = diagnostics::list_reason_ids(&manifest);
    assert_eq!(ids.len(), 33);
    assert!(ids.iter().any(|id| id == "sel_committed_path_persisted_new_session_required"));
    let explained = diagnostics::explain_reason(&manifest, "cleanup_incomplete_residue_retained")
        .ok_or("missing reason from explain")?;
    assert!(explained.contains("\"reason_role\": \"additional_reasons\""));
    assert!(explained.contains("manual_owned_state_resolution"));
    assert!(diagnostics::explain_reason(&manifest, "no_such_reason").is_none());
    Ok(())
}

// ---------------------------------------------------------------------------
// Totality and reachability
// ---------------------------------------------------------------------------

#[test]
fn rejects_registry_gap_when_a_reason_is_removed() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let index = position_of(&manifest, "primary_reasons", "rollback_committed_current_restored")
        .ok_or("missing reason")?;
    reasons_mut(&mut manifest, "primary_reasons").ok_or("missing primary reasons")?.remove(index);
    expect_violation(&manifest, "registry gap")
}

#[test]
fn rejects_a_shadowed_reason() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let broad = position_of(&manifest, "primary_reasons", "rollback_committed_current_restored")
        .ok_or("missing reason")?;
    let narrow =
        position_of(&manifest, "primary_reasons", "rollback_committed_path_persistence_failed")
            .ok_or("missing reason")?;
    let reasons = reasons_mut(&mut manifest, "primary_reasons").ok_or("missing primary reasons")?;
    let broad_row = reasons.remove(broad);
    reasons.insert(narrow, broad_row);
    expect_violation(&manifest, "is shadowed and cannot be reached")
}

#[test]
fn rejects_an_unconditional_catch_all_selector() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason =
        reason_mut(&mut manifest, "primary_reasons", "rollback_committed_current_restored")
            .ok_or("missing reason")?;
    reason["selector"] = json!({});
    expect_violation(&manifest, "empty selector")
}

#[test]
fn rejects_a_selector_that_restates_a_whole_domain() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(&mut manifest, "primary_reasons", "failed_preserved_current")
        .ok_or("missing reason")?;
    reason["selector"]["operation"] = json!(["install", "repair", "update", "rollback"]);
    expect_violation(&manifest, "lists the whole domain")
}

// ---------------------------------------------------------------------------
// Typed results, never tool prose, select a reason
// ---------------------------------------------------------------------------

#[test]
fn rejects_a_selector_on_free_text() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(&mut manifest, "primary_reasons", "failed_preserved_current")
        .ok_or("missing reason")?;
    reason["selector"]["bounded_reason"] = json!(["network timeout"]);
    expect_violation(&manifest, "which is not a typed selector field")
}

#[test]
fn rejects_a_selector_on_per_attempt_identity() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(&mut manifest, "primary_reasons", "failed_preserved_current")
        .ok_or("missing reason")?;
    reason["selector"]["transaction_id"] = json!(["tx-1"]);
    expect_violation(&manifest, "which is not a typed selector field")
}

#[test]
fn rejects_a_selector_value_outside_the_typed_domain() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(&mut manifest, "primary_reasons", "failed_preserved_current")
        .ok_or("missing reason")?;
    reason["selector"]["disposition"] = json!(["checksum_mismatch"]);
    expect_violation(&manifest, "outside the typed domain")
}

#[test]
fn rejects_an_input_contract_that_permits_free_text_selection() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["input_contract"]["forbidden_selector_fields"] = json!(["candidate_id"]);
    expect_violation(&manifest, "must forbid `bounded_reason`")
}

#[test]
fn rejects_a_template_that_renders_free_text() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let templates = manifest
        .get_mut("summary_templates")
        .and_then(Value::as_array_mut)
        .ok_or("missing templates")?;
    let template = templates
        .iter_mut()
        .find(|template| {
            template.get("template_id").and_then(Value::as_str) == Some("t_startup_failed")
        })
        .ok_or("missing template")?;
    template["text"] = json!("Install failed: {bounded_reason}");
    expect_violation(&manifest, "renders forbidden parameter `bounded_reason`")
}

#[test]
fn rejects_an_allowed_render_parameter_that_is_not_typed() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["render"]["allowed_parameters"]
        .as_array_mut()
        .ok_or("missing allowed parameters")?
        .push(json!("install_root"));
    expect_violation(&manifest, "is both allowed and forbidden")
}

// ---------------------------------------------------------------------------
// Claim honesty
// ---------------------------------------------------------------------------

#[test]
fn rejects_an_unproven_claim_without_a_retained_limitation() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(
        &mut manifest,
        "primary_reasons",
        "sel_committed_path_persisted_new_session_required",
    )
    .ok_or("missing reason")?;
    reason["required_limitations"] = json!([]);
    expect_violation(&manifest, "retains no limitation")
}

#[test]
fn rejects_an_unproven_claim_that_asks_for_nothing() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(
        &mut manifest,
        "primary_reasons",
        "sel_committed_path_persisted_new_session_required",
    )
    .ok_or("missing reason")?;
    reason["action_ids"] = json!(["no_action_required"]);
    expect_violation(&manifest, "may not render as nothing to do")
}

#[test]
fn rejects_no_action_required_combined_with_a_real_action() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(
        &mut manifest,
        "primary_reasons",
        "sel_committed_path_persisted_startup_verified",
    )
    .ok_or("missing reason")?;
    reason["action_ids"] = json!(["no_action_required", "inspect_exact_receipt"]);
    expect_violation(&manifest, "combines `no_action_required`")
}

#[test]
fn rejects_a_deferred_domain_that_names_this_issue_as_owner() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let domain = manifest
        .get_mut("deferred_reason_domains")
        .and_then(Value::as_array_mut)
        .and_then(|domains| domains.first_mut())
        .ok_or("missing deferred domain")?;
    domain["owner_issue"] = json!("#11493");
    expect_violation(&manifest, "must transfer to the issue that types its stage result")
}

// ---------------------------------------------------------------------------
// Action authority
// ---------------------------------------------------------------------------

#[test]
fn rejects_an_action_that_authorizes_external_mutation() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let action = manifest
        .get_mut("actions")
        .and_then(Value::as_array_mut)
        .and_then(|actions| {
            actions.iter_mut().find(|action| {
                action.get("action_id").and_then(Value::as_str) == Some("run_explicit_repair")
            })
        })
        .ok_or("missing action")?;
    action["external"] = json!(true);
    expect_violation(&manifest, "may not authorize release, publication, or upstream mutation")
}

#[test]
fn rejects_an_action_that_requires_elevation() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let action = manifest
        .get_mut("actions")
        .and_then(Value::as_array_mut)
        .and_then(|actions| {
            actions.iter_mut().find(|action| {
                action.get("action_id").and_then(Value::as_str) == Some("run_explicit_repair")
            })
        })
        .ok_or("missing action")?;
    action["elevated"] = json!(true);
    expect_violation(&manifest, "privileged operations are not owned by this registry")
}

#[test]
fn rejects_an_unreferenced_action() -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest.get_mut("actions").and_then(Value::as_array_mut).ok_or("missing actions")?.push(
        json!({
            "action_id": "disable_tls_verification",
            "action_kind": "guidance",
            "applicability": "never",
            "destructive": false,
            "external": false,
            "elevated": false,
            "manual": false,
            "platform_scope": "any",
            "forbidden_substitutions": [],
            "public_rendering_ceiling": "public_safe"
        }),
    );
    expect_violation(&manifest, "is never referenced by a reason")
}

#[test]
fn rejects_a_reason_naming_an_unknown_action() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(&mut manifest, "primary_reasons", "failed_preserved_current")
        .ok_or("missing reason")?;
    reason["action_ids"] = json!(["reinstall_everything"]);
    expect_violation(&manifest, "names unknown action `reinstall_everything`")
}

#[test]
fn rejects_a_duplicate_reason_id() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let reason = reason_mut(&mut manifest, "primary_reasons", "failed_preserved_current")
        .ok_or("missing reason")?;
    reason["reason_id"] = json!("cancelled_preserved_current");
    expect_violation(&manifest, "duplicate reason id")
}

// ---------------------------------------------------------------------------
// Projection behaviour
// ---------------------------------------------------------------------------

#[test]
fn free_text_never_changes_the_projection() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let dimensions = json!({
        "product_units": "installed",
        "cleanup": "completed",
        "process_startup": "unproven",
        "path_persistence": "persisted"
    });
    let honest = packet("selection_committed", dimensions.clone(), "installed cleanly");
    let misleading = packet(
        "selection_committed",
        dimensions,
        "curl: (60) SSL certificate problem: unable to get local issuer certificate",
    );
    let left = diagnostics::project_packet(&manifest, &honest)?;
    let right = diagnostics::project_packet(&manifest, &misleading)?;
    assert_eq!(left, right, "bounded_reason must not influence the projection");
    assert_eq!(
        left.get("primary_reason").and_then(Value::as_str),
        Some("sel_committed_path_persisted_new_session_required")
    );
    Ok(())
}

#[test]
fn persisted_path_is_never_rendered_as_fresh_process_visibility() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for startup in ["unproven", "not_applicable"] {
        let projection = diagnostics::project_packet(
            &manifest,
            &packet(
                "selection_committed",
                json!({
                    "product_units": "installed",
                    "cleanup": "completed",
                    "process_startup": startup,
                    "path_persistence": "persisted"
                }),
                "persisted",
            ),
        )?;
        assert_eq!(
            projection.get("claim_ceiling").and_then(Value::as_str),
            Some("command_available_after_new_session"),
            "PATH persistence must not claim immediate availability for startup `{startup}`"
        );
        assert_eq!(
            projection.pointer("/consequences/path").and_then(Value::as_str),
            Some("persisted_new_session_required")
        );
        assert!(action_ids(&projection).iter().any(|id| id == "start_documented_new_session"));
    }

    // Only an observed fresh process may claim immediate availability.
    let verified = diagnostics::project_packet(
        &manifest,
        &packet(
            "selection_committed",
            json!({
                "product_units": "installed",
                "cleanup": "completed",
                "process_startup": "verified",
                "path_persistence": "persisted"
            }),
            "persisted",
        ),
    )?;
    assert_eq!(
        verified.get("claim_ceiling").and_then(Value::as_str),
        Some("command_available_now")
    );
    Ok(())
}

#[test]
fn a_failed_attempt_always_reports_its_known_good_consequence() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for disposition in
        ["failed_preserved_current", "cancelled_preserved_current", "not_proven_preserved_current"]
    {
        for startup in ["verified", "unproven", "failed", "not_applicable"] {
            let projection = diagnostics::project_packet(
                &manifest,
                &packet(
                    disposition,
                    json!({
                        "product_units": "preserved_prior",
                        "cleanup": "completed",
                        "process_startup": startup,
                        "path_persistence": "unchanged"
                    }),
                    "attempt did not complete",
                ),
            )?;
            let known_good = projection
                .pointer("/consequences/known_good")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let rollback = projection
                .pointer("/consequences/rollback")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert_eq!(rollback, "not_required_current_preserved");
            if startup == "failed" {
                assert_eq!(
                    known_good, "retained_but_startup_unproven",
                    "a retained installation that will not start must not be reported as healthy"
                );
            } else {
                assert_eq!(known_good, "retained");
            }
        }
    }
    Ok(())
}

#[test]
fn an_unproven_outcome_never_degrades_into_a_bare_retry() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for combination in diagnostics::all_combinations() {
        if combination.disposition != "not_proven_preserved_current" {
            continue;
        }
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        let actions = action_ids(&projection);
        assert!(
            !actions.iter().any(|id| id == "retry_same_exact_subject"),
            "an unproven result must not be offered as a plain retry: {combination:?}"
        );
        assert!(
            actions.iter().any(|id| id == "report_instrument_failure"),
            "an unproven result must report instrument failure: {combination:?}"
        );
        assert_eq!(
            projection.get("classification").and_then(Value::as_str),
            Some("instrument"),
            "an unproven result must stay distinct from a product failure: {combination:?}"
        );
    }
    Ok(())
}

#[test]
fn residual_state_is_never_resolved_by_a_recursive_delete() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projection = diagnostics::project_packet(
        &manifest,
        &packet(
            "failed_preserved_current",
            json!({
                "product_units": "preserved_prior",
                "cleanup": "failed_preserved",
                "process_startup": "verified",
                "path_persistence": "unchanged"
            }),
            "cleanup failed",
        ),
    )?;
    assert!(
        projection.get("additional_reasons").and_then(Value::as_array).is_some_and(|reasons| {
            reasons
                .iter()
                .any(|reason| reason.as_str() == Some("cleanup_incomplete_residue_retained"))
        }),
        "incomplete cleanup must surface as an additional reason"
    );
    let forbidden = projection
        .get("allowed_actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter(|action| {
                    action.get("action_id").and_then(Value::as_str)
                        == Some("manual_owned_state_resolution")
                })
                .filter_map(|action| action.get("forbidden_substitutions"))
                .filter_map(Value::as_array)
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    assert!(forbidden.iter().any(|item| item == "recursive_delete_unknown_state"));
    assert!(forbidden.iter().any(|item| item == "delete_foreign_installation"));
    Ok(())
}

#[test]
fn a_self_contradictory_packet_is_an_instrument_failure_not_a_product_claim() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projection = diagnostics::project_packet(
        &manifest,
        &packet(
            "failed_preserved_current",
            json!({
                "product_units": "installed",
                "cleanup": "completed",
                "process_startup": "verified",
                "path_persistence": "unchanged"
            }),
            "failed but somehow installed",
        ),
    )?;
    assert_eq!(
        projection.get("primary_reason").and_then(Value::as_str),
        Some("inv_preserved_current_with_product_mutation")
    );
    assert_eq!(
        projection.get("claim_ceiling").and_then(Value::as_str),
        Some("support_claim_withheld")
    );
    assert!(action_ids(&projection).iter().any(|id| id == "report_instrument_failure"));
    Ok(())
}

#[test]
fn projection_is_deterministic() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let subject = packet(
        "selection_unchanged",
        json!({
            "product_units": "unchanged",
            "cleanup": "deferred",
            "process_startup": "unproven",
            "path_persistence": "failed"
        }),
        "no change",
    );
    let first = serde_json::to_string(&diagnostics::project_packet(&manifest, &subject)?)?;
    let second = serde_json::to_string(&diagnostics::project_packet(&manifest, &subject)?)?;
    assert_eq!(first, second);
    Ok(())
}

// ---------------------------------------------------------------------------
// Packet admission
// ---------------------------------------------------------------------------

#[test]
fn rejects_a_packet_with_an_untyped_outcome() -> TestResult {
    let subject = packet(
        "selection_committed",
        json!({
            "product_units": "installed",
            "cleanup": "completed",
            "process_startup": "probably_fine",
            "path_persistence": "persisted"
        }),
        "reason",
    );
    let error = match diagnostics::combination_from_packet(&subject) {
        Ok(_) => "packet unexpectedly admitted".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("outside the typed domain"), "got: {error}");
    Ok(())
}

#[test]
fn rejects_a_packet_from_another_contract() -> TestResult {
    let mut subject = packet(
        "selection_committed",
        json!({
            "product_units": "installed",
            "cleanup": "completed",
            "process_startup": "verified",
            "path_persistence": "persisted"
        }),
        "reason",
    );
    subject["schema_version"] = json!("install_transition.v1");
    let error = match diagnostics::combination_from_packet(&subject) {
        Ok(_) => "packet unexpectedly admitted".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("schema_version must be"), "got: {error}");
    Ok(())
}

#[test]
fn a_missing_reason_field_is_an_admission_failure() -> TestResult {
    let mut subject = packet(
        "selection_committed",
        json!({
            "product_units": "installed",
            "cleanup": "completed",
            "process_startup": "verified",
            "path_persistence": "persisted"
        }),
        "reason",
    );
    subject.as_object_mut().ok_or("packet is not an object")?.remove("bounded_reason");
    let error = match diagnostics::combination_from_packet(&subject) {
        Ok(_) => "packet unexpectedly admitted".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("missing `bounded_reason`"), "got: {error}");
    assert_eq!(error.matches("bounded_reason").count(), 1, "got: {error}");
    Ok(())
}

#[test]
fn an_unmapped_combination_is_a_registry_gap_not_generic_advice() -> TestResult {
    let mut manifest = canonical_manifest()?;
    let index = position_of(&manifest, "primary_reasons", "rollback_committed_current_restored")
        .ok_or("missing reason")?;
    reasons_mut(&mut manifest, "primary_reasons").ok_or("missing primary reasons")?.remove(index);
    let subject = combination(
        "rollback",
        "rollback_committed",
        "rolled_back",
        "completed",
        "unproven",
        "unchanged",
    );
    let error = match diagnostics::project_combination(&manifest, &subject, None) {
        Ok(_) => "projection unexpectedly succeeded".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("registry gap"), "got: {error}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Drift between the registry and the typed input contract
// ---------------------------------------------------------------------------

/// Build a throwaway repository root holding only the three files
/// `validate_manifest_file` reads, so the input contract can be mutated without
/// touching the working tree.
fn staged_root(label: &str) -> TestResult<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "standalone-diagnostics-{}-{}-{label}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    for relative in [MANIFEST_PATH, diagnostics::SCHEMA_PATH, diagnostics::INPUT_SCHEMA_PATH] {
        let destination = root.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(repo_root().join(relative), destination)?;
    }
    Ok(root)
}

#[test]
fn the_staged_copy_of_the_registry_still_validates() -> TestResult {
    let root = staged_root("control")?;
    let result = diagnostics::validate_manifest_file(&root);
    std::fs::remove_dir_all(&root).ok();
    result?;
    Ok(())
}

#[test]
fn rejects_a_widened_input_contract_the_registry_does_not_cover() -> TestResult {
    let root = staged_root("widened")?;
    let schema_path = root.join(diagnostics::INPUT_SCHEMA_PATH);
    let mut schema: Value = serde_json::from_slice(&std::fs::read(&schema_path)?)?;
    // A new installer outcome nobody has mapped to a user consequence yet.
    schema["properties"]["disposition"]["enum"]
        .as_array_mut()
        .ok_or("disposition enum missing")?
        .push(json!("provenance_rejected_preserved_current"));
    std::fs::write(&schema_path, format!("{}\n", serde_json::to_string_pretty(&schema)?))?;

    let error = validation_error(diagnostics::validate_manifest_file(&root));
    std::fs::remove_dir_all(&root).ok();
    assert!(
        error.contains("`disposition` domain drifted"),
        "a widened installer contract must fail the registry, got:\n{error}"
    );
    Ok(())
}

#[test]
fn rejects_a_non_canonical_registry_file() -> TestResult {
    let root = staged_root("noncanonical")?;
    let manifest_path = root.join(MANIFEST_PATH);
    let text = std::fs::read_to_string(&manifest_path)?;
    std::fs::write(&manifest_path, text.replace("\n  \"actions\"", "\n    \"actions\""))?;

    let error = validation_error(diagnostics::validate_manifest_file(&root));
    std::fs::remove_dir_all(&root).ok();
    assert!(error.contains("is not canonical"), "got:\n{error}");
    Ok(())
}

// ---------------------------------------------------------------------------
// The registry schema is the structural authority (#13800 review)
// ---------------------------------------------------------------------------

#[test]
fn rejects_a_manifest_that_violates_the_registry_schema() -> TestResult {
    let root = staged_root("schema-invalid")?;
    let manifest_path = root.join(MANIFEST_PATH);
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    // `applicability` is required by the schema but is not one of the semantic
    // rules the handwritten validator enforces, so only applying the schema
    // catches this.
    manifest["actions"]
        .as_array_mut()
        .and_then(|actions| actions.first_mut())
        .and_then(Value::as_object_mut)
        .ok_or("missing action")?
        .remove("applicability");
    std::fs::write(&manifest_path, format!("{}\n", serde_json::to_string_pretty(&manifest)?))?;

    let error = validation_error(diagnostics::validate_manifest_file(&root));
    std::fs::remove_dir_all(&root).ok();
    assert!(error.contains("registry schema violation"), "got:\n{error}");
    Ok(())
}

#[test]
fn rejects_a_reason_row_with_an_unknown_field() -> TestResult {
    let root = staged_root("schema-unknown-key")?;
    let manifest_path = root.join(MANIFEST_PATH);
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    manifest["primary_reasons"]
        .as_array_mut()
        .and_then(|reasons| reasons.first_mut())
        .ok_or("missing reason")?["severity"] = json!("critical");
    std::fs::write(&manifest_path, format!("{}\n", serde_json::to_string_pretty(&manifest)?))?;

    let error = validation_error(diagnostics::validate_manifest_file(&root));
    std::fs::remove_dir_all(&root).ok();
    assert!(error.contains("registry schema violation"), "got:\n{error}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Packet admission is total over the input contract (#13800 review)
// ---------------------------------------------------------------------------

fn admission_error(subject: &Value) -> String {
    match diagnostics::read_packet(subject) {
        Ok(_) => "packet unexpectedly admitted".to_string(),
        Err(error) => error.to_string(),
    }
}

fn valid_packet() -> Value {
    packet(
        "selection_committed",
        json!({
            "product_units": "installed",
            "cleanup": "completed",
            "process_startup": "verified",
            "path_persistence": "persisted"
        }),
        "installed cleanly",
    )
}

#[test]
fn rejects_a_packet_with_an_arbitrary_route_mode() -> TestResult {
    let mut subject = valid_packet();
    subject["route_mode"] = json!("../../etc/passwd or any unbounded operator text");
    let error = admission_error(&subject);
    assert!(error.contains("`route_mode` value"), "got: {error}");
    assert!(error.contains("may not carry arbitrary text"), "got: {error}");
    Ok(())
}

#[test]
fn rejects_a_packet_with_an_unknown_field() -> TestResult {
    let mut subject = valid_packet();
    subject
        .as_object_mut()
        .ok_or("packet is not an object")?
        .insert("severity".to_string(), json!("critical"));
    let error = admission_error(&subject);
    assert!(error.contains("unknown field `severity`"), "got: {error}");
    Ok(())
}

#[test]
fn rejects_a_packet_missing_required_transaction_identity() -> TestResult {
    for key in ["transaction_id", "attempt_id"] {
        let mut subject = valid_packet();
        subject.as_object_mut().ok_or("packet is not an object")?.remove(key);
        let error = admission_error(&subject);
        assert!(error.contains(&format!("missing `{key}`")), "for {key}, got: {error}");
        assert_eq!(error.matches(key).count(), 1, "for {key}, got: {error}");
    }
    Ok(())
}

#[test]
fn rejects_a_packet_whose_route_mode_is_not_a_string() -> TestResult {
    let mut subject = valid_packet();
    subject["route_mode"] = json!(7);
    let error = admission_error(&subject);
    assert!(error.contains("`route_mode` value"), "got: {error}");
    Ok(())
}

#[test]
fn rejects_a_packet_with_a_malformed_candidate_digest() -> TestResult {
    let mut subject = valid_packet();
    subject["candidate_id"] = json!("not-a-sha256");
    let error = admission_error(&subject);
    assert!(error.contains("must be a sha256 digest or null"), "got: {error}");
    Ok(())
}

#[test]
fn an_admitted_packet_renders_only_its_bounded_route_mode() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let admitted = diagnostics::read_packet(&valid_packet())?;
    assert_eq!(admitted.route_mode, "first_party_posix");
    let projection = diagnostics::project_packet(&manifest, &valid_packet())?;
    assert_eq!(projection.get("route_mode").and_then(Value::as_str), Some("first_party_posix"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Packet-consistency invariants must bind the whole contradiction set
// (independent semantic review of #13800)
// ---------------------------------------------------------------------------

/// `xtask/examples/standalone_candidate_selection.rs` is the origin admission
/// authority for this contract: "a committed selection must move product units
/// to installed/repaired/updated". Every other value is a contradiction and must
/// reach the instrument family, never a product success.
#[test]
fn a_committed_selection_without_a_product_effect_is_an_instrument_failure() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for product_units in ["rolled_back", "unchanged", "preserved_prior", "not_applicable"] {
        let projection = diagnostics::project_packet(
            &manifest,
            &packet(
                "selection_committed",
                json!({
                    "product_units": product_units,
                    "cleanup": "completed",
                    // The most flattering possible dimensions: if the registry
                    // is going to overclaim, it will do it here.
                    "process_startup": "verified",
                    "path_persistence": "persisted"
                }),
                "claims a committed selection with no product effect",
            ),
        )?;
        assert_eq!(
            projection.get("primary_reason").and_then(Value::as_str),
            Some("inv_selection_committed_without_product_effect"),
            "product_units `{product_units}` under a committed selection must be a contradiction"
        );
        assert_eq!(
            projection.get("claim_ceiling").and_then(Value::as_str),
            Some("support_claim_withheld"),
            "a contradictory packet must not claim availability for `{product_units}`"
        );
        assert_eq!(projection.get("classification").and_then(Value::as_str), Some("instrument"));
    }

    // Positive control: the three admitted values stay ordinary outcomes.
    for product_units in ["installed", "repaired", "updated"] {
        let projection = diagnostics::project_packet(
            &manifest,
            &packet(
                "selection_committed",
                json!({
                    "product_units": product_units,
                    "cleanup": "completed",
                    "process_startup": "verified",
                    "path_persistence": "persisted"
                }),
                "ordinary install",
            ),
        )?;
        assert_eq!(
            projection.get("primary_reason").and_then(Value::as_str),
            Some("sel_committed_path_persisted_startup_verified"),
            "`{product_units}` is an admitted committed-selection outcome"
        );
    }
    Ok(())
}

/// Negative control for the invariant above: the origin authority deliberately
/// leaves `product_units` unconstrained for `selection_unchanged`, because a
/// repair re-commits the current candidate while its units legitimately change.
/// Forbidding a product effect there would classify a correct installer as
/// broken.
#[test]
fn a_repair_that_keeps_the_current_selection_is_not_a_contradiction() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for product_units in ["repaired", "updated", "installed"] {
        let projection = diagnostics::project_packet(
            &manifest,
            &packet(
                "selection_unchanged",
                json!({
                    "product_units": product_units,
                    "cleanup": "completed",
                    "process_startup": "verified",
                    "path_persistence": "unchanged"
                }),
                "repaired the current installation in place",
            ),
        )?;
        let reason = projection.get("primary_reason").and_then(Value::as_str).unwrap_or_default();
        assert!(
            !reason.starts_with("inv_"),
            "`selection_unchanged` with `{product_units}` is a legitimate repair, got `{reason}`"
        );
    }
    Ok(())
}

#[test]
fn rejects_an_invariant_shadowed_by_an_ordinary_reason() -> TestResult {
    let mut manifest = canonical_manifest()?;
    // Move an ordinary success ahead of the whole invariant family. Every reason
    // still fires somewhere, so first-match reachability alone stays satisfied.
    let index =
        position_of(&manifest, "primary_reasons", "sel_committed_path_persisted_startup_verified")
            .ok_or("missing reason")?;
    let reasons = reasons_mut(&mut manifest, "primary_reasons").ok_or("missing primary reasons")?;
    let row = reasons.remove(index);
    reasons.insert(0, row);
    expect_violation(&manifest, "is shadowed by")?;
    expect_violation(
        &manifest,
        "a self-contradictory packet would be reported as a product outcome",
    )
}
