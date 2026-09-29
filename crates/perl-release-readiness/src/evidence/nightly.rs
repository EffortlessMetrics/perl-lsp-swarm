//! Nightly-only advisory receipt readers.
//!
//! These back the broad, receipt-heavy advisory indicators that only run under
//! the nightly profile. Each reads a JSON receipt another xtask task produced
//! and reports a healthy/unhealthy verdict. Because they are advisory
//! (non-mandatory), an unhealthy result is a `Warn`, not a `Fail`; a missing
//! receipt is `Unverified`.
//!
//! Receipt field names are taken verbatim from the xtask writer structs:
//! - `native_format_corpus` — `passed: bool`
//!   (`xtask/src/tasks/native_format.rs::NativeFormatCorpusReceipt`).
//! - `native_critic_check` (false-positive fixtures) — `findings_count`,
//!   `suppressed_findings_count`, `files_with_parse_errors`
//!   (`xtask/src/tasks/native_critic.rs::NativeCriticCheckReceipt`).
//! - `native_format_perltidy_compat` / `native_tooling_perlcritic_compat` —
//!   `external_only_count`.

use std::path::Path;

use serde::Deserialize;

use crate::evidence::{Outcome, is_stale};
use crate::indicator::EvidenceRef;

/// Read + deserialize a JSON receipt, returning `None` on any error.
fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Shared shell: resolve the receipt path, read it, and map a health predicate
/// to an advisory outcome (Pass / Warn / Unverified — never Fail).
///
/// Receipt freshness is enforced: a healthy receipt whose stamped `commit`
/// differs from `expected_commit` is downgraded from `Pass` to `Warn`, matching
/// the rule already applied by [`crate::evidence::readiness`] and
/// [`crate::evidence::quality_gate`].
fn advisory<T, F>(
    path: Option<&Path>,
    expected_commit: &str,
    command: &str,
    fix: &str,
    healthy: F,
) -> Outcome
where
    T: ReceiptCommit + for<'de> Deserialize<'de>,
    F: FnOnce(&T) -> (bool, String),
{
    let Some(path) = path else {
        return Outcome::unverified(
            vec![EvidenceRef::command(command.to_string())],
            format!("Provide the receipt (run `{command}`). {fix}"),
        );
    };
    let display = path.display().to_string();
    let receipt_ev = EvidenceRef::receipt(display.clone());

    let Some(receipt) = read::<T>(path) else {
        return Outcome::unverified(
            vec![receipt_ev],
            format!("Receipt at {display} is missing or unparseable. {fix}"),
        );
    };

    let (ok, detail) = healthy(&receipt);
    let mut evidence = vec![receipt_ev, EvidenceRef::new("note", detail)];

    // Freshness: a healthy receipt from a different commit is not trustworthy.
    let stale = is_stale(receipt.commit(), expected_commit);
    if stale {
        evidence.push(EvidenceRef::new(
            "note",
            format!("stale receipt: commit {} != HEAD {}", receipt.commit(), expected_commit),
        ));
    }

    // A healthy receipt from a different commit is not trustworthy as a pass,
    // so downgrade to warn. An unhealthy receipt is warn regardless of freshness.
    if ok && !stale { Outcome::pass(evidence) } else { Outcome::warn(evidence, fix.to_string()) }
}

/// Projection of the `commit` field stamped by every nightly receipt generator.
/// Each receipt struct implements this so [`advisory`] can read the stamped
/// commit without a generic per-struct accessor.
trait ReceiptCommit {
    fn commit(&self) -> &str;
}

#[derive(Debug, Deserialize)]
struct CorpusReceipt {
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    files_checked: u64,
    #[serde(default)]
    commit: String,
}

impl ReceiptCommit for CorpusReceipt {
    fn commit(&self) -> &str {
        &self.commit
    }
}

/// `formatter.corpus_idempotent`.
pub(crate) fn formatter_corpus_idempotent(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<CorpusReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-format corpus",
        "Fix files where the native formatter is not idempotent or changes the parse.",
        |r| (r.passed, format!("passed={} over {} files", r.passed, r.files_checked)),
    )
}

#[derive(Debug, Deserialize)]
struct CriticFalsePositiveReceipt {
    #[serde(default)]
    findings_count: u64,
    #[serde(default)]
    suppressed_findings_count: u64,
    #[serde(default)]
    files_with_parse_errors: u64,
    #[serde(default)]
    commit: String,
}

impl ReceiptCommit for CriticFalsePositiveReceipt {
    fn commit(&self) -> &str {
        &self.commit
    }
}

/// `critic.no_false_positives`.
pub(crate) fn critic_no_false_positives(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<CriticFalsePositiveReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-critic check (false-positive fixtures)",
        "Eliminate findings/parse errors the native critic raises on known-clean code.",
        |r| {
            let clean = r.findings_count == 0
                && r.suppressed_findings_count == 0
                && r.files_with_parse_errors == 0;
            (
                clean,
                format!(
                    "findings={} suppressed={} parse_errors={}",
                    r.findings_count, r.suppressed_findings_count, r.files_with_parse_errors
                ),
            )
        },
    )
}

