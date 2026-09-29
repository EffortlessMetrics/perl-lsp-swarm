//! Hosted no-publish readiness rehearsal matrix (#16788).
//!
//! This module owns the hosted routing seam only: the admitted platform rows,
//! per-row identity binding, collision-proof artifact names, and fail-closed
//! fan-in. It does not implement package, VSIX, archive, SBOM, or release-shadow
//! rehearsal stages (#16785/#16786/#16787) and does not publish.

use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Tracked workflow that must invoke this command family and nothing else for
/// product verdicts.
pub const WORKFLOW_PATH: &str = ".github/workflows/readiness-rehearsal-hosted.yml";

/// Versioned hosted-row receipt schema. Distinct from the sibling rehearsal
/// product schema owned by #16785.
pub const HOSTED_ROW_SCHEMA: &str = "readiness_rehearsal_hosted_row.v1";

/// Versioned fan-in receipt schema.
pub const HOSTED_FANIN_SCHEMA: &str = "readiness_rehearsal_hosted_fanin.v1";

/// Versioned admitted-plan schema.
pub const HOSTED_PLAN_SCHEMA: &str = "readiness_rehearsal_hosted_plan.v1";

/// Well-known row receipt file name under the row output directory.
pub const ROW_RECEIPT_FILE: &str = "row.json";

/// Controlling issue for this hosted routing leaf.
pub const CONTROLLING_ISSUE: u64 = 16788;

/// Sibling receipt/schema owner. Missing inner receipts name this issue.
pub const RECEIPT_SCHEMA_ISSUE: u64 = 16785;

/// Package/VSIX rehearsal owner. Missing product stages name this issue.
pub const PACKAGE_VSIX_ISSUE: u64 = 16786;

/// Archive/supply-chain rehearsal owner.
pub const ARCHIVE_SUPPLY_ISSUE: u64 = 16787;

/// Manual-only trigger. Not a PR gate and not a scheduled spend.
pub const ADMITTED_FREQUENCY: &str = "workflow_dispatch";

/// Analogous three-OS hosted lane used for cost disposition (#16788 admission).
pub const COST_ANALOG_LANE: &str = "vscode_smoke_matrix";

/// Base LEM recorded for the analog three-OS smoke matrix.
pub const COST_ANALOG_BASE_LEM: u32 = 35;

/// One native hosted row admitted by the #16788 packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AdmittedRow {
    /// GitHub-hosted runner label.
    pub os: &'static str,
    /// Stable matrix subject used in artifact identity.
    pub subject: &'static str,
    /// Native OS family this row claims. Emulated hosts cannot be `pass`.
    pub native_os: &'static str,
    /// Evidence this row adds that one local Linux host cannot provide.
    pub evidence_proposition: &'static str,
}

/// Smallest hosted matrix justified without a local rehearsal-cost measurement.
///
/// Local #16786/#16787 rehearsal timings are not on current main. The analog
/// `vscode_smoke_matrix` already pays a three-OS hosted cost (~35 LEM, ~20 min
/// wall). Each native GitHub-hosted OS adds instrument and path evidence that a
/// single local Linux host cannot produce. Trigger remains explicit/manual so
/// the lane does not become a repository-wide required gate or a scheduled
/// spend before product-stage costs exist.
pub const ADMITTED_ROWS: &[AdmittedRow] = &[
    AdmittedRow {
        os: "ubuntu-24.04",
        subject: "ubuntu-24.04",
        native_os: "linux",
        evidence_proposition: "GitHub-hosted Linux runner, GITHUB run/attempt identity, and artifact fan-in vs a local Linux host",
    },
    AdmittedRow {
        os: "windows-latest",
        subject: "windows-latest",
        native_os: "windows",
        evidence_proposition: "Native Windows path/install/editor instruments unavailable from a Linux local host",
    },
    AdmittedRow {
        os: "macos-latest",
        subject: "macos-latest",
        native_os: "macos",
        evidence_proposition: "Native macOS path/install/editor instruments unavailable from a Linux local host",
    },
];

/// CLI for `cargo xtask readiness-rehearsal`.
#[derive(Subcommand)]
pub enum ReadinessRehearsalCommand {
    /// Emit the admitted hosted matrix plan consumed by workflow YAML.
    #[command(name = "hosted-plan")]
    HostedPlan {
        /// JSON plan output path.
        #[arg(long)]
        out: PathBuf,
        /// Optional GitHub Actions `GITHUB_OUTPUT` file to receive `matrix=`.
        #[arg(long)]
        github_output: Option<PathBuf>,
        /// Optional Markdown admission summary.
        #[arg(long)]
        summary: Option<PathBuf>,
    },
    /// Bind one matrix row's identity and consume an optional rehearsal receipt.
    #[command(name = "hosted-row")]
    HostedRow {
        /// Exact repository SHA this row claims.
        #[arg(long)]
        head: String,
        /// GitHub Actions run id.
        #[arg(long)]
        run_id: String,
        /// GitHub Actions run attempt.
        #[arg(long)]
        attempt: u32,
        /// Admitted matrix subject (must match `hosted-plan`).
        #[arg(long)]
        matrix_subject: String,
        /// Output directory for this row's collision-proof receipt.
        #[arg(long)]
        out: PathBuf,
        /// Repository root used for lockfile and git HEAD observation.
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
        /// Optional inner rehearsal receipt produced by #16785 consumers.
        #[arg(long)]
        rehearsal_receipt: Option<PathBuf>,
    },
    /// Fan-in every expected row; missing/cancelled/colliding producers stay non-green.
    #[command(name = "hosted-fanin")]
    HostedFanin {
        /// Admitted plan JSON from `hosted-plan`.
        #[arg(long)]
        plan: PathBuf,
        /// Directory tree containing downloaded row artifacts.
        #[arg(long)]
        rows_dir: PathBuf,
        /// Fan-in JSON receipt.
        #[arg(long)]
        out: PathBuf,
        /// Optional Markdown summary.
        #[arg(long)]
        summary: Option<PathBuf>,
    },
}

