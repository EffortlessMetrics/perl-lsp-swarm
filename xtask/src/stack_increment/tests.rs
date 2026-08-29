//! Falsifier-first suite for the exact parent-to-child incremental proof
//! (#11229). Every test names one refused or discriminated behavior from the
//! issue's falsifier list, executed against the pure typed domain.
//!
//! This module is `cfg(test)`-only: assertive unwraps are idiomatic here and
//! stay inside this file.
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use super::plan::{STACK_LOCAL_PROFILE, StackGateScope, stack_plan_digest};
use super::result::{
    ChildIncrementStatus, ContextStatus, ParentPrerequisiteState, RunIdentity, StackObservation,
    StackResultInput, StackRowResult, compile_result, render_explanation, validate_result,
};
use super::subject::{
    EdgeKind, RelatedHistory, StackEdgeDeclaration, StackEndpoint, StackSubjectCompileError,
    StackSubjectInput, TrustContext, compile_subject, parse_stack_edge_declaration,
    refresh_currentness, subject_digest, validate_subject,
};
use super::{
    DeltaPath, DeltaStatus, PROTECTED_MAIN_NOT_EVALUATED, STACK_ADVISORY_CONTEXT_NAME,
    STACK_INCREMENT_RESULT_SCHEMA, STACK_INCREMENT_SUBJECT_SCHEMA, sha256_hex,
};
use crate::ci_route_plan::{
    ExpansionStatus, LifecycleDisposition, LifecycleState, PlannedOutcome, PolicyRole, Resolution,
    RouteDispositionInput, RouteExecutionIdentity, RouteProfileExpansionInput,
};

const TEST_GATE: &str = "stack-rust-tests";
const STATIC_GATE: &str = "stack-rust-static";
const SCOPE_PREFIX: &str = "crates/perl-parser/";

/// Deterministic 40-character lowercase-hex fixture SHA.
fn hex40(seed: u8) -> String {
    (0..40u32)
        .map(|index| {
            let digit = (u32::from(seed) + index) % 10;
            char::from(b'0' + u8::try_from(digit).unwrap_or_default())
        })
        .collect()
}

const PARENT_HEAD: fn() -> String = || hex40(1);
const PARENT_TREE: fn() -> String = || hex40(2);
const CHILD_HEAD: fn() -> String = || hex40(3);
const CHILD_TREE: fn() -> String = || hex40(4);

fn digest_fixture(seed: &[u8]) -> String {
    sha256_hex(seed)
}

fn endpoint(pr_number: u64, branch: &str, head: String, tree: String) -> StackEndpoint {
    StackEndpoint {
        pr_number,
        issue_node_id: format!("node_{pr_number}"),
        branch: branch.to_string(),
        head_sha: head,
        head_tree: tree,
    }
}

fn declared_edge(parent_pr: u64) -> Option<StackEdgeDeclaration> {
    Some(StackEdgeDeclaration {
        dependency: EdgeKind::ProgrammeDependency,
        parent_pr_number: parent_pr,
        scope_paths: vec![SCOPE_PREFIX.to_string()],
        declared_parent_head_sha: None,
    })
}

fn changed_path(path: &str) -> Vec<DeltaPath> {
    vec![DeltaPath { status: DeltaStatus::Modified, path: path.to_string(), renamed_from: None }]
}

fn subject_input() -> StackSubjectInput {
    let paths = changed_path(&format!("{SCOPE_PREFIX}src/lib.rs"));
    StackSubjectInput {
        repository: "perl-lsp/perl-lsp".to_string(),
        event_id: Some("observation-1".to_string()),
        parent: endpoint(100, "train/100", PARENT_HEAD(), PARENT_TREE()),
        child: endpoint(101, "train/101-child", CHILD_HEAD(), CHILD_TREE()),
        edge: declared_edge(100),
        child_base_expected_head_sha: PARENT_HEAD(),
        observed_parent_head_sha: PARENT_HEAD(),
        observed_child_head_sha: CHILD_HEAD(),
        trust: TrustContext { same_repository_declared: true, external_context_admitted: false },
        history: RelatedHistory::Ancestor,
        delta: super::ChildDelta {
            bound_parent_tree: PARENT_TREE(),
            bound_child_tree: CHILD_TREE(),
            fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &paths),
            paths,
        },
    }
}

fn compiled_subject() -> super::StackIncrementSubjectV1 {
    compile_subject(subject_input()).unwrap()
}

