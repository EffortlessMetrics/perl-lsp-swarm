//! Upstream-derived compiler conformance status projection (#12532).
//!
//! This module owns the deterministic machine packet
//! `compiler_upstream_conformance_status.v1` and its generated Markdown view.
//! It projects already-observed upstream conformance facts from a reviewed
//! inputs root into one bounded, byte-stable packet. It never reruns the
//! oracle or compiler, recomputes classifications, alters profile truth,
//! accepts limitations, repairs product behavior, or grants support/release
//! authority: those planes stay structurally separate here.
//!
//! No-score law: the packet and the rendered view carry no scalar readiness
//! score, percentage, traffic light, maturity level, majority-pass override,
//! or "mostly supports Perl" conclusion. Descriptive counts appear only with
//! their exact named denominator visible, and can never override a row.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const PACKET_SCHEMA_VERSION: &str = "compiler_upstream_conformance_status.v1";
pub const INPUTS_SCHEMA_VERSION: &str = "compiler_upstream_conformance_inputs.v1";
pub const GENERATOR_IDENTITY: &str = "cargo-xtask.compiler_upstream_status.v1";
pub const NO_SCORE_STATEMENT: &str = "No scalar readiness score, percentage, traffic light, maturity level, majority-pass override, or mostly-supports-Perl conclusion exists anywhere in this packet or its generated views; descriptive counts cannot override any row.";

/// Maximum number of validation violations reported in one bounded message.
pub const MAX_REPORTED_VIOLATIONS: usize = 40;

// ---------------------------------------------------------------------------
// Closed vocabularies
// ---------------------------------------------------------------------------

/// Closed terminal-state vocabulary for one exact obligation row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalState {
    AgreementCurrent,
    AgreementWithDeclaredLimitation,
    CompilerFailed,
    NotProven,
    Stale,
    InvalidOrConflicting,
    UnsupportedOrExternalBoundary,
    PlatformOrConfigurationBound,
    ClassificationPending,
    WitnessPending,
    RegressionNotInstalled,
    NoCurrentSnapshot,
    NoCurrentCompilerObservation,
}

impl TerminalState {
    pub fn as_str(self) -> &'static str {
        match self {
            TerminalState::AgreementCurrent => "agreement_current",
            TerminalState::AgreementWithDeclaredLimitation => "agreement_with_declared_limitation",
            TerminalState::CompilerFailed => "compiler_failed",
            TerminalState::NotProven => "not_proven",
            TerminalState::Stale => "stale",
            TerminalState::InvalidOrConflicting => "invalid_or_conflicting",
            TerminalState::UnsupportedOrExternalBoundary => "unsupported_or_external_boundary",
            TerminalState::PlatformOrConfigurationBound => "platform_or_configuration_bound",
            TerminalState::ClassificationPending => "classification_pending",
            TerminalState::WitnessPending => "witness_pending",
            TerminalState::RegressionNotInstalled => "regression_not_installed",
            TerminalState::NoCurrentSnapshot => "no_current_snapshot",
            TerminalState::NoCurrentCompilerObservation => "no_current_compiler_observation",
        }
    }

    /// Falsifier 11 plane rule: performance evidence only exists after a
    /// current correctness agreement in the exact declared state.
    fn permits_performance_evidence(self) -> bool {
        matches!(
            self,
            TerminalState::AgreementCurrent | TerminalState::AgreementWithDeclaredLimitation
        )
    }

    pub const ALL: [TerminalState; 13] = [
        TerminalState::AgreementCurrent,
        TerminalState::AgreementWithDeclaredLimitation,
        TerminalState::CompilerFailed,
        TerminalState::NotProven,
        TerminalState::Stale,
        TerminalState::InvalidOrConflicting,
        TerminalState::UnsupportedOrExternalBoundary,
        TerminalState::PlatformOrConfigurationBound,
        TerminalState::ClassificationPending,
        TerminalState::WitnessPending,
        TerminalState::RegressionNotInstalled,
        TerminalState::NoCurrentSnapshot,
        TerminalState::NoCurrentCompilerObservation,
    ];
}

/// Selected parser/HIR/PIR/world/EIR/runtime observation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationBoundary {
    Parser,
    Hir,
    Pir,
    World,
    Eir,
    Runtime,
}

impl ObservationBoundary {
    pub fn as_str(self) -> &'static str {
        match self {
            ObservationBoundary::Parser => "parser",
            ObservationBoundary::Hir => "hir",
            ObservationBoundary::Pir => "pir",
            ObservationBoundary::World => "world",
            ObservationBoundary::Eir => "eir",
            ObservationBoundary::Runtime => "runtime",
        }
    }
}

/// Supported/unsupported/external/manual claim boundary of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportBoundary {
    Supported,
    Unsupported,
    ExternalBoundary,
    Manual,
}

/// Historical movement of the upstream obligation relative to the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamChange {
    None,
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WitnessKind {
    Minimized,
    Embedded,
    Recorded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WitnessInstallation {
    Installed,
    NotInstalled,
}

impl WitnessInstallation {
    pub fn as_str(self) -> &'static str {
        match self {
            WitnessInstallation::Installed => "installed",
            WitnessInstallation::NotInstalled => "not_installed",
        }
    }
}

// ---------------------------------------------------------------------------
// Shared records
// ---------------------------------------------------------------------------

/// Original upstream provenance, always retained independently of witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamCaseRef {
    pub snapshot_ref: String,
    pub case_path: String,
    pub case_name: String,
}

/// Minimized-witness record; never replaces the original upstream case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessRecord {
    pub kind: WitnessKind,
    pub identity: String,
    /// Exact original case this witness minimizes; must differ from the
    /// retained original upstream case path on the same row.
    pub minimizes_case_path: String,
    pub installation: WitnessInstallation,
}

/// Declared limitation, nonclaims, and maximum wording for one row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitationRecord {
    pub statement: String,
    #[serde(default)]
    pub nonclaims: Vec<String>,
    pub claim_ceiling: String,
}

/// Canonical ownership for one exact obligation row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerRecord {
    pub canonical_owner: String,
    #[serde(default)]
    pub first_blocker: Option<String>,
    #[serde(default)]
    pub wake_event: Option<String>,
}

/// Performance plane of a row; only reachable when correctness is eligible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceEvidence {
    pub correctness_eligible: bool,
    #[serde(default)]
    pub evidence_identity: Option<String>,
}

/// Immutable historical relations of one obligation row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowHistory {
    pub upstream_change: UpstreamChange,
    #[serde(default)]
    pub retained_obligation_after_removal: bool,
    #[serde(default)]
    pub predecessor_row_id: Option<String>,
    #[serde(default)]
    pub successor_row_id: Option<String>,
    #[serde(default)]
    pub recurrence_of_row_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Inputs (`--inputs <root>`)
// ---------------------------------------------------------------------------

/// One maintained Perl-series/profile selector in the inputs manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesSelectorInput {
    pub series_id: String,
    pub role: String,
    /// Absent means no accepted upstream snapshot is currently selected:
    /// every projected row of the series must report `no_current_snapshot`.
    #[serde(default)]
    pub snapshot_identity: Option<String>,
    #[serde(default)]
    pub upstream_index_identity: Option<String>,
    #[serde(default)]
    pub snapshot_relation: Option<String>,
}