/// Dispatch `cargo xtask readiness-rehearsal`.
pub fn run(command: ReadinessRehearsalCommand) -> Result<()> {
    match command {
        ReadinessRehearsalCommand::HostedPlan { out, github_output, summary } => {
            run_hosted_plan(&out, github_output.as_deref(), summary.as_deref())
        }
        ReadinessRehearsalCommand::HostedRow {
            head,
            run_id,
            attempt,
            matrix_subject,
            out,
            repo_root,
            rehearsal_receipt,
        } => {
            let observed_git_head = git_head(&repo_root)?;
            let rustc_host = observe_rustc_host()?;
            let runner_os = observe_runner_os();
            let lockfile_digest = lockfile_digest(&repo_root)?;
            let request = HostedRowRequest {
                head,
                observed_git_head,
                run_id,
                attempt,
                matrix_subject,
                out_dir: out,
                rehearsal_receipt,
                job_conclusion: JobConclusion::Success,
                cleanup: CleanupDisposition::Pass,
                runner_os,
                rustc_host,
                lockfile_digest,
            };
            let receipt = compile_hosted_row(&request)?;
            write_row_receipt(&request.out_dir, &receipt)?;
            if receipt.fan_in_green() {
                Ok(())
            } else {
                bail!(
                    "hosted row {} is {} (non-green)",
                    receipt.artifact_id,
                    receipt.row_status.as_str()
                )
            }
        }
        ReadinessRehearsalCommand::HostedFanin { plan, rows_dir, out, summary } => {
            let plan = read_plan(&plan)?;
            let fanin = compile_hosted_fanin(&plan, &rows_dir)?;
            write_json(&out, &fanin)?;
            if let Some(summary) = summary {
                fs::write(&summary, render_fanin_markdown(&fanin))
                    .with_context(|| format!("writing {}", summary.display()))?;
            }
            if fanin.verdict == FaninVerdict::Green {
                Ok(())
            } else {
                bail!("hosted fan-in is {} (non-green)", fanin.verdict.as_str())
            }
        }
    }
}

/// Inputs for one hosted row. Tests construct this directly; the CLI observes.
#[derive(Debug, Clone)]
pub struct HostedRowRequest {
    /// Claimed repository SHA.
    pub head: String,
    /// `git rev-parse HEAD` (or test double).
    pub observed_git_head: String,
    /// GitHub run id.
    pub run_id: String,
    /// GitHub run attempt.
    pub attempt: u32,
    /// Admitted matrix subject.
    pub matrix_subject: String,
    /// Directory that will hold `row.json`.
    pub out_dir: PathBuf,
    /// Optional inner rehearsal receipt path.
    pub rehearsal_receipt: Option<PathBuf>,
    /// Producer job conclusion as observed by the row (tests inject cancelled).
    pub job_conclusion: JobConclusion,
    /// Cleanup disposition for this hosted row's own outputs.
    pub cleanup: CleanupDisposition,
    /// `GITHUB_RUNNER_OS` or local equivalent (`Linux` / `Windows` / `macOS`).
    pub runner_os: String,
    /// `rustc -vV` host triple.
    pub rustc_host: String,
    /// SHA-256 of `Cargo.lock`.
    pub lockfile_digest: String,
}

/// Producer job conclusion. Cancelled/timed-out/skipped/missing stay non-green.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobConclusion {
    /// The row command finished and wrote a receipt.
    Success,
    /// The row command failed after writing a receipt.
    Failure,
    /// The producer was cancelled.
    Cancelled,
    /// The producer timed out.
    TimedOut,
    /// The producer was skipped.
    Skipped,
    /// No producer ran.
    Missing,
}

impl JobConclusion {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
            Self::Missing => "missing",
        }
    }

    fn is_complete(self) -> bool {
        matches!(self, Self::Success | Self::Failure)
    }
}

/// Cleanup of the hosted row's own staging outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupDisposition {
    /// Only `--out` was written and retained as intended.
    Pass,
    /// Cleanup failed. Cannot be hidden by a later upload.
    Failed,
    /// Cleanup evidence is missing.
    NotProven,
}

impl CleanupDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Failed => "failed",
            Self::NotProven => "not_proven",
        }
    }
}

/// Row-level status vocabulary shared with the parent rehearsal contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    /// Native row with a green inner receipt and successful cleanup.
    Pass,
    /// Completed with an explicit limitation (not a missing producer).
    Limited,
    /// Deterministic product or identity failure.
    Failed,
    /// Missing, incomplete, cancelled, emulated, or instrument-failed evidence.
    NotProven,
}

impl RowStatus {
    /// Stable snake_case token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Limited => "limited",
            Self::Failed => "failed",
            Self::NotProven => "not_proven",
        }
    }

    fn is_fan_in_green(self) -> bool {
        matches!(self, Self::Pass | Self::Limited)
    }
}

/// Named limitation bound to an owning issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limitation {
    /// Stable code for tests and summaries.
    pub code: String,
    /// Owning GitHub issue.
    pub owning_issue: u64,
    /// Human description.
    pub message: String,
}

/// Hosted row receipt. Semantic identity is independent of timestamps and
/// absolute temporary paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedRowReceipt {
    /// Schema id.
    pub schema: String,
    /// Exact repository SHA.
    pub repository_sha: String,
    /// GitHub run id.
    pub run_id: String,
    /// GitHub run attempt.
    pub run_attempt: u32,
    /// Admitted matrix subject.
    pub matrix_subject: String,
    /// Collision-proof artifact identity: `{run_id}-{attempt}-{subject}`.
    pub artifact_id: String,
    /// Runner OS label.
    pub runner_os: String,
    /// rustc host triple.
    pub rustc_host: String,
    /// SHA-256 hex of `Cargo.lock`.
    pub lockfile_digest: String,
    /// Producer job conclusion.
    pub job_conclusion: JobConclusion,
    /// Hosted-row status.
    pub row_status: RowStatus,
    /// Cleanup disposition.
    pub cleanup: CleanupDisposition,
    /// Must remain empty. Non-empty fails the row.
    pub published_channels: Vec<String>,
    /// Must remain false. `true` fails the row.
    pub release_cut: bool,
    /// Whether rustc host matches the admitted native OS.
    pub native_host: bool,
    /// Digest of the consumed inner rehearsal receipt, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rehearsal_receipt_digest: Option<String>,
    /// Inner rehearsal status token, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rehearsal_status: Option<String>,
    /// Limitations and owning issues.
    pub limitations: Vec<Limitation>,
}

