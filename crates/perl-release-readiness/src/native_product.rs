//! Canonical native-product readiness payload (`native_product_readiness.v1`).
//!
//! This module is the one producer of the native-product posture payload
//! described by #7168. It answers one question: does this candidate present
//! and configure the native Perl tooling product honestly? It does not score
//! engineering proof, archive integrity, or installed behavior.
//!
//! Each row reuses the existing evidence adapters
//! ([`product_surface::native_only`](crate::evidence::product_surface),
//! [`dap::cli_native_only`](crate::evidence::dap), and the readiness-receipt
//! native-default readers) rather than re-deriving engine defaults, so the
//! legacy indicator, this payload, and the product itself cannot drift into
//! three separate definitions of "native by default".
//!
//! The verdict is conjunctive and scoreless: any failed row blocks, any
//! unverified row leaves the payload `not_proven`, any warning limits it,
//! and only an all-pass catalog is `ready`. One green tool never compensates
//! for another blocked mandatory row.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::evidence::{Outcome, dap, product_surface, readiness};
use crate::indicator::{EvidenceRef, IndicatorStatus};
use crate::profile::KwaliteeProfile;

mod validation;

/// Payload kind stamped into every emitted receipt.
pub const NATIVE_PRODUCT_KIND: &str = "native_product_readiness";
/// Payload schema version.
pub const NATIVE_PRODUCT_SCHEMA_VERSION: u32 = 1;

/// Row identifier for the first-mile native-only product surface.
pub const ROW_NATIVE_ONLY: &str = "native_product.native_only";
/// Row identifier for the shipped DAP CLI native-only surface.
pub const ROW_DAP_CLI_NATIVE_ONLY: &str = "native_product.dap_cli_native_only";
/// Row identifier for the formatter native-default posture.
pub const ROW_FORMATTER_NATIVE_DEFAULT: &str = "native_product.formatter_native_default";
/// Row identifier for the critic native-default posture.
pub const ROW_CRITIC_NATIVE_DEFAULT: &str = "native_product.critic_native_default";

/// The fixed row catalog, in deterministic emission order.
pub const NATIVE_PRODUCT_ROWS: [&str; 4] = [
    ROW_NATIVE_ONLY,
    ROW_DAP_CLI_NATIVE_ONLY,
    ROW_FORMATTER_NATIVE_DEFAULT,
    ROW_CRITIC_NATIVE_DEFAULT,
];

/// Overall domain verdict for the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProductVerdict {
    /// Every mandatory row passes.
    Ready,
    /// No row blocks, but at least one row only warns.
    Limited,
    /// At least one row fails: a prohibited product path is present.
    Blocked,
    /// At least one row lacks the evidence to decide.
    NotProven,
}

impl NativeProductVerdict {
    /// Lowercase wire/display name.
    pub fn as_str(self) -> &'static str {
        match self {
            NativeProductVerdict::Ready => "ready",
            NativeProductVerdict::Limited => "limited",
            NativeProductVerdict::Blocked => "blocked",
            NativeProductVerdict::NotProven => "not_proven",
        }
    }
}

/// Posture of one catalog row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProductRowStatus {
    /// The row's requirement is met.
    Pass,
    /// The row passes with an advisory limitation.
    Warn,
    /// The row's requirement is violated.
    Fail,
    /// Evidence to decide the row was not available.
    Unverified,
}

impl NativeProductRowStatus {
    /// Lowercase wire/display name.
    pub fn as_str(self) -> &'static str {
        match self {
            NativeProductRowStatus::Pass => "pass",
            NativeProductRowStatus::Warn => "warn",
            NativeProductRowStatus::Fail => "fail",
            NativeProductRowStatus::Unverified => "unverified",
        }
    }
}

