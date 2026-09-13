//! Typed base-versus-candidate transition validation for registered
//! time-bound policy records.
//!
//! [`super`] projects the obligations of one tree at an explicit `as_of` date.
//! It cannot tell a reviewed disposition apart from calendar movement, because
//! it never sees the accepted representation the candidate is moving away from.
//!
//! This module compares the two. It does not decide whether debt is
//! acceptable; the domain validators remain authoritative for that. It proves
//! only that any later movement of a governing date arrives with the decision
//! and the changed evidence that the domain requires (#7053 slice T1).

use super::{QualityLedger, ScenarioManifest, quality_entry_invalid_reason};
use crate::tasks::staged::{StagedPathText, read_staged_path_text};
use chrono::NaiveDate;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA_VERSION: &str = "policy_transition.v1";

/// How one governing date moved between the base and candidate trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeMovement {
    /// The date is absent from both trees, or identical in both.
    Unchanged,
    /// The candidate date is earlier than the base date.
    Shortened,
    /// The candidate date is later than the base date. Only this movement
    /// needs a reviewed disposition and fresh evidence.
    Extended,
    /// The base carried a date and the candidate does not.
    Removed,
    /// The candidate carries a date the base did not.
    Added,
    /// A date on either side is not a real `YYYY-MM-DD` value.
    Invalid,
}

/// The domain's own decision, projected onto one shared vocabulary so the
/// outer transition rule can be stated once without losing domain meaning.
///
/// `replace` is deliberately absent: no T1 adapter can currently emit it. It
/// arrives with the non-Rust allowlist adapter, which stays outside this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    /// Keep the record under a refreshed justification.
    Retain,
    /// Keep a strictly smaller record.
    Narrow,
    /// Remove the record outright.
    Retire,
    /// Deliberately allow the record to lapse on its current date.
    LetExpire,
    /// No supported decision is recorded.
    NotProven,
}

impl ReviewDisposition {
    /// Whether this decision can support carrying the record past its current
    /// date at all. `Retire` and `LetExpire` contradict a later date; a record
    /// being removed or allowed to lapse is not one being renewed.
    fn supports_continuation(self) -> bool {
        matches!(self, Self::Retain | Self::Narrow)
    }
}

/// Whether the candidate evidence can support continuing the record.
///
/// Freshness is subject-bound, not merely newer text: re-dating prose while
/// every measured subject stays identical is [`Self::Stale`], never fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    /// The evidence changed and introduces a measured subject the base
    /// evidence did not carry.
    FreshMatching,
    /// The evidence is byte-identical to the accepted base evidence.
    Unchanged,
    /// The evidence text changed but names no measured subject the base
    /// evidence did not already carry.
    Stale,
    /// The domain carries an evidence field and it is empty.
    Missing,
    /// The domain validator rejects the record itself.
    Invalid,
    /// The domain registers no evidence field for this record at all.
    NotProven,
}

impl EvidenceState {
    /// Only genuinely refreshed, subject-bound evidence can justify a later date.
    fn supports_extension(self) -> bool {
        matches!(self, Self::FreshMatching)
    }
}

/// The transition verdict for one registered record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionVerdict {
    /// No movement needing review, or a movement that carries its decision and
    /// refreshed evidence.
    Pass,
    /// A governing date moved later, or a changed record is domain-invalid,
    /// without the evidence and decision the domain requires.
    Violation,
}

/// One registered time-bound record, normalized across domains.
///
/// `evidence` distinguishes "the domain has no evidence field" (`None`) from
/// "the field exists and is empty" (`Some("")`). Both refuse an extension, for
/// different reasons that an owner needs to tell apart.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TimeBoundRecord {
    record_id: String,
    source_kind: String,
    source_path: String,
    owner: String,
    review_after: Option<String>,
    expires: Option<String>,
    evidence: Option<String>,
    disposition: Option<String>,
    invalid_reason: Option<String>,
}

impl TimeBoundRecord {
    fn key(&self) -> (&str, &str) {
        (self.source_kind.as_str(), self.record_id.as_str())
    }

    /// Whether the candidate differs from the accepted base in any field this
    /// validator governs. A record nobody touched must not newly fail an
    /// unrelated candidate.
    ///
    /// `invalid_reason` is compared too, and it is not redundant: it is derived
    /// from fields this validator does not carry directly, so deleting a
    /// required falsifier turns a valid base record into an invalid candidate
    /// while every other compared field stays equal.
    fn differs_from(&self, base: &Self) -> bool {
        self.review_after != base.review_after
            || self.expires != base.expires
            || self.evidence != base.evidence
            || self.disposition != base.disposition
            || self.invalid_reason != base.invalid_reason
    }
}

/// One immutable transition result row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransitionRow {
    pub record_id: String,
    pub source_kind: String,
    pub source_path: String,
    pub owner: String,
    pub review_movement: TimeMovement,
    pub expiry_movement: TimeMovement,
    pub disposition: ReviewDisposition,
    pub evidence_state: EvidenceState,
    pub verdict: TransitionVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_review_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_review_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_expires: Option<String>,
}

#[derive(Debug, Serialize)]
struct TransitionReceipt {
    schema_version: &'static str,
    base: String,
    /// How the candidate side was read. The base is pinned to a tree object,
    /// but in this local check mode the candidate comes from live working-tree
    /// reads that are not atomic with each other, so a receipt taken during a
    /// concurrent edit can describe no single candidate tree. Recording it
    /// keeps the receipt honest about its own subject; routed enforcement
    /// (#7053 T2) evaluates a pinned candidate commit instead.
    candidate_source: &'static str,
    /// This command checks a transition; it does not measure domain debt.
    measures_domain_debt: bool,
    rows: Vec<TransitionRow>,
    violations: usize,
}