impl HostedRowReceipt {
    fn fan_in_green(&self) -> bool {
        self.row_status.is_fan_in_green()
            && self.published_channels.is_empty()
            && !self.release_cut
            && self.cleanup == CleanupDisposition::Pass
            && self.job_conclusion == JobConclusion::Success
    }
}

/// Opaque-plus-guard view of a sibling rehearsal receipt. Unknown fields are
/// ignored so #16785 can grow the product schema without a hosted-layer fork.
#[derive(Debug, Clone, Default, Deserialize)]
struct RehearsalReceiptView {
    #[serde(default)]
    published_channels: Vec<String>,
    #[serde(default)]
    release_cut: bool,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    cleanup: Option<String>,
}

enum InnerReceiptInspect {
    Unreadable { digest: Option<String>, message: String },
    Malformed { digest: String, message: String },
    Usable { digest: String, view: RehearsalReceiptView },
}

fn inspect_inner_receipt(path: &Path) -> InnerReceiptInspect {
    match fs::read(path) {
        Err(error) => InnerReceiptInspect::Unreadable {
            digest: None,
            message: format!("could not read {}: {error}", path.display()),
        },
        Ok(bytes) => {
            let digest = hex_digest(&bytes);
            match serde_json::from_slice::<RehearsalReceiptView>(&bytes) {
                Ok(view) => InnerReceiptInspect::Usable { digest, view },
                Err(error) => InnerReceiptInspect::Malformed {
                    digest,
                    message: format!("inner rehearsal receipt is not usable JSON: {error}"),
                },
            }
        }
    }
}

/// Admitted plan emitted by `hosted-plan` and required by fan-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedPlan {
    /// Schema id.
    pub schema: String,
    /// Tracked workflow this plan is for.
    pub workflow: String,
    /// Controlling issue.
    pub controlling_issue: u64,
    /// Trigger frequency.
    pub frequency: String,
    /// Analogous lane used for cost disposition.
    pub cost_analog_lane: String,
    /// Analogous base LEM.
    pub cost_analog_base_lem: u32,
    /// Whether this lane is a required PR gate (must stay false).
    pub required_pr_gate: bool,
    /// Admitted rows.
    pub rows: Vec<HostedPlanRow>,
}

/// One plan row, including the GitHub Actions runner label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedPlanRow {
    /// Runner label (`runs-on`).
    pub os: String,
    /// Matrix subject / artifact identity token.
    pub subject: String,
    /// Native OS family.
    pub native_os: String,
    /// Why this row exists.
    pub evidence_proposition: String,
}

/// Compact matrix object written to `GITHUB_OUTPUT`.
#[derive(Debug, Clone, Serialize)]
struct GithubMatrix {
    include: Vec<GithubMatrixRow>,
}

#[derive(Debug, Clone, Serialize)]
struct GithubMatrixRow {
    os: String,
    subject: String,
    native_os: String,
}

/// Fan-in overall verdict. `limited` rows may still be green; missing is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaninVerdict {
    /// Every expected row is present and `pass` or `limited`.
    Green,
    /// At least one expected producer failed, is missing, or is not_proven.
    NonGreen,
}

impl FaninVerdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::NonGreen => "non_green",
        }
    }
}

/// One row as observed by fan-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaninRowObservation {
    /// Expected matrix subject.
    pub matrix_subject: String,
    /// Observation kind.
    pub outcome: String,
    /// Artifact id when a receipt was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    /// Row status when a receipt was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_status: Option<RowStatus>,
    /// Why this observation is non-green, if it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Fan-in receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedFaninReceipt {
    /// Schema id.
    pub schema: String,
    /// Exact SHA every row must share.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_sha: Option<String>,
    /// Run id every row must share.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Attempt every row must share.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_attempt: Option<u32>,
    /// Overall verdict.
    pub verdict: FaninVerdict,
    /// Per-subject observations, including missing producers.
    pub rows: Vec<FaninRowObservation>,
    /// Extra receipts that were not in the admitted plan.
    pub unexpected_subjects: Vec<String>,
    /// Identity collisions (same artifact id or subject claimed twice).
    pub collisions: Vec<String>,
    /// Summary limitations.
    pub limitations: Vec<Limitation>,
}

/// Compile the admitted plan (pure).
pub fn admitted_plan() -> HostedPlan {
    HostedPlan {
        schema: HOSTED_PLAN_SCHEMA.to_string(),
        workflow: WORKFLOW_PATH.to_string(),
        controlling_issue: CONTROLLING_ISSUE,
        frequency: ADMITTED_FREQUENCY.to_string(),
        cost_analog_lane: COST_ANALOG_LANE.to_string(),
        cost_analog_base_lem: COST_ANALOG_BASE_LEM,
        required_pr_gate: false,
        rows: ADMITTED_ROWS
            .iter()
            .map(|row| HostedPlanRow {
                os: row.os.to_string(),
                subject: row.subject.to_string(),
                native_os: row.native_os.to_string(),
                evidence_proposition: row.evidence_proposition.to_string(),
            })
            .collect(),
    }
}

/// Artifact identity token. Run + attempt + subject cannot be reused across rows.
pub fn artifact_id(run_id: &str, attempt: u32, matrix_subject: &str) -> String {
    format!("{run_id}-{attempt}-{matrix_subject}")
}

