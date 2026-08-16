use xtask::ci_route_plan::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CiRoutePlanV1,
    CompileRoutePlanInput, LegacyGatePlanInput, LegacyGatePolicyInput, LegacyGatePolicyRow,
    LegacyPlannedGate, LegacyPlanningRole, LegacyScopeInput, LegacySkippedGate, PlannedOutcome,
    RouteSubjectRef, ScopedIdentity,
};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn input(reverse_policy: bool) -> CompileRoutePlanInput {
    let selected = vec![LegacyPlannedGate {
        name: "fmt".to_string(),
        role: LegacyPlanningRole::AlwaysOn,
        reason: "always on".to_string(),
    }];
    let skipped = vec![LegacySkippedGate {
        name: "unit_scoped".to_string(),
        role: Some(LegacyPlanningRole::RustScoped),
        reason: "no affected Rust package".to_string(),
    }];
    let mut policy = vec![
        LegacyGatePolicyRow {
            name: "fmt".to_string(),
            tier: "pr_fast".to_string(),
            required: true,
            command: "cargo fmt --all -- --check".to_string(),
            timeout_seconds: 60,
            quarantine: false,
            quarantine_owner_issue: None,
            quarantine_review_after: None,
        },
        LegacyGatePolicyRow {
            name: "unit_scoped".to_string(),
            tier: "pr_fast".to_string(),
            required: true,
            command: "cargo test --lib".to_string(),
            timeout_seconds: 600,
            quarantine: false,
            quarantine_owner_issue: None,
            quarantine_review_after: None,
        },
    ];
    if reverse_policy {
        policy.reverse();
    }
    CompileRoutePlanInput {
        subject: RouteSubjectRef {
            kind: "pull_request".to_string(),
            head_sha: SHA_A.to_string(),
            base_sha: Some(SHA_B.to_string()),
            subject_digest: DIGEST_A.to_string(),
        },
        plan: LegacyGatePlanInput {
            tier: "pr_fast".to_string(),
            base: SHA_B.to_string(),
            scope_ok: true,
            fallback_used: false,
            fallback_reason: None,
            package_args: vec!["-p".to_string(), "perl-parser".to_string()],
            scope: Some(LegacyScopeInput {
                head_sha: SHA_A.to_string(),
                diff_class: "code".to_string(),
                direct_crates: vec![ScopedIdentity {
                    name: "perl-parser".to_string(),
                    reason: "changed source".to_string(),
                }],
                reverse_dependencies: vec![ScopedIdentity {
                    name: "perl-lsp-rs".to_string(),
                    reason: "depends on perl-parser".to_string(),
                }],
                architecture_wideners: vec![ScopedIdentity {
                    name: "perl-lsp-rs".to_string(),
                    reason: "parser_to_lsp".to_string(),
                }],
                risk_tags: vec!["parser-recovery".to_string()],
            }),
            selected,
            skipped,
        },
        policy: LegacyGatePolicyInput { gates: policy },
        policy_digest: DIGEST_B.to_string(),
        workflow_digest: DIGEST_C.to_string(),
        selector_digest: DIGEST_A.to_string(),
    }
}

#[test]
fn legacy_plan_compiles_every_governed_gate() {
    let plan = CiRoutePlanV1::compile(input(false)).expect("compile route plan");
    assert_eq!(plan.schema, CI_ROUTE_PLAN_SCHEMA);
    assert_eq!(plan.producer, CI_ROUTE_PLAN_PRODUCER);
    assert_eq!(plan.summary.governed, 2);
    assert_eq!(plan.summary.run, 1);
    assert_eq!(plan.summary.scoped_noop, 1);
    assert_eq!(plan.rows[0].gate_id, "fmt");
    assert_eq!(plan.rows[1].gate_id, "unit_scoped");
    assert_eq!(plan.rows[1].applicability, Applicability::NotApplicable);
}

#[test]
fn input_order_cannot_change_canonical_bytes_or_fingerprint() {
    let left = CiRoutePlanV1::compile(input(false)).expect("left");
    let right = CiRoutePlanV1::compile(input(true)).expect("right");
    assert_eq!(left.semantic_fingerprint, right.semantic_fingerprint);
    assert_eq!(
        left.canonical_json().expect("left json"),
        right.canonical_json().expect("right json")
    );
}

#[test]
fn semantic_change_moves_fingerprint() {
    let original = CiRoutePlanV1::compile(input(false)).expect("original");
    let mut changed_input = input(false);
    changed_input.policy.gates[0].command = "cargo fmt --check".to_string();
    let changed = CiRoutePlanV1::compile(changed_input).expect("changed");
    assert_ne!(original.semantic_fingerprint, changed.semantic_fingerprint);
}

#[test]
fn omitted_governed_gate_fails_closed() {
    let mut input = input(false);
    input.plan.skipped.clear();
    let error = CiRoutePlanV1::compile(input).expect_err("missing governed gate must fail");
    assert!(error.contains("omits governed gate"));
}

#[test]
fn selected_and_skipped_gate_cannot_overlap() {
    let mut input = input(false);
    input.plan.skipped[0].name = "fmt".to_string();
    let error = CiRoutePlanV1::compile(input).expect_err("overlap must fail");
    assert!(error.contains("both selected and skipped"));
}

#[test]
fn ownerless_quarantine_becomes_explicit_error_row() {
    let mut input = input(false);
    input.policy.gates[0].quarantine = true;
    let plan = CiRoutePlanV1::compile(input).expect("ownerless quarantine is represented");
    assert_eq!(plan.summary.error, 1);
    assert_eq!(plan.rows[0].applicability, Applicability::Unknown);
    assert!(matches!(
        &plan.rows[0].outcome,
        PlannedOutcome::Error { .. }
    ));
}

#[test]
fn contradictory_applicability_and_outcome_is_rejected() {
    let mut plan = CiRoutePlanV1::compile(input(false)).expect("plan");
    plan.rows[0].applicability = Applicability::NotApplicable;
    let error = plan.validate().expect_err("contradiction must fail");
    assert!(error.contains("contradictory"));
}

#[test]
fn malformed_fingerprint_is_rejected() {
    let mut plan = CiRoutePlanV1::compile(input(false)).expect("plan");
    plan.semantic_fingerprint = "0".repeat(64);
    let error = plan.validate().expect_err("stale fingerprint must fail");
    assert!(error.contains("does not match"));
}

#[test]
fn explain_is_bounded_to_summary_or_one_gate() {
    let plan = CiRoutePlanV1::compile(input(false)).expect("plan");
    let summary = plan.explain(None).expect("summary");
    assert!(summary.contains("\"governed\": 2"));
    let row = plan.explain(Some("fmt")).expect("row");
    assert!(row.contains("cargo fmt --all -- --check"));
    assert!(plan.explain(Some("missing")).is_err());
}

#[test]
fn run_and_noop_outcomes_remain_closed() {
    let plan = CiRoutePlanV1::compile(input(false)).expect("plan");
    assert!(matches!(
        &plan.rows[0].outcome,
        PlannedOutcome::Run { .. }
    ));
    assert!(matches!(
        &plan.rows[1].outcome,
        PlannedOutcome::ScopedNoop { .. }
    ));
}
