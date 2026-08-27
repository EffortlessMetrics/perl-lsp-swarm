//! Canonical compiler-profile test-topology contract proofs (#12411).
//!
//! Two families live here, answering the two topology-owned active rows of
//! `.ci/test-topology/compiler-profile.v1.toml`:
//!
//! - `compiler_profile_test_topology_register_self_*` loads and re-checks
//!   the canonical register itself (advisory row
//!   `test-topology/register-self-consistency`);
//! - every other test exercises one issue-required routing falsifier against
//!   the selector/receipt/fan-in engines (required row
//!   `test-topology/routing-falsifiers`, whose empty filter runs the whole
//!   binary so no control can be omitted from its executed denominator).
//!
//! Every control uses the real public engine surfaces from
//! `xtask::test_topology`; nothing re-implements selection, verdicts, or
//! fan-in. Assertions prefer `matches!` over accessor unwrapping so a shape
//! drift fails loudly instead of panicking.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use xtask::test_topology::model::{
    ExecutionKind, RECEIPT_SCHEMA_VERSION, REGISTER_SCHEMA_VERSION, RouteClass, TargetStatus,
    TopologyRegister, TopologyRow,
};
use xtask::test_topology::receipts::{
    ClassifiedFile, FanInEntry, LibTestCounters, LibTestSummary, ReceiptVerdict, ScopeNamespace,
    ScopedNoopProof, TestTopologyReceipt, build_fan_in, canonical_fan_in_digest, evaluate_run,
    parse_libtest_summaries,
};
use xtask::test_topology::route::{
    CONTROL_PLANE_PREFIXES, DiscoveredTestTarget, check_discovery_membership,
    discover_workspace_test_targets, select_active_scope,
};
use xtask::test_topology::runner::RETRY_CEILING;

// ---------------------------------------------------------------------------
// Synthetic fixtures
// ---------------------------------------------------------------------------

/// Repo root directory for this checkout (`xtask/..`).
fn repo_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

/// Load the canonical checked register shipped in this repository.
fn canonical_register() -> Result<TopologyRegister, Box<dyn std::error::Error>> {
    let path = repo_root().join(".ci").join("test-topology").join("compiler-profile.v1.toml");
    Ok(TopologyRegister::load(&path)?)
}

/// Minimal register fixture around prebuilt rows.
fn synthetic_register(rows: Vec<TopologyRow>) -> TopologyRegister {
    TopologyRegister {
        schema_version: REGISTER_SCHEMA_VERSION.to_owned(),
        cohort: "compiler-profile".to_owned(),
        register_id: "synthetic.v1".to_owned(),
        description: "synthetic routing fixture".to_owned(),
        watch_packages: vec!["xtask".to_owned()],
        namespace_markers: vec!["compiler_profile_test_topology".to_owned()],
        rows,
    }
}

/// Active required row fixture with an exact cargo-test command.
fn active_required_row(target_id: &str, min_work: u32) -> TopologyRow {
    TopologyRow {
        target_id: target_id.to_owned(),
        owner_issue: 12411,
        cohort: "compiler-profile".to_owned(),
        subject: format!("subject of {target_id}"),
        claim_boundary: format!("claim boundary of {target_id}"),
        proof_role: "routing falsifier fixture".to_owned(),
        route_class: RouteClass::RequiredAffected,
        status: TargetStatus::Active,
        candidate_profiles: vec!["pr_focused".to_owned()],
        subjects: vec![format!("crates/{target_id}/")],
        execution: Some(ExecutionKind::CargoTest {
            package: "xtask".to_owned(),
            test_target: Some("compiler_profile_test_topology".to_owned()),
            filter: target_id.replace('-', "_"),
            feature_profile: "--locked".to_owned(),
        }),
        min_work_items: min_work,
        budget_seconds: 120,
        receipt_schema: RECEIPT_SCHEMA_VERSION.to_owned(),
    }
}

/// Dormant row fixture pinning a future leaf identity.
fn dormant_row(target_id: &str) -> TopologyRow {
    TopologyRow {
        target_id: target_id.to_owned(),
        owner_issue: 12499,
        cohort: "compiler-profile".to_owned(),
        subject: format!("subject of {target_id}"),
        claim_boundary: format!("claim boundary of {target_id}"),
        proof_role: "declared future leaf".to_owned(),
        route_class: RouteClass::RequiredAffected,
        status: TargetStatus::DeclaredPending,
        candidate_profiles: Vec::new(),
        subjects: vec![format!("crates/{target_id}-pending/")],
        execution: None,
        min_work_items: 0,
        budget_seconds: 0,
        receipt_schema: RECEIPT_SCHEMA_VERSION.to_owned(),
    }
}

