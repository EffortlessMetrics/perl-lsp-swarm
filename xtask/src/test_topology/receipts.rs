//! Structured nonzero-work route receipts and deterministic fan-in (#12411).
//!
//! A receipt is the only accepted semantic evidence for a routed target. It
//! binds the exact candidate head, the rendered command, parsed libtest work
//! counters, elapsed duration, and one typed verdict. Prose, workflow state,
//! check colour, and receipt files written by other candidates (different
//! head SHA) are rejected.
//!
//! The fan-in report is the same-candidate aggregation surface: it lists one
//! receipt per required selected target plus checked scoped no-ops, rejects
//! missing, stale-from-other-heads, duplicate, non-green, nontransferable-
//! namespace, or retry-laundered evidence, and digests deterministically
//! (volatile timing fields excluded) so identical evidence yields identical
//! artifact bytes.

use crate::test_topology::model::{
    RECEIPT_SCHEMA_VERSION, RouteClass, TargetStatus, TopologyRegister, TopologyRow,
};

/// Aggregate fan-in artifact wire format version.
pub const FAN_IN_SCHEMA_VERSION: &str = "test_topology_fan_in.v1";
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Candidate profile namespaces (issue #12125 vocabulary). A receipt records
/// the namespace it ran under; advisory, scheduled, and manual results can
/// never satisfy a required row because required rows only admit receipts on
/// PR-focused or merge-required lanes and remain keyed per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeNamespace {
    /// Required affected PR lane.
    PrFocused,
    /// Merge-required integration lane.
    MergeRequired,
    /// Scheduled pressure lane.
    ScheduledPressure,
    /// Local reproduction lane.
    LocalReproduce,
    /// Explicit manual research operation.
    ManualResearch,
}

impl ScopeNamespace {
    /// Canonical machine tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::PrFocused => "pr_focused",
            Self::MergeRequired => "merge_required",
            Self::ScheduledPressure => "scheduled_pressure",
            Self::LocalReproduce => "local_reproduce",
            Self::ManualResearch => "manual_research",
        }
    }

    /// Parse from canonical tag; rejects anything untyped as green.
    pub fn parse_tag(value: &str) -> Result<Self> {
        match value {
            "pr_focused" => Ok(Self::PrFocused),
            "merge_required" => Ok(Self::MergeRequired),
            "scheduled_pressure" => Ok(Self::ScheduledPressure),
            "local_reproduce" => Ok(Self::LocalReproduce),
            "manual_research" => Ok(Self::ManualResearch),
            other => bail!("unknown scope namespace tag {other:?}"),
        }
    }

    /// Namespaces a route class may legally run under.
    pub fn allowed_for_route_class(class: RouteClass) -> &'static [Self] {
        match class {
            RouteClass::RequiredAffected => &[Self::PrFocused, Self::MergeRequired],
            RouteClass::Advisory => &[Self::PrFocused, Self::LocalReproduce],
            RouteClass::Scheduled => &[Self::ScheduledPressure],
            RouteClass::Manual => &[Self::ManualResearch],
        }
    }
}

/// Typed verdict of one routed run. Only [`ReceiptVerdict::Pass`] is green;
/// every other variant names the exact reason the route stayed open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum ReceiptVerdict {
    /// Exit zero with executed work at or above the row minimum.
    Pass,
    /// The routed tests failed.
    FailedTests { failed: u32 },
    /// The filter selected zero executing work items.
    ZeroSelection,
    /// Executed work fell below the row's declared minimum.
    WorkShortfall { executed: u32, minimum: u32 },
    /// Ignored or skipped items appeared inside an executed selection.
    IgnoredOrSkippedPresent { count: u32 },
    /// The process was killed after exceeding its declared budget.
    TimedOut { budget_seconds: u64 },
    /// The process died without a parseable libtest summary (cancel/crash).
    CancelledOrInstrumentFailure { detail: String },
}

