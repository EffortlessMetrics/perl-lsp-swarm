use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::project_root;
use perl_lsp_rs_core::hashing::fnv1a64_hex;

const SCHEMA_VERSION: u32 = 1;
const CHECK_NAME: &str = "merge-readiness";
const DEFAULT_RECEIPT_PATH: &str = "target/receipts/merge-readiness.json";
const REQUIRED_CHECKS_PATH: &str = ".ci/policies/required-checks.toml";
const FAN_IN_SCHEMA_VERSION: u32 = 1;
const FAN_IN_CHECK_NAME: &str = "merge-readiness-fan-in";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReadinessReceipt {
    pub check: String,
    pub schema_version: u32,
    pub event: String,
    pub pr: u64,
    pub head_sha: String,
    pub base_sha: String,
    pub gate_graph_version: String,
    pub required_checks: Vec<String>,
    pub review_evidence: Vec<String>,
    pub blocker_labels_absent: bool,
    pub verdict: String,
    pub expires_when: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyStatus {
    Valid,
    StaleHead,
    StaleBase,
    StaleGateGraph,
    Blocked,
    NotProven,
    Missing,
}

/// Result class reported by an upstream exact-head evidence producer.
///
/// These values intentionally preserve instrument state instead of collapsing
/// every non-success into a product failure. The fan-in is read-only and must
/// never turn a skipped, cancelled, stale, or instrument-failed input into
/// merge authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceClass {
    Success,
    PolicyFinding,
    NotProven,
    DraftSkip,
    Cancelled,
    NotApplicable,
    Stale,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredCheckEvidence {
    pub name: String,
    pub evaluated_sha: String,
    pub result: EvidenceClass,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewConvergenceEvidence {
    pub evaluated_sha: String,
    pub result: EvidenceClass,
    pub converged: bool,
    pub unresolved_conversations: u32,
    pub evidenced_dispositions: bool,
    pub required_review_in_flight: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangelogEvidence {
    pub evaluated_sha: String,
    pub result: EvidenceClass,
    pub disposition: Option<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtectionEvidence {
    pub evaluated_sha: String,
    #[serde(default)]
    pub evaluated_merge_group_sha: Option<String>,
    pub result: EvidenceClass,
    pub merge_permitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeReadinessSnapshot {
    pub schema_version: u32,
    pub repository: String,
    pub pr: u64,
    pub base_sha: String,
    pub head_sha: String,
    pub merge_group_sha: Option<String>,
    pub draft: bool,
    pub required_check_names: Vec<String>,
    pub checks: Vec<RequiredCheckEvidence>,
    pub review: ReviewConvergenceEvidence,
    pub changelog: ChangelogEvidence,
    pub protection: ProtectionEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeReadinessStatus {
    Ready,
    Blocked,
    Pending,
    NotProven,
    Stale,
    DraftSkip,
    Cancelled,
    NotApplicable,
}

impl MergeReadinessStatus {
    /// Lowercase verdict string written into the merge-readiness receipt.
    ///
    /// `Ready` maps to `"valid"`; every other status maps to its
    /// snake_case name so a non-ready fan-in never produces a `valid`
    /// receipt (the #4649 false-confidence defect).
    fn as_verdict(self) -> String {
        match self {
            Self::Ready => "valid".to_string(),
            Self::Blocked => "blocked".to_string(),
            Self::Pending => "pending".to_string(),
            Self::NotProven => "not_proven".to_string(),
            Self::Stale => "stale".to_string(),
            Self::DraftSkip => "draft_skip".to_string(),
            Self::Cancelled => "cancelled".to_string(),
            Self::NotApplicable => "not_applicable".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeReadinessFinding {
    pub source: String,
    pub class: EvidenceClass,
    pub blocking: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeReadinessEvaluation {
    pub check: String,
    pub schema_version: u32,
    pub repository: String,
    pub pr: u64,
    pub base_sha: String,
    pub head_sha: String,
    pub merge_group_sha: Option<String>,
    pub status: MergeReadinessStatus,
    pub findings: Vec<MergeReadinessFinding>,
}

impl VerifyStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::StaleHead => "stale_head",
            Self::StaleBase => "stale_base",
            Self::StaleGateGraph => "stale_gate_graph",
            Self::Blocked => "blocked",
            Self::NotProven => "not_proven",
            Self::Missing => "missing",
        }
    }
}

/// Evaluate a caller-supplied live snapshot without querying GitHub or
/// changing repository/PR state. This is the M1 seam: callers own collection
/// of GitHub facts, while this function owns one deterministic fan-in rule.
pub fn evaluate_snapshot_file(snapshot_path: &Path, output_path: Option<&Path>) -> Result<()> {
    let raw = fs::read_to_string(snapshot_path)
        .with_context(|| format!("failed to read snapshot: {}", snapshot_path.display()))?;
    let snapshot: MergeReadinessSnapshot = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse snapshot: {}", snapshot_path.display()))?;
    let evaluation = evaluate_snapshot(&snapshot)?;
    let json =
        serde_json::to_string_pretty(&evaluation).context("failed to serialize evaluation")?;

    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory: {}", parent.display()))?;
            }
        fs::write(path, json)
            .with_context(|| format!("failed to write evaluation: {}", path.display()))?;
        println!("wrote {}", path.display());
    } else {
        println!("{json}");
    }

    Ok(())
}

fn validate_object_id(field: &str, value: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("merge-readiness snapshot {field} must be a full 40-character object ID");
    }
    Ok(())
}

pub fn evaluate_snapshot(snapshot: &MergeReadinessSnapshot) -> Result<MergeReadinessEvaluation> {
    if snapshot.schema_version != FAN_IN_SCHEMA_VERSION {
        bail!(
            "unsupported merge-readiness snapshot schema: {} (expected {})",
            snapshot.schema_version,
            FAN_IN_SCHEMA_VERSION
        );
    }
    if snapshot.repository.trim().is_empty() {
        bail!("merge-readiness snapshot repository is empty");
    }
    if snapshot.pr == 0 {
        bail!("merge-readiness snapshot PR number must be positive");
    }
    validate_object_id("base_sha", &snapshot.base_sha)?;
    validate_object_id("head_sha", &snapshot.head_sha)?;
    if let Some(merge_group_sha) = snapshot.merge_group_sha.as_deref() {
        validate_object_id("merge_group_sha", merge_group_sha)?;
    }
    if snapshot.required_check_names.is_empty() {
        bail!("merge-readiness snapshot has no required checks");
    }
    let mut required_names = BTreeMap::new();
    for name in &snapshot.required_check_names {
        if name.trim().is_empty() {
            bail!("merge-readiness snapshot contains a blank required check name");
        }
        if required_names.insert(name, ()).is_some() {
            bail!("merge-readiness snapshot repeats required check name: {name}");
        }
    }
    for check in &snapshot.checks {
        if check.name.trim().is_empty() {
            bail!("merge-readiness snapshot contains a blank check name");
        }
        validate_object_id("checks[].evaluated_sha", &check.evaluated_sha)?;
    }
    validate_object_id("review.evaluated_sha", &snapshot.review.evaluated_sha)?;
    validate_object_id("changelog.evaluated_sha", &snapshot.changelog.evaluated_sha)?;
    validate_object_id("protection.evaluated_sha", &snapshot.protection.evaluated_sha)?;
    if let Some(merge_group_sha) = snapshot.protection.evaluated_merge_group_sha.as_deref() {
        validate_object_id("protection.evaluated_merge_group_sha", merge_group_sha)?;
    }

    let mut findings = Vec::new();
    if snapshot.draft {
        findings.push(MergeReadinessFinding {
            source: "pull_request".to_string(),
            class: EvidenceClass::DraftSkip,
            blocking: true,
            detail: "draft pull requests are not evaluated for merge authorization".to_string(),
        });
    } else {
        let mut checks_by_name: BTreeMap<&str, Vec<&RequiredCheckEvidence>> = BTreeMap::new();
        for check in &snapshot.checks {
            checks_by_name.entry(&check.name).or_default().push(check);
        }

        for required_name in &snapshot.required_check_names {
            match checks_by_name.get(required_name.as_str()).map(Vec::as_slice) {
                None | Some([]) => findings.push(MergeReadinessFinding {
                    source: format!("required_check:{required_name}"),
                    class: EvidenceClass::NotProven,
                    blocking: true,
                    detail: "required check is missing from the current check-run snapshot"
                        .to_string(),
                }),
                Some([_, _, ..]) => findings.push(MergeReadinessFinding {
                    source: format!("required_check:{required_name}"),
                    class: EvidenceClass::NotProven,
                    blocking: true,
                    detail: "required check appears more than once in the snapshot".to_string(),
                }),
                Some([check]) if check.evaluated_sha != snapshot.head_sha => {
                    findings.push(MergeReadinessFinding {
                        source: format!("required_check:{required_name}"),
                        class: EvidenceClass::Stale,
                        blocking: true,
                        detail: format!(
                            "check evaluated {} but current PR head is {}",
                            check.evaluated_sha, snapshot.head_sha
                        ),
                    });
                }
                Some([check]) if check.result != EvidenceClass::Success => {
                    findings.push(MergeReadinessFinding {
                        source: format!("required_check:{required_name}"),
                        class: check.result,
                        blocking: true,
                        detail: "required check did not produce exact-head success".to_string(),
                    });
                }
                Some([_]) => {}
            }
        }

        evaluate_review(&snapshot.review, &snapshot.head_sha, &mut findings);
        evaluate_changelog(&snapshot.changelog, &snapshot.head_sha, &mut findings);
        evaluate_protection(
            &snapshot.protection,
            &snapshot.head_sha,
            snapshot.merge_group_sha.as_deref(),
            &mut findings,
        );
    }

    let status = status_from_findings(&findings);
    Ok(MergeReadinessEvaluation {
        check: FAN_IN_CHECK_NAME.to_string(),
        schema_version: FAN_IN_SCHEMA_VERSION,
        repository: snapshot.repository.clone(),
        pr: snapshot.pr,
        base_sha: snapshot.base_sha.clone(),
        head_sha: snapshot.head_sha.clone(),
        merge_group_sha: snapshot.merge_group_sha.clone(),
        status,
        findings,
    })
}

fn evaluate_review(
    review: &ReviewConvergenceEvidence,
    current_head: &str,
    findings: &mut Vec<MergeReadinessFinding>,
) {
    if review.evaluated_sha != current_head {
        findings.push(MergeReadinessFinding {
            source: "review_convergence".to_string(),
            class: EvidenceClass::Stale,
            blocking: true,
            detail: format!(
                "review convergence evaluated {} but current PR head is {}",
                review.evaluated_sha, current_head
            ),
        });
        return;
    }
    if review.result != EvidenceClass::Success {
        findings.push(MergeReadinessFinding {
            source: "review_convergence".to_string(),
            class: review.result,
            blocking: true,
            detail: "#3693 review convergence did not succeed".to_string(),
        });
    }
    if !review.converged {
        findings.push(MergeReadinessFinding {
            source: "review_convergence".to_string(),
            class: EvidenceClass::PolicyFinding,
            blocking: true,
            detail: "review convergence reports a non-converged current head".to_string(),
        });
    }
    if review.unresolved_conversations != 0 {
        findings.push(MergeReadinessFinding {
            source: "review_convergence".to_string(),
            class: EvidenceClass::PolicyFinding,
            blocking: true,
            detail: format!(
                "{} unresolved conversation(s) remain, including outdated threads",
                review.unresolved_conversations
            ),
        });
    }
    if !review.evidenced_dispositions {
        findings.push(MergeReadinessFinding {
            source: "review_convergence".to_string(),
            class: EvidenceClass::PolicyFinding,
            blocking: true,
            detail: "review dispositions are not evidenced".to_string(),
        });
    }
    if review.required_review_in_flight {
        findings.push(MergeReadinessFinding {
            source: "review_convergence".to_string(),
            class: EvidenceClass::Pending,
            blocking: true,
            detail: "a required review is still in flight".to_string(),
        });
    }
}

fn evaluate_changelog(
    changelog: &ChangelogEvidence,
    current_head: &str,
    findings: &mut Vec<MergeReadinessFinding>,
) {
    if changelog.evaluated_sha != current_head {
        findings.push(MergeReadinessFinding {
            source: "changelog".to_string(),
            class: EvidenceClass::Stale,
            blocking: true,
            detail: format!(
                "Changie disposition evaluated {} but current PR head is {}",
                changelog.evaluated_sha, current_head
            ),
        });
    } else if changelog.result != EvidenceClass::Success {
        findings.push(MergeReadinessFinding {
            source: "changelog".to_string(),
            class: changelog.result,
            blocking: changelog.result != EvidenceClass::PolicyFinding || changelog.blocking,
            detail: "Changie disposition is not current-head success".to_string(),
        });
    } else if changelog
        .disposition
        .as_deref()
        .is_none_or(|disposition| disposition.trim().is_empty())
    {
        findings.push(MergeReadinessFinding {
            source: "changelog".to_string(),
            class: EvidenceClass::NotProven,
            blocking: true,
            detail: "Changie evidence succeeded without a disposition".to_string(),
        });
    }
}

fn evaluate_protection(
    protection: &ProtectionEvidence,
    current_head: &str,
    current_merge_group: Option<&str>,
    findings: &mut Vec<MergeReadinessFinding>,
) {
    if protection.evaluated_sha != current_head {
        findings.push(MergeReadinessFinding {
            source: "protection".to_string(),
            class: EvidenceClass::Stale,
            blocking: true,
            detail: format!(
                "ruleset/merge permission evaluated {} but current PR head is {}",
                protection.evaluated_sha, current_head
            ),
        });
    } else {
        if let Some(expected_merge_group) = current_merge_group {
            match protection.evaluated_merge_group_sha.as_deref() {
                None => findings.push(MergeReadinessFinding {
                    source: "protection".to_string(),
                    class: EvidenceClass::NotProven,
                    blocking: true,
                    detail:
                        "merge-group SHA is present but protection evidence did not evaluate it"
                            .to_string(),
                }),
                Some(actual_merge_group) if actual_merge_group != expected_merge_group => {
                    findings.push(MergeReadinessFinding {
                        source: "protection".to_string(),
                        class: EvidenceClass::Stale,
                        blocking: true,
                        detail: format!(
                            "protection evaluated merge group {} but current merge group is {}",
                            actual_merge_group, expected_merge_group
                        ),
                    });
                }
                Some(_) => {}
            }
        } else if let Some(actual_merge_group) = protection.evaluated_merge_group_sha.as_deref() {
            findings.push(MergeReadinessFinding {
                source: "protection".to_string(),
                class: EvidenceClass::Stale,
                blocking: true,
                detail: format!(
                    "protection evidence evaluated unexpected merge group {} while snapshot has no merge group",
                    actual_merge_group
                ),
            });
        }

        if protection.result != EvidenceClass::Success {
            findings.push(MergeReadinessFinding {
                source: "protection".to_string(),
                class: protection.result,
                blocking: true,
                detail: "live protected integration state is not proven".to_string(),
            });
        } else if !protection.merge_permitted {
            findings.push(MergeReadinessFinding {
                source: "protection".to_string(),
                class: EvidenceClass::PolicyFinding,
                blocking: true,
                detail: "branch protection or merge-queue policy does not permit integration"
                    .to_string(),
            });
        }
    }
}

fn status_from_findings(findings: &[MergeReadinessFinding]) -> MergeReadinessStatus {
    let class_order = [
        (EvidenceClass::Stale, MergeReadinessStatus::Stale),
        (EvidenceClass::NotProven, MergeReadinessStatus::NotProven),
        (EvidenceClass::Cancelled, MergeReadinessStatus::Cancelled),
        (EvidenceClass::Pending, MergeReadinessStatus::Pending),
        (EvidenceClass::NotApplicable, MergeReadinessStatus::NotApplicable),
        (EvidenceClass::DraftSkip, MergeReadinessStatus::DraftSkip),
        (EvidenceClass::PolicyFinding, MergeReadinessStatus::Blocked),
    ];

    for (class, status) in class_order {
        if findings.iter().any(|finding| finding.blocking && finding.class == class) {
            return status;
        }
    }

    MergeReadinessStatus::Ready
}

/// Emit a merge-readiness receipt for a PR.
///
/// Without `--snapshot` this command cannot evaluate fan-in evidence (CI,
/// review convergence, changelog, branch protection) — it only collects
/// locally-derivable facts (head/base SHA, gate-graph version, required-check
/// inventory). To avoid the #4649 false-confidence defect it stamps
/// `verdict: "not_proven"` and `blocker_labels_absent: false` in that case,
/// which `verify` refuses to collapse to `valid`.
///
/// Pass `--snapshot <path>` to supply a live current-head fan-in snapshot; the
/// verdict is then derived from the real `evaluate_snapshot` fan-in so the
/// receipt only says `valid` when every required check, review, changelog, and
/// protection input has exact-head success evidence.
pub fn emit(pr: u64, receipt_path: Option<PathBuf>, snapshot_path: Option<PathBuf>) -> Result<()> {
    let root = project_root()?;
    let required_checks = load_required_checks(&root)?;
    let head_sha = git_output(&root, &["rev-parse", "HEAD"])?;
    let base_sha = resolve_base_sha(&root)?;
    let gate_graph_version = compute_gate_graph_version(&root, &required_checks)?;

    let (verdict, blocker_labels_absent, review_evidence) = match snapshot_path.as_deref() {
        Some(snapshot) => {
            let raw = fs::read_to_string(snapshot)
                .with_context(|| format!("failed to read snapshot: {}", snapshot.display()))?;
            let snap: MergeReadinessSnapshot = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse snapshot: {}", snapshot.display()))?;
            let evaluation = evaluate_snapshot(&snap)?;
            let blocking = evaluation
                .findings
                .iter()
                .filter(|finding| finding.blocking)
                .map(|finding| finding.detail.clone())
                .collect::<Vec<_>>();
            let is_ready = evaluation.status == MergeReadinessStatus::Ready;
            let verdict = if required_checks.is_empty() {
                "blocked".to_string()
            } else if is_ready {
                "valid".to_string()
            } else {
                evaluation.status.as_verdict()
            };
            let blocker_labels_absent = blocking.is_empty() && is_ready;
            let review_evidence = if is_ready {
                vec!["reviewed-deep".to_string(), "ci-green".to_string()]
            } else if blocking.is_empty() {
                vec![format!("fan-in status: {}", evaluation.status.as_verdict())]
            } else {
                blocking
            };
            (verdict, blocker_labels_absent, review_evidence)
        }
        None => {
            // No fan-in evidence was evaluated. Stamp an honest not-proven
            // verdict so `verify` cannot fabricate a green from this receipt.
            eprintln!(
                "merge-ready emit: no --snapshot supplied; fan-in evidence was NOT evaluated. \
                 Receipt stamped 'not_proven' — run `merge-ready evaluate` with a live snapshot \
                 to produce a 'valid' receipt."
            );
            ("not_proven".to_string(), false, vec!["no fan-in evidence evaluated".to_string()])
        }
    };

    let receipt = MergeReadinessReceipt {
        check: CHECK_NAME.to_string(),
        schema_version: SCHEMA_VERSION,
        event: "pull_request".to_string(),
        pr,
        head_sha,
        base_sha,
        gate_graph_version,
        required_checks,
        review_evidence,
        blocker_labels_absent,
        verdict,
        expires_when: "on_new_commit_or_base_or_policy_change".to_string(),
    };

    let output_path = receipt_path.unwrap_or_else(|| root.join(DEFAULT_RECEIPT_PATH));
    write_receipt(&output_path, &receipt)?;
    println!("wrote {}", output_path.display());

    Ok(())
}

pub fn verify(pr: Option<u64>, fixture: Option<PathBuf>) -> Result<()> {
    let root = project_root()?;
    let path = if let Some(fixture_path) = fixture {
        fixture_path
    } else {
        let _ = pr;
        root.join(DEFAULT_RECEIPT_PATH)
    };

    if !path.exists() {
        println!("{}", VerifyStatus::Missing.as_str());
        bail!("receipt not found: {}", path.display());
    }

    let receipt = load_receipt(&path)?;
    let required_checks = load_required_checks(&root)?;
    let current_head = git_output(&root, &["rev-parse", "HEAD"])?;
    let current_base = resolve_base_sha(&root)?;
    let current_gate_graph = compute_gate_graph_version(&root, &required_checks)?;

    let status = evaluate_receipt(&receipt, &current_head, &current_base, &current_gate_graph);
    println!("{}", status.as_str());

    if status == VerifyStatus::Valid {
        Ok(())
    } else {
        bail!("receipt status: {}", status.as_str())
    }
}

fn evaluate_receipt(
    receipt: &MergeReadinessReceipt,
    current_head: &str,
    current_base: &str,
    current_gate_graph: &str,
) -> VerifyStatus {
    if receipt.verdict == "blocked" || !receipt.blocker_labels_absent {
        return VerifyStatus::Blocked;
    }

    // A receipt stamped `not_proven` was emitted without evaluating fan-in
    // evidence (see `emit`). It must never collapse to `valid` even when the
    // head/base/gate-graph SHAs happen to match — that is the false-confidence
    // defect from #4649. Treat it as not-proven regardless of staleness.
    if receipt.verdict == "not_proven" {
        return VerifyStatus::NotProven;
    }

    let receipt_head =
        resolve_runtime_token(&receipt.head_sha, current_head, current_base, current_gate_graph);
    let receipt_base =
        resolve_runtime_token(&receipt.base_sha, current_head, current_base, current_gate_graph);
    let receipt_gate = resolve_runtime_token(
        &receipt.gate_graph_version,
        current_head,
        current_base,
        current_gate_graph,
    );

    if receipt_head != current_head {
        return VerifyStatus::StaleHead;
    }

    if receipt_base != current_base {
        return VerifyStatus::StaleBase;
    }

    if receipt_gate != current_gate_graph {
        return VerifyStatus::StaleGateGraph;
    }

    VerifyStatus::Valid
}

fn resolve_runtime_token(
    value: &str,
    current_head: &str,
    current_base: &str,
    current_gate: &str,
) -> String {
    match value {
        "$CURRENT_HEAD" => current_head.to_string(),
        "$CURRENT_BASE" => current_base.to_string(),
        "$CURRENT_GATE_GRAPH" => current_gate.to_string(),
        _ => value.to_string(),
    }
}

fn load_receipt(path: &Path) -> Result<MergeReadinessReceipt> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read receipt: {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse receipt: {}", path.display()))
}

fn write_receipt(path: &Path, receipt: &MergeReadinessReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(receipt).context("failed to serialize receipt")?;
    fs::write(path, json).with_context(|| format!("failed to write receipt: {}", path.display()))
}

fn load_required_checks(root: &Path) -> Result<Vec<String>> {
    let path = root.join(REQUIRED_CHECKS_PATH);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read required checks policy: {}", path.display()))?;
    let value: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("failed to parse required checks policy: {}", path.display()))?;

    Ok(required_check_names_from_policy(&value))
}

fn required_check_names_from_policy(value: &toml::Value) -> Vec<String> {
    let mut checks = Vec::new();

    // `[[checks]]` is the merge-ready / branch-protection status-context
    // inventory — the primary source for required check names.
    if let Some(array) = value.get("checks").and_then(toml::Value::as_array) {
        for item in array {
            if item.get("required").and_then(toml::Value::as_bool) == Some(true)
                && let Some(name) = item.get("name").and_then(toml::Value::as_str)
            {
                checks.push(name.to_string());
            }
        }
    }

    // `[[check]]` (singular) is consumed by workflow-trigger-lint for required-
    // style workflow shape. Per #4649 the previous parser silently dropped
    // these, so a maintainer moving a required check from `[[checks]]` to
    // `[[check]]` would silently lose merge-ready coverage. We now union any
    // `required = true` `[[check]]` entries and log a note about the schema
    // split so the inclusion is visible rather than silent.
    let singular_required: Vec<String> = value
        .get("check")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("required").and_then(toml::Value::as_bool) == Some(true))
        .filter_map(|item| item.get("name").and_then(toml::Value::as_str).map(String::from))
        .collect();

    if !singular_required.is_empty() {
        eprintln!(
            "merge-ready: unioning {} required [[check]] entry/ies into the status-context \
             inventory (schema split: [[check]] = workflow-trigger-lint shape, [[checks]] = \
             branch-protection contexts): {}",
            singular_required.len(),
            singular_required.join(", ")
        );
        checks.extend(singular_required);
    }

    checks.sort_unstable();
    checks.dedup();
    checks
}

fn resolve_base_sha(root: &Path) -> Result<String> {
    for base_ref in ["origin/master", "origin/main", "master", "main"] {
        if git_output(root, &["rev-parse", "--verify", base_ref]).is_ok() {
            return git_output(root, &["merge-base", "HEAD", base_ref]);
        }
    }

    git_output(root, &["rev-parse", "HEAD"])
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

fn compute_gate_graph_version(root: &Path, required_checks: &[String]) -> Result<String> {
    let mut inputs: BTreeMap<String, String> = BTreeMap::new();

    for rel in collect_gate_files(root)? {
        let path = root.join(&rel);
        if path.is_file() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read gate graph input: {}", path.display()))?;
            inputs.insert(rel, content.replace("\r\n", "\n"));
        }
    }

    inputs.insert(
        "required_checks".to_string(),
        serde_json::to_string(required_checks).context("failed to encode required checks")?,
    );

    let mut material = String::new();
    for (path, content) in inputs {
        material.push_str("## ");
        material.push_str(&path);
        material.push('\n');
        material.push_str(&content);
        material.push('\n');
    }

    Ok(fnv1a64_hex(material.as_bytes()))
}

fn collect_gate_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();

    for rel in
        [".ci/policies/required-checks.toml", ".ci/policies", ".ci/gates.d", ".github/workflows"]
    {
        let dir = root.join(rel);
        if dir.is_file() {
            files.push(rel.to_string());
            continue;
        }

        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_file())
        {
            let path = entry.path();
            let rel_path = path
                .strip_prefix(root)
                .context("failed to strip repository root")?
                .to_string_lossy()
                .to_string();

            if rel == ".github/workflows" && !is_required_workflow_candidate(path) {
                continue;
            }

            files.push(rel_path);
        }
    }

    files.sort_unstable();
    files.dedup();
    Ok(files)
}