/// Advisory row fixture; never discharges a required obligation.
fn advisory_row(target_id: &str, min_work: u32) -> TopologyRow {
    let mut row = active_required_row(target_id, min_work);
    row.route_class = RouteClass::Advisory;
    row.candidate_profiles = vec!["pr_focused".to_owned(), "local_reproduce".to_owned()];
    row
}

/// Green receipt fixture bound to `head`.
fn pass_receipt(
    target_id: &str,
    head: &str,
    passed: u32,
    class: RouteClass,
    ns: ScopeNamespace,
) -> TestTopologyReceipt {
    TestTopologyReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_owned(),
        cohort: "compiler-profile".to_owned(),
        target_id: target_id.to_owned(),
        head_sha: head.to_owned(),
        base_sha: "0base0000000000000000000000000000000000".to_owned(),
        namespace: ns.tag().to_owned(),
        route_class: class.tag().to_owned(),
        command: format!("cargo test -p xtask --locked -- {}", target_id),
        work: LibTestCounters { passed, failed: 0, ignored: 0, filtered_out: 0 },
        duration_ms: 1234,
        budget_seconds: 120,
        retries: 0,
        verdict: ReceiptVerdict::Pass,
    }
}

const HEAD_A: &str = "1111111111111111111111111111111111111111";
const HEAD_B: &str = "2222222222222222222222222222222222222222";

// ---------------------------------------------------------------------------
// Register self-consistency (advisory row
// test-topology/register-self-consistency)
// ---------------------------------------------------------------------------

#[test]
fn compiler_profile_test_topology_register_self_canonical_register_loads_and_validates()
-> Result<(), Box<dyn std::error::Error>> {
    let register = canonical_register()?;
    assert_eq!(register.schema_version, REGISTER_SCHEMA_VERSION);
    assert_eq!(register.cohort, "compiler-profile");
    assert_eq!(register.register_id, "compiler-profile.v1");
    assert!(register.watch_packages.iter().any(|package| package == "xtask"));
    assert!(
        !register.rows.is_empty(),
        "canonical register must declare the maintained compiler-profile denominator"
    );
    // Re-validation through the parser entry keeps laws enforced on every load.
    register.validate()?;
    Ok(())
}

#[test]
fn compiler_profile_test_topology_register_self_target_identity_is_unique()
-> Result<(), Box<dyn std::error::Error>> {
    let register = canonical_register()?;
    let mut seen = std::collections::BTreeSet::new();
    for row in register.rows() {
        assert!(seen.insert(row.target_id.as_str()), "duplicate target identity {}", row.target_id);
        assert!(row.owner_issue > 0, "{} must name its owner issue", row.target_id);
        assert!(!row.subject.trim().is_empty());
        assert!(!row.claim_boundary.trim().is_empty());
        assert!(!row.proof_role.trim().is_empty());
    }
    Ok(())
}

#[test]
fn compiler_profile_test_topology_register_self_active_rows_carry_nonzero_work_floors_and_budgets()
-> Result<(), Box<dyn std::error::Error>> {
    let register = canonical_register()?;
    let mut active = 0usize;
    let mut pending = 0usize;
    for row in register.rows() {
        match row.status {
            TargetStatus::Active => {
                active += 1;
                assert!(row.execution.is_some(), "active {} lacks execution", row.target_id);
                assert!(row.min_work_items >= 1, "active {} lacks nonzero floor", row.target_id);
                assert!(row.budget_seconds >= 1, "active {} lacks a budget", row.target_id);
                assert_eq!(row.receipt_schema, RECEIPT_SCHEMA_VERSION);
                if let Some(execution) = &row.execution {
                    let argv = execution.render_argv();
                    assert_eq!(argv.first().map(String::as_str), Some("cargo"));
                    assert_eq!(argv.get(1).map(String::as_str), Some("test"));
                    assert!(
                        argv.windows(2).any(|pair| pair[0] == "-p"),
                        "active {} must pin its package",
                        row.target_id
                    );
                    assert!(
                        argv.iter().any(|token| token == "--locked"),
                        "active {} must run locked",
                        row.target_id
                    );
                }
            }
            TargetStatus::DeclaredPending => {
                pending += 1;
                assert!(
                    row.execution.is_none(),
                    "dormant {} carries an execution command",
                    row.target_id
                );
                assert_eq!(row.min_work_items, 0, "dormant {} declares work", row.target_id);
            }
        }
    }
    assert!(active >= 7, "expected the seven active lanes, found {active}");
    assert!(pending >= 80, "declared cohort leaves must stay pinned");
    Ok(())
}

