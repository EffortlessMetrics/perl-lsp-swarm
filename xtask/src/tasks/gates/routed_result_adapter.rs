//! Runner-side adapters from the existing gate runner to
//! [`xtask::routed_result`] (`routed_gate_result.v1`, #9156).
//!
//! Everything here projects facts the runner already observed ([`GateResult`],
//! gate logs, hosted environment variables); no command is re-run, no log text
//! is reinterpreted into a verdict, and no plan identity is re-derived — the
//! published `ci_route_plan.v1` (#10179) is consumed and validated verbatim,
//! and its subject + row execution identity are bound to the actual
//! invocation before any gate runs.
//!
//! Offline-by-default wiring lives in `GateRunnerConfig::route_plan_path`:
//! unset means unchanged legacy behavior (workflow topology is untouched);
//! set means one normalized, durably published result per executed planned
//! `run` row under `target/receipts/routed-results/`.
//!
//! # Runner-surface support boundary (review thread 3872200285)
//!
//! The typed domain covers the issue's full closed outcome vocabulary
//! (dependency-blocked, signal, cancellation, missing/stale included), but
//! this live adapter can only emit classes the current runner surface
//! actually observes:
//!
//! - `GateResult` records exit codes, the runner's timeout flag, and
//!   never-started (`error`/`skip`) statuses — those bind directly;
//! - the runner observes no signal identities, has no cancellation path,
//!   and has no dependency gating, so `signal` is `None`, `cancelled` is
//!   `false`, and dependency maps are empty on every live record: these are
//!   the runner's true observations, not fabricated verdicts;
//! - a completed command implies its prerequisites were ready (the process
//!   ran), so `Ready` is recorded only for started commands; a
//!   never-started command carries no prerequisite evidence at all rather
//!   than an assumed-ready fact;
//! - when structured executor observation (#11618/#9548) lands, this
//!   adapter binds those fields; until then the narrower live claim is
//!   explicit here and in the PR contract, and the schema stays
//!   incapable of encoding the mis-attributions the issue forbids.

use std::collections::BTreeMap;
use std::path::Path;

use color_eyre::eyre::{Context as _, Result, bail, eyre};
use sha2::{Digest as _, Sha256};

use xtask::ci_route_plan::{Applicability, CiRoutePlanV1, PlannedOutcome, RouteSubjectRef};
use xtask::routed_result::{
    ArtifactRef, ChildObservation, HostedIdentity, ObservationTiming, PrerequisiteEvidence,
    PrerequisiteState, RoutedGateResultV1, RoutedReaderGateStatus, RunObservation,
    build_routed_result, hex, publish_routed_receipt,
};

use super::{GateDefinition, GateResult, GateTier};

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

/// Mirrors the runner's own quarantine skip in `run_single_gate`: a
/// quarantined gate is selected but returns a skip without executing unless
/// the run is verbose. Routed preflight and emission must use exactly this
/// predicate, or they will disagree with what actually executed.
pub(super) fn is_quarantine_skipped(gate: &GateDefinition, verbose: bool) -> bool {
    gate.quarantine && !verbose
}

/// Verify the supplied plan exactly covers the gates this invocation will
/// run, in both directions, before any gate executes (review thread
/// 3871822409): every applicable planned `run` row is among the executed
/// selections, every selected gate has an applicable planned `run` row, and
/// each matched row's execution identity (command, timeout policy) agrees
/// with the actually loaded gate policy. An untampered but stale plan is
/// refused here instead of publishing a result attributed to a different
/// command or timeout (review thread 3871822396).
pub(super) fn ensure_plan_covers_selection(
    plan: &CiRoutePlanV1,
    selected_gates: &[&GateDefinition],
    verbose: bool,
) -> Result<()> {
    let planned_run_rows: Vec<&xtask::ci_route_plan::RoutePlanRow> = plan
        .rows
        .iter()
        .filter(|row| {
            matches!(row.outcome, PlannedOutcome::Run { .. })
                && row.applicability == Applicability::Applicable
        })
        .collect();

    for row in &planned_run_rows {
        let Some(selected) = selected_gates.iter().find(|gate| gate.name == row.gate_id) else {
            bail!(
                "route plan marks gate {:?} as an applicable run row but the \
                 runner did not select it; refusing to run against a partially \
                 matching plan",
                row.gate_id
            );
        };
        // The other direction of the same disagreement: the plan expects a
        // real execution, but this runner would skip the gate as
        // quarantined and publish nothing for it.
        if is_quarantine_skipped(selected, verbose) {
            bail!(
                "route plan marks gate {:?} as an applicable run row but this runner \
                 quarantines it and would skip it without executing; refusing a plan \
                 that disagrees with the loaded gate policy on quarantine",
                row.gate_id
            );
        }
    }
    for gate in selected_gates {
        // A quarantined gate is selected into the plan but returns a skip
        // without executing (`run_single_gate`), so the plan correctly
        // carries it as `Quarantined` rather than an applicable run row.
        // Requiring a run row for it would abort every routed run of a tier
        // that contains one, before any gate executes.
        if is_quarantine_skipped(gate, verbose) {
            continue;
        }
        let Some(row) = planned_run_rows.iter().find(|row| row.gate_id == gate.name) else {
            bail!(
                "runner selects gate {:?} but the plan has no applicable run row for it \
                 (absent from the denominator, scoped-noop, quarantined, or error row); \
                 plan mismatch is refused before any gate runs",
                gate.name
            );
        };
        let PlannedOutcome::Run { command, timeout_seconds, .. } = &row.outcome else {
            continue;
        };
        if gate.command.trim() != command.trim() || gate.timeout_seconds != *timeout_seconds {
            bail!(
                "plan row execution identity for gate {:?} disagrees with the loaded gate \
                 policy (command {:?}/{}s vs planned {:?}/{}s); \
                 refusing a stale plan before execution",
                gate.name,
                gate.command.trim(),
                gate.timeout_seconds,
                command.trim(),
                timeout_seconds
            );
        }
    }
    Ok(())
}

