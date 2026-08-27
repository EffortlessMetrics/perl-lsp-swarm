//! Runner-side adapters from the existing gate runner to
//! [`xtask::routed_result`] (`routed_gate_result.v1`, #9156).
//!
//! Everything here projects facts the runner already observed ([`GateResult`],
//! gate logs, hosted environment variables); no command is re-run, no log text
//! is reinterpreted into a verdict, and no plan identity is re-derived — the
//! published `ci_route_plan.v1` (#10179) is consumed and validated verbatim
//! before any work starts.
//!
//! Offline-by-default wiring lives in `GateRunnerConfig::route_plan_path`:
//! unset means unchanged legacy behavior (workflow topology is untouched);
//! set means one normalized, durably published result per executed planned
//! `run` row under `target/receipts/routed-results/`.

use std::collections::BTreeMap;
use std::path::Path;

use color_eyre::eyre::{Context as _, Result, bail, eyre};
use sha2::{Digest as _, Sha256};

use xtask::ci_route_plan::CiRoutePlanV1;
use xtask::routed_result::{
    ArtifactRef, ChildObservation, HostedIdentity, ObservationTiming, PlaneOutcome,
    PrerequisiteEvidence, PrerequisiteState, RoutedGateResultV1, RoutedReaderGateStatus,
    RunObservation, TerminalOutcome, build_routed_result, publish_routed_receipt,
};

use super::{GateDefinition, GateResult};

/// Directory under the repo root receiving normalized results.
pub(super) const ROUTED_RESULTS_DIR: &str = "target/receipts/routed-results";

/// Consume and validate a published `ci_route_plan.v1` before any execution.
/// The plan's own validator recomputes its semantic fingerprint over the
/// canonical bytes, so tampered/stale plans fail closed here.
pub(super) fn load_compiled_plan(path: &Path) -> Result<CiRoutePlanV1> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading route plan {}", path.display()))?;
    let plan: CiRoutePlanV1 = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing route plan {}", path.display()))?;
    plan.validate().map_err(|error| eyre!("route plan {} refused: {error}", path.display()))?;
    Ok(plan)
}

/// Verify the supplied plan actually covers the gates this invocation will
/// run: every applicable planned `run` row is among the executed selections,
/// otherwise a planned row silently produces no result. Fails closed before
/// any gate executes.
pub(super) fn ensure_plan_covers_selection(
    plan: &CiRoutePlanV1,
    selected_gate_names: &[&str],
) -> Result<()> {
    for row in &plan.rows {
        let is_run_row = matches!(row.outcome, xtask::ci_route_plan::PlannedOutcome::Run { .. })
            && row.applicability == xtask::ci_route_plan::Applicability::Applicable;
        if is_run_row && !selected_gate_names.contains(&row.gate_id.as_str()) {
            bail!(
                "route plan marks gate {:?} as an applicable run row but the \
                 runner did not select it; refusing to run against a partially \
                 matching plan",
                row.gate_id
            );
        }
    }
    Ok(())
}

/// Hosted CI identity from the ambient GitHub Actions environment; `None`
/// offline where inventing workflow identity would be fabrication.
pub(super) fn collect_hosted_identity() -> Option<HostedIdentity> {
    let run_id = std::env::var("GITHUB_RUN_ID").ok()?;
    Some(HostedIdentity {
        workflow: std::env::var("GITHUB_WORKFLOW").ok(),
        job: std::env::var("GITHUB_JOB").ok(),
        run_id: Some(run_id),
        run_attempt: std::env::var("GITHUB_RUN_ATTEMPT")
            .ok()
            .and_then(|attempt| attempt.parse().ok())
            .unwrap_or(0),
        matrix: std::env::var("GITHUB_JOB_MATRIX").ok(),
    })
}

