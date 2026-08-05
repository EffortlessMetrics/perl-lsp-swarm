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
//!
//! ## Freshness
//!
//! All four generators stamp a `commit` field via `git rev-parse HEAD`. The
//! same rule applied by the readiness and quality-gate readers applies here:
//! when the receipt commit differs from the expected HEAD the indicator is
//! downgraded from `pass` to `warn` (stale evidence is not trusted as a pass).
//! An empty or `"unknown"` expected commit skips the check so that callers
//! without git access do not manufacture spurious warns.

use std::path::Path;

use serde::Deserialize;

use crate::evidence::Outcome;
use crate::indicator::{EvidenceRef, IndicatorStatus};

/// Read + deserialize a JSON receipt, returning `None` on any error.
fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Returns `true` when `receipt_commit` describes a different commit than
/// `expected_commit` and both values are non-empty and non-`"unknown"`.
///
/// This is the canonical staleness predicate shared by all receipt readers in
/// this module. Keeping it in one place prevents the drift that caused nightly
/// readers to lack freshness checks in the first place.
fn is_receipt_stale(expected_commit: &str, receipt_commit: &str) -> bool {
    !expected_commit.is_empty()
        && expected_commit != "unknown"
        && !receipt_commit.is_empty()
        && receipt_commit != expected_commit
}