/// Bind the plan subject to the actual invocation subject before any gate
/// runs: a plan compiled for a different head SHA must never publish a
/// result on this checkout (cross-SHA negative control; review thread
/// 3871822396).
pub(super) fn ensure_plan_subject_matches_invocation(
    plan: &CiRoutePlanV1,
    actual_head_sha: &str,
) -> Result<()> {
    if plan.subject.head_sha != actual_head_sha {
        bail!(
            "route plan subject head {} does not match this invocation's HEAD {}; \
             refusing to execute against a foreign-subject plan",
            plan.subject.head_sha,
            actual_head_sha
        );
    }
    Ok(())
}

/// Bind the route plan to the immutable subject receipt that supplied its
/// digest. Checking only `HEAD` is insufficient: a caller can keep the same
/// commit while changing the index or working tree that a gate actually
/// inspects. The subject loader re-resolves the receipt's tree inputs against
/// this checkout, so a same-HEAD/different-tree invocation is refused before
/// execution rather than publishing a result for the wrong bytes.
pub(super) fn ensure_plan_subject_matches_receipt(
    plan: &CiRoutePlanV1,
    subject_path: &Path,
    root: &Path,
) -> Result<()> {
    let subject = crate::tasks::ci_subject::load_and_resolve(subject_path, root)
        .with_context(|| format!("loading route-plan subject {}", subject_path.display()))?;
    ensure_plan_subject_fields_match_receipt(&plan.subject, &subject.receipt)
}