impl From<IndicatorStatus> for NativeProductRowStatus {
    fn from(status: IndicatorStatus) -> Self {
        match status {
            IndicatorStatus::Pass => NativeProductRowStatus::Pass,
            IndicatorStatus::Warn => NativeProductRowStatus::Warn,
            IndicatorStatus::Fail => NativeProductRowStatus::Fail,
            // The native-product catalog has no profile-scoped rows today;
            // a not-applicable outcome means the evidence did not decide
            // the row, which must stay visible rather than pass silently.
            IndicatorStatus::NotApplicable | IndicatorStatus::Unverified => {
                NativeProductRowStatus::Unverified
            }
        }
    }
}

/// One posture row in the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProductRow {
    /// Stable row identifier from [`NATIVE_PRODUCT_ROWS`].
    pub id: String,
    /// Evaluated posture.
    pub status: NativeProductRowStatus,
    /// Evidence backing the status, in adapter emission order.
    pub evidence: Vec<EvidenceRef>,
    /// What to change when the row does not pass.
    pub remediation: Option<String>,
    /// Standing claim boundary for the row.
    pub limitation: String,
}

/// The canonical native-product readiness payload.
///
/// Deserialization validates the v1 identity, exact ordered catalog, and earned
/// verdict. It rejects malformed packets instead of normalizing them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "validation::NativeProductReadinessWire")]
pub struct NativeProductReadiness {
    /// Payload kind; always [`NATIVE_PRODUCT_KIND`].
    pub kind: String,
    /// Payload schema version; always [`NATIVE_PRODUCT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Conjunctive, scoreless domain verdict.
    pub verdict: NativeProductVerdict,
    /// One row per catalog entry, in [`NATIVE_PRODUCT_ROWS`] order.
    pub rows: Vec<NativeProductRow>,
}

impl NativeProductReadiness {
    /// Assemble a payload from already-evaluated rows.
    ///
    /// Rows are re-emitted in catalog order regardless of input order, and
    /// the verdict is derived conjunctively from the row statuses. Catalog
    /// rows missing from `rows` are treated as unverified: missing evidence
    /// is `not_proven`, never a silent pass.
    pub fn from_rows(rows: Vec<NativeProductRow>) -> Self {
        let ordered: Vec<NativeProductRow> = NATIVE_PRODUCT_ROWS
            .iter()
            .map(|id| {
                merge_duplicates(&rows, id).unwrap_or_else(|| NativeProductRow {
                    id: (*id).to_string(),
                    status: NativeProductRowStatus::Unverified,
                    evidence: Vec::new(),
                    remediation: Some(
                        "This row was not evaluated; wire its evidence adapter.".to_string(),
                    ),
                    limitation: limitation_for(id).to_string(),
                })
            })
            .collect();
        let verdict = verdict_for(&ordered);
        NativeProductReadiness {
            kind: NATIVE_PRODUCT_KIND.to_string(),
            schema_version: NATIVE_PRODUCT_SCHEMA_VERSION,
            verdict,
            rows: ordered,
        }
    }

    /// Serialize deterministically (struct field order, catalog row order).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Merge every input row for one catalog id into a single row.
///
/// Duplicate ids cannot hide a worse outcome: the merged status is the
/// most severe supplied (fail, then unverified, then warn, then pass),
/// evidence is concatenated in input order, and the first non-empty
/// remediation wins. Unknown ids are ignored by the catalog assembly.
fn merge_duplicates(rows: &[NativeProductRow], id: &str) -> Option<NativeProductRow> {
    let mut merged: Option<NativeProductRow> = None;
    for candidate in rows.iter().filter(|row| row.id == id) {
        match &mut merged {
            None => merged = Some(candidate.clone()),
            Some(existing) => {
                if severity(candidate.status) > severity(existing.status) {
                    existing.status = candidate.status;
                }
                existing.evidence.extend(candidate.evidence.iter().cloned());
                if existing.remediation.is_none() {
                    existing.remediation.clone_from(&candidate.remediation);
                }
            }
        }
    }
    merged
}

fn severity(status: NativeProductRowStatus) -> u8 {
    match status {
        NativeProductRowStatus::Pass => 0,
        NativeProductRowStatus::Warn => 1,
        NativeProductRowStatus::Unverified => 2,
        NativeProductRowStatus::Fail => 3,
    }
}

fn limitation_for(id: &str) -> &'static str {
    match id {
        ROW_NATIVE_ONLY => {
            "First-mile surfaces must not present external Perl tools as product requirements."
        }
        ROW_DAP_CLI_NATIVE_ONLY => "The shipped DAP CLI must not offer a bridge product mode.",
        ROW_FORMATTER_NATIVE_DEFAULT => {
            "Posture only: formatter correctness and corpus proof are engineering evidence."
        }
        ROW_CRITIC_NATIVE_DEFAULT => {
            "Posture only: critic rule proof and false-positive evidence are separate rails."
        }
        _ => "Native-product posture row.",
    }
}

