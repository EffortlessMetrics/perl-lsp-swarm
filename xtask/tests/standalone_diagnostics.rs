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
    assert_eq!(stats.summary_templates, 22);
    assert_eq!(stats.primary_reasons, 32);
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
    assert_eq!(ids.len(), 36);
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
    // `elapsed_seconds` is absent from `forbidden_parameters`, so the overlap
    // check cannot fire and the untyped-parameter branch is the only thing that
    // can reject this. An earlier version of this test pushed `install_root`,
    // which is forbidden, so it passed on the overlap message and would have
    // kept passing if the branch it names were deleted.
    let mut manifest = canonical_manifest()?;
    manifest["render"]["allowed_parameters"]
        .as_array_mut()
        .ok_or("missing allowed parameters")?
        .push(json!("elapsed_seconds"));
    expect_violation(&manifest, "which is not a typed selector field or route_mode")
}

#[test]
fn rejects_a_render_parameter_that_is_allowed_and_forbidden() -> TestResult {
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

// ---------------------------------------------------------------------------
// Second independent review pass (#13800)
// ---------------------------------------------------------------------------

/// `xtask/examples/standalone_candidate_selection.rs` requires a committed
/// rollback to carry the rollback operation: "rollback_committed requires a
/// rollback operation with a committed selection change". A forward operation
/// reporting a committed rollback is therefore a contradiction, not a
/// restoration to celebrate.
#[test]
fn a_forward_operation_cannot_report_a_committed_rollback() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for operation in ["install", "repair", "update"] {
        let subject = combination(
            operation,
            "rollback_committed",
            "rolled_back",
            "completed",
            "verified",
            "persisted",
        );
        let projection = diagnostics::project_combination(&manifest, &subject, None)?;
        assert_eq!(
            projection.get("primary_reason").and_then(Value::as_str),
            Some("inv_rollback_committed_requires_rollback_operation"),
            "`{operation}` reporting a committed rollback must be an instrument failure"
        );
        assert_eq!(
            projection.get("claim_ceiling").and_then(Value::as_str),
            Some("support_claim_withheld")
        );
    }

    // Positive control: the rollback operation itself still restores normally.
    let honest = combination(
        "rollback",
        "rollback_committed",
        "rolled_back",
        "completed",
        "verified",
        "persisted",
    );
    let projection = diagnostics::project_combination(&manifest, &honest, None)?;
    assert_eq!(
        projection.get("primary_reason").and_then(Value::as_str),
        Some("rollback_committed_current_restored_startup_verified")
    );
    Ok(())
}

#[test]
fn rejects_a_packet_with_an_unknown_outcome_dimension() -> TestResult {
    let mut subject = valid_packet();
    subject["outcome_dimensions"]["disk_pressure"] = json!("high");
    let error = admission_error(&subject);
    assert!(
        error.contains("`outcome_dimensions` has unknown field `disk_pressure`"),
        "got: {error}"
    );
    Ok(())
}

#[test]
fn the_public_projection_entry_point_bounds_its_route_mode() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let subject = combination(
        "install",
        "selection_committed",
        "installed",
        "completed",
        "verified",
        "persisted",
    );
    // `project_packet` admits first, but `project_combination` is public and
    // must not become a way around the closed route set.
    let error = match diagnostics::project_combination(&manifest, &subject, Some("anything at all"))
    {
        Ok(_) => "projection unexpectedly succeeded".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("outside the typed domain"), "got: {error}");
    diagnostics::project_combination(&manifest, &subject, Some("first_party_powershell"))?;
    diagnostics::project_combination(&manifest, &subject, None)?;
    Ok(())
}

#[test]
fn a_multibyte_reason_within_the_character_limit_is_admitted() -> TestResult {
    // The input schema bounds `bounded_reason` at 512 *characters*. Measuring
    // UTF-8 bytes would reject a legitimate non-ASCII reason well inside it.
    let reason: String = "é".repeat(400);
    assert!(reason.len() > 512, "fixture must exceed 512 bytes to be discriminating");
    assert!(reason.chars().count() <= 512);
    let subject = packet(
        "selection_committed",
        json!({
            "product_units": "installed",
            "cleanup": "completed",
            "process_startup": "verified",
            "path_persistence": "persisted"
        }),
        &reason,
    );
    diagnostics::read_packet(&subject)?;

    // Still bounded: past the character limit it is rejected.
    let overlong: String = "é".repeat(513);
    let subject = packet(
        "selection_committed",
        json!({
            "product_units": "installed",
            "cleanup": "completed",
            "process_startup": "verified",
            "path_persistence": "persisted"
        }),
        &overlong,
    );
    let error = admission_error(&subject);
    assert!(error.contains("1 to 512 characters"), "got: {error}");
    Ok(())
}