/// Compile one hosted row receipt without writing it.
pub fn compile_hosted_row(request: &HostedRowRequest) -> Result<HostedRowReceipt> {
    let mut limitations = Vec::new();
    let admitted = ADMITTED_ROWS.iter().find(|row| row.subject == request.matrix_subject);
    let Some(admitted) = admitted else {
        return Ok(failed_row(
            request,
            RowStatus::Failed,
            Limitation {
                code: "unadmitted_matrix_subject".to_string(),
                owning_issue: CONTROLLING_ISSUE,
                message: format!(
                    "matrix subject `{}` is not in the admitted hosted plan",
                    request.matrix_subject
                ),
            },
        ));
    };

    if request.head != request.observed_git_head {
        return Ok(failed_row(
            request,
            RowStatus::Failed,
            Limitation {
                code: "head_mismatch".to_string(),
                owning_issue: CONTROLLING_ISSUE,
                message: format!(
                    "claimed SHA {} is not the observed git HEAD {}",
                    request.head, request.observed_git_head
                ),
            },
        ));
    }

    let native_host = rustc_host_matches(admitted.native_os, &request.rustc_host)
        && runner_os_matches(admitted.native_os, &request.runner_os);
    if !native_host {
        limitations.push(Limitation {
            code: "emulated_or_mismatched_host".to_string(),
            owning_issue: CONTROLLING_ISSUE,
            message: format!(
                "runner_os `{}` rustc_host `{}` is not native `{}`; cannot be pass",
                request.runner_os, request.rustc_host, admitted.native_os
            ),
        });
    }

    if request.cleanup != CleanupDisposition::Pass {
        limitations.push(Limitation {
            code: "cleanup_not_pass".to_string(),
            owning_issue: CONTROLLING_ISSUE,
            message: format!(
                "cleanup is {}; upload success cannot hide cleanup failure",
                request.cleanup.as_str()
            ),
        });
    }

    if !request.job_conclusion.is_complete() {
        limitations.push(Limitation {
            code: format!("producer_{}", request.job_conclusion.as_str()),
            owning_issue: CONTROLLING_ISSUE,
            message: format!(
                "producer conclusion `{}` is incomplete and cannot be pass",
                request.job_conclusion.as_str()
            ),
        });
    }

    let mut published_channels = Vec::new();
    let mut release_cut = false;
    let mut rehearsal_receipt_digest = None;
    let mut rehearsal_status = None;

    if let Some(path) = &request.rehearsal_receipt {
        match inspect_inner_receipt(path) {
            InnerReceiptInspect::Unreadable { digest, message } => {
                rehearsal_receipt_digest = digest;
                limitations.push(Limitation {
                    code: "unreadable_rehearsal_receipt".to_string(),
                    owning_issue: RECEIPT_SCHEMA_ISSUE,
                    message,
                });
            }
            InnerReceiptInspect::Malformed { digest, message } => {
                rehearsal_receipt_digest = Some(digest);
                limitations.push(Limitation {
                    code: "malformed_rehearsal_receipt".to_string(),
                    owning_issue: RECEIPT_SCHEMA_ISSUE,
                    message,
                });
            }
            InnerReceiptInspect::Usable { digest, view } => {
                rehearsal_receipt_digest = Some(digest);
                published_channels = view.published_channels;
                release_cut = view.release_cut;
                rehearsal_status = view.status.clone();
                if !published_channels.is_empty() {
                    limitations.push(Limitation {
                        code: "published_channels_not_empty".to_string(),
                        owning_issue: CONTROLLING_ISSUE,
                        message: format!("inner receipt published_channels={published_channels:?}"),
                    });
                }
                if release_cut {
                    limitations.push(Limitation {
                        code: "release_cut_true".to_string(),
                        owning_issue: CONTROLLING_ISSUE,
                        message: "inner receipt set release_cut=true".to_string(),
                    });
                }
                if view.cleanup.as_deref() == Some("failed") {
                    limitations.push(Limitation {
                        code: "inner_cleanup_failed".to_string(),
                        owning_issue: RECEIPT_SCHEMA_ISSUE,
                        message: "inner rehearsal cleanup failed".to_string(),
                    });
                }
                match view.status.as_deref() {
                    Some("pass") | Some("limited") | Some("failed") | Some("not_proven") => {}
                    Some(other) => limitations.push(Limitation {
                        code: "unknown_inner_status".to_string(),
                        owning_issue: RECEIPT_SCHEMA_ISSUE,
                        message: format!("inner status `{other}` is not a known rehearsal status"),
                    }),
                    None => limitations.push(Limitation {
                        code: "missing_inner_status".to_string(),
                        owning_issue: RECEIPT_SCHEMA_ISSUE,
                        message: "inner rehearsal receipt omitted status".to_string(),
                    }),
                }
            }
        }
    } else {
        limitations.push(Limitation {
            code: "missing_rehearsal_receipt".to_string(),
            owning_issue: RECEIPT_SCHEMA_ISSUE,
            message: format!(
                "no inner rehearsal receipt; product stages remain not_proven until #{PACKAGE_VSIX_ISSUE}/#{ARCHIVE_SUPPLY_ISSUE} produce a #16785 receipt"
            ),
        });
    }

    let row_status = decide_row_status(
        request,
        native_host,
        &published_channels,
        release_cut,
        rehearsal_status.as_deref(),
        &limitations,
    );

    Ok(HostedRowReceipt {
        schema: HOSTED_ROW_SCHEMA.to_string(),
        repository_sha: request.head.clone(),
        run_id: request.run_id.clone(),
        run_attempt: request.attempt,
        matrix_subject: request.matrix_subject.clone(),
        artifact_id: artifact_id(&request.run_id, request.attempt, &request.matrix_subject),
        runner_os: request.runner_os.clone(),
        rustc_host: request.rustc_host.clone(),
        lockfile_digest: request.lockfile_digest.clone(),
        job_conclusion: request.job_conclusion,
        row_status,
        cleanup: request.cleanup,
        published_channels,
        release_cut,
        native_host,
        rehearsal_receipt_digest,
        rehearsal_status,
        limitations,
    })
}

fn decide_row_status(
    request: &HostedRowRequest,
    native_host: bool,
    published_channels: &[String],
    release_cut: bool,
    rehearsal_status: Option<&str>,
    limitations: &[Limitation],
) -> RowStatus {
    if !published_channels.is_empty() || release_cut {
        return RowStatus::Failed;
    }
    if request.cleanup != CleanupDisposition::Pass {
        return RowStatus::Failed;
    }
    if !request.job_conclusion.is_complete() {
        return RowStatus::NotProven;
    }
    if request.job_conclusion == JobConclusion::Failure {
        return RowStatus::Failed;
    }
    if limitations.iter().any(|item| {
        matches!(
            item.code.as_str(),
            "malformed_rehearsal_receipt" | "unreadable_rehearsal_receipt" | "inner_cleanup_failed"
        )
    }) {
        return RowStatus::Failed;
    }
    if !native_host {
        return RowStatus::NotProven;
    }
    if request.rehearsal_receipt.is_none() {
        return RowStatus::NotProven;
    }
    match rehearsal_status {
        Some("pass")
            if limitations.iter().all(|item| {
                item.code != "inner_cleanup_failed" && item.code != "unknown_inner_status"
            }) =>
        {
            RowStatus::Pass
        }
        Some("limited") => RowStatus::Limited,
        Some("failed") => RowStatus::Failed,
        Some("not_proven") | None => RowStatus::NotProven,
        Some(_) => RowStatus::NotProven,
    }
}

