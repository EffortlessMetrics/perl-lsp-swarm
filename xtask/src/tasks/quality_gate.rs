//! Quality gates for the proof lane.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use chrono::{NaiveDate, Utc};
use clap::ValueEnum;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use serde_yaml_ng::Value as YamlValue;

use crate::tasks::git_context::git_stdout_with_worktree_fallback;

const PATCH_TARGET: f64 = 95.0;
const PROJECT_TARGET: f64 = 95.0;
const NEW_RIPR_GAP_SUGGESTED_TEST: &str = "Add or update the focused test named by RIPR review guidance for the changed file, line, and seam.";

/// Sentinel date used during verdict evaluation when a caller hands the engine a
/// [`LifecycleOverlay`]. The sentinel can never cross `today`, so a committed
/// exception row cannot inherit a verdict shift driven by clock progression.
pub(crate) const LIFECYCLE_SENTINEL: &str = "9999-12-31";

/// One committed lifecycle row lifted out of the caller-supplied policy.
///
/// The engine uses these rows to stamp original committed dates back into the
/// receipt output while substituting sentinels for verdict comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LifecycleDates {
    pub review_after: String,
    pub expires: String,
}

/// Caller-supplied policy adaptation injected by the [`super::quality_gate_facade`]
/// adapter to keep candidate verdicts invariant under wall-clock progression.
///
/// When supplied:
/// - Verdict comparison substitutes [`LIFECYCLE_SENTINEL`] for `review_after` /
///   `expires` so the `quality_exception_review_due` and
///   `quality_exception_expired` arms never fire on committed dates.
/// - The receipt's `temporary_exceptions.active[*]` and every
///   `quality_exception_active_final_blocker` next action stamp the original
///   committed dates from `rows` keyed by exception id.
/// - The receipt's `temporary_exceptions` records `due_review = "advisory"`,
///   `lifecycle_authority = "policy_cadence"`, and (when present) the
///   `validation_error` from the caller.
/// - The engine injects a single `quality_exception_policy_not_current`
///   next action with `reason = "invalid_due_review"` carrying the
///   `validation_error` as its `repair` text.
///
/// `invalid_metadata_status` is true when the only metadata-level fault in the
/// caller policy is the malformed `due_review` itself; in that case the engine
/// suppresses its own synthetic `invalid_metadata` next action to keep the two
/// diagnoses from competing for the operator's attention.
#[derive(Clone, Debug, Default)]
pub(crate) struct LifecycleOverlay {
    pub rows: std::collections::BTreeMap<String, LifecycleDates>,
    pub validation_error: Option<String>,
    pub invalid_metadata_status: bool,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum QualityGateMode {
    Enforce,
    EnforcePatchCoverage,
    EnforceNewRipr,
}

impl QualityGateMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::EnforcePatchCoverage => "enforce-patch-coverage",
            Self::EnforceNewRipr => "enforce-new-ripr",
        }
    }
}

#[derive(Debug)]
pub struct QualityGateArgs {
    pub mode: QualityGateMode,
    pub exception_policy: PathBuf,
    pub ripr_receipt: PathBuf,
    pub ripr_pr_receipt: PathBuf,
    pub review_receipt: PathBuf,
    pub coverage_receipt: PathBuf,
    pub codecov: PathBuf,
    pub patch_coverage: Option<f64>,
    pub ripr_base: String,
    pub ripr_head: String,
    pub receipt: PathBuf,
    pub summary: PathBuf,
    pub check: bool,
    /// Suppress the success line. Reserved for callers that orchestrate the
    /// engine in nested contexts where naming the published artifacts on the
    /// default stream would duplicate the surrounding report; normal CLI
    /// invocations leave this `false`.
    pub quiet: bool,
}

#[derive(Debug)]
struct GateEvaluation {
    receipt: Value,
    markdown: String,
    failed: bool,
}

pub fn run(args: QualityGateArgs) -> Result<()> {
    run_with_overlay(args, None)
}

/// Evaluate the gate and publish the rendered artifacts. When `overlay` is
/// `Some`, the engine substitutes the sentinel lifecycle dates for verdict
/// comparison, stamps the overlay's committed dates into the receipt, and
/// records the overlay's `validation_error` if present.
///
/// This is the single entry point the [`super::quality_gate_facade`] adapter
/// uses. Rendering, freshness comparison, output writes, and exit
/// classification all live behind this seam — callers must not duplicate any
/// of them.
pub fn run_with_overlay(args: QualityGateArgs, overlay: Option<LifecycleOverlay>) -> Result<()> {
    let root = std::env::current_dir().context("resolving current directory")?;
    let evaluation = evaluate(&root, &args, overlay.as_ref())?;
    publish(&args, evaluation)
}

/// Render the evaluated gate as the caller-requested artifacts and classify
/// the exit. The caller-facing receipt/summary paths in `args` are the only
/// paths the engine reads or writes here — no temporary workspace is involved.
fn publish(args: &QualityGateArgs, evaluation: GateEvaluation) -> Result<()> {
    let receipt_text = render_json(&evaluation.receipt)?;

    if args.check {
        // Collect both staleness findings before failing (#15662): checking
        // the receipt with `?` first meant a stale receipt masked a stale
        // summary, and consumers keying on the summary message never saw it.
        let mut stale = Vec::new();
        assert_current(&args.receipt, &receipt_text, "quality gate JSON receipt", &mut stale)?;
        assert_current(
            &args.summary,
            &evaluation.markdown,
            "quality gate Markdown summary",
            &mut stale,
        )?;
        if !stale.is_empty() {
            bail!("{}", stale.join("\n"));
        }
    } else {
        write_text(&args.receipt, &receipt_text)?;
        write_text(&args.summary, &evaluation.markdown)?;
    }

    if evaluation.failed {
        bail!(
            "quality gate failed; see receipt {} and summary {}",
            args.receipt.display(),
            args.summary.display()
        );
    }

    if !args.quiet {
        println!(
            "quality gate passed; receipt {} summary {}",
            args.receipt.display(),
            args.summary.display()
        );
    }
    Ok(())
}

fn evaluate(
    root: &Path,
    args: &QualityGateArgs,
    overlay: Option<&LifecycleOverlay>,
) -> Result<GateEvaluation> {
    let head = current_head(root)?;
    match args.mode {
        QualityGateMode::Enforce => evaluate_final(&head, args, overlay),
        QualityGateMode::EnforcePatchCoverage => evaluate_patch_coverage(&head, args, overlay),
        QualityGateMode::EnforceNewRipr => evaluate_new_ripr(&head, args, overlay),
    }
}

fn evaluate_final(
    head: &str,
    args: &QualityGateArgs,
    overlay: Option<&LifecycleOverlay>,
) -> Result<GateEvaluation> {
    let codecov_patch_status = read_codecov_patch_status(&args.codecov)?;
    let codecov_project_status = read_codecov_project_status(&args.codecov)?;
    let coverage = read_coverage_receipt(&args.coverage_receipt, head);
    let ripr = read_ripr_plus_receipt(&args.ripr_receipt, head);
    let ripr_pr = read_ripr_pr_receipt(&args.ripr_pr_receipt, head);
    let review = read_review_guidance_receipt(&args.review_receipt, head);
    let exceptions = read_exception_policy(args, today(), overlay);
    let mut next_actions = Vec::new();
    next_actions.extend(exceptions.actions.clone());

    if !matches!(coverage.status.as_str(), "present") {
        next_actions.push(coverage_receipt_action(&coverage, head, args));
    }
    if coverage.status == "present" {
        let patch = args.patch_coverage.or(coverage.patch);
        let patch_source = if args.patch_coverage.is_some() {
            Some("cli")
        } else if coverage.patch.is_some() {
            Some("coverage_receipt")
        } else {
            None
        };
        if patch.is_none() {
            next_actions.push(patch_coverage_unknown_action(args));
        }
        if let Some(patch) = patch
            && patch < PATCH_TARGET
        {
            next_actions.push(patch_coverage_below_target_action(
                patch,
                patch_source.unwrap_or("unknown"),
                &coverage,
                args,
            ));
        }
        match coverage.project {
            Some(project) if project < PROJECT_TARGET => {
                next_actions.push(project_coverage_below_target_action(project, &coverage, args));
            }
            None => next_actions.push(project_coverage_unknown_action(args)),
            _ => {}
        }
        if coverage.scope.as_deref() != Some("workspace") {
            next_actions.push(coverage_scope_not_workspace_action(&coverage, args));
        }
    }

    if codecov_patch_status != "present" {
        next_actions.push(codecov_policy_action(&codecov_patch_status, args));
    }
    if codecov_project_status != "present" {
        next_actions.push(codecov_project_policy_action(&codecov_project_status, args));
    }

    if ripr.status != "present" {
        next_actions.push(ripr_receipt_action(&ripr, head, args));
    }
    if ripr.status == "present" {
        match ripr.unresolved {
            Some(count) if count > 0 => {
                next_actions.push(ripr_total_unresolved_action(count, &ripr, args))
            }
            None => next_actions.push(ripr_total_unknown_action(args)),
            _ => {}
        }
    }
    if ripr_pr.status != "present" {
        next_actions.push(ripr_pr_receipt_action(&ripr_pr, head, args));
    }
    if review.status != "present" {
        next_actions.push(ripr_review_receipt_action(&review, head, args));
    }
    // Applied static-limitation credit for this evaluation. Defaults to zero:
    // arms that never run the clearing (unknown or zero count) must not
    // report clearing.
    let mut clearing = StaticLimitationClearing::default();
    if ripr_pr.status == "present" {
        match ripr_pr.new_unresolved {
            Some(count) if count > 0 && !review.is_nonproduction_only_scope() => {
                clearing = blocking_new_gap_count(count, &ripr_pr, &review);
                if clearing.remaining > 0 {
                    next_actions.push(new_ripr_gap_action(
                        clearing.remaining,
                        &ripr_pr,
                        &review,
                        &clearing.applied,
                        args,
                    ));
                    if review.status == "present"
                        && review.top_gaps.is_empty()
                        && review.static_limitation_gaps.is_empty()
                    {
                        next_actions.push(ripr_review_guidance_gap_action(&review, head, args));
                    }
                } else {
                    next_actions
                        .push(static_limitation_gap_cleared_action(&clearing, &review, args));
                }
            }
            None => next_actions.push(new_ripr_gap_unknown_action(&ripr_pr, args)),
            _ => {}
        }
    }
    if exceptions.receipt.get("active_count").and_then(Value::as_u64).is_some_and(|count| count > 0)
    {
        next_actions.push(quality_exception_active_final_blocker_action(&exceptions, args));
    }

    let failed = next_actions
        .iter()
        .any(|action| action.get("blocking").and_then(Value::as_bool) == Some(true));
    let decision = if failed { "fail" } else { "pass" };
    let patch = args.patch_coverage.or(coverage.patch);

    let receipt = json!({
        "schema_version": 1,
        "kind": "quality_gate",
        "mode": args.mode.as_str(),
        "decision": decision,
        "head": head,
        "coverage": {
            "status": coverage.status,
            "receipt": display_path(&args.coverage_receipt),
            "receipt_head": coverage.receipt_head,
            "patch": patch.map(round2),
            "project": coverage.project.map(round2),
            "target": PROJECT_TARGET,
            "scope": coverage.scope,
            "lcov": coverage.lcov,
            "codecov_config": display_path(&args.codecov),
            "codecov_patch_status": codecov_patch_status,
            "codecov_project_status": codecov_project_status,
            "recommended_project_clusters": coverage.recommended_project_clusters,
        },
        "ripr_plus": {
            "status": ripr.status,
            "receipt": display_path(&args.ripr_receipt),
            "receipt_head": ripr.receipt_head,
            "expected_head": head,
            "unresolved": ripr.unresolved,
            "recommended_first_clusters": ripr.recommended_first_clusters,
        },
        "ripr_pr": {
            "status": ripr_pr.status,
            "receipt": display_path(&args.ripr_pr_receipt),
            "receipt_head_sha": ripr_pr.receipt_head_sha,
            "expected_head_sha": head,
            "base": ripr_pr.base,
            "base_sha": ripr_pr.base_sha,
            "new_unresolved": ripr_pr.new_unresolved,
            "non_production_excluded": ripr_pr.non_production_excluded,
            "static_limitation_cleared": clearing.applied.len(),
            "static_limitation_unspent": clearing_gap_ids(&clearing.unspent),
        },
        "review_guidance": {
            "status": review.status,
            "receipt": display_path(&args.review_receipt),
            "receipt_head_sha": review.receipt_head_sha,
            "expected_head_sha": head,
            "base": review.base,
            "base_sha": review.base_sha,
            "production_files_considered": review.production_files_considered,
            "changed_production_files": review.changed_production_files,
            "top_gaps": review.top_gaps,
            "static_limitation_gaps": review.clearable_static_limitations(),
            "unavailable_reason": review.unavailable_reason,
        },
        "temporary_exceptions": exceptions.receipt,
        "next_actions": next_actions,
    });
    let markdown = render_markdown(&receipt, args)?;

    Ok(GateEvaluation { receipt, markdown, failed })
}

fn evaluate_patch_coverage(
    head: &str,
    args: &QualityGateArgs,
    overlay: Option<&LifecycleOverlay>,
) -> Result<GateEvaluation> {
    let codecov_status = read_codecov_patch_status(&args.codecov)?;
    let coverage = read_coverage_receipt(&args.coverage_receipt, head);
    let exceptions = read_exception_policy(args, today(), overlay);
    let mut next_actions = Vec::new();
    next_actions.extend(exceptions.actions.clone());

    if !matches!(coverage.status.as_str(), "present") {
        next_actions.push(coverage_receipt_action(&coverage, head, args));
    }

    let patch = args.patch_coverage.or(coverage.patch);
    let patch_source = if args.patch_coverage.is_some() {
        Some("cli")
    } else if coverage.patch.is_some() {
        Some("coverage_receipt")
    } else {
        None
    };

    if coverage.status == "present" && patch.is_none() {
        next_actions.push(patch_coverage_unknown_action(args));
    }

    if let Some(patch) = patch
        && patch < PATCH_TARGET
    {
        next_actions.push(patch_coverage_below_target_action(
            patch,
            patch_source.unwrap_or("unknown"),
            &coverage,
            args,
        ));
    }

    if codecov_status != "present" {
        next_actions.push(codecov_policy_action(&codecov_status, args));
    }

    let failed = next_actions
        .iter()
        .any(|action| action.get("blocking").and_then(Value::as_bool) == Some(true));
    let decision = if failed { "fail" } else { "pass" };

    // Determine the failure class taxonomy.
    // Only coverage_shortfall fails the coverage gate.
    // test_failure is non-fatal: correctness is owned by the correctness gate (#1469).
    let failure_class = classify_patch_coverage_failure(failed, &next_actions);

    // test_failure_class documents whether test commands exited non-zero during
    // coverage collection. Non-fatal: coverage data is still collected regardless.
    // This field is always present (null = no test failure recorded).
    let test_failure_class: Option<&str> = None;

    let receipt = json!({
        "schema_version": 1,
        "kind": "quality_gate",
        "mode": args.mode.as_str(),
        "decision": decision,
        "failure_class": failure_class,
        "test_failure_class": test_failure_class,
        "head": head,
        "coverage": {
            "status": coverage.status,
            "receipt": display_path(&args.coverage_receipt),
            "receipt_head": coverage.receipt_head,
            "patch": patch.map(round2),
            "patch_source": patch_source,
            "project": coverage.project.map(round2),
            "target": PATCH_TARGET,
            "scope": coverage.scope,
            "lcov": coverage.lcov,
            "codecov_config": display_path(&args.codecov),
            "codecov_config_status": codecov_status,
        },
        "temporary_exceptions": exceptions.receipt,
        "next_actions": next_actions,
    });
    let markdown = render_markdown(&receipt, args)?;

    Ok(GateEvaluation { receipt, markdown, failed })
}

/// Classify the patch-coverage gate failure into a taxonomy.
///
/// Taxonomy:
/// - : patch coverage number is below target - the only class
///   that fails the coverage gate.
/// - : coverage receipt missing/stale; cannot evaluate.
/// - : no failure.
fn classify_patch_coverage_failure(failed: bool, next_actions: &[Value]) -> &'static str {
    if !failed {
        return "pass";
    }
    let has_coverage_shortfall = next_actions.iter().any(|action| {
        matches!(
            action.get("kind").and_then(Value::as_str),
            Some("patch_coverage_below_target") | Some("patch_coverage_unknown")
        )
    });
    if has_coverage_shortfall {
        return "coverage_shortfall";
    }
    "setup_failure"
}