/// Project one executed gate's runner facts into a typed observation. The
/// projection preserves the runner's own distinctions exactly:
///
/// - `error` means the command never produced a process result
///   (`exit_code: None`) — the #10160 never-started class;
/// - `timeout` keeps its dedicated flag so it cannot flatten into failure;
/// - `skip` on an applicable run row stays honest as never-started
///   instrument evidence (a zero-exit free-form skip is not success and not
///   applicability).
pub(super) fn observation_from_gate_result(
    _gate: &GateDefinition,
    result: &GateResult,
    receipt_root: &Path,
    hosted: Option<HostedIdentity>,
) -> RunObservation {
    // The runner assigns statuses after observing the child, so the mapping
    // below is total over the statuses run_single_gate produces for
    // post-planning gates: pass/fail/timeout/error/skip.
    let (runner_status, command_started, child) = match result.status.as_str() {
        "pass" => (
            RoutedReaderGateStatus::Pass,
            true,
            ChildObservation {
                exit_code: result.exit_code,
                signal: None,
                timed_out: false,
                cancelled: false,
            },
        ),
        "fail" => (
            RoutedReaderGateStatus::Fail,
            true,
            ChildObservation {
                exit_code: result.exit_code,
                signal: None,
                timed_out: false,
                cancelled: false,
            },
        ),
        "timeout" => (
            RoutedReaderGateStatus::Timeout,
            true,
            ChildObservation {
                exit_code: result.exit_code,
                signal: None,
                timed_out: true,
                cancelled: false,
            },
        ),
        // "error" (spawn/setup never produced a process result) and any
        // runtime "skip" both mean the planned command did not start.
        _ => (
            RoutedReaderGateStatus::SpawnErrorBeforeStart,
            false,
            ChildObservation { exit_code: None, signal: None, timed_out: false, cancelled: false },
        ),
    };

    let duration_i64 = i64::try_from(result.duration_ms).unwrap_or(i64::MAX);
    // A never-started command has no product-command window at all, so its
    // timing stays honestly empty instead of reporting spawn overhead; a
    // started one derives start from a single observed endpoint so the
    // ended-started invariant holds exactly.
    let timing = if command_started {
        let ended_at_unix_ms = unix_millis_now();
        let started_at_unix_ms = ended_at_unix_ms.map(|end| end.saturating_sub(duration_i64));
        ObservationTiming { started_at_unix_ms, ended_at_unix_ms, duration_ms: result.duration_ms }
    } else {
        ObservationTiming { started_at_unix_ms: None, ended_at_unix_ms: None, duration_ms: 0 }
    };

    let mut prerequisites_missing: Vec<String> = Vec::new();
    let artifacts = project_log_artifact(result, receipt_root, &mut prerequisites_missing);

    RunObservation {
        runner_status,
        hosted,
        prerequisites: Some(PrerequisiteEvidence {
            state: PrerequisiteState::Ready,
            missing_artifacts: prerequisites_missing,
            dependency_gates: BTreeMap::new(),
        }),
        command_started,
        child,
        timing,
        artifacts,
    }
}

fn project_log_artifact(
    result: &GateResult,
    receipt_root: &Path,
    prerequisites_missing: &mut Vec<String>,
) -> Vec<ArtifactRef> {
    let Some(log_relative) = result.log_path.as_deref() else {
        if result.status != "skip" && result.status != "error" {
            // A settled command with no log path cannot offer bounded
            // receipt identities; the fact stays visible as missing
            // instrument evidence rather than being papered over.
            prerequisites_missing
                .push(format!("receipt log absent for completed gate {}", result.gate_name));
        }
        return Vec::new();
    };
    let absolute = receipt_root.join(log_relative);
    match std::fs::read(&absolute) {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            vec![ArtifactRef {
                role: "log".to_string(),
                path: format!("target/receipts/{log_relative}"),
                sha256: Some(hex(&hasher.finalize())),
            }]
        }
        Err(_) => {
            prerequisites_missing
                .push(format!("receipt log unreadable: target/receipts/{log_relative}"));
            Vec::new()
        }
    }
}

/// Build, validate, and durably publish the normalized result for one
/// executed planned `run` row. Publication or validation non-success returns
/// a typed error; the atomic publisher guarantees no valid-looking partial
/// result survives such a failure.
pub(super) fn emit_planned_run_row_result(
    plan: &CiRoutePlanV1,
    gate: &GateDefinition,
    result: &GateResult,
    receipt_root: &Path,
    output_dir: &Path,
    hosted: Option<HostedIdentity>,
) -> Result<RoutedGateResultV1> {
    let observation = observation_from_gate_result(gate, result, receipt_root, hosted);
    let mut built = build_routed_result(plan, &gate.name, observation)
        .map_err(|error| eyre!("result normalization refused for {}: {error}", gate.name))?;
    built.reporting = record_publication(output_dir, &built)?;
    // Reporting truth changed after sealing: re-seal with the publication
    // observation attached so the stored fingerprint covers it.
    built.result_fingerprint =
        built.semantic_fingerprint_of().map_err(|error| eyre!("re-sealing failed: {error}"))?;
    if built.reporting.outcome != TerminalOutcome::Success {
        // Re-publish once so the durable artifact itself carries the
        // reporting failure instead of leaving pre-publication bytes behind.
        record_publication(output_dir, &built)?;
    }
    built.validate().map_err(|error| eyre!("published result failed validation: {error}"))?;
    Ok(built)
}