impl ReceiptVerdict {
    /// Whether this verdict discharges the route.
    pub fn is_green(self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// Parsed counters from one or more `test result:` summary lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibTestSummary {
    /// Tests reported passed.
    pub passed: u32,
    /// Tests reported failed.
    pub failed: u32,
    /// Tests reported ignored.
    pub ignored: u32,
    /// Tests filtered out by name selection.
    pub filtered_out: u32,
}

impl LibTestSummary {
    /// Work items that actually executed to a conclusion.
    pub fn executed_work(&self) -> u32 {
        self.passed + self.failed
    }
}

/// Parse every `test result:` line in captured libtest output, summing all
/// blocks. Returns `None` when no summary exists at all, which is itself
/// instrument failure: a zero exit without counters proves no work ran.
///
/// Real libtest summaries look like
/// `test result: ok. 12 passed; 3 failed; 1 ignored; 4 filtered out; finished in 0.01s`
/// — a status token with trailing period followed by `<count> <word>`
/// segments. Anything unshapeable rejects the whole capture so malformed or
/// foreign output can never read as evidence.
/// Counting word bucket for one libtest summary segment.
///
/// Returns the parsed value plus which counter it belongs to.
fn parse_counter(text: &str) -> Option<(u32, &'static str)> {
    for (word, bucket) in [
        ("filtered out", "filtered"),
        ("passed", "passed"),
        ("ignored", "ignored"),
        ("failed", "failed"),
    ] {
        if let Some(count) = text.strip_suffix(word) {
            return Some((count.trim().parse::<u32>().ok()?, bucket));
        }
    }
    None
}

/// Parse every `test result:` line in captured libtest output, summing all
/// blocks. Returns `None` when no summary exists at all, which is itself
/// instrument failure: a zero exit without counters proves no work ran.
///
/// Real libtest summaries look like
/// `test result: ok. 12 passed; 3 failed; 1 ignored; 4 filtered out; finished in 0.01s`
/// — a status token with trailing period followed by `<count> <word>`
/// segments. Anything unshapeable rejects the whole capture so malformed or
/// foreign output can never read as evidence.
pub fn parse_libtest_summaries(output: &str) -> Option<LibTestSummary> {
    let mut total = LibTestSummary::default();
    let mut found = false;
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("test result: ") else {
            continue;
        };
        let mut counts = LibTestSummary::default();
        let mut saw_status = false;
        let mut saw_counter = false;
        for segment in rest.split(';').map(str::trim) {
            if segment.is_empty() {
                return None;
            }
            // The status token stands alone (`ok.` / `FAILED.`) or fuses with
            // the first counter segment (`ok. 12 passed`).
            let body =
                match segment.strip_prefix("ok. ").or_else(|| segment.strip_prefix("FAILED. ")) {
                    Some(body) => {
                        saw_status = true;
                        body
                    }
                    None => {
                        if segment == "ok." || segment == "FAILED." {
                            saw_status = true;
                            continue;
                        }
                        segment
                    }
                };
            if let Some((value, bucket)) = parse_counter(body) {
                saw_counter = true;
                match bucket {
                    "filtered" => counts.filtered_out += value,
                    "passed" => counts.passed += value,
                    "ignored" => counts.ignored += value,
                    _ => counts.failed += value,
                }
            } else if !body.starts_with("finished in") && !body.starts_with("measured") {
                return None;
            }
        }
        if !saw_status || !saw_counter {
            return None;
        }
        found = true;
        total.passed += counts.passed;
        total.failed += counts.failed;
        total.ignored += counts.ignored;
        total.filtered_out += counts.filtered_out;
    }
    found.then_some(total)
}

/// JSON-friendly mirror of [`LibTestSummary`] used inside receipts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibTestCounters {
    /// Passed items.
    pub passed: u32,
    /// Failed items.
    pub failed: u32,
    /// Ignored/skipped items (never allowed inside a selected proof).
    pub ignored: u32,
    /// Items filtered out of the selection.
    pub filtered_out: u32,
}

impl From<LibTestSummary> for LibTestCounters {
    fn from(summary: LibTestSummary) -> Self {
        Self {
            passed: summary.passed,
            failed: summary.failed,
            ignored: summary.ignored,
            filtered_out: summary.filtered_out,
        }
    }
}