fn verdict_for(rows: &[NativeProductRow]) -> NativeProductVerdict {
    if rows.iter().any(|row| row.status == NativeProductRowStatus::Fail) {
        return NativeProductVerdict::Blocked;
    }
    if rows.iter().any(|row| row.status == NativeProductRowStatus::Unverified) {
        return NativeProductVerdict::NotProven;
    }
    if rows.iter().any(|row| row.status == NativeProductRowStatus::Warn) {
        return NativeProductVerdict::Limited;
    }
    NativeProductVerdict::Ready
}

fn row_from_outcome(id: &str, outcome: Outcome) -> NativeProductRow {
    NativeProductRow {
        id: id.to_string(),
        status: outcome.status.into(),
        evidence: outcome.evidence,
        remediation: outcome.remediation,
        limitation: limitation_for(id).to_string(),
    }
}

/// Evaluate the canonical payload from repository evidence.
///
/// `repo_root` supplies the first-mile and DAP surfaces under `profile`
/// (release candidates must pass [`KwaliteeProfile::Release`] so the
/// release-strict surface rules apply); the formatter and critic default
/// rows consume the native-tooling readiness receipt at
/// `readiness_receipt` (see `cargo xtask native-tooling readiness`), whose
/// freshness is judged against `expected_commit`. No installed artifact is
/// executed and no process is spawned.
pub fn evaluate_native_product(
    repo_root: &Path,
    profile: KwaliteeProfile,
    readiness_receipt: Option<&Path>,
    expected_commit: &str,
) -> NativeProductReadiness {
    let rows = vec![
        row_from_outcome(ROW_NATIVE_ONLY, product_surface::native_only(repo_root, profile)),
        row_from_outcome(ROW_DAP_CLI_NATIVE_ONLY, dap::cli_native_only(repo_root)),
        row_from_outcome(
            ROW_FORMATTER_NATIVE_DEFAULT,
            readiness::formatter_native_default(readiness_receipt, expected_commit),
        ),
        row_from_outcome(
            ROW_CRITIC_NATIVE_DEFAULT,
            readiness::critic_native_default(readiness_receipt, expected_commit),
        ),
    ];
    NativeProductReadiness::from_rows(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, status: NativeProductRowStatus) -> NativeProductRow {
        NativeProductRow {
            id: id.to_string(),
            status,
            evidence: Vec::new(),
            remediation: None,
            limitation: limitation_for(id).to_string(),
        }
    }

    fn all_rows(status: NativeProductRowStatus) -> Vec<NativeProductRow> {
        NATIVE_PRODUCT_ROWS.iter().map(|id| row(id, status)).collect()
    }

    #[test]
    fn all_pass_rows_are_ready() {
        let payload = NativeProductReadiness::from_rows(all_rows(NativeProductRowStatus::Pass));
        assert_eq!(payload.verdict, NativeProductVerdict::Ready);
        assert_eq!(payload.kind, NATIVE_PRODUCT_KIND);
        assert_eq!(payload.schema_version, NATIVE_PRODUCT_SCHEMA_VERSION);
        assert_eq!(payload.rows.len(), NATIVE_PRODUCT_ROWS.len());
    }

    #[test]
    fn one_failure_blocks_even_when_the_rest_pass() {
        let mut rows = all_rows(NativeProductRowStatus::Pass);
        rows[2].status = NativeProductRowStatus::Fail;
        let payload = NativeProductReadiness::from_rows(rows);
        assert_eq!(payload.verdict, NativeProductVerdict::Blocked);
    }

    #[test]
    fn missing_row_is_not_proven_never_ready() {
        let rows = vec![row(ROW_NATIVE_ONLY, NativeProductRowStatus::Pass)];
        let payload = NativeProductReadiness::from_rows(rows);
        assert_eq!(payload.verdict, NativeProductVerdict::NotProven);
        assert_eq!(payload.rows.len(), NATIVE_PRODUCT_ROWS.len());
        assert!(
            payload
                .rows
                .iter()
                .filter(|row| row.id != ROW_NATIVE_ONLY)
                .all(|row| row.status == NativeProductRowStatus::Unverified)
        );
    }

    #[test]
    fn warning_limits_without_blocking() {
        let mut rows = all_rows(NativeProductRowStatus::Pass);
        rows[0].status = NativeProductRowStatus::Warn;
        let payload = NativeProductReadiness::from_rows(rows);
        assert_eq!(payload.verdict, NativeProductVerdict::Limited);
    }

    #[test]
    fn blocked_outranks_not_proven() {
        let mut rows = all_rows(NativeProductRowStatus::Unverified);
        rows[1].status = NativeProductRowStatus::Fail;
        let payload = NativeProductReadiness::from_rows(rows);
        assert_eq!(payload.verdict, NativeProductVerdict::Blocked);
    }

    #[test]
    fn rows_emit_in_catalog_order_regardless_of_input_order() {
        let mut rows = all_rows(NativeProductRowStatus::Pass);
        rows.reverse();
        let payload = NativeProductReadiness::from_rows(rows);
        let ids: Vec<&str> = payload.rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, NATIVE_PRODUCT_ROWS);
    }

    #[test]
    fn duplicate_rows_cannot_hide_a_failure() {
        let mut rows = all_rows(NativeProductRowStatus::Pass);
        rows.push(row(ROW_NATIVE_ONLY, NativeProductRowStatus::Fail));
        let payload = NativeProductReadiness::from_rows(rows);
        assert_eq!(payload.verdict, NativeProductVerdict::Blocked);
        let native_only =
            payload.rows.iter().find(|row| row.id == ROW_NATIVE_ONLY).expect("row present");
        assert_eq!(native_only.status, NativeProductRowStatus::Fail);
    }

    #[test]
    fn payload_json_round_trips_and_rejects_unknown_fields() {
        let payload = NativeProductReadiness::from_rows(all_rows(NativeProductRowStatus::Pass));
        let json = payload.to_json().expect("payload serializes");
        let parsed: NativeProductReadiness =
            serde_json::from_str(&json).expect("payload parses back");
        assert_eq!(parsed, payload);
        assert!(json.contains("\"kind\": \"native_product_readiness\""));
        assert!(json.contains("\"verdict\": \"ready\""));

        let unknown = json.replacen("{", "{\"unexpected\": true,", 1);
        assert!(
            serde_json::from_str::<NativeProductReadiness>(&unknown).is_err(),
            "unknown top-level fields must be rejected"
        );
    }

    #[test]
    fn evaluate_without_receipt_is_not_proven() {
        // An empty directory has no first-mile surfaces and no readiness
        // receipt: posture cannot be decided, so nothing may read ready.
        let dir = tempfile::tempdir().expect("tempdir");
        let payload = evaluate_native_product(dir.path(), KwaliteeProfile::Pr, None, "unknown");
        assert_eq!(payload.verdict, NativeProductVerdict::NotProven);
        assert!(payload.rows.iter().any(|row| row.id == ROW_FORMATTER_NATIVE_DEFAULT
            && row.status == NativeProductRowStatus::Unverified));
    }
}