/// One publication attempt returning the resulting reporting plane.
fn record_publication(output_dir: &Path, candidate: &RoutedGateResultV1) -> Result<PlaneOutcome> {
    match publish_routed_receipt(output_dir, candidate) {
        Ok(destination) => Ok(PlaneOutcome {
            outcome: TerminalOutcome::Success,
            detail: destination.display().to_string(),
        }),
        Err(publication_error) => Ok(PlaneOutcome {
            outcome: TerminalOutcome::InstrumentFailure,
            detail: publication_error,
        }),
    }
}

fn unix_millis_now() -> Option<i64> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(now.as_millis()).ok()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod fixtures {
    #![expect(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "test assertions on fixture building"
    )]

    use super::*;
    use xtask::ci_route_plan::{
        Applicability, CompileRoutePlanInput, ExpansionStatus, GateSelectorInput,
        LifecycleDisposition, LifecycleState, PlannedOutcome, PolicyRole, Resolution,
        RouteDispositionInput, RouteExecutionIdentity, RouteProfileExpansionInput,
        RouteSelectionEvidence, RouteSubjectRef, SelectorPlacement, SelectorProof, SelectorRole,
    };

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    pub(super) fn fixture_input() -> CompileRoutePlanInput {
        let expansion = RouteProfileExpansionInput {
            requested_profile: "merge_gate".to_string(),
            included_native_tiers: vec!["merge_gate".to_string()],
            semantic_fingerprint: DIGEST_B.to_string(),
            policy_digest: DIGEST_A.to_string(),
            denominator: vec!["fmt_gate".to_string()],
            resolution: ExpansionStatus::Complete,
            detail: None,
        };
        CompileRoutePlanInput {
            subject: RouteSubjectRef {
                kind: "pull_request".to_string(),
                head_sha: SHA_A.to_string(),
                base_sha: Some(SHA_B.to_string()),
                subject_digest: DIGEST_A.to_string(),
            },
            expansion,
            dispositions: vec![RouteDispositionInput {
                gate_id: "fmt_gate".to_string(),
                policy_role: PolicyRole::Required,
                lifecycle: LifecycleDisposition {
                    state: LifecycleState::Active,
                    resolution: Resolution::Current,
                },
                native_tier: "merge_gate".to_string(),
                quarantine: None,
                detail: None,
            }],
            disposition_digest: DIGEST_B.to_string(),
            workflow_digest: DIGEST_A.to_string(),
            selectors: vec![GateSelectorInput {
                gate_id: "fmt_gate".to_string(),
                placement: SelectorPlacement::Selected,
                role: Some(SelectorRole::AlwaysOn),
                reason: "always on".to_string(),
                proof: Some(SelectorProof::Applicable),
            }],
            selection: RouteSelectionEvidence {
                base: SHA_B.to_string(),
                scope_ok: true,
                fallback_used: false,
                fallback_reason: None,
                package_args: Vec::new(),
                scope: None,
                selector_digest: DIGEST_B.to_string(),
            },
            execution: vec![RouteExecutionIdentity {
                gate_id: "fmt_gate".to_string(),
                command: "cargo fmt --check".to_string(),
                timeout_seconds: 60,
            }],
        }
    }

    /// Mirror of the upstream planning_types test-compile contract: input
    /// compiles offline and validates. Kept identical to keep the seam exact.
    pub(super) fn compiled_fixture() -> CiRoutePlanV1 {
        let mut input = fixture_input();
        input.selectors[0].reason = "always on".to_string();
        let plan = CiRoutePlanV1::compile(input).expect("fixture plan compiles");
        plan.validate().expect("fixture plan validates");
        plan
    }

    #[test]
    fn happy_gate_result_publishes_one_normalized_result() {
        let plan = compiled_fixture();
        let dir = tempfile::tempdir().expect("tempdir");
        let receipt_root = dir.path().join("target/receipts");
        let log_dir = receipt_root.join("logs");
        std::fs::create_dir_all(&log_dir).expect("log dir");
        std::fs::write(log_dir.join("fmt_gate.log"), b"fmt clean").expect("log");

        let gate = fmt_gate_definition();
        let result = passing_result();
        let built = emit_planned_run_row_result(
            &plan,
            &gate,
            &result,
            &receipt_root,
            dir.path().join("routed").as_path(),
            None,
        )
        .expect("emission succeeds");
        assert!(matches!(built.product.outcome, TerminalOutcome::Success));
        assert_eq!(built.row.command, "cargo fmt --check");
        assert_eq!(built.artifacts.len(), 1);
    }

    #[test]
    fn execution_error_is_never_started_instrument_failure() {
        let plan = compiled_fixture();
        let dir = tempfile::tempdir().expect("tempdir");
        let receipt_root = dir.path().join("target/receipts");
        std::fs::create_dir_all(&receipt_root).expect("receipts");

        let gate = fmt_gate_definition();
        let result = error_result();
        let built = emit_planned_run_row_result(
            &plan,
            &gate,
            &result,
            &receipt_root,
            &dir.path().join("routed"),
            None,
        )
        .expect("emission succeeds honestly");
        assert!(matches!(built.product.outcome, TerminalOutcome::BlockedNotProven));
        assert!(!built.command_started);
        assert!(matches!(built.instrument.outcome, TerminalOutcome::InstrumentFailure));
    }

    #[test]
    fn plan_not_covering_a_selected_run_row_is_refused_before_running() {
        let plan = compiled_fixture();
        assert!(ensure_plan_covers_selection(&plan, &["fmt_gate"]).is_ok());
        let refused = ensure_plan_covers_selection(&plan, &["other_gate"]);
        assert!(refused.is_err());
        assert!(refused.err().unwrap().to_string().contains("fmt_gate"));
    }

    #[test]
    fn stale_published_plan_bytes_fail_closed() {
        let plan = compiled_fixture();
        let dir = tempfile::tempdir().expect("tempdir");
        let canonical: serde_json::Value =
            serde_json::from_slice(&plan.canonical_json().expect("canonical bytes"))
                .expect("value");
        let mut json = canonical;
        json["subject"]["head_sha"] = serde_json::Value::String(SHA_B.to_string());
        let path = dir.path().join("tampered.json");
        std::fs::write(&path, serde_json::to_vec(&json).expect("bytes")).expect("write");
        assert!(load_compiled_plan(&path).is_err(), "tampered plan must be refused");
    }

    fn fmt_gate_definition() -> GateDefinition {
        serde_yaml_ng::from_str(
            "name: fmt_gate\ntier: merge_gate\ndescription: fixture\nrequired: true\ncommand: cargo fmt --check\ntimeout_seconds: 60\n",
        )
        .expect("gate definition")
    }

    fn passing_result() -> GateResult {
        GateResult {
            gate_name: "fmt_gate".to_string(),
            tier: "merge_gate".to_string(),
            status: "pass".to_string(),
            required: Some(true),
            duration_ms: 3500,
            command: "cargo fmt --check".to_string(),
            exit_code: Some(0),
            output_summary: None,
            log_path: Some("logs/fmt_gate.log".to_string()),
            metrics: None,
            artifacts: None,
            first_failure: None,
        }
    }

    fn error_result() -> GateResult {
        GateResult {
            gate_name: "fmt_gate".to_string(),
            tier: "merge_gate".to_string(),
            status: "error".to_string(),
            required: Some(true),
            duration_ms: 12,
            command: "cargo fmt --check".to_string(),
            exit_code: None,
            output_summary: Some("Execution error: binary missing".to_string()),
            log_path: None,
            metrics: None,
            artifacts: None,
            first_failure: None,
        }
    }

    // Silence unused-import lint for re-exported helper types used by
    // fixtures above in stricter configurations.
    #[allow(dead_code)]
    fn _type_witness(_: Applicability) {}
}
