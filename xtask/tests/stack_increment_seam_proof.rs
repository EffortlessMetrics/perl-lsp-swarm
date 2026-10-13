//! Integration proof for the public stack-increment trust seams.

use anyhow::{Result, bail, ensure};
use xtask::ci_route_plan::{
    CiRoutePlanV1, ExpansionStatus, LifecycleDisposition, LifecycleState, PlannedOutcome,
    PolicyRole, Resolution, RouteDispositionInput, RouteExecutionIdentity,
    RouteProfileExpansionInput, SelectorPlacement,
};
use xtask::stack_increment::{
    ChildDelta, ContextStatus, DeltaPath, DeltaStatus, EdgeKind, PROTECTED_MAIN_NOT_EVALUATED,
    ParentPrerequisiteState, RunIdentity, STACK_ADVISORY_CONTEXT_NAME, STACK_LOCAL_PROFILE,
    StackEndpoint, StackGateScope, StackIncrementSubjectV1, StackObservation, StackPlanRequest,
    StackResultInput, StackSubjectInput, TrustContext, check_declared_scope, compile_result,
    compile_stack_plan, compile_subject, delta_fingerprint, derive_selectors,
    parse_stack_edge_declaration, render_explanation, stack_plan_digest, validate_result,
    validate_subject,
};

const PARENT_HEAD: &str = "1111111111111111111111111111111111111111";
const PARENT_TREE: &str = "2222222222222222222222222222222222222222";
const CHILD_HEAD: &str = "3333333333333333333333333333333333333333";
const CHILD_TREE: &str = "4444444444444444444444444444444444444444";
const SOURCE_GATE: &str = "stack-source-tests";
const DESTINATION_GATE: &str = "stack-destination-tests";
const SOURCE_PATH: &str = "crates/source/src/lib.rs";
const DESTINATION_PATH: &str = "crates/destination/src/lib.rs";

fn digest(byte: u8) -> String {
    std::iter::repeat_n(char::from(byte), 64).collect()
}

fn endpoint(pr_number: u64, branch: &str, head_sha: &str, head_tree: &str) -> StackEndpoint {
    StackEndpoint {
        pr_number,
        issue_node_id: format!("node-{pr_number}"),
        branch: branch.to_string(),
        head_sha: head_sha.to_string(),
        head_tree: head_tree.to_string(),
    }
}

fn renamed_delta() -> ChildDelta {
    let paths = vec![DeltaPath {
        status: DeltaStatus::Renamed,
        path: DESTINATION_PATH.to_string(),
        renamed_from: Some(SOURCE_PATH.to_string()),
    }];
    ChildDelta {
        bound_parent_tree: PARENT_TREE.to_string(),
        bound_child_tree: CHILD_TREE.to_string(),
        fingerprint: delta_fingerprint(PARENT_TREE, CHILD_TREE, &paths),
        paths,
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
        native_tier: STACK_LOCAL_PROFILE.to_string(),
        quarantine: None,
        detail: None,
    }
}

fn plan_request(subject: &StackIncrementSubjectV1) -> StackPlanRequest {
    let denominator = vec![DESTINATION_GATE.to_string(), SOURCE_GATE.to_string()];
    StackPlanRequest {
        subject: subject.clone(),
        expansion: RouteProfileExpansionInput {
            requested_profile: STACK_LOCAL_PROFILE.to_string(),
            included_native_tiers: vec![STACK_LOCAL_PROFILE.to_string()],
            semantic_fingerprint: digest(b'a'),
            policy_digest: digest(b'b'),
            denominator,
            resolution: ExpansionStatus::Complete,
            detail: None,
        },
        dispositions: vec![active_disposition(DESTINATION_GATE), active_disposition(SOURCE_GATE)],
        disposition_digest: digest(b'c'),
        workflow_digest: digest(b'd'),
        gate_scopes: vec![
            StackGateScope {
                gate_id: SOURCE_GATE.to_string(),
                path_prefixes: vec!["crates/source/".to_string()],
            },
            StackGateScope {
                gate_id: DESTINATION_GATE.to_string(),
                path_prefixes: vec!["crates/destination/".to_string()],
            },
        ],
        execution: vec![
            RouteExecutionIdentity {
                gate_id: DESTINATION_GATE.to_string(),
                command: format!("cargo test -p xtask --locked {DESTINATION_GATE}"),
                timeout_seconds: 600,
            },
            RouteExecutionIdentity {
                gate_id: SOURCE_GATE.to_string(),
                command: format!("cargo test -p xtask --locked {SOURCE_GATE}"),
                timeout_seconds: 600,
            },
        ],
    }
}