#[test]
fn a_failed_fresh_process_is_not_reported_as_awaiting_a_new_session() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for path_persistence in ["persisted", "unchanged"] {
        let subject = combination(
            "install",
            "selection_committed",
            "installed",
            "completed",
            "failed",
            path_persistence,
        );
        let projection = diagnostics::project_combination(&manifest, &subject, None)?;
        let consequence =
            projection.pointer("/consequences/path").and_then(Value::as_str).unwrap_or_default();
        assert!(
            consequence.ends_with("startup_failed"),
            "`{path_persistence}` with a failed fresh process must not name a new session as the \
             remaining step, got `{consequence}`"
        );
    }
    Ok(())
}

#[test]
fn a_registry_gap_reports_its_true_size_not_the_example_cap() -> TestResult {
    let mut manifest = canonical_manifest()?;
    // Removing this reason uncovers far more than the five retained examples.
    let index = position_of(&manifest, "primary_reasons", "rollback_committed_current_restored")
        .ok_or("missing reason")?;
    reasons_mut(&mut manifest, "primary_reasons").ok_or("missing primary reasons")?.remove(index);
    let error = validation_error(diagnostics::validate_manifest_value(&manifest));
    assert!(error.contains("registry gap"), "got: {error}");
    assert!(
        !error.contains("matches 5 typed combination(s)"),
        "the gap total must not be the capped example count, got: {error}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Third independent review pass (#13800)
// ---------------------------------------------------------------------------

#[test]
fn the_public_projection_entry_point_rejects_an_untyped_combination() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    // `Combination` has public fields, so a caller can build one the packet
    // admission path would never have produced.
    let subject = combination(
        "reinstall_everything",
        "selection_committed",
        "installed",
        "completed",
        "verified",
        "persisted",
    );
    let error = match diagnostics::project_combination(&manifest, &subject, None) {
        Ok(_) => "projection unexpectedly succeeded".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("`operation` value"), "got: {error}");
    assert!(error.contains("outside the typed domain"), "got: {error}");
    Ok(())
}

/// A clean primary outcome must not present as finished while an additional
/// reason still needs the user to do something. The residue was previously
/// reported only as an identifier beside success text.
#[test]
fn outstanding_cleanup_is_not_hidden_behind_a_successful_install() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projection = diagnostics::project_packet(
        &manifest,
        &packet(
            "selection_committed",
            json!({
                "product_units": "installed",
                "cleanup": "failed_preserved",
                "process_startup": "verified",
                "path_persistence": "persisted"
            }),
            "installed, but cleanup left residue",
        ),
    )?;

    // The primary outcome is still an honest terminal success on its own axis…
    assert_eq!(
        projection.get("primary_terminality").and_then(Value::as_str),
        Some("terminal_success")
    );
    // …but the reported state must not claim the transaction is finished.
    assert_eq!(
        projection.get("terminality").and_then(Value::as_str),
        Some("nonterminal_actionable"),
        "outstanding actionable work must degrade the reported terminality"
    );
    // And the residue must be readable, not just referenced by id.
    let outstanding = projection
        .pointer("/render/outstanding")
        .and_then(Value::as_array)
        .ok_or("missing render.outstanding")?;
    assert!(
        outstanding.iter().any(|row| {
            row.get("reason_id").and_then(Value::as_str)
                == Some("cleanup_incomplete_residue_retained")
                && row.get("text").and_then(Value::as_str).is_some_and(|text| !text.is_empty())
        }),
        "the outstanding cleanup must carry its own rendered text, got {outstanding:?}"
    );
    assert!(action_ids(&projection).iter().any(|id| id == "manual_owned_state_resolution"));

    // Control: a clean cleanup leaves the terminal success intact and nothing
    // outstanding, so the degradation above is not unconditional.
    let clean = diagnostics::project_packet(
        &manifest,
        &packet(
            "selection_committed",
            json!({
                "product_units": "installed",
                "cleanup": "completed",
                "process_startup": "verified",
                "path_persistence": "persisted"
            }),
            "installed cleanly",
        ),
    )?;
    assert_eq!(clean.get("terminality").and_then(Value::as_str), Some("terminal_success"));
    assert_eq!(
        clean.pointer("/render/outstanding").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );
    Ok(())
}