#[test]
fn compiler_profile_test_topology_register_self_namespace_markers_cover_registered_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let register = canonical_register()?;
    // The discovered workspace names of this binary must already answer in the
    // register (no violation today), while a hypothetical new leaf missing a
    // row fails the guard immediately.
    let discovered = vec![DiscoveredTestTarget {
        package: "xtask".to_owned(),
        target_name: "compiler_profile_test_topology".to_owned(),
    }];
    assert!(
        check_discovery_membership(&register, &discovered).is_empty(),
        "registered topology-owned target must satisfy discovery membership"
    );
    let newcomer = vec![DiscoveredTestTarget {
        package: "xtask".to_owned(),
        target_name: "compiler_profile_test_topology_successor_leaf".to_owned(),
    }];
    let violations = check_discovery_membership(&register, &newcomer);
    assert_eq!(violations.len(), 1, "a new leaf omitting its row must fail closed");
    assert!(matches!(
        violations.first(),
        Some(xtask::test_topology::route::DiscoveryViolation::OmittedNewTarget { .. })
    ));
    Ok(())
}

#[test]
fn compiler_profile_test_topology_register_self_workspace_discovers_its_declared_target()
-> Result<(), Box<dyn std::error::Error>> {
    // Real cargo metadata discovery must see this binary so the membership
    // guard above operates on live workspace truth, not assumptions.
    let discovered = discover_workspace_test_targets(&repo_root())?;
    assert!(
        discovered.iter().any(|target| target.package == "xtask"
            && target.target_name == "compiler_profile_test_topology"),
        "workspace discovery must find the topology-owned proof binary"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Issue-required routing falsifiers (#12411)
// ---------------------------------------------------------------------------

/// 1. A registered-but-unlanded target never executes.
#[test]
fn falsifier_01_dormant_target_never_executes() -> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![
        active_required_row("live-target", 3),
        dormant_row("dormant-future-leaf"),
    ]);
    let changed = vec![PathBuf::from("crates/dormant-future-leaf-pending/src/lib.rs")];
    let selection = select_active_scope(&register, &changed);
    assert!(
        selection.decision.selected_target_ids().is_empty(),
        "dormant rows never enter the routed set"
    );
    assert_eq!(selection.dormant_selected.len(), 1);

    // Both execution layers independently refuse dormancy.
    let receipts_dir = repo_root().join("target/test-topology/selftest-dormant");
    let refusal = xtask::test_topology::runner::run_selected_rows(
        &repo_root(),
        &register,
        &[&register.rows[1]],
        HEAD_A,
        "0base0000000000000000000000000000000000",
        ScopeNamespace::PrFocused,
        &receipts_dir,
    )
    .err();
    assert!(refusal.is_some(), "executing a dormant row must fail loudly");
    let text = refusal.map(|error| error.to_string()).unwrap_or_default();
    assert!(text.contains("declared_pending"), "refusal must name dormancy");
    Ok(())
}

/// 2. A filter selecting zero executing work items cannot pass the route.
#[test]
fn falsifier_02_zero_filter_selection_stays_non_green() -> Result<(), Box<dyn std::error::Error>> {
    let row = active_required_row("zero-filter-row", 4);
    let output = "\
running 0 tests

test result: FAILED. 0 passed; 0 failed; 0 ignored; 6 filtered out; 0 measured; finished in 0.01s\n";
    let verdict = evaluate_run(&row, output, true, false, None);
    assert!(matches!(verdict, ReceiptVerdict::ZeroSelection));
    assert!(!verdict.clone().is_green());

    // Exit-zero over a filtered-out-only selection fails equally.
    let exit_zero_output = "\
running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 9 filtered out; 0 measured; finished in 0.00s\n";
    let verdict = evaluate_run(&row, exit_zero_output, true, false, None);
    assert!(matches!(verdict, ReceiptVerdict::ZeroSelection));
    assert!(!verdict.is_green());
    Ok(())
}

/// 3. One target's work count cannot fill another selected target.
#[test]
fn falsifier_03_one_targets_work_count_cannot_fill_another()
-> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![
        active_required_row("well-executed-target", 1),
        active_required_row("omitted-target", 5),
    ]);
    let receipts = vec![pass_receipt(
        "well-executed-target",
        HEAD_A,
        50,
        RouteClass::RequiredAffected,
        ScopeNamespace::PrFocused,
    )];
    let required = vec!["well-executed-target".to_owned(), "omitted-target".to_owned()];
    let report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_A,
        ScopeNamespace::PrFocused,
        &required,
        Vec::new(),
        &receipts,
        &[],
    )?;
    let backfilled = report.violations.iter().any(|violation| {
        matches!(
            violation,
            xtask::test_topology::receipts::FanInViolation::MissingReceipt { target_id }
                if target_id == "omitted-target"
        )
    });
    assert!(backfilled, "aggregate surplus from one row must not discharge another");
    assert_eq!(report.accepted.len(), 1);
    Ok(())
}