fn failed_row(
    request: &HostedRowRequest,
    row_status: RowStatus,
    limitation: Limitation,
) -> HostedRowReceipt {
    HostedRowReceipt {
        schema: HOSTED_ROW_SCHEMA.to_string(),
        repository_sha: request.head.clone(),
        run_id: request.run_id.clone(),
        run_attempt: request.attempt,
        matrix_subject: request.matrix_subject.clone(),
        artifact_id: artifact_id(&request.run_id, request.attempt, &request.matrix_subject),
        runner_os: request.runner_os.clone(),
        rustc_host: request.rustc_host.clone(),
        lockfile_digest: request.lockfile_digest.clone(),
        job_conclusion: request.job_conclusion,
        row_status,
        cleanup: request.cleanup,
        published_channels: Vec::new(),
        release_cut: false,
        native_host: false,
        rehearsal_receipt_digest: None,
        rehearsal_status: None,
        limitations: vec![limitation],
    }
}

/// Compile fan-in from an admitted plan and a directory of row receipts.
pub fn compile_hosted_fanin(plan: &HostedPlan, rows_dir: &Path) -> Result<HostedFaninReceipt> {
    if plan.required_pr_gate {
        bail!("admitted plan must not set required_pr_gate");
    }
    if plan.frequency != ADMITTED_FREQUENCY {
        bail!("admitted plan frequency must remain {ADMITTED_FREQUENCY}");
    }

    let found = collect_row_receipts(rows_dir)?;
    let mut by_subject: BTreeMap<String, Vec<HostedRowReceipt>> = BTreeMap::new();
    let mut collisions = Vec::new();
    let mut artifact_ids: BTreeSet<String> = BTreeSet::new();

    for receipt in found {
        if !artifact_ids.insert(receipt.artifact_id.clone()) {
            collisions.push(format!("duplicate artifact_id {}", receipt.artifact_id));
        }
        by_subject.entry(receipt.matrix_subject.clone()).or_default().push(receipt);
    }

    let expected: Vec<String> = plan.rows.iter().map(|row| row.subject.clone()).collect();
    let expected_set: BTreeSet<String> = expected.iter().cloned().collect();
    let unexpected_subjects: Vec<String> =
        by_subject.keys().filter(|subject| !expected_set.contains(*subject)).cloned().collect();

    for (subject, rows) in &by_subject {
        if rows.len() > 1 {
            collisions.push(format!("subject `{subject}` claimed by {} receipts", rows.len()));
        }
        for row in rows {
            let expected_id = artifact_id(&row.run_id, row.run_attempt, &row.matrix_subject);
            if row.artifact_id != expected_id {
                collisions.push(format!(
                    "subject `{subject}` artifact_id {} does not match {}",
                    row.artifact_id, expected_id
                ));
            }
        }
    }

    let mut shared_sha: Option<String> = None;
    let mut shared_run: Option<String> = None;
    let mut shared_attempt: Option<u32> = None;
    let mut observations = Vec::new();
    let mut limitations = Vec::new();

    for subject in &expected {
        match by_subject.get(subject) {
            None => {
                observations.push(FaninRowObservation {
                    matrix_subject: subject.clone(),
                    outcome: "missing".to_string(),
                    artifact_id: None,
                    row_status: None,
                    detail: Some("expected producer receipt was not present in fan-in".to_string()),
                });
                limitations.push(Limitation {
                    code: "missing_producer".to_string(),
                    owning_issue: CONTROLLING_ISSUE,
                    message: format!("matrix subject `{subject}` produced no receipt"),
                });
            }
            Some(rows) => {
                let row = &rows[0];
                if let Some(sha) = &shared_sha {
                    if sha != &row.repository_sha {
                        collisions.push(format!(
                            "subject `{subject}` SHA {} mixed with {sha}",
                            row.repository_sha
                        ));
                    }
                } else {
                    shared_sha = Some(row.repository_sha.clone());
                }
                if let Some(run) = &shared_run {
                    if run != &row.run_id {
                        collisions.push(format!(
                            "subject `{subject}` run {} mixed with {run}",
                            row.run_id
                        ));
                    }
                } else {
                    shared_run = Some(row.run_id.clone());
                }
                if let Some(attempt) = shared_attempt {
                    if attempt != row.run_attempt {
                        collisions.push(format!(
                            "subject `{subject}` attempt {} mixed with {attempt}",
                            row.run_attempt
                        ));
                    }
                } else {
                    shared_attempt = Some(row.run_attempt);
                }

                let mut detail = None;
                let mut outcome = row.row_status.as_str().to_string();
                if !row.fan_in_green() {
                    detail = Some(format!(
                        "status={} conclusion={} cleanup={} native_host={}",
                        row.row_status.as_str(),
                        row.job_conclusion.as_str(),
                        row.cleanup.as_str(),
                        row.native_host
                    ));
                    outcome = if row.job_conclusion == JobConclusion::Cancelled {
                        "cancelled".to_string()
                    } else if row.job_conclusion == JobConclusion::TimedOut {
                        "timed_out".to_string()
                    } else if row.job_conclusion == JobConclusion::Skipped {
                        "skipped".to_string()
                    } else if row.job_conclusion == JobConclusion::Missing {
                        "missing".to_string()
                    } else {
                        row.row_status.as_str().to_string()
                    };
                }
                observations.push(FaninRowObservation {
                    matrix_subject: subject.clone(),
                    outcome,
                    artifact_id: Some(row.artifact_id.clone()),
                    row_status: Some(row.row_status),
                    detail,
                });
            }
        }
    }

    for subject in &unexpected_subjects {
        limitations.push(Limitation {
            code: "unadmitted_matrix_expansion".to_string(),
            owning_issue: CONTROLLING_ISSUE,
            message: format!("fan-in saw subject `{subject}` which is not in the admitted plan"),
        });
    }
    for collision in &collisions {
        limitations.push(Limitation {
            code: "identity_collision".to_string(),
            owning_issue: CONTROLLING_ISSUE,
            message: collision.clone(),
        });
    }

    let any_non_green = observations.iter().any(|row| {
        row.row_status.map(RowStatus::is_fan_in_green) != Some(true) || row.detail.is_some()
    }) || !unexpected_subjects.is_empty()
        || !collisions.is_empty();

    Ok(HostedFaninReceipt {
        schema: HOSTED_FANIN_SCHEMA.to_string(),
        repository_sha: shared_sha,
        run_id: shared_run,
        run_attempt: shared_attempt,
        verdict: if any_non_green { FaninVerdict::NonGreen } else { FaninVerdict::Green },
        rows: observations,
        unexpected_subjects,
        collisions,
        limitations,
    })
}