/// Domain agreement is about which values are admitted. Reordering an enum
/// leaves the accepted packet domain identical and must not read as drift,
/// while a genuinely widened contract still must.
#[test]
fn reordering_an_input_enum_is_not_contract_drift() -> TestResult {
    let root = staged_root("reordered")?;
    let schema_path = root.join(diagnostics::INPUT_SCHEMA_PATH);
    let mut schema: Value = serde_json::from_slice(&std::fs::read(&schema_path)?)?;
    let dispositions = schema["properties"]["disposition"]["enum"]
        .as_array_mut()
        .ok_or("disposition enum missing")?;
    dispositions.reverse();
    std::fs::write(&schema_path, format!("{}\n", serde_json::to_string_pretty(&schema)?))?;

    let result = diagnostics::validate_manifest_file(&root);
    std::fs::remove_dir_all(&root).ok();
    result?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Fourth independent review pass (#13800): consequences the packet never
// established
// ---------------------------------------------------------------------------

/// The first outstanding-work fix filtered on one terminality value. The honest
/// predicate is "not terminal", so a deferred cleanup counts too — but it is
/// awaited, not actionable, and must not be reported as work the user owes.
#[test]
fn deferred_cleanup_is_outstanding_without_being_called_actionable() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projection = diagnostics::project_packet(
        &manifest,
        &packet(
            "selection_committed",
            json!({
                "product_units": "installed",
                "cleanup": "deferred",
                "process_startup": "verified",
                "path_persistence": "persisted"
            }),
            "installed, cleanup deferred",
        ),
    )?;
    assert_eq!(
        projection.get("terminality").and_then(Value::as_str),
        Some("nonterminal_awaiting_next_stage"),
        "a deferred cleanup is outstanding, so the transaction is not finished"
    );
    assert_ne!(
        projection.get("terminality").and_then(Value::as_str),
        Some("nonterminal_actionable"),
        "a deferred cleanup asks nothing of the user and must not be called actionable"
    );
    assert!(
        projection.pointer("/render/outstanding").and_then(Value::as_array).is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("reason_id").and_then(Value::as_str) == Some("cleanup_deferred_pending")
            })
        }),
        "the deferred cleanup must be readable, not just referenced"
    );
    Ok(())
}

/// The input contract permits verifying or publishing a candidate with no prior
/// selection at all, so those outcomes cannot assert a retained known-good
/// installation — there may be none.
#[test]
fn a_candidate_only_outcome_does_not_invent_a_known_good_installation() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for disposition in ["candidate_verified", "candidate_published_unselected"] {
        let mut subject = packet(
            disposition,
            json!({
                "product_units": "not_applicable",
                "cleanup": "completed",
                "process_startup": "unproven",
                "path_persistence": "not_applicable"
            }),
            "candidate stage only; nothing installed before",
        );
        // `candidate_published_unselected` must name the candidate it
        // published; the origin authority resolves that id against the
        // catalog, so a null here is not an admissible packet.
        if disposition == "candidate_published_unselected" {
            subject["candidate_id"] = json!("b".repeat(64));
        }
        let projection = diagnostics::project_packet(&manifest, &subject)?;
        assert_eq!(
            projection.pointer("/consequences/known_good").and_then(Value::as_str),
            Some("not_established_by_this_transaction"),
            "`{disposition}` must not claim a retained known-good installation"
        );
    }

    // Control: a preserved current genuinely does retain one.
    let preserved = diagnostics::project_packet(
        &manifest,
        &packet(
            "failed_preserved_current",
            json!({
                "product_units": "preserved_prior",
                "cleanup": "completed",
                "process_startup": "verified",
                "path_persistence": "unchanged"
            }),
            "failed, prior installation preserved",
        ),
    )?;
    assert_eq!(
        preserved.pointer("/consequences/known_good").and_then(Value::as_str),
        Some("retained")
    );
    Ok(())
}