/// Every subject field the plan mirrors from the receipt must agree with the
/// receipt, not only the digest and head. The digest is authority-supplied
/// rather than recomputed here, so a plan could otherwise copy a valid digest
/// and head while changing its subject kind or base SHA and publish a false
/// subject identity under a valid digest.
pub(super) fn ensure_plan_subject_fields_match_receipt(
    plan_subject: &RouteSubjectRef,
    receipt: &crate::tasks::ci_subject::CiSubjectReceipt,
) -> Result<()> {
    if receipt.subject_digest != plan_subject.subject_digest {
        bail!(
            "route plan subject digest {} does not match immutable subject receipt {}; \
             refusing to execute against a foreign subject",
            plan_subject.subject_digest,
            receipt.subject_digest
        );
    }
    if receipt.head_sha != plan_subject.head_sha {
        bail!(
            "route plan subject head {} does not match immutable subject receipt {}; \
             refusing to execute against a foreign subject",
            plan_subject.head_sha,
            receipt.head_sha
        );
    }
    if let Some(base_sha) = plan_subject.base_sha.as_deref()
        && base_sha != receipt.base_sha
    {
        bail!(
            "route plan subject base {} does not match immutable subject receipt base {}; \
             refusing to publish a result under a false subject identity",
            base_sha,
            receipt.base_sha
        );
    }
    let receipt_kind = serde_json::to_value(receipt.event_kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    if plan_subject.kind != receipt_kind {
        bail!(
            "route plan subject kind {:?} does not match immutable subject receipt event kind {:?}; \
             refusing to publish a result under a false subject identity",
            plan_subject.kind,
            receipt_kind
        );
    }
    Ok(())
}

/// Bind the plan's selection authority to the runner that is about to execute
/// it. Matching gate commands is not enough: a plan compiled for another
/// profile or another selection base can select the same commands, and its
/// published results would then claim a selection authority that did not
/// govern this execution.
pub(super) fn ensure_plan_authority_matches_invocation(
    plan: &CiRoutePlanV1,
    tier: &GateTier,
    resolved_base_sha: Option<&str>,
) -> Result<()> {
    let runner_profile = tier.to_string();
    if plan.requested_profile != runner_profile {
        bail!(
            "route plan was compiled for profile {:?} but this runner executes tier {:?}; \
             refusing a plan whose selection authority did not govern this invocation",
            plan.requested_profile,
            runner_profile
        );
    }
    if let Some(base) = resolved_base_sha
        && plan.selection.base != base
    {
        bail!(
            "route plan selection base {} does not match this runner's resolved base {}; \
             refusing a plan whose selection authority did not govern this invocation",
            plan.selection.base,
            base
        );
    }
    Ok(())
}

/// A route plan is a committed-subject contract. Refuse tracked staged or
/// working-tree changes before any gate runs; otherwise the plan can name the
/// current commit while the command observes different source bytes.
pub(super) fn ensure_execution_tree_is_clean(status: &str) -> Result<()> {
    if !status.trim().is_empty() {
        bail!(
            "route plan requires a clean tracked index and working tree; \
             refusing to execute against an unbound tree (git status: {})",
            status.trim()
        );
    }
    Ok(())
}

/// Hosted CI identity from the ambient GitHub Actions environment; `None`
/// only when no GitHub environment exists at all (offline). A partial
/// environment (some variables present, the claimed identity set incomplete)
/// is a typed refusal, never a silently unbound hosted record (review
/// thread 3872200290). `GITHUB_JOB_MATRIX` stays optional: a non-matrix job
/// genuinely has none.
pub(super) fn collect_hosted_identity_from(
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<Option<HostedIdentity>> {
    let workflow = lookup("GITHUB_WORKFLOW");
    let job = lookup("GITHUB_JOB");
    let run_id = lookup("GITHUB_RUN_ID");
    let run_attempt = lookup("GITHUB_RUN_ATTEMPT");
    let matrix = lookup("GITHUB_JOB_MATRIX");

    if workflow.is_none() && job.is_none() && run_id.is_none() && run_attempt.is_none() {
        return Ok(None);
    }
    let run_id = run_id.ok_or_else(|| {
        eyre!(
            "partial hosted identity environment: GITHUB_RUN_ID missing while other GitHub \
             variables are present; refusing to normalize an unbound hosted result"
        )
    })?;
    let workflow = workflow.ok_or_else(|| {
        eyre!(
            "partial hosted identity environment: GITHUB_WORKFLOW missing while other GitHub \
             variables are present; refusing to normalize an unbound hosted result"
        )
    })?;
    let job = job.ok_or_else(|| {
        eyre!(
            "partial hosted identity environment: GITHUB_JOB missing while other GitHub \
             variables are present; refusing to normalize an unbound hosted result"
        )
    })?;
    let run_attempt: u64 = run_attempt
        .ok_or_else(|| {
            eyre!(
                "partial hosted identity environment: GITHUB_RUN_ATTEMPT missing while other \
                 GitHub variables are present; refusing to normalize an unbound hosted result"
            )
        })?
        .parse()
        .map_err(|error| eyre!("GITHUB_RUN_ATTEMPT is not a real attempt identity: {error}"))?;
    if run_attempt == 0 {
        bail!("partial hosted identity environment: GITHUB_RUN_ATTEMPT is not a positive attempt");
    }
    Ok(Some(HostedIdentity {
        workflow: Some(workflow),
        job: Some(job),
        run_id: Some(run_id),
        run_attempt,
        matrix,
    }))
}

/// Ambient-environment entry point used by the runner wiring.
pub(super) fn collect_hosted_identity() -> Result<Option<HostedIdentity>> {
    collect_hosted_identity_from(|name| std::env::var(name).ok())
}

/// Project one executed gate's runner facts into a typed observation. The
/// projection preserves the runner's own distinctions exactly (see the
/// module-level support boundary):
///
/// - `error` means the command never produced a process result
///   (`exit_code: None`) — the #10160 never-started class; no prerequisite
///   evidence is invented for it;
/// - a `pass`/`fail` with `exit_code: None` is the runner's in-process
///   dispatch (internal gates never carry an exit code), recorded as
///   `in_process` instead of fabricating a process exit;
/// - `timeout` keeps its dedicated flag so it cannot flatten into failure;
/// - `skip` on an applicable run row stays honest as never-started
///   instrument evidence (a zero-exit free-form skip is not success and not
///   applicability).
pub(super) fn observation_from_gate_result(
    _gate: &GateDefinition,
    result: &GateResult,
    root: &Path,
    receipt_root: &Path,
    hosted: Option<HostedIdentity>,
) -> Result<RunObservation> {
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
                // In-process dispatch is the only producer of pass/fail
                // without an exit code (`ShellExecutionResult.exit_code`
                // is always an i32 for command gates).
                in_process: result.exit_code.is_none(),
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
                in_process: result.exit_code.is_none(),
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
                in_process: false,
            },
        ),
        // "error" (spawn/setup never produced a process result) and any
        // runtime "skip" both mean the planned command did not start.
        _ => (
            RoutedReaderGateStatus::SpawnErrorBeforeStart,
            false,
            ChildObservation {
                exit_code: None,
                signal: None,
                timed_out: false,
                cancelled: false,
                in_process: false,
            },
        ),
    };

    // A never-started command has no product-command window at all, so its
    // timing stays honestly empty instead of reporting spawn overhead; a
    // started one derives start from a single observed endpoint so the
    // ended-started invariant holds exactly.
    //
    // A started command whose clock or duration cannot be represented is a
    // refusal, not a degradation: emptying the timing of a command that did
    // start would publish it as never-started, which is exactly the
    // mis-attribution this record exists to prevent.
    let timing = if command_started {
        let duration_i64 = i64::try_from(result.duration_ms).map_err(|_| {
            eyre!(
                "gate {} reports a duration of {}ms, which exceeds the representable \
                 observation window; refusing rather than truncating it",
                result.gate_name,
                result.duration_ms
            )
        })?;
        let ended_at_unix_ms = unix_millis_now().ok_or_else(|| {
            eyre!(
                "gate {} completed but the system clock is not representable as UNIX \
                 milliseconds; refusing rather than publishing it as never-started",
                result.gate_name
            )
        })?;
        ObservationTiming {
            started_at_unix_ms: Some(ended_at_unix_ms.saturating_sub(duration_i64)),
            ended_at_unix_ms: Some(ended_at_unix_ms),
            duration_ms: result.duration_ms,
        }
    } else {
        ObservationTiming { started_at_unix_ms: None, ended_at_unix_ms: None, duration_ms: 0 }
    };

    // Receipt/reporting evidence is independent of the pre-command
    // prerequisite state: a missing or unreadable log on a completed command
    // must degrade the reporting plane, never masquerade as a prerequisite
    // fact or disappear behind an unconditional instrument success (review
    // thread 3871822416).
    let mut receipt_shortfall: Vec<String> = Vec::new();
    let mut artifacts = project_log_artifact(result, receipt_root, &mut receipt_shortfall);
    artifacts.extend(project_declared_artifacts(result, root, &mut receipt_shortfall));

    Ok(RunObservation {
        runner_status,
        hosted,
        // A started command implies its prerequisites were ready (the
        // process ran); a never-started command carries no prerequisite
        // evidence rather than an assumed-ready fact (review thread
        // 3872200285).
        prerequisites: if command_started {
            Some(PrerequisiteEvidence {
                state: PrerequisiteState::Ready,
                missing_artifacts: Vec::new(),
                dependency_gates: BTreeMap::new(),
            })
        } else {
            None
        },
        command_started,
        child,
        timing,
        artifacts,
        receipt_shortfall,
    })
}