/// Inputs manifest schema (`<inputs>/manifest.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInputsManifest {
    pub schema_version: String,
    pub status_id: String,
    pub compiler_candidate_identity: String,
    pub toolchain_build_identity: String,
    #[serde(default)]
    pub semantic_obligation_graph_identity: Option<String>,
    #[serde(default)]
    pub slice_registry_identity: Option<String>,
    #[serde(default)]
    pub maintained_sync_identity: Option<String>,
    #[serde(default)]
    pub performance_packet_identity: Option<String>,
    /// Informational only: compiler-profile generation state never
    /// manufactures upstream conformance (profile/conformance separation).
    #[serde(default)]
    pub compiler_profile_generation_identity: Option<String>,
    #[serde(default)]
    pub maintained_series: Vec<SeriesSelectorInput>,
}

/// Input-row file schema (`<inputs>/rows/*.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseInputRow {
    pub schema_version: String,
    pub row_id: String,
    pub series_id: String,
    pub concept_family: String,
    pub concept_id: String,
    pub obligation_id: String,
    pub boundary: ObservationBoundary,
    pub oracle_subject: String,
    pub compiler_subject: String,
    pub instrument_identity: String,
    pub upstream_case: UpstreamCaseRef,
    pub terminal_state: TerminalState,
    #[serde(default)]
    pub witness: Option<WitnessRecord>,
    pub support_boundary: SupportBoundary,
    #[serde(default)]
    pub limitation: Option<LimitationRecord>,
    pub owner: OwnerRecord,
    pub performance: PerformanceEvidence,
    pub history: RowHistory,
}