fn expansion(denominator: Vec<String>) -> RouteProfileExpansionInput {
    RouteProfileExpansionInput {
        requested_profile: STACK_LOCAL_PROFILE.to_string(),
        included_native_tiers: vec!["stack_local".to_string()],
        semantic_fingerprint: digest_fixture(b"fingerprint"),
        policy_digest: digest_fixture(b"policy"),
        denominator,
        resolution: ExpansionStatus::Complete,
        detail: None,
    }
}

fn active_disposition(gate_id: &str) -> RouteDispositionInput {
    RouteDispositionInput {
        gate_id: gate_id.to_string(),
        policy_role: PolicyRole::Advisory,
        lifecycle: LifecycleDisposition {
            state: LifecycleState::Active,
            resolution: Resolution::Current,
        },
        native_tier: "stack_local".to_string(),
        quarantine: None,
        detail: None,
    }
}

fn execution_identity(gate_id: &str) -> RouteExecutionIdentity {
    RouteExecutionIdentity {
        gate_id: gate_id.to_string(),
        command: format!("cargo test -p xtask --locked {gate_id}"),
        timeout_seconds: 600,
    }
}

fn plan_request_both_gates(subject: &super::StackIncrementSubjectV1) -> super::StackPlanRequest {
    super::StackPlanRequest {
        subject: subject.clone(),
        expansion: expansion(vec![STATIC_GATE.to_string(), TEST_GATE.to_string()]),
        dispositions: vec![active_disposition(TEST_GATE), active_disposition(STATIC_GATE)],
        disposition_digest: digest_fixture(b"disposition"),
        workflow_digest: digest_fixture(b"workflow"),
        gate_scopes: vec![
            StackGateScope {
                gate_id: TEST_GATE.to_string(),
                path_prefixes: vec![SCOPE_PREFIX.to_string()],
            },
            StackGateScope { gate_id: STATIC_GATE.to_string(), path_prefixes: Vec::new() },
        ],
        execution: vec![execution_identity(TEST_GATE), execution_identity(STATIC_GATE)],
    }
}

fn compiled_plan(subject: &super::StackIncrementSubjectV1) -> crate::ci_route_plan::CiRoutePlanV1 {
    let plan = super::compile_stack_plan(plan_request_both_gates(subject)).unwrap();
    assert!(plan.validate().is_ok());
    plan
}

fn passing_observation(gate_id: &str) -> StackObservation {
    StackObservation {
        gate_id: gate_id.to_string(),
        identity: RunIdentity {
            command: format!("cargo test -p xtask --locked {gate_id}"),
            timeout_seconds: 600,
        },
        passed: true,
        exit_code: Some(0),
        output_digest: Some(digest_fixture(gate_id.as_bytes())),
        instrument_failed: false,
    }
}