/// CLI configuration for the read-only transition check.
pub struct TransitionArgs {
    pub base: String,
    pub json: PathBuf,
    pub markdown: Option<PathBuf>,
}

/// One registered transition source: the tracked path, and the projection that
/// turns that file's text into normalized records.
///
/// Both projections deserialize through the same `serde` structs the cadence
/// inventory already uses, so this adds no second parser for the same files.
/// A file not listed here acquires no transition authority, however many
/// date-looking fields it happens to contain.
type Projection = fn(&str) -> Result<Vec<TimeBoundRecord>>;

const SOURCES: &[(&str, Projection)] = &[
    ("policy/quality-gate-exceptions.toml", quality_records),
    ("crates/perl-lsp-ux-tests/fixtures/golden_editor_workload.json", scenario_records),
];

fn quality_records(text: &str) -> Result<Vec<TimeBoundRecord>> {
    let ledger: QualityLedger = toml::from_str(text).context("parsing quality exceptions")?;
    Ok(ledger
        .exception
        .into_iter()
        .map(|entry| {
            let invalid_reason = quality_entry_invalid_reason(&entry);
            TimeBoundRecord {
                record_id: entry.id,
                source_kind: "quality_gate_exception".to_string(),
                source_path: "policy/quality-gate-exceptions.toml".to_string(),
                owner: entry.owner,
                review_after: Some(entry.review_after),
                expires: Some(entry.expires),
                evidence: Some(entry.evidence),
                disposition: entry.disposition,
                invalid_reason,
            }
        })
        .collect())
}

fn scenario_records(text: &str) -> Result<Vec<TimeBoundRecord>> {
    let manifest: ScenarioManifest =
        serde_json::from_str(text).context("parsing Scenario 67 manifest")?;
    Ok(manifest
        .error_waivers
        .into_iter()
        .map(|entry| TimeBoundRecord {
            record_id: format!("{}:{}", entry.project, entry.journey),
            source_kind: "scenario_67_error_waiver".to_string(),
            source_path: "crates/perl-lsp-ux-tests/fixtures/golden_editor_workload.json"
                .to_string(),
            owner: format!("#{}", entry.issue),
            review_after: None,
            expires: Some(entry.expires_after),
            // The Scenario 67 manifest registers no evidence field, so no
            // extension of one of its waivers can be proven from the record.
            evidence: None,
            disposition: None,
            invalid_reason: None,
        })
        .collect())
}

/// Compare the registered time-bound records at `base` against the working
/// tree and emit one deterministic transition receipt.
pub fn run(root: &Path, args: TransitionArgs) -> Result<()> {
    let receipt = build_receipt(root, &args.base)?;
    let json = serde_json::to_string_pretty(&receipt)? + "\n";
    write_output(root, &args.json, &json)?;
    if let Some(markdown) = &args.markdown {
        write_output(root, markdown, &render_markdown(&receipt))?;
    }
    for row in receipt.rows.iter().filter(|row| row.verdict == TransitionVerdict::Violation) {
        println!(
            "VIOLATION {}/{}: {}",
            row.source_kind,
            row.record_id,
            row.reason.as_deref().unwrap_or("unsupported transition")
        );
    }
    println!(
        "Policy transition: {} record(s) compared against {}; {} violation(s); wrote {}",
        receipt.rows.len(),
        receipt.base,
        receipt.violations,
        args.json.display()
    );
    if receipt.violations > 0 {
        bail!(
            "{} registered time-bound record(s) moved a governing date without a supported \
             disposition and refreshed subject-bound evidence",
            receipt.violations
        );
    }
    Ok(())
}

fn build_receipt(root: &Path, base: &str) -> Result<TransitionReceipt> {
    let base_tree = resolve_base_tree(root, base)?;
    let mut rows = Vec::new();
    for (path, project) in SOURCES {
        let base_text = read_base(root, path, &base_tree)?;
        let candidate_text = read_candidate(root, path)?;
        let base_records = match &base_text {
            Some(text) => {
                project(text).with_context(|| format!("parsing {path} at base {base}"))?
            }
            None => Vec::new(),
        };
        let candidate_records = match &candidate_text {
            Some(text) => project(text).with_context(|| format!("parsing candidate {path}"))?,
            None => Vec::new(),
        };
        // A registered policy source that the accepted base carried and the
        // candidate does not is a decommission decision, not a silent no-op:
        // without this the whole file's date movements escape review by moving
        // or deleting the file. #4092 settled the same question for the Changie
        // config — absence is never authority to infer decommissioning.
        if base_text.is_some() && candidate_text.is_none() {
            rows.push(removed_source_row(path, base_records.len()));
            continue;
        }
        rows.extend(compare(&base_records, &candidate_records));
    }
    rows.sort_by(|left, right| {
        (&left.source_kind, &left.record_id).cmp(&(&right.source_kind, &right.record_id))
    });
    let violations = rows.iter().filter(|row| row.verdict == TransitionVerdict::Violation).count();
    Ok(TransitionReceipt {
        schema_version: SCHEMA_VERSION,
        base: base.to_string(),
        candidate_source: "working_tree",
        measures_domain_debt: false,
        rows,
        violations,
    })
}

/// Resolve the accepted base revision to the tree object the staged reader
/// requires.
///
/// A branch, tag, commit, or tree spelling all resolve here; anything that
/// does not resolve is an instrument failure, never an empty base that would
/// silently report every record as newly added.
fn resolve_base_tree(root: &Path, base: &str) -> Result<String> {
    let spec = format!("{base}^{{tree}}");
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", "--quiet", &spec])
        .output()
        .with_context(|| format!("failed to resolve base revision {base}"))?;
    let oid = String::from_utf8(output.stdout)
        .map_err(|_| eyre!("git printed a non-UTF-8 object id for base revision {base}"))?
        .trim()
        .to_string();
    if !output.status.success() || oid.is_empty() {
        bail!("base revision `{base}` does not resolve to a tree in this repository");
    }
    Ok(oid)
}

