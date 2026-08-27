//! Normalized `routed_gate_result.v1` falsifiers (#9156).
//!
//! The typed result binds one executed planned `run` row of an exact
//! `ci_route_plan.v1` (#10179 identity) to three independent verdict
//! planes (product / instrument / reporting). These tests pin the
//! contract offline and deterministically: real process execution is
//! covered by focused unit tests beside the gate runner; here every
//! mis-attribution class the issue lists is exercised against the type's
//! validation alone, plus the durable publication pipeline.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use xtask::ci_route_plan::{
    Applicability, CiRoutePlanV1, CompileRoutePlanInput, ExpansionStatus, GateSelectorInput,
    LifecycleDisposition, LifecycleState, PolicyRole, Resolution,
    RouteDispositionInput, RouteExecutionIdentity, RouteProfileExpansionInput,
    RouteSelectionEvidence, RouteSubjectRef, SelectorPlacement, SelectorProof, SelectorRole,
};
use xtask::routed_result::{
    build_routed_result, publish_routed_receipt, ArtifactRef, ChildObservation,
    HostedIdentity, ObservationTiming, PlaneOutcome, PlanAuthorityIdentity, PlannedRowIdentity,
    PrerequisiteEvidence, PrerequisiteState, RoutedReaderGateStatus,
    RoutedGateResultV1, RunObservation, FINGERPRINT_DOMAIN, ROUTED_GATE_RESULT_PRODUCER,
    ROUTED_GATE_RESULT_SCHEMA,
};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

// ---------------------------------------------------------------------------
// ci_route_plan.v1 fixtures (exact helpers-shaped like the #10179 suite)
// ---------------------------------------------------------------------------

fn plan_input() -> CompileRoutePlanInput {
    let expansion = RouteProfileExpansionInput {
        requested_profile: "merge_gate".to_string(),
        included_native_tiers: vec!["merge_gate".to_string(), "pr_fast".to_string()],
        semantic_fingerprint: DIGEST_B.to_string(),
        policy_digest: DIGEST_C.to_string(),
        denominator: vec!["fmt_gate".to_string(), "unit_gate".to_string()],
        resolution: ExpansionStatus::Complete,
        detail: None,
    };
    let dispositions = vec![
        RouteDispositionInput {
            gate_id: "fmt_gate".to_string(),
            policy_role: PolicyRole::Required,
            lifecycle: LifecycleDisposition {
                state: LifecycleState::Active,
                resolution: Resolution::Current,
            },
            native_tier: "pr_fast".to_string(),
            quarantine: None,
            detail: None,
        },
        RouteDispositionInput {
            gate_id: "unit_gate".to_string(),
            policy_role: PolicyRole::Advisory,
            lifecycle: LifecycleDisposition {
                state: LifecycleState::Active,
                resolution: Resolution::Current,
            },
            native_tier: "pr_fast".to_string(),
            quarantine: None,
            detail: None,
        },
    ];
    CompileRoutePlanInput {
        subject: RouteSubjectRef {
            kind: "pull_request".to_string(),
            head_sha: SHA_A.to_string(),
            base_sha: Some(SHA_B.to_string()),
            subject_digest: DIGEST_A.to_string(),
        },
        expansion,
        dispositions,
        disposition_digest: DIGEST_B.to_string(),
        workflow_digest: DIGEST_C.to_string(),
        selectors: vec![
            GateSelectorInput {
                gate_id: "fmt_gate".to_string(),
                placement: SelectorPlacement::Selected,
                role: Some(SelectorRole::AlwaysOn),
                reason: "always on".to_string(),
                proof: Some(SelectorProof::Applicable),
            },
            GateSelectorInput {
                gate_id: "unit_gate".to_string(),
                placement: SelectorPlacement::Skipped,
                role: Some(SelectorRole::RustScoped),
                reason: "scope selector decided".to_string(),
                proof: Some(SelectorProof::NotApplicableToSubject),
            },
        ],
        selection: RouteSelectionEvidence {
            base: SHA_B.to_string(),
            scope_ok: true,
            fallback_used: false,
            fallback_reason: None,
            package_args: Vec::new(),
            scope: None,
            selector_digest: DIGEST_A.to_string(),
        },
        execution: vec![
            RouteExecutionIdentity {
                gate_id: "fmt_gate".to_string(),
                command: "cargo fmt --check".to_string(),
                timeout_seconds: 60,
            },
            RouteExecutionIdentity {
                gate_id: "unit_gate".to_string(),
                command: "cargo test -p unit".to_string(),
                timeout_seconds: 60,
            },
        ],
    }
}