fn is_required_workflow_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    name.contains("ci") || name.contains("gate") || name.contains("merge")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must;

    fn make_receipt(
        head_sha: &str,
        base_sha: &str,
        gate_graph_version: &str,
        verdict: &str,
        blocker_labels_absent: bool,
    ) -> MergeReadinessReceipt {
        MergeReadinessReceipt {
            check: CHECK_NAME.to_string(),
            schema_version: SCHEMA_VERSION,
            event: "pull_request".to_string(),
            pr: 1,
            head_sha: head_sha.to_string(),
            base_sha: base_sha.to_string(),
            gate_graph_version: gate_graph_version.to_string(),
            required_checks: vec!["build".to_string()],
            review_evidence: vec!["reviewed-deep".to_string()],
            blocker_labels_absent,
            verdict: verdict.to_string(),
            expires_when: "on_new_commit_or_base_or_policy_change".to_string(),
        }
    }

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
    const GATE_V1: &str = "fnv1a64:0000000000000001";
    const GATE_V2: &str = "fnv1a64:0000000000000002";

    #[test]
    fn test_verify_returns_valid_for_current_receipt() {
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Valid);
    }

    #[test]
    fn test_verify_returns_stale_head() {
        // Receipt was emitted against SHA_A, but current head is SHA_C
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_C, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::StaleHead);
    }

    #[test]
    fn test_verify_returns_stale_base() {
        // Receipt base matches SHA_B, but master has advanced to SHA_C
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_C, GATE_V1);
        assert_eq!(status, VerifyStatus::StaleBase);
    }

    #[test]
    fn test_verify_returns_stale_gate_graph() {
        // Gate policy changed: GATE_V1 receipt vs GATE_V2 current
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V2);
        assert_eq!(status, VerifyStatus::StaleGateGraph);
    }

    #[test]
    fn test_verify_returns_blocked_when_needs_label_present() {
        // blocker_labels_absent = false indicates a needs-* label is set
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", false);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn test_verify_returns_blocked_when_verdict_is_blocked() {
        // verdict = "blocked" takes priority even if all SHAs match
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "blocked", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn test_verify_returns_missing_when_no_receipt_file() -> color_eyre::eyre::Result<()> {
        // Write receipt to a temp file, then delete it and pass the path to verify()
        let tmp = tempfile::NamedTempFile::new()?;
        let path = tmp.path().to_path_buf();
        // Drop the file so it no longer exists on disk
        drop(tmp);

        // verify() should output "missing" and bail
        let result = verify(None, Some(path));
        assert!(result.is_err(), "verify should return Err for missing receipt");
        Ok(())
    }

    #[test]
    fn test_verify_status_as_str_covers_all_variants() {
        assert_eq!(VerifyStatus::Valid.as_str(), "valid");
        assert_eq!(VerifyStatus::StaleHead.as_str(), "stale_head");
        assert_eq!(VerifyStatus::StaleBase.as_str(), "stale_base");
        assert_eq!(VerifyStatus::StaleGateGraph.as_str(), "stale_gate_graph");
        assert_eq!(VerifyStatus::Blocked.as_str(), "blocked");
        assert_eq!(VerifyStatus::NotProven.as_str(), "not_proven");
        assert_eq!(VerifyStatus::Missing.as_str(), "missing");
    }

    #[test]
    fn test_verify_returns_not_proven_for_not_proven_verdict_even_when_shas_match() {
        // The #4649 defect: a receipt stamped "not_proven" (emit without a
        // snapshot) must never collapse to "valid" even if head/base/gate all
        // match. verify() should report not_proven.
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "not_proven", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::NotProven);
    }

    #[test]
    fn test_verify_returns_not_proven_even_when_blocker_labels_claimed_absent() {
        // blocker_labels_absent = true must not rescue a not_proven verdict.
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "not_proven", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::NotProven);
    }

    #[test]
    fn test_evaluate_receipt_checks_blocked_before_staleness() {
        // If blocked, should return Blocked even if head/base are mismatched
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "blocked", true);
        // Different head and base to confirm Blocked is checked first
        let status = evaluate_receipt(&receipt, SHA_C, SHA_C, GATE_V2);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn test_resolve_runtime_token_substitutes_current_head() {
        let result = resolve_runtime_token("$CURRENT_HEAD", SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, SHA_A);
    }

    #[test]
    fn test_resolve_runtime_token_substitutes_current_base() {
        let result = resolve_runtime_token("$CURRENT_BASE", SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, SHA_B);
    }

    #[test]
    fn test_resolve_runtime_token_substitutes_gate_graph() {
        let result = resolve_runtime_token("$CURRENT_GATE_GRAPH", SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, GATE_V1);
    }

    #[test]
    fn test_resolve_runtime_token_returns_literal_for_unknown_token() {
        let literal = "abc1234def5678";
        let result = resolve_runtime_token(literal, SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, literal);
    }

    #[test]
    fn test_write_and_load_receipt_round_trip() -> color_eyre::eyre::Result<()> {
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let tmp = tempfile::NamedTempFile::new()?;
        write_receipt(tmp.path(), &receipt)?;
        let loaded = load_receipt(tmp.path())?;
        assert_eq!(loaded.head_sha, SHA_A);
        assert_eq!(loaded.base_sha, SHA_B);
        assert_eq!(loaded.gate_graph_version, GATE_V1);
        assert_eq!(loaded.verdict, "valid");
        assert!(loaded.blocker_labels_absent);
        Ok(())
    }

    #[test]
    fn test_write_receipt_creates_parent_dirs() -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let nested_path = tmp_dir.path().join("nested").join("dirs").join("receipt.json");
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        write_receipt(&nested_path, &receipt)?;
        assert!(nested_path.exists());
        Ok(())
    }

    #[test]
    fn test_fnv1a64_hex_is_deterministic() {
        let h1 = fnv1a64_hex(b"hello");
        let h2 = fnv1a64_hex(b"hello");
        assert_eq!(h1, h2);
        assert!(h1.starts_with("fnv1a64:"));
    }

    #[test]
    fn test_fnv1a64_hex_differs_on_different_input() {
        let h1 = fnv1a64_hex(b"hello");
        let h2 = fnv1a64_hex(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_load_required_checks_reads_required_status_contexts_from_policy_file() {
        let tmp_dir = must(tempfile::tempdir());
        let policy_dir = tmp_dir.path().join(".ci").join("policies");
        must(fs::create_dir_all(&policy_dir));
        must(fs::write(
            policy_dir.join("required-checks.toml"),
            concat!(
                "[[check]]\n",
                "name = \"Workflow-shape lint only\"\n",
                "required = true\n",
                "\n",
                "[[checks]]\n",
                "name = \"Codecov / Patch 95\"\n",
                "required = true\n",
                "\n",
                "[[checks]]\n",
                "name = \"ripr+ New Gap Gate\"\n",
                "required = true\n",
            ),
        ));

        // #4649: required `[[check]]` entries are now unioned with `[[checks]]`
        // (with a logged note) instead of being silently dropped.
        let checks = must(load_required_checks(tmp_dir.path()));
        assert_eq!(
            checks,
            vec![
                "Codecov / Patch 95".to_string(),
                "Workflow-shape lint only".to_string(),
                "ripr+ New Gap Gate".to_string(),
            ]
        );
    }

    #[test]
    fn test_required_check_names_include_only_required_status_contexts() {
        let policy: toml::Value = must(toml::from_str(concat!(
            "[[check]]\n",
            "name = \"Workflow-shape lint only\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"Proof required\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"Missing required flag\"\n",
        )));

        // #4649: the singular `[[check]]` required entry is now unioned in.
        let checks = required_check_names_from_policy(&policy);
        assert_eq!(
            checks,
            vec!["Proof required".to_string(), "Workflow-shape lint only".to_string()]
        );
    }

    #[test]
    fn test_required_check_names_union_deduplicates_singular_and_plural_entries() {
        // When a name appears required in both `[[check]]` and `[[checks]]`,
        // the union must deduplicate rather than emit it twice.
        let policy: toml::Value = must(toml::from_str(concat!(
            "[[check]]\n",
            "name = \"ripr+ New Gap Gate\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"Codecov / Patch 95\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"ripr+ New Gap Gate\"\n",
            "required = true\n",
        )));

        let checks = required_check_names_from_policy(&policy);
        assert_eq!(checks, vec!["Codecov / Patch 95", "ripr+ New Gap Gate"]);
    }

    #[test]
    fn test_required_check_names_ignores_non_required_singular_entries() {
        // `[[check]]` entries without `required = true` must not be unioned.
        let policy: toml::Value = must(toml::from_str(concat!(
            "[[check]]\n",
            "name = \"Advisory shape lint\"\n",
            "required = false\n",
            "\n",
            "[[checks]]\n",
            "name = \"Proof required\"\n",
            "required = true\n",
        )));

        let checks = required_check_names_from_policy(&policy);
        assert_eq!(checks, vec!["Proof required"]);
    }

    #[test]
    fn test_required_workflow_candidate_matches_ci_gate_and_merge_names() {
        for candidate in ["ci.yml", "ci-nightly.yml", "quality-gate.yml", "merge-ready.yml"] {
            assert!(
                is_required_workflow_candidate(Path::new(candidate)),
                "{candidate} should be included in the merge-readiness gate graph"
            );
        }

        for candidate in ["docs.yml", "release.yml", "scorecard.yml"] {
            assert!(
                !is_required_workflow_candidate(Path::new(candidate)),
                "{candidate} should not be included in the merge-readiness gate graph"
            );
        }
    }

    #[test]
    fn test_collect_gate_files_returns_empty_when_gate_inputs_are_absent()
    -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let files = collect_gate_files(tmp_dir.path())?;
        assert!(files.is_empty());
        Ok(())
    }

    #[test]
    fn test_collect_gate_files_includes_policy_gate_and_required_workflows()
    -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let root = tmp_dir.path();

        fs::create_dir_all(root.join(".ci").join("policies"))?;
        fs::create_dir_all(root.join(".ci").join("gates.d"))?;
        fs::create_dir_all(root.join(".github").join("workflows"))?;
        fs::write(root.join(".ci").join("policies").join("required-checks.toml"), "checks = []")?;
        fs::write(root.join(".ci").join("gates.d").join("quality.toml"), "gate = true")?;
        fs::write(root.join(".github").join("workflows").join("ci.yml"), "name: ci")?;
        fs::write(root.join(".github").join("workflows").join("merge-ready.yml"), "name: merge")?;
        fs::write(root.join(".github").join("workflows").join("docs.yml"), "name: docs")?;

        let files = collect_gate_files(root)?;
        let normalized: Vec<String> =
            files.into_iter().map(|path| path.replace('\\', "/")).collect();

        assert!(normalized.contains(&".ci/policies/required-checks.toml".to_string()));
        assert!(normalized.contains(&".ci/gates.d/quality.toml".to_string()));
        assert!(normalized.contains(&".github/workflows/ci.yml".to_string()));
        assert!(normalized.contains(&".github/workflows/merge-ready.yml".to_string()));
        assert!(!normalized.contains(&".github/workflows/docs.yml".to_string()));

        Ok(())
    }

    #[test]
    fn test_compute_gate_graph_version_tracks_required_checks_and_file_content()
    -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let root = tmp_dir.path();

        fs::create_dir_all(root.join(".ci").join("policies"))?;
        fs::create_dir_all(root.join(".ci").join("gates.d"))?;
        fs::write(
            root.join(".ci").join("policies").join("required-checks.toml"),
            "[[checks]]\nname = \"Codecov / Patch 95\"\nrequired = true\n",
        )?;
        let gate_path = root.join(".ci").join("gates.d").join("quality.toml");
        fs::write(&gate_path, "mode = \"advisory\"\n")?;

        let baseline = compute_gate_graph_version(root, &["Codecov / Patch 95".to_string()])?;
        let changed_checks = compute_gate_graph_version(root, &["ripr+ New Gap Gate".to_string()])?;
        fs::write(&gate_path, "mode = \"enforce\"\n")?;
        let changed_file = compute_gate_graph_version(root, &["Codecov / Patch 95".to_string()])?;

        assert_ne!(baseline, changed_checks);
        assert_ne!(baseline, changed_file);

        Ok(())
    }

    fn fan_in_snapshot() -> MergeReadinessSnapshot {
        MergeReadinessSnapshot {
            schema_version: FAN_IN_SCHEMA_VERSION,
            repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
            pr: 3988,
            base_sha: SHA_B.to_string(),
            head_sha: SHA_A.to_string(),
            merge_group_sha: None,
            draft: false,
            required_check_names: vec!["rust".to_string(), "ripr".to_string()],
            checks: vec![
                RequiredCheckEvidence {
                    name: "rust".to_string(),
                    evaluated_sha: SHA_A.to_string(),
                    result: EvidenceClass::Success,
                },
                RequiredCheckEvidence {
                    name: "ripr".to_string(),
                    evaluated_sha: SHA_A.to_string(),
                    result: EvidenceClass::Success,
                },
            ],
            review: ReviewConvergenceEvidence {
                evaluated_sha: SHA_A.to_string(),
                result: EvidenceClass::Success,
                converged: true,
                unresolved_conversations: 0,
                evidenced_dispositions: true,
                required_review_in_flight: false,
            },
            changelog: ChangelogEvidence {
                evaluated_sha: SHA_A.to_string(),
                result: EvidenceClass::Success,
                disposition: Some("exemption: ci".to_string()),
                blocking: false,
            },
            protection: ProtectionEvidence {
                evaluated_sha: SHA_A.to_string(),
                evaluated_merge_group_sha: None,
                result: EvidenceClass::Success,
                merge_permitted: true,
            },
        }
    }

    #[test]
    fn fan_in_ready_requires_all_exact_head_inputs() -> color_eyre::eyre::Result<()> {
        let evaluation = evaluate_snapshot(&fan_in_snapshot())?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Ready);
        color_eyre::eyre::ensure!(evaluation.findings.is_empty());
        Ok(())
    }

    #[test]
    fn fan_in_missing_required_check_is_not_proven() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.checks.pop();
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::NotProven);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "required_check:ripr" && finding.class == EvidenceClass::NotProven
        }));
        Ok(())
    }

    #[test]
    fn fan_in_duplicate_required_check_is_not_proven() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.checks.push(snapshot.checks[0].clone());
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::NotProven);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "required_check:rust"
                && finding.class == EvidenceClass::NotProven
                && finding.detail.contains("more than once")
        }));
        Ok(())
    }

    #[test]
    fn fan_in_duplicate_required_name_is_rejected() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.required_check_names.push("rust".to_string());
        let result = evaluate_snapshot(&snapshot);
        color_eyre::eyre::ensure!(result.is_err());
        Ok(())
    }

    #[test]
    fn fan_in_blank_required_name_is_rejected() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.required_check_names[0] = "  ".to_string();
        let result = evaluate_snapshot(&snapshot);
        color_eyre::eyre::ensure!(result.is_err());
        Ok(())
    }

    #[test]
    fn fan_in_rejects_non_object_id_identity() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.head_sha = "matching-but-not-a-full-object-id".to_string();
        let result = evaluate_snapshot(&snapshot);
        color_eyre::eyre::ensure!(result.is_err());
        Ok(())
    }

    #[test]
    fn fan_in_older_head_success_is_stale() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.checks[0].evaluated_sha = SHA_C.to_string();
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Stale);
        Ok(())
    }

    #[test]
    fn fan_in_stale_precedes_missing_evidence() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.checks[0].evaluated_sha = SHA_C.to_string();
        snapshot.checks.pop();
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Stale);
        Ok(())
    }

    #[test]
    fn fan_in_merge_group_evidence_must_match() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.merge_group_sha = Some(SHA_C.to_string());
        snapshot.protection.evaluated_merge_group_sha = Some(SHA_B.to_string());
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Stale);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "protection" && finding.class == EvidenceClass::Stale
        }));
        Ok(())
    }

    #[test]
    fn fan_in_unexpected_merge_group_evidence_is_stale() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.protection.evaluated_merge_group_sha = Some(SHA_C.to_string());
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Stale);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "protection"
                && finding.class == EvidenceClass::Stale
                && finding.detail.contains(SHA_C)
        }));
        Ok(())
    }

    #[test]
    fn fan_in_review_in_flight_is_pending() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.review.required_review_in_flight = true;
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Pending);
        Ok(())
    }

    #[test]
    fn fan_in_preserves_non_success_check_classes() -> color_eyre::eyre::Result<()> {
        for (class, status) in [
            (EvidenceClass::NotProven, MergeReadinessStatus::NotProven),
            (EvidenceClass::Cancelled, MergeReadinessStatus::Cancelled),
            (EvidenceClass::NotApplicable, MergeReadinessStatus::NotApplicable),
            (EvidenceClass::Pending, MergeReadinessStatus::Pending),
            (EvidenceClass::PolicyFinding, MergeReadinessStatus::Blocked),
        ] {
            let mut snapshot = fan_in_snapshot();
            snapshot.checks[0].result = class;
            let evaluation = evaluate_snapshot(&snapshot)?;
            color_eyre::eyre::ensure!(evaluation.status == status);
            color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
                finding.source == "required_check:rust" && finding.class == class
            }));
        }
        Ok(())
    }

    #[test]
    fn fan_in_draft_is_explicitly_skipped() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.draft = true;
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::DraftSkip);
        color_eyre::eyre::ensure!(
            evaluation
                .findings
                .iter()
                .any(|finding| { finding.class == EvidenceClass::DraftSkip && finding.blocking })
        );
        Ok(())
    }

    #[test]
    fn fan_in_unresolved_conversations_block_even_when_outdated() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.review.unresolved_conversations = 1;
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Blocked);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "review_convergence"
                && finding.class == EvidenceClass::PolicyFinding
                && finding.detail.contains("outdated")
        }));
        Ok(())
    }

    #[test]
    fn fan_in_advisory_changelog_finding_remains_visible_without_blocking()
    -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.changelog.result = EvidenceClass::PolicyFinding;
        snapshot.changelog.disposition = None;
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Ready);
        color_eyre::eyre::ensure!(evaluation.findings.len() == 1);
        color_eyre::eyre::ensure!(!evaluation.findings[0].blocking);
        Ok(())
    }

    #[test]
    fn fan_in_stale_changelog_is_stale_even_when_advisory() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.changelog.evaluated_sha = SHA_C.to_string();
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Stale);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "changelog"
                && finding.class == EvidenceClass::Stale
                && finding.blocking
        }));
        Ok(())
    }

    #[test]
    fn fan_in_blank_changelog_disposition_is_not_proven() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.changelog.disposition = Some("  ".to_string());
        snapshot.changelog.blocking = true;
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::NotProven);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "changelog" && finding.class == EvidenceClass::NotProven
        }));
        Ok(())
    }

    #[test]
    fn fan_in_advisory_changelog_failures_remain_blocking() -> color_eyre::eyre::Result<()> {
        for result in [
            EvidenceClass::NotProven,
            EvidenceClass::Pending,
            EvidenceClass::Cancelled,
            EvidenceClass::NotApplicable,
        ] {
            let mut snapshot = fan_in_snapshot();
            snapshot.changelog.result = result;
            snapshot.changelog.blocking = false;
            let evaluation = evaluate_snapshot(&snapshot)?;
            color_eyre::eyre::ensure!(evaluation.status != MergeReadinessStatus::Ready);
            color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
                finding.source == "changelog" && finding.class == result && finding.blocking
            }));
        }
        Ok(())
    }

    #[test]
    fn fan_in_advisory_blank_changelog_disposition_remains_blocking() -> color_eyre::eyre::Result<()>
    {
        let mut snapshot = fan_in_snapshot();
        snapshot.changelog.disposition = Some("  ".to_string());
        snapshot.changelog.blocking = false;
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::NotProven);
        color_eyre::eyre::ensure!(evaluation.findings.iter().any(|finding| {
            finding.source == "changelog"
                && finding.class == EvidenceClass::NotProven
                && finding.blocking
        }));
        Ok(())
    }

    #[test]
    fn fan_in_rejects_unknown_schema() -> color_eyre::eyre::Result<()> {
        let mut snapshot = fan_in_snapshot();
        snapshot.schema_version = FAN_IN_SCHEMA_VERSION + 1;
        let result = evaluate_snapshot(&snapshot);
        color_eyre::eyre::ensure!(result.is_err());
        Ok(())
    }

    #[test]
    fn fan_in_cli_round_trips_checked_in_fixture() -> color_eyre::eyre::Result<()> {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("merge-ready")
            .join("fan-in-ready.json");
        let output = tempfile::NamedTempFile::new()?;
        evaluate_snapshot_file(&fixture, Some(output.path()))?;
        let raw = fs::read_to_string(output.path())?;
        let evaluation: MergeReadinessEvaluation = serde_json::from_str(&raw)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Ready);
        Ok(())
    }

    #[test]
    fn fan_in_cli_reports_stale_checked_in_fixture() -> color_eyre::eyre::Result<()> {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("merge-ready")
            .join("fan-in-stale-check.json");
        let output = tempfile::NamedTempFile::new()?;
        evaluate_snapshot_file(&fixture, Some(output.path()))?;
        let raw = fs::read_to_string(output.path())?;
        let evaluation: MergeReadinessEvaluation = serde_json::from_str(&raw)?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Stale);
        Ok(())
    }

    #[test]
    fn merge_readiness_status_as_verdict_maps_ready_to_valid() {
        assert_eq!(MergeReadinessStatus::Ready.as_verdict(), "valid");
    }

    #[test]
    fn merge_readiness_status_as_verdict_never_returns_valid_for_non_ready() {
        // The #4649 invariant: only Ready maps to "valid". Every other status
        // must map to a distinct non-"valid" verdict string.
        for status in [
            MergeReadinessStatus::Blocked,
            MergeReadinessStatus::Pending,
            MergeReadinessStatus::NotProven,
            MergeReadinessStatus::Stale,
            MergeReadinessStatus::DraftSkip,
            MergeReadinessStatus::Cancelled,
            MergeReadinessStatus::NotApplicable,
        ] {
            let verdict = status.as_verdict();
            assert_ne!(verdict, "valid", "non-ready status {status:?} must not map to valid");
            assert!(!verdict.is_empty());
        }
    }

    #[test]
    fn emit_without_snapshot_stamps_not_proven_and_verify_rejects_it()
    -> color_eyre::eyre::Result<()> {
        // The #4649 core invariant: a receipt stamped "not_proven" must never
        // collapse to Valid, even when blocker labels are claimed absent and
        // head/base/gate SHAs all match. verify() must report not_proven.
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "not_proven", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::NotProven);
        Ok(())
    }

    #[test]
    fn emit_without_snapshot_blocker_labels_unknown_is_blocked() {
        // The real emit-without-snapshot path sets blocker_labels_absent = false
        // (label state is unverified), which verify reports as Blocked — also
        // never Valid.
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "not_proven", false);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn emit_with_ready_snapshot_stamps_valid() -> color_eyre::eyre::Result<()> {
        // A ready fan-in snapshot must produce a "valid" verdict through the
        // emit path. We exercise the verdict-derivation logic directly by
        // evaluating the ready snapshot and mapping status -> verdict.
        let evaluation = evaluate_snapshot(&fan_in_snapshot())?;
        color_eyre::eyre::ensure!(evaluation.status == MergeReadinessStatus::Ready);
        let verdict = evaluation.status.as_verdict();
        color_eyre::eyre::ensure!(verdict == "valid");
        Ok(())
    }

    #[test]
    fn emit_with_stale_snapshot_does_not_stamp_valid() -> color_eyre::eyre::Result<()> {
        // A stale fan-in must never produce a "valid" verdict through emit.
        let mut snapshot = fan_in_snapshot();
        snapshot.checks[0].evaluated_sha = SHA_C.to_string();
        let evaluation = evaluate_snapshot(&snapshot)?;
        color_eyre::eyre::ensure!(evaluation.status != MergeReadinessStatus::Ready);
        color_eyre::eyre::ensure!(evaluation.status.as_verdict() != "valid");
        Ok(())
    }
}