/// "Rollback not required" is a claim about a forward operation. A rollback
/// that did not commit is a rollback that failed to happen.
#[test]
fn a_rollback_that_did_not_commit_is_not_reported_as_unnecessary() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for disposition in
        ["failed_preserved_current", "cancelled_preserved_current", "not_proven_preserved_current"]
    {
        let subject = combination(
            "rollback",
            disposition,
            "preserved_prior",
            "completed",
            "verified",
            "unchanged",
        );
        let projection = diagnostics::project_combination(&manifest, &subject, None)?;
        assert_eq!(
            projection.pointer("/consequences/rollback").and_then(Value::as_str),
            Some("attempted_and_not_committed"),
            "a rollback operation ending in `{disposition}` was attempted, not unnecessary"
        );
    }

    // Control: a forward operation that failed genuinely needed no rollback.
    let forward = combination(
        "install",
        "failed_preserved_current",
        "preserved_prior",
        "completed",
        "verified",
        "unchanged",
    );
    let projection = diagnostics::project_combination(&manifest, &forward, None)?;
    assert_eq!(
        projection.pointer("/consequences/rollback").and_then(Value::as_str),
        Some("not_required_current_preserved")
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Review pass: malformed templates, aggregated retryability, ill-typed fields,
// and dispositions that cannot be described without an identity.
// ---------------------------------------------------------------------------

/// Replace the text of the first summary template and expect a violation.
fn expect_template_text_violation(text: &str, needle: &str) -> TestResult {
    let mut manifest = canonical_manifest()?;
    manifest["summary_templates"]
        .as_array_mut()
        .and_then(|items| items.first_mut())
        .ok_or("missing template")?["text"] = json!(text);
    expect_violation(&manifest, needle)
}

#[test]
fn rejects_a_template_with_an_unterminated_placeholder() -> TestResult {
    // The dangerous case is precisely this one: the fragment after `{` is
    // `operation`, an allowed parameter, so the old parser handed back a legal
    // parameter name and the malformed text validated.
    expect_template_text_violation("Install failed: {operation", "unterminated `{` at byte")
}

#[test]
fn rejects_a_template_with_an_unmatched_closing_brace() -> TestResult {
    expect_template_text_violation("Install failed: operation}", "unmatched `}` at byte")
}

#[test]
fn rejects_a_template_with_an_empty_placeholder() -> TestResult {
    expect_template_text_violation("Install failed: {}", "empty placeholder `{}` at byte")
}

#[test]
fn rejects_a_template_with_nested_placeholder_braces() -> TestResult {
    expect_template_text_violation("Install failed: {oper{ation}", "nested `{` at byte")
}

#[test]
fn a_well_formed_template_is_still_accepted() -> TestResult {
    // Negative control for the four rejections above: the brace check must not
    // reject the shape the registry actually uses.
    let mut manifest = canonical_manifest()?;
    manifest["summary_templates"]
        .as_array_mut()
        .and_then(|items| items.first_mut())
        .ok_or("missing template")?["text"] = json!("The {operation} finished via {route_mode}.");
    diagnostics::validate_manifest_value(&manifest)?;
    Ok(())
}

#[test]
fn a_failed_cleanup_raises_the_retry_precondition() -> TestResult {
    // `failed_preserved_current` is `retryable_same_subject` on its own. With
    // residue retained, telling a consumer to retry the same subject omits the
    // manual resolution that must happen first.
    let manifest = canonical_manifest()?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "failed_preserved_current",
            "unchanged",
            "failed_preserved",
            "not_applicable",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(projected["primary_retryability"], json!("retryable_same_subject"));
    assert_eq!(projected["retryability"], json!("retryable_after_user_action"));
    Ok(())
}

#[test]
fn an_unproven_cleanup_forbids_a_bare_retry() -> TestResult {
    let manifest = canonical_manifest()?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "failed_preserved_current",
            "unchanged",
            "not_proven",
            "not_applicable",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(projected["retryability"], json!("retry_forbidden_requires_replan"));
    Ok(())
}

#[test]
fn a_deferred_cleanup_does_not_forbid_retrying_the_failure_it_accompanies() -> TestResult {
    // Negative control against over-aggregating. `cleanup_deferred_pending`
    // carries `not_retryable`, but that is a statement about the cleanup, not a
    // prohibition on retrying the install, so the primary must survive.
    let manifest = canonical_manifest()?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "failed_preserved_current",
            "unchanged",
            "deferred",
            "not_applicable",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(projected["retryability"], json!("retryable_same_subject"));
    Ok(())
}

#[test]
fn outstanding_work_never_makes_a_successful_outcome_retryable() -> TestResult {
    // The opposite direction: a primary that has nothing to retry must not be
    // turned into an invitation to reinstall by an accompanying obligation.
    let manifest = canonical_manifest()?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "selection_committed",
            "installed",
            "failed_preserved",
            "verified",
            "persisted",
        ),
        None,
    )?;
    assert_eq!(projected["retryability"], json!("not_retryable"));
    Ok(())
}

#[test]
fn a_present_but_non_string_typed_field_is_not_reported_as_missing() -> TestResult {
    let mut subject = valid_packet();
    subject["outcome_dimensions"]["cleanup"] = json!(7);
    let error = admission_error(&subject);
    assert!(
        error.contains("`cleanup` must be a string in the typed domain"),
        "expected an ill-shaped-value diagnostic, got:\n{error}"
    );
    assert!(
        !error.contains("missing `cleanup`"),
        "a present field must not be reported as missing, got:\n{error}"
    );
    Ok(())
}

#[test]
fn a_top_level_non_string_typed_field_is_not_reported_as_missing() -> TestResult {
    let mut subject = valid_packet();
    subject["operation"] = json!(7);
    let error = admission_error(&subject);
    assert!(
        error.contains("`operation` must be a string in the typed domain"),
        "expected an ill-shaped-value diagnostic, got:\n{error}"
    );
    assert!(
        !error.contains("missing `operation`"),
        "a present field must not be reported as missing, got:\n{error}"
    );
    Ok(())
}

#[test]
fn an_absent_typed_field_is_still_reported_as_missing() -> TestResult {
    // Negative control: the missing-key message must survive for genuinely
    // absent keys, which is the case the ill-shaped arm could have swallowed.
    let mut subject = valid_packet();
    subject
        .as_object_mut()
        .ok_or("packet must be an object")?
        .remove("operation")
        .ok_or("packet must carry operation")?;
    let error = admission_error(&subject);
    assert!(
        error.contains("transition packet is missing `operation`"),
        "expected a missing-key diagnostic, got:\n{error}"
    );
    Ok(())
}

#[test]
fn a_committed_rollback_must_name_the_candidate_it_demoted() -> TestResult {
    let mut subject = valid_packet();
    subject["operation"] = json!("rollback");
    subject["disposition"] = json!("rollback_committed");
    subject["outcome_dimensions"]["product_units"] = json!("rolled_back");
    let error = admission_error(&subject);
    assert!(
        error.contains("`rollback_committed` must name `prior_current_candidate_id`"),
        "expected an identity admission failure, got:\n{error}"
    );
    Ok(())
}