/// Shared shell: resolve the receipt path, read it, and map a health predicate
/// to an advisory outcome (Pass / Warn / Unverified — never Fail).
///
/// `expected_commit` is the HEAD SHA the caller obtained from
/// [`KwaliteeOptions::commit`](crate::KwaliteeOptions). A non-empty, non-
/// `"unknown"` value enables the staleness check: a receipt whose embedded
/// `commit` differs from `expected_commit` has a passing verdict downgraded
/// to `Warn`.
fn advisory<T, F>(
    path: Option<&Path>,
    expected_commit: &str,
    command: &str,
    fix: &str,
    commit_of: impl Fn(&T) -> &str,
    healthy: F,
) -> Outcome
where
    T: for<'de> Deserialize<'de>,
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
    let mut status = if ok { IndicatorStatus::Pass } else { IndicatorStatus::Warn };
    let mut evidence = vec![receipt_ev, EvidenceRef::new("note", detail)];

    // Freshness: a receipt from a different commit is not trustworthy as a pass.
    let receipt_commit = commit_of(&receipt);
    if is_receipt_stale(expected_commit, receipt_commit) {
        evidence.push(EvidenceRef::new(
            "note",
            format!("stale receipt: commit {} != HEAD {}", receipt_commit, expected_commit),
        ));
        if status == IndicatorStatus::Pass {
            status = IndicatorStatus::Warn;
        }
    }

    if status == IndicatorStatus::Pass {
        Outcome::pass(evidence)
    } else {
        Outcome::warn(evidence, fix.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct CorpusReceipt {
    #[serde(default)]
    commit: String,
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    files_checked: u64,
}

/// `formatter.corpus_idempotent`.
pub(crate) fn formatter_corpus_idempotent(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<CorpusReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-format corpus",
        "Fix files where the native formatter is not idempotent or changes the parse.",
        |r| &r.commit,
        |r| (r.passed, format!("passed={} over {} files", r.passed, r.files_checked)),
    )
}

#[derive(Debug, Deserialize)]
struct CriticFalsePositiveReceipt {
    #[serde(default)]
    commit: String,
    #[serde(default)]
    findings_count: u64,
    #[serde(default)]
    suppressed_findings_count: u64,
    #[serde(default)]
    files_with_parse_errors: u64,
}

/// `critic.no_false_positives`.
pub(crate) fn critic_no_false_positives(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<CriticFalsePositiveReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-critic check (false-positive fixtures)",
        "Eliminate findings/parse errors the native critic raises on known-clean code.",
        |r| &r.commit,
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
    commit: String,
    #[serde(default)]
    external_only_count: u64,
}

/// `formatter.perltidy_compat_no_external_only`.
pub(crate) fn formatter_perltidy_compat(path: Option<&Path>, expected_commit: &str) -> Outcome {
    advisory::<ExternalOnlyReceipt, _>(
        path,
        expected_commit,
        "cargo xtask native-format perltidy-compat",
        "Close or re-classify the external-only perltidy options.",
        |r| &r.commit,
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
        |r| &r.commit,
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

    // ── missing-receipt ────────────────────────────────────────────────────────

    #[test]
    fn missing_receipts_are_unverified() {
        assert_eq!(formatter_corpus_idempotent(None, "").status, IndicatorStatus::Unverified);
        assert_eq!(critic_no_false_positives(None, "").status, IndicatorStatus::Unverified);
        assert_eq!(formatter_perltidy_compat(None, "").status, IndicatorStatus::Unverified);
        assert_eq!(critic_perlcritic_compat(None, "").status, IndicatorStatus::Unverified);
    }

    // ── corpus ─────────────────────────────────────────────────────────────────

    #[test]
    fn corpus_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(&d, "c1.json", "{\"commit\":\"abc\",\"passed\":true,\"files_checked\":42}");
        assert_eq!(formatter_corpus_idempotent(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad =
            write(&d, "c2.json", "{\"commit\":\"abc\",\"passed\":false,\"files_checked\":42}");
        assert_eq!(formatter_corpus_idempotent(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn corpus_stale_receipt_downgrades_pass_to_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "c.json", "{\"commit\":\"oldsha\",\"passed\":true,\"files_checked\":10}");
        assert_eq!(formatter_corpus_idempotent(Some(&p), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn corpus_stale_receipt_does_not_upgrade_warn() {
        // A failing receipt should stay Warn even when stale — staleness never
        // escalates to Fail.
        let d = tempfile::tempdir().expect("tmp");
        let p =
            write(&d, "c.json", "{\"commit\":\"oldsha\",\"passed\":false,\"files_checked\":10}");
        assert_eq!(formatter_corpus_idempotent(Some(&p), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn corpus_unknown_expected_commit_does_not_downgrade() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "c.json", "{\"commit\":\"realsha\",\"passed\":true,\"files_checked\":5}");
        assert_eq!(formatter_corpus_idempotent(Some(&p), "unknown").status, IndicatorStatus::Pass);
    }

    #[test]
    fn corpus_empty_expected_commit_does_not_downgrade() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "c.json", "{\"commit\":\"realsha\",\"passed\":true,\"files_checked\":5}");
        assert_eq!(formatter_corpus_idempotent(Some(&p), "").status, IndicatorStatus::Pass);
    }

    // ── critic false-positive ──────────────────────────────────────────────────

    #[test]
    fn critic_false_positive_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(
            &d,
            "fp1.json",
            "{\"commit\":\"abc\",\"findings_count\":0,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad = write(
            &d,
            "fp2.json",
            "{\"commit\":\"abc\",\"findings_count\":2,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn critic_false_positive_stale_receipt_downgrades_pass_to_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(
            &d,
            "fp.json",
            "{\"commit\":\"oldsha\",\"findings_count\":0,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&p), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn critic_false_positive_unknown_expected_commit_does_not_downgrade() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(
            &d,
            "fp.json",
            "{\"commit\":\"realsha\",\"findings_count\":0,\"suppressed_findings_count\":0,\"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&p), "unknown").status, IndicatorStatus::Pass);
    }

    // ── external-only (perltidy + perlcritic) ─────────────────────────────────

    #[test]
    fn external_only_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(&d, "e1.json", "{\"commit\":\"abc\",\"external_only_count\":0}");
        assert_eq!(formatter_perltidy_compat(Some(&ok), "abc").status, IndicatorStatus::Pass);
        assert_eq!(critic_perlcritic_compat(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad = write(&d, "e2.json", "{\"commit\":\"abc\",\"external_only_count\":3}");
        assert_eq!(formatter_perltidy_compat(Some(&bad), "abc").status, IndicatorStatus::Warn);
        assert_eq!(critic_perlcritic_compat(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn perltidy_compat_stale_receipt_downgrades_pass_to_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "e.json", "{\"commit\":\"oldsha\",\"external_only_count\":0}");
        assert_eq!(formatter_perltidy_compat(Some(&p), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn perlcritic_compat_stale_receipt_downgrades_pass_to_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "e.json", "{\"commit\":\"oldsha\",\"external_only_count\":0}");
        assert_eq!(critic_perlcritic_compat(Some(&p), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn external_only_unknown_expected_commit_does_not_downgrade() {
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "e.json", "{\"commit\":\"realsha\",\"external_only_count\":0}");
        assert_eq!(formatter_perltidy_compat(Some(&p), "unknown").status, IndicatorStatus::Pass);
        assert_eq!(critic_perlcritic_compat(Some(&p), "unknown").status, IndicatorStatus::Pass);
    }

    // ── is_receipt_stale unit tests ────────────────────────────────────────────

    #[test]
    fn stale_predicate_coverage() {
        // Different commits → stale
        assert!(is_receipt_stale("head1", "head2"));
        // Same commit → fresh
        assert!(!is_receipt_stale("abc", "abc"));
        // Empty expected → skip check
        assert!(!is_receipt_stale("", "oldsha"));
        // "unknown" expected → skip check
        assert!(!is_receipt_stale("unknown", "oldsha"));
        // Empty receipt commit → skip check
        assert!(!is_receipt_stale("head", ""));
    }
}