fn result_input_current(
    subject: &super::StackIncrementSubjectV1,
    plan: &crate::ci_route_plan::CiRoutePlanV1,
    observations: Vec<StackObservation>,
) -> StackResultInput {
    StackResultInput {
        subject: subject.clone(),
        plan: plan.clone(),
        plan_digest: stack_plan_digest(plan).unwrap(),
        observations,
        parent_state: Some(ParentPrerequisiteState::Passed),
        observed_parent_head_sha: subject.parent.head_sha.clone(),
        observed_child_head_sha: subject.child.head_sha.clone(),
        reproduce_command:
            "cargo xtask ci-stack explain --result target/receipts/ci-stack-result.json".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Subject admission discriminators
// ---------------------------------------------------------------------------

#[test]
fn ci_stack_increment_subject_compiles_and_binds_exact_trees() {
    let subject = compiled_subject();
    assert_eq!(subject.schema, STACK_INCREMENT_SUBJECT_SCHEMA);
    assert_eq!(subject.protected_main_state, PROTECTED_MAIN_NOT_EVALUATED);
    assert_eq!(subject.child_base_expected_head_sha, subject.parent.head_sha);
    assert_eq!(subject.delta.bound_parent_tree, subject.parent.head_tree);
    assert_eq!(subject.delta.bound_child_tree, subject.child.head_tree);
    assert!(validate_subject(&subject).is_ok());
}

#[test]
fn ci_stack_increment_falsifies_convention_derived_edges() {
    // No declaration at all.
    let mut input = subject_input();
    input.edge = None;
    let error = compile_subject(input).unwrap_err();
    assert_eq!(error.code, "edge_undeclared");

    // Body without the machine-readable line cannot admit an edge.
    let body = "branch: train/101-child\nfixes #11229\n";
    let error = parse_stack_edge_declaration(body).unwrap_err();
    assert_eq!(error.code, "edge_undeclared");

    // Branch-name-derived pseudo edges stay invalid declarations.
    let error = parse_stack_edge_declaration("stack-edge: parent_branch=train/100").unwrap_err();
    assert_eq!(error.code, "edge_declaration_invalid");
}

#[test]
fn ci_stack_increment_parser_accepts_exactly_one_strict_declaration() {
    let edge = parse_stack_edge_declaration(
        "stack-edge: dependency=programme_dependency parent-pr=100 \
         scope=-",
    )
    .unwrap();
    assert_eq!(edge.parent_pr_number, 100);
    assert!(edge.scope_paths.is_empty());

    let duplicated = parse_stack_edge_declaration(
        "stack-edge: dependency=programme_dependency parent-pr=100 scope=-\n\
         stack-edge: dependency=programme_dependency parent-pr=100 scope=-",
    )
    .unwrap_err();
    assert_eq!(duplicated.code, "edge_declaration_invalid");
}

#[test]
fn ci_stack_increment_falsifies_wrong_parent_base_binding() {
    let mut input = subject_input();
    input.child_base_expected_head_sha = hex40(9);
    let error = validate_admission(input);
    assert_eq!(error.code, "wrong_parent_base");
}

#[test]
fn ci_stack_increment_falsifies_parent_movement_before_execution() {
    let subject = compiled_subject();
    let error = refresh_currentness(&subject, &hex40(8), &subject.child.head_sha).unwrap_err();
    assert_eq!(error.code, "parent_moved_since_admission");
}

#[test]
fn ci_stack_increment_falsifies_child_movement_before_publication() {
    let subject = compiled_subject();
    let error = refresh_currentness(&subject, &subject.parent.head_sha, &hex40(7)).unwrap_err();
    assert_eq!(error.code, "child_moved_since_admission");
}

#[test]
fn ci_stack_increment_refuses_unrelated_history() {
    let mut input = subject_input();
    input.history = RelatedHistory::Unrelated;
    let error = validate_admission(input);
    assert_eq!(error.code, "unrelated_stack_history");
}

#[test]
fn ci_stack_increment_refuses_unproven_history_evidence() {
    for relation in [
        RelatedHistory::Diverged,
        RelatedHistory::NotProvenShallow,
        RelatedHistory::NotProvenPartialClone,
        RelatedHistory::NotProvenMissingObject,
    ] {
        let mut input = subject_input();
        input.history = relation;
        let error = validate_admission(input);
        assert!(
            error.code == "unrelated_stack_history" || error.code == "history_not_proven",
            "relation {} produced {}",
            relation.as_str(),
            error.code
        );
    }
}

#[test]
fn ci_stack_increment_refuses_delta_outside_declared_scope() {
    let paths = changed_path("docs/unrelated.md");
    let mut input = subject_input();
    input.delta = super::ChildDelta {
        bound_parent_tree: PARENT_TREE(),
        bound_child_tree: CHILD_TREE(),
        fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &paths),
        paths,
    };
    let error = validate_admission(input);
    assert_eq!(error.code, "undeclared_delta_surface");
}

#[test]
fn ci_stack_increment_fingerprint_rejects_tree_drift() {
    let subject = compiled_subject();
    let drifted = super::StackIncrementSubjectV1 {
        child: endpoint(101, "train/101-child", CHILD_HEAD(), hex40(5)),
        ..subject.clone()
    };
    let error = validate_subject(&drifted).unwrap_err();
    assert_eq!(error.code, "delta_unbound_to_trees");
}

#[test]
fn ci_stack_increment_protected_main_cannot_be_promoted() {
    let subject = compiled_subject();
    let mut promoted = subject.clone();
    promoted.protected_main_state = "CURRENT_SUCCESS".to_string();
    let error = validate_subject(&promoted).unwrap_err();
    assert_eq!(error.code, "protected_main_promotion");
}

fn validate_admission(input: StackSubjectInput) -> StackSubjectCompileError {
    // Recompute nothing: inputs drift deliberately per falsifier.
    match compile_subject(input) {
        Ok(_) => panic!("admission unexpectedly succeeded"),
        Err(error) => error,
    }
}

// ---------------------------------------------------------------------------
// Plan selection discriminators
// ---------------------------------------------------------------------------

#[test]
fn ci_stack_increment_candidate_cannot_choose_another_profile() {
    let subject = compiled_subject();
    let mut request = plan_request_both_gates(&subject);
    request.expansion.requested_profile = "pr_fast".to_string();
    let error = super::compile_stack_plan(request).unwrap_err();
    assert_eq!(error.code, "profile_not_stack_local");
}

#[test]
fn ci_stack_increment_selection_applies_only_to_exact_child_head() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    assert_eq!(plan.subject.head_sha, subject.child.head_sha);
    assert_eq!(plan.subject.base_sha.as_deref(), Some(subject.parent.head_sha.as_str()));
    assert_eq!(
        plan.subject.subject_digest,
        subject_digest(&subject),
        "route-plan subject must bind to the exact stack subject digest"
    );
}