/// 4. Skipped or ignored items inside a selected proof stay non-green.
#[test]
fn falsifier_04_skipped_as_pass_is_structurally_impossible()
-> Result<(), Box<dyn std::error::Error>> {
    let row = active_required_row("skip-heavy-row", 2);
    let output = "\
running 8 tests

test result: ok. 5 passed; 0 failed; 3 ignored; 0 filtered out; 0 measured; finished in 0.02s\n";
    let verdict = evaluate_run(&row, output, true, false, None);
    match &verdict {
        ReceiptVerdict::IgnoredOrSkippedPresent { count } => assert_eq!(*count, 3),
        other => return Err(format!("expected ignored-present verdict, got {other:?}").into()),
    }
    assert!(!verdict.is_green());
    Ok(())
}

/// 5. Advisory (or scheduled/manual) evidence can never satisfy a required row.
#[test]
fn falsifier_05_advisory_as_required_never_satisfies_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![
        active_required_row("gated-required-row", 10),
        advisory_row("helpful-advisory-lane", 10),
    ]);
    // Perfectly green advisory receipt, same work count, same head, same lane.
    let receipts = vec![pass_receipt(
        "helpful-advisory-lane",
        HEAD_A,
        25,
        RouteClass::Advisory,
        ScopeNamespace::PrFocused,
    )];
    let report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_A,
        ScopeNamespace::PrFocused,
        &["gated-required-row".to_owned()],
        Vec::new(),
        &receipts,
        &[],
    )?;
    assert_eq!(report.auxiliary.len(), 1, "green advisory evidence stays auxiliary");
    assert!(report.accepted.is_empty(), "auxiliary buckets never become acceptance");
    assert!(
        report.violations.iter().any(|violation| matches!(
            violation,
            xtask::test_topology::receipts::FanInViolation::MissingReceipt { target_id }
                if target_id == "gated-required-row"
        )),
        "the required row must remain open"
    );
    assert!(!RouteClass::Scheduled.satisfies_required());
    assert!(!RouteClass::Manual.satisfies_required());
    assert!(
        !ScopeNamespace::allowed_for_route_class(RouteClass::Manual)
            .contains(&ScopeNamespace::MergeRequired)
    );
    Ok(())
}

/// 6. A profile-impacting change cannot evade selection: control-plane edits
/// select every active required row, subject prefixes select their rows.
#[test]
fn falsifier_06_impact_missed_selection_selects_every_active_required_row()
-> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![
        active_required_row("observed-target", 1),
        advisory_row("advisory-sidecar", 1),
        dormant_row("future-dormant-leaf"),
    ]);
    let changed = vec![
        PathBuf::from(".ci/test-topology/compiler-profile.v1.toml"),
        PathBuf::from("unrelated/docs/note.md"),
    ];
    let selection = select_active_scope(&register, &changed);
    match &selection.decision {
        xtask::test_topology::route::SelectionDecision::Selected {
            target_ids,
            control_plane_change,
        } => {
            assert!(*control_plane_change);
            assert_eq!(
                target_ids.as_slice(),
                &["observed-target".to_owned()][..],
                "control-plane change routes exactly the active required rows"
            );
        }
        other => return Err(format!("expected Selected decision, got {other:?}").into()),
    }

    // Subject-prefix intersecting selection stays exact without control-plane
    // reach.
    let plain =
        select_active_scope(&register, &[PathBuf::from("crates/observed-target/src/inner.rs")]);
    assert_eq!(plain.decision.selected_target_ids(), ["observed-target"]);
    assert!(!plain.dormant_selected.iter().any(|id| id == "future-dormant-leaf"));
    Ok(())
}