#[test]
fn a_published_candidate_must_name_itself() -> TestResult {
    let mut subject = valid_packet();
    subject["disposition"] = json!("candidate_published_unselected");
    subject["outcome_dimensions"]["product_units"] = json!("not_applicable");
    let error = admission_error(&subject);
    assert!(
        error.contains("`candidate_published_unselected` must name `candidate_id`"),
        "expected an identity admission failure, got:\n{error}"
    );
    Ok(())
}

#[test]
fn a_committed_selection_is_not_required_to_name_a_candidate_id() -> TestResult {
    // Negative control against a false invariant. The origin authority binds
    // `selection_committed` identity through the packet's `next_selection`
    // record, which the transition record does not carry; requiring
    // `candidate_id` here would reject packets the authority admits.
    let subject = valid_packet();
    assert_eq!(subject["candidate_id"], json!(null));
    diagnostics::read_packet(&subject)?;
    Ok(())
}

#[test]
fn a_committed_rollback_that_names_its_prior_candidate_is_admitted() -> TestResult {
    // Positive control for the rollback identity rule.
    let mut subject = valid_packet();
    subject["operation"] = json!("rollback");
    subject["disposition"] = json!("rollback_committed");
    subject["outcome_dimensions"]["product_units"] = json!("rolled_back");
    subject["prior_current_candidate_id"] = json!("a".repeat(64));
    diagnostics::read_packet(&subject)?;
    Ok(())
}

#[test]
fn actions_are_offered_in_selection_order_not_registry_order() -> TestResult {
    // Every `inv_` reason declares `report_instrument_failure` first: when the
    // record contradicts itself, reporting the instrument is the first move and
    // reading the receipt is secondary. The global action registry lists
    // `inspect_exact_receipt` earlier, so emitting rows in registry order
    // inverted the advice on exactly the outcomes that matter most.
    let manifest = canonical_manifest()?;
    let registry_order: Vec<String> = manifest["actions"]
        .as_array()
        .ok_or("missing actions")?
        .iter()
        .filter_map(|action| action.get("action_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let receipt = registry_order
        .iter()
        .position(|id| id == "inspect_exact_receipt")
        .ok_or("missing inspect_exact_receipt")?;
    let instrument = registry_order
        .iter()
        .position(|id| id == "report_instrument_failure")
        .ok_or("missing report_instrument_failure")?;
    assert!(
        receipt < instrument,
        "this test only discriminates while the registry lists the receipt action first"
    );

    // `selection_committed` with unchanged units is a contradictory packet and
    // reaches `inv_selection_committed_without_product_effect`.
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "selection_committed",
            "unchanged",
            "completed",
            "verified",
            "persisted",
        ),
        None,
    )?;
    let offered = action_ids(&projected);
    assert_eq!(
        offered.first().map(String::as_str),
        Some("report_instrument_failure"),
        "a contradictory packet must lead with the instrument failure, got {offered:?}"
    );
    Ok(())
}