/// Read the accepted representation from the base tree.
///
/// Reuses the #4092 staged reader so a path genuinely absent from the base
/// tree is told apart from a git instrument failure by exit-code structure,
/// never by parsing localized message text. An unreadable base is an error:
/// it can never silently become a pass.
fn read_base(root: &Path, path: &str, base: &str) -> Result<Option<String>> {
    match read_staged_path_text(root, path, Some(base))
        .with_context(|| format!("reading {path} at base {base}"))?
    {
        StagedPathText::Present(text) => Ok(Some(text)),
        StagedPathText::Absent => Ok(None),
        StagedPathText::Binary => {
            Err(eyre!("registered transition source {path} is not text at base {base}"))
        }
    }
}

fn read_candidate(root: &Path, path: &str) -> Result<Option<String>> {
    let full = root.join(path);
    if !full.exists() {
        return Ok(None);
    }
    fs::read_to_string(&full)
        .map(Some)
        .with_context(|| format!("reading candidate {}", full.display()))
}

fn compare(base: &[TimeBoundRecord], candidate: &[TimeBoundRecord]) -> Vec<TransitionRow> {
    let mut rows = Vec::new();
    let mut reported_ambiguous: Vec<(String, String)> = Vec::new();
    for record in candidate {
        // A repeated identity has no single accepted predecessor, so its
        // transition cannot be judged: joining to the first match would compare
        // against an arbitrary base row. Fail closed rather than guess.
        if occurrences(candidate, record) > 1 || occurrences(base, record) > 1 {
            let key = (record.source_kind.clone(), record.record_id.clone());
            if !reported_ambiguous.contains(&key) {
                reported_ambiguous.push(key);
                rows.push(ambiguous_identity_row(record));
            }
            continue;
        }
        let prior = base
            .iter()
            .find(|item| item.key() == record.key())
            .map(RenameLink::Unique)
            .unwrap_or_else(|| renamed_predecessor(base, candidate, record));
        match prior {
            RenameLink::Unique(prior) => rows.push(evaluate(Some(prior), record)),
            RenameLink::None => rows.push(evaluate(None, record)),
            RenameLink::Ambiguous => rows.push(ambiguous_rename_row(record)),
        }
    }
    for record in base {
        if occurrences(base, record) > 1 {
            continue;
        }
        if candidate.iter().any(|item| item.key() == record.key()) {
            continue;
        }
        // Skip a record already judged under its new name; emitting a Removed
        // row too would report one record twice.
        if candidate.iter().any(|item| {
            matches!(
                renamed_predecessor(base, candidate, item),
                RenameLink::Unique(prior) if prior.key() == record.key()
            )
        }) {
            continue;
        }
        rows.push(removed_row(record));
    }
    rows
}

/// Re-identify a candidate record that matches no base identity but carries the
/// exact evidence of a record this candidate dropped.
///
/// Renaming must not launder an extension. Without this, dropping
/// `ripr-total-burndown` and adding `ripr-total-burndown-2026h2` with a later
/// date and byte-identical evidence reads as an unrelated removal plus a new
/// record, and both pass — the #6216 incident with one extra field edited.
///
/// Byte-identical evidence is a precise re-identification signal rather than a
/// name heuristic, and it is the signal that matters here: an author who has
/// genuinely re-measured holds new evidence and has no reason to rename.
///
/// Two dropped records can share copy-pasted evidence, which makes the link
/// ambiguous. That case fails closed rather than taking the first match: this
/// module never guesses a predecessor, and picking one here could compare a
/// later date against the wrong accepted record.
enum RenameLink<'a> {
    /// No dropped record carries this candidate's evidence.
    None,
    /// Exactly one dropped record does.
    Unique(&'a TimeBoundRecord),
    /// More than one does, so the predecessor cannot be determined.
    Ambiguous,
}

fn renamed_predecessor<'a>(
    base: &'a [TimeBoundRecord],
    candidate: &[TimeBoundRecord],
    record: &TimeBoundRecord,
) -> RenameLink<'a> {
    let Some(evidence) = record.evidence.as_deref().filter(|text| !text.trim().is_empty()) else {
        return RenameLink::None;
    };
    let mut dropped = base.iter().filter(|prior| {
        prior.source_kind == record.source_kind
            && prior.evidence.as_deref() == Some(evidence)
            && !candidate.iter().any(|item| item.key() == prior.key())
    });
    let Some(first) = dropped.next() else {
        return RenameLink::None;
    };
    if dropped.next().is_some() {
        return RenameLink::Ambiguous;
    }
    RenameLink::Unique(first)
}

fn ambiguous_rename_row(record: &TimeBoundRecord) -> TransitionRow {
    TransitionRow {
        record_id: record.record_id.clone(),
        source_kind: record.source_kind.clone(),
        source_path: record.source_path.clone(),
        owner: record.owner.clone(),
        review_movement: TimeMovement::Invalid,
        expiry_movement: TimeMovement::Invalid,
        disposition: disposition_of(record.disposition.as_deref()),
        evidence_state: EvidenceState::Invalid,
        verdict: TransitionVerdict::Violation,
        reason: Some(
            "this record matches no accepted identity and carries evidence shared by more than \
             one record the candidate dropped, so its predecessor cannot be determined"
                .to_string(),
        ),
        base_review_after: None,
        candidate_review_after: record.review_after.clone(),
        base_expires: None,
        candidate_expires: record.expires.clone(),
    }
}