fn project_log_artifact(
    result: &GateResult,
    receipt_root: &Path,
    receipt_shortfall: &mut Vec<String>,
) -> Vec<ArtifactRef> {
    let Some(log_relative) = result.log_path.as_deref() else {
        if result.status != "skip" && result.status != "error" {
            // A settled command with no log path cannot offer bounded
            // receipt identities; the fact stays visible as missing
            // reporting evidence rather than being papered over.
            receipt_shortfall
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
            receipt_shortfall
                .push(format!("receipt log unreadable: target/receipts/{log_relative}"));
            Vec::new()
        }
    }
}

/// Project the gate policy's declared artifacts (repository-root relative
/// paths such as `target/receipts/clippy.json`) into bounded receipt
/// identities. A declared artifact that a completed command did not produce
/// is a named reporting shortfall, so consumers can inspect declared outputs
/// through the normalized record instead of re-deriving them from policy.
/// Never-started commands declare nothing.
fn project_declared_artifacts(
    result: &GateResult,
    root: &Path,
    receipt_shortfall: &mut Vec<String>,
) -> Vec<ArtifactRef> {
    let Some(declared) = result.artifacts.as_deref() else {
        return Vec::new();
    };
    if result.status == "skip" || result.status == "error" {
        return Vec::new();
    }
    let mut projected = Vec::new();
    for path in declared {
        match std::fs::read(root.join(path)) {
            Ok(bytes) => {
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                projected.push(ArtifactRef {
                    role: "artifact".to_string(),
                    path: path.clone(),
                    sha256: Some(hex(&hasher.finalize())),
                });
            }
            Err(_) => receipt_shortfall.push(format!(
                "declared artifact absent or unreadable for completed gate {}: {path}",
                result.gate_name
            )),
        }
    }
    projected
}