/// Evaluate raw execution facts against one row into its typed verdict.
///
/// Order encodes severity honestly: instrument failure before zero selection,
/// forbidden ignores before failures, shortfall before aggregate pass.
pub fn evaluate_run(
    row: &TopologyRow,
    output: &str,
    exit_ok: bool,
    timed_out: bool,
    cancel_detail: Option<String>,
) -> ReceiptVerdict {
    if row.status == TargetStatus::DeclaredPending {
        return ReceiptVerdict::CancelledOrInstrumentFailure {
            detail: "declared_pending rows cannot execute".to_owned(),
        };
    }
    if let Some(detail) = cancel_detail {
        return ReceiptVerdict::CancelledOrInstrumentFailure { detail };
    }
    if timed_out {
        return ReceiptVerdict::TimedOut { budget_seconds: row.budget_seconds };
    }
    let Some(summary) = parse_libtest_summaries(output) else {
        return ReceiptVerdict::CancelledOrInstrumentFailure {
            detail: "exit produced no parseable libtest summary".to_owned(),
        };
    };
    if summary.ignored > 0 {
        return ReceiptVerdict::IgnoredOrSkippedPresent { count: summary.ignored };
    }
    if summary.executed_work() == 0 {
        return ReceiptVerdict::ZeroSelection;
    }
    if summary.executed_work() < row.min_work_items {
        return ReceiptVerdict::WorkShortfall {
            executed: summary.executed_work(),
            minimum: row.min_work_items,
        };
    }
    // Green requires exit zero: a dying process over passing counters is an
    // instrument contradiction, never pass coverage.
    if summary.failed > 0 || !exit_ok {
        return ReceiptVerdict::FailedTests { failed: summary.failed.max(u32::from(!exit_ok)) };
    }
    ReceiptVerdict::Pass
}

/// One structured route receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestTopologyReceipt {
    /// Receipt wire format; pinned by the register schema pair.
    pub schema_version: String,
    /// Cohort selector this receipt belongs to.
    pub cohort: String,
    /// Row identity this receipt answers for; exactly one per row per head.
    pub target_id: String,
    /// Candidate head SHA this execution proved.
    pub head_sha: String,
    /// Base SHA recorded for diagnostics only.
    pub base_sha: String,
    /// Namespace lane the row ran under.
    pub namespace: String,
    /// Route class copied from the row; validated on fan-in.
    pub route_class: String,
    /// Exact rendered command text (display only; never evidence).
    pub command: String,
    /// Parsed work counters proving nonzero useful execution.
    pub work: LibTestCounters,
    /// Wall-clock duration of the execution in milliseconds.
    pub duration_ms: u64,
    /// Declared budget in seconds.
    pub budget_seconds: u64,
    /// Retry counter; structurally pinned to zero by the runner API.
    pub retries: u32,
    /// Typed verdict.
    #[serde(flatten)]
    pub verdict: ReceiptVerdict,
}

/// A checked scoped no-op emitted when changed subjects intersect no row but
/// every change has been classified through the canonical selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedNoopProof {
    /// Cohort the scoped no-op was checked against.
    pub cohort: String,
    /// Every considered change with its classification reason.
    pub classified_files: Vec<ClassifiedFile>,
    /// Head SHA binding.
    pub head_sha: String,
}

/// One changed file plus why the canonical selector did not select rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifiedFile {
    /// Repo-relative path (forward slashes).
    pub path: String,
    /// Why no row was selected: the file lies outside registered subjects.
    pub reason: String,
}

/// Digestable semantic projection of one receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanInEntry {
    /// Candidate head proven.
    pub head_sha: String,
    /// Namespace actually attached to the receipt.
    pub namespace: String,
    /// Route class declared by the row.
    pub route_class: String,
    /// Parsed work counters.
    pub work: LibTestCounters,
    /// Typed verdict.
    #[serde(flatten)]
    pub verdict: ReceiptVerdict,
    /// Retry counter (must be zero).
    pub retries: u32,
}