fn removed_source_row(path: &str, base_record_count: usize) -> TransitionRow {
    TransitionRow {
        record_id: path.to_string(),
        source_kind: "registered_source".to_string(),
        source_path: path.to_string(),
        owner: String::new(),
        review_movement: TimeMovement::Removed,
        expiry_movement: TimeMovement::Removed,
        disposition: ReviewDisposition::NotProven,
        evidence_state: EvidenceState::NotProven,
        verdict: TransitionVerdict::Violation,
        reason: Some(format!(
            "the accepted base carried this registered policy source with {base_record_count} \
             time-bound record(s) and the candidate does not; decommissioning it is an explicit \
             reviewed decision, not an absence"
        )),
        base_review_after: None,
        candidate_review_after: None,
        base_expires: None,
        candidate_expires: None,
    }
}

fn occurrences(records: &[TimeBoundRecord], record: &TimeBoundRecord) -> usize {
    records.iter().filter(|item| item.key() == record.key()).count()
}

fn ambiguous_identity_row(record: &TimeBoundRecord) -> TransitionRow {
    TransitionRow {
        record_id: record.record_id.clone(),
        source_kind: record.source_kind.clone(),
        source_path: record.source_path.clone(),
        owner: record.owner.clone(),
        review_movement: TimeMovement::Invalid,
        expiry_movement: TimeMovement::Invalid,
        disposition: disposition_of(record.disposition.as_deref()),
        evidence_state: EvidenceState::Invalid,
        verdict: TransitionVerdict::Violation,
        reason: Some(
            "the record identity appears more than once in its source, so no single accepted \
             predecessor exists to compare against"
                .to_string(),
        ),
        base_review_after: None,
        candidate_review_after: record.review_after.clone(),
        base_expires: None,
        candidate_expires: record.expires.clone(),
    }
}

fn evaluate(base: Option<&TimeBoundRecord>, candidate: &TimeBoundRecord) -> TransitionRow {
    let review_movement = movement(
        base.and_then(|item| item.review_after.as_deref()),
        candidate.review_after.as_deref(),
    );
    let expiry_movement =
        movement(base.and_then(|item| item.expires.as_deref()), candidate.expires.as_deref());
    let disposition = disposition_of(candidate.disposition.as_deref());
    let evidence_state = evidence_state_of(
        base.and_then(|item| item.evidence.as_deref()),
        candidate.evidence.as_deref(),
        candidate.invalid_reason.is_some(),
    );
    let changed = base.is_none_or(|prior| candidate.differs_from(prior));
    let extended =
        review_movement == TimeMovement::Extended || expiry_movement == TimeMovement::Extended;
    let invalid_movement =
        review_movement == TimeMovement::Invalid || expiry_movement == TimeMovement::Invalid;

    let reason = if changed && invalid_movement {
        Some("a governing date is not a real YYYY-MM-DD value".to_string())
    } else if changed && let Some(detail) = candidate.invalid_reason.as_deref() {
        // A record nobody touched stays the domain validator's business. A
        // changed one must be valid before its transition can be judged.
        Some(format!("changed record is rejected by its domain validator: {detail}"))
    } else if !extended {
        None
    } else if !evidence_state.supports_extension() {
        Some(extension_evidence_reason(evidence_state))
    } else if !disposition.supports_continuation() {
        Some(extension_disposition_reason(disposition))
    } else {
        None
    };

    TransitionRow {
        record_id: candidate.record_id.clone(),
        source_kind: candidate.source_kind.clone(),
        source_path: candidate.source_path.clone(),
        owner: candidate.owner.clone(),
        review_movement,
        expiry_movement,
        disposition,
        evidence_state,
        verdict: if reason.is_some() {
            TransitionVerdict::Violation
        } else {
            TransitionVerdict::Pass
        },
        reason,
        base_review_after: base.and_then(|item| item.review_after.clone()),
        candidate_review_after: candidate.review_after.clone(),
        base_expires: base.and_then(|item| item.expires.clone()),
        candidate_expires: candidate.expires.clone(),
    }
}

fn removed_row(base: &TimeBoundRecord) -> TransitionRow {
    // Removal is not a renewal. It follows the domain's own validation.
    TransitionRow {
        record_id: base.record_id.clone(),
        source_kind: base.source_kind.clone(),
        source_path: base.source_path.clone(),
        owner: base.owner.clone(),
        review_movement: base
            .review_after
            .as_ref()
            .map_or(TimeMovement::Unchanged, |_| TimeMovement::Removed),
        expiry_movement: base
            .expires
            .as_ref()
            .map_or(TimeMovement::Unchanged, |_| TimeMovement::Removed),
        disposition: disposition_of(base.disposition.as_deref()),
        evidence_state: EvidenceState::Unchanged,
        verdict: TransitionVerdict::Pass,
        reason: None,
        base_review_after: base.review_after.clone(),
        candidate_review_after: None,
        base_expires: base.expires.clone(),
        candidate_expires: None,
    }
}

fn extension_evidence_reason(state: EvidenceState) -> String {
    let detail = match state {
        EvidenceState::Unchanged => "its evidence is byte-identical to the accepted base evidence",
        EvidenceState::Stale => {
            "its evidence names no measured subject the base evidence did not already carry"
        }
        EvidenceState::Missing => "its evidence field is empty",
        EvidenceState::Invalid => "its domain validator rejects the record",
        EvidenceState::NotProven => "the domain registers no evidence for this record",
        EvidenceState::FreshMatching => "its evidence is fresh",
    };
    format!("a governing date moved later while {detail}")
}