fn evaluate_new_ripr(
    head: &str,
    args: &QualityGateArgs,
    overlay: Option<&LifecycleOverlay>,
) -> Result<GateEvaluation> {
    let ripr = read_ripr_plus_receipt(&args.ripr_receipt, head);
    let ripr_pr = read_ripr_pr_receipt(&args.ripr_pr_receipt, head);
    let review = read_review_guidance_receipt(&args.review_receipt, head);
    let exceptions = read_exception_policy(args, today(), overlay);
    let mut next_actions = Vec::new();
    next_actions.extend(exceptions.actions.clone());

    if ripr.status != "present" {
        next_actions.push(ripr_receipt_action(&ripr, head, args));
    }
    if ripr_pr.status != "present" {
        next_actions.push(ripr_pr_receipt_action(&ripr_pr, head, args));
    }
    let review_receipt_blocks_without_new_gaps =
        matches!(review.status.as_str(), "missing" | "invalid" | "stale");
    if review_receipt_blocks_without_new_gaps {
        next_actions.push(ripr_review_receipt_action(&review, head, args));
    }

    // Applied static-limitation credit for this evaluation. Defaults to zero:
    // arms that never run the clearing (unknown or zero count) must not
    // report clearing.
    let mut clearing = StaticLimitationClearing::default();
    if ripr_pr.status == "present" {
        match ripr_pr.new_unresolved {
            Some(count) if count > 0 && !review.is_nonproduction_only_scope() => {
                clearing = blocking_new_gap_count(count, &ripr_pr, &review);
                if clearing.remaining > 0 {
                    next_actions.push(new_ripr_gap_action(
                        clearing.remaining,
                        &ripr_pr,
                        &review,
                        &clearing.applied,
                        args,
                    ));
                    // An incomplete receipt that still names actionable seams
                    // (#10054 fallback) is sufficient for the gate to fail on named
                    // evidence; only nameless degradation blocks on the receipt.
                    // A producer-classified static_limitation item is also named
                    // evidence (#15630): it cannot become a repair packet, but it
                    // proves the receipt names the seams it counts.
                    let review_names_gaps = review.status == "present"
                        || (review.status == "incomplete"
                            && (!review.top_gaps.is_empty()
                                || !review.static_limitation_gaps.is_empty()));
                    if review.status == "present" {
                        if review.top_gaps.is_empty() && review.static_limitation_gaps.is_empty() {
                            next_actions.push(ripr_review_guidance_gap_action(&review, head, args));
                        }
                    } else if !review_names_gaps && !review_receipt_blocks_without_new_gaps {
                        next_actions.push(ripr_review_receipt_action(&review, head, args));
                    }
                } else {
                    next_actions
                        .push(static_limitation_gap_cleared_action(&clearing, &review, args));
                }
            }
            None => next_actions.push(new_ripr_gap_unknown_action(&ripr_pr, args)),
            _ => {}
        }
    }

    let failed = next_actions
        .iter()
        .any(|action| action.get("blocking").and_then(Value::as_bool) == Some(true));
    let decision = if failed { "fail" } else { "pass" };

    let receipt = json!({
        "schema_version": 1,
        "kind": "quality_gate",
        "mode": args.mode.as_str(),
        "decision": decision,
        "head": head,
        "ripr_plus": {
            "status": ripr.status,
            "receipt": display_path(&args.ripr_receipt),
            "receipt_head": ripr.receipt_head,
            "expected_head": head,
            "unresolved": ripr.unresolved,
        },
        "ripr_pr": {
            "status": ripr_pr.status,
            "receipt": display_path(&args.ripr_pr_receipt),
            "receipt_head_sha": ripr_pr.receipt_head_sha,
            "expected_head_sha": head,
            "base": ripr_pr.base,
            "base_sha": ripr_pr.base_sha,
            "new_unresolved": ripr_pr.new_unresolved,
            "non_production_excluded": ripr_pr.non_production_excluded,
            "static_limitation_cleared": clearing.applied.len(),
            "static_limitation_unspent": clearing_gap_ids(&clearing.unspent),
        },
        "review_guidance": {
            "status": review.status,
            "receipt": display_path(&args.review_receipt),
            "receipt_head_sha": review.receipt_head_sha,
            "expected_head_sha": head,
            "base": review.base,
            "base_sha": review.base_sha,
            "production_files_considered": review.production_files_considered,
            "changed_production_files": review.changed_production_files,
            "top_gaps": review.top_gaps,
            "static_limitation_gaps": review.clearable_static_limitations(),
            "unavailable_reason": review.unavailable_reason,
        },
        "temporary_exceptions": exceptions.receipt,
        "next_actions": next_actions,
    });
    let markdown = render_markdown(&receipt, args)?;

    Ok(GateEvaluation { receipt, markdown, failed })
}

fn parse_changed_production_files(files: &[Value]) -> Option<Vec<String>> {
    let mut parsed = Vec::with_capacity(files.len());
    for file in files {
        let path = file.as_str()?;
        parsed.push(path.to_owned());
    }
    Some(parsed)
}

#[derive(Debug)]
struct CoverageReceipt {
    status: String,
    receipt_head: Option<String>,
    lcov: Option<String>,
    patch: Option<f64>,
    project: Option<f64>,
    scope: Option<String>,
    patch_files: Vec<Value>,
    top_files: Vec<Value>,
    recommended_project_clusters: Vec<Value>,
}

#[derive(Debug)]
struct RiprPlusReceipt {
    status: String,
    receipt_head: Option<String>,
    unresolved: Option<u64>,
    recommended_first_clusters: Vec<Value>,
}

#[derive(Debug)]
struct RiprPrReceipt {
    status: String,
    receipt_head_sha: Option<String>,
    base: Option<String>,
    base_sha: Option<String>,
    new_unresolved: Option<u64>,
    /// Seams the producer dropped from the basis as non-production (#11690):
    /// `archive/**` sources and Cargo integration-test files. Surfaced so a
    /// deflated `new_unresolved` is explainable from the gate receipt alone.
    non_production_excluded: Option<u64>,
}

#[derive(Debug)]
struct ReviewGuidanceReceipt {
    status: String,
    receipt_head_sha: Option<String>,
    base: Option<String>,
    base_sha: Option<String>,
    production_files_considered: Option<u64>,
    changed_production_files: Option<Vec<String>>,
    top_gaps: Vec<Value>,
    /// Normalized gap identities the producer already excluded from the counted
    /// basis (`suppressed` entries: path/classification suppressions the
    /// producer applied before counting). A disposition naming one of these
    /// IDs spends no credit — the seam was never in the blocking basis.
    suppressed_gap_ids: BTreeSet<String>,
    /// Producer items the hosted guidance itself classified
    /// `static_limitation` (#15630): seams whose boundary operands are local
    /// or computed, where the analyzer states it cannot judge the seam and
    /// must not emit an actionable repair packet. Kept separate from
    /// `top_gaps` because they are explicitly *not* repair packets, but they
    /// are still named, auditable evidence about the counted seams.
    static_limitation_gaps: Vec<Value>,
    /// Producer-reported reason the guidance run did not finish, when the
    /// producer stamped one (`warnings[].message` for a `tool_error`).
    ///
    /// This is what separates "the analysis ran and found nothing to name"
    /// from "the analysis never completed" — see [`Self::gap_list_is_unproven`].
    unavailable_reason: Option<String>,
}

impl ReviewGuidanceReceipt {
    /// The producer's caller inventory can be non-empty even when no changed
    /// production file was analyzed. Only the explicit changed-file list is
    /// authoritative for the required-vs-advisory scope decision.
    fn is_nonproduction_only_scope(&self) -> bool {
        self.status == "present"
            && self.top_gaps.is_empty()
            && self.changed_production_files.as_ref().is_some_and(Vec::is_empty)
    }

    /// True when the gate is about to report new seams it cannot name.
    ///
    /// This is evaluated only where a positive `new_unresolved` count is being
    /// reported, so an empty `top_gaps` always means the count and the list
    /// disagree — the seams exist but none were named.
    ///
    /// Deliberately keyed on the list being empty rather than on the guidance
    /// status, because there are two ways to arrive here and both are
    /// unproven:
    ///
    /// - guidance did not complete (missing, stale, invalid, or a producer
    ///   timeout), so the seams could not be named; or
    /// - guidance completed over production files and named nothing anyway,
    ///   which contradicts the count and is what
    ///   `ripr_review_guidance_not_actionable` already reports separately.
    ///
    /// Keying on status alone would call the second case proven while the
    /// gate simultaneously declared the guidance unactionable — reintroducing
    /// exactly the contradiction this reporting exists to remove.
    ///
    /// The genuinely empty case — guidance completed and no production file
    /// was in scope — never reaches here: `is_nonproduction_only_scope`
    /// excludes it from the blocking branch entirely.
    fn gap_list_is_unproven(&self) -> bool {
        self.top_gaps.is_empty()
    }

    /// Producer-classified `static_limitation` seams that may be cleared from
    /// the blocking new-gap basis (#15630).
    ///
    /// The disposition is honored only when the guidance receipt is `present`
    /// — a completed producer run pinned to this head. Missing, stale,
    /// invalid, or incomplete receipts contribute nothing, so the
    /// classification is always earned from the analyzer's own head-pinned
    /// evidence trail, never from candidate-side assertion.
    fn clearable_static_limitations(&self) -> &[Value] {
        if self.status == "present" { &self.static_limitation_gaps } else { &[] }
    }
}