// ---------------------------------------------------------------------------
// Published packet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedSeries {
    pub series_id: String,
    pub role: String,
    pub snapshot_identity: Option<String>,
    pub upstream_index_identity: Option<String>,
    pub snapshot_relation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectBinding {
    pub maintained_series: Vec<PublishedSeries>,
    pub compiler_candidate_identity: String,
    pub toolchain_build_identity: String,
    pub semantic_obligation_graph_identity: Option<String>,
    pub slice_registry_identity: Option<String>,
    pub maintained_sync_identity: Option<String>,
    pub performance_packet_identity: Option<String>,
    pub compiler_profile_generation_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuralConstants {
    pub support_authorized: bool,
    pub release_authorized: bool,
    pub published_channels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedRow {
    pub row_id: String,
    pub series_id: String,
    pub concept_family: String,
    pub concept_id: String,
    pub obligation_id: String,
    pub boundary: ObservationBoundary,
    pub oracle_subject: String,
    pub compiler_subject: String,
    pub instrument_identity: String,
    pub upstream_case: UpstreamCaseRef,
    pub terminal_state: TerminalState,
    #[serde(default)]
    pub witness: Option<WitnessRecord>,
    pub support_boundary: SupportBoundary,
    #[serde(default)]
    pub limitation: Option<LimitationRecord>,
    pub owner: OwnerRecord,
    pub performance: PerformanceEvidence,
    pub history: RowHistory,
}

impl PublishedRow {
    fn sort_key(&self) -> (&str, &str, &str, &str, &str) {
        (
            self.series_id.as_str(),
            self.concept_family.as_str(),
            self.concept_id.as_str(),
            self.obligation_id.as_str(),
            self.row_id.as_str(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesCounts {
    pub series_id: String,
    pub total_rows: u64,
    pub by_terminal_state: BTreeMap<TerminalState, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescriptiveCounts {
    pub total_rows: u64,
    pub by_terminal_state: BTreeMap<TerminalState, u64>,
    pub per_series: Vec<SeriesCounts>,
}

/// The complete `compiler_upstream_conformance_status.v1` packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceStatusPacket {
    pub schema_version: String,
    pub status_id: String,
    pub generator_identity: String,
    pub no_score_statement: String,
    pub subject_binding: SubjectBinding,
    pub structural_constants: StructuralConstants,
    pub rows: Vec<PublishedRow>,
    pub descriptive_counts: DescriptiveCounts,
}

fn structural_constants() -> StructuralConstants {
    StructuralConstants {
        support_authorized: false,
        release_authorized: false,
        published_channels: Vec::new(),
    }
}

/// Deterministic canonical serialization: stable field order from serde
/// declaration order, sorted collections everywhere, LF newline terminator.
pub fn canonical_bytes(packet: &ConformanceStatusPacket) -> Result<Vec<u8>> {
    let mut text = serde_json::to_string_pretty(packet).context("serialize status packet")?;
    text.push('\n');
    Ok(text.into_bytes())
}

pub fn packet_identity(packet: &ConformanceStatusPacket) -> Result<String> {
    let digest = Sha256::digest(canonical_bytes(packet)?);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(format!("sha256:{hex}"))
}

fn compute_counts(rows: &[PublishedRow]) -> DescriptiveCounts {
    let mut overall: BTreeMap<TerminalState, u64> = BTreeMap::new();
    let mut per_series: BTreeMap<&str, (u64, BTreeMap<TerminalState, u64>)> = BTreeMap::new();
    for row in rows {
        *overall.entry(row.terminal_state).or_insert(0) += 1;
        let entry = per_series.entry(row.series_id.as_str()).or_insert((0, BTreeMap::new()));
        entry.0 += 1;
        *entry.1.entry(row.terminal_state).or_insert(0) += 1;
    }
    DescriptiveCounts {
        total_rows: rows.len() as u64,
        by_terminal_state: overall,
        per_series: per_series
            .into_iter()
            .map(|(series_id, (total, states))| SeriesCounts {
                series_id: series_id.to_string(),
                total_rows: total,
                by_terminal_state: states,
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

const FORBIDDEN_LEAK_SUBSTRINGS: &[&str] =
    &["\\", "${", "%TEMP%", "%APPDATA%", "%LOCALAPPDATA%", "/home/", "/Users/", "file://"];

fn leak_violation(field: &str, value: &str) -> Option<String> {
    if FORBIDDEN_LEAK_SUBSTRINGS.iter().any(|bad| value.contains(bad)) {
        return Some(format!("field `{field}` leaks host/private/path detail"));
    }
    let bytes = value.as_bytes();
    if bytes.windows(3).enumerate().any(|(index, w)| {
        w[0].is_ascii_alphabetic()
            && w[1] == b':'
            && (w[2] == b'\\' || w[2] == b'/')
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
    }) {
        return Some(format!("field `{field}` contains an absolute host path"));
    }
    None
}

fn markdown_violation(field: &str, value: &str) -> Option<String> {
    if value.chars().any(char::is_control) {
        Some(format!("field `{field}` contains Markdown control syntax"))
    } else {
        None
    }
}

fn is_identifier_charset(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/' | ':' | '#'))
}

fn is_relative_normalized_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('~')
        && !value.contains('\\')
        && value.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

struct Violations<'a> {
    out: Vec<String>,
    scope: &'a str,
}

impl<'a> Violations<'a> {
    fn new(scope: &'a str) -> Self {
        Self { out: Vec::new(), scope }
    }

    fn add(&mut self, message: String) {
        self.out.push(format!("[{}]: {}", self.scope, message));
    }

    fn reject_unless(&mut self, condition: bool, message: String) {
        if !condition {
            self.add(message);
        }
    }

    fn scan_text(&mut self, field: &str, value: &Option<String>) {
        if let Some(text) = value {
            self.scan_text_value(field, text);
        }
    }

    fn scan_text_value(&mut self, field: &str, value: &str) {
        if let Some(violation) = leak_violation(field, value) {
            self.add(violation);
        }
        if let Some(violation) = markdown_violation(field, value) {
            self.add(violation);
        }
    }

    fn scan_identity(&mut self, field: &str, value: &str) {
        self.reject_unless(
            is_identifier_charset(value),
            format!("field `{field}` has invalid identity charset"),
        );
        self.reject_unless(
            !value.starts_with('/'),
            format!("field `{field}` must not be an absolute POSIX path"),
        );
        self.scan_text_value(field, value);
    }

    fn scan_optional_identity(&mut self, field: &str, value: &Option<String>) {
        if let Some(value) = value {
            self.scan_text_value(field, value);
            self.reject_unless(
                !value.starts_with('/'),
                format!("field `{field}` must not be an absolute POSIX path"),
            );
            self.reject_unless(
                !value.chars().any(char::is_control),
                format!("field `{field}` contains control characters"),
            );
        }
    }

    fn scan_free_text(&mut self, field: &str, value: &str) {
        self.scan_text_value(field, value);
        self.reject_unless(!value.trim().is_empty(), format!("field `{field}` must not be empty"));
        // Generated Markdown stays line-oriented and byte-stable.
        self.reject_unless(
            !value.chars().any(char::is_control),
            format!("field `{field}` contains control characters"),
        );
    }
}

fn bounded_report(mut violations: Vec<String>) -> Vec<String> {
    if violations.len() > MAX_REPORTED_VIOLATIONS {
        let overflow = violations.len() - MAX_REPORTED_VIOLATIONS;
        violations.truncate(MAX_REPORTED_VIOLATIONS);
        violations.push(format!("...and {overflow} more validation violations (truncated)"));
    }
    violations
}

fn validate_witness(
    v: &mut Violations,
    row_id: &str,
    upstream: &UpstreamCaseRef,
    witness: &WitnessRecord,
) {
    v.reject_unless(
        is_identifier_charset(&witness.identity),
        format!("row `{row_id}` witness identity has invalid charset"),
    );
    v.scan_text_value("witness.minimizes_case_path", &witness.minimizes_case_path);
    v.reject_unless(
        is_relative_normalized_path(&witness.minimizes_case_path),
        format!("row `{row_id}` witness.minimizes_case_path must be a normalized relative path"),
    );
    // Falsifier 2: a minimized witness never replaces the original case.
    if witness.minimizes_case_path == upstream.case_path {
        v.add(format!(
            "row `{row_id}` minimized witness replaces the original upstream case instead of minimizing it"
        ));
    }
}

fn validate_limitation(v: &mut Violations, limitation: &LimitationRecord) {
    v.scan_free_text("limitation.statement", &limitation.statement);
    v.scan_free_text("limitation.claim_ceiling", &limitation.claim_ceiling);
    let mut seen = BTreeSet::new();
    for nonclaim in &limitation.nonclaims {
        v.scan_text_value("limitation.nonclaim", nonclaim);
        if !seen.insert(nonclaim.as_str()) {
            v.add("duplicate nonclaim entry is redundant".to_string());
        }
    }
}

fn validate_owner(v: &mut Violations, owner: &OwnerRecord) {
    v.scan_free_text("owner.canonical_owner", &owner.canonical_owner);
    v.scan_text("owner.first_blocker", &owner.first_blocker);
    v.scan_text("owner.wake_event", &owner.wake_event);
}

fn validate_upstream_ref(v: &mut Violations, upstream: &UpstreamCaseRef) {
    v.reject_unless(
        is_identifier_charset(&upstream.snapshot_ref),
        "upstream snapshot_ref has invalid charset".to_string(),
    );
    v.reject_unless(
        is_relative_normalized_path(&upstream.case_path),
        "upstream case_path must be a normalized relative path without '..'".to_string(),
    );
    v.reject_unless(
        !upstream.case_name.trim().is_empty(),
        "upstream case_name is empty".to_string(),
    );
    v.scan_text_value("upstream.case_path", &upstream.case_path);
    v.scan_text_value("upstream.case_name", &upstream.case_name);
}

fn validate_history(
    v: &mut Violations,
    row_id: &str,
    history: &RowHistory,
    row_ids: &BTreeSet<&str>,
) {
    match history.upstream_change {
        UpstreamChange::Removed => {
            // Falsifier 8: a removed upstream test keeps its semantic
            // obligation active through an explicit successor mapping or a
            // declared retained local regression / maintained-older-series duty.
            v.reject_unless(
                history.retained_obligation_after_removal || history.successor_row_id.is_some(),
                "removed upstream case loses its semantic obligation without successor or retained-regression declaration".to_string(),
            );
        }
        UpstreamChange::Added | UpstreamChange::None | UpstreamChange::Changed => {}
    }
    let links = [
        ("history.predecessor_row_id", &history.predecessor_row_id),
        ("history.successor_row_id", &history.successor_row_id),
        ("history.recurrence_of_row_id", &history.recurrence_of_row_id),
    ];
    for (field, link) in links {
        if let Some(id) = link {
            v.reject_unless(id != row_id, format!("{field} references its own row"));
            v.reject_unless(is_identifier_charset(id), format!("{field} has invalid charset"));
            if field == "history.successor_row_id" {
                v.reject_unless(
                    row_ids.contains(id.as_str()),
                    format!("{field} references missing row `{id}`"),
                );
            }
        }
    }
}

fn validate_performance(
    v: &mut Violations,
    terminal_state: TerminalState,
    performance: &PerformanceEvidence,
) {
    // Performance plane independence: eligibility only after current
    // correctness agreement; evidence identity only when eligible.
    if !terminal_state.permits_performance_evidence() {
        v.reject_unless(
            !performance.correctness_eligible,
            format!(
                "performance eligibility claimed while terminal state is `{}`",
                terminal_state.as_str()
            ),
        );
        v.reject_unless(
            performance.evidence_identity.is_none(),
            "performance evidence attached without correctness eligibility".to_string(),
        );
    }
    if performance.correctness_eligible {
        v.reject_unless(
            performance.evidence_identity.is_some(),
            "performance eligibility without an evidence identity".to_string(),
        );
        if let Some(evidence) = &performance.evidence_identity {
            v.reject_unless(
                is_identifier_charset(evidence),
                "performance evidence identity has invalid charset".to_string(),
            );
        }
    } else {
        v.reject_unless(
            performance.evidence_identity.is_none(),
            "performance evidence attached without correctness eligibility".to_string(),
        );
    }
}

fn validate_row_against_series(
    row: &PublishedRow,
    selected_snapshot_by_series: &BTreeMap<String, Option<String>>,
    row_ids: &BTreeSet<&str>,
) -> Vec<String> {
    let mut v = Violations::new(&row.row_id);

    for (field, value) in [
        ("row_id", &row.row_id),
        ("series_id", &row.series_id),
        ("concept_family", &row.concept_family),
        ("concept_id", &row.concept_id),
        ("obligation_id", &row.obligation_id),
        ("instrument_identity", &row.instrument_identity),
        ("oracle_subject", &row.oracle_subject),
        ("compiler_subject", &row.compiler_subject),
    ] {
        v.reject_unless(is_identifier_charset(value), format!("{field} has invalid charset"));
    }

    validate_upstream_ref(&mut v, &row.upstream_case);

    // Currentness selection law: absence of an accepted series snapshot
    // surfaces exactly as no_current_snapshot, never as a preferred state.
    if let Some(selected_snapshot) = selected_snapshot_by_series.get(&row.series_id) {
        match selected_snapshot {
            Some(selected_snapshot) => {
                v.reject_unless(
                    row.upstream_case.snapshot_ref == *selected_snapshot,
                    format!(
                        "row snapshot_ref `{}` does not match selected snapshot `{}` for series `{}`",
                        row.upstream_case.snapshot_ref, selected_snapshot, row.series_id
                    ),
                );
                v.reject_unless(
                    row.terminal_state != TerminalState::NoCurrentSnapshot,
                    format!(
                        "series `{}` selects an accepted snapshot so the row cannot be no_current_snapshot",
                        row.series_id
                    ),
                );
            }
            None if row.terminal_state != TerminalState::NoCurrentSnapshot => {
                v.add(format!(
                    "series `{}` selects no accepted snapshot so the row state must be no_current_snapshot, found `{}`",
                    row.series_id,
                    row.terminal_state.as_str()
                ));
            }
            None => {}
        }
    } else {
        v.add(format!("row references undeclared series `{}`", row.series_id));
    }

    match row.terminal_state {
        TerminalState::AgreementCurrent => {
            match &row.witness {
                Some(witness) => {
                    v.reject_unless(
                        witness.installation == WitnessInstallation::Installed,
                        "agreement_current requires an installed witness".to_string(),
                    );
                }
                None => v.add(
                    "agreement_current requires a recorded installed witness; use witness_pending otherwise"
                        .to_string(),
                ),
            }
            v.reject_unless(
                row.limitation.is_none(),
                "agreement_current carries no limitation; use agreement_with_declared_limitation"
                    .to_string(),
            );
        }
        TerminalState::AgreementWithDeclaredLimitation => {
            match &row.limitation {
                Some(limitation) => validate_limitation(&mut v, limitation),
                None => v.add(
                    "agreement_with_declared_limitation requires a limitation record".to_string(),
                ),
            }
            match &row.witness {
                Some(witness) => v.reject_unless(
                    witness.installation == WitnessInstallation::Installed,
                    "agreement_with_declared_limitation requires an installed witness".to_string(),
                ),
                None => v.add(
                    "agreement_with_declared_limitation requires a recorded installed witness"
                        .to_string(),
                ),
            }
        }
        TerminalState::UnsupportedOrExternalBoundary => {
            v.reject_unless(
                matches!(
                    row.support_boundary,
                    SupportBoundary::Unsupported | SupportBoundary::ExternalBoundary
                ),
                "unsupported_or_external_boundary requires an unsupported or external support boundary on this plane".to_string(),
            );
        }
        TerminalState::PlatformOrConfigurationBound => {
            v.reject_unless(
                row.support_boundary == SupportBoundary::Supported,
                "platform_or_configuration_bound stays inside supported claims bound to one platform".to_string(),
            );
            match &row.limitation {
                Some(limitation) => validate_limitation(&mut v, limitation),
                None => v.add(
                    "platform_or_configuration_bound requires a platform-binding limitation"
                        .to_string(),
                ),
            }
        }
        TerminalState::Stale => v.reject_unless(
            row.history.predecessor_row_id.is_some(),
            "stale rows must expose their historical predecessor relation".to_string(),
        ),
        TerminalState::WitnessPending => v.reject_unless(
            row.witness.is_none(),
            "witness_pending conflicts with a present witness record".to_string(),
        ),
        TerminalState::RegressionNotInstalled => match &row.witness {
            Some(witness) => v.reject_unless(
                witness.installation == WitnessInstallation::NotInstalled,
                "regression_not_installed requires a witness marked not_installed".to_string(),
            ),
            None => v.add(
                "regression_not_installed requires the pending regression witness record"
                    .to_string(),
            ),
        },
        _ => {}
    }

    let witness_absence_state = matches!(
        row.terminal_state,
        TerminalState::WitnessPending
            | TerminalState::NoCurrentSnapshot
            | TerminalState::NoCurrentCompilerObservation
            | TerminalState::ClassificationPending
    );
    if witness_absence_state {
        v.reject_unless(
            row.witness.is_none(),
            format!("{} conflicts with a present witness record", row.terminal_state.as_str()),
        );
    } else if let Some(witness) = &row.witness {
        validate_witness(&mut v, &row.row_id, &row.upstream_case, witness);
    }

    validate_owner(&mut v, &row.owner);
    validate_history(&mut v, &row.row_id, &row.history, row_ids);
    validate_performance(&mut v, row.terminal_state, &row.performance);

    v.out
}

fn validate_manifest(v: &mut Violations, manifest: &StatusInputsManifest) {
    v.reject_unless(
        manifest.schema_version == INPUTS_SCHEMA_VERSION,
        format!(
            "unexpected manifest schema_version `{}`, expected `{}`",
            manifest.schema_version, INPUTS_SCHEMA_VERSION
        ),
    );
    v.scan_identity("status_id", &manifest.status_id);
    v.scan_identity("compiler_candidate_identity", &manifest.compiler_candidate_identity);
    v.scan_identity("toolchain_build_identity", &manifest.toolchain_build_identity);
    v.scan_optional_identity(
        "semantic_obligation_graph_identity",
        &manifest.semantic_obligation_graph_identity,
    );
    v.scan_optional_identity("slice_registry_identity", &manifest.slice_registry_identity);
    v.scan_optional_identity("maintained_sync_identity", &manifest.maintained_sync_identity);
    v.scan_optional_identity("performance_packet_identity", &manifest.performance_packet_identity);
    v.scan_optional_identity(
        "compiler_profile_generation_identity",
        &manifest.compiler_profile_generation_identity,
    );
    let mut seen_series = BTreeSet::new();
    for series in &manifest.maintained_series {
        if seen_series.insert(series.series_id.as_str()) {
            v.reject_unless(
                is_identifier_charset(&series.series_id) && is_identifier_charset(&series.role),
                format!("series `{}` id/role have invalid charset", series.series_id),
            );
        } else {
            v.add(format!("duplicate series selector `{}`", series.series_id));
        }
    }
}

/// Full packet validation used by `check`, `show`, `diff`, `docs`, `docs-check`.
pub fn validate_packet(packet: &ConformanceStatusPacket) -> Result<()> {
    let mut global: Vec<String> = Vec::new();

    if packet.schema_version != PACKET_SCHEMA_VERSION {
        global.push(format!(
            "[packet]: unexpected schema_version `{}`, expected `{}`",
            packet.schema_version, PACKET_SCHEMA_VERSION
        ));
    }
    if packet.generator_identity != GENERATOR_IDENTITY {
        global.push(format!(
            "[packet]: unexpected generator_identity `{}`",
            packet.generator_identity
        ));
    }
    if packet.no_score_statement != NO_SCORE_STATEMENT {
        global.push("[packet]: no-score statement was altered or removed".to_string());
    }

    {
        let mut v = Violations::new("packet");
        v.scan_identity("status_id", &packet.status_id);
        global.extend(v.out);
    }

    let constants = packet.structural_constants.clone();
    if constants.support_authorized
        || constants.release_authorized
        || !constants.published_channels.is_empty()
    {
        global.push(
            "[packet]: structural authorization constants were tampered; support/release/publication authority cannot be granted here".to_string(),
        );
    }

    let mut v = Violations::new("subject_binding");
    v.scan_identity(
        "compiler_candidate_identity",
        &packet.subject_binding.compiler_candidate_identity,
    );
    v.scan_identity("toolchain_build_identity", &packet.subject_binding.toolchain_build_identity);
    v.scan_optional_identity(
        "semantic_obligation_graph_identity",
        &packet.subject_binding.semantic_obligation_graph_identity,
    );
    v.scan_optional_identity(
        "slice_registry_identity",
        &packet.subject_binding.slice_registry_identity,
    );
    v.scan_optional_identity(
        "maintained_sync_identity",
        &packet.subject_binding.maintained_sync_identity,
    );
    v.scan_optional_identity(
        "performance_packet_identity",
        &packet.subject_binding.performance_packet_identity,
    );
    v.scan_optional_identity(
        "compiler_profile_generation_identity",
        &packet.subject_binding.compiler_profile_generation_identity,
    );
    let mut seen_series = BTreeSet::new();
    for series in &packet.subject_binding.maintained_series {
        v.scan_identity("series.series_id", &series.series_id);
        if !seen_series.insert(series.series_id.as_str()) {
            v.add(format!("duplicate published series `{}`", series.series_id));
        }
        v.scan_text_value("series.role", &series.role);
        v.scan_text("series.snapshot_relation", &series.snapshot_relation);
        v.scan_optional_identity("series.snapshot_identity", &series.snapshot_identity);
        v.scan_optional_identity("series.upstream_index_identity", &series.upstream_index_identity);
    }
    let sorted_series_ids: Vec<&str> =
        packet.subject_binding.maintained_series.iter().map(|s| s.series_id.as_str()).collect();
    let mut sorted_sorted = sorted_series_ids.clone();
    sorted_sorted.sort_unstable();
    if sorted_series_ids != sorted_sorted {
        v.add("published series are not deterministically ordered by series_id".to_string());
    }
    global.extend(v.out);

    let selected_snapshot_by_series = packet
        .subject_binding
        .maintained_series
        .iter()
        .map(|series| (series.series_id.clone(), series.snapshot_identity.clone()))
        .collect::<BTreeMap<String, Option<String>>>();

    let row_ids: BTreeSet<&str> = packet.rows.iter().map(|row| row.row_id.as_str()).collect();
    let mut row_ids_seen = BTreeSet::new();
    let mut ordered_keys: Vec<(&str, &str, &str, &str, &str)> = Vec::new();
    for row in &packet.rows {
        if !row_ids_seen.insert(row.row_id.as_str()) {
            global.push(format!("[rows]: duplicate row_id `{}`", row.row_id));
            continue;
        }
        global.extend(validate_row_against_series(row, &selected_snapshot_by_series, &row_ids));
        ordered_keys.push(row.sort_key());
    }
    let mut ordered_sorted = ordered_keys.clone();
    ordered_sorted.sort_unstable();
    if ordered_keys != ordered_sorted {
        global.push("[rows]: rows are not in deterministic canonical order".to_string());
    }

    if packet.descriptive_counts != compute_counts(&packet.rows) {
        global.push(
            "[counts]: descriptive_counts diverge from rows (omitted rows or falsified denominators)".to_string(),
        );
    }

    let mut seen_counted_series = BTreeSet::new();
    for series in &packet.descriptive_counts.per_series {
        let mut v = Violations::new("counts");
        v.scan_identity("series_id", &series.series_id);
        global.extend(v.out);
        if !seen_counted_series.insert(series.series_id.as_str()) {
            global.push(format!("[counts]: duplicate per-series count `{}`", series.series_id));
        }
    }

    let violations = bounded_report(global);
    if violations.is_empty() {
        Ok(())
    } else {
        bail!("status packet failed validation:\n{}", violations.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// Inputs loading and packet projection
// ---------------------------------------------------------------------------

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, what: &str) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read {what} at {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {what} at {}", path.display()))
}

/// Loads and shapes reviewed inputs from `<root>/manifest.json` and
/// `<root>/rows/*.json`. Directory order never influences bytes.
pub fn load_inputs(root: &Path) -> Result<(StatusInputsManifest, Vec<CaseInputRow>)> {
    let manifest: StatusInputsManifest =
        read_json(&root.join("manifest.json"), "status inputs manifest")?;
    {
        let mut violations_scope = Violations::new("manifest");
        validate_manifest(&mut violations_scope, &manifest);
        let manifest_violations = bounded_report(violations_scope.out);
        if !manifest_violations.is_empty() {
            bail!(
                "status inputs manifest failed validation:
{}",
                manifest_violations.join(
                    "
"
                )
            );
        }
    }

    let rows_dir = root.join("rows");
    if !rows_dir.is_dir() {
        bail!("inputs root {} has no rows directory", root.display());
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&rows_dir).with_context(|| format!("list {}", rows_dir.display()))? {
        let path = entry.with_context(|| format!("list {}", rows_dir.display()))?.path();
        if !path.is_file() {
            bail!("unexpected non-file under {}: {}", rows_dir.display(), path.display());
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            bail!("unexpected non-json input file: {}", path.display());
        }
        entries.push(path);
    }
    entries.sort();

    let mut rows: Vec<CaseInputRow> = Vec::new();
    for path in entries {
        let row: CaseInputRow = read_json(&path, "case input row")?;
        if row.schema_version != INPUTS_SCHEMA_VERSION {
            bail!(
                "{} declares schema_version `{}` but this projector requires `{}`",
                path.display(),
                row.schema_version,
                INPUTS_SCHEMA_VERSION
            );
        }
        rows.push(row);
    }
    Ok((manifest, rows))
}

/// Projects reviewed inputs into the canonical packet shape. Deterministic:
/// sorted series, sorted rows, derived counts, structural constants fixed.
/// Falsifier 16: no execution, network, product, or profile mutation here.
pub fn project_packet(
    manifest: StatusInputsManifest,
    rows: Vec<CaseInputRow>,
) -> Result<ConformanceStatusPacket> {
    validate_row_snapshot_bindings(&manifest, &rows)?;

    let mut selectors = manifest.maintained_series.clone();
    selectors.sort_by(|left, right| left.series_id.cmp(&right.series_id));

    let mut published_rows: Vec<PublishedRow> = rows
        .into_iter()
        .map(|row| PublishedRow {
            row_id: row.row_id,
            series_id: row.series_id,
            concept_family: row.concept_family,
            concept_id: row.concept_id,
            obligation_id: row.obligation_id,
            boundary: row.boundary,
            oracle_subject: row.oracle_subject,
            compiler_subject: row.compiler_subject,
            instrument_identity: row.instrument_identity,
            upstream_case: row.upstream_case,
            terminal_state: row.terminal_state,
            witness: row.witness,
            support_boundary: row.support_boundary,
            limitation: row.limitation,
            owner: row.owner,
            performance: row.performance,
            history: row.history,
        })
        .collect();
    published_rows.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));

    let descriptive_counts = compute_counts(&published_rows);

    Ok(ConformanceStatusPacket {
        schema_version: PACKET_SCHEMA_VERSION.to_string(),
        status_id: manifest.status_id,
        generator_identity: GENERATOR_IDENTITY.to_string(),
        no_score_statement: NO_SCORE_STATEMENT.to_string(),
        subject_binding: SubjectBinding {
            maintained_series: selectors
                .into_iter()
                .map(|series| PublishedSeries {
                    series_id: series.series_id,
                    role: series.role,
                    snapshot_identity: series.snapshot_identity,
                    upstream_index_identity: series.upstream_index_identity,
                    snapshot_relation: series.snapshot_relation,
                })
                .collect(),
            compiler_candidate_identity: manifest.compiler_candidate_identity,
            toolchain_build_identity: manifest.toolchain_build_identity,
            semantic_obligation_graph_identity: manifest.semantic_obligation_graph_identity,
            slice_registry_identity: manifest.slice_registry_identity,
            maintained_sync_identity: manifest.maintained_sync_identity,
            performance_packet_identity: manifest.performance_packet_identity,
            compiler_profile_generation_identity: manifest.compiler_profile_generation_identity,
        },
        structural_constants: structural_constants(),
        rows: published_rows,
        descriptive_counts,
    })
}

fn validate_row_snapshot_bindings(
    manifest: &StatusInputsManifest,
    rows: &[CaseInputRow],
) -> Result<()> {
    let selected_snapshots = manifest
        .maintained_series
        .iter()
        .map(|series| (series.series_id.as_str(), series.snapshot_identity.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let mut violations = Vec::new();

    for row in rows {
        if let Some(Some(selected_snapshot)) = selected_snapshots.get(row.series_id.as_str()) {
            if row.upstream_case.snapshot_ref != *selected_snapshot {
                violations.push(format!(
                    "row `{}` upstream snapshot_ref `{}` does not match selected snapshot `{}` for series `{}`",
                    row.row_id,
                    row.upstream_case.snapshot_ref,
                    selected_snapshot,
                    row.series_id
                ));
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        bail!(
            "status input rows failed snapshot binding validation:\n{}",
            bounded_report(violations).join("\n")
        )
    }
}

fn load_packet(path: &Path) -> Result<ConformanceStatusPacket> {
    let packet: ConformanceStatusPacket = read_json(path, "conformance status packet")?;
    validate_packet(&packet)?;
    Ok(packet)
}

pub fn run_build(inputs: &Path, output: &Path) -> Result<String> {
    let (manifest, rows) = load_inputs(inputs)?;
    let packet = project_packet(manifest, rows)?;
    validate_packet(&packet)?;
    let bytes = canonical_bytes(&packet)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(output, &bytes).with_context(|| format!("write {}", output.display()))?;
    Ok(format!(
        "built {} rows={} identity={}",
        PACKET_SCHEMA_VERSION,
        packet.rows.len(),
        packet_identity(&packet)?
    ))
}

pub fn run_check(path: &Path) -> Result<String> {
    let packet = load_packet(path)?;
    Ok(format!(
        "{} identity={} status_id={} series={} rows={}",
        PACKET_SCHEMA_VERSION,
        packet_identity(&packet)?,
        packet.status_id,
        packet.subject_binding.maintained_series.len(),
        packet.rows.len()
    ))
}

pub fn run_show(path: &Path, series: Option<&str>, concept: Option<&str>) -> Result<Vec<String>> {
    let packet = load_packet(path)?;
    let mut lines =
        vec![format!("status_id={} identity={}", packet.status_id, packet_identity(&packet)?)];
    for row in &packet.rows {
        let series_matches = match series {
            Some(wanted) => row.series_id == wanted,
            None => true,
        };
        let concept_matches = match concept {
            Some(wanted) => row.concept_id == wanted || row.concept_family == wanted,
            None => true,
        };
        if series_matches && concept_matches {
            lines.extend(row_display_lines(row));
        }
    }
    Ok(lines)
}

fn row_display_lines(row: &PublishedRow) -> Vec<String> {
    let evidence_display = match row.performance.evidence_identity.as_deref() {
        Some(identity) => identity.to_string(),
        None => "<none>".to_string(),
    };
    let witness_line = match &row.witness {
        Some(witness) => format!(
            "  witness kind={} identity={} minimizes_case_path={} installation={}",
            witness_kind_str(witness.kind),
            witness.identity,
            witness.minimizes_case_path,
            witness.installation.as_str()
        ),
        None => "  witness none".to_string(),
    };
    vec![
        format!(
            "row {} series={} concept_family={} concept_id={} obligation_id={}",
            row.row_id, row.series_id, row.concept_family, row.concept_id, row.obligation_id
        ),
        format!(
            "  state={} boundary={} support_boundary={}",
            row.terminal_state.as_str(),
            row.boundary.as_str(),
            support_boundary_str(row.support_boundary)
        ),
        format!(
            "  subjects oracle={} compiler={} instrument={}",
            row.oracle_subject, row.compiler_subject, row.instrument_identity
        ),
        format!(
            "  upstream original retained snapshot_ref={} case_path={} case_name={}",
            row.upstream_case.snapshot_ref,
            row.upstream_case.case_path,
            row.upstream_case.case_name
        ),
        witness_line,
        format!("  owner={}", row.owner.canonical_owner),
        format!(
            "  performance correctness_eligible={} evidence={}",
            row.performance.correctness_eligible, evidence_display
        ),
        format!(
            "  history upstream_change={:?} retained_obligation_after_removal={}",
            row.history.upstream_change, row.history.retained_obligation_after_removal
        ),
    ]
}

fn support_boundary_str(value: SupportBoundary) -> &'static str {
    match value {
        SupportBoundary::Supported => "supported",
        SupportBoundary::Unsupported => "unsupported",
        SupportBoundary::ExternalBoundary => "external_boundary",
        SupportBoundary::Manual => "manual",
    }
}

fn witness_kind_str(value: WitnessKind) -> &'static str {
    match value {
        WitnessKind::Minimized => "minimized",
        WitnessKind::Embedded => "embedded",
        WitnessKind::Recorded => "recorded",
    }
}

fn upstream_change_str(value: UpstreamChange) -> &'static str {
    match value {
        UpstreamChange::None => "none",
        UpstreamChange::Added => "added",
        UpstreamChange::Removed => "removed",
        UpstreamChange::Changed => "changed",
    }
}

const DIFF_MAX_LINES: usize = 100;

pub fn run_diff(before: &Path, after: &Path) -> Result<String> {
    let before_packet = load_packet(before)?;
    let after_packet = load_packet(after)?;

    if before_packet == after_packet {
        return Ok(format!("identical identity={}", packet_identity(&before_packet)?));
    }

    let mut lines: Vec<String> = Vec::new();
    summarize_binding_diff(
        &before_packet.subject_binding,
        &after_packet.subject_binding,
        &mut lines,
    );

    let before_by_id: BTreeMap<&str, &PublishedRow> =
        before_packet.rows.iter().map(|row| (row.row_id.as_str(), row)).collect();
    let after_by_id: BTreeMap<&str, &PublishedRow> =
        after_packet.rows.iter().map(|row| (row.row_id.as_str(), row)).collect();
    for (row_id, before_row) in &before_by_id {
        match after_by_id.get(row_id) {
            None => lines.push(format!("- row {row_id} disappeared")),
            Some(after_row) => {
                diff_row_field(
                    &mut lines,
                    row_id,
                    "series_id",
                    &before_row.series_id,
                    &after_row.series_id,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "concept_family",
                    &before_row.concept_family,
                    &after_row.concept_family,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "concept_id",
                    &before_row.concept_id,
                    &after_row.concept_id,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "obligation_id",
                    &before_row.obligation_id,
                    &after_row.obligation_id,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "boundary",
                    &before_row.boundary,
                    &after_row.boundary,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "oracle_subject",
                    &before_row.oracle_subject,
                    &after_row.oracle_subject,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "compiler_subject",
                    &before_row.compiler_subject,
                    &after_row.compiler_subject,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "instrument_identity",
                    &before_row.instrument_identity,
                    &after_row.instrument_identity,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "upstream_case",
                    &before_row.upstream_case,
                    &after_row.upstream_case,
                );
                if before_row.terminal_state != after_row.terminal_state {
                    lines.push(format!(
                        "~ row {row_id}: `{}` -> `{}`",
                        before_row.terminal_state.as_str(),
                        after_row.terminal_state.as_str()
                    ));
                }
                diff_row_field(
                    &mut lines,
                    row_id,
                    "witness",
                    &before_row.witness,
                    &after_row.witness,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "support_boundary",
                    &before_row.support_boundary,
                    &after_row.support_boundary,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "limitation",
                    &before_row.limitation,
                    &after_row.limitation,
                );
                diff_row_field(&mut lines, row_id, "owner", &before_row.owner, &after_row.owner);
                diff_row_field(
                    &mut lines,
                    row_id,
                    "performance",
                    &before_row.performance,
                    &after_row.performance,
                );
                diff_row_field(
                    &mut lines,
                    row_id,
                    "history",
                    &before_row.history,
                    &after_row.history,
                );
            }
        }
    }
    for row_id in after_by_id.keys() {
        if !before_by_id.contains_key(row_id) {
            lines.push(format!("+ row {row_id} appeared"));
        }
    }

    bail!(
        "packets differ ({} summarized differences, max {}):\n{}",
        lines.len().min(DIFF_MAX_LINES),
        DIFF_MAX_LINES,
        lines.into_iter().take(DIFF_MAX_LINES).collect::<Vec<_>>().join("\n")
    )
}

fn diff_row_field<T: PartialEq>(
    lines: &mut Vec<String>,
    row_id: &str,
    field: &str,
    before: &T,
    after: &T,
) {
    if before != after {
        lines.push(format!("~ row {row_id}: {field} changed"));
    }
}

fn summarize_binding_diff(
    before: &SubjectBinding,
    after: &SubjectBinding,
    lines: &mut Vec<String>,
) {
    diff_binding_field(
        lines,
        "compiler_candidate_identity",
        &before.compiler_candidate_identity,
        &after.compiler_candidate_identity,
    );
    diff_binding_field(
        lines,
        "toolchain_build_identity",
        &before.toolchain_build_identity,
        &after.toolchain_build_identity,
    );
    diff_binding_field(
        lines,
        "semantic_obligation_graph_identity",
        &before.semantic_obligation_graph_identity,
        &after.semantic_obligation_graph_identity,
    );
    diff_binding_field(
        lines,
        "slice_registry_identity",
        &before.slice_registry_identity,
        &after.slice_registry_identity,
    );
    diff_binding_field(
        lines,
        "maintained_sync_identity",
        &before.maintained_sync_identity,
        &after.maintained_sync_identity,
    );
    diff_binding_field(
        lines,
        "performance_packet_identity",
        &before.performance_packet_identity,
        &after.performance_packet_identity,
    );
    diff_binding_field(
        lines,
        "compiler_profile_generation_identity",
        &before.compiler_profile_generation_identity,
        &after.compiler_profile_generation_identity,
    );
    if before.maintained_series != after.maintained_series {
        lines.push("~ subject binding maintained_series changed".to_string());
    }
}

fn diff_binding_field<T: PartialEq>(lines: &mut Vec<String>, name: &str, before: &T, after: &T) {
    if before != after {
        lines.push(format!("~ subject binding {name} changed"));
    }
}

// ---------------------------------------------------------------------------
// Generated human view (deterministic Markdown)
// ---------------------------------------------------------------------------

fn opt_line(label: &str, value: &Option<String>) -> String {
    match value {
        Some(text) => format!("- {label}: `{}`\n", markdown_code(text)),
        None => format!("- {label}: absent\n"),
    }
}

fn markdown_code(value: &str) -> String {
    value.replace('\\', "\\\\").replace('`', "\\`")
}

fn markdown_code_span(value: &str) -> String {
    let longest_run = value
        .chars()
        .fold((0usize, 0usize), |(longest, current), character| {
            if character == '`' { (longest.max(current + 1), current + 1) } else { (longest, 0) }
        })
        .0;
    let delimiter = "`".repeat(longest_run.max(1) + 1);
    format!("{delimiter}{value}{delimiter}")
}

fn markdown_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '<'
                | '>'
                | '('
                | ')'
                | '#'
                | '+'
                | '-'
                | '.'
                | '!'
                | '|'
                | '~'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn quote_text(value: &str) -> String {
    format!("\"{}\"", markdown_text(value))
}

/// Renders the generated Markdown view. Consumes only the already-validated
/// machine packet; front matter, badges, checks, prior prose cannot set state
/// (falsifier 13). Output is byte-stable for identical packets.
pub fn render_markdown(packet: &ConformanceStatusPacket) -> Result<String> {
    validate_packet(packet).context("validate packet before Markdown rendering")?;
    let binding = &packet.subject_binding;
    let mut out = String::new();
    out.push_str("# Compiler upstream conformance status\n\n");
    out.push_str(&format!("- packet_schema: `{PACKET_SCHEMA_VERSION}`\n"));
    out.push_str(&format!("- status_id: `{}`\n", markdown_code(&packet.status_id)));
    out.push_str(&format!(
        "- generator_identity: `{}`\n",
        markdown_code(&packet.generator_identity)
    ));
    out.push_str(&opt_line(
        "compiler_candidate_identity",
        &binding.compiler_candidate_identity.clone().into_option(),
    ));
    out.push_str(&opt_line(
        "toolchain_build_identity",
        &binding.toolchain_build_identity.clone().into_option(),
    ));
    out.push_str(&opt_line(
        "semantic_obligation_graph_identity",
        &binding.semantic_obligation_graph_identity,
    ));
    out.push_str(&opt_line("slice_registry_identity", &binding.slice_registry_identity));
    out.push_str(&opt_line("maintained_sync_identity", &binding.maintained_sync_identity));
    out.push_str(&opt_line("performance_packet_identity", &binding.performance_packet_identity));
    out.push_str(&opt_line(
        "compiler_profile_generation_identity_informational_only",
        &binding.compiler_profile_generation_identity,
    ));
    out.push_str("- support_authorized: false\n");
    out.push_str("- release_authorized: false\n");
    out.push_str("- published_channels: (none; structural constant)\n");
    out.push('\n');
    out.push_str(&packet.no_score_statement);
    out.push_str("\n\n");

    // Progressive disclosure level 1: series and snapshots.
    out.push_str("## Maintained series and snapshots\n");
    for series in &binding.maintained_series {
        out.push_str(&format!("\n### {}\n", markdown_text(&series.series_id)));
        out.push_str(&format!("- role: `{}`\n", markdown_code(&series.role)));
        match &series.snapshot_identity {
            Some(snapshot) => {
                out.push_str(&format!("- snapshot_identity: `{}`\n", markdown_code(snapshot)))
            }
            None => out
                .push_str("- snapshot_identity: absent (no accepted current upstream snapshot)\n"),
        }
        out.push_str(&opt_line("upstream_index_identity", &series.upstream_index_identity));
        out.push_str(&opt_line("snapshot_relation", &series.snapshot_relation));
    }
    if binding.maintained_series.is_empty() {
        out.push_str("\nNo maintained Perl series is selected in this packet.\n");
    }

    // Level 2-4: rows grouped by series, family, concept.
    out.push_str("\n## Obligation rows\n");
    if packet.rows.is_empty() {
        out.push_str(
            "\nNo obligation rows are recorded; absence stays visible instead of\nprior prose.\n",
        );
    }
    let mut last_series: Option<&str> = None;
    let mut last_family_concept: Option<(&str, &str)> = None;
    for row in &packet.rows {
        if last_series != Some(row.series_id.as_str()) {
            out.push_str(&format!("\n### {}\n", markdown_text(&row.series_id)));
            last_series = Some(row.series_id.as_str());
            last_family_concept = None;
        }
        if last_family_concept != Some((row.concept_family.as_str(), row.concept_id.as_str())) {
            out.push_str(&format!(
                "\n#### {} / {}\n",
                markdown_text(&row.concept_family),
                markdown_text(&row.concept_id)
            ));
            last_family_concept = Some((row.concept_family.as_str(), row.concept_id.as_str()));
        }
        out.push('\n');
        out.push_str(&format!(
            "##### {} (`{}`)\n\n",
            markdown_text(&row.row_id),
            markdown_code(&row.obligation_id)
        ));
        out.push_str(&format!(
            "current result: `{}`\n\n",
            markdown_code(row.terminal_state.as_str())
        ));
        out.push_str(&format!("- selected observation boundary: {}\n", row.boundary.as_str()));
        out.push_str(&format!(
            "- oracle subject: `{}`\n- compiler subject: `{}`\n- instrument identity: `{}`\n",
            markdown_code(&row.oracle_subject),
            markdown_code(&row.compiler_subject),
            markdown_code(&row.instrument_identity)
        ));
        out.push_str(&format!(
            "- upstream original (retained independent of witnesses): snapshot_ref={}, case_path={}, case_name={}\n",
            markdown_code_span(&row.upstream_case.snapshot_ref),
            markdown_code_span(&row.upstream_case.case_path),
            markdown_code_span(&row.upstream_case.case_name)
        ));
        match &row.witness {
            Some(witness) => out.push_str(&format!(
                "- minimized witness (does not replace the original): kind=`{}`, identity=`{}`, installation={}, minimizes_case_path=`{}`\n",
                witness_kind_str(witness.kind),
                markdown_code(&witness.identity),
                witness.installation.as_str(),
                markdown_code(&witness.minimizes_case_path)
            )),
            None => out.push_str(
                "- witness: none (the current result records why nothing is witnessed here)\n",
            ),
        }
        out.push_str(&format!(
            "- support boundary: {}\n",
            support_boundary_str(row.support_boundary)
        ));
        out.push_str(&format!("- owner: {}\n", markdown_text(&row.owner.canonical_owner)));
        match &row.owner.first_blocker {
            Some(blocker) => {
                out.push_str(&format!("- first blocker: {}\n", markdown_text(blocker)))
            }
            None => out.push_str("- first blocker: absent\n"),
        }
        match &row.owner.wake_event {
            Some(wake) => out.push_str(&format!("- wake event: {}\n", markdown_text(wake))),
            None => out.push_str("- wake event: absent\n"),
        }
        match &row.limitation {
            Some(limitation) => {
                out.push_str(&format!(
                    "- limitation: {} ; claim ceiling: {}\n",
                    quote_text(&limitation.statement),
                    quote_text(&limitation.claim_ceiling)
                ));
                if limitation.nonclaims.is_empty() {
                    out.push_str("  - nonclaims: none declared\n");
                } else {
                    for nonclaim in &limitation.nonclaims {
                        out.push_str(&format!("  - nonclaim: {}\n", quote_text(nonclaim)));
                    }
                }
            }
            None => out.push_str("- limitation: none (state carries no extra wording allowance)\n"),
        }
        if row.performance.correctness_eligible {
            out.push_str(&format!(
                "- performance: correctness eligible; evidence identity: `{}`\n",
                row.performance.evidence_identity.as_deref().unwrap_or("<missing>")
            ));
        } else {
            out.push_str(
                "- performance: not reported because correctness/currentness eligibility is absent\n",
            );
        }
        out.push_str(&format!(
            "- history: upstream_change={}, predecessor_row_id={}, successor_row_id={}, recurrence_of_row_id={}, retained_obligation_after_removal={}\n",
            upstream_change_str(row.history.upstream_change),
            format_opt_id(&row.history.predecessor_row_id),
            format_opt_id(&row.history.successor_row_id),
            format_opt_id(&row.history.recurrence_of_row_id),
            row.history.retained_obligation_after_removal
        ));
    }

    // Descriptive counts with visible denominators.
    out.push_str("\n## Descriptive counts (no aggregate judgment)\n\n");
    out.push_str(&format!(
        "Exact row denominator: {}. Every underlying row remains addressable above;\ncounts cannot replace terminal concept/stage results or claim ceilings and\na high passing count does not compensate any required failing boundary.\n\n",
        packet.descriptive_counts.total_rows
    ));
    out.push_str("| terminal_state | count |\n|---|---|\n");
    for (state, count) in &packet.descriptive_counts.by_terminal_state {
        out.push_str(&format!("| {} | {count} |\n", state.as_str()));
    }
    out.push_str("\n| series | total rows | terminal states |\n|---|---|---|\n");
    for series in &packet.descriptive_counts.per_series {
        let mut states = Vec::new();
        for (state, count) in &series.by_terminal_state {
            states.push(format!("{}={count}", markdown_code(state.as_str())));
        }
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_text(&series.series_id),
            series.total_rows,
            states.join("; ")
        ));
    }

    // History movement summary (immutable relations stay explicit).
    out.push_str("\n## History movement\n\n");
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut recurrence = Vec::new();
    for row in &packet.rows {
        match row.history.upstream_change {
            UpstreamChange::Added => added.push(markdown_code(&row.row_id)),
            UpstreamChange::Removed => {
                if let Some(successor) = &row.history.successor_row_id {
                    removed.push(format!(
                        "`{}` (obligation continues via successor `{}`)",
                        markdown_code(&row.row_id),
                        markdown_code(successor)
                    ));
                } else if row.history.retained_obligation_after_removal {
                    removed.push(format!(
                        "`{}` (semantic obligation retained locally)",
                        markdown_code(&row.row_id)
                    ));
                }
            }
            UpstreamChange::Changed => changed.push(markdown_code(&row.row_id)),
            UpstreamChange::None => {}
        }
        if let Some(recurrence_of) = &row.history.recurrence_of_row_id {
            recurrence.push(format!(
                "`{}` recurs `{}`",
                markdown_code(&row.row_id),
                markdown_code(recurrence_of)
            ));
        }
    }
    out.push_str(&format!("- added obligations: {}\n", join_or_none(&added)));
    out.push_str(&format!("- removed upstream cases: {}\n", join_or_none(&removed)));
    out.push_str(&format!("- changed obligations: {}\n", join_or_none(&changed)));
    out.push_str(&format!("- recurrences: {}\n", join_or_none(&recurrence)));

    Ok(out)
}

fn format_opt_id(value: &Option<String>) -> String {
    match value {
        Some(id) => format!("`{}`", markdown_code(id)),
        None => "absent".to_string(),
    }
}

fn join_or_none<S: std::fmt::Display>(values: &[S]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(", ")
    }
}

trait OneLineOption {
    fn into_option(self) -> Option<String>;
}

impl OneLineOption for String {
    fn into_option(self) -> Option<String> {
        Some(self)
    }
}

pub fn run_docs(status: &Path, output: &Path) -> Result<String> {
    let packet = load_packet(status)?;
    let markdown = render_markdown(&packet)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(output, markdown.as_bytes())
        .with_context(|| format!("write {}", output.display()))?;
    Ok(format!("rendered {} bytes to {}", markdown.len(), output.display()))
}

pub fn run_docs_check(status: &Path, path: &Path) -> Result<String> {
    let packet = load_packet(status)?;
    let expected = render_markdown(&packet)?;
    let actual = fs::read_to_string(path)
        .with_context(|| format!("read generated view {}", path.display()))?;
    if actual == expected {
        return Ok(format!("generated view matches its validated packet ({})", path.display()));
    }
    let first_divergent =
        expected.lines().zip(actual.lines()).position(|(expected, actual)| expected != actual);
    bail!(
        "generated view at {} drifts from its validated packet{}; regenerate with `cargo xtask compiler upstream status docs`, never by editing prose",
        path.display(),
        match first_divergent {
            Some(line) => format!(" near line {}", line + 1),
            None => String::new(),
        }
    )
}

#[cfg(test)]
mod tests;