/// Deterministic digest over the semantic receipt surface only (identities,
/// head, work counters, verdict); volatile timing fields are excluded so
/// re-checking identical evidence yields identical bytes.
pub fn canonical_fan_in_digest(
    cohort: &str,
    head_sha: &str,
    entries: &BTreeMap<String, FanInEntry>,
) -> Result<String> {
    let normalized: BTreeMap<&str, BTreeMap<&str, String>> = entries
        .iter()
        .map(|(id, entry)| {
            Ok((
                id.as_str(),
                BTreeMap::from([
                    ("head_sha", entry.head_sha.clone()),
                    ("namespace", entry.namespace.clone()),
                    ("route_class", entry.route_class.clone()),
                    (
                        "work",
                        format!(
                            "{}/{}/{}/{}",
                            entry.work.passed,
                            entry.work.failed,
                            entry.work.ignored,
                            entry.work.filtered_out
                        ),
                    ),
                    ("verdict", serde_json::to_string(&entry.verdict)?),
                    ("retries", entry.retries.to_string()),
                ]),
            ))
        })
        .collect::<Result<_>>()?;
    let payload = serde_json::json!({
        "schema_version": RECEIPT_SCHEMA_VERSION,
        "cohort": cohort,
        "head_sha": head_sha,
        "entries": normalized,
    });
    let mut hasher = Sha256::new();
    hasher.update(payload.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(byte & 0x0F), 16).unwrap_or('0'));
    }
    Ok(out)
}

/// Everything the fan-in checker produces as its evidence verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanInReport {
    /// Aggregate wire format version.
    pub schema_version: String,
    /// Cohort.
    pub cohort: String,
    /// Candidate head checked.
    pub head_sha: String,
    /// Base SHA carried for diagnostics only.
    pub base_sha: String,
    /// Namespace lane checked.
    pub namespace: String,
    /// Required targets that needed receipts in this scope.
    pub required_targets: Vec<String>,
    /// Accepted entries keyed by target id (green required rows only).
    pub accepted: BTreeMap<String, FanInEntry>,
    /// Advisory/scheduled/manual executions retained separately; these never
    /// count toward required satisfaction regardless of verdict.
    pub auxiliary: BTreeMap<String, FanInEntry>,
    /// Checked scoped no-op proofs.
    pub scoped_noops: Vec<ScopedNoopProof>,
    /// Every violation found; empty means the selected route is discharged.
    pub violations: Vec<FanInViolation>,
    /// Deterministic digest of the semantic evidence surface.
    pub digest: String,
}

/// One fan-in violation with enough machine detail to disposition it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FanInViolation {
    /// A required selected row produced no current-head receipt.
    MissingReceipt { target_id: String },
    /// Receipts exist for the row but from other heads only.
    StaleOnlyEvidence { target_id: String, heads: Vec<String> },
    /// The row's current-head receipt did not pass.
    NotGreen { target_id: String, verdict: ReceiptVerdict },
    /// A dormant row was selected by affected routing.
    DormantSelected { target_id: String },
    /// Row appears in multiple same-head receipts.
    DuplicateReceipt { target_id: String },
    /// Receipt carries a namespace outside the row's route-class lanes.
    NamespaceTransfer { target_id: String, expected: Vec<String>, observed: String },
    /// Receipt route class does not answer for the registered row class.
    ClassMismatch { target_id: String, expected: String, observed: String },
    /// Receipt retries exceed the structural zero ceiling.
    RetryLaundering { target_id: String, retries: u32 },
    /// Receipt does not answer for any registered row of this cohort.
    UnregisteredTarget { target_id: String, observed_head: String },
}

/// Fallback deterministic label if serialization ever fails (never expected;
/// keeps violation ordering total even then).
fn violation_discriminant(violation: &FanInViolation) -> String {
    match violation {
        FanInViolation::MissingReceipt { target_id } => format!("missing:{target_id}"),
        FanInViolation::StaleOnlyEvidence { target_id, .. } => format!("stale:{target_id}"),
        FanInViolation::NotGreen { target_id, .. } => format!("not_green:{target_id}"),
        FanInViolation::DormantSelected { target_id } => format!("dormant:{target_id}"),
        FanInViolation::DuplicateReceipt { target_id } => format!("duplicate:{target_id}"),
        FanInViolation::NamespaceTransfer { target_id, .. } => format!("namespace:{target_id}"),
        FanInViolation::ClassMismatch { target_id, .. } => format!("class:{target_id}"),
        FanInViolation::RetryLaundering { target_id, .. } => format!("retry:{target_id}"),
        FanInViolation::UnregisteredTarget { target_id, .. } => format!("unregistered:{target_id}"),
    }
}