/// 7. Unrelated changes emit a checked scoped no-op and never force the full
/// expensive denominator.
#[test]
fn falsifier_07_unrelated_changes_produce_checked_scoped_noop()
-> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![active_required_row("expensive-denominator", 40)]);
    let changed =
        vec![PathBuf::from("docs/devex/some-guide.md"), PathBuf::from("scripts/misc/lint.sh")];
    let selection = select_active_scope(&register, &changed);
    let noop = match &selection.decision {
        xtask::test_topology::route::SelectionDecision::ScopedNoop(proof) => proof,
        other => return Err(format!("expected scoped no-op, got {other:?}").into()),
    };
    assert_eq!(noop.classified_files.len(), changed.len());
    for (file, classified) in changed.iter().zip(noop.classified_files.iter()) {
        assert_eq!(classified.path, file.to_string_lossy().replace('\\', "/"));
        assert!(classified.reason.contains("outside registered"), "{}", classified.reason);
    }
    assert!(selection.decision.selected_target_ids().is_empty());
    Ok(())
}

/// 8. Wrong feature/build profiles deviate from the declared argv the receipt
/// binds, so profile substitution is detectable byte-for-byte.
#[test]
fn falsifier_08_wrong_feature_profile_deviates_from_declared_argv()
-> Result<(), Box<dyn std::error::Error>> {
    let mut scheduled = active_required_row("profile-sensitive-row", 1);
    scheduled.route_class = RouteClass::Scheduled;
    scheduled.candidate_profiles = vec!["scheduled_pressure".to_owned()];
    scheduled.execution = Some(ExecutionKind::CargoTest {
        package: "xtask".to_owned(),
        test_target: None,
        filter: String::new(),
        feature_profile: "--locked --profile agent".to_owned(),
    });
    let declared = scheduled
        .execution
        .as_ref()
        .map(|execution| execution.render_argv())
        .ok_or("fixture row lost its execution")?;
    let expected = ["cargo", "test", "-p", "xtask", "--lib", "--locked", "--profile", "agent"];
    assert_eq!(declared, expected, "declared argv must carry the exact build profile");
    // A foreign runner substitute lacking the declared profile is detectable.
    let wrong = ["cargo", "test", "-p", "xtask", "--lib", "--locked"];
    assert_ne!(wrong[..], declared[..], "missing profile flags must not collide");

    // Receipts bind the rendered command text for later comparison.
    let receipt = {
        let mut receipt = pass_receipt(
            "profile-sensitive-row",
            HEAD_A,
            4,
            RouteClass::Scheduled,
            ScopeNamespace::ScheduledPressure,
        );
        receipt.command = declared.join(" ");
        receipt
    };
    assert!(
        receipt.command.contains("--profile agent"),
        "receipt preserves the exact proved profile"
    );
    Ok(())
}

/// 9. A stale receipt written by another candidate cannot satisfy the route.
#[test]
fn falsifier_09_stale_receipt_from_other_candidate_fails_route()
-> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![active_required_row("fresh-head-row", 3)]);
    let stale = vec![pass_receipt(
        "fresh-head-row",
        HEAD_B,
        9,
        RouteClass::RequiredAffected,
        ScopeNamespace::PrFocused,
    )];
    let report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_A,
        ScopeNamespace::PrFocused,
        &["fresh-head-row".to_owned()],
        Vec::new(),
        &stale,
        &[],
    )?;
    let shape = report.violations.iter().find(|violation| {
        matches!(
            violation,
            xtask::test_topology::receipts::FanInViolation::StaleOnlyEvidence { .. }
        )
    });
    assert!(shape.is_some(), "stale-from-other-heads evidence must surface as stale");
    if let Some(xtask::test_topology::receipts::FanInViolation::StaleOnlyEvidence {
        target_id,
        heads,
    }) = shape
    {
        assert_eq!(target_id, "fresh-head-row");
        assert_eq!(heads.as_slice(), &[HEAD_B.to_owned()][..]);
    }
    Ok(())
}