#[test]
fn ci_stack_increment_missing_scope_gate_is_a_visible_error_row() {
    let subject = compiled_subject();
    let mut request = plan_request_both_gates(&subject);
    request.gate_scopes.retain(|scope| scope.gate_id != STATIC_GATE);
    let plan = super::compile_stack_plan(request).unwrap();
    let error_row = plan.rows.iter().find(|row| row.gate_id == STATIC_GATE).unwrap();
    match &error_row.outcome {
        PlannedOutcome::Error { code, .. } => assert_eq!(code, "selector_evidence_missing"),
        other => panic!("expected selector-evidence error row, found {other:?}"),
    }
}

#[test]
fn ci_stack_increment_changed_delta_selects_matching_gate_only() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let test_row = plan.rows.iter().find(|row| row.gate_id == TEST_GATE).unwrap();
    let static_row = plan.rows.iter().find(|row| row.gate_id == STATIC_GATE).unwrap();
    assert!(matches!(test_row.outcome, PlannedOutcome::Run { .. }));
    assert!(matches!(static_row.outcome, PlannedOutcome::ScopedNoop { .. }));
}

#[test]
fn ci_stack_increment_plan_digest_is_deterministic() {
    let subject = compiled_subject();
    let left = compiled_plan(&subject);
    let right = compiled_plan(&compiled_subject());
    assert_eq!(stack_plan_digest(&left).unwrap(), stack_plan_digest(&right).unwrap());
}

// ---------------------------------------------------------------------------
// Result fan-in discriminators
// ---------------------------------------------------------------------------

#[test]
fn ci_stack_increment_green_requires_executed_bound_run_rows() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let result =
        compile_result(result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]))
            .unwrap();
    assert_eq!(result.context_status, ContextStatus::CurrentSuccess);
    assert_eq!(result.child_increment_status, ChildIncrementStatus::CurrentSuccess);
    assert_eq!(result.parent_prerequisite_state, ParentPrerequisiteState::Passed);
    match result.rows.get(TEST_GATE) {
        Some(StackRowResult::RunPassed { output_digest }) => {
            assert_eq!(output_digest, &digest_fixture(TEST_GATE.as_bytes()));
        }
        other => panic!("expected run-passed row, found {other:?}"),
    }
}

#[test]
fn ci_stack_increment_metadata_only_green_refused() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut observation = passing_observation(TEST_GATE);
    observation.output_digest = None;
    let error =
        compile_result(result_input_current(&subject, &plan, vec![observation])).unwrap_err();
    assert_eq!(error.code, "run_identity_missing");
}

#[test]
fn ci_stack_increment_observation_identity_mismatch_refused() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut observation = passing_observation(TEST_GATE);
    observation.identity.command = "echo green".to_string();
    let error =
        compile_result(result_input_current(&subject, &plan, vec![observation])).unwrap_err();
    assert_eq!(error.code, "observation_identity_mismatch");
}

#[test]
fn ci_stack_increment_unplanned_observation_refused() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let observation = passing_observation("some-other-suite");
    let error =
        compile_result(result_input_current(&subject, &plan, vec![observation])).unwrap_err();
    assert_eq!(error.code, "unplanned_observation");
}

#[test]
fn ci_stack_increment_instrument_failure_is_never_green() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut observation = passing_observation(TEST_GATE);
    observation.instrument_failed = true;
    let result = compile_result(result_input_current(&subject, &plan, vec![observation])).unwrap();
    assert_eq!(result.context_status, ContextStatus::InstrumentFailed);
    assert_eq!(result.child_increment_status, ChildIncrementStatus::InstrumentFailed);

    // A failure reported without an usable exit code is an instrument state,
    // not semantic evidence of a failing product.
    let mut without_exit = passing_observation(TEST_GATE);
    without_exit.passed = false;
    without_exit.exit_code = None;
    let result = compile_result(result_input_current(&subject, &plan, vec![without_exit])).unwrap();
    assert_eq!(result.context_status, ContextStatus::InstrumentFailed);
}