/// Load all cohort-tagged receipts under `receipts_dir`.
///
/// Returns parsed receipts plus unregistered ones so foreign evidence fails
/// loudly rather than vanishing. Receipts from other cohorts are ignored.
pub fn load_receipts(
    register: &TopologyRegister,
    receipts_dir: &Path,
) -> Result<(Vec<TestTopologyReceipt>, Vec<TestTopologyReceipt>)> {
    let rows_by_id: BTreeMap<&str, &TopologyRow> =
        register.rows().iter().map(|row| (row.target_id.as_str(), row)).collect();
    let dir_entries = match std::fs::read_dir(receipts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read receipts directory {}", receipts_dir.display()));
        }
    };
    let mut registered = Vec::new();
    let mut unregistered = Vec::new();
    for entry in dir_entries {
        let path = entry?.path();
        let file_name = path.to_string_lossy().into_owned();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json")
            || !file_name.ends_with(".receipt.json")
        {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("read receipt {}", path.display()))?;
        let receipt: TestTopologyReceipt = serde_json::from_str(&source)
            .with_context(|| format!("parse receipt {}", path.display()))?;
        if receipt.schema_version != RECEIPT_SCHEMA_VERSION {
            bail!(
                "receipt {} carries unsupported schema {:?}",
                path.display(),
                receipt.schema_version
            );
        }
        if receipt.cohort != register.cohort {
            continue;
        }
        match rows_by_id.get(receipt.target_id.as_str()) {
            Some(row) if row.cohort == register.cohort => registered.push(receipt),
            _ => unregistered.push(receipt),
        }
    }
    Ok((registered, unregistered))
}

/// Validate one loaded receipt against its row before fan-in admission.
fn admit_receipt(row: &TopologyRow, receipt: &TestTopologyReceipt) -> Option<FanInViolation> {
    if ScopeNamespace::parse_tag(&receipt.namespace).is_err()
        || !ScopeNamespace::allowed_for_route_class(row.route_class)
            .iter()
            .any(|lane| lane.tag() == receipt.namespace)
    {
        return Some(FanInViolation::NamespaceTransfer {
            target_id: row.target_id.clone(),
            expected: ScopeNamespace::allowed_for_route_class(row.route_class)
                .iter()
                .map(|lane| lane.tag().to_owned())
                .collect(),
            observed: receipt.namespace.clone(),
        });
    }
    if receipt.route_class != row.route_class.tag() {
        return Some(FanInViolation::ClassMismatch {
            target_id: row.target_id.clone(),
            expected: row.route_class.tag().to_owned(),
            observed: receipt.route_class.clone(),
        });
    }
    if receipt.retries > 0 {
        return Some(FanInViolation::RetryLaundering {
            target_id: row.target_id.clone(),
            retries: receipt.retries,
        });
    }
    if !matches!(receipt.verdict, ReceiptVerdict::Pass) {
        return Some(FanInViolation::NotGreen {
            target_id: row.target_id.clone(),
            verdict: receipt.verdict.clone(),
        });
    }
    None
}