fn collect_row_receipts(rows_dir: &Path) -> Result<Vec<HostedRowReceipt>> {
    let mut receipts = Vec::new();
    if !rows_dir.exists() {
        return Ok(receipts);
    }
    collect_row_receipts_walk(rows_dir, &mut receipts)?;
    Ok(receipts)
}

fn collect_row_receipts_walk(dir: &Path, receipts: &mut Vec<HostedRowReceipt>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry.file_type().with_context(|| format!("stat {}", path.display()))?;
        if file_type.is_dir() {
            collect_row_receipts_walk(&path, receipts)?;
            continue;
        }
        if file_type.is_file()
            && path.file_name().and_then(|name| name.to_str()) == Some(ROW_RECEIPT_FILE)
        {
            let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let receipt: HostedRowReceipt = serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing hosted row receipt {}", path.display()))?;
            receipts.push(receipt);
        }
    }
    Ok(())
}

fn run_hosted_plan(out: &Path, github_output: Option<&Path>, summary: Option<&Path>) -> Result<()> {
    let plan = admitted_plan();
    write_json(out, &plan)?;
    if let Some(github_output) = github_output {
        let matrix = GithubMatrix {
            include: plan
                .rows
                .iter()
                .map(|row| GithubMatrixRow {
                    os: row.os.clone(),
                    subject: row.subject.clone(),
                    native_os: row.native_os.clone(),
                })
                .collect(),
        };
        let encoded = serde_json::to_string(&matrix).context("encoding GitHub matrix JSON")?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(github_output)
            .with_context(|| format!("opening {}", github_output.display()))?;
        writeln!(file, "matrix={encoded}")
            .with_context(|| format!("writing matrix to {}", github_output.display()))?;
    }
    if let Some(summary) = summary {
        fs::write(summary, render_plan_markdown(&plan))
            .with_context(|| format!("writing {}", summary.display()))?;
    }
    Ok(())
}

fn read_plan(path: &Path) -> Result<HostedPlan> {
    let bytes = fs::read(path).with_context(|| format!("reading plan {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing plan {}", path.display()))
}