/// Build, validate, and durably publish the normalized result for one
/// executed planned `run` row.
///
/// The reporting plane is sealed into the bytes before publication (it
/// records the run's observed receipt instruments), so there is exactly one
/// publication of exactly the sealed bytes through the shared durable
/// substrate; no pre-seal artifact can survive as the sole fresh record
/// (review thread 3871822398). A publication that never succeeds surfaces
/// as a typed error so the gates command fails loudly instead of continuing
/// with no durable result (review thread 3871822403).
pub(super) fn emit_planned_run_row_result(
    plan: &CiRoutePlanV1,
    gate: &GateDefinition,
    result: &GateResult,
    root: &Path,
    receipt_root: &Path,
    output_dir: &Path,
    hosted: Option<HostedIdentity>,
) -> Result<RoutedGateResultV1> {
    let observation = observation_from_gate_result(gate, result, root, receipt_root, hosted)?;
    let built = build_routed_result(plan, &gate.name, observation)
        .map_err(|error| eyre!("result normalization refused for {}: {error}", gate.name))?;
    // canonical_json validates before any filesystem effect, then the
    // substrate guarantees the destination holds exactly these bytes or the
    // error propagates.
    publish_routed_receipt(output_dir, &built)
        .map_err(|error| eyre!("durable publication refused for {}: {error}", gate.name))?;
    Ok(built)
}