#[test]
fn every_combination_leads_with_its_primary_reasons_first_action() -> TestResult {
    // Sweeps the ordering property, not merely the presence of rows. For each
    // of the 17,920 combinations the first offered action must be the first
    // action the *matched primary reason* declares — read back out of the
    // manifest, so the expectation comes from the registry rather than from the
    // projection that is under test.
    //
    // An earlier version of this sweep asserted only that rows existed and
    // carried ids, which the registry-order implementation also satisfied. It
    // proved nothing about order, which was the whole subject.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for combination in diagnostics::all_combinations() {
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        let rows = projection
            .get("allowed_actions")
            .and_then(Value::as_array)
            .ok_or("missing allowed actions")?;
        assert!(!rows.is_empty(), "no action offered for {combination:?}");

        let primary_id = projection
            .get("primary_reason")
            .and_then(Value::as_str)
            .ok_or("missing primary reason")?;
        let declared = manifest["primary_reasons"]
            .as_array()
            .ok_or("missing primary reasons")?
            .iter()
            .find(|reason| reason.get("reason_id").and_then(Value::as_str) == Some(primary_id))
            .and_then(|reason| reason.get("action_ids"))
            .and_then(Value::as_array)
            .and_then(|ids| ids.first())
            .and_then(Value::as_str)
            .ok_or("primary reason declares no action")?;

        let offered = rows
            .first()
            .and_then(|row| row.get("action_id"))
            .and_then(Value::as_str)
            .ok_or("action row without an id")?;

        // `no_action_required` is dropped when other actions join it, so it is
        // the one declared-first value that legitimately does not lead.
        if declared != "no_action_required" {
            assert_eq!(
                offered, declared,
                "`{primary_id}` declares `{declared}` first but the projection led with \
                 `{offered}` for {combination:?}"
            );
        }

        for row in rows {
            assert!(
                row.get("action_id").and_then(Value::as_str).is_some(),
                "action row without an id for {combination:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_present_but_non_object_outcome_dimensions_is_not_reported_as_missing() -> TestResult {
    let mut subject = valid_packet();
    subject["outcome_dimensions"] = json!(null);
    let error = admission_error(&subject);
    assert!(
        error.contains("`outcome_dimensions` must be an object"),
        "expected an ill-shaped-container diagnostic, got:\n{error}"
    );
    assert!(
        !error.contains("missing `outcome_dimensions`"),
        "a present field must not be reported as missing, got:\n{error}"
    );
    Ok(())
}

#[test]
fn an_absent_outcome_dimensions_is_still_reported_as_missing() -> TestResult {
    let mut subject = valid_packet();
    subject
        .as_object_mut()
        .ok_or("packet must be an object")?
        .remove("outcome_dimensions")
        .ok_or("packet must carry outcome_dimensions")?;
    let error = admission_error(&subject);
    assert!(
        error.contains("transition packet is missing `outcome_dimensions`"),
        "expected a missing-key diagnostic, got:\n{error}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Fifth review pass: a widened domain the string comparison could not see, and
// a candidate stage described as a completed installation.
// ---------------------------------------------------------------------------

#[test]
fn a_non_string_input_enum_member_is_contract_drift() -> TestResult {
    // A JSON Schema enum may hold any JSON value, so a non-string member really
    // does widen the admitted packet domain. Filtering to strings before the
    // set comparison discarded it, leaving the sets equal and the drift silent
    // — which is the one thing this check exists to prevent.
    let root = staged_root("nonstring-enum")?;
    let schema_path = root.join(diagnostics::INPUT_SCHEMA_PATH);
    let mut schema: Value = serde_json::from_slice(&std::fs::read(&schema_path)?)?;
    schema["properties"]["disposition"]["enum"]
        .as_array_mut()
        .ok_or("disposition enum missing")?
        .push(json!(7));
    std::fs::write(&schema_path, format!("{}\n", serde_json::to_string_pretty(&schema)?))?;

    let result = diagnostics::validate_manifest_file(&root);
    std::fs::remove_dir_all(&root).ok();
    let error = validation_error(result);
    assert!(
        error.contains("non-string enum member"),
        "expected non-string enum drift, got:\n{error}"
    );
    Ok(())
}

#[test]
fn a_null_input_enum_member_is_contract_drift() -> TestResult {
    // `null` is the member most likely to be added by accident, and the one a
    // string filter is most likely to swallow.
    let root = staged_root("null-enum")?;
    let schema_path = root.join(diagnostics::INPUT_SCHEMA_PATH);
    let mut schema: Value = serde_json::from_slice(&std::fs::read(&schema_path)?)?;
    schema["properties"]["operation"]["enum"]
        .as_array_mut()
        .ok_or("operation enum missing")?
        .push(Value::Null);
    std::fs::write(&schema_path, format!("{}\n", serde_json::to_string_pretty(&schema)?))?;

    let result = diagnostics::validate_manifest_file(&root);
    std::fs::remove_dir_all(&root).ok();
    let error = validation_error(result);
    assert!(
        error.contains("non-string enum member"),
        "expected non-string enum drift, got:\n{error}"
    );
    Ok(())
}

#[test]
fn a_candidate_stage_startup_failure_never_claims_an_installation() -> TestResult {
    // `candidate_verified` and `candidate_published_unselected` precede
    // selection by definition, so nothing is installed as current. Both reasons
    // reused the committed-install template, whose text says the operation
    // "finished" and refers to "the installed command", and both offered a
    // repair action whose applicability requires an owned installation. The
    // structured consequence was already honest; the rendered text and the
    // offered action were not.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for (disposition, expected_template) in [
        ("candidate_verified", "t_candidate_verified_startup_failed"),
        ("candidate_published_unselected", "t_candidate_published_startup_failed"),
    ] {
        let mut subject = packet(
            disposition,
            json!({
                "product_units": "not_applicable",
                "cleanup": "completed",
                "process_startup": "failed",
                "path_persistence": "not_applicable"
            }),
            "candidate stage only",
        );
        if disposition == "candidate_published_unselected" {
            subject["candidate_id"] = json!("c".repeat(64));
        }
        let projection = diagnostics::project_packet(&manifest, &subject)?;

        let text = projection
            .pointer("/render/text")
            .and_then(Value::as_str)
            .ok_or("missing rendered text")?;
        assert!(
            !text.contains("installed command"),
            "`{disposition}` must not name an installed command: {text}"
        );
        assert!(
            !text.contains("finished"),
            "`{disposition}` must not report the operation as finished: {text}"
        );
        assert_eq!(
            projection.pointer("/render/template_id").and_then(Value::as_str),
            Some(expected_template)
        );

        let offered = action_ids(&projection);
        assert!(
            !offered.iter().any(|id| id == "run_explicit_repair"),
            "`{disposition}` must not offer to repair an installation it never established, got {offered:?}"
        );

        // The consequence must stay withheld, which was already true and must
        // not regress while the wording is corrected.
        assert_eq!(
            projection.pointer("/consequences/known_good").and_then(Value::as_str),
            Some("not_established_by_this_transaction")
        );
    }
    Ok(())
}

#[test]
fn a_committed_startup_failure_still_names_the_installed_command() -> TestResult {
    // Negative control. `t_startup_failed` is correct where an install really
    // did commit, so the candidate-stage fix must not have blanked the wording
    // for the case it was written for.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projection = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "selection_committed",
            "installed",
            "completed",
            "failed",
            "persisted",
        ),
        None,
    )?;
    let text = projection
        .pointer("/render/text")
        .and_then(Value::as_str)
        .ok_or("missing rendered text")?;
    assert!(
        text.contains("installed command"),
        "committed startup failure lost its wording: {text}"
    );
    assert!(action_ids(&projection).iter().any(|id| id == "run_explicit_repair"));
    Ok(())
}

#[test]
fn a_required_new_session_is_always_offered_as_an_action() -> TestResult {
    // The rule existed for `selection_committed` only. `rollback_committed`
    // and `selection_unchanged` reach the same PATH consequence through their
    // broad fallback rows, which reported `terminal_success` and offered only
    // receipt inspection — so the projection told the user a new session was
    // required in `consequences.path` while its terminality said there was
    // nothing left to do and no action named the session.
    //
    // Swept across all three dispositions rather than pinned for the two that
    // were broken, so a fourth disposition reaching this consequence cannot
    // repeat it.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let mut checked = 0usize;
    for combination in diagnostics::all_combinations() {
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        // The property is about the *claim*, not the PATH dimension alone.
        // Writing it against `consequences.path` was too strong and produced
        // two false positives worth recording: a contradictory packet, whose
        // support claim is withheld precisely so nothing acts on its PATH
        // report, and a failed attempt with a preserved prior installation,
        // where "start a new session to use the command" would name the wrong
        // command. Neither is a defect. The honest invariant is narrower and
        // stronger: whenever the projection *promises* the command becomes
        // available after a new session, it must say to start one.
        if projection.get("claim_ceiling").and_then(Value::as_str)
            != Some("command_available_after_new_session")
        {
            continue;
        }
        checked += 1;
        let offered = action_ids(&projection);
        assert!(
            offered.iter().any(|id| id == "start_documented_new_session"),
            "{combination:?} says a new session is required but offers {offered:?}"
        );
        assert_eq!(
            projection.get("primary_terminality").and_then(Value::as_str),
            Some("terminal_success_pending_user_step"),
            "{combination:?} requires a user step, so its primary terminality must say so"
        );
    }
    assert!(checked > 0, "no combination exercised the new-session consequence");
    Ok(())
}

#[test]
fn an_observed_fresh_process_never_asks_for_a_new_session() -> TestResult {
    // Control in the opposite direction: the fix must not have widened the
    // new-session claim onto outcomes that already proved visibility.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    for combination in diagnostics::all_combinations() {
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        if projection.pointer("/consequences/path").and_then(Value::as_str)
            != Some("persisted_and_visible")
        {
            continue;
        }
        assert!(
            !action_ids(&projection).iter().any(|id| id == "start_documented_new_session"),
            "{combination:?} already observed a fresh process and must not ask for a new session"
        );
    }
    Ok(())
}