#[test]
fn ci_stack_increment_parent_blocked_keeps_child_visible() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut input = result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]);
    input.parent_state = Some(ParentPrerequisiteState::Failed);
    let result = compile_result(input).unwrap();
    assert_eq!(result.context_status, ContextStatus::BlockedParent);
    assert_eq!(result.child_increment_status, ChildIncrementStatus::CurrentSuccess);
    assert_eq!(result.parent_prerequisite_state, ParentPrerequisiteState::Failed);
}

#[test]
fn ci_stack_increment_stale_heads_override_the_published_status() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut input = result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]);
    input.observed_child_head_sha = hex40(6);
    let result = compile_result(input).unwrap();
    assert_eq!(result.context_status, ContextStatus::Stale);
    assert_eq!(result.child_increment_status, ChildIncrementStatus::CurrentSuccess);
    assert!(result.staleness_reason.is_some());
}

#[test]
fn ci_stack_increment_old_results_never_satisfy_a_new_pair() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let old_result =
        compile_result(result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]))
            .unwrap();

    // Move only the child endpoint; the entire subject identity moves.
    let moved_paths = changed_path(&format!("{SCOPE_PREFIX}src/lib.rs"));
    let mut moved_input = subject_input();
    moved_input.child = endpoint(101, "train/101-child", hex40(6), hex40(7));
    moved_input.observed_child_head_sha = hex40(6);
    moved_input.delta = super::ChildDelta {
        bound_parent_tree: PARENT_TREE(),
        bound_child_tree: hex40(7),
        fingerprint: super::delta_fingerprint(&PARENT_TREE(), &hex40(7), &moved_paths),
        paths: moved_paths,
    };
    let moved_subject = compile_subject(moved_input).unwrap();
    assert_ne!(subject_digest(&moved_subject), old_result.subject_digest);

    // Replaying the old route-plan digest against the new pair refuses.
    let moved_plan = compiled_plan(&moved_subject);
    let mut replay = result_input_current(&moved_subject, &moved_plan, vec![]);
    replay.plan_digest = old_result.route_plan_digest.clone();
    let error = compile_result(replay).unwrap_err();
    assert_eq!(error.code, "plan_digest_mismatch");
}

#[test]
fn ci_stack_increment_pending_runs_hold_green_back() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let result = compile_result(result_input_current(&subject, &plan, vec![])).unwrap();
    assert_eq!(result.context_status, ContextStatus::Pending);
    assert_eq!(result.child_increment_status, ChildIncrementStatus::Pending);
    match result.rows.get(TEST_GATE) {
        Some(StackRowResult::Pending) => {}
        other => panic!("expected pending row, found {other:?}"),
    }
}

#[test]
fn ci_stack_increment_failed_run_rows_publish_failure() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut observation = passing_observation(TEST_GATE);
    observation.passed = false;
    observation.exit_code = Some(2);
    let result = compile_result(result_input_current(&subject, &plan, vec![observation])).unwrap();
    assert_eq!(result.context_status, ContextStatus::CurrentFailure);
    match result.rows.get(TEST_GATE) {
        Some(StackRowResult::RunFailed { exit_code }) => assert_eq!(*exit_code, 2),
        other => panic!("expected run-failed row, found {other:?}"),
    }
}

#[test]
fn ci_stack_increment_selector_error_rows_hold_not_proven_even_with_passes() {
    let subject = compiled_subject();
    let mut request = plan_request_both_gates(&subject);
    request.gate_scopes.retain(|scope| scope.gate_id != STATIC_GATE);
    let plan = super::compile_stack_plan(request).unwrap();
    let result =
        compile_result(result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]))
            .unwrap();
    assert_eq!(result.context_status, ContextStatus::NotProven);
}

#[test]
fn ci_stack_increment_empty_delta_scopes_to_honest_noop_green() {
    let mut input = subject_input();
    input.edge = declared_edge(100);
    input.delta = super::ChildDelta {
        bound_parent_tree: PARENT_TREE(),
        bound_child_tree: CHILD_TREE(),
        fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &[]),
        paths: Vec::new(),
    };
    let subject = compile_subject(input).unwrap();
    let plan = compiled_plan(&subject);
    let result = compile_result(result_input_current(&subject, &plan, vec![])).unwrap();
    assert_eq!(result.context_status, ContextStatus::ScopedNoop);
    // Protected-main stays untouched even on the only green flavor.
    assert_eq!(result.protected_main_state, PROTECTED_MAIN_NOT_EVALUATED);
}