fn unix_millis_now() -> Option<i64> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(now.as_millis()).ok()
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
    use xtask::routed_result::TerminalOutcome;

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
            dir.path(),
            &receipt_root,
            dir.path().join("routed").as_path(),
            None,
        )
        .expect("emission succeeds");
        assert!(matches!(built.product.outcome, TerminalOutcome::Success));
        assert_eq!(built.row.command, "cargo fmt --check");
        assert_eq!(built.artifacts.len(), 1);

        // The durable artifact must be the resealed record itself: parse the
        // published bytes back and require the reporting plane on disk to be
        // the same fact the returned object carries (review thread
        // 3871822398 — the pre-reseal bytes must never survive as the sole
        // fresh artifact).
        let published: Vec<_> = std::fs::read_dir(dir.path().join("routed"))
            .expect("output dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        assert_eq!(published.len(), 1, "exactly one normalized result per run: {published:?}");
        let on_disk: RoutedGateResultV1 =
            serde_json::from_slice(&std::fs::read(&published[0]).expect("artifact bytes"))
                .expect("artifact parses");
        on_disk.validate().expect("published artifact validates");
        assert_eq!(on_disk.result_fingerprint, built.result_fingerprint);
        assert!(
            matches!(on_disk.reporting.outcome, TerminalOutcome::Success),
            "durable artifact must carry the sealed reporting success, got {:?}",
            on_disk.reporting
        );
    }

    #[test]
    fn publication_failure_fails_the_invocation() {
        // A publication that can never succeed must surface as an error from
        // the emission path so the gates command fails loudly instead of
        // continuing with no durable result (review thread 3871822403).
        let plan = compiled_fixture();
        let dir = tempfile::tempdir().expect("tempdir");
        let receipt_root = dir.path().join("target/receipts");
        std::fs::create_dir_all(&receipt_root).expect("receipts");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"a file, not a directory").expect("blocker");
        let output_dir = blocker.join("nested").join("routed-results");

        let gate = fmt_gate_definition();
        let result = passing_result();
        let refused = emit_planned_run_row_result(
            &plan,
            &gate,
            &result,
            dir.path(),
            &receipt_root,
            &output_dir,
            None,
        );
        assert!(refused.is_err(), "unpublishable result must fail the invocation");
    }

    #[test]
    fn execution_error_is_never_started_with_no_invented_prerequisites() {
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
            dir.path(),
            &receipt_root,
            &dir.path().join("routed"),
            None,
        )
        .expect("emission succeeds honestly");
        assert!(matches!(built.product.outcome, TerminalOutcome::BlockedNotProven));
        assert!(!built.command_started);
        // No prerequisite evidence is invented for a never-started command:
        // the builder records Missing, never an assumed-ready fact (review
        // thread 3872200285).
        assert!(matches!(built.prerequisites.state, PrerequisiteState::Missing));
        assert!(matches!(built.instrument.outcome, TerminalOutcome::Missing));
    }

    #[test]
    fn hosted_identity_requires_the_complete_claimed_binding_set() {
        // Review thread 3872200290: a partial ambient GitHub environment is
        // a typed refusal, never a silently unbound hosted record.
        use std::collections::HashMap;
        let full: HashMap<String, String> = HashMap::from([
            ("GITHUB_WORKFLOW".to_string(), "PR Smoke".to_string()),
            ("GITHUB_JOB".to_string(), "fast".to_string()),
            ("GITHUB_RUN_ID".to_string(), "90210".to_string()),
            ("GITHUB_RUN_ATTEMPT".to_string(), "2".to_string()),
        ]);
        let collected = collect_hosted_identity_from(|name| full.get(name).cloned())
            .expect("complete environment binds");
        let hosted = collected.expect("hosted identity present");
        assert_eq!(hosted.run_attempt, 2);
        assert!(hosted.matrix.is_none(), "matrix absence is legal for a non-matrix job");

        let mut partial = full.clone();
        partial.remove("GITHUB_JOB");
        let refused = collect_hosted_identity_from(|name| partial.get(name).cloned());
        assert!(refused.is_err(), "run id without job must refuse");

        let mut missing_attempt = full.clone();
        missing_attempt.remove("GITHUB_RUN_ATTEMPT");
        let refused = collect_hosted_identity_from(|name| missing_attempt.get(name).cloned())
            .expect_err("run id without attempt must refuse");
        assert!(
            refused.to_string().contains("GITHUB_RUN_ATTEMPT missing"),
            "missing and malformed attempt identities must remain distinguishable: {refused}"
        );

        let offline = collect_hosted_identity_from(|_| None).expect("offline lookup");
        assert!(offline.is_none(), "no GitHub environment means no hosted identity");
    }

    #[test]
    fn plan_selection_must_match_the_plan_exactly_before_running() {
        // Both directions refuse before any gate executes (review thread
        // 3871822409): a plan run row the runner did not select, and a
        // selected gate the plan does not cover with an applicable run row.
        let plan = compiled_fixture();
        let fmt_gate = fmt_gate_definition();
        assert!(ensure_plan_covers_selection(&plan, &[&fmt_gate], false).is_ok());

        let other_gate = gate_definition_named("other_gate");
        let refused_missing = ensure_plan_covers_selection(&plan, &[&other_gate], false);
        assert!(refused_missing.is_err());
        assert!(refused_missing.err().unwrap().to_string().contains("fmt_gate"));

        let refused_extra = ensure_plan_covers_selection(&plan, &[&fmt_gate, &other_gate], false);
        assert!(
            refused_extra.is_err(),
            "a selected gate absent from the plan denominator must refuse before execution, got {:?}",
            refused_extra
        );
        assert!(refused_extra.err().unwrap().to_string().contains("other_gate"));
    }

    #[test]
    fn an_unrepresentable_duration_refuses_instead_of_degrading_to_never_started() {
        // Truncating the duration, or emptying the timing when the clock
        // cannot be read, would publish a command that did start as a
        // never-started record — the exact mis-attribution this record
        // exists to prevent, so it refuses instead.
        let plan = compiled_fixture();
        let gate = fmt_gate_definition();
        let mut result = passing_result();
        result.duration_ms = u64::MAX;

        let refused =
            observation_from_gate_result(&gate, &result, Path::new("."), Path::new("target"), None);
        assert!(
            refused.is_err(),
            "an unrepresentable duration on a started command must refuse, got {refused:?}"
        );

        // The ordinary duration still produces a bound observation window.
        let ok = observation_from_gate_result(
            &gate,
            &passing_result(),
            Path::new("."),
            Path::new("target"),
            None,
        )
        .expect("an ordinary duration observes cleanly");
        assert!(ok.timing.started_at_unix_ms.is_some());
        assert!(ok.timing.ended_at_unix_ms.is_some());
    }

    #[test]
    fn a_quarantined_selected_gate_does_not_abort_the_routed_run() {
        // A quarantined gate is selected into the runner's plan but returns
        // a skip without executing, so the compiled plan carries it as
        // `Quarantined`, not an applicable run row. Requiring a run row for
        // it aborted every routed run of a tier containing one, before any
        // gate executed.
        let plan = compiled_fixture();
        let fmt_gate = fmt_gate_definition();
        let mut quarantined = gate_definition_named("other_gate");
        quarantined.quarantine = true;

        let accepted = ensure_plan_covers_selection(&plan, &[&fmt_gate, &quarantined], false);
        assert!(
            accepted.is_ok(),
            "a tier carrying one run row and one quarantined row must still run, got {accepted:?}"
        );

        // Verbose runs actually execute quarantined gates, so there the
        // missing run row is a real plan mismatch again.
        let refused = ensure_plan_covers_selection(&plan, &[&fmt_gate, &quarantined], true);
        assert!(
            refused.is_err(),
            "a verbose run executes the quarantined gate, so the plan must cover it"
        );

        // The other direction: the plan expects a real execution of a gate
        // this runner would skip as quarantined.
        let mut quarantined_fmt = fmt_gate_definition();
        quarantined_fmt.quarantine = true;
        let disagreement = ensure_plan_covers_selection(&plan, &[&quarantined_fmt], false);
        assert!(
            disagreement.is_err(),
            "a planned run row for a locally quarantined gate is a policy disagreement"
        );
        assert!(disagreement.err().unwrap().to_string().contains("quarantine"));
    }

    #[test]
    fn plan_subject_and_row_execution_identity_bind_to_the_invocation() {
        // A valid but stale plan must refuse before running: its subject
        // head SHA and each row's execution identity (command/timeout) are
        // checked against the actual invocation (review thread 3871822396).
        let plan = compiled_fixture();
        assert!(ensure_plan_subject_matches_invocation(&plan, SHA_A).is_ok());
        let cross_sha = ensure_plan_subject_matches_invocation(&plan, SHA_B);
        assert!(cross_sha.is_err(), "cross-SHA plan must refuse before execution");
        assert!(cross_sha.err().unwrap().to_string().contains(SHA_B));

        let mut stale_gate = fmt_gate_definition();
        stale_gate.command = "cargo fmt --check --changed".to_string();
        let stale_command = ensure_plan_covers_selection(&plan, &[&stale_gate], false);
        assert!(
            stale_command.is_err(),
            "a gate whose command disagrees with the plan row must refuse before execution"
        );
        assert!(stale_command.err().unwrap().to_string().contains("execution identity"));

        let mut stale_timeout = fmt_gate_definition();
        stale_timeout.timeout_seconds = 61;
        let refused_timeout = ensure_plan_covers_selection(&plan, &[&stale_timeout], false);
        assert!(refused_timeout.is_err(), "a timeout-policy disagreement must refuse");
    }

    #[test]
    fn route_plan_refuses_a_dirty_execution_tree() {
        assert!(ensure_execution_tree_is_clean("").is_ok());
        for status in [" M tracked.rs", "M  staged.rs", "?? untracked.rs"] {
            let refused = ensure_execution_tree_is_clean(status)
                .expect_err("a route plan must not run against an unbound tree");
            assert!(refused.to_string().contains("clean tracked index"));
        }
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

    fn gate_definition_named(expected_name: &str) -> GateDefinition {
        let gate: GateDefinition = serde_yaml_ng::from_str(
            "name: other_gate
tier: merge_gate
description: fixture
required: true
command: cargo fmt --check
timeout_seconds: 60
",
        )
        .expect("gate definition");
        assert_eq!(gate.name, expected_name);
        gate
    }

    fn fmt_gate_definition() -> GateDefinition {
        serde_yaml_ng::from_str(
            "name: fmt_gate\ntier: merge_gate\ndescription: fixture\nrequired: true\ncommand: cargo fmt --check\ntimeout_seconds: 60\n",
        )
        .expect("gate definition")
    }

    fn passing_result() -> GateResult {
        // Honest for the in-process fmt gate: internal dispatch never
        // carries an exit code, so the record must not fabricate Some(0)
        // (review thread 3871822391).
        GateResult {
            gate_name: "fmt_gate".to_string(),
            tier: "merge_gate".to_string(),
            status: "pass".to_string(),
            required: Some(true),
            duration_ms: 3500,
            command: "cargo fmt --check".to_string(),
            exit_code: None,
            output_summary: Some("Executed internally via xtask task dispatch".to_string()),
            log_path: Some("logs/fmt_gate.log".to_string()),
            metrics: None,
            artifacts: None,
            first_failure: None,
        }
    }

    #[test]
    fn process_pass_with_clean_exit_still_normalizes_as_success() {
        // The command-executed path (exit code observed by the runner)
        // stays a clean process success, not an in-process record.
        let plan = compiled_fixture();
        let dir = tempfile::tempdir().expect("tempdir");
        let receipt_root = dir.path().join("target/receipts");
        let log_dir = receipt_root.join("logs");
        std::fs::create_dir_all(&log_dir).expect("log dir");
        std::fs::write(log_dir.join("fmt_gate.log"), b"unit clean").expect("log");

        let gate = fmt_gate_definition();
        let mut result = passing_result();
        result.exit_code = Some(0);
        result.output_summary = Some("command output".to_string());
        let built = emit_planned_run_row_result(
            &plan,
            &gate,
            &result,
            dir.path(),
            &receipt_root,
            &dir.path().join("routed"),
            None,
        )
        .expect("emission succeeds");
        assert!(matches!(built.product.outcome, TerminalOutcome::Success));
        assert!(!built.child.in_process);
        assert_eq!(built.child.exit_code, Some(0));
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
    fn receipt_fixture() -> crate::tasks::ci_subject::CiSubjectReceipt {
        use crate::tasks::ci_subject::{
            CiEventKind, CiSubjectReceipt, SubjectDiffMode, SubjectResolutionSource, SubjectStatus,
        };
        CiSubjectReceipt {
            schema_version: "ci_subject.v1".to_string(),
            producer: "test".to_string(),
            status: SubjectStatus::Resolved,
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            event_kind: CiEventKind::PullRequest,
            resolution_source: SubjectResolutionSource::ExplicitInput,
            diff_mode: SubjectDiffMode::MergeBase,
            base_sha: SHA_B.to_string(),
            head_sha: SHA_A.to_string(),
            base_tree: SHA_B.to_string(),
            head_tree: SHA_A.to_string(),
            diff_base_sha: SHA_B.to_string(),
            diff_base_tree: SHA_B.to_string(),
            changed_file_count: 0,
            changed_input_digest: DIGEST_B.to_string(),
            subject_digest: DIGEST_A.to_string(),
            error_code: None,
        }
    }

    #[test]
    fn plan_subject_binds_every_mirrored_receipt_field_not_only_the_digest() {
        let plan = compiled_fixture();
        let receipt = receipt_fixture();
        assert!(ensure_plan_subject_fields_match_receipt(&plan.subject, &receipt).is_ok());

        // Same digest and head, different base: a false subject identity under
        // a valid digest must be refused.
        let mut foreign_base = plan.subject.clone();
        foreign_base.base_sha = Some(SHA_A.to_string());
        let refused = ensure_plan_subject_fields_match_receipt(&foreign_base, &receipt);
        assert!(refused.is_err(), "base mismatch must refuse, got {refused:?}");
        assert!(refused.unwrap_err().to_string().contains("subject base"));

        let mut foreign_kind = plan.subject.clone();
        foreign_kind.kind = "merge_group".to_string();
        let refused = ensure_plan_subject_fields_match_receipt(&foreign_kind, &receipt);
        assert!(refused.is_err(), "kind mismatch must refuse, got {refused:?}");
        assert!(refused.unwrap_err().to_string().contains("subject kind"));

        // A plan that carries no base binds nothing extra and stays accepted.
        let mut baseless = plan.subject.clone();
        baseless.base_sha = None;
        assert!(ensure_plan_subject_fields_match_receipt(&baseless, &receipt).is_ok());
    }

    #[test]
    fn plan_selection_authority_binds_runner_profile_and_base() {
        let plan = compiled_fixture();
        assert!(
            ensure_plan_authority_matches_invocation(&plan, &GateTier::MergeGate, Some(SHA_B))
                .is_ok()
        );
        assert!(
            ensure_plan_authority_matches_invocation(&plan, &GateTier::MergeGate, None).is_ok(),
            "a runner without a base ref binds the profile only"
        );

        let foreign_profile =
            ensure_plan_authority_matches_invocation(&plan, &GateTier::PrFast, Some(SHA_B));
        assert!(foreign_profile.is_err(), "profile mismatch must refuse: {foreign_profile:?}");
        assert!(foreign_profile.unwrap_err().to_string().contains("profile"));

        let foreign_base =
            ensure_plan_authority_matches_invocation(&plan, &GateTier::MergeGate, Some(SHA_A));
        assert!(foreign_base.is_err(), "base mismatch must refuse: {foreign_base:?}");
        assert!(foreign_base.unwrap_err().to_string().contains("selection base"));
    }

    #[test]
    fn declared_artifacts_are_projected_or_named_as_shortfall() {
        let dir = tempfile::tempdir().expect("tempdir");
        let receipt_root = dir.path().join("target/receipts");
        std::fs::create_dir_all(receipt_root.join("logs")).expect("log dir");
        std::fs::write(receipt_root.join("logs/fmt_gate.log"), b"fmt clean").expect("log");
        std::fs::write(receipt_root.join("present.json"), b"{}").expect("artifact");

        let gate = fmt_gate_definition();
        let mut result = passing_result();
        result.artifacts = Some(vec![
            "target/receipts/present.json".to_string(),
            "target/receipts/absent.json".to_string(),
        ]);
        let observation =
            observation_from_gate_result(&gate, &result, dir.path(), &receipt_root, None)
                .expect("observation");
        let roles: Vec<(&str, &str)> = observation
            .artifacts
            .iter()
            .map(|artifact| (artifact.role.as_str(), artifact.path.as_str()))
            .collect();
        assert!(roles.contains(&("log", "target/receipts/logs/fmt_gate.log")), "{roles:?}");
        assert!(roles.contains(&("artifact", "target/receipts/present.json")), "{roles:?}");
        assert!(
            observation
                .artifacts
                .iter()
                .all(|artifact| artifact.sha256.as_deref().is_some_and(|sha| sha.len() == 64)),
            "every projected artifact carries a bounded sha256"
        );
        assert!(
            observation.receipt_shortfall.iter().any(|entry| entry.contains("absent.json")),
            "a declared-but-missing artifact is named as a reporting shortfall: {:?}",
            observation.receipt_shortfall
        );

        // A never-started command declares nothing.
        let never_started =
            observation_from_gate_result(&gate, &error_result(), dir.path(), &receipt_root, None)
                .expect("observation");
        assert!(never_started.artifacts.is_empty());
    }

    fn _type_witness(_: Applicability) {}
}