#[derive(Debug, Deserialize)]
struct ExternalOnlyReceipt {
    #[serde(default)]
    external_only_count: u64,
    #[serde(default)]
    commit: String,
}

impl ReceiptCommit for ExternalOnlyReceipt {
    fn commit(&self) -> &str {
        &self.commit
    }
}

/// `formatter.perltidy_compat_no_external_only`.
pub(crate) fn formatter_perltidy_compat(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<ExternalOnlyReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-format perltidy-compat",
        "Close or re-classify the external-only perltidy options.",
        |r| (r.external_only_count == 0, format!("external_only_count={}", r.external_only_count)),
    )
}

/// `critic.perlcritic_compat_no_external_only`.
pub(crate) fn critic_perlcritic_compat(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<ExternalOnlyReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-tooling perlcritic-compat",
        "Close or re-classify the external-only perlcritic rules.",
        |r| (r.external_only_count == 0, format!("external_only_count={}", r.external_only_count)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::IndicatorStatus;
    use std::fs;

    fn write(dir: &tempfile::TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let p = dir.path().join(name);
        fs::write(&p, body).expect("write");
        p
    }

    #[test]
    fn missing_receipts_are_unverified() {
        assert_eq!(formatter_corpus_idempotent(None, "").status, IndicatorStatus::Unverified);
        assert_eq!(critic_no_false_positives(None, "").status, IndicatorStatus::Unverified);
        assert_eq!(formatter_perltidy_compat(None, "").status, IndicatorStatus::Unverified);
        assert_eq!(critic_perlcritic_compat(None, "").status, IndicatorStatus::Unverified);
    }

    #[test]
    fn corpus_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(&d, "c1.json", "{\"passed\":true,\"files_checked\":42,\"commit\":\"abc\"}");
        assert_eq!(formatter_corpus_idempotent(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad =
            write(&d, "c2.json", "{\"passed\":false,\"files_checked\":42,\"commit\":\"abc\"}");
        assert_eq!(formatter_corpus_idempotent(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn critic_false_positive_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(
            &d,
            "fp1.json",
            "{\"findings_count\":0,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0,\"commit\":\"abc\"}",
        );
        assert_eq!(critic_no_false_positives(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad = write(
            &d,
            "fp2.json",
            "{\"findings_count\":2,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0,\"commit\":\"abc\"}",
        );
        assert_eq!(critic_no_false_positives(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn external_only_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(&d, "e1.json", "{\"external_only_count\":0,\"commit\":\"abc\"}");
        assert_eq!(formatter_perltidy_compat(Some(&ok), "abc").status, IndicatorStatus::Pass);
        assert_eq!(critic_perlcritic_compat(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad = write(&d, "e2.json", "{\"external_only_count\":3,\"commit\":\"abc\"}");
        assert_eq!(formatter_perltidy_compat(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn stale_receipt_downgrades_pass_to_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok =
            write(&d, "fresh.json", "{\"passed\":true,\"files_checked\":42,\"commit\":\"oldsha\"}");
        assert_eq!(formatter_corpus_idempotent(Some(&ok), "newsha").status, IndicatorStatus::Warn);
        // Critic false-positive: clean receipt but stale commit.
        let clean = write(
            &d,
            "fpfresh.json",
            "{\"findings_count\":0,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0,\"commit\":\"oldsha\"}",
        );
        assert_eq!(critic_no_false_positives(Some(&clean), "newsha").status, IndicatorStatus::Warn);
        // External-only: zero count but stale commit.
        let ext = write(&d, "ext.json", "{\"external_only_count\":0,\"commit\":\"oldsha\"}");
        assert_eq!(formatter_perltidy_compat(Some(&ext), "newsha").status, IndicatorStatus::Warn);
        assert_eq!(critic_perlcritic_compat(Some(&ext), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn unknown_expected_commit_does_not_downgrade() {
        let d = tempfile::tempdir().expect("tmp");
        let ok =
            write(&d, "ok.json", "{\"passed\":true,\"files_checked\":42,\"commit\":\"realsha\"}");
        assert_eq!(formatter_corpus_idempotent(Some(&ok), "unknown").status, IndicatorStatus::Pass);
    }

    #[test]
    fn missing_commit_field_does_not_downgrade() {
        // A receipt without a stamped commit is trusted, matching the readiness
        // and quality-gate readers. Generators that omit the stamp should be
        // fixed separately, not silently downgraded.
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(&d, "nostamp.json", "{\"passed\":true,\"files_checked\":42}");
        assert_eq!(formatter_corpus_idempotent(Some(&ok), "newsha").status, IndicatorStatus::Pass);
    }

    #[test]
    fn stale_receipt_with_unhealthy_remains_warn() {
        // An unhealthy receipt stays Warn regardless of freshness.
        let d = tempfile::tempdir().expect("tmp");
        let bad =
            write(&d, "bad.json", "{\"passed\":false,\"files_checked\":42,\"commit\":\"oldsha\"}");
        assert_eq!(formatter_corpus_idempotent(Some(&bad), "newsha").status, IndicatorStatus::Warn);
    }
}