/// Build the fan-in report for one checked scope.
///
/// `required_selected` lists target ids of required rows selected by routing;
/// `scoped_noops` carries checked scoped no-op proofs; `unregistered` carries
/// foreign receipts surfaced by [`load_receipts`].
#[allow(clippy::too_many_arguments)]
pub fn build_fan_in(
    register: &TopologyRegister,
    base_sha: &str,
    head_sha: &str,
    namespace: ScopeNamespace,
    required_selected: &[String],
    scoped_noops: Vec<ScopedNoopProof>,
    registered_receipts: &[TestTopologyReceipt],
    unregistered_receipts: &[TestTopologyReceipt],
) -> Result<FanInReport> {
    let mut violations: Vec<FanInViolation> = unregistered_receipts
        .iter()
        .map(|receipt| FanInViolation::UnregisteredTarget {
            target_id: receipt.target_id.clone(),
            observed_head: receipt.head_sha.clone(),
        })
        .collect();

    let mut accepted: BTreeMap<String, FanInEntry> = BTreeMap::new();
    let mut auxiliary: BTreeMap<String, FanInEntry> = BTreeMap::new();

    for receipt in registered_receipts.iter().filter(|r| r.head_sha == head_sha) {
        // Admission itself decides greenness; duplicates are detected below.
        let occupied =
            accepted.contains_key(&receipt.target_id) || auxiliary.contains_key(&receipt.target_id);
        if !occupied {
            match admit::decide(register, receipt, head_sha) {
                admit::Decision::AdmitRequired(entry) => {
                    accepted.insert(receipt.target_id.clone(), entry);
                }
                admit::Decision::AdmitAuxiliary(entry) => {
                    auxiliary.insert(receipt.target_id.clone(), entry);
                }
                admit::Decision::Reject(violation) => violations.push(violation),
                admit::Decision::ForeignHead => {}
            }
        } else {
            violations
                .push(FanInViolation::DuplicateReceipt { target_id: receipt.target_id.clone() });
        }
    }

    for row in register.rows() {
        let required_now = required_selected.contains(&row.target_id);
        if !required_now {
            continue;
        }
        match row.status {
            TargetStatus::DeclaredPending => {
                violations
                    .push(FanInViolation::DormantSelected { target_id: row.target_id.clone() });
            }
            TargetStatus::Active if accepted.contains_key(&row.target_id) => {}
            TargetStatus::Active => {
                let heads: Vec<String> = registered_receipts
                    .iter()
                    .filter(|r| r.target_id == row.target_id && r.head_sha != head_sha)
                    .map(|r| r.head_sha.clone())
                    .collect();
                if heads.is_empty() {
                    violations
                        .push(FanInViolation::MissingReceipt { target_id: row.target_id.clone() });
                } else {
                    violations.push(FanInViolation::StaleOnlyEvidence {
                        target_id: row.target_id.clone(),
                        heads,
                    });
                }
            }
        }
    }

    violations.sort_by_key(|violation| {
        serde_json::to_string(violation).unwrap_or_else(|_| violation_discriminant(violation))
    });

    let digest = canonical_fan_in_digest(&register.cohort, head_sha, &accepted)?;

    Ok(FanInReport {
        schema_version: FAN_IN_SCHEMA_VERSION.to_owned(),
        cohort: register.cohort.clone(),
        head_sha: head_sha.to_owned(),
        base_sha: base_sha.to_owned(),
        namespace: namespace.tag().to_owned(),
        required_targets: required_selected.to_vec(),
        accepted,
        auxiliary,
        scoped_noops,
        violations,
        digest,
    })
}

mod admit {
    //! Small admission decider separating fan-in bookkeeping from verdict
    //! validation so each law stays independently testable.

    use super::{FanInEntry, FanInViolation, TestTopologyReceipt};
    use crate::test_topology::model::{TargetStatus, TopologyRegister};

    pub(super) enum Decision {
        AdmitRequired(FanInEntry),
        AdmitAuxiliary(FanInEntry),
        Reject(FanInViolation),
        ForeignHead,
    }

    pub(super) fn decide(
        register: &TopologyRegister,
        receipt: &TestTopologyReceipt,
        head_sha: &str,
    ) -> Decision {
        if receipt.head_sha != head_sha {
            return Decision::ForeignHead;
        }
        let Some(row) = register.rows().iter().find(|row| row.target_id == receipt.target_id)
        else {
            return Decision::Reject(FanInViolation::UnregisteredTarget {
                target_id: receipt.target_id.clone(),
                observed_head: receipt.head_sha.clone(),
            });
        };
        if row.status == TargetStatus::DeclaredPending {
            return Decision::Reject(FanInViolation::DormantSelected {
                target_id: row.target_id.clone(),
            });
        }
        if let Some(violation) = super::admit_receipt(row, receipt) {
            return Decision::Reject(violation);
        }
        let entry = FanInEntry {
            head_sha: receipt.head_sha.clone(),
            namespace: receipt.namespace.clone(),
            route_class: receipt.route_class.clone(),
            work: receipt.work,
            verdict: receipt.verdict.clone(),
            retries: receipt.retries,
        };
        if row.route_class.satisfies_required() {
            Decision::AdmitRequired(entry)
        } else {
            Decision::AdmitAuxiliary(entry)
        }
    }
}