#[test]
fn ci_stack_increment_forged_empty_rows_cannot_claim_scoped_noop() {
    let mut input = subject_input();
    input.delta = super::ChildDelta {
        bound_parent_tree: PARENT_TREE(),
        bound_child_tree: CHILD_TREE(),
        fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &[]),
        paths: Vec::new(),
    };
    let subject = compile_subject(input).unwrap();
    let plan = compiled_plan(&subject);
    let valid = compile_result(result_input_current(&subject, &plan, vec![])).unwrap();
    assert_eq!(valid.context_status, ContextStatus::ScopedNoop);
    assert!(valid.planned_gate_count > 0);

    let mut forged = valid;
    forged.rows.clear();
    let error = validate_result(&forged).unwrap_err();
    assert_eq!(error.code, "malformed_result");
    assert!(error.message.contains("rows") && error.message.contains("denominator"));
}

#[test]
fn ci_stack_increment_changed_delta_with_zero_runs_never_publishes_green() {
    // Every governed gate proves its own disengagement while the delta moved
    // files inside the stack scope: that selection is insufficient and must
    // refuse rather than publish an empty-plan green.
    let paths = changed_path(&format!("{SCOPE_PREFIX}src/docs.rs"));
    let mut input = subject_input();
    input.delta = super::ChildDelta {
        bound_parent_tree: PARENT_TREE(),
        bound_child_tree: CHILD_TREE(),
        fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &paths),
        paths,
    };
    let subject = compile_subject(input).unwrap();
    let mut request = plan_request_both_gates(&subject);
    // Give the runnable gate no scope hit for the new path layout: strip its
    // prefixes so it too lands scoped-noop.
    for scope in &mut request.gate_scopes {
        if scope.gate_id == TEST_GATE {
            scope.path_prefixes.clear();
        }
    }
    let plan = super::compile_stack_plan(request).unwrap();
    let error = compile_result(result_input_current(&subject, &plan, vec![])).unwrap_err();
    assert!(
        error.message.contains("insufficient selector evidence")
            || error.message.contains("empty-plan green"),
        "{error}"
    );
}

#[test]
fn ci_stack_increment_row_map_normalizes_insertion_order() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let forward =
        compile_result(result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]))
            .unwrap();

    // The canonical check: identical logical results serialize identically
    // regardless of construction order. Rebuild the same logical result a
    // second time through a fresh fixture chain.
    let rebuilt = compile_result(result_input_current(
        &compiled_subject(),
        &compiled_plan(&compiled_subject()),
        vec![passing_observation(TEST_GATE)],
    ))
    .unwrap();
    let left = serde_json::to_vec(&forward).unwrap();
    let right = serde_json::to_vec(&rebuilt).unwrap();
    assert_eq!(left, right);

    // And the serialized artifact carries the stable contract name.
    let rendered = serde_json::from_slice::<serde_json::Value>(&left).unwrap();
    assert_eq!(
        rendered.get("schema").and_then(|value| value.as_str()),
        Some(STACK_INCREMENT_RESULT_SCHEMA)
    );
    assert_eq!(
        rendered.get("context_name").and_then(|value| value.as_str()),
        Some(STACK_ADVISORY_CONTEXT_NAME)
    );
}

#[test]
fn ci_stack_increment_explanation_carries_required_summary_lines() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let result =
        compile_result(result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]))
            .unwrap();
    let text = render_explanation(&result);
    assert!(text.contains(STACK_ADVISORY_CONTEXT_NAME));
    assert!(text.contains(PROTECTED_MAIN_NOT_EVALUATED));
    assert!(text.contains(&format!("repository: {}", subject.repository)));
    assert!(text.contains("#100") && text.contains("#101"));
    assert!(text.contains(&subject_digest(&subject)));
    assert!(text.contains("Reproduce:"));
}

// ---------------------------------------------------------------------------
// #13360 admission-trust discriminators
// ---------------------------------------------------------------------------