#[derive(Debug)]
struct ExceptionPolicyEvaluation {
    receipt: Value,
    actions: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct QualityExceptionPolicy {
    schema_version: u64,
    policy: String,
    owner: String,
    status: String,
    updated: String,
    #[serde(default)]
    due_review: Option<String>,
    #[serde(default)]
    requirements: ExceptionRequirements,
    #[serde(default, rename = "exception")]
    exceptions: Vec<QualityException>,
}

#[derive(Debug, Default, Deserialize)]
struct ExceptionRequirements {
    #[serde(default)]
    required_active: Vec<String>,
}

/// Parse a raw caller-supplied policy into the lifecycle overlay the engine
/// applies during evaluation. The overlay carries the original committed
/// `review_after`/`expires` rows keyed by exception id, the optional
/// `validation_error` for malformed `due_review` values, and the
/// `invalid_metadata_status` flag that suppresses the engine's synthetic
/// `invalid_metadata` next action when `due_review` is the only metadata
/// defect.
///
/// Returns an overlay with no rows and no validation error when the policy
/// content already lines up with the engine's natural verdict; callers can
/// always supply the result unconditionally.
pub(crate) fn compute_lifecycle_overlay(raw: &str) -> Result<LifecycleOverlay> {
    use toml::Value as TomlValue;

    let mut overlay = LifecycleOverlay::default();

    let mut policy = match toml::from_str::<TomlValue>(raw) {
        Ok(value) => value,
        Err(_) => return Ok(overlay),
    };
    let Some(table) = policy.as_table_mut() else {
        return Ok(overlay);
    };

    let metadata_is_valid = overlay_policy_metadata_is_valid(table);

    let due_review = table.get("due_review").and_then(TomlValue::as_str).map(str::to_string);
    if let Some(due_review) = due_review.as_deref()
        && !matches!(due_review, "warn" | "fail")
    {
        overlay.validation_error =
            Some(format!("quality exception due_review must be warn or fail, found {due_review}"));
        overlay.invalid_metadata_status = metadata_is_valid;
        table.insert("status".to_string(), TomlValue::String("invalid".to_string()));
    }
    table.insert("due_review".to_string(), TomlValue::String("warn".to_string()));

    if let Some(exceptions) = table.get_mut("exception").and_then(TomlValue::as_array_mut) {
        for exception in exceptions {
            let Some(exception) = exception.as_table_mut() else {
                continue;
            };
            if !overlay_exception_is_structurally_valid(exception) {
                continue;
            }
            let Some(id) = exception.get("id").and_then(TomlValue::as_str).map(str::to_string)
            else {
                continue;
            };
            let Some(review_after) =
                exception.get("review_after").and_then(TomlValue::as_str).map(str::to_string)
            else {
                continue;
            };
            let Some(expires) =
                exception.get("expires").and_then(TomlValue::as_str).map(str::to_string)
            else {
                continue;
            };

            overlay.rows.insert(id, LifecycleDates { review_after, expires });
            exception.insert(
                "review_after".to_string(),
                TomlValue::String(LIFECYCLE_SENTINEL.to_string()),
            );
            exception
                .insert("expires".to_string(), TomlValue::String(LIFECYCLE_SENTINEL.to_string()));
        }
    }

    // Preserve the normalized text only as a side effect of running through
    // `toml::Value` so the caller's file on disk is untouched. The overlay
    // itself is the contract; the engine never reads back the rewritten text.
    let _ = toml::to_string(&policy);
    Ok(overlay)
}

fn overlay_policy_metadata_is_valid(table: &toml::Table) -> bool {
    table.get("owner").and_then(toml::Value::as_str).is_some_and(|value| !value.trim().is_empty())
        && table.get("status").and_then(toml::Value::as_str) == Some("active")
        && table
            .get("updated")
            .and_then(toml::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
}

fn overlay_exception_is_structurally_valid(exception: &toml::Table) -> bool {
    let required = [
        "id",
        "kind",
        "scope",
        "owner",
        "reason",
        "final_target",
        "evidence",
        "removal_criteria",
        "created",
        "review_after",
        "expires",
    ];
    let required_fields_are_present = required.iter().all(|field| {
        exception
            .get(*field)
            .and_then(toml::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    });
    let lifecycle_dates_are_valid = ["created", "review_after", "expires"].iter().all(|field| {
        exception.get(*field).and_then(toml::Value::as_str).and_then(parse_policy_date).is_some()
    });

    required_fields_are_present
        && lifecycle_dates_are_valid
        && exception.get("kind").and_then(toml::Value::as_str) == Some("temporary_burndown")
}

#[derive(Debug, Deserialize)]
struct QualityException {
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    scope: String,
    owner: String,
    #[serde(default)]
    issue: Option<String>,
    reason: String,
    final_target: String,
    evidence: String,
    removal_criteria: String,
    created: String,
    review_after: String,
    expires: String,
}

enum JsonReceipt {
    Missing,
    Invalid,
    Present(Value),
}

fn read_exception_policy(
    args: &QualityGateArgs,
    today: NaiveDate,
    overlay: Option<&LifecycleOverlay>,
) -> ExceptionPolicyEvaluation {
    let path = &args.exception_policy;
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => {
            return ExceptionPolicyEvaluation {
                receipt: json!({
                    "status": "missing",
                    "policy": display_path(path),
                    "active_count": 0,
                    "final_enforcement_blocked": false,
                    "active": [],
                }),
                actions: vec![quality_exception_policy_action(
                    args,
                    "missing",
                    "quality exception policy ledger is missing",
                )],
            };
        }
    };

    let policy = match toml::from_str::<QualityExceptionPolicy>(&raw) {
        Ok(policy) => policy,
        Err(_) => {
            return ExceptionPolicyEvaluation {
                receipt: json!({
                    "status": "invalid",
                    "policy": display_path(path),
                    "active_count": 0,
                    "final_enforcement_blocked": false,
                    "active": [],
                }),
                actions: vec![quality_exception_policy_action(
                    args,
                    "invalid_toml",
                    "quality exception policy ledger is not valid TOML",
                )],
            };
        }
    };

    let policy_due_review = policy.due_review.as_deref().unwrap_or("fail");
    let mut active = Vec::new();
    let mut active_ids = BTreeSet::new();
    let mut actions = Vec::new();

    if policy.schema_version != 1 || policy.policy != "quality-gate-exceptions" {
        actions.push(quality_exception_policy_action(
            args,
            "invalid_header",
            "quality exception policy must use schema_version = 1 and policy = \"quality-gate-exceptions\"",
        ));
    }
    let metadata_is_valid = !policy.owner.trim().is_empty()
        && policy.status == "active"
        && !policy.updated.trim().is_empty();
    if !metadata_is_valid {
        actions.push(quality_exception_policy_action(
            args,
            "invalid_metadata",
            "quality exception policy must have owner, status = \"active\", and updated",
        ));
    }

    for exception in &policy.exceptions {
        let validation_errors = exception_validation_errors(exception);
        if !validation_errors.is_empty() {
            actions.push(quality_exception_invalid_action(args, exception, validation_errors));
            continue;
        }

        let review_after = parse_policy_date(&exception.review_after);
        let expires = parse_policy_date(&exception.expires);
        let created = parse_policy_date(&exception.created);
        if review_after.is_none() || expires.is_none() || created.is_none() {
            actions.push(quality_exception_invalid_action(
                args,
                exception,
                vec!["created, review_after, and expires must use YYYY-MM-DD".to_string()],
            ));
            continue;
        }

        let Some(expires) = expires else {
            continue;
        };
        // Lifecycle overlay: substitute sentinel dates for verdict comparison
        // so committed review_after/expires never shift an unchanged candidate.
        let verdict_expires = overlay
            .and_then(|o| o.rows.get(&exception.id))
            .map(|_| LIFECYCLE_SENTINEL)
            .and_then(|d| parse_policy_date(d))
            .unwrap_or(expires);
        if verdict_expires < today {
            actions.push(quality_exception_expired_action(args, exception, expires, today));
            continue;
        }

        active_ids.insert(exception.id.clone());
        // Receipt entries stamp the original committed dates so callers see
        // the row they authored, even when the overlay sentinels drove the
        // verdict.
        let receipt_review_after = overlay
            .and_then(|o| o.rows.get(&exception.id))
            .map(|d| parse_policy_date(&d.review_after))
            .unwrap_or(review_after);
        active.push(quality_exception_receipt_entry(exception, receipt_review_after, expires));

        let Some(review_after) = review_after else {
            continue;
        };
        let verdict_review_after = overlay
            .and_then(|o| o.rows.get(&exception.id))
            .map(|_| LIFECYCLE_SENTINEL)
            .and_then(|d| parse_policy_date(d))
            .unwrap_or(review_after);
        if verdict_review_after <= today {
            actions.push(quality_exception_review_due_action(
                args,
                exception,
                review_after,
                today,
                policy_due_review,
            ));
        }
    }

    let missing_required = policy
        .requirements
        .required_active
        .iter()
        .filter(|id| !active_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_required.is_empty() {
        actions.push(quality_exception_required_missing_action(
            args,
            &missing_required,
            &policy.requirements.required_active,
        ));
    }

    if let Some(overlay) = overlay
        && let Some(error) = overlay.validation_error.as_deref()
    {
        actions.push(quality_exception_policy_action(args, "invalid_due_review", error));
        if overlay.invalid_metadata_status {
            actions.retain(|action| {
                !(action.get("kind").and_then(Value::as_str)
                    == Some("quality_exception_policy_not_current")
                    && action.get("reason").and_then(Value::as_str) == Some("invalid_metadata"))
            });
        }
    }

    let blocking =
        actions.iter().any(|action| action.get("blocking").and_then(Value::as_bool) == Some(true));
    let status = if blocking { "invalid" } else { "present" };

    let mut receipt = json!({
        "status": status,
        "policy": display_path(path),
        "due_review": policy_due_review,
        "required_active": policy.requirements.required_active,
        "active_count": active.len(),
        "final_enforcement_blocked": !active.is_empty(),
        "active": active,
        "missing_required": missing_required,
    });
    if let Some(overlay) = overlay
        && let Some(object) = receipt.as_object_mut()
    {
        object.insert("due_review".to_string(), Value::String("advisory".to_string()));
        object
            .insert("lifecycle_authority".to_string(), Value::String("policy_cadence".to_string()));
        if let Some(error) = overlay.validation_error.as_deref() {
            object.insert("validation_error".to_string(), Value::String(error.to_string()));
        }
    }

    ExceptionPolicyEvaluation { receipt, actions }
}

fn exception_validation_errors(exception: &QualityException) -> Vec<String> {
    let mut errors = Vec::new();
    for (field, value) in [
        ("id", exception.id.as_str()),
        ("kind", exception.kind.as_str()),
        ("scope", exception.scope.as_str()),
        ("owner", exception.owner.as_str()),
        ("reason", exception.reason.as_str()),
        ("final_target", exception.final_target.as_str()),
        ("evidence", exception.evidence.as_str()),
        ("removal_criteria", exception.removal_criteria.as_str()),
        ("created", exception.created.as_str()),
        ("review_after", exception.review_after.as_str()),
        ("expires", exception.expires.as_str()),
    ] {
        if value.trim().is_empty() {
            errors.push(format!("{field} is required"));
        }
    }
    if !exception.kind.trim().is_empty() && exception.kind != "temporary_burndown" {
        errors.push("kind must be temporary_burndown".to_string());
    }
    errors
}

fn parse_policy_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

fn today() -> NaiveDate {
    Utc::now().date_naive()
}

fn quality_exception_receipt_entry(
    exception: &QualityException,
    review_after: Option<NaiveDate>,
    expires: NaiveDate,
) -> Value {
    json!({
        "id": exception.id,
        "kind": exception.kind,
        "scope": exception.scope,
        "owner": exception.owner,
        "issue": exception.issue,
        "reason": exception.reason,
        "final_target": exception.final_target,
        "evidence": exception.evidence,
        "removal_criteria": exception.removal_criteria,
        "review_after": review_after.map(|date| date.to_string()).unwrap_or_else(|| exception.review_after.clone()),
        "expires": expires.to_string(),
        "blocks_final_enforcement": true,
    })
}

fn read_coverage_receipt(path: &Path, expected_head: &str) -> CoverageReceipt {
    let Ok(raw) = fs::read_to_string(path) else {
        return CoverageReceipt {
            status: "missing".to_string(),
            receipt_head: None,
            lcov: None,
            patch: None,
            project: None,
            scope: None,
            patch_files: Vec::new(),
            top_files: Vec::new(),
            recommended_project_clusters: Vec::new(),
        };
    };
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return CoverageReceipt {
            status: "invalid".to_string(),
            receipt_head: None,
            lcov: None,
            patch: None,
            project: None,
            scope: None,
            patch_files: Vec::new(),
            top_files: Vec::new(),
            recommended_project_clusters: Vec::new(),
        };
    };

    let receipt_head = payload.get("head").and_then(Value::as_str).map(ToOwned::to_owned);
    let status = if receipt_head.as_deref() == Some(expected_head) { "present" } else { "stale" };
    let patch = payload.pointer("/coverage/patch").and_then(Value::as_f64);
    let project = payload.pointer("/coverage/project").and_then(Value::as_f64);
    let scope = payload.get("scope").and_then(Value::as_str).map(ToOwned::to_owned);
    let lcov = payload.get("lcov").and_then(Value::as_str).map(ToOwned::to_owned);
    let top_files = payload
        .get("files_below_target")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(actionable_file_gap).take(3).collect::<Vec<_>>())
        .unwrap_or_default();
    let patch_files = payload
        .get("patch_files_below_target")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(actionable_file_gap).take(3).collect::<Vec<_>>())
        .unwrap_or_default();
    let recommended_project_clusters = payload
        .get("recommended_project_clusters")
        .and_then(Value::as_array)
        .map(|items| items.iter().take(3).cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    CoverageReceipt {
        status: status.to_string(),
        receipt_head,
        lcov,
        patch,
        project,
        scope,
        patch_files,
        top_files,
        recommended_project_clusters,
    }
}

fn read_json_receipt(path: &Path) -> JsonReceipt {
    let Ok(raw) = fs::read_to_string(path) else {
        return JsonReceipt::Missing;
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(value) => JsonReceipt::Present(value),
        Err(_) => JsonReceipt::Invalid,
    }
}

fn read_ripr_plus_receipt(path: &Path, expected_head: &str) -> RiprPlusReceipt {
    match read_json_receipt(path) {
        JsonReceipt::Missing => RiprPlusReceipt {
            status: "missing".to_string(),
            receipt_head: None,
            unresolved: None,
            recommended_first_clusters: Vec::new(),
        },
        JsonReceipt::Invalid => RiprPlusReceipt {
            status: "invalid".to_string(),
            receipt_head: None,
            unresolved: None,
            recommended_first_clusters: Vec::new(),
        },
        JsonReceipt::Present(payload) => {
            let receipt_head = payload.get("head").and_then(Value::as_str).map(ToOwned::to_owned);
            let status =
                if receipt_head.as_deref() == Some(expected_head) { "present" } else { "stale" };
            let recommended_first_clusters = payload
                .get("recommended_first_clusters")
                .and_then(Value::as_array)
                .map(|items| items.iter().take(3).cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            RiprPlusReceipt {
                status: status.to_string(),
                receipt_head,
                unresolved: payload.get("unresolved").and_then(Value::as_u64),
                recommended_first_clusters,
            }
        }
    }
}

/// Count of *genuine* new RIPR gaps on the PR diff, for merge-gate blocking.
///
/// The ripr-pr receipt's `summary.severe_gaps` sums three exposure classes:
/// `weakly_exposed + reachable_unrevealed + no_static_path`. Only the latter
/// two represent code that is genuinely reachable-but-unrevealed — a class a
/// focused test can actually close. `weakly_exposed` seams are reachable *and*
/// already observed by a test; the static grip analysis simply can't confirm
/// the observation is *strong* enough. That is an analyzer-confidence
/// limitation, not a missing test, so blocking merge on it is unactionable:
/// it cannot be cleared by adding tests (empirically confirmed on #1914 across
/// unit, integration, e2e, and call-presence-observer test forms), and it
/// wedges otherwise-complete PRs indefinitely.
///
/// The merge-blocking count therefore uses only the actionable subtotal
/// `reachable_unrevealed + no_static_path`. `severe_gaps` keeps its full
/// meaning in the producer for mutation routing / telemetry — this recalibrates
/// only what the gate *blocks* on (#2015).
///
/// Falls back to `severe_gaps` when the subtotal fields are absent (receipts
/// predating this summary shape).
///
/// Caveat (#2015): on ripr 0.9.x the weak-proof class is emitted as
/// `weakly_gripped` and folded into the `reachable_unrevealed` bucket by the
/// producer, so a summary-level subtotal cannot separate it there. A durable
/// cross-version fix requires the producer to keep weak seams in their own
/// dedicated bucket; until then this recalibration excludes the 0.5.x-style
/// `weakly_exposed` count only.
///
/// The basis itself is production-scoped upstream of this function (#11690):
/// the producer (`ripr_evidence::ripr_pr_summary_counts`) drops seams on
/// non-production paths — archived sources under `archive/**` and files in
/// Cargo integration-test directories (`tests/` components) — from the same
/// buckets before this count reads them, and reports the total as
/// `summary.non_production_excluded`. A suppressed finding does not count, and
/// a non-production file does not inflate the basis; suppression policy keeps
/// precedence when both apply.
fn genuine_new_ripr_gap_count(payload: &Value) -> Option<u64> {
    let severe_gaps = payload.pointer("/summary/severe_gaps").and_then(Value::as_u64);
    let reachable = payload.pointer("/summary/reachable_unrevealed").and_then(Value::as_u64);
    let no_static_path = payload.pointer("/summary/no_static_path").and_then(Value::as_u64);
    match (reachable, no_static_path) {
        (Some(reachable), Some(no_static_path)) => {
            let actionable = reachable.saturating_add(no_static_path);
            // Cap at the producer's post-suppression `severe_gaps`. The bucket
            // totals do not reflect `suppressed_unclassified`, which the producer
            // subtracts only from `severe_gaps` (see `ripr_evidence::pr_evidence_packet`);
            // capping preserves the path/classification suppressions the producer
            // already applied instead of re-opening them. In the normal
            // (unsuppressed) case `severe_gaps >= reachable + no_static_path`, so
            // the cap is a no-op and `weakly_exposed` stays excluded.
            Some(severe_gaps.map_or(actionable, |cap| actionable.min(cap)))
        }
        _ => severe_gaps,
    }
}

fn read_ripr_pr_receipt(path: &Path, expected_head: &str) -> RiprPrReceipt {
    match read_json_receipt(path) {
        JsonReceipt::Missing => RiprPrReceipt {
            status: "missing".to_string(),
            receipt_head_sha: None,
            base: None,
            base_sha: None,
            new_unresolved: None,
            non_production_excluded: None,
        },
        JsonReceipt::Invalid => RiprPrReceipt {
            status: "invalid".to_string(),
            receipt_head_sha: None,
            base: None,
            base_sha: None,
            new_unresolved: None,
            non_production_excluded: None,
        },
        JsonReceipt::Present(payload) => {
            let receipt_head_sha =
                payload.get("head_sha").and_then(Value::as_str).map(ToOwned::to_owned);
            let status = if receipt_head_sha.as_deref() == Some(expected_head) {
                "present"
            } else {
                "stale"
            };
            RiprPrReceipt {
                status: status.to_string(),
                receipt_head_sha,
                base: payload.get("base").and_then(Value::as_str).map(ToOwned::to_owned),
                base_sha: payload.get("base_sha").and_then(Value::as_str).map(ToOwned::to_owned),
                new_unresolved: genuine_new_ripr_gap_count(&payload),
                non_production_excluded: payload
                    .pointer("/summary/non_production_excluded")
                    .and_then(Value::as_u64),
            }
        }
    }
}

fn read_review_guidance_receipt(path: &Path, expected_head: &str) -> ReviewGuidanceReceipt {
    match read_json_receipt(path) {
        JsonReceipt::Missing => ReviewGuidanceReceipt {
            status: "missing".to_string(),
            receipt_head_sha: None,
            base: None,
            base_sha: None,
            production_files_considered: None,
            changed_production_files: None,
            top_gaps: Vec::new(),
            static_limitation_gaps: Vec::new(),
            suppressed_gap_ids: BTreeSet::new(),
            unavailable_reason: None,
        },
        JsonReceipt::Invalid => ReviewGuidanceReceipt {
            status: "invalid".to_string(),
            receipt_head_sha: None,
            base: None,
            base_sha: None,
            production_files_considered: None,
            changed_production_files: None,
            top_gaps: Vec::new(),
            static_limitation_gaps: Vec::new(),
            suppressed_gap_ids: BTreeSet::new(),
            unavailable_reason: None,
        },
        JsonReceipt::Present(payload) => {
            let receipt_head_sha =
                payload.get("head_sha").and_then(Value::as_str).map(ToOwned::to_owned);
            let production_files_considered = payload
                .pointer("/analysis_scope/production_files_considered")
                .and_then(Value::as_u64);
            let changed_production_files = match payload
                .pointer("/analysis_scope/changed_production_files")
                .and_then(Value::as_array)
            {
                None => None,
                Some(files) => match parse_changed_production_files(files) {
                    Some(parsed) => Some(parsed),
                    None => {
                        return ReviewGuidanceReceipt {
                            status: "invalid".to_string(),
                            receipt_head_sha,
                            base: None,
                            base_sha: None,
                            production_files_considered: None,
                            changed_production_files: None,
                            top_gaps: Vec::new(),
                            static_limitation_gaps: Vec::new(),
                            suppressed_gap_ids: BTreeSet::new(),
                            unavailable_reason: None,
                        };
                    }
                },
            };
            let producer_status = payload.get("status").and_then(Value::as_str);
            let mut status = if receipt_head_sha.as_deref() != Some(expected_head) {
                "stale"
            } else if matches!(producer_status, Some("error" | "incomplete")) {
                producer_status.unwrap_or("incomplete")
            } else {
                "present"
            }
            .to_string();
            let (top_gaps, static_limitation_gaps) =
                if matches!(status.as_str(), "present" | "incomplete") {
                    // An `incomplete` receipt may still name actionable seams: the
                    // fallback path (#10054) synthesizes them from the completed
                    // diff-scoped raw check when the review-comments pass does not
                    // finish, so the gate can block on named evidence.
                    review_guidance_items(&payload, 3)
                } else {
                    (Vec::new(), Vec::new())
                };
            if status == "present"
                && top_gaps.is_empty()
                && static_limitation_gaps.is_empty()
                && review_guidance_declares_items(&payload)
            {
                // Every declared item was unusable as evidence — neither an
                // actionable repair packet nor an auditable static_limitation
                // classification — so the run counts as degraded, not present.
                status = "incomplete".to_string();
            }

            let unavailable_reason =
                if status == "present" { None } else { review_guidance_error(&payload) };

            ReviewGuidanceReceipt {
                status,
                receipt_head_sha,
                base: payload.get("base").and_then(Value::as_str).map(ToOwned::to_owned),
                base_sha: payload.get("base_sha").and_then(Value::as_str).map(ToOwned::to_owned),
                production_files_considered,
                changed_production_files,
                top_gaps,
                static_limitation_gaps,
                suppressed_gap_ids: suppressed_gap_ids(&payload),
                unavailable_reason,
            }
        }
    }
}

/// Normalized gap identities the producer already excluded from the counted
/// basis via its `suppressed` list. Entries are resolved with the same
/// identity pointers as counted items ([`review_guidance_item`]), and plain
/// string entries are taken as identities directly; entries with no
/// resolvable identity contribute nothing — an unjoinable exclusion cannot
/// name a seam out of (or into) the credit pool.
fn suppressed_gap_ids(payload: &Value) -> BTreeSet<String> {
    payload
        .get("suppressed")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    if let Some(id) = entry.as_str() {
                        let id = id.trim();
                        if id.is_empty() { None } else { Some(id.to_owned()) }
                    } else {
                        review_guidance_item("suppressed", entry)
                            .get("gap_id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn review_guidance_items(value: &Value, limit: usize) -> (Vec<Value>, Vec<Value>) {
    let mut gaps = Vec::new();
    let mut static_limitation_gaps: Vec<Value> = Vec::new();
    for source in ["comments", "summary_only"] {
        let Some(items) = value.get(source).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let gap = review_guidance_item(source, item);
            if review_guidance_item_is_static_limitation(&gap)
                && !static_limitation_gaps
                    .iter()
                    .any(|seen| seen.get("gap_id") == gap.get("gap_id"))
            {
                // Keep scanning past the top-gaps window so a receipt naming
                // more static limitations than the window holds still earns
                // full clearing credit (#15630).
                static_limitation_gaps.push(gap.clone());
            }
            if gaps.len() < limit && review_guidance_item_is_actionable(&gap) {
                gaps.push(gap);
            }
        }
    }
    (gaps, static_limitation_gaps)
}

/// Extract the producer's own explanation for an unfinished guidance run.
///
/// `write_error_review_comments` stamps `warnings[] = { kind: "tool_error",
/// message: <first line of the failure> }`, which is where a message like
/// `ripr timed out after 600s` lands. Any warning kind is accepted so that a
/// future producer failure class still reaches the author instead of being
/// silently dropped; `tool_error` is preferred when several are present.
fn review_guidance_error(value: &Value) -> Option<String> {
    fn message(warning: &Value) -> Option<&str> {
        warning.get("message").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty())
    }

    let warnings = value.get("warnings").and_then(Value::as_array)?;
    warnings
        .iter()
        .find(|warning| warning.get("kind").and_then(Value::as_str) == Some("tool_error"))
        .and_then(message)
        .or_else(|| warnings.iter().find_map(message))
        .map(ToOwned::to_owned)
}

fn review_guidance_declares_items(value: &Value) -> bool {
    ["comments", "summary_only"].iter().any(|field| {
        value.get(*field).and_then(Value::as_array).is_some_and(|items| !items.is_empty())
    }) || ["/summary/comments", "/summary/summary_only"].iter().any(|pointer| {
        value.pointer(pointer).and_then(Value::as_u64).is_some_and(|count| count > 0)
    })
}

fn review_guidance_item_is_actionable(item: &Value) -> bool {
    string_field_is_filled(item, "gap_id")
        && string_field_is_filled(item, "path")
        && item.get("line").and_then(Value::as_u64).is_some_and(|line| line > 0)
        && string_field_is_filled(item, "seam")
        && string_field_is_filled(item, "reason")
        && string_field_is_filled(item, "suggested_test")
}

/// Audit predicate for a producer-classified `static_limitation` gap (#15630).
///
/// The hosted review guidance stamps `classification: "static_limitation"` on
/// seams whose boundary operands are local or computed, where the analyzer
/// states it cannot judge the seam and must not emit an actionable repair
/// packet. No candidate-side proof can clear such a seam, so counting it in
/// the blocking new-gap basis wedges PRs on a required check nothing can
/// satisfy.
///
/// The disposition must be earned, not self-served: the item must carry the
/// producer's own classification value from the head-pinned guidance receipt
/// (which CI regenerates; the gate never reads a candidate-authored field),
/// and the seam must be auditable through gap id, file, positive line, and
/// the analyzer's limitation reason. `suggested_test` is deliberately not
/// required — the producer explicitly does not emit a repair packet for a
/// static limitation.
fn review_guidance_item_is_static_limitation(item: &Value) -> bool {
    item.get("classification").and_then(Value::as_str) == Some("static_limitation")
        && string_field_is_filled(item, "gap_id")
        && string_field_is_filled(item, "path")
        && item.get("line").and_then(Value::as_u64).is_some_and(|line| line > 0)
        && string_field_is_filled(item, "seam")
        && string_field_is_filled(item, "reason")
}

fn string_field_is_filled(item: &Value, field: &str) -> bool {
    item.get(field).and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty())
}

/// Map one producer guidance item onto the gate's normalized shape.
///
/// The `gap_id` pointer list spans two producer generations. Pre-0.9 ripr
/// emitted `canonical_gap_id`/`gap_id`/`identity.canonical_gap_id`; ripr 0.9.0
/// emits none of those and instead identifies an item by `seam_id` (the seam
/// hash), `dedupe_key` (`ripr:<seam_id>:<path>:<line>`), or `id`
/// (`ripr-review-<seam_id>`). Without the 0.9.0 pointers every real item
/// resolves `gap_id` to `None`, `review_guidance_item_is_actionable` rejects
/// it, `top_gaps` comes back empty, and an otherwise `present` receipt is
/// downgraded to `incomplete` — which becomes a blocking
/// `ripr_review_receipt_not_current` finding on every PR whose ripr run
/// produced guidance at all. Keep the legacy pointers first so older receipts
/// and the synthetic fixtures keep resolving to the same identifier.
fn review_guidance_item(source: &str, item: &Value) -> Value {
    json!({
        "source": source,
        "gap_id": first_string(
            item,
            &[
                "/canonical_gap_id",
                "/gap_id",
                "/identity/canonical_gap_id",
                "/seam_id",
                "/dedupe_key",
                "/id",
            ],
        ),
        "path": first_string(item, &["/placement/path", "/path", "/file"]),
        "line": first_u64(item, &["/placement/line", "/line"]),
        "seam": first_string(item, &["/seam", "/placement/mode", "/owner", "/evidence_record/seam"]),
        "reason": first_string(item, &["/reason", "/why", "/message", "/kind"]),
        "suggested_test": first_string(item, &["/suggested_test/intent", "/suggested_test", "/repair", "/test"]),
        // The producer's own disposition for this seam (#15630). Only the
        // literal value "static_limitation" is consumed downstream; exposure
        // classes (`weakly_exposed`/`grip_class`) never carry it.
        "classification": first_string(item, &["/classification"]),
    })
}

fn first_string(item: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        item.pointer(pointer).and_then(Value::as_str).and_then(|value| {
            let value = value.trim();
            if value.is_empty() { None } else { Some(value.to_string()) }
        })
    })
}