fn compiled_plan() -> CiRoutePlanV1 {
    CiRoutePlanV1::compile(plan_input()).expect("fixture plan compiles")
}

// ---------------------------------------------------------------------------
// Observations feeding the builder
// ---------------------------------------------------------------------------

fn ready_prerequisites() -> PrerequisiteEvidence {
    PrerequisiteEvidence {
        state: PrerequisiteState::Ready,
        missing_artifacts: Vec::new(),
        dependency_gates: BTreeMap::new(),
    }
}

fn hosted() -> HostedIdentity {
    HostedIdentity {
        workflow: Some("PR Smoke".to_string()),
        job: Some("fast".to_string()),
        run_id: Some("90210".to_string()),
        run_attempt: 2,
        matrix: Some("shard:2/4".to_string()),
    }
}

fn timing(started: bool) -> ObservationTiming {
    if started {
        ObservationTiming {
            started_at_unix_ms: Some(1_000),
            ended_at_unix_ms: Some(4_500),
            duration_ms: 3_500,
        }
    } else {
        ObservationTiming {
            started_at_unix_ms: None,
            ended_at_unix_ms: None,
            duration_ms: 0,
        }
    }
}

/// A faithful successful execution of the planned `fmt_gate` row.
fn success_observation() -> RunObservation {
    RunObservation {
        runner_status: RoutedReaderGateStatus::Pass,
        hosted: Some(hosted()),
        prerequisites: Some(ready_prerequisites()),
        command_started: true,
        child: ChildObservation {
            exit_code: Some(0),
            signal: None,
            timed_out: false,
            cancelled: false,
        },
        timing: timing(true),
        artifacts: Vec::new(),
    }
}

fn build_success(plan: &CiRoutePlanV1) -> RoutedGateResultV1 {
    build_routed_result(plan, "fmt_gate", success_observation()).expect("successful result builds")
}

#[test]
fn happy_run_row_binds_exact_plan_identity_and_validates() {
    let plan = compiled_plan();
    let result = build_success(&plan);

    assert_eq!(result.schema, ROUTED_GATE_RESULT_SCHEMA);
    assert_eq!(result.producer, ROUTED_GATE_RESULT_PRODUCER);
    // Exact #10179 authority binding: consumed verbatim, never re-derived.
    assert_eq!(result.route_plan_fingerprint, plan.semantic_fingerprint);
    let row_identity: &PlannedRowIdentity = &result.row;
    assert_eq!(row_identity.gate_id, "fmt_gate");
    assert_eq!(row_identity.policy_role, PolicyRole::Required);
    assert_eq!(row_identity.command, "cargo fmt --check");
    assert_eq!(row_identity.timeout_seconds, 60);
    assert_eq!(row_identity.applicability_expected, Applicability::Applicable);
    assert_eq!(result.subject.head_sha, SHA_A);
    assert_eq!(result.subject.base_sha.as_deref(), Some(SHA_B));
    let _: &PlanAuthorityIdentity = &result.plan_authority;
    result.validate().expect("faithful success validates");
}

#[test]
fn product_pass_with_reporting_failure_keeps_two_facts() {
    let plan = compiled_plan();
    let mut result = build_success(&plan);
    result.reporting = PlaneOutcome {
        outcome: xtask::routed_result::TerminalOutcome::InstrumentFailure,
        detail: "codecov upload failed after green gate".to_string(),
    };
    // Any semantic mutation invalidates the recorded identity until the
    // record is re-sealed; here the writer re-seals its own edit.
    result.result_fingerprint =
        result.semantic_fingerprint_of().expect("re-seal after mutation");
    // Product stays success while reporting failure is retained exactly.
    result.validate().expect("plane independence is legal");
    assert!(matches!(
        result.product.outcome,
        xtask::routed_result::TerminalOutcome::Success
    ));
    assert!(!matches!(
        result.reporting.outcome,
        xtask::routed_result::TerminalOutcome::Success
    ));
}

#[test]
fn missing_binary_is_block_not_proven_without_product_verdict() {
    let plan = compiled_plan();
    let observation = RunObservation {
        runner_status: RoutedReaderGateStatus::SpawnErrorBeforeStart,
        hosted: None,
        prerequisites: Some(PrerequisiteEvidence {
            state: PrerequisiteState::Missing,
            missing_artifacts: vec!["target/release/perllsp".to_string()],
            dependency_gates: BTreeMap::new(),
        }),
        command_started: false,
        child: ChildObservation { exit_code: None, signal: None, timed_out: false, cancelled: false },
        timing: timing(false),
        artifacts: Vec::new(),
    };
    let result =
        build_routed_result(&plan, "fmt_gate", observation).expect("blocked result builds");
    result.validate().expect("blocked-not-proven shape validates");
    assert!(matches!(
        result.product.outcome,
        xtask::routed_result::TerminalOutcome::BlockedNotProven
    ));
    assert!(matches!(
        result.instrument.outcome,
        xtask::routed_result::TerminalOutcome::Missing
    ));
    assert!(result.prerequisites.missing_artifacts.contains(&"target/release/perllsp".to_string()));
}

