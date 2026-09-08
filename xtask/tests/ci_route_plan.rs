//! Domain falsifiers for `ci_route_plan.v1` (#9148): one negative control
//! per required control list entry, plus the baseline compile. Authority
//! derivations stay in their owners; these tests use synthetic projected
//! authority results and verify the domain's composition and validation
//! contracts. The real #10176/#10178 -> adapter seam is covered by the
//! `route_plan_seam_tests` in `xtask/src/tasks/gates/planning_types.rs`.

use xtask::ci_route_plan::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CiRoutePlanV1,
    CompileRoutePlanInput, ExpansionStatus, GateSelectorInput, LifecycleDisposition,
    LifecycleState, PlannedOutcome, PolicyRole, Resolution, RouteDispositionInput,
    RouteExecutionIdentity, RouteProfileExpansionInput, RouteScopeEvidence, RouteSelectionEvidence,
    RouteSubjectRef, ScopedIdentity, SelectorPlacement, SelectorProof, SelectorRole,
};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn subject() -> RouteSubjectRef {
    RouteSubjectRef {
        kind: "pull_request".to_string(),
        head_sha: SHA_A.to_string(),
        base_sha: Some(SHA_B.to_string()),
        subject_digest: DIGEST_A.to_string(),
    }
}

fn selection() -> RouteSelectionEvidence {
    RouteSelectionEvidence {
        base: SHA_B.to_string(),
        scope_ok: true,
        fallback_used: false,
        fallback_reason: None,
        package_args: vec!["-p".to_string(), "perl-parser".to_string()],
        scope: None,
        selector_digest: DIGEST_A.to_string(),
    }
}

fn expansion(profile: &str, tiers: &[&str], denominator: &[&str]) -> RouteProfileExpansionInput {
    RouteProfileExpansionInput {
        requested_profile: profile.to_string(),
        included_native_tiers: tiers.iter().map(|tier| tier.to_string()).collect(),
        semantic_fingerprint: DIGEST_B.to_string(),
        policy_digest: DIGEST_C.to_string(),
        denominator: denominator.iter().map(|gate| gate.to_string()).collect(),
        resolution: ExpansionStatus::Complete,
        detail: None,
    }
}

fn active_disposition(gate_id: &str, tier: &str, role: PolicyRole) -> RouteDispositionInput {
    RouteDispositionInput {
        gate_id: gate_id.to_string(),
        policy_role: role,
        lifecycle: LifecycleDisposition {
            state: LifecycleState::Active,
            resolution: Resolution::Current,
        },
        native_tier: tier.to_string(),
        quarantine: None,
        detail: None,
    }
}

fn quarantined_disposition(gate_id: &str, resolution: Resolution) -> RouteDispositionInput {
    RouteDispositionInput {
        gate_id: gate_id.to_string(),
        policy_role: PolicyRole::Advisory,
        lifecycle: LifecycleDisposition { state: LifecycleState::Quarantined, resolution },
        native_tier: "pr_fast".to_string(),
        quarantine: None,
        detail: match resolution {
            Resolution::Current => None,
            _ => Some("quarantine evidence problem".to_string()),
        },
    }
}

fn non_runnable_disposition(gate_id: &str, state: LifecycleState) -> RouteDispositionInput {
    RouteDispositionInput {
        gate_id: gate_id.to_string(),
        policy_role: PolicyRole::Required,
        lifecycle: LifecycleDisposition { state, resolution: Resolution::Current },
        native_tier: "pr_fast".to_string(),
        quarantine: None,
        detail: None,
    }
}

fn selected(gate_id: &str, proof: Option<SelectorProof>) -> GateSelectorInput {
    GateSelectorInput {
        gate_id: gate_id.to_string(),
        placement: SelectorPlacement::Selected,
        role: Some(SelectorRole::AlwaysOn),
        reason: "selected by selector".to_string(),
        proof,
    }
}

fn skipped(gate_id: &str, proof: Option<SelectorProof>) -> GateSelectorInput {
    GateSelectorInput {
        gate_id: gate_id.to_string(),
        placement: SelectorPlacement::Skipped,
        role: Some(SelectorRole::RustScoped),
        reason: "scope selector decided".to_string(),
        proof,
    }
}

fn execution(gate_id: &str) -> RouteExecutionIdentity {
    RouteExecutionIdentity {
        gate_id: gate_id.to_string(),
        command: format!("run {gate_id}"),
        timeout_seconds: 60,
    }
}