/// 10. Cancellation, timeout, and instrument failure each stay non-green.
#[test]
fn falsifier_10_cancel_timeout_and_instrument_failure_stay_non_green()
-> Result<(), Box<dyn std::error::Error>> {
    let row = active_required_row("budget-row", 2);
    let healthy_output = "\
running 3 tests

test result: ok. 3 passed; 0 failed; 0 ignored; 0 filtered out; 0 measured; finished in 0.30s\n";

    let timed_out = evaluate_run(&row, healthy_output, false, true, None);
    match &timed_out {
        ReceiptVerdict::TimedOut { budget_seconds } => assert_eq!(*budget_seconds, 120),
        other => return Err(format!("expected timeout verdict, got {other:?}").into()),
    }
    let cancelled = evaluate_run(&row, "", false, false, Some("killed by ctrl-c".to_owned()));
    assert!(matches!(cancelled, ReceiptVerdict::CancelledOrInstrumentFailure { .. }));
    let crashed = evaluate_run(&row, healthy_output, false, false, None);
    assert!(
        matches!(crashed, ReceiptVerdict::FailedTests { .. }),
        "nonzero exit over passing counts must not read as pass"
    );
    assert!(!timed_out.clone().is_green());
    assert!(!cancelled.clone().is_green());
    assert!(!crashed.is_green());
    Ok(())
}

/// 11. Base/main failure stays separate from the candidate: a base-bound
/// receipt never discharges the candidate head and vice versa.
#[test]
fn falsifier_11_base_and_candidate_failures_stay_separate() -> Result<(), Box<dyn std::error::Error>>
{
    let register = synthetic_register(vec![active_required_row("head-keyed-row", 2)]);
    // Red on the candidate head.
    let mut failed = pass_receipt(
        "head-keyed-row",
        HEAD_A,
        2,
        RouteClass::RequiredAffected,
        ScopeNamespace::PrFocused,
    );
    failed.work.passed = 1;
    failed.work.failed = 1;
    failed.verdict = ReceiptVerdict::FailedTests { failed: 1 };
    let report = build_fan_in(
        &register,
        HEAD_B,
        HEAD_A,
        ScopeNamespace::PrFocused,
        &["head-keyed-row".to_owned()],
        Vec::new(),
        std::slice::from_ref(&failed),
        &[],
    )?;
    assert!(report.violations.iter().any(|violation| matches!(
        violation,
        xtask::test_topology::receipts::FanInViolation::NotGreen { .. }
    )));

    // Evidence completed against an earlier head stays intact (valuable
    // in-flight proof is not cancelled by later candidate movement), yet the
    // new head still owes its own execution.
    let head_b_report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_B,
        ScopeNamespace::PrFocused,
        &["head-keyed-row".to_owned()],
        Vec::new(),
        std::slice::from_ref(&failed),
        &[],
    )?;
    assert!(head_b_report.violations.iter().any(|violation| matches!(
        violation,
        xtask::test_topology::receipts::FanInViolation::StaleOnlyEvidence { .. }
    )));
    assert_eq!(report.head_sha, HEAD_A);
    assert_eq!(head_b_report.head_sha, HEAD_B);
    Ok(())
}

/// 12. Rerun-until-green laundering is structurally absent.
#[test]
fn falsifier_12_rerun_until_green_laundering_is_absent() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(RETRY_CEILING, 0, "the runner ceiling forbids retries");
    let register = synthetic_register(vec![active_required_row("no-retry-row", 1)]);
    let mut laundered = pass_receipt(
        "no-retry-row",
        HEAD_A,
        5,
        RouteClass::RequiredAffected,
        ScopeNamespace::PrFocused,
    );
    laundered.retries = 2;
    let report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_A,
        ScopeNamespace::PrFocused,
        &["no-retry-row".to_owned()],
        Vec::new(),
        &[laundered],
        &[],
    )?;
    assert!(report.violations.iter().any(|violation| matches!(
        violation,
        xtask::test_topology::receipts::FanInViolation::RetryLaundering { retries: 2, .. }
    )));
    Ok(())
}