#[test]
fn prerequisite_failure_cannot_be_labelled_product_failure() {
    let plan = compiled_plan();

    // Dishonest combination: claims the child ran to a product failure even
    // though a dependency gate had already failed before start. The builder
    // refuses to mint that shape at all — mis-attribution cannot be encoded.
    let dishonest = RunObservation {
        runner_status: RoutedReaderGateStatus::Fail,
        hosted: None,
        prerequisites: Some(PrerequisiteEvidence {
            state: PrerequisiteState::Failed,
            missing_artifacts: Vec::new(),
            dependency_gates: BTreeMap::from([(
                "build_binaries".to_string(),
                "dependency failed first".to_string(),
            )]),
        }),
        command_started: true,
        child: ChildObservation {
            exit_code: Some(101),
            signal: None,
            timed_out: false,
            cancelled: false,
        },
        timing: timing(true),
        artifacts: Vec::new(),
    };
    assert!(
        build_routed_result(&plan, "fmt_gate", dishonest).is_err(),
        "a dependency-blocked child must never be recorded as product failure"
    );

    // The honest projection of the same event: never-started, blocked.
    let honest = RunObservation {
        runner_status: RoutedReaderGateStatus::SpawnErrorBeforeStart,
        hosted: None,
        prerequisites: Some(PrerequisiteEvidence {
            state: PrerequisiteState::Failed,
            missing_artifacts: Vec::new(),
            dependency_gates: BTreeMap::from([(
                "build_binaries".to_string(),
                "dependency failed first".to_string(),
            )]),
        }),
        command_started: false,
        child: ChildObservation { exit_code: None, signal: None, timed_out: false, cancelled: false },
        timing: timing(false),
        artifacts: Vec::new(),
    };
    let result =
        build_routed_result(&plan, "fmt_gate", honest).expect("honest block builds");
    result.validate().expect("honest block validates");
    assert!(matches!(
        result.product.outcome,
        xtask::routed_result::TerminalOutcome::BlockedNotProven
    ));
}

#[test]
fn timeout_signal_cancel_stay_distinct() {
    let plan = compiled_plan();

    let timeout_observation = RunObservation {
        runner_status: RoutedReaderGateStatus::Timeout,
        hosted: None,
        prerequisites: Some(ready_prerequisites()),
        command_started: true,
        child: ChildObservation {
            exit_code: None,
            signal: Some("SIGKILL(watchdog)".to_string()),
            timed_out: true,
            cancelled: false,
        },
        timing: timing(true),
        artifacts: Vec::new(),
    };
    let result = build_routed_result(&plan, "fmt_gate", timeout_observation).expect("timeout");
    result.validate().expect("timeout shape validates");
    assert!(matches!(
        result.product.outcome,
        xtask::routed_result::TerminalOutcome::Timeout
    ));

    // A watchdog kill flattened into assertion failure would be a different,
    // rejected world: Failure with no exit code but a signal is only valid
    // when it was neither timed out nor cancelled.
    let flattened = RunObservation {
        runner_status: RoutedReaderGateStatus::Fail,
        hosted: None,
        prerequisites: Some(ready_prerequisites()),
        command_started: true,
        child: ChildObservation {
            exit_code: None,
            signal: Some("SIGKILL".to_string()),
            timed_out: false,
            cancelled: false,
        },
        timing: timing(true),
        artifacts: Vec::new(),
    };
    let result = build_routed_result(&plan, "fmt_gate", flattened).expect("signal fail");
    assert!(matches!(
        result.product.outcome,
        xtask::routed_result::TerminalOutcome::Failure
    ));
    assert!(result.child.signal.is_some());

    let cancelled_observation = RunObservation {
        runner_status: RoutedReaderGateStatus::CancelledAfterStart,
        hosted: None,
        prerequisites: Some(ready_prerequisites()),
        command_started: true,
        child: ChildObservation {
            exit_code: None,
            signal: None,
            timed_out: false,
            cancelled: true,
        },
        timing: timing(true),
        artifacts: Vec::new(),
    };
    let result =
        build_routed_result(&plan, "fmt_gate", cancelled_observation).expect("cancelled");
    result.validate().expect("cancelled shape validates");
    assert!(matches!(
        result.product.outcome,
        xtask::routed_result::TerminalOutcome::Cancelled
    ));
}