/// Baseline: two governed denominator gates, one selected with proof, one
/// skipped with positive non-applicability proof.
fn baseline_input() -> CompileRoutePlanInput {
    CompileRoutePlanInput {
        subject: subject(),
        expansion: expansion("merge_gate", &["pr_fast", "merge_gate"], &["fmt_gate", "unit_gate"]),
        dispositions: vec![
            active_disposition("fmt_gate", "pr_fast", PolicyRole::Required),
            active_disposition("unit_gate", "pr_fast", PolicyRole::Advisory),
        ],
        disposition_digest: DIGEST_B.to_string(),
        workflow_digest: DIGEST_C.to_string(),
        selectors: vec![
            selected("fmt_gate", Some(SelectorProof::Applicable)),
            skipped("unit_gate", Some(SelectorProof::NotApplicableToSubject)),
        ],
        selection: selection(),
        execution: vec![execution("fmt_gate"), execution("unit_gate")],
    }
}

#[test]
fn baseline_compiles_one_row_per_denominator_gate() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("compile");
    assert_eq!(plan.schema, CI_ROUTE_PLAN_SCHEMA);
    assert_eq!(plan.producer, CI_ROUTE_PLAN_PRODUCER);
    assert_eq!(plan.requested_profile, "merge_gate");
    assert_eq!(plan.summary.governed, 2);
    assert_eq!(plan.summary.run, 1);
    assert_eq!(plan.summary.scoped_noop, 1);
    assert_eq!(plan.summary.quarantined, 0);
    assert_eq!(plan.summary.error, 0);
    // The aggregate merge_gate profile inherited the pr_fast gate.
    assert_eq!(plan.rows[0].native_tier, "pr_fast");
    assert_eq!(plan.rows[1].native_tier, "pr_fast");
    assert!(matches!(&plan.rows[0].outcome, PlannedOutcome::Run { .. }));
    assert!(matches!(&plan.rows[1].outcome, PlannedOutcome::ScopedNoop { .. }));
    plan.validate().expect("valid");
}

// Control 1: one gate from the governed denominator is omitted.
#[test]
fn omitted_denominator_gate_fails_validation() {
    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("compile");
    plan.rows.remove(1);
    plan.summary.governed = 1;
    plan.summary.scoped_noop = 0;
    plan.summary.by_policy_role.remove(&PolicyRole::Advisory);
    let error = plan.validate().expect_err("omission must not validate");
    assert!(error.contains("rows"), "{error}");
}

// Controls 1+2: duplicates fail closed at compile and at validation.
#[test]
fn duplicate_gate_fails_closed_everywhere() {
    let mut input = baseline_input();
    input.dispositions.push(active_disposition("fmt_gate", "pr_fast", PolicyRole::Required));
    let error = CiRoutePlanV1::compile(input).expect_err("duplicate disposition must fail");
    assert!(error.contains("duplicate disposition"), "{error}");

    let mut input = baseline_input();
    input.expansion.denominator = vec!["fmt_gate".to_string(), "fmt_gate".to_string()];
    let error = CiRoutePlanV1::compile(input).expect_err("duplicate denominator must fail");
    assert!(error.contains("duplicate"), "{error}");

    let mut input = baseline_input();
    input.selectors.push(selected("fmt_gate", Some(SelectorProof::Applicable)));
    let error = CiRoutePlanV1::compile(input).expect_err("duplicate selector must fail");
    assert!(error.contains("duplicate selector"), "{error}");
}

// Control 3: an aggregate profile cannot lose an inherited concrete-tier
// gate — the denominator is consumed verbatim from the expansion, every
// denominator gate receives exactly one row, and a gate with no selector
// input cannot silently disappear.
#[test]
fn aggregate_profile_cannot_lose_inherited_gate() {
    let input = baseline_input();
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    let inherited =
        plan.rows.iter().any(|row| row.native_tier == "pr_fast" && row.gate_id == "fmt_gate");
    assert!(inherited, "merge_gate plan must keep the inherited pr_fast gate");

    // A denominator gate with no selector input at all still receives its
    // row — as a typed error, never an omission.
    let mut input = baseline_input();
    input.selectors.retain(|selector| selector.gate_id == "unit_gate");
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    assert_eq!(plan.summary.governed, 2);
    assert_eq!(plan.summary.error, 1);
    match &plan.rows[0].outcome {
        PlannedOutcome::Error { code, .. } => assert_eq!(code, "selector_evidence_missing"),
        other => panic!("expected error outcome, got {other:?}"),
    }
    // The gate that kept its positive non-applicability proof is unaffected.
    assert!(matches!(&plan.rows[1].outcome, PlannedOutcome::ScopedNoop { .. }));
}