/// 13. Workflow state and check colour are never semantic profile evidence.
#[test]
fn falsifier_13_workflow_state_is_not_semantic_evidence() -> Result<(), Box<dyn std::error::Error>>
{
    let row = active_required_row("colour-blind-row", 1);
    // Exit-zero process whose stdout is pure workflow prose: no counters, no
    // proof. This is exactly what `gh checks` output looks like.
    let workflow_green = "All checks have passed\n  7 successful checks\n";
    let verdict = evaluate_run(&row, workflow_green, true, false, None);
    assert!(matches!(verdict, ReceiptVerdict::CancelledOrInstrumentFailure { .. }));
    assert!(!verdict.clone().is_green());

    // Malformed summaries rejected: bare prose numbers are not libtest.
    assert!(parse_libtest_summaries("passed 100 tests successfully").is_none());
    assert!(parse_libtest_summaries("test result: WINNING. lots passed; done").is_none());

    // Only a genuine parsed summary can produce Pass.
    let real = "\
running 1 test

test result: ok. 1 passed; 0 failed; 0 ignored; 0 filtered out; 0 measured; finished in 0.01s\n";
    let verdict = evaluate_run(&row, real, true, false, None);
    assert!(matches!(verdict, ReceiptVerdict::Pass));
    Ok(())
}

/// 14. Live repository-settings mutation sits outside the route surface.
#[test]
fn falsifier_14_settings_mutation_is_outside_the_route_surface()
-> Result<(), Box<dyn std::error::Error>> {
    // The engine renders only cargo-test invocations; no GitHub or git-mutation
    // verb is reachable through declared rows.
    let row = active_required_row("surface-row", 1);
    let argv = row
        .execution
        .as_ref()
        .map(|execution| execution.render_argv())
        .ok_or("fixture row lost its execution")?;
    for token in &argv {
        assert_ne!(token, "gh");
        assert!(!token.contains("api."));
        assert!(!token.contains("protected-branches"));
        assert!(!token.contains("--force"));
        assert!(!token.contains("push"));
    }
    // Control-plane authority is bounded to the checked register and module.
    assert_eq!(
        CONTROL_PLANE_PREFIXES,
        &[".ci/test-topology/", "xtask/src/test_topology"],
        "selection authority may not silently widen toward workflows/settings"
    );
    // Receipt identity is schema-pinned so evidence vocabulary cannot drift.
    assert_eq!(RECEIPT_SCHEMA_VERSION, "test_topology_receipt.v1");
    assert_eq!(REGISTER_SCHEMA_VERSION, "test_topology_register.v1");
    Ok(())
}

/// 15. A newly introduced cohort leaf omitting its topology row fails the
/// checked register instead of entering CI silently.
#[test]
fn falsifier_15_omitted_new_target_fails_the_topology_guard()
-> Result<(), Box<dyn std::error::Error>> {
    let register = canonical_register()?;
    let violations = check_discovery_membership(
        &register,
        &[
            DiscoveredTestTarget {
                package: "xtask".to_owned(),
                target_name: "compiler_profile_test_topology".to_owned(),
            },
            DiscoveredTestTarget {
                package: "xtask".to_owned(),
                target_name: "compiler_profile_new_evaluation_child".to_owned(),
            },
        ],
    );
    assert_eq!(violations.len(), 1, "exactly the unregistered leaf must be named");
    match violations.first() {
        Some(xtask::test_topology::route::DiscoveryViolation::OmittedNewTarget {
            package,
            target_name,
        }) => {
            assert_eq!(package, "xtask");
            assert_eq!(target_name, "compiler_profile_new_evaluation_child");
        }
        other => return Err(format!("unexpected discovery shape {other:?}").into()),
    }
    Ok(())
}

/// Work shortfall below the declared minimum refuses the aggregate pass even
/// when everything else looked green (complement of zero-selection).
#[test]
fn falsifier_work_shortfall_blocks_minimum_evasion() -> Result<(), Box<dyn std::error::Error>> {
    let row = active_required_row("floor-row", 6);
    let output = "\
running 4 tests

test result: ok. 4 passed; 0 failed; 0 ignored; 2 filtered out; 0 measured; finished in 0.04s\n";
    let verdict = evaluate_run(&row, output, true, false, None);
    match &verdict {
        ReceiptVerdict::WorkShortfall { executed, minimum } => {
            assert_eq!(*executed, 4);
            assert_eq!(*minimum, 6);
        }
        other => return Err(format!("expected shortfall verdict, got {other:?}").into()),
    }
    assert!(!verdict.is_green());
    Ok(())
}