fn write_row_receipt(out_dir: &Path, receipt: &HostedRowReceipt) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    write_json(&out_dir.join(ROW_RECEIPT_FILE), receipt)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).context("serializing JSON")?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn render_plan_markdown(plan: &HostedPlan) -> String {
    let mut lines = vec![
        "# Hosted no-publish rehearsal admission".to_string(),
        String::new(),
        format!("- controlling issue: #{}", plan.controlling_issue),
        format!("- frequency: `{}` (not a required PR gate)", plan.frequency),
        format!("- cost analog: `{}` ({} LEM)", plan.cost_analog_lane, plan.cost_analog_base_lem),
        format!("- required_pr_gate: {}", plan.required_pr_gate),
        String::new(),
        "| subject | runner | native_os | evidence |".to_string(),
        "| --- | --- | --- | --- |".to_string(),
    ];
    for row in &plan.rows {
        lines.push(format!(
            "| `{}` | `{}` | `{}` | {} |",
            row.subject, row.os, row.native_os, row.evidence_proposition
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn render_fanin_markdown(fanin: &HostedFaninReceipt) -> String {
    let mut lines = vec![
        "# Hosted no-publish rehearsal fan-in".to_string(),
        String::new(),
        format!("- verdict: `{}`", fanin.verdict.as_str()),
        format!("- repository_sha: {}", fanin.repository_sha.as_deref().unwrap_or("missing")),
        format!("- run_id: {}", fanin.run_id.as_deref().unwrap_or("missing")),
        format!(
            "- run_attempt: {}",
            fanin
                .run_attempt
                .map(|attempt| attempt.to_string())
                .unwrap_or_else(|| "missing".to_string())
        ),
        String::new(),
        "| subject | outcome | artifact | detail |".to_string(),
        "| --- | --- | --- | --- |".to_string(),
    ];
    for row in &fanin.rows {
        lines.push(format!(
            "| `{}` | `{}` | `{}` | {} |",
            row.matrix_subject,
            row.outcome,
            row.artifact_id.as_deref().unwrap_or("missing"),
            row.detail.as_deref().unwrap_or("")
        ));
    }
    if !fanin.unexpected_subjects.is_empty() {
        lines.push(String::new());
        lines.push(format!("unexpected subjects: {}", fanin.unexpected_subjects.join(", ")));
    }
    if !fanin.collisions.is_empty() {
        lines.push(String::new());
        lines.push("collisions:".to_string());
        for collision in &fanin.collisions {
            lines.push(format!("- {collision}"));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

fn git_head(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .context("running git rev-parse HEAD")?;
    if !output.status.success() {
        bail!("git rev-parse HEAD failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let head = String::from_utf8(output.stdout).context("git HEAD was not UTF-8")?;
    Ok(head.trim().to_string())
}

fn observe_rustc_host() -> Result<String> {
    let output = Command::new("rustc").arg("-vV").output().context("running rustc -vV")?;
    if !output.status.success() {
        return Err(eyre!("rustc -vV failed; hosted row cannot bind toolchain identity"));
    }
    let text = String::from_utf8(output.stdout).context("rustc -vV was not UTF-8")?;
    parse_rustc_host(&text).ok_or_else(|| eyre!("rustc -vV did not include a host: line"))
}

fn parse_rustc_host(text: &str) -> Option<String> {
    text.lines().find_map(|line| line.strip_prefix("host: ").map(str::trim).map(ToOwned::to_owned))
}

fn observe_runner_os() -> String {
    std::env::var("GITHUB_RUNNER_OS").unwrap_or_else(|_| match std::env::consts::OS {
        "linux" => "Linux".to_string(),
        "windows" => "Windows".to_string(),
        "macos" => "macOS".to_string(),
        other => other.to_string(),
    })
}

fn lockfile_digest(repo_root: &Path) -> Result<String> {
    let path = repo_root.join("Cargo.lock");
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rustc_host_matches(native_os: &str, rustc_host: &str) -> bool {
    match native_os {
        "linux" => rustc_host.contains("linux"),
        "windows" => rustc_host.contains("windows"),
        "macos" => rustc_host.contains("apple-darwin") || rustc_host.contains("macos"),
        _ => false,
    }
}

fn runner_os_matches(native_os: &str, runner_os: &str) -> bool {
    match native_os {
        "linux" => runner_os.eq_ignore_ascii_case("linux"),
        "windows" => runner_os.eq_ignore_ascii_case("windows"),
        "macos" => {
            runner_os.eq_ignore_ascii_case("macos") || runner_os.eq_ignore_ascii_case("macOS")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn native_linux_request(dir: &Path) -> HostedRowRequest {
        HostedRowRequest {
            head: "abc123".to_string(),
            observed_git_head: "abc123".to_string(),
            run_id: "44".to_string(),
            attempt: 1,
            matrix_subject: "ubuntu-24.04".to_string(),
            out_dir: dir.join("row"),
            rehearsal_receipt: None,
            job_conclusion: JobConclusion::Success,
            cleanup: CleanupDisposition::Pass,
            runner_os: "Linux".to_string(),
            rustc_host: "x86_64-unknown-linux-gnu".to_string(),
            lockfile_digest: "lock".to_string(),
        }
    }

    fn write_inner(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("rehearsal.json");
        fs::write(&path, body).expect("write inner receipt");
        path
    }

    fn green_inner(dir: &Path) -> PathBuf {
        write_inner(
            dir,
            r#"{"published_channels":[],"release_cut":false,"status":"pass","cleanup":"pass"}"#,
        )
    }

    #[test]
    fn admitted_plan_is_the_three_native_hosted_rows() {
        let plan = admitted_plan();
        let subjects: Vec<&str> = plan.rows.iter().map(|row| row.subject.as_str()).collect();
        assert_eq!(subjects, vec!["ubuntu-24.04", "windows-latest", "macos-latest"]);
        assert_eq!(plan.workflow, WORKFLOW_PATH);
        assert!(!plan.required_pr_gate);
        assert_eq!(plan.frequency, ADMITTED_FREQUENCY);
        assert_eq!(plan.cost_analog_lane, COST_ANALOG_LANE);
        assert_eq!(plan.cost_analog_base_lem, COST_ANALOG_BASE_LEM);
    }

    #[test]
    fn missing_inner_receipt_is_not_proven() {
        let dir = tempdir().expect("tempdir");
        let receipt = compile_hosted_row(&native_linux_request(dir.path())).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::NotProven);
        assert!(receipt.limitations.iter().any(|item| item.code == "missing_rehearsal_receipt"));
        assert_eq!(receipt.owning_missing_issue(), Some(RECEIPT_SCHEMA_ISSUE));
        assert!(!receipt.fan_in_green());
    }

    #[test]
    fn green_inner_native_row_passes() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(green_inner(dir.path()));
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Pass);
        assert!(receipt.fan_in_green());
        assert_eq!(receipt.artifact_id, "44-1-ubuntu-24.04");
        assert!(receipt.published_channels.is_empty());
        assert!(!receipt.release_cut);
    }

    #[test]
    fn malformed_inner_receipt_fails() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(write_inner(dir.path(), "not-json"));
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
        assert!(receipt.limitations.iter().any(|item| item.code == "malformed_rehearsal_receipt"));
    }

    #[test]
    fn published_channels_fail_the_row() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(write_inner(
            dir.path(),
            r#"{"published_channels":["crates.io"],"release_cut":false,"status":"pass"}"#,
        ));
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
        assert!(!receipt.fan_in_green());
    }

    #[test]
    fn release_cut_fails_the_row() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(write_inner(
            dir.path(),
            r#"{"published_channels":[],"release_cut":true,"status":"pass"}"#,
        ));
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
    }

    #[test]
    fn cleanup_failure_is_not_hidden_by_inner_pass() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(green_inner(dir.path()));
        request.cleanup = CleanupDisposition::Failed;
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
        assert!(receipt.limitations.iter().any(|item| item.code == "cleanup_not_pass"));
    }

    #[test]
    fn inner_cleanup_failure_is_not_hidden_by_inner_pass() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(write_inner(
            dir.path(),
            r#"{"published_channels":[],"release_cut":false,"status":"pass","cleanup":"failed"}"#,
        ));
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
    }

    #[test]
    fn emulated_host_cannot_be_pass() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(green_inner(dir.path()));
        request.rustc_host = "x86_64-pc-windows-msvc".to_string();
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::NotProven);
        assert!(!receipt.native_host);
    }

    #[test]
    fn unadmitted_subject_fails() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.matrix_subject = "qemu-s390x".to_string();
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
        assert!(receipt.limitations.iter().any(|item| item.code == "unadmitted_matrix_subject"));
    }

    #[test]
    fn head_mismatch_fails() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.observed_git_head = "def456".to_string();
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::Failed);
        assert!(receipt.limitations.iter().any(|item| item.code == "head_mismatch"));
    }

    #[test]
    fn cancelled_producer_is_not_proven() {
        let dir = tempdir().expect("tempdir");
        let mut request = native_linux_request(dir.path());
        request.rehearsal_receipt = Some(green_inner(dir.path()));
        request.job_conclusion = JobConclusion::Cancelled;
        let receipt = compile_hosted_row(&request).expect("compile");
        assert_eq!(receipt.row_status, RowStatus::NotProven);
        assert!(!receipt.fan_in_green());
    }

    #[test]
    fn fan_in_missing_producer_is_non_green() {
        let dir = tempdir().expect("tempdir");
        let plan = admitted_plan();
        let fanin = compile_hosted_fanin(&plan, dir.path()).expect("fanin");
        assert_eq!(fanin.verdict, FaninVerdict::NonGreen);
        assert_eq!(fanin.rows.len(), 3);
        assert!(fanin.rows.iter().all(|row| row.outcome == "missing"));
    }

    #[test]
    fn fan_in_does_not_consume_another_attempt() {
        let dir = tempdir().expect("tempdir");
        let mut linux = native_linux_request(dir.path());
        linux.rehearsal_receipt = Some(green_inner(dir.path()));
        let mut stale = linux.clone();
        stale.attempt = 2;
        stale.out_dir = dir.path().join("stale");
        let current = compile_hosted_row(&linux).expect("current");
        let other_attempt = compile_hosted_row(&stale).expect("stale");
        let inbox = dir.path().join("inbox");
        write_row_receipt(&inbox.join("linux"), &current).expect("write current");
        write_row_receipt(&inbox.join("stale"), &other_attempt).expect("write stale");
        write_windows_and_macos_pass(&inbox, "44", 1, "abc123");

        let fanin = compile_hosted_fanin(&admitted_plan(), &inbox).expect("fanin");
        assert_eq!(fanin.verdict, FaninVerdict::NonGreen);
        assert!(fanin.collisions.iter().any(|item| item.contains("attempt")));
    }

    #[test]
    fn fan_in_rejects_cross_row_artifact_id() {
        let dir = tempdir().expect("tempdir");
        let mut linux = native_linux_request(dir.path());
        linux.rehearsal_receipt = Some(green_inner(dir.path()));
        let mut stolen = compile_hosted_row(&linux).expect("linux");
        stolen.matrix_subject = "windows-latest".to_string();
        // Keep linux artifact_id so Windows appears to reuse the Linux artifact.
        let inbox = dir.path().join("inbox");
        write_row_receipt(&inbox.join("linux"), &compile_hosted_row(&linux).expect("linux"))
            .expect("write linux");
        write_row_receipt(&inbox.join("windows"), &stolen).expect("write stolen");
        write_macos_pass(&inbox, "44", 1, "abc123");

        let fanin = compile_hosted_fanin(&admitted_plan(), &inbox).expect("fanin");
        assert_eq!(fanin.verdict, FaninVerdict::NonGreen);
        assert!(fanin.collisions.iter().any(|item| item.contains("artifact_id")));
    }

    #[test]
    fn fan_in_rejects_unadmitted_extra_row() {
        let dir = tempdir().expect("tempdir");
        let inbox = dir.path().join("inbox");
        write_windows_and_macos_pass(&inbox, "44", 1, "abc123");
        let mut linux = native_linux_request(dir.path());
        linux.rehearsal_receipt = Some(green_inner(dir.path()));
        write_row_receipt(&inbox.join("linux"), &compile_hosted_row(&linux).expect("linux"))
            .expect("linux");
        let extra = HostedRowReceipt {
            schema: HOSTED_ROW_SCHEMA.to_string(),
            repository_sha: "abc123".to_string(),
            run_id: "44".to_string(),
            run_attempt: 1,
            matrix_subject: "qemu-s390x".to_string(),
            artifact_id: "44-1-qemu-s390x".to_string(),
            runner_os: "Linux".to_string(),
            rustc_host: "s390x-unknown-linux-gnu".to_string(),
            lockfile_digest: "lock".to_string(),
            job_conclusion: JobConclusion::Success,
            row_status: RowStatus::Pass,
            cleanup: CleanupDisposition::Pass,
            published_channels: Vec::new(),
            release_cut: false,
            native_host: true,
            rehearsal_receipt_digest: None,
            rehearsal_status: Some("pass".to_string()),
            limitations: Vec::new(),
        };
        write_row_receipt(&inbox.join("extra"), &extra).expect("extra");
        let fanin = compile_hosted_fanin(&admitted_plan(), &inbox).expect("fanin");
        assert_eq!(fanin.verdict, FaninVerdict::NonGreen);
        assert_eq!(fanin.unexpected_subjects, vec!["qemu-s390x".to_string()]);
    }

    #[test]
    fn fan_in_all_admitted_pass_rows_are_green() {
        let dir = tempdir().expect("tempdir");
        let inbox = dir.path().join("inbox");
        let mut linux = native_linux_request(dir.path());
        linux.rehearsal_receipt = Some(green_inner(dir.path()));
        write_row_receipt(&inbox.join("linux"), &compile_hosted_row(&linux).expect("linux"))
            .expect("linux");
        write_windows_and_macos_pass(&inbox, "44", 1, "abc123");
        let fanin = compile_hosted_fanin(&admitted_plan(), &inbox).expect("fanin");
        assert_eq!(fanin.verdict, FaninVerdict::Green, "{fanin:?}");
    }

    fn write_windows_and_macos_pass(inbox: &Path, run_id: &str, attempt: u32, sha: &str) {
        write_native_pass(
            inbox,
            "windows-latest",
            "Windows",
            "x86_64-pc-windows-msvc",
            run_id,
            attempt,
            sha,
        );
        write_macos_pass(inbox, run_id, attempt, sha);
    }

    fn write_macos_pass(inbox: &Path, run_id: &str, attempt: u32, sha: &str) {
        write_native_pass(
            inbox,
            "macos-latest",
            "macOS",
            "aarch64-apple-darwin",
            run_id,
            attempt,
            sha,
        );
    }

    fn write_native_pass(
        inbox: &Path,
        subject: &str,
        runner_os: &str,
        rustc_host: &str,
        run_id: &str,
        attempt: u32,
        sha: &str,
    ) {
        let receipt = HostedRowReceipt {
            schema: HOSTED_ROW_SCHEMA.to_string(),
            repository_sha: sha.to_string(),
            run_id: run_id.to_string(),
            run_attempt: attempt,
            matrix_subject: subject.to_string(),
            artifact_id: artifact_id(run_id, attempt, subject),
            runner_os: runner_os.to_string(),
            rustc_host: rustc_host.to_string(),
            lockfile_digest: "lock".to_string(),
            job_conclusion: JobConclusion::Success,
            row_status: RowStatus::Pass,
            cleanup: CleanupDisposition::Pass,
            published_channels: Vec::new(),
            release_cut: false,
            native_host: true,
            rehearsal_receipt_digest: Some("abc".to_string()),
            rehearsal_status: Some("pass".to_string()),
            limitations: Vec::new(),
        };
        write_row_receipt(&inbox.join(subject), &receipt).expect("write native pass");
    }

    impl HostedRowReceipt {
        fn owning_missing_issue(&self) -> Option<u64> {
            self.limitations
                .iter()
                .find(|item| item.code == "missing_rehearsal_receipt")
                .map(|item| item.owning_issue)
        }
    }
}