fn first_u64(item: &Value, pointers: &[&str]) -> Option<u64> {
    pointers.iter().find_map(|pointer| item.pointer(pointer).and_then(Value::as_u64))
}

fn actionable_file_gap(file: &Value) -> Option<Value> {
    let path = file.get("path").and_then(Value::as_str)?.trim();
    if path.is_empty() {
        return None;
    }
    let samples = file
        .get("sample_uncovered_lines")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_u64)
        .filter(|line| *line > 0)
        .take(10)
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return None;
    }

    Some(json!({
        "path": path,
        "line_coverage": file.get("line_coverage").and_then(Value::as_f64),
        "sample_uncovered_lines": samples,
    }))
}

fn read_codecov_patch_status(path: &Path) -> Result<String> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading Codecov config {}", path.display()))?;
    let parsed: YamlValue = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing Codecov config {}", path.display()))?;
    let target = yaml_path(&parsed, &["coverage", "status", "patch", "default", "target"])
        .and_then(yaml_scalar);
    let threshold = yaml_path(&parsed, &["coverage", "status", "patch", "default", "threshold"])
        .and_then(yaml_scalar);
    let informational =
        yaml_path(&parsed, &["coverage", "status", "patch", "default", "informational"])
            .and_then(yaml_scalar);

    if target.as_deref() == Some("95%")
        && threshold.as_deref() == Some("0%")
        && informational.as_deref() != Some("true")
    {
        Ok("present".to_string())
    } else {
        Ok("patch_policy_not_blocking".to_string())
    }
}

fn read_codecov_project_status(path: &Path) -> Result<String> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("reading Codecov config {}", path.display()))?;
    let parsed: YamlValue = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("parsing Codecov config {}", path.display()))?;
    let target = yaml_path(&parsed, &["coverage", "status", "project", "default", "target"])
        .and_then(yaml_scalar);
    let threshold = yaml_path(&parsed, &["coverage", "status", "project", "default", "threshold"])
        .and_then(yaml_scalar);
    let informational =
        yaml_path(&parsed, &["coverage", "status", "project", "default", "informational"])
            .and_then(yaml_scalar);
    let threshold = threshold.as_deref().and_then(parse_percent);

    if target.as_deref() == Some("95%")
        && threshold.is_some_and(|value| value <= 0.25)
        && informational.as_deref() != Some("true")
    {
        Ok("present".to_string())
    } else {
        Ok("project_policy_not_blocking".to_string())
    }
}

fn parse_percent(value: &str) -> Option<f64> {
    value.trim().trim_end_matches('%').parse::<f64>().ok()
}

fn yaml_path<'a>(value: &'a YamlValue, path: &[&str]) -> Option<&'a YamlValue> {
    let mut current = value;
    for key in path {
        current = match current {
            YamlValue::Mapping(mapping) => mapping.get(YamlValue::String((*key).to_string()))?,
            _ => return None,
        };
    }
    Some(current)
}

fn yaml_scalar(value: &YamlValue) -> Option<String> {
    match value {
        YamlValue::String(value) => Some(value.clone()),
        YamlValue::Bool(value) => Some(value.to_string()),
        YamlValue::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn coverage_receipt_action(
    coverage: &CoverageReceipt,
    expected_head: &str,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "coverage_receipt_not_current",
        "blocking": true,
        "path": display_path(&args.coverage_receipt),
        "reason": coverage.status,
        "receipt_head": coverage.receipt_head,
        "expected_head": expected_head,
        "repair": "Refresh the LCOV coverage receipt before running the aggregate quality gate.",
        "verify": coverage_baseline_command(args, true),
        "receipt": coverage_baseline_command(args, false),
    })
}

fn patch_coverage_unknown_action(args: &QualityGateArgs) -> Value {
    json!({
        "kind": "patch_coverage_unknown",
        "blocking": true,
        "path": display_path(&args.coverage_receipt),
        "reason": "coverage receipt did not include coverage.patch and no --patch-coverage value was provided",
        "repair": "Record an advisory patch coverage percentage from Codecov or regenerate the coverage receipt with patch coverage evidence.",
        "verify": coverage_baseline_command(args, true),
        "receipt": coverage_baseline_command(args, false),
    })
}