// Control 4: a quarantined gate compiled through the skipped path stays
// `quarantined`, never `scoped_noop`.
#[test]
fn skipped_quarantined_gate_never_becomes_scoped_noop() {
    let mut input = baseline_input();
    input.dispositions[0] = RouteDispositionInput {
        gate_id: "fmt_gate".to_string(),
        policy_role: PolicyRole::Advisory,
        lifecycle: LifecycleDisposition {
            state: LifecycleState::Quarantined,
            resolution: Resolution::Current,
        },
        native_tier: "pr_fast".to_string(),
        quarantine: Some(xtask::ci_route_plan::RouteQuarantineEvidence {
            owner: "ci-owner".to_string(),
            owner_issue: Some("10176".to_string()),
            reason_token: "secondary_failure".to_string(),
            review_after: "2030-01-01".to_string(),
        }),
        detail: None,
    };
    // The runner placed the quarantined gate in the skipped population with
    // positive non-applicability proof.
    input.selectors[0] = skipped("fmt_gate", Some(SelectorProof::NotApplicableToSubject));
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    assert_eq!(plan.summary.quarantined, 1);
    assert_eq!(plan.summary.run, 0);
    // fmt_gate must be quarantined despite its skip; the other gate keeps its
    // legitimate scoped_noop, so exactly one scoped_noop remains.
    assert_eq!(plan.summary.scoped_noop, 1);
    assert!(matches!(&plan.rows[1].outcome, PlannedOutcome::ScopedNoop { .. }));
    match &plan.rows[0].outcome {
        PlannedOutcome::Quarantined { owner, .. } => assert_eq!(owner, "ci-owner"),
        other => panic!("expected quarantined outcome, got {other:?}"),
    }
}

// Control 5: a free-form skip without positive selector evidence is an
// error row, never pass/non-applicable.
#[test]
fn generic_skip_without_selector_proof_is_error() {
    let mut input = baseline_input();
    input.selectors[1] = skipped("unit_gate", None);
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    assert_eq!(plan.summary.scoped_noop, 0);
    assert_eq!(plan.summary.error, 1);
    assert_eq!(plan.rows[1].applicability, Applicability::Unknown);
    match &plan.rows[1].outcome {
        PlannedOutcome::Error { code, .. } => assert_eq!(code, "selector_evidence_missing"),
        other => panic!("expected error outcome, got {other:?}"),
    }
}

// Control 6: the accepted policy role never depends on the requested
// profile; release/commit placement is a profile fact, not a role.
#[test]
fn policy_role_is_independent_of_requested_profile() {
    let base = baseline_input();
    let commit_input = CompileRoutePlanInput {
        expansion: expansion("commit", &["commit"], &["fmt_gate"]),
        selectors: vec![selected("fmt_gate", Some(SelectorProof::Applicable))],
        ..base.clone()
    };
    let mut release_expansion = expansion("release", &["release"], &["fmt_gate"]);
    release_expansion.resolution = ExpansionStatus::Unsupported;
    release_expansion.detail = Some("no reviewed composition".to_string());
    let release_input = CompileRoutePlanInput { expansion: release_expansion, ..base };
    let commit_plan = CiRoutePlanV1::compile(commit_input).expect("commit compile");
    // Release without a reviewed composition is typed non-success, never a
    // silent alias of another profile (control 11 applies here too).
    let error = CiRoutePlanV1::compile(release_input).expect_err("release must be unsupported");
    assert!(error.contains("not consumable"), "{error}");
    assert_eq!(commit_plan.rows[0].policy_role, PolicyRole::Required);
}