/// Sibling-prefix collision under a subtree scope (#13360 root cause 8).
///
/// Under the old string-prefix check (`strip_suffix('/')` +
/// `starts_with`), this exact input passed admission: delta path
/// `crates/foobar/src/lib.rs` started with `crates/foo`. The component-wise
/// boundary test must refuse it as an undeclared surface, while the subtree
/// root and its real children stay admitted.
#[test]
fn ci_stack_increment_sibling_prefix_cannot_pass_subtree_scope() {
    const SIBLING_SCOPE: &str = "crates/foo/";
    let paths = changed_path("crates/foobar/src/lib.rs");
    let mut input = subject_input();
    input.edge = Some(StackEdgeDeclaration {
        dependency: EdgeKind::ProgrammeDependency,
        parent_pr_number: 100,
        scope_paths: vec![SIBLING_SCOPE.to_string()],
        declared_parent_head_sha: None,
    });
    input.delta = super::ChildDelta {
        bound_parent_tree: PARENT_TREE(),
        bound_child_tree: CHILD_TREE(),
        fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &paths),
        paths,
    };
    let error = validate_admission(input);
    assert_eq!(error.code, "undeclared_delta_surface");

    // Positive controls: the declared subtree root and its real children
    // keep admitting; the sibling directory never does.
    for admitted in ["crates/foo", "crates/foo/src/lib.rs", "crates/foo/deep/nested.rs"] {
        let paths = changed_path(admitted);
        let mut input = subject_input();
        input.edge = Some(StackEdgeDeclaration {
            dependency: EdgeKind::ProgrammeDependency,
            parent_pr_number: 100,
            scope_paths: vec![SIBLING_SCOPE.to_string()],
            declared_parent_head_sha: None,
        });
        input.delta = super::ChildDelta {
            bound_parent_tree: PARENT_TREE(),
            bound_child_tree: CHILD_TREE(),
            fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &paths),
            paths,
        };
        assert!(
            compile_subject(input).is_ok(),
            "declared subtree path {admitted:?} must stay admitted"
        );
    }
}

/// The selector derivation honors the same component-wise boundary: a
/// sibling-prefix delta path must not select a gate scoped to `crates/foo/`.
#[test]
fn ci_stack_increment_sibling_prefix_selects_no_gate() {
    let sibling = DeltaPath {
        status: DeltaStatus::Modified,
        path: "crates/foobar/src/lib.rs".to_string(),
        renamed_from: None,
    };
    let selectors = super::derive_selectors(
        &super::ChildDelta {
            bound_parent_tree: PARENT_TREE(),
            bound_child_tree: CHILD_TREE(),
            fingerprint: super::delta_fingerprint(
                &PARENT_TREE(),
                &CHILD_TREE(),
                std::slice::from_ref(&sibling),
            ),
            paths: vec![sibling],
        },
        &[TEST_GATE.to_string()],
        &[StackGateScope {
            gate_id: TEST_GATE.to_string(),
            path_prefixes: vec!["crates/foo/".to_string()],
        }],
    );
    assert_eq!(selectors.len(), 1);
    assert_eq!(selectors[0].placement, crate::ci_route_plan::SelectorPlacement::Skipped);
    assert_eq!(
        selectors[0].proof,
        Some(crate::ci_route_plan::SelectorProof::NotApplicableToSubject)
    );
}

#[test]
fn ci_stack_increment_rename_and_copy_scope_checks_both_paths() {
    let edge = StackEdgeDeclaration {
        dependency: EdgeKind::ProgrammeDependency,
        parent_pr_number: 100,
        scope_paths: vec!["crates/foo/".to_string()],
        declared_parent_head_sha: None,
    };
    let delta = |source: &str, destination: &str| {
        let paths = vec![DeltaPath {
            status: DeltaStatus::Renamed,
            path: destination.to_string(),
            renamed_from: Some(source.to_string()),
        }];
        super::ChildDelta {
            bound_parent_tree: PARENT_TREE(),
            bound_child_tree: CHILD_TREE(),
            fingerprint: super::delta_fingerprint(&PARENT_TREE(), &CHILD_TREE(), &paths),
            paths,
        }
    };

    let error =
        super::check_declared_scope(&delta("crates/outside/old.rs", "crates/foo/new.rs"), &edge)
            .unwrap_err();
    assert!(error.1.contains("source path"));

    let error =
        super::check_declared_scope(&delta("crates/foo/old.rs", "crates/outside/new.rs"), &edge)
            .unwrap_err();
    assert!(error.1.contains("destination path"));

    assert!(
        super::check_declared_scope(&delta("crates/foo/old.rs", "crates/foo/new.rs"), &edge,)
            .is_ok()
    );
}

/// Admission invariants are re-checked at validation, so an edited
/// serialized subject cannot smuggle a non-ancestor history or a parent
/// identity its own edge declaration does not assert (#13360 root cause 1).
#[test]
fn ci_stack_increment_edited_artifact_cannot_revalidate_admission() {
    let subject = compiled_subject();

    let mut unrelated = subject.clone();
    unrelated.history_relation = RelatedHistory::Unrelated;
    assert_eq!(validate_subject(&unrelated).unwrap_err().code, "unrelated_stack_history");

    let mut diverged = subject.clone();
    diverged.history_relation = RelatedHistory::Diverged;
    assert_eq!(validate_subject(&diverged).unwrap_err().code, "unrelated_stack_history");

    let mut shallow = subject.clone();
    shallow.history_relation = RelatedHistory::NotProvenShallow;
    assert_eq!(validate_subject(&shallow).unwrap_err().code, "history_not_proven");

    let mut repointed = subject.clone();
    repointed.edge.parent_pr_number = 999;
    assert_eq!(validate_subject(&repointed).unwrap_err().code, "edge_declaration_invalid");

    let mut repinned = subject.clone();
    repinned.edge.declared_parent_head_sha = Some(hex40(9));
    assert_eq!(validate_subject(&repinned).unwrap_err().code, "edge_declaration_invalid");
}