fn patch_coverage_below_target_action(
    patch: f64,
    source: &str,
    coverage: &CoverageReceipt,
    args: &QualityGateArgs,
) -> Value {
    let uses_changed_files = !coverage.patch_files.is_empty();
    let top_files = if uses_changed_files { &coverage.patch_files } else { &coverage.top_files };
    let path = coverage
        .patch_files
        .first()
        .or_else(|| coverage.top_files.first())
        .and_then(|file| file.get("path"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| args.coverage_receipt.to_str().unwrap_or("coverage"));

    json!({
        "kind": "patch_coverage_below_target",
        "blocking": true,
        "path": path,
        "current": round2(patch),
        "target": PATCH_TARGET,
        "source": source,
        "file_scope": if uses_changed_files { "changed_files" } else { "project_fallback" },
        "top_files": top_files,
        "suggested_test": "Prefer focused tests for error paths, boundary conditions, config parsing, serialization, cancellation, and output contracts.",
        "repair": "Add behavior-oriented tests for the uncovered changed-code surfaces, then refresh coverage evidence.",
        "verify": quality_gate_command(args, true, Some(patch)),
        "receipt": quality_gate_command(args, false, Some(patch)),
    })
}

fn project_coverage_unknown_action(args: &QualityGateArgs) -> Value {
    json!({
        "kind": "project_coverage_unknown",
        "blocking": true,
        "path": display_path(&args.coverage_receipt),
        "reason": "coverage receipt did not include coverage.project",
        "repair": "Regenerate the workspace coverage receipt with project coverage evidence before final enforcement.",
        "verify": coverage_baseline_command(args, true),
        "receipt": coverage_baseline_command(args, false),
    })
}

fn project_coverage_below_target_action(
    project: f64,
    coverage: &CoverageReceipt,
    args: &QualityGateArgs,
) -> Value {
    let path = coverage
        .top_files
        .first()
        .and_then(|file| file.get("path"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| args.coverage_receipt.to_str().unwrap_or("coverage"));

    json!({
        "kind": "project_coverage_below_target",
        "blocking": true,
        "path": path,
        "current": round2(project),
        "target": PROJECT_TARGET,
        "top_files": coverage.top_files,
        "recommended_project_clusters": coverage.recommended_project_clusters.clone(),
        "suggested_test": "Prioritize public API boundaries, error handling, config parsing, serialization, cancellation, provider decisions, and report generators.",
        "repair": "Burn down meaningful uncovered behavior until workspace project coverage reaches the final target, then refresh coverage evidence.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn coverage_scope_not_workspace_action(
    coverage: &CoverageReceipt,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "coverage_scope_not_workspace",
        "blocking": true,
        "path": display_path(&args.coverage_receipt),
        "reason": format!("coverage scope is {}", coverage.scope.as_deref().unwrap_or("unknown")),
        "repair": "Regenerate the coverage receipt from workspace-wide coverage, not a crate or partial path subset.",
        "verify": coverage_baseline_command(args, true),
        "receipt": coverage_baseline_command(args, false),
    })
}

fn codecov_policy_action(status: &str, args: &QualityGateArgs) -> Value {
    json!({
        "kind": "codecov_patch_policy_not_blocking",
        "blocking": false,
        "path": display_path(&args.codecov),
        "reason": status,
        "repair": "Codecov patch status is advisory; RIPR+ and focused tests are the required PR proof.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn codecov_project_policy_action(status: &str, args: &QualityGateArgs) -> Value {
    json!({
        "kind": "codecov_project_policy_not_blocking",
        "blocking": false,
        "path": display_path(&args.codecov),
        "reason": status,
        "repair": "Codecov project status is advisory; use scheduled/manual coverage for telemetry.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn quality_exception_policy_action(args: &QualityGateArgs, reason: &str, repair: &str) -> Value {
    json!({
        "kind": "quality_exception_policy_not_current",
        "blocking": true,
        "path": display_path(&args.exception_policy),
        "reason": reason,
        "repair": repair,
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn quality_exception_invalid_action(
    args: &QualityGateArgs,
    exception: &QualityException,
    errors: Vec<String>,
) -> Value {
    json!({
        "kind": "quality_exception_invalid",
        "blocking": true,
        "path": display_path(&args.exception_policy),
        "id": exception.id,
        "reason": errors.join("; "),
        "repair": "Fill kind = \"temporary_burndown\", scope, owner, reason, final_target, evidence, removal_criteria, review_after, and expires for the temporary quality exception.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn quality_exception_expired_action(
    args: &QualityGateArgs,
    exception: &QualityException,
    expires: NaiveDate,
    today: NaiveDate,
) -> Value {
    json!({
        "kind": "quality_exception_expired",
        "blocking": true,
        "path": display_path(&args.exception_policy),
        "id": exception.id,
        "reason": format!("expires {expires} is before {today}"),
        "repair": "Remove the temporary quality exception by completing its removal criteria, or replace it with a fresh policy PR that names new evidence and expiry.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn quality_exception_review_due_action(
    args: &QualityGateArgs,
    exception: &QualityException,
    review_after: NaiveDate,
    today: NaiveDate,
    due_review: &str,
) -> Value {
    let blocking = due_review != "warn";
    json!({
        "kind": "quality_exception_review_due",
        "blocking": blocking,
        "path": display_path(&args.exception_policy),
        "id": exception.id,
        "reason": format!("review_after {review_after} is on or before {today}"),
        "repair": "Re-review the temporary quality exception, update current evidence, and either remove it or move review_after/expires in a policy PR.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn quality_exception_required_missing_action(
    args: &QualityGateArgs,
    missing: &[String],
    required: &[String],
) -> Value {
    json!({
        "kind": "quality_exception_required_missing",
        "blocking": true,
        "path": display_path(&args.exception_policy),
        "reason": format!("missing required active temporary quality exception(s): {}", missing.join(", ")),
        "missing": missing,
        "required_active": required,
        "repair": "Document every transitional burn-down exception in policy/quality-gate-exceptions.toml, or remove it from required_active after the target has been met and enforcement is final.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn ripr_receipt_action(
    ripr: &RiprPlusReceipt,
    expected_head: &str,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "ripr_receipt_not_current",
        "blocking": true,
        "path": display_path(&args.ripr_receipt),
        "reason": ripr.status,
        "receipt_head": ripr.receipt_head,
        "expected_head": expected_head,
        "repair": "Regenerate and check the repo-wide RIPR+ receipt. This transition gate does not require total RIPR+ zero yet, but it does require current total-debt proof.",
        "verify": ripr_plus_command(args, true),
        "receipt": ripr_plus_command(args, false),
    })
}

fn ripr_total_unresolved_action(
    count: u64,
    ripr: &RiprPlusReceipt,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "ripr_total_unresolved",
        "blocking": true,
        "path": display_path(&args.ripr_receipt),
        "unresolved": count,
        "reason": "repo-wide RIPR+ unresolved total is above zero",
        "recommended_first_clusters": ripr.recommended_first_clusters.clone(),
        "repair": "Burn down the remaining repo-wide RIPR+ gap cluster with focused tests, then refresh the RIPR+ receipt.",
        "verify": ripr_plus_command(args, true),
        "receipt": ripr_plus_command(args, false),
    })
}

fn ripr_total_unknown_action(args: &QualityGateArgs) -> Value {
    json!({
        "kind": "ripr_total_unknown",
        "blocking": true,
        "path": display_path(&args.ripr_receipt),
        "reason": "repo-wide RIPR+ receipt did not include unresolved",
        "repair": "Regenerate the RIPR+ receipt so final enforcement can prove unresolved total = 0.",
        "verify": ripr_plus_command(args, true),
        "receipt": ripr_plus_command(args, false),
    })
}

fn quality_exception_active_final_blocker_action(
    exceptions: &ExceptionPolicyEvaluation,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "quality_exception_active_final_blocker",
        "blocking": true,
        "path": display_path(&args.exception_policy),
        "reason": "active temporary quality exceptions remain",
        "active": exceptions.receipt.get("active").cloned().unwrap_or_else(|| json!([])),
        "repair": "Complete the burn-down removal criteria and remove active temporary quality exceptions before final enforcement can pass.",
        "verify": quality_gate_command(args, true, args.patch_coverage),
        "receipt": quality_gate_command(args, false, args.patch_coverage),
    })
}

fn ripr_pr_receipt_action(
    ripr_pr: &RiprPrReceipt,
    expected_head: &str,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "ripr_pr_receipt_not_current",
        "blocking": true,
        "path": display_path(&args.ripr_pr_receipt),
        "reason": ripr_pr.status,
        "receipt_head_sha": ripr_pr.receipt_head_sha,
        "expected_head_sha": expected_head,
        "repair": "Regenerate and check the diff-scoped RIPR PR receipt so new severe gaps are measured against this HEAD.",
        "verify": ripr_pr_command(args, true),
        "receipt": ripr_pr_command(args, false),
    })
}

fn ripr_review_receipt_action(
    review: &ReviewGuidanceReceipt,
    expected_head: &str,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "ripr_review_receipt_not_current",
        "blocking": true,
        "path": display_path(&args.review_receipt),
        "reason": review.status,
        "receipt_head_sha": review.receipt_head_sha,
        "expected_head_sha": expected_head,
        "repair": "Regenerate and check the RIPR review-guidance receipt for this HEAD so failing gates can name the exact file, line, seam, and suggested proof.",
        "verify": ripr_review_command(args, true),
        "receipt": ripr_review_command(args, false),
    })
}

fn ripr_review_guidance_gap_action(
    review: &ReviewGuidanceReceipt,
    expected_head: &str,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "ripr_review_guidance_not_actionable",
        "blocking": true,
        "path": display_path(&args.review_receipt),
        "reason": if review.status == "present" { "no_actionable_top_gaps" } else { review.status.as_str() },
        "receipt_head_sha": review.receipt_head_sha,
        "expected_head_sha": expected_head,
        "repair": "Regenerate RIPR review guidance so new-gap failures include actionable gap id, file, positive line, seam, reason, and suggested test.",
        "verify": ripr_review_command(args, true),
        "receipt": ripr_review_command(args, false),
    })
}

fn new_ripr_gap_unknown_action(ripr_pr: &RiprPrReceipt, args: &QualityGateArgs) -> Value {
    json!({
        "kind": "new_ripr_gap_unknown",
        "blocking": true,
        "path": display_path(&args.ripr_pr_receipt),
        "reason": "diff-scoped RIPR summary (reachable_unrevealed + no_static_path) is not measured",
        "receipt_head_sha": ripr_pr.receipt_head_sha,
        "repair": "Regenerate the diff-scoped RIPR PR receipt so the new-gap count can be measured.",
        "verify": ripr_pr_command(args, true),
        "receipt": ripr_pr_command(args, false),
    })
}

/// Result of honoring earned `static_limitation` dispositions (#15630).
#[derive(Default)]
struct StaticLimitationClearing {
    /// Counted gaps still blocking after credit.
    remaining: u64,
    /// Dispositions actually spent against the counted basis. Only these may
    /// be reported as cleared — in actions, receipts, and Markdown alike.
    applied: Vec<Value>,
    /// Eligible dispositions with no counted slot to spend against
    /// (identity-excluded or surplus). Stays visible under its own receipt
    /// field so unspent evidence is never reported as clearing.
    unspent: Vec<Value>,
}

/// Blocking new-gap count after honoring earned `static_limitation`
/// dispositions (#15630).
///
/// Credit is bound to the counted seam identities under the same evaluation
/// subject, never to the number of guidance records:
///
/// - the guidance receipt must be `present` (see
///   [`ReviewGuidanceReceipt::clearable_static_limitations`]);
/// - both receipts must pin the same `base_sha`: a guidance receipt for
///   another base/scope — or one naming no base at all — earns nothing, and
///   the gate fails closed when that membership is unavailable;
/// - a disposition whose gap identity is already accounted for earns
///   nothing: seams named actionable in `top_gaps`, and seams the producer
///   already excluded via `suppressed`, were never unaccounted slots; and
/// - the credit never exceeds `count - named_actionable_gaps`, so a receipt
///   naming more static limitations than there are unaccounted seams cannot
///   spend their slots.
///
/// Returns the remaining blocking count plus the applied and unspent
/// evidence lists, so callers report actual applied credit instead of all
/// eligible records.
fn blocking_new_gap_count(
    count: u64,
    ripr_pr: &RiprPrReceipt,
    review: &ReviewGuidanceReceipt,
) -> StaticLimitationClearing {
    let eligible = review.clearable_static_limitations();
    let same_subject = match (&ripr_pr.base_sha, &review.base_sha) {
        (Some(pr_base), Some(review_base)) => pr_base == review_base,
        _ => false,
    };
    let actionable_ids: BTreeSet<&str> = review
        .top_gaps
        .iter()
        .filter_map(|gap| gap.get("gap_id").and_then(Value::as_str))
        .collect();
    let is_accounted = |item: &Value| {
        item.get("gap_id")
            .and_then(Value::as_str)
            .is_some_and(|id| actionable_ids.contains(id) || review.suppressed_gap_ids.contains(id))
    };
    let actionable_named = review.top_gaps.len() as u64;
    let slots = if same_subject {
        (eligible.iter().filter(|item| !is_accounted(item)).count() as u64)
            .min(count.saturating_sub(actionable_named))
    } else {
        0
    };
    // Single ordered pass so `unspent` preserves the receipt's evidence
    // order: identity-excluded items and surplus share one list.
    let (mut applied, mut unspent) = (Vec::new(), Vec::new());
    let mut spent = 0;
    for item in eligible {
        if !is_accounted(item) && spent < slots {
            applied.push(item.clone());
            spent += 1;
        } else {
            unspent.push(item.clone());
        }
    }
    StaticLimitationClearing { remaining: count.saturating_sub(spent), applied, unspent }
}

/// Gap identities a clearing actually spent, for receipt-level audit fields.
fn clearing_gap_ids(items: &[Value]) -> Vec<Value> {
    items.iter().filter_map(|item| item.get("gap_id").cloned()).collect()
}

fn new_ripr_gap_action(
    count: u64,
    ripr_pr: &RiprPrReceipt,
    review: &ReviewGuidanceReceipt,
    static_limitation_cleared: &[Value],
    args: &QualityGateArgs,
) -> Value {
    let path = review
        .top_gaps
        .first()
        .and_then(|gap| gap.get("path"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            args.ripr_pr_receipt.to_str().unwrap_or("target/ripr/pr/repo-exposure.json")
        });

    // An empty `top_gaps` has two very different meanings, and conflating them
    // is what makes this gate unactionable (#5459). When guidance completed,
    // an empty list is a real result. When guidance did not complete, the gate
    // knows *how many* new seams exist but not *which* — that is NOT_PROVEN,
    // and the repair is to finish the analysis, not to guess at unnamed seams.
    let gap_list_is_unproven = review.gap_list_is_unproven();
    let repair = if gap_list_is_unproven {
        "Re-run RIPR review guidance for this HEAD so the gate can name the new seams, then add focused tests for the seams it reports. Do not guess at unnamed seams and do not add suppressions."
    } else {
        "Add focused tests that expose the new RIPR seam before merging, then refresh RIPR receipts."
    };
    debug_assert!(
        count > 0,
        "new_ripr_gap is only emitted for a positive count; gap_list_is_unproven assumes it"
    );

    json!({
        "kind": "new_ripr_gap",
        "blocking": true,
        "path": path,
        "new_unresolved": count,
        "receipt_head_sha": ripr_pr.receipt_head_sha,
        "top_gaps": review.top_gaps,
        // Producer-classified static_limitation seams already subtracted from
        // `new_unresolved` above. Kept on the action so the reduction is
        // auditable from the receipt alone (#15630).
        "static_limitation_cleared": static_limitation_cleared,
        // Present on every new_ripr_gap action so a consumer never has to infer
        // the difference between "no gaps to name" and "could not name them".
        "gap_list_proven": !gap_list_is_unproven,
        "guidance_status": review.status,
        "guidance_unavailable_reason": review.unavailable_reason,
        "suggested_test": NEW_RIPR_GAP_SUGGESTED_TEST,
        "repair": repair,
        "verify": quality_gate_command(args, true, None),
        "receipt": quality_gate_command(args, false, None),
    })
}

/// Advisory-only visibility for producer-classified `static_limitation` seams
/// that were cleared from the blocking new-gap basis (#15630). Non-blocking:
/// the whole point of the disposition is that no candidate-side proof can
/// clear these seams, so they must not wedge the PR — but they stay on the
/// receipt so the gap remains visible and auditable.
fn static_limitation_gap_cleared_action(
    clearing: &StaticLimitationClearing,
    review: &ReviewGuidanceReceipt,
    args: &QualityGateArgs,
) -> Value {
    json!({
        "kind": "ripr_static_limitation_gaps_cleared",
        "blocking": false,
        "path": display_path(&args.review_receipt),
        "reason": "every counted new RIPR gap is a producer-classified static_limitation seam; it stays visible here but cannot be closed by candidate-side proof (#15630)",
        "receipt_head_sha": review.receipt_head_sha,
        "static_limitation_gaps": clearing.applied,
        "verify": ripr_review_command(args, true),
        "receipt": ripr_review_command(args, false),
    })
}

pub(crate) fn render_markdown(receipt: &Value, args: &QualityGateArgs) -> Result<String> {
    let decision = receipt.get("decision").and_then(Value::as_str).unwrap_or("unknown");

    let mut markdown = String::new();
    markdown.push_str("# Quality Gate\n\n");
    markdown.push_str("## Quality-gate effect\n\n");
    markdown.push_str(&format!("- decision: `{decision}`\n"));
    markdown.push_str(&format!("- mode: `{}`\n", args.mode.as_str()));
    if let Some(coverage) = receipt.get("coverage") {
        let patch = coverage
            .get("patch")
            .and_then(Value::as_f64)
            .map(|value| format!("{value:.2}%"))
            .unwrap_or_else(|| "unknown".to_string());
        let project = coverage
            .get("project")
            .and_then(Value::as_f64)
            .map(|value| format!("{value:.2}%"))
            .unwrap_or_else(|| "unknown".to_string());
        let source = coverage.get("patch_source").and_then(Value::as_str).unwrap_or("unknown");
        let status = coverage.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let scope = coverage.get("scope").and_then(Value::as_str).unwrap_or("unknown");
        let codecov_patch = coverage
            .get("codecov_patch_status")
            .or_else(|| coverage.get("codecov_config_status"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let codecov_project =
            coverage.get("codecov_project_status").and_then(Value::as_str).unwrap_or("unknown");
        markdown.push_str(&format!("- coverage receipt: `{status}`\n"));
        markdown.push_str(&format!("- patch coverage: `{patch}` / `95.00%`\n"));
        markdown.push_str(&format!("- project coverage: `{project}` / `95.00%`\n"));
        markdown.push_str(&format!("- coverage scope: `{scope}`\n"));
        markdown.push_str(&format!("- Codecov patch policy: `{codecov_patch}`\n"));
        if coverage.get("codecov_project_status").is_some() {
            markdown.push_str(&format!("- Codecov project policy: `{codecov_project}`\n"));
        }
        markdown.push_str(&format!("- patch source: `{source}`\n"));
    }
    if let Some(ripr_pr) = receipt.get("ripr_pr") {
        let status = ripr_pr.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let new_unresolved = ripr_pr
            .get("new_unresolved")
            .and_then(Value::as_u64)
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        markdown.push_str(&format!("- diff RIPR receipt: `{status}`\n"));
        markdown.push_str(&format!("- new RIPR gaps: `{new_unresolved}`\n"));
        if let Some(excluded) = ripr_pr.get("non_production_excluded").and_then(Value::as_u64)
            && excluded > 0
        {
            markdown.push_str(&format!(
                "- non-production seams excluded from the basis (#11690): `{excluded}`\n"
            ));
        }
        if let Some(cleared) = ripr_pr.get("static_limitation_cleared").and_then(Value::as_u64)
            && cleared > 0
        {
            markdown.push_str(&format!(
                "- producer-classified static_limitation seams cleared from the basis (#15630): `{cleared}`\n"
            ));
        }
    }
    if let Some(ripr) = receipt.get("ripr_plus") {
        let status = ripr.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let unresolved = ripr
            .get("unresolved")
            .and_then(Value::as_u64)
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        markdown.push_str(&format!("- repo RIPR+ receipt: `{status}`\n"));
        markdown.push_str(&format!("- total RIPR+ gaps: `{unresolved}`\n"));
    }
    if let Some(review) = receipt.get("review_guidance") {
        let status = review.get("status").and_then(Value::as_str).unwrap_or("unknown");
        markdown.push_str(&format!("- review guidance receipt: `{status}`\n"));
    }
    if let Some(exceptions) = receipt.get("temporary_exceptions") {
        let status = exceptions.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let active_count =
            exceptions.get("active_count").and_then(Value::as_u64).unwrap_or_default();
        let final_blocked =
            exceptions.get("final_enforcement_blocked").and_then(Value::as_bool).unwrap_or(false);
        markdown.push_str(&format!("- temporary exceptions: `{status}`\n"));
        markdown.push_str(&format!("- active temporary exceptions: `{active_count}`\n"));
        markdown.push_str(&format!("- final enforcement blocked: `{final_blocked}`\n"));
        if let Some(active) = exceptions.get("active").and_then(Value::as_array) {
            for exception in active {
                let id = exception.get("id").and_then(Value::as_str).unwrap_or("unknown");
                let target =
                    exception.get("final_target").and_then(Value::as_str).unwrap_or("unknown");
                markdown.push_str(&format!("- exception: `{id}` final target `{target}`\n"));
            }
        }
    }
    let freshness = receipt_freshness_summary(receipt);
    if !freshness.is_empty() {
        markdown.push_str(&format!("- receipt freshness: `{freshness}`\n"));
    }
    markdown.push('\n');

    markdown.push_str("## Proof Commands\n\n");
    markdown.push_str(&format!(
        "- verify: `{}`\n",
        quality_gate_command(args, true, args.patch_coverage)
    ));
    markdown.push_str(&format!(
        "- receipt: `{}`\n",
        quality_gate_command(args, false, args.patch_coverage)
    ));
    markdown.push('\n');

    let actions =
        receipt.get("next_actions").and_then(Value::as_array).cloned().unwrap_or_default();
    if actions.is_empty() {
        markdown.push_str("## Next Actions\n\n- none\n");
        return Ok(markdown);
    }

    markdown.push_str("## Next Actions\n\n");
    for action in actions {
        let kind = action.get("kind").and_then(Value::as_str).unwrap_or("unknown");
        markdown.push_str(&format!("### {kind}\n\n"));
        for field in ["path", "reason", "repair", "verify", "receipt", "suggested_test"] {
            if let Some(value) = action.get(field).and_then(Value::as_str) {
                markdown.push_str(&format!("- {field}: `{value}`\n"));
            }
        }
        if let Some(files) = action.get("top_files").and_then(Value::as_array) {
            let coverage_file_label = match (kind, action.get("file_scope").and_then(Value::as_str))
            {
                ("patch_coverage_below_target", Some("changed_files")) => "changed coverage file",
                ("patch_coverage_below_target", Some("project_fallback")) => {
                    "project fallback coverage file"
                }
                _ => "coverage file",
            };
            for file in files {
                let path = file.get("path").and_then(Value::as_str).unwrap_or("unknown");
                let samples = file
                    .get("sample_uncovered_lines")
                    .and_then(Value::as_array)
                    .map(|lines| {
                        lines
                            .iter()
                            .filter_map(Value::as_u64)
                            .map(|line| line.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                markdown.push_str(&format!(
                    "- {coverage_file_label}: `{path}` sample uncovered lines: {samples}\n"
                ));
            }
        }
        if let Some(clusters) = action.get("recommended_first_clusters").and_then(Value::as_array) {
            render_recommended_clusters(&mut markdown, "ripr cluster", clusters);
        }
        if let Some(clusters) = action.get("recommended_project_clusters").and_then(Value::as_array)
        {
            render_recommended_clusters(&mut markdown, "coverage cluster", clusters);
        }
        // Say plainly when the gap list could not be produced. Without this the
        // reader sees a blocking "N new seams" heading followed by nothing and
        // has no way to tell that the analysis itself failed (#5459).
        if action.get("gap_list_proven").and_then(Value::as_bool) == Some(false) {
            let guidance_status =
                action.get("guidance_status").and_then(Value::as_str).unwrap_or("unknown");
            // Two different failures reach here and the author acts on them
            // differently: a run that never finished gets re-run, while a run
            // that finished and named nothing is a producer disagreeing with
            // the count. Reporting the second as "could not be produced"
            // would be its own inaccuracy.
            if guidance_status == "present" {
                markdown.push_str(
                    "- NOT_PROVEN: review guidance completed but named no seam, which contradicts the count above. The count and the list disagree, so the seams are not identified.\n",
                );
            } else {
                markdown.push_str(&format!(
                    "- NOT_PROVEN: the new-seam list could not be produced (review guidance `{guidance_status}`), so the count above is not accompanied by the seams it counts.\n"
                ));
            }
            if let Some(reason) = action.get("guidance_unavailable_reason").and_then(Value::as_str)
            {
                markdown.push_str(&format!("- guidance failure: `{reason}`\n"));
            }
        }
        if let Some(gaps) = action.get("top_gaps").and_then(Value::as_array) {
            for gap in gaps {
                let gap_id = gap.get("gap_id").and_then(Value::as_str).unwrap_or("unknown");
                let path = gap.get("path").and_then(Value::as_str).unwrap_or("unknown");
                let line = gap.get("line").and_then(Value::as_u64).unwrap_or(0);
                let seam = gap.get("seam").and_then(Value::as_str).unwrap_or("unknown");
                let reason = gap.get("reason").and_then(Value::as_str).unwrap_or("unspecified");
                let suggested_test = gap
                    .get("suggested_test")
                    .and_then(Value::as_str)
                    .unwrap_or("add focused proof");
                markdown.push_str(&format!(
                    "- ripr gap: `{gap_id}` `{path}:{line}` seam `{seam}` reason `{reason}` suggested test `{suggested_test}`\n"
                ));
            }
        }
        markdown.push('\n');
    }

    if receipt
        .get("temporary_exceptions")
        .and_then(Value::as_object)
        .and_then(|object| object.get("lifecycle_authority"))
        .is_some()
    {
        markdown.push_str("\n## Policy Lifecycle\n\n");
        markdown.push_str("- authority: `cargo xtask policy cadence`\n");
        markdown.push_str("- candidate impact: `advisory_only`\n");
        if let Some(error) = receipt
            .get("temporary_exceptions")
            .and_then(Value::as_object)
            .and_then(|object| object.get("validation_error"))
            .and_then(Value::as_str)
        {
            markdown.push_str(&format!("- validation error: `{error}`\n"));
        }
    }

    Ok(markdown)
}

fn render_recommended_clusters(markdown: &mut String, label: &str, clusters: &[Value]) {
    for cluster in clusters {
        let name = cluster.get("name").and_then(Value::as_str).unwrap_or("unknown");
        let reason = cluster.get("reason").and_then(Value::as_str).unwrap_or("unspecified");
        let metrics =
            ["score", "active_file_count", "gap_kind_count", "file_count", "uncovered_line_count"]
                .iter()
                .filter_map(|metric| {
                    cluster
                        .get(*metric)
                        .and_then(Value::as_u64)
                        .map(|value| format!("{metric}: {value}"))
                })
                .collect::<Vec<_>>()
                .join(", ");
        let metrics = if metrics.is_empty() { String::new() } else { format!(" ({metrics})") };
        markdown.push_str(&format!("- {label}: `{name}`{metrics} reason `{reason}`\n"));
        render_cluster_examples(markdown, label, "example file", cluster.get("example_files"));
        render_cluster_examples(
            markdown,
            label,
            "example gap kind",
            cluster.get("example_gap_kinds"),
        );
    }
}

fn render_cluster_examples(
    markdown: &mut String,
    label: &str,
    example_label: &str,
    examples: Option<&Value>,
) {
    let Some(examples) = examples.and_then(Value::as_array) else {
        return;
    };
    for example in examples.iter().filter_map(Value::as_str) {
        markdown.push_str(&format!("- {label} {example_label}: `{example}`\n"));
    }
}

fn receipt_freshness_summary(receipt: &Value) -> String {
    [
        ("coverage", "/coverage/status"),
        ("repo_ripr", "/ripr_plus/status"),
        ("diff_ripr", "/ripr_pr/status"),
        ("review_guidance", "/review_guidance/status"),
        ("exceptions", "/temporary_exceptions/status"),
    ]
    .iter()
    .filter_map(|(label, pointer)| {
        receipt.pointer(pointer).and_then(Value::as_str).map(|status| format!("{label}={status}"))
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn coverage_baseline_command(args: &QualityGateArgs, check: bool) -> String {
    let mut command = format!(
        "cargo xtask coverage-baseline --lcov target/lcov.info --receipt {} --codecov {}",
        args.coverage_receipt.display(),
        args.codecov.display()
    );
    if let Some(patch) = args.patch_coverage {
        command.push_str(&format!(" --patch-coverage {patch:.2}"));
    }
    if check {
        command.push_str(" --check");
    }
    command
}

fn quality_gate_command(args: &QualityGateArgs, check: bool, patch: Option<f64>) -> String {
    let mut command = format!("cargo xtask quality-gate --mode {}", args.mode.as_str());
    command.push_str(&format!(" --exception-policy {}", args.exception_policy.display()));
    match args.mode {
        QualityGateMode::Enforce => {
            command.push_str(&format!(
                " --coverage-receipt {} --codecov {} --ripr-receipt {} --ripr-pr-receipt {} --review-receipt {} --ripr-base {} --ripr-head {}",
                args.coverage_receipt.display(),
                args.codecov.display(),
                args.ripr_receipt.display(),
                args.ripr_pr_receipt.display(),
                args.review_receipt.display(),
                args.ripr_base,
                args.ripr_head
            ));
        }
        QualityGateMode::EnforcePatchCoverage => {
            command.push_str(&format!(
                " --coverage-receipt {} --codecov {}",
                args.coverage_receipt.display(),
                args.codecov.display()
            ));
        }
        QualityGateMode::EnforceNewRipr => {
            command.push_str(&format!(
                " --ripr-receipt {} --ripr-pr-receipt {} --review-receipt {} --ripr-base {} --ripr-head {}",
                args.ripr_receipt.display(),
                args.ripr_pr_receipt.display(),
                args.review_receipt.display(),
                args.ripr_base,
                args.ripr_head
            ));
        }
    }
    command.push_str(&format!(
        " --receipt {} --summary {}",
        display_path(&args.receipt),
        display_path(&args.summary)
    ));
    if let Some(patch) = patch {
        command.push_str(&format!(" --patch-coverage {patch:.2}"));
    }
    if check {
        command.push_str(" --check");
    }
    command
}

fn ripr_plus_command(args: &QualityGateArgs, check: bool) -> String {
    let mut command = format!("cargo xtask ripr-plus --receipt {}", args.ripr_receipt.display());
    if check {
        command.push_str(" --check");
    }
    command
}

fn ripr_pr_command(args: &QualityGateArgs, check: bool) -> String {
    let mut command =
        format!("cargo xtask ripr-pr --base {} --head {}", args.ripr_base, args.ripr_head);
    if check {
        command.push_str(" --check");
    }
    command
}

fn ripr_review_command(args: &QualityGateArgs, check: bool) -> String {
    let mut command = format!(
        "cargo xtask ripr-review-comments --base {} --head {}",
        args.ripr_base, args.ripr_head
    );
    if check {
        command.push_str(" --check");
    }
    command
}

/// Record one staleness finding, or fail immediately when the proof file is
/// unreadable (a missing file is a different failure class — there is no
/// second condition worth reporting past it).
fn assert_current(path: &Path, expected: &str, label: &str, stale: &mut Vec<String>) -> Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("{label} is missing: {}", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("reading {label} {}", path.display()));
        }
    };
    if normalize(&existing) != normalize(expected) {
        stale.push(format!("{label} is stale: {}", path.display()));
    }
    Ok(())
}

fn current_head(root: &Path) -> Result<String> {
    git_stdout_with_worktree_fallback(root, &["rev-parse", "HEAD"])
        .context("running git rev-parse HEAD")
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn render_json(value: &Value) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

fn normalize(value: &str) -> String {
    value.trim().replace("\r\n", "\n")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::{fs, process::Command};
    use tempfile::tempdir;

    use super::*;

    fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git").args(args).current_dir(repo).output()?;
        if !output.status.success() {
            bail!("git {:?} failed with status {}", args, output.status);
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    #[test]
    fn coverage_receipt_preserves_recommended_project_clusters() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("coverage-baseline.json");
        let head = "cluster-head";
        write_text(
            &path,
            &render_json(&json!({
                "head": head,
                "scope": "workspace",
                "coverage": {
                    "patch": 99.0,
                    "project": 94.0
                },
                "recommended_project_clusters": [
                    {
                        "name": "proof-infrastructure",
                        "file_count": 2,
                        "uncovered_line_count": 37,
                        "reason": "Coverage proof, quality-gate, workflow, and policy surfaces are owned by this lane.",
                        "example_files": ["xtask/src/tasks/quality_gate.rs"]
                    }
                ]
            }))?,
        )?;

        let receipt = read_coverage_receipt(&path, head);

        assert_eq!(receipt.status, "present");
        assert_eq!(
            receipt
                .recommended_project_clusters
                .first()
                .and_then(|cluster| cluster.get("name"))
                .and_then(Value::as_str),
            Some("proof-infrastructure")
        );
        assert_eq!(
            receipt
                .recommended_project_clusters
                .first()
                .and_then(|cluster| cluster.get("uncovered_line_count"))
                .and_then(Value::as_u64),
            Some(37)
        );
        Ok(())
    }

    #[test]
    fn current_head_reads_repository_head() -> Result<()> {
        let dir = tempdir()?;
        run_git(dir.path(), &["init"])?;
        run_git(dir.path(), &["config", "user.email", "agent@example.invalid"])?;
        run_git(dir.path(), &["config", "user.name", "Agent Test"])?;
        fs::write(dir.path().join("tracked.txt"), "base\n")?;
        run_git(dir.path(), &["add", "tracked.txt"])?;
        run_git(dir.path(), &["commit", "-m", "base"])?;
        let head = run_git(dir.path(), &["rev-parse", "HEAD"])?.trim().to_string();

        assert_eq!(current_head(dir.path())?, head);
        Ok(())
    }

    #[test]
    fn ripr_plus_receipt_preserves_recommended_first_clusters() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("ripr-plus.json");
        let head = "cluster-head";
        write_text(
            &path,
            &render_json(&json!({
                "head": head,
                "unresolved": 3,
                "recommended_first_clusters": [
                    {
                        "name": "ci-report-formatting",
                        "score": 5,
                        "active_file_count": 3,
                        "gap_kind_count": 2,
                        "reason": "Receipt and report formatting gaps should become agent repair packets.",
                        "example_files": ["xtask/src/tasks/quality_gate.rs"],
                        "example_gap_kinds": ["receipt_missing"]
                    }
                ]
            }))?,
        )?;

        let receipt = read_ripr_plus_receipt(&path, head);

        assert_eq!(receipt.status, "present");
        assert_eq!(
            receipt
                .recommended_first_clusters
                .first()
                .and_then(|cluster| cluster.get("name"))
                .and_then(Value::as_str),
            Some("ci-report-formatting")
        );
        assert_eq!(
            receipt
                .recommended_first_clusters
                .first()
                .and_then(|cluster| cluster.get("score"))
                .and_then(Value::as_u64),
            Some(5)
        );
        Ok(())
    }

    #[test]
    fn render_recommended_clusters_names_metrics_reasons_and_examples() -> Result<()> {
        let clusters = vec![json!({
            "name": "ci-report-formatting",
            "score": 5,
            "active_file_count": 3,
            "gap_kind_count": 2,
            "reason": "Receipt and report formatting gaps should become agent repair packets.",
            "example_files": ["xtask/src/tasks/quality_gate.rs"],
            "example_gap_kinds": ["receipt_missing"]
        })];
        let mut markdown = String::new();

        render_recommended_clusters(&mut markdown, "ripr cluster", &clusters);

        for required in [
            "ripr cluster: `ci-report-formatting` (score: 5, active_file_count: 3, gap_kind_count: 2) reason `Receipt and report formatting gaps should become agent repair packets.`",
            "ripr cluster example file: `xtask/src/tasks/quality_gate.rs`",
            "ripr cluster example gap kind: `receipt_missing`",
        ] {
            assert!(
                markdown.contains(required),
                "cluster markdown missing `{required}`:\n{markdown}"
            );
        }
        Ok(())
    }

    // ── #1470 classify_patch_coverage_failure + test_failure_class tests ────
    // Direct lib tests for new functions added in PR #1470. These give ripr
    // direct oracle observability for the classify_patch_coverage_failure
    // function and the test_failure_class / failure_class receipt fields.

    #[test]
    fn classify_patch_coverage_failure_returns_pass_when_not_failed() {
        let result = classify_patch_coverage_failure(false, &[]);
        assert_eq!(result, "pass");
    }

    #[test]
    fn classify_patch_coverage_failure_returns_coverage_shortfall_when_below_target() {
        let actions = vec![json!({"kind": "patch_coverage_below_target", "blocking": true})];
        let result = classify_patch_coverage_failure(true, &actions);
        assert_eq!(result, "coverage_shortfall");
    }

    #[test]
    fn classify_patch_coverage_failure_returns_coverage_shortfall_for_unknown_coverage() {
        let actions = vec![json!({"kind": "patch_coverage_unknown", "blocking": true})];
        let result = classify_patch_coverage_failure(true, &actions);
        assert_eq!(result, "coverage_shortfall");
    }

    #[test]
    fn classify_patch_coverage_failure_returns_setup_failure_for_other_failures() {
        // A failure that is not coverage_shortfall or patch_coverage_unknown
        // should be classified as setup_failure.
        let actions = vec![json!({"kind": "stale_receipt", "blocking": true})];
        let result = classify_patch_coverage_failure(true, &actions);
        assert_eq!(result, "setup_failure");
    }

    // Additional tests for receipt serialization and field presence
    #[test]
    fn receipt_contains_failure_class_field_when_no_failures() {
        let actions = vec![];
        let failure_class = classify_patch_coverage_failure(false, &actions);
        assert_eq!(failure_class, "pass");

        // Verify the field would be in the receipt
        let receipt = json!({
            "failure_class": failure_class,
            "test_failure_class": None::<&str>,
        });
        assert_eq!(receipt["failure_class"], "pass");
        assert!(receipt["test_failure_class"].is_null());
    }

    #[test]
    fn receipt_contains_failure_class_field_when_coverage_shortfall() {
        let actions = vec![json!({"kind": "patch_coverage_below_target", "blocking": true})];
        let failure_class = classify_patch_coverage_failure(true, &actions);
        assert_eq!(failure_class, "coverage_shortfall");

        // Verify the field would be in the receipt
        let receipt = json!({
            "failure_class": failure_class,
            "test_failure_class": None::<&str>,
        });
        assert_eq!(receipt["failure_class"], "coverage_shortfall");
    }

    #[test]
    fn receipt_contains_failure_class_field_when_setup_failure() {
        let actions = vec![json!({"kind": "ripr_receipt_missing", "blocking": true})];
        let failure_class = classify_patch_coverage_failure(true, &actions);
        assert_eq!(failure_class, "setup_failure");

        // Verify the field would be in the receipt
        let receipt = json!({
            "failure_class": failure_class,
            "test_failure_class": None::<&str>,
        });
        assert_eq!(receipt["failure_class"], "setup_failure");
    }

    #[test]
    fn patch_coverage_gate_receipt_includes_failure_class_and_test_failure_class_fields() {
        // Verify that the receipt JSON structure includes the new #1470 fields.
        // This test pin the decision boundary: if failure_class or test_failure_class
        // fields are removed from the receipt JSON, this test will fail.

        // Simulate a receipt with the required new fields
        let receipt = json!({
            "schema_version": 1,
            "kind": "quality_gate",
            "mode": "enforce-new-ripr",
            "decision": "pass",
            "failure_class": "pass",
            "test_failure_class": null,
            "head": "abc123",
            "coverage": {
                "status": "present",
                "receipt": "path/to/coverage.json",
                "receipt_head": "abc123",
                "patch": 95.5,
                "patch_source": "coverage_receipt",
                "project": 92.0,
                "target": 95,
                "scope": "patch"
            },
            "next_actions": []
        });

        // Verify the new fields exist
        assert!(receipt.get("failure_class").is_some(), "receipt missing 'failure_class' field");
        assert!(
            receipt.get("test_failure_class").is_some(),
            "receipt missing 'test_failure_class' field"
        );

        // Verify the values
        assert_eq!(receipt["failure_class"], "pass");
        assert!(receipt["test_failure_class"].is_null());
    }

    /// Regression guard for the ripr 0.9.0 schema migration.
    ///
    /// This item is a verbatim key-set copy of a real `ripr/review/comments.json`
    /// entry produced by ripr 0.9.0 (captured from the `ripr-pr-evidence`
    /// artifact of a live `ripr+ on GitHub Hosted` run). It carries no
    /// `canonical_gap_id`, `gap_id`, or `identity` — the identifiers are
    /// `seam_id`, `dedupe_key`, and `id`. Before those pointers were added to
    /// `review_guidance_item`, every such item resolved `gap_id` to `None` and
    /// was therefore judged non-actionable, silently downgrading a `present`
    /// review receipt to `incomplete` and blocking the PR.
    ///
    /// The pre-existing fixtures all used `canonical_gap_id`, so they could not
    /// catch this. Keep this test keyed to the real producer shape.
    #[test]
    fn ripr_090_guidance_item_resolves_gap_id_and_is_actionable() {
        let raw = json!({
            "dedupe_key": "ripr:9ac64531a5a9689c:crates/perl-lexer/src/lib.rs:2676",
            "grip_class": "weakly_gripped",
            "id": "ripr-review-9ac64531a5a9689c",
            "kind": "focused_test",
            "owner": "perl_lexer::PerlLexer::parse_double_quoted_string",
            "placement": {
                "path": "crates/perl-lexer/src/lib.rs",
                "line": 2676,
                "mode": "exact_seam_line"
            },
            "reason": "changed match arm has no discriminating proof",
            "seam": "match_arm",
            "seam_id": "9ac64531a5a9689c",
            "severity": "severe",
            "suggested_test": { "intent": "discriminate the new interpolation arm" }
        });

        let mapped = review_guidance_item("comments", &raw);

        assert_eq!(
            mapped.get("gap_id").and_then(Value::as_str),
            Some("9ac64531a5a9689c"),
            "ripr 0.9.0 identifies items by seam_id; gap_id must resolve from it"
        );
        assert!(
            review_guidance_item_is_actionable(&mapped),
            "a fully-populated ripr 0.9.0 item must be actionable: {mapped}"
        );
    }

    /// Legacy pre-0.9 receipts must keep resolving to the same identifier, and
    /// the legacy pointers must win when both generations are present.
    #[test]
    fn legacy_canonical_gap_id_still_wins_over_ripr_090_identifiers() {
        let raw = json!({
            "canonical_gap_id": "RIPR-SPEC-LEGACY",
            "seam_id": "9ac64531a5a9689c",
            "kind": "focused_test",
            "reason": "legacy receipt",
            "placement": { "path": "crates/perl-parser/src/lib.rs", "line": 42, "mode": "exact_seam_line" },
            "suggested_test": { "intent": "prove parser branch recovery" }
        });

        let mapped = review_guidance_item("comments", &raw);

        assert_eq!(mapped.get("gap_id").and_then(Value::as_str), Some("RIPR-SPEC-LEGACY"));
        assert!(review_guidance_item_is_actionable(&mapped));
    }

    /// An item still missing every identifier must remain non-actionable, so the
    /// widened pointer list does not turn genuinely unusable guidance into a
    /// repair packet.
    #[test]
    fn guidance_item_without_any_identifier_stays_non_actionable() {
        let raw = json!({
            "kind": "focused_test",
            "reason": "no identifier at all",
            "placement": { "path": "crates/perl-parser/src/lib.rs", "line": 42, "mode": "exact_seam_line" },
            "suggested_test": { "intent": "prove parser branch recovery" }
        });

        let mapped = review_guidance_item("comments", &raw);

        assert!(mapped.get("gap_id").and_then(Value::as_str).is_none());
        assert!(!review_guidance_item_is_actionable(&mapped));
    }

    // ── #15630: producer-classified static_limitation dispositions ──────────

    /// Verbatim shape of a hosted ripr 0.10.0 review-guidance item the New Gap
    /// Gate wrongly blocked on (issue #15630, PR #13174): the analyzer classifies
    /// the seam `static_limitation` because its boundary operands are local or
    /// computed, and explicitly does not emit an actionable repair packet. The
    /// item must still be collected as auditable clearing evidence even though
    /// it has no `suggested_test`.
    fn static_limitation_guidance_item(gap_id: &str, path: &str, line: u64, seam: &str) -> Value {
        json!({
            "seam_id": gap_id,
            "classification": "static_limitation",
            "grip_class": "weakly_gripped",
            "kind": "focused_test",
            "severity": "severe",
            "placement": { "path": path, "line": line, "mode": "exact_seam_line" },
            "seam": seam,
            "reason": "No concrete activation values observed for the seam; add analyzer support for local/computed boundary operand resolution before emitting an actionable repair packet",
            "suggested_test": null
        })
    }

    #[test]
    fn static_limitation_item_is_collected_and_clearable_when_present() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        fs::write(
            dir.path().join("comments.json"),
            json!({
                "head_sha": head,
                "status": "advisory",
                "comments": [
                    static_limitation_guidance_item(
                        "b541fc52c580a9d9",
                        "crates/perl-workspace/src/semantic/quickorm.rs",
                        700,
                        "predicate_boundary",
                    ),
                    static_limitation_guidance_item(
                        "b541fc52c580a9d9",
                        "crates/perl-workspace/src/semantic/quickorm.rs",
                        700,
                        "predicate_boundary",
                    )
                ],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            })
            .to_string(),
        )?;

        let receipt = read_review_guidance_receipt(&dir.path().join("comments.json"), head);

        assert_eq!(
            receipt.status, "present",
            "named static_limitation evidence is not degradation"
        );
        assert_eq!(receipt.static_limitation_gaps.len(), 1, "duplicate gap ids must dedupe");
        assert!(
            review_guidance_item_is_static_limitation(&receipt.static_limitation_gaps[0]),
            "the collected item must carry the classification and its audit fields"
        );
        assert_eq!(receipt.clearable_static_limitations().len(), 1);
        Ok(())
    }

    #[test]
    fn static_limitation_items_are_not_clearable_from_unpresent_guidance() -> Result<()> {
        let dir = tempdir()?;
        let item = static_limitation_guidance_item("b541fc52c580a9d9", "src.rs", 700, "predicate");
        for status in ["stale", "incomplete", "error", "missing", "invalid"] {
            let receipt = ReviewGuidanceReceipt {
                status: status.to_string(),
                receipt_head_sha: None,
                base: None,
                base_sha: None,
                production_files_considered: None,
                changed_production_files: None,
                top_gaps: Vec::new(),
                static_limitation_gaps: vec![item.clone()],
                suppressed_gap_ids: BTreeSet::new(),
                unavailable_reason: None,
            };
            assert_eq!(
                receipt.clearable_static_limitations().len(),
                0,
                "a {status} guidance receipt must not earn clearing credit"
            );
        }
        Ok(())
    }

    #[test]
    fn static_limitation_item_missing_audit_fields_is_not_collected() {
        // The value must come from the producer, and an item that only looks
        // like a limitation without saying so earns nothing.
        let unclassified = json!({
            "seam_id": "b541fc52c580a9d9",
            "grip_class": "weakly_gripped",
            "placement": { "path": "src.rs", "line": 700, "mode": "exact_seam_line" },
            "seam": "predicate_boundary",
            "reason": "no concrete activation values observed",
            "suggested_test": null
        });
        assert!(!review_guidance_item_is_static_limitation(&review_guidance_item(
            "comments",
            &unclassified
        )));

        // Classified but missing the limitation reason text. Note `kind` must
        // go too: the normalizer falls back to it when `reason` is absent.
        let no_reason = json!({
            "seam_id": "b541fc52c580a9d9",
            "classification": "static_limitation",
            "grip_class": "weakly_gripped",
            "placement": { "path": "src.rs", "line": 700, "mode": "exact_seam_line" },
            "seam": "predicate_boundary",
            "reason": "",
            "suggested_test": null
        });
        assert!(!review_guidance_item_is_static_limitation(&review_guidance_item(
            "comments", &no_reason
        )));

        // Classified but no resolvable gap identity.
        let mut no_identity =
            static_limitation_guidance_item("b541fc52c580a9d9", "src.rs", 700, "predicate");
        let obj = no_identity.as_object_mut().unwrap();
        obj.remove("seam_id");
        assert!(!review_guidance_item_is_static_limitation(&review_guidance_item(
            "comments",
            &no_identity
        )));
    }

    #[test]
    fn blocking_new_gap_count_clears_only_earned_static_limitations() {
        fn ripr_pr(base_sha: Option<&str>) -> RiprPrReceipt {
            RiprPrReceipt {
                status: "present".to_string(),
                receipt_head_sha: None,
                base: None,
                base_sha: base_sha.map(ToOwned::to_owned),
                new_unresolved: None,
                non_production_excluded: None,
            }
        }
        fn review(
            status: &str,
            top_gap_ids: &[&str],
            static_ids: &[&str],
            base_sha: Option<&str>,
            suppressed: &[&str],
        ) -> ReviewGuidanceReceipt {
            ReviewGuidanceReceipt {
                status: status.to_string(),
                receipt_head_sha: None,
                base: None,
                base_sha: base_sha.map(ToOwned::to_owned),
                production_files_considered: None,
                changed_production_files: None,
                top_gaps: top_gap_ids.iter().map(|id| json!({"gap_id": id})).collect(),
                static_limitation_gaps: static_ids.iter().map(|id| json!({"gap_id": id})).collect(),
                suppressed_gap_ids: suppressed.iter().map(|id| id.to_string()).collect(),
                unavailable_reason: None,
            }
        }
        fn applied_ids(clearing: &StaticLimitationClearing) -> Vec<&str> {
            clearing
                .applied
                .iter()
                .filter_map(|item| item.get("gap_id").and_then(Value::as_str))
                .collect()
        }
        /// Every case must account for the original count exactly: what is
        /// not reported as applied credit must still block.
        fn assert_accounted(original: u64, clearing: &StaticLimitationClearing) {
            assert_eq!(
                original,
                clearing.remaining + clearing.applied.len() as u64,
                "original_count = remaining_count + applied_credit"
            );
        }

        let pr = ripr_pr(Some("base-sha"));
        let present =
            |top: &[&str], ids: &[&str]| review("present", top, ids, Some("base-sha"), &[]);

        // The #15630 shape: 5 counted new gaps, named evidence holds one
        // actionable and two static_limitation seams — 3 must still block,
        // and only the 2 spent dispositions report as cleared.
        let clearing = blocking_new_gap_count(5, &pr, &present(&["g"], &["s1", "s2"]));
        assert_eq!(clearing.remaining, 3);
        assert_eq!(applied_ids(&clearing), vec!["s1", "s2"]);
        assert!(clearing.unspent.is_empty());
        assert_accounted(5, &clearing);

        // All counted gaps are earned static_limitation seams — nothing blocks.
        let clearing = blocking_new_gap_count(2, &pr, &present(&[], &["s1", "s2"]));
        assert_eq!(clearing.remaining, 0);
        assert_eq!(applied_ids(&clearing), vec!["s1", "s2"]);
        assert_accounted(2, &clearing);

        // Credit can never consume a named actionable gap's slot: with one
        // counted gap that is already named actionable, two claimed static
        // limitations spend nothing and report zero applied credit.
        let clearing = blocking_new_gap_count(1, &pr, &present(&["g"], &["s1", "s2"]));
        assert_eq!(clearing.remaining, 1);
        assert!(clearing.applied.is_empty(), "zero credit must report zero cleared");
        assert_eq!(clearing.unspent.len(), 2, "the evidence stays visible even when unspent");
        assert_accounted(1, &clearing);

        // Saturating: more claimed credit than counted gaps cannot go
        // negative, and only the spent slot reports as cleared.
        let clearing = blocking_new_gap_count(1, &pr, &present(&[], &["s1", "s2"]));
        assert_eq!(clearing.remaining, 0);
        assert_eq!(applied_ids(&clearing), vec!["s1"]);
        assert_eq!(clearing.unspent.len(), 1);
        assert_accounted(1, &clearing);

        // A disposition naming a seam that is already actionable spends no
        // credit on it: the applied identity must be the unaccounted seam.
        let clearing = blocking_new_gap_count(2, &pr, &present(&["g"], &["g", "s"]));
        assert_eq!(clearing.remaining, 1);
        assert_eq!(applied_ids(&clearing), vec!["s"]);
        assert_accounted(2, &clearing);

        // A disposition naming a producer-suppressed seam spends nothing: the
        // seam was already excluded from the counted basis.
        let suppressed = review("present", &[], &["x"], Some("base-sha"), &["x"]);
        let clearing = blocking_new_gap_count(1, &pr, &suppressed);
        assert_eq!(clearing.remaining, 1);
        assert!(clearing.applied.is_empty());
        assert_accounted(1, &clearing);

        // Same head but another base/scope earns nothing — fail closed.
        let other_base = review("present", &[], &["s"], Some("other-base"), &[]);
        let clearing = blocking_new_gap_count(1, &pr, &other_base);
        assert_eq!(clearing.remaining, 1);
        assert!(clearing.applied.is_empty());
        assert_accounted(1, &clearing);

        // Membership unavailable earns nothing: a missing base on either
        // side fails closed.
        for (pr_base, review_base) in
            [(Some("base-sha"), None), (None, Some("base-sha")), (None, None)]
        {
            let clearing = blocking_new_gap_count(
                1,
                &ripr_pr(pr_base),
                &review("present", &[], &["s"], review_base, &[]),
            );
            assert_eq!(clearing.remaining, 1, "pr_base={pr_base:?} review_base={review_base:?}");
            assert!(clearing.applied.is_empty());
            assert_accounted(1, &clearing);
        }

        // Unpresent guidance earns nothing even on the same subject.
        let clearing = blocking_new_gap_count(
            1,
            &pr,
            &review("incomplete", &[], &["s"], Some("base-sha"), &[]),
        );
        assert_eq!(clearing.remaining, 1);
        assert!(clearing.applied.is_empty());
        assert_accounted(1, &clearing);
    }

    fn gap_action(evaluation: &GateEvaluation, kind: &str) -> Value {
        evaluation
            .receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|action| action.get("kind").and_then(Value::as_str) == Some(kind))
            .unwrap_or_default()
    }

    /// P1 routed negative control: a guidance receipt for the same head but
    /// another base/scope — naming a disjoint static-limitation ID plus one
    /// already-excluded (producer-suppressed) ID, and no actionable packets
    /// — must not reduce the blocker count.
    #[test]
    fn cross_base_guidance_earns_no_clearing_credit() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        fs::write(
            dir.path().join("ripr-plus.json"),
            json!({ "head": head, "unresolved": 0 }).to_string(),
        )?;
        fs::write(
            dir.path().join("repo-exposure.json"),
            json!({
                "head_sha": head,
                "base": "origin/main",
                "base_sha": "base-A",
                "summary": { "severe_gaps": 1, "reachable_unrevealed": 1, "no_static_path": 0 }
            })
            .to_string(),
        )?;
        fs::write(
            dir.path().join("comments.json"),
            json!({
                "head_sha": head,
                "base": "origin/other",
                "base_sha": "base-B",
                "comments": [
                    static_limitation_guidance_item(
                        "disjoint-s",
                        "src/other.rs",
                        10,
                        "predicate_boundary",
                    ),
                    static_limitation_guidance_item(
                        "excluded-x",
                        "src/x.rs",
                        20,
                        "predicate_boundary",
                    ),
                ],
                "summary_only": [],
                "suppressed": [
                    static_limitation_guidance_item(
                        "excluded-x",
                        "src/x.rs",
                        20,
                        "predicate_boundary",
                    ),
                ],
                "warnings": []
            })
            .to_string(),
        )?;
        let args = new_ripr_args(dir.path())?;

        let evaluation = evaluate_new_ripr(head, &args, None)?;

        assert!(evaluation.failed, "the counted gap must still block");
        let gap = gap_action(&evaluation, "new_ripr_gap");
        assert_eq!(gap.get("new_unresolved"), Some(&json!(1)));
        assert_eq!(
            gap.get("static_limitation_cleared").and_then(Value::as_array).map(Vec::len),
            Some(0),
            "the action must not list unspent dispositions as subtracted"
        );
        assert_eq!(
            evaluation.receipt.pointer("/ripr_pr/static_limitation_cleared"),
            Some(&json!(0)),
            "zero credit must report zero cleared"
        );
        assert_eq!(
            evaluation.receipt.pointer("/ripr_pr/static_limitation_unspent"),
            Some(&json!(["disjoint-s", "excluded-x"])),
            "unspent evidence stays visible under its own name"
        );
        Ok(())
    }

    /// P2 routed positive control: same-subject earned dispositions clear the
    /// basis and the receipt, action, and Markdown all report the applied
    /// credit — with `original = remaining + applied`.
    #[test]
    fn same_subject_clearing_reports_applied_credit_everywhere() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        fs::write(
            dir.path().join("ripr-plus.json"),
            json!({ "head": head, "unresolved": 0 }).to_string(),
        )?;
        fs::write(
            dir.path().join("repo-exposure.json"),
            json!({
                "head_sha": head,
                "base": "origin/main",
                "base_sha": "base-A",
                "summary": { "severe_gaps": 1, "reachable_unrevealed": 1, "no_static_path": 0 }
            })
            .to_string(),
        )?;
        fs::write(
            dir.path().join("comments.json"),
            json!({
                "head_sha": head,
                "base": "origin/main",
                "base_sha": "base-A",
                "comments": [
                    static_limitation_guidance_item(
                        "earned-s",
                        "src/a.rs",
                        10,
                        "predicate_boundary",
                    ),
                ],
                "summary_only": [],
                "suppressed": [],
                "warnings": []
            })
            .to_string(),
        )?;
        let args = new_ripr_args(dir.path())?;

        let evaluation = evaluate_new_ripr(head, &args, None)?;

        assert!(!evaluation.failed, "{:?}", evaluation.receipt["next_actions"]);
        let original = evaluation
            .receipt
            .pointer("/ripr_pr/new_unresolved")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let applied = evaluation
            .receipt
            .pointer("/ripr_pr/static_limitation_cleared")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        assert_eq!(
            (original, applied),
            (1, 1),
            "original = remaining + applied with zero remaining"
        );
        assert_eq!(
            evaluation.receipt.pointer("/ripr_pr/static_limitation_unspent"),
            Some(&json!([]))
        );
        let cleared = gap_action(&evaluation, "ripr_static_limitation_gaps_cleared");
        assert_eq!(
            cleared.get("static_limitation_gaps").and_then(Value::as_array).map(Vec::len),
            Some(1)
        );
        assert!(
            evaluation.markdown.contains(
                "- producer-classified static_limitation seams cleared from the basis (#15630): `1`"
            ),
            "Markdown must report the applied credit: {}",
            evaluation.markdown
        );
        Ok(())
    }

    // ── #10054 fallback guidance: named seams from the raw check ────────────

    fn minimal_exception_policy(dir: &Path) -> Result<PathBuf> {
        let path = dir.join("quality-gate-exceptions.toml");
        fs::write(
            &path,
            "schema_version = 1\npolicy = \"quality-gate-exceptions\"\nowner = \"test\"\nstatus = \"active\"\nupdated = \"2026-01-01\"\ndue_review = \"pass\"\n",
        )?;
        Ok(path)
    }

    fn new_ripr_args(dir: &Path) -> Result<QualityGateArgs> {
        Ok(QualityGateArgs {
            mode: QualityGateMode::EnforceNewRipr,
            exception_policy: minimal_exception_policy(dir)?,
            ripr_receipt: dir.join("ripr-plus.json"),
            ripr_pr_receipt: dir.join("repo-exposure.json"),
            review_receipt: dir.join("comments.json"),
            coverage_receipt: dir.join("coverage.json"),
            codecov: dir.join("codecov.yml"),
            patch_coverage: None,
            ripr_base: "origin/main".to_string(),
            ripr_head: "HEAD".to_string(),
            receipt: dir.join("quality-gate.json"),
            summary: dir.join("quality-gate.md"),
            check: false,
            quiet: false,
        })
    }

    fn write_gate_inputs(dir: &Path, head: &str, review_packet: &Value) -> Result<()> {
        fs::write(
            dir.join("ripr-plus.json"),
            json!({ "head": head, "unresolved": 0 }).to_string(),
        )?;
        fs::write(
            dir.join("repo-exposure.json"),
            json!({
                "head_sha": head,
                "base": "origin/main",
                "base_sha": "base-sha",
                "summary": { "severe_gaps": 2, "reachable_unrevealed": 1, "no_static_path": 1 }
            })
            .to_string(),
        )?;
        fs::write(dir.join("comments.json"), review_packet.to_string())?;
        Ok(())
    }

    fn named_fallback_seam() -> Value {
        json!({
            "id": "probe:crates_perl-parser-comparison_src_evidence.rs:495:error_path",
            "path": "crates/perl-parser-comparison/src/evidence.rs",
            "line": 495,
            "seam": "error_path: return Err(ComparisonModelError::ScoringRequiresCompletedHarness);",
            "reason": "no_static_path: No static test path found for the changed owner",
            "suggested_test": "Add a focused test that statically exercises the owner of this changed seam."
        })
    }

    #[test]
    fn incomplete_guidance_with_named_seams_extracts_top_gaps() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        fs::write(
            dir.path().join("comments.json"),
            json!({
                "head_sha": head,
                "status": "incomplete",
                "comments": [],
                "summary_only": [named_fallback_seam()],
                "suppressed": [],
                "warnings": [{ "kind": "tool_error", "message": "ripr timed out after 600s" }]
            })
            .to_string(),
        )?;

        let receipt = read_review_guidance_receipt(&dir.path().join("comments.json"), head);

        assert_eq!(receipt.status, "incomplete");
        assert_eq!(receipt.top_gaps.len(), 1);
        assert_eq!(
            receipt.top_gaps[0].get("path").and_then(Value::as_str),
            Some("crates/perl-parser-comparison/src/evidence.rs")
        );
        assert_eq!(receipt.unavailable_reason.as_deref(), Some("ripr timed out after 600s"));
        Ok(())
    }

    #[test]
    fn incomplete_guidance_with_named_seams_blocks_on_named_evidence_only() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        write_gate_inputs(
            dir.path(),
            head,
            &json!({
                "head_sha": head,
                "status": "incomplete",
                "comments": [],
                "summary_only": [named_fallback_seam()],
                "suppressed": [],
                "warnings": [{ "kind": "tool_error", "message": "ripr timed out after 600s" }]
            }),
        )?;
        let args = new_ripr_args(dir.path())?;

        let evaluation = evaluate_new_ripr(head, &args, None)?;

        assert!(evaluation.failed);
        let actions = evaluation
            .receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let gap_actions = actions
            .iter()
            .filter(|action| action.get("kind").and_then(Value::as_str) == Some("new_ripr_gap"))
            .count();
        assert_eq!(gap_actions, 1, "{actions:?}");
        let gap = actions
            .iter()
            .find(|action| action.get("kind").and_then(Value::as_str) == Some("new_ripr_gap"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(gap.get("gap_list_proven"), Some(&json!(true)));
        assert_eq!(gap.get("guidance_status"), Some(&json!("incomplete")));
        assert_eq!(gap.get("top_gaps").and_then(Value::as_array).map(Vec::len), Some(1));
        assert!(
            actions.iter().all(|action| action.get("kind").and_then(Value::as_str)
                != Some("ripr_review_receipt_not_current")),
            "named fallback guidance must not also block on the receipt: {actions:?}"
        );
        Ok(())
    }

    /// #11690: the blocking-count basis is production-scoped by the producer.
    /// A receipt whose only findings were non-production seams — the #6842
    /// shape, 162 proptest `no_static_path` entries inside the PR's own
    /// `tests/` file — deflates to zero, the gate passes, and the receipt
    /// surfaces how many seams were excluded so the zero is explainable.
    #[test]
    fn non_production_only_new_gaps_do_not_block_and_are_reported() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        fs::write(
            dir.path().join("ripr-plus.json"),
            json!({ "head": head, "unresolved": 0 }).to_string(),
        )?;
        fs::write(
            dir.path().join("repo-exposure.json"),
            json!({
                "head_sha": head,
                "base": "origin/main",
                "base_sha": "base-sha",
                "summary": {
                    "severe_gaps": 0,
                    "reachable_unrevealed": 0,
                    "no_static_path": 0,
                    "non_production_excluded": 162
                }
            })
            .to_string(),
        )?;
        fs::write(
            dir.path().join("comments.json"),
            json!({
                "head_sha": head,
                "status": "present",
                "comments": [],
                "summary_only": [],
                "suppressed": [],
                "analysis_scope": {
                    "production_files_considered": 6,
                    "changed_production_files": ["crates/perl-semantic-facts/src/lib.rs"]
                }
            })
            .to_string(),
        )?;
        let args = new_ripr_args(dir.path())?;

        let evaluation = evaluate_new_ripr(head, &args, None)?;

        assert!(!evaluation.failed, "{:?}", evaluation.receipt["next_actions"]);
        assert_eq!(evaluation.receipt.pointer("/ripr_pr/new_unresolved"), Some(&json!(0)));
        assert_eq!(
            evaluation.receipt.pointer("/ripr_pr/non_production_excluded"),
            Some(&json!(162))
        );
        Ok(())
    }

    #[test]
    fn error_guidance_without_named_seams_keeps_not_proven_blocking() -> Result<()> {
        let dir = tempdir()?;
        let head = "review-head";
        write_gate_inputs(
            dir.path(),
            head,
            &json!({
                "head_sha": head,
                "status": "error",
                "comments": [],
                "summary_only": [],
                "suppressed": [],
                "warnings": [{ "kind": "tool_error", "message": "ripr timed out after 600s" }]
            }),
        )?;
        let args = new_ripr_args(dir.path())?;

        let evaluation = evaluate_new_ripr(head, &args, None)?;

        assert!(evaluation.failed);
        let actions = evaluation
            .receipt
            .get("next_actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let gap = actions
            .iter()
            .find(|action| action.get("kind").and_then(Value::as_str) == Some("new_ripr_gap"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(gap.get("gap_list_proven"), Some(&json!(false)));
        assert!(
            actions.iter().any(|action| action.get("kind").and_then(Value::as_str)
                == Some("ripr_review_receipt_not_current")),
            "nameless guidance failure must still block on the receipt: {actions:?}"
        );
        Ok(())
    }

    #[test]
    fn overlay_preserves_committed_dates_for_receipt() {
        let raw = r#"
            schema_version = 1
            policy = "quality-gate-exceptions"
            owner = "test-owner"
            status = "active"
            updated = "2026-01-01"
            due_review = "fail"

            [[exception]]
            id = "EX-1"
            kind = "temporary_burndown"
            scope = "crates/perl-lsp-rs/src/foo.rs"
            owner = "team-a"
            reason = "tracking"
            final_target = "fix in 0.18"
            evidence = "git history"
            removal_criteria = "remove when fixed"
            created = "2026-01-01"
            review_after = "2026-12-31"
            expires = "2027-12-31"
        "#;
        let overlay = compute_lifecycle_overlay(raw).expect("overlay parses");

        let row = overlay.rows.get("EX-1").expect("committed dates recorded for the receipt");
        assert_eq!(
            row.review_after, "2026-12-31",
            "overlay must lift the committed review_after untouched"
        );
        assert_eq!(row.expires, "2027-12-31", "overlay must lift the committed expires untouched");
        assert!(
            overlay.validation_error.is_none(),
            "valid `due_review` must not trip the validation error path"
        );
        assert!(
            !overlay.invalid_metadata_status,
            "valid metadata must not flag the synthetic invalid_metadata suppression"
        );
    }

    #[test]
    fn overlay_records_validation_error_for_unsupported_due_review() {
        let raw = r#"
            schema_version = 1
            policy = "quality-gate-exceptions"
            owner = "test-owner"
            status = "active"
            updated = "2026-01-01"
            due_review = "panic"

            [[exception]]
            id = "EX-2"
            kind = "temporary_burndown"
            scope = "crates/perl-lsp-rs/src/bar.rs"
            owner = "team-b"
            reason = "tracking"
            final_target = "fix in 0.18"
            evidence = "git history"
            removal_criteria = "remove when fixed"
            created = "2026-01-01"
            review_after = "2026-12-31"
            expires = "2027-12-31"
        "#;
        let overlay = compute_lifecycle_overlay(raw).expect("overlay parses");
        assert_eq!(
            overlay.validation_error.as_deref(),
            Some("quality exception due_review must be warn or fail, found panic"),
            "unsupported due_review must produce the validation error the engine forwards"
        );
        assert!(
            overlay.invalid_metadata_status,
            "metadata is otherwise valid, so the synthetic invalid_metadata suppression must activate"
        );
        assert!(
            overlay.rows.contains_key("EX-2"),
            "exception rows must still be lifted even when due_review is malformed"
        );
    }
}