fn extension_disposition_reason(disposition: ReviewDisposition) -> String {
    let detail = match disposition {
        ReviewDisposition::NotProven => "no supported disposition is recorded",
        ReviewDisposition::Retire => "its disposition is to remove the record",
        ReviewDisposition::LetExpire => "its disposition is to let the record expire",
        ReviewDisposition::Retain | ReviewDisposition::Narrow => "its disposition supports renewal",
    };
    format!("a governing date moved later while {detail}")
}

fn movement(base: Option<&str>, candidate: Option<&str>) -> TimeMovement {
    match (base, candidate) {
        (None, None) => TimeMovement::Unchanged,
        (None, Some(_)) => TimeMovement::Added,
        (Some(_), None) => TimeMovement::Removed,
        (Some(before), Some(after)) => {
            let (Some(before), Some(after)) = (parse_date(before), parse_date(after)) else {
                return TimeMovement::Invalid;
            };
            match after.cmp(&before) {
                std::cmp::Ordering::Greater => TimeMovement::Extended,
                std::cmp::Ordering::Less => TimeMovement::Shortened,
                std::cmp::Ordering::Equal => TimeMovement::Unchanged,
            }
        }
    }
}

fn parse_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

fn disposition_of(raw: Option<&str>) -> ReviewDisposition {
    match raw.map(str::trim) {
        Some("remove") => ReviewDisposition::Retire,
        Some("narrow") => ReviewDisposition::Narrow,
        Some("rejustify" | "renew_with_new_evidence") => ReviewDisposition::Retain,
        Some("let_expire") => ReviewDisposition::LetExpire,
        // An unrecognized spelling is not a decision. The domain validator
        // reports it separately as an invalid record.
        _ => ReviewDisposition::NotProven,
    }
}

fn evidence_state_of(
    base: Option<&str>,
    candidate: Option<&str>,
    domain_invalid: bool,
) -> EvidenceState {
    if domain_invalid {
        return EvidenceState::Invalid;
    }
    let Some(candidate) = candidate else {
        return EvidenceState::NotProven;
    };
    if candidate.trim().is_empty() {
        return EvidenceState::Missing;
    }
    let Some(base) = base else {
        // No accepted prior evidence to compare against: this is a new record,
        // whose justification is the domain validator's to judge.
        return EvidenceState::FreshMatching;
    };
    if base == candidate {
        return EvidenceState::Unchanged;
    }
    if subject_tokens(candidate).is_subset(&subject_tokens(base)) {
        return EvidenceState::Stale;
    }
    EvidenceState::FreshMatching
}

/// The measured subjects an evidence string names.
///
/// A commit SHA, a content digest, and a workflow run id each identify a
/// concrete measurement. Prose, dates, and issue references do not: an owner
/// can rewrite them without running anything. Requiring a subject the base
/// evidence did not carry is what makes "fresh" mean measured-again rather
/// than merely edited.
///
/// A run id is admitted only where a run-context word introduces it. A bare
/// decimal is not self-identifying: a measured count such as
/// `active_unresolved=123456789` is long enough to look like a run id, and
/// admitting it would let a changed metric authorize a later date while the
/// accepted commit, digest, and run all stayed the same. Digests are
/// self-identifying by shape and need no context.
///
/// Tokens keep `_` so a field name stays one word: `active_unresolved` must not
/// decay into a bare `unresolved` that could pass as run context.
fn subject_tokens(text: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let mut previous: Option<String> = None;
    for raw in text
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
    {
        let lowered = raw.to_ascii_lowercase();
        if is_digest_token(raw) {
            tokens.insert(lowered.clone());
        } else if is_run_id(raw) && previous.as_deref().is_some_and(is_run_context) {
            // Namespaced so a run id can never collide with a digest spelling.
            tokens.insert(format!("run:{lowered}"));
        }
        previous = Some(lowered);
    }
    tokens
}

/// A 40-hex commit or a 64-hex content digest.
fn is_digest_token(token: &str) -> bool {
    let hex = |length: usize| {
        token.len() == length && token.chars().all(|character| character.is_ascii_hexdigit())
    };
    hex(40) || hex(64)
}

/// Nine digits keeps a bare eight-digit date out of the subject set.
fn is_run_id(token: &str) -> bool {
    token.len() >= 9 && token.chars().all(|character| character.is_ascii_digit())
}

fn is_run_context(previous: &str) -> bool {
    matches!(
        previous,
        "run" | "runs" | "run_id" | "job" | "jobs" | "job_id" | "workflow" | "workflow_run"
    )
}