/// Result fan-in runs the plan's semantic validator and pins the exact
/// stack-local profile before trusting any digest (#13360 root cause 2).
#[test]
fn ci_stack_increment_forged_or_foreign_plan_refused_at_fan_in() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);

    // A different but internally valid profile: only the stack-local profile
    // can back a stack result.
    let mut foreign = plan.clone();
    foreign.requested_profile = "pr_fast".to_string();
    let error = compile_result(StackResultInput {
        plan: foreign,
        ..result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)])
    })
    .unwrap_err();
    assert_eq!(error.code, "plan_untrusted");

    // An internally inconsistent plan: one governed gate lost its row, so
    // the plan's own semantic validator refuses it before any digest is
    // consulted.
    let mut inconsistent = plan.clone();
    inconsistent.rows.remove(0);
    let error = compile_result(StackResultInput {
        plan: inconsistent,
        ..result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)])
    })
    .unwrap_err();
    assert_eq!(error.code, "plan_untrusted");
}

/// A missing parent-pass report can never publish a green advisory, even
/// with every child row passing (#13360 root cause 3).
#[test]
fn ci_stack_increment_missing_parent_pass_is_not_proven() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let mut input = result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]);
    input.parent_state = None;
    let result = compile_result(input).unwrap();
    assert_eq!(result.parent_prerequisite_state, ParentPrerequisiteState::NotReported);
    assert_eq!(result.child_increment_status, ChildIncrementStatus::CurrentSuccess);
    assert_eq!(result.context_status, ContextStatus::NotProven);
}

/// Contradictory runner evidence (`passed: true` with a non-zero or absent
/// exit code) is refused instead of counted as success (#13360 root cause 4).
#[test]
fn ci_stack_increment_contradicted_pass_evidence_refused() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);

    let mut nonzero = passing_observation(TEST_GATE);
    nonzero.exit_code = Some(1);
    let error = compile_result(result_input_current(&subject, &plan, vec![nonzero])).unwrap_err();
    assert_eq!(error.code, "run_evidence_contradicted");

    let mut without_exit = passing_observation(TEST_GATE);
    without_exit.exit_code = None;
    let error =
        compile_result(result_input_current(&subject, &plan, vec![without_exit])).unwrap_err();
    assert_eq!(error.code, "run_evidence_contradicted");
}

/// The semantic result validator reconciles rows, aggregates, digests, and
/// the published status, so a hand-authored forged artifact cannot pass
/// `validate` or drive `explain` to success (#13360 root cause 5).
#[test]
fn ci_stack_increment_forged_result_artifact_refused() {
    let subject = compiled_subject();
    let plan = compiled_plan(&subject);
    let green =
        compile_result(result_input_current(&subject, &plan, vec![passing_observation(TEST_GATE)]))
            .unwrap();
    assert_eq!(validate_result(&green).unwrap(), ());

    // A missing parent report hidden under a forged green status refuses.
    let mut forged_status = green.clone();
    forged_status.context_status = ContextStatus::CurrentSuccess;
    forged_status.parent_prerequisite_state = ParentPrerequisiteState::NotReported;
    assert!(validate_result(&forged_status).is_err());

    // Failing rows re-aggregate to CurrentFailure: a stored success status
    // no longer reconciles.
    let mut forged_rows = green.clone();
    forged_rows.rows.insert(STATIC_GATE.to_string(), StackRowResult::RunFailed { exit_code: 2 });
    assert!(validate_result(&forged_rows).is_err());

    // Identity fields stay validated even on an otherwise consistent body.
    let mut bad_digest = green.clone();
    bad_digest.subject_digest = "deadbeef".to_string();
    assert_eq!(validate_result(&bad_digest).unwrap_err().code, "malformed_result");
    let mut promoted = green;
    promoted.protected_main_state = "CURRENT_SUCCESS".to_string();
    assert_eq!(validate_result(&promoted).unwrap_err().code, "protected_main_promotion");
}