// Control 7: missing, expired, and ownerless quarantine evidence stay
// action-required error rows; a current quarantine projection without
// evidence is refused outright.
#[test]
fn expired_invalid_and_ownerless_quarantine_cannot_become_active() {
    for resolution in [Resolution::Expired, Resolution::Invalid] {
        let mut input = baseline_input();
        input.dispositions[1] = quarantined_disposition("unit_gate", resolution);
        // Even with a selected placement and applicable proof.
        input.selectors[1] = selected("unit_gate", Some(SelectorProof::Applicable));
        let plan = CiRoutePlanV1::compile(input).expect("compile");
        match &plan.rows[1].outcome {
            PlannedOutcome::Error { code, .. } => {
                assert!(code == "disposition_expired" || code == "disposition_invalid");
            }
            other => panic!("expected error outcome for {resolution:?}, got {other:?}"),
        }
        assert_eq!(plan.summary.run, 1, "only the healthy gate runs");
    }

    // A projected current quarantine without evidence is a broken
    // projection: compile refuses rather than inventing an outcome.
    let mut input = baseline_input();
    input.dispositions[1] = quarantined_disposition("unit_gate", Resolution::Current);
    input.selectors[1] = skipped("unit_gate", Some(SelectorProof::NotApplicableToSubject));
    let error = CiRoutePlanV1::compile(input).expect_err("missing quarantine evidence must fail");
    assert!(error.contains("no quarantine evidence"), "{error}");
}

// Control 8: dormant, retired, and blocked lifecycles never become runnable,
// even with a selected placement and applicable proof.
#[test]
fn non_runnable_lifecycles_never_become_runnable() {
    for state in [LifecycleState::Dormant, LifecycleState::Retired, LifecycleState::Blocked] {
        let mut input = baseline_input();
        input.dispositions[1] = non_runnable_disposition("unit_gate", state);
        input.selectors[1] = selected("unit_gate", Some(SelectorProof::Applicable));
        let plan = CiRoutePlanV1::compile(input).expect("compile");
        assert_eq!(plan.summary.run, 1, "{state:?} gate must not run");
        match &plan.rows[1].outcome {
            PlannedOutcome::Error { code, .. } => assert_eq!(code, "lifecycle_non_runnable"),
            other => panic!("expected error outcome for {state:?}, got {other:?}"),
        }
    }
}

// Control 9: missing selector evidence cannot become `scoped_noop`; the
// selected-and-contradicted and skipped-and-contradicted placements are
// typed contradictions, not silent passes.
#[test]
fn missing_or_contradictory_selector_evidence_is_never_scoped_noop() {
    // No selector input at all.
    let mut input = baseline_input();
    input.selectors.clear();
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    assert_eq!(plan.summary.error, 2);
    assert_eq!(plan.summary.scoped_noop, 0);

    // Skipped placement with applicable proof contradicts itself.
    let mut input = baseline_input();
    input.selectors[0] = skipped("fmt_gate", Some(SelectorProof::Applicable));
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    match &plan.rows[0].outcome {
        PlannedOutcome::Error { code, .. } => assert_eq!(code, "selector_contradiction"),
        other => panic!("expected contradiction error, got {other:?}"),
    }

    // Selected placement with non-applicable proof contradicts itself.
    let mut input = baseline_input();
    input.selectors[1] = selected("unit_gate", Some(SelectorProof::NotApplicableToSubject));
    let plan = CiRoutePlanV1::compile(input).expect("compile");
    match &plan.rows[1].outcome {
        PlannedOutcome::Error { code, .. } => assert_eq!(code, "selector_contradiction"),
        other => panic!("expected contradiction error, got {other:?}"),
    }
}

// Control 10: contradictory applicability/outcome combinations must not
// validate.
#[test]
fn contradictory_applicability_outcome_fails_validation() {
    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    plan.rows[0].applicability = Applicability::NotApplicable;
    let error = plan.validate().expect_err("contradiction must fail");
    assert!(error.contains("run requires applicable"), "{error}");

    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    plan.rows[1].applicability = Applicability::Applicable;
    let error = plan.validate().expect_err("contradiction must fail");
    assert!(error.contains("scoped_noop requires not-applicable"), "{error}");

    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    plan.rows[0].selector_placement = SelectorPlacement::Skipped;
    let error = plan.validate().expect_err("placement contradiction must fail");
    assert!(error.contains("selected placement"), "{error}");
}