#[test]
fn canonical_bytes_are_order_independent_and_domain_separated() {
    let plan = compiled_plan();
    let mut left = build_success(&plan);
    // Same semantic content, different artifact source order...
    left.artifacts = vec![
        ArtifactRef {
            role: "log".to_string(),
            path: "target/receipts/logs/zzz.log".to_string(),
            sha256: Some(DIGEST_B.to_string()),
        },
        ArtifactRef {
            role: "log".to_string(),
            path: "target/receipts/logs/aaa.log".to_string(),
            sha256: Some(DIGEST_C.to_string()),
        },
    ];
    // Re-seal after the writer's mutation; tamper-evidence stays strict.
    left.result_fingerprint = left.semantic_fingerprint_of().expect("re-seal");
    let mut right = left.clone();
    right.artifacts.reverse();

    let left_bytes = left.canonical_json().expect("left encodes");
    let right_bytes = right.canonical_json().expect("right encodes");
    assert_eq!(
        left_bytes, right_bytes,
        "set-like collections must normalize; ordered ones may not move identity"
    );
    assert_eq!(left.result_fingerprint, right.result_fingerprint);
    // Domain separation matches the frozen #10179 spelling style.
    assert!(String::from_utf8_lossy(FINGERPRINT_DOMAIN).starts_with(ROUTED_GATE_RESULT_SCHEMA));
}

#[test]
fn tampered_fingerprint_or_unknown_fields_fail_closed() {
    let plan = compiled_plan();
    let mut result = build_success(&plan);
    result.result_fingerprint = DIGEST_C.to_string();
    assert!(
        result.validate().is_err(),
        "a digest that does not match the recomputed preimage must be refused"
    );

    let json = build_success(&compiled_plan()).canonical_json().expect("bytes");
    let parsed: Result<RoutedGateResultV1, _> = {
        let mut value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        value
            .as_object_mut()
            .expect("object")
            .insert("invented_field".to_string(), serde_json::Value::Bool(true));
        serde_json::from_value(value)
    };
    assert!(parsed.is_err(), "unknown fields fail closed");

    // Explicit null for an optional (base_sha) is not a canonical spelling.
    let mut value: serde_json::Value =
        serde_json::from_slice(&build_success(&compiled_plan()).canonical_json().unwrap())
            .unwrap();
    value["subject"]["base_sha"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<RoutedGateResultV1>(value).is_err(),
        "null optionals fail closed"
    );
}

#[test]
fn publication_is_durable_readback_verified_and_leave_no_temp() {
    let plan = compiled_plan();
    let result = build_success(&plan);
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let destination: PathBuf =
        publish_routed_receipt(dir.path(), &result).expect("publication succeeds");

    assert!(destination.is_file());
    let read_back = fs::read(&destination).expect("published receipt readable");
    assert_eq!(
        read_back,
        result.canonical_json().expect("canonical bytes"),
        "read-back must equal the encoded bytes"
    );
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .expect("dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers.iter().all(|name| !name.contains(".tmp")),
        "no temporary artifact survives completion: {leftovers:?}"
    );

    // A corrupted final artifact is observable: publishing identical bytes
    // again is idempotent-safe via unique temps (no create-new collision).
    let second = publish_routed_receipt(dir.path(), &result);
    assert!(second.is_ok(), "repeat publication into the same directory succeeds");
}

#[test]
fn unsupported_runner_status_has_a_closed_vocabulary() {
    let plan = compiled_plan();
    let variants = [
        RoutedReaderGateStatus::Pass,
        RoutedReaderGateStatus::Fail,
        RoutedReaderGateStatus::Timeout,
        RoutedReaderGateStatus::CancelledAfterStart,
        RoutedReaderGateStatus::SpawnErrorBeforeStart,
    ];
    for status in variants {
        let observation = RunObservation {
            runner_status: status,
            hosted: None,
            prerequisites: Some(ready_prerequisites()),
            command_started: true,
            child: ChildObservation {
                exit_code: Some(0),
                signal: None,
                timed_out: false,
                cancelled: false,
            },
            timing: timing(true),
            artifacts: Vec::new(),
        };
        if let Ok(result) = build_routed_result(&plan, "fmt_gate", observation) {
            result
                .validate()
                .unwrap_or_else(|error| panic!("closed vocabulary outcome: {error}"));
        }
    }
}