fn observation(plan: &CiRoutePlanV1, gate_id: &str) -> Result<StackObservation> {
    let row = plan
        .rows
        .iter()
        .find(|row| row.gate_id == gate_id)
        .ok_or_else(|| anyhow::anyhow!("missing planned row for {gate_id}"))?;
    let PlannedOutcome::Run { command, timeout_seconds, .. } = &row.outcome else {
        bail!("expected a runnable row for {gate_id}, found {:?}", row.outcome);
    };
    Ok(StackObservation {
        gate_id: gate_id.to_string(),
        identity: RunIdentity { command: command.clone(), timeout_seconds: *timeout_seconds },
        passed: true,
        exit_code: Some(0),
        output_digest: Some(digest(b'e')),
        instrument_failed: false,
    })
}

#[test]
fn public_stack_increment_pipeline_has_accepting_and_refusing_seams() -> Result<()> {
    let edge = parse_stack_edge_declaration(
        "stack-edge: dependency=programme_dependency parent-pr=100 \
         scope=crates/source/,crates/destination/",
    )?;
    ensure!(edge.dependency == EdgeKind::ProgrammeDependency);
    ensure!(edge.parent_pr_number == 100);
    ensure!(edge.scope_paths.len() == 2);

    let delta = renamed_delta();
    let subject = compile_subject(StackSubjectInput {
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        event_id: Some("seam-proof".to_string()),
        parent: endpoint(100, "train/100", PARENT_HEAD, PARENT_TREE),
        child: endpoint(101, "train/101-child", CHILD_HEAD, CHILD_TREE),
        edge: Some(edge),
        child_base_expected_head_sha: PARENT_HEAD.to_string(),
        observed_parent_head_sha: PARENT_HEAD.to_string(),
        observed_child_head_sha: CHILD_HEAD.to_string(),
        trust: TrustContext { same_repository_declared: true, external_context_admitted: false },
        history: xtask::stack_increment::RelatedHistory::Ancestor,
        delta,
    })?;
    validate_subject(&subject)?;
    ensure!(subject.delta.paths[0].renamed_from.as_deref() == Some(SOURCE_PATH));

    let scope_ok = check_declared_scope(&subject.delta, &subject.edge);
    ensure!(scope_ok.is_ok(), "renamed source and destination should be in scope");
    let mut narrow_edge = subject.edge.clone();
    narrow_edge.scope_paths = vec!["crates/destination/".to_string()];
    let scope_error = check_declared_scope(&subject.delta, &narrow_edge)
        .err()
        .ok_or_else(|| anyhow::anyhow!("out-of-scope rename source was accepted"))?;
    ensure!(scope_error.0 == "undeclared_delta_surface");

    let mut malformed_subject = subject.clone();
    let row = malformed_subject
        .delta
        .paths
        .first_mut()
        .ok_or_else(|| anyhow::anyhow!("renamed fixture has no delta row"))?;
    row.renamed_from = None;
    let subject_error = validate_subject(&malformed_subject)
        .err()
        .ok_or_else(|| anyhow::anyhow!("rename without source was accepted"))?;
    ensure!(subject_error.code == "malformed_subject");

    let request = plan_request(&subject);
    let selectors =
        derive_selectors(&subject.delta, &request.expansion.denominator, &request.gate_scopes);
    ensure!(selectors.len() == 2);
    ensure!(selectors.iter().all(|selector| selector.placement == SelectorPlacement::Selected));

    let plan = compile_stack_plan(request)?;
    let plan_digest = stack_plan_digest(&plan)?;
    let result = compile_result(StackResultInput {
        subject: subject.clone(),
        plan: plan.clone(),
        plan_digest,
        observations: vec![observation(&plan, SOURCE_GATE)?, observation(&plan, DESTINATION_GATE)?],
        parent_state: Some(ParentPrerequisiteState::Passed),
        observed_parent_head_sha: PARENT_HEAD.to_string(),
        observed_child_head_sha: CHILD_HEAD.to_string(),
        reproduce_command: "cargo xtask ci-stack explain --result result.json".to_string(),
    })?;
    ensure!(result.context_status == ContextStatus::CurrentSuccess);
    validate_result(&result)?;

    let mut rowless = result.clone();
    rowless.rows.clear();
    rowless.planned_gate_count = 0;
    let rowless_error = validate_result(&rowless)
        .err()
        .ok_or_else(|| anyhow::anyhow!("rowless zero-denominator result was accepted"))?;
    ensure!(rowless_error.code == "malformed_result");

    let mut promoted = result.clone();
    promoted.protected_main_state = "CURRENT_SUCCESS".to_string();
    let promotion_error = validate_result(&promoted)
        .err()
        .ok_or_else(|| anyhow::anyhow!("protected-main promotion was accepted"))?;
    ensure!(promotion_error.code == "protected_main_promotion");

    let explanation = render_explanation(&result);
    ensure!(explanation.contains(STACK_ADVISORY_CONTEXT_NAME));
    ensure!(explanation.contains(PROTECTED_MAIN_NOT_EVALUATED));
    ensure!(explanation.contains(SOURCE_GATE) && explanation.contains(DESTINATION_GATE));
    ensure!(explanation.contains("Reproduce:"));
    Ok(())
}