/// Fan-in digests are deterministic and exclude volatile timing, so identical
/// evidence yields identical artifact bytes across reruns.
#[test]
fn falsifier_fan_in_digest_is_deterministic_across_timing_noise()
-> Result<(), Box<dyn std::error::Error>> {
    let entry = |passed: u32| FanInEntry {
        head_sha: HEAD_A.to_owned(),
        namespace: ScopeNamespace::PrFocused.tag().to_owned(),
        route_class: RouteClass::RequiredAffected.tag().to_owned(),
        work: LibTestCounters { passed, failed: 0, ignored: 0, filtered_out: 0 },
        verdict: ReceiptVerdict::Pass,
        retries: 0,
    };
    let mut baseline: BTreeMap<String, FanInEntry> = BTreeMap::new();
    baseline.insert("digest-row".to_owned(), entry(3));
    let digest_baseline = canonical_fan_in_digest("compiler-profile", HEAD_A, &baseline)?;

    // Timing-noise rerun built from a different raw receipt duration still
    // folds into the same semantic entry because FanInEntry carries no clock.
    let register = synthetic_register(vec![active_required_row("digest-row", 1)]);
    let mut noisy_receipt = pass_receipt(
        "digest-row",
        HEAD_A,
        3,
        RouteClass::RequiredAffected,
        ScopeNamespace::PrFocused,
    );
    noisy_receipt.duration_ms = 999_999;
    let report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_A,
        ScopeNamespace::PrFocused,
        &["digest-row".to_owned()],
        Vec::new(),
        std::slice::from_ref(&noisy_receipt),
        &[],
    )?;
    assert!(report.violations.is_empty());
    assert_eq!(report.digest, digest_baseline, "duration noise must not move the digest");

    // A different executed work count does move it.
    let mut changed: BTreeMap<String, FanInEntry> = BTreeMap::new();
    changed.insert("digest-row".to_owned(), entry(4));
    let digest_changed = canonical_fan_in_digest("compiler-profile", HEAD_A, &changed)?;
    assert_ne!(digest_changed, digest_baseline);
    Ok(())
}

/// A scoped no-op binds its head, so unrelated-change classification is
/// per-candidate evidence, not a reusable global skip.
#[test]
fn falsifier_scoped_noop_proof_binds_its_own_head() -> Result<(), Box<dyn std::error::Error>> {
    let register = synthetic_register(vec![active_required_row("scoped-row", 1)]);
    let proof = ScopedNoopProof {
        cohort: "compiler-profile".to_owned(),
        classified_files: vec![ClassifiedFile {
            path: "docs/misc/readme.md".to_owned(),
            reason: "outside registered compiler-profile subjects".to_owned(),
        }],
        head_sha: HEAD_A.to_owned(),
    };
    let report = build_fan_in(
        &register,
        "0base0000000000000000000000000000000000",
        HEAD_A,
        ScopeNamespace::PrFocused,
        &[],
        vec![proof],
        &[],
        &[],
    )?;
    assert!(report.violations.is_empty(), "checked unrelated scope must discharge cleanly");
    assert_eq!(report.scoped_noops.len(), 1);
    assert_eq!(report.scoped_noops.first().map(|proof| proof.head_sha.as_str()), Some(HEAD_A));
    assert!(report.accepted.is_empty());
    Ok(())
}

/// Parsed summary arithmetic treats failed executions as executed work but
/// never as pass coverage (honest nonzero-work accounting).
#[test]
fn falsifier_summary_arithmetic_counts_only_executing_items()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_libtest_summaries(
        "test result: ok. 2 passed; 0 failed; 0 ignored; 1 filtered out; finished in 0.01s\n\
         test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 filtered out; 0 measured; finished in 0.02s\n",
    )
    .ok_or("summaries must parse")?;
    assert_eq!(parsed, LibTestSummary { passed: 2, failed: 3, ignored: 0, filtered_out: 1 });
    assert_eq!(parsed.executed_work(), 5);
    Ok(())
}