#[test]
fn an_awaiting_stage_with_manual_cleanup_reports_the_manual_work() -> TestResult {
    // `candidate_verified` awaits publication (`nonterminal_awaiting_next_stage`).
    // A failed cleanup adds `cleanup_incomplete_residue_retained`, which is
    // `nonterminal_actionable` and contributes `manual_owned_state_resolution`
    // to the offered actions. Terminality must say so; reporting "awaiting"
    // tells a consumer there is nothing for the user while the action list
    // says otherwise.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "candidate_verified",
            "not_applicable",
            "failed_preserved",
            "unproven",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(projected["primary_terminality"], json!("nonterminal_awaiting_next_stage"));
    assert_eq!(projected["terminality"], json!("nonterminal_actionable"));
    assert!(action_ids(&projected).iter().any(|id| id == "manual_owned_state_resolution"));
    Ok(())
}

#[test]
fn an_awaiting_stage_with_unproven_cleanup_reports_the_instrument_work() -> TestResult {
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "candidate_published_unselected",
            "not_applicable",
            "not_proven",
            "unproven",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(projected["primary_terminality"], json!("nonterminal_awaiting_next_stage"));
    assert_eq!(projected["terminality"], json!("nonterminal_actionable"));
    Ok(())
}

#[test]
fn a_deferred_cleanup_does_not_invent_user_work_for_an_awaiting_stage() -> TestResult {
    // Control in the opposite direction. `cleanup_deferred_pending` is
    // `nonterminal_awaiting_next_stage`, so it must not promote an awaiting
    // stage to actionable — there is nothing for the user to do yet.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "candidate_verified",
            "not_applicable",
            "deferred",
            "unproven",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(projected["terminality"], json!("nonterminal_awaiting_next_stage"));
    Ok(())
}