// Control 11: unknown schema, profile, disposition shapes, or error codes
// fail closed; unknown outcome kinds cannot even deserialize.
#[test]
fn unknown_identities_fail_closed() {
    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    plan.schema = "ci_route_plan.v2".to_string();
    assert!(plan.validate().is_err());

    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    plan.requested_profile = "bogus_profile".to_string();
    let error = plan.validate().expect_err("unknown profile must fail");
    assert!(error.contains("unknown requested profile"), "{error}");

    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    // Fabricate a free-form error code on the run row; the closed vocabulary
    // must refuse it at validation.
    plan.rows[0].outcome = PlannedOutcome::Error {
        code: "free_form_code".to_string(),
        message: "fabricated error".to_string(),
    };
    plan.summary.run = 0;
    plan.summary.error = 1;
    let error = plan.validate().expect_err("free-form code must fail");
    assert!(error.contains("unknown error code"), "{error}");

    let serialized =
        serde_json::to_string(&CiRoutePlanV1::compile(baseline_input()).expect("plan"))
            .expect("serialize");
    let tampered = serialized.replace("\"kind\":\"run\"", "\"kind\":\"teleport\"");
    assert_ne!(tampered, serialized, "outcome tag must appear in serialized plan");
    assert!(serde_json::from_str::<CiRoutePlanV1>(&tampered).is_err());
}

// Control 12: the planning payload cannot carry live enforcement or
// execution-result observations — those axes belong to #9156/#9159.
#[test]
fn payload_carries_no_enforcement_or_execution_result() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("plan");
    let serialized = serde_json::to_string(&plan).expect("serialize");
    for forbidden in ["\"enforcement\"", "\"conclusion\"", "\"check_run\"", "\"result\""] {
        assert!(!serialized.contains(forbidden), "planning payload must not carry {forbidden}");
    }
}

#[test]
fn unsupported_and_invalid_expansions_fail_closed_at_compile() {
    for resolution in [ExpansionStatus::Unsupported, ExpansionStatus::Invalid] {
        let mut input = baseline_input();
        input.expansion.resolution = resolution;
        input.expansion.detail = Some("projection detail".to_string());
        let error = CiRoutePlanV1::compile(input).expect_err("non-complete expansion must fail");
        assert!(error.contains("not consumable"), "{error}");
    }
}

#[test]
fn selection_evidence_shape_is_enforced() {
    let mut input = baseline_input();
    input.selection.fallback_used = true;
    input.selection.fallback_reason = None;
    let error = CiRoutePlanV1::compile(input).expect_err("fallback shape must fail");
    assert!(error.contains("fallback_used requires fallback_reason"), "{error}");

    let mut input = baseline_input();
    input.selection.selector_digest = "AB".to_string();
    let error = CiRoutePlanV1::compile(input).expect_err("digest shape must fail");
    assert!(error.contains("selector_digest"), "{error}");
}

// Scope evidence produced for another subject cannot back this subject's
// selector-proved outcomes, even when every field is individually
// well-formed: a `scoped_noop` must never be built from stale evidence.
#[test]
fn stale_scope_evidence_from_another_subject_refuses() {
    let mut input = baseline_input();
    // Scope computed for head SHA B while the route subject is head SHA A.
    input.selection.scope = Some(RouteScopeEvidence {
        head_sha: SHA_B.to_string(),
        diff_class: "rust".to_string(),
        direct_crates: vec![ScopedIdentity {
            name: "perl-parser".to_string(),
            reason: "direct".to_string(),
        }],
        reverse_dependencies: vec![],
        architecture_wideners: vec![],
        risk_tags: vec![],
    });
    input.selectors[1] = skipped("unit_gate", Some(SelectorProof::NotApplicableToSubject));
    let error = CiRoutePlanV1::compile(input).expect_err("stale scope must refuse");
    assert!(
        error.contains("scope head SHA") && error.contains("route subject head SHA"),
        "{error}"
    );

    // The same scope bound to the exact subject validates.
    let mut input = baseline_input();
    input.selection.scope = Some(RouteScopeEvidence {
        head_sha: SHA_A.to_string(),
        diff_class: "rust".to_string(),
        direct_crates: vec![ScopedIdentity {
            name: "perl-parser".to_string(),
            reason: "direct".to_string(),
        }],
        reverse_dependencies: vec![],
        architecture_wideners: vec![],
        risk_tags: vec![],
    });
    input.selectors[1] = skipped("unit_gate", Some(SelectorProof::NotApplicableToSubject));
    let plan = CiRoutePlanV1::compile(input).expect("bound scope validates");
    assert!(matches!(&plan.rows[1].outcome, PlannedOutcome::ScopedNoop { .. }));
}