fn write_output(root: &Path, path: &Path, contents: &str) -> Result<()> {
    let path = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

fn render_markdown(receipt: &TransitionReceipt) -> String {
    let mut output = format!(
        "# Policy transition check\n\nAgainst base `{}`. {} violation(s).\n\n",
        receipt.base, receipt.violations
    );
    output.push_str(
        "| Verdict | Kind | Record | Review | Expiry | Disposition | Evidence | Reason |\n",
    );
    output.push_str("|---|---|---|---|---|---|---|---|\n");
    for row in &receipt.rows {
        output.push_str(&format!(
            "| `{:?}` | `{}` | `{}` | `{:?}` | `{:?}` | `{:?}` | `{:?}` | {} |\n",
            row.verdict,
            row.source_kind,
            row.record_id,
            row.review_movement,
            row.expiry_movement,
            row.disposition,
            row.evidence_state,
            row.reason.as_deref().unwrap_or("—").replace('|', "\\|"),
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_EVIDENCE: &str = "repo-wide receipt at head 7df72b907a01bda78fc899620019da87b4e4db9c; run 34289196987 measured active_unresolved=5037; reviewed under #6216 on 2026-08-12";

    fn quality(
        review: &str,
        expires: &str,
        evidence: &str,
        disposition: Option<&str>,
    ) -> TimeBoundRecord {
        TimeBoundRecord {
            record_id: "ripr-total-burndown".to_string(),
            source_kind: "quality_gate_exception".to_string(),
            source_path: "policy/quality-gate-exceptions.toml".to_string(),
            owner: "proof-lane".to_string(),
            review_after: Some(review.to_string()),
            expires: Some(expires.to_string()),
            evidence: Some(evidence.to_string()),
            disposition: disposition.map(str::to_string),
            invalid_reason: None,
        }
    }

    fn waiver(expires_after: &str) -> TimeBoundRecord {
        TimeBoundRecord {
            record_id: "mojolicious:edit_burst_completion".to_string(),
            source_kind: "scenario_67_error_waiver".to_string(),
            source_path: "crates/perl-lsp-ux-tests/fixtures/golden_editor_workload.json"
                .to_string(),
            owner: "#4050".to_string(),
            review_after: None,
            expires: Some(expires_after.to_string()),
            evidence: None,
            disposition: None,
            invalid_reason: None,
        }
    }

    fn verdict(base: &TimeBoundRecord, candidate: &TimeBoundRecord) -> TransitionRow {
        evaluate(Some(base), candidate)
    }

    /// The #6216 incident: both review dates moved inside an unrelated squash
    /// with no refreshed evidence.
    #[test]
    fn date_only_extension_fails() {
        let base =
            quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("renew_with_new_evidence"));
        let candidate =
            quality("2026-10-16", "2026-10-30", BASE_EVIDENCE, Some("renew_with_new_evidence"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
        assert_eq!(row.review_movement, TimeMovement::Extended);
        assert_eq!(row.evidence_state, EvidenceState::Unchanged);
    }

    /// Changing the evidence string is not producing evidence. Re-dated prose
    /// naming the same head, run and digest is a field-presence performance.
    #[test]
    fn re_dated_evidence_naming_no_new_subject_is_stale() {
        let base =
            quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("renew_with_new_evidence"));
        let restated = BASE_EVIDENCE.replace("2026-08-12", "2026-09-16");
        assert_ne!(restated, BASE_EVIDENCE, "fixture must actually change the text");
        let candidate =
            quality("2026-10-16", "2026-09-30", &restated, Some("renew_with_new_evidence"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.evidence_state, EvidenceState::Stale);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    #[test]
    fn extension_with_new_measured_subject_and_supporting_disposition_passes() {
        let base =
            quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("renew_with_new_evidence"));
        let refreshed = "repo-wide receipt at head 1111111111111111111111111111111111111111; run 34990000001 measured active_unresolved=4400; reviewed under #6216 on 2026-09-16";
        let candidate =
            quality("2026-10-16", "2026-10-30", refreshed, Some("renew_with_new_evidence"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.evidence_state, EvidenceState::FreshMatching);
        assert_eq!(row.disposition, ReviewDisposition::Retain);
        assert_eq!(row.verdict, TransitionVerdict::Pass);
        assert_eq!(row.reason, None);
    }

    /// A record being allowed to lapse cannot simultaneously be renewed.
    #[test]
    fn extension_contradicting_its_disposition_fails() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("let_expire"));
        let refreshed = "measured again at head 2222222222222222222222222222222222222222";
        let candidate = quality("2026-10-16", "2026-10-30", refreshed, Some("let_expire"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.evidence_state, EvidenceState::FreshMatching);
        assert_eq!(row.disposition, ReviewDisposition::LetExpire);
        assert_eq!(row.verdict, TransitionVerdict::Violation);

        let retiring = quality("2026-10-16", "2026-10-30", refreshed, Some("remove"));
        assert_eq!(
            evaluate(
                Some(&quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("remove"))),
                &retiring
            )
            .verdict,
            TransitionVerdict::Violation
        );
    }

    #[test]
    fn extension_without_any_disposition_fails() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, None);
        let refreshed = "measured again at head 3333333333333333333333333333333333333333";
        let candidate = quality("2026-10-16", "2026-10-30", refreshed, None);
        let row = verdict(&base, &candidate);
        assert_eq!(row.disposition, ReviewDisposition::NotProven);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    #[test]
    fn shortening_and_unchanged_records_are_not_renewals() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("let_expire"));
        let shortened = quality("2026-09-01", "2026-09-10", BASE_EVIDENCE, Some("let_expire"));
        let row = verdict(&base, &shortened);
        assert_eq!(row.review_movement, TimeMovement::Shortened);
        assert_eq!(row.verdict, TransitionVerdict::Pass);

        let untouched = verdict(&base, &base.clone());
        assert_eq!(untouched.review_movement, TimeMovement::Unchanged);
        assert_eq!(untouched.verdict, TransitionVerdict::Pass);
    }

    #[test]
    fn removal_and_addition_are_not_extensions() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("remove"));
        let rows = compare(std::slice::from_ref(&base), &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].expiry_movement, TimeMovement::Removed);
        assert_eq!(rows[0].verdict, TransitionVerdict::Pass);

        let added = evaluate(None, &base);
        assert_eq!(added.review_movement, TimeMovement::Added);
        assert_eq!(added.verdict, TransitionVerdict::Pass);
    }

    /// #5779: four Scenario 67 waivers were extended to 2026-08-11 and the
    /// repository-wide red returned on 2026-08-12. The manifest registers no
    /// evidence, so no such extension can be proven from the record.
    #[test]
    fn scenario_waiver_extension_is_not_proven() {
        let row = verdict(&waiver("2026-08-04"), &waiver("2026-08-11"));
        assert_eq!(row.expiry_movement, TimeMovement::Extended);
        assert_eq!(row.evidence_state, EvidenceState::NotProven);
        assert_eq!(row.verdict, TransitionVerdict::Violation);

        // Letting it lapse on its accepted date stays available.
        assert_eq!(
            verdict(&waiver("2026-08-04"), &waiver("2026-08-04")).verdict,
            TransitionVerdict::Pass
        );
    }

    #[test]
    fn empty_evidence_cannot_justify_an_extension() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let candidate = quality("2026-10-16", "2026-10-30", "   ", Some("narrow"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.evidence_state, EvidenceState::Missing);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    #[test]
    fn a_changed_domain_invalid_record_fails_but_an_untouched_one_does_not() {
        let mut base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        base.invalid_reason = Some("disposition requires a non-empty falsifier".to_string());

        let untouched = verdict(&base, &base.clone());
        assert_eq!(untouched.verdict, TransitionVerdict::Pass);

        let mut changed = base.clone();
        changed.evidence =
            Some("re-measured at head 4444444444444444444444444444444444444444".to_string());
        let row = verdict(&base, &changed);
        assert_eq!(row.evidence_state, EvidenceState::Invalid);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    #[test]
    fn a_malformed_governing_date_is_not_silently_unchanged() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let candidate = quality("2026-02-30", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.review_movement, TimeMovement::Invalid);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    /// Aggregate reporting: one valid and one unsupported transition must both
    /// survive into the receipt.
    #[test]
    fn both_rows_survive_a_mixed_aggregate() {
        let refreshed = "re-measured at head 5555555555555555555555555555555555555555";
        let mut valid_base =
            quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("renew_with_new_evidence"));
        valid_base.record_id = "project-coverage-burndown".to_string();
        let mut valid_candidate =
            quality("2026-10-16", "2026-10-30", refreshed, Some("renew_with_new_evidence"));
        valid_candidate.record_id = "project-coverage-burndown".to_string();

        let bad_base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("let_expire"));
        let bad_candidate = quality("2026-10-16", "2026-10-30", BASE_EVIDENCE, Some("let_expire"));

        let rows = compare(&[valid_base, bad_base], &[valid_candidate, bad_candidate]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().filter(|r| r.verdict == TransitionVerdict::Violation).count(), 1);
        assert_eq!(rows.iter().filter(|r| r.verdict == TransitionVerdict::Pass).count(), 1);
    }

    /// A measured count is not a measured subject. Changing only a long metric
    /// value leaves the accepted commit, digest, and run untouched, so it must
    /// not authorize a later date.
    #[test]
    fn a_changed_metric_count_is_not_a_fresh_subject() {
        let base =
            quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("renew_with_new_evidence"));
        let recounted =
            BASE_EVIDENCE.replace("active_unresolved=5037", "active_unresolved=123456789");
        assert_ne!(recounted, BASE_EVIDENCE, "fixture must actually change the text");
        let candidate =
            quality("2026-10-16", "2026-10-30", &recounted, Some("renew_with_new_evidence"));
        let row = verdict(&base, &candidate);
        assert_eq!(row.evidence_state, EvidenceState::Stale);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    /// Deleting a required falsifier makes a valid record invalid without
    /// touching any date, the evidence, or the disposition.
    #[test]
    fn clearing_a_required_falsifier_is_a_changed_record() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let mut candidate = base.clone();
        candidate.invalid_reason =
            Some("quality exception disposition requires a non-empty falsifier".to_string());
        assert!(candidate.differs_from(&base));
        let row = verdict(&base, &candidate);
        assert_eq!(row.evidence_state, EvidenceState::Invalid);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
    }

    /// The quality projection must actually derive that invalid state from the
    /// committed schema, so the guard above is reachable from a real ledger.
    #[test]
    fn the_quality_projection_reports_a_missing_falsifier() -> Result<()> {
        let ledger = r#"
[[exception]]
id = "fixture"
owner = "team"
scope = "fixture"
evidence = "receipt"
review_after = "2026-09-16"
expires = "2026-09-30"
disposition = "narrow"
"#;
        let records = quality_records(ledger)?;
        assert_eq!(records.len(), 1);
        assert!(
            records[0].invalid_reason.as_deref().is_some_and(|reason| reason.contains("falsifier"))
        );
        Ok(())
    }

    /// A repeated identity has no single accepted predecessor, so it must fail
    /// closed instead of silently comparing against an arbitrary base row.
    #[test]
    fn a_duplicated_record_identity_cannot_be_judged() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let duplicate = quality("2026-12-01", "2026-12-31", BASE_EVIDENCE, Some("narrow"));
        let rows = compare(std::slice::from_ref(&base), &[duplicate.clone(), duplicate.clone()]);
        assert_eq!(rows.len(), 1, "one ambiguity row per identity: {rows:?}");
        assert_eq!(rows[0].verdict, TransitionVerdict::Violation);
        assert!(
            rows[0].reason.as_deref().is_some_and(|reason| reason.contains("more than once")),
            "{:?}",
            rows[0].reason
        );
    }

    /// Renaming a record must not launder an extension past the gate. The
    /// evidence is the thing an author cannot fake without re-measuring, so a
    /// dropped record whose exact evidence reappears under a new id is the same
    /// record and its date movement is judged as such.
    #[test]
    fn renaming_a_record_does_not_launder_an_extension() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let mut renamed = quality("2027-03-31", "2027-03-31", BASE_EVIDENCE, Some("let_expire"));
        renamed.record_id = "ripr-total-burndown-2026h2".to_string();

        let rows = compare(std::slice::from_ref(&base), std::slice::from_ref(&renamed));
        assert_eq!(rows.len(), 1, "the rename is one record, not a removal plus an addition");
        assert_eq!(rows[0].review_movement, TimeMovement::Extended);
        assert_eq!(rows[0].evidence_state, EvidenceState::Unchanged);
        assert_eq!(rows[0].verdict, TransitionVerdict::Violation);
    }

    /// Copy-pasted evidence across two dropped records makes the predecessor
    /// ambiguous. Taking the first match could compare a later date against the
    /// wrong accepted record, so the link fails closed like every other
    /// ambiguity here.
    #[test]
    fn an_ambiguous_rename_predecessor_cannot_be_judged() {
        let mut first = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        first.record_id = "burndown-a".to_string();
        let mut second = quality("2026-01-01", "2026-02-01", BASE_EVIDENCE, Some("narrow"));
        second.record_id = "burndown-b".to_string();
        let mut renamed = quality("2027-03-31", "2027-03-31", BASE_EVIDENCE, Some("narrow"));
        renamed.record_id = "burndown-merged".to_string();

        let rows = compare(&[first, second], std::slice::from_ref(&renamed));
        let judged = rows
            .iter()
            .find(|row| row.record_id == "burndown-merged")
            .expect("the renamed record is judged");
        assert_eq!(judged.verdict, TransitionVerdict::Violation);
        assert!(
            judged.reason.as_deref().is_some_and(|reason| reason.contains("cannot be determined")),
            "{:?}",
            judged.reason
        );
    }

    /// A genuinely new record is still not an extension, so re-identification
    /// must not fire on one that carries its own distinct evidence.
    #[test]
    fn a_genuinely_new_record_is_not_treated_as_a_rename() {
        let base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let mut added = quality(
            "2027-03-31",
            "2027-03-31",
            "measured at head 7777777777777777777777777777777777777777",
            Some("narrow"),
        );
        added.record_id = "a-distinct-claim".to_string();

        let rows = compare(std::slice::from_ref(&base), &[base.clone(), added]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.verdict == TransitionVerdict::Pass), "{rows:?}");
    }

    /// Moving or deleting a registered policy source must not take its date
    /// movements out of review. #4092 settled the same question for the Changie
    /// config: absence is never authority to infer decommissioning.
    #[test]
    fn losing_a_registered_source_is_a_reviewed_decision() {
        let row = removed_source_row("policy/quality-gate-exceptions.toml", 2);
        assert_eq!(row.verdict, TransitionVerdict::Violation);
        assert!(
            row.reason.as_deref().is_some_and(|reason| reason.contains("reviewed decision")),
            "{:?}",
            row.reason
        );
    }

    #[test]
    fn subject_tokens_ignore_prose_dates_and_issue_references() {
        let tokens = subject_tokens("reviewed under #6216 on 2026-09-16, target 95%");
        assert!(tokens.is_empty(), "prose must name no measured subject: {tokens:?}");

        let measured = subject_tokens(
            "head 7df72b907a01bda78fc899620019da87b4e4db9c run 34289196987 sha256 be6cb0a9aa691a4c1e29de21c72432cead21f946738c816b1c0fcffec2b1b014",
        );
        assert_eq!(measured.len(), 3);
    }

    #[test]
    fn receipt_bytes_do_not_depend_on_input_order() -> Result<()> {
        let refreshed = "re-measured at head 6666666666666666666666666666666666666666";
        let first_base = quality("2026-09-16", "2026-09-30", BASE_EVIDENCE, Some("narrow"));
        let mut second_base = first_base.clone();
        second_base.record_id = "project-coverage-burndown".to_string();
        let first = quality("2026-10-16", "2026-10-30", refreshed, Some("narrow"));
        let mut second = first.clone();
        second.record_id = "project-coverage-burndown".to_string();

        let forward =
            compare(&[first_base.clone(), second_base.clone()], &[first.clone(), second.clone()]);
        let reversed = compare(&[second_base, first_base], &[second, first]);
        let canonical = |mut rows: Vec<TransitionRow>| {
            rows.sort_by(|left, right| {
                (&left.source_kind, &left.record_id).cmp(&(&right.source_kind, &right.record_id))
            });
            serde_json::to_vec_pretty(&rows)
        };
        assert_eq!(canonical(forward)?, canonical(reversed)?);
        Ok(())
    }

    /// The registered projections must accept the shapes actually committed.
    #[test]
    fn registered_projections_accept_current_repository_sources() -> Result<()> {
        let root = crate::utils::project_root()?;
        for (path, project) in SOURCES {
            let text = fs::read_to_string(root.join(path))?;
            project(&text)?;
        }
        Ok(())
    }

    #[test]
    fn a_base_revision_resolves_to_a_tree_and_an_unknown_one_is_an_instrument_failure() -> Result<()>
    {
        let root = crate::utils::project_root()?;
        let from_commit = resolve_base_tree(&root, "HEAD")?;
        assert_eq!(from_commit.len(), 40, "expected a resolved tree oid: {from_commit}");
        // A tree spelling must resolve to itself, so callers may pass either.
        assert_eq!(resolve_base_tree(&root, &from_commit)?, from_commit);
        assert!(resolve_base_tree(&root, "definitely-not-a-ref-7053").is_err());
        Ok(())
    }

    /// The current tree against itself moves no date, so the check is clean
    /// and cannot be satisfied by an empty comparison.
    #[test]
    fn current_tree_against_itself_has_no_violation() -> Result<()> {
        let root = crate::utils::project_root()?;
        let mut compared = 0usize;
        for (path, project) in SOURCES {
            let records = project(&fs::read_to_string(root.join(path))?)?;
            compared += records.len();
            for row in compare(&records, &records) {
                assert_eq!(row.verdict, TransitionVerdict::Pass, "{}", row.record_id);
            }
        }
        assert!(compared > 0, "no registered record was compared");
        Ok(())
    }
}