#[test]
fn every_offered_manual_action_is_reported_as_actionable() -> TestResult {
    // Sweep the property rather than pin the two cases above: if the
    // projection asks the user to do something manually, its terminality must
    // not claim the work belongs to the next stage.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let mut checked = 0usize;
    for combination in diagnostics::all_combinations() {
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        let manual = action_ids(&projection).into_iter().any(|id| {
            id == "manual_owned_state_resolution" || id == "manual_path_persistence_required"
        });
        if !manual {
            continue;
        }
        checked += 1;
        assert_ne!(
            projection.get("terminality").and_then(Value::as_str),
            Some("nonterminal_awaiting_next_stage"),
            "{combination:?} asks the user to act manually but reports the work as awaiting the next stage"
        );
    }
    assert!(checked > 0, "no combination offered a manual action");
    Ok(())
}

#[test]
fn a_deferred_cleanup_never_erases_a_pending_user_step() -> TestResult {
    // `sel_committed_path_persistence_failed` is
    // `terminal_success_pending_user_step` and offers
    // `manual_path_persistence_required` — real user work. A *deferred*
    // cleanup is `awaiting`, and degrading the primary to that value would
    // drop the user step from terminality while the action stayed on offer.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "selection_committed",
            "installed",
            "deferred",
            "verified",
            "failed",
        ),
        None,
    )?;
    assert_eq!(projected["primary_terminality"], json!("terminal_success_pending_user_step"));
    assert_eq!(projected["terminality"], json!("terminal_success_pending_user_step"));
    assert!(action_ids(&projected).iter().any(|id| id == "manual_path_persistence_required"));
    Ok(())
}

#[test]
fn a_failed_cleanup_still_outranks_a_pending_user_step() -> TestResult {
    // Control: the pending-step rank must not become a ceiling. A cleanup that
    // is actionable is the stronger obligation and still wins.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "selection_committed",
            "installed",
            "failed_preserved",
            "verified",
            "failed",
        ),
        None,
    )?;
    assert_eq!(projected["primary_terminality"], json!("terminal_success_pending_user_step"));
    assert_eq!(projected["terminality"], json!("nonterminal_actionable"));
    Ok(())
}

#[test]
fn a_contradictory_packet_never_names_a_current_installation() -> TestResult {
    // The `inv_` family exists because the record contradicts itself. Naming a
    // specific installation as current — preserved, advanced, or restored —
    // decides on the strength of a record the projection has just declared
    // untrustworthy, and "preserved" in particular asserts the *prior*
    // installation is still current when a committed selection may well have
    // advanced it. Swept over the whole cross-product so a later invariant row
    // cannot reintroduce it.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let mut checked = 0usize;
    for combination in diagnostics::all_combinations() {
        let projection = diagnostics::project_combination(&manifest, &combination, None)?;
        let primary = projection
            .get("primary_reason")
            .and_then(Value::as_str)
            .ok_or("missing primary reason")?;
        if !primary.starts_with("inv_") {
            continue;
        }
        checked += 1;
        let current = projection
            .pointer("/consequences/current")
            .and_then(Value::as_str)
            .ok_or("missing current consequence")?;
        assert!(
            matches!(current, "unknown_after_contradiction" | "unchanged"),
            "`{primary}` reported current as `{current}` for {combination:?}"
        );
        let known_good = projection
            .pointer("/consequences/known_good")
            .and_then(Value::as_str)
            .ok_or("missing known_good consequence")?;
        assert_eq!(
            known_good, "not_established_by_this_transaction",
            "`{primary}` claimed a known-good installation for {combination:?}"
        );
        assert_eq!(
            projection.get("claim_ceiling").and_then(Value::as_str),
            Some("support_claim_withheld"),
            "`{primary}` must withhold the support claim for {combination:?}"
        );
    }
    assert!(checked > 0, "no combination reached a packet-consistency invariant");
    Ok(())
}

#[test]
fn a_genuinely_preserved_current_still_reports_preservation() -> TestResult {
    // Control: `preserved_but_unproven` remains correct where the packet does
    // establish it, so the invariant fix must not have blanked the honest case.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "failed_preserved_current",
            "preserved_prior",
            "completed",
            "failed",
            "not_applicable",
        ),
        None,
    )?;
    assert_eq!(
        projected["primary_reason"],
        json!("failed_preserved_current_known_good_startup_failed")
    );
    assert_eq!(projected.pointer("/consequences/current"), Some(&json!("preserved_but_unproven")));
    assert_eq!(
        projected.pointer("/consequences/known_good"),
        Some(&json!("retained_but_startup_unproven"))
    );
    Ok(())
}

#[test]
fn a_genuinely_known_good_current_still_reports_it_retained() -> TestResult {
    // Second control, on the other preserved variant: a preserved current that
    // did start must keep both its `preserved` current and its `retained`
    // known-good.
    let manifest = diagnostics::load_manifest(&repo_root())?;
    let projected = diagnostics::project_combination(
        &manifest,
        &combination(
            "install",
            "failed_preserved_current",
            "preserved_prior",
            "completed",
            "verified",
            "not_applicable",
        ),
        None,
    )?;
    assert!(!projected["primary_reason"].as_str().unwrap_or_default().starts_with("inv_"));
    assert_eq!(projected.pointer("/consequences/current"), Some(&json!("preserved")));
    assert_eq!(projected.pointer("/consequences/known_good"), Some(&json!("retained")));
    Ok(())
}
