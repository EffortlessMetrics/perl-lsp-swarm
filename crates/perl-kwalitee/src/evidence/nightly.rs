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

use crate::evidence::{Outcome, apply_freshness};
use crate::indicator::{EvidenceRef, IndicatorStatus};

/// A nightly receipt that records the commit it was generated at.
///
/// Every nightly generator stamps `commit` with `git rev-parse HEAD` (falling
/// back to `"unknown"`), which lets these advisory readers apply the same
/// freshness rule as the readiness and quality-gate readers.
trait NightlyReceipt {
    fn commit(&self) -> &str;
}

/// Read + deserialize a JSON receipt, returning `None` on any error.
fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Shared shell: resolve the receipt path, read it, and map a health predicate
/// to an advisory outcome (Pass / Warn / Unverified — never Fail).
///
/// A receipt stamped at a commit other than `expected_commit` is stale, and a
/// stale `Pass` is downgraded to `Warn` exactly as it is for the readiness and
/// quality-gate receipts: these indicators are advisory, so the downgrade can
/// never turn into a mandatory failure, but the scoreboard must not report a
/// clean pass on evidence that does not describe the current tree.
fn advisory<T, F>(
    path: Option<&Path>,
    expected_commit: &str,
    command: &str,
    fix: &str,
    healthy: F,
) -> Outcome
where
    T: for<'de> Deserialize<'de> + NightlyReceipt,
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
    let status = apply_freshness(
        if ok { IndicatorStatus::Pass } else { IndicatorStatus::Warn },
        receipt.commit(),
        expected_commit,
        "commit",
        &mut evidence,
    );

    match (ok, status) {
        (_, IndicatorStatus::Pass) => Outcome::pass(evidence),
        // Healthy but stale: the content is fine, the provenance is not.
        (true, _) => Outcome::warn(
            evidence,
            format!("Receipt is stale. Regenerate it at the current HEAD: `{command}`."),
        ),
        (false, _) => Outcome::warn(evidence, fix.to_string()),
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

impl NightlyReceipt for CorpusReceipt {
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
    commit: String,
    #[serde(default)]
    findings_count: u64,
    #[serde(default)]
    suppressed_findings_count: u64,
    #[serde(default)]
    files_with_parse_errors: u64,
}

impl NightlyReceipt for CriticFalsePositiveReceipt {
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
    commit: String,
    #[serde(default)]
    external_only_count: u64,
}

impl NightlyReceipt for ExternalOnlyReceipt {
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
        assert_eq!(formatter_corpus_idempotent(None, "abc").status, IndicatorStatus::Unverified);
        assert_eq!(critic_no_false_positives(None, "abc").status, IndicatorStatus::Unverified);
        assert_eq!(formatter_perltidy_compat(None, "abc").status, IndicatorStatus::Unverified);
        assert_eq!(critic_perlcritic_compat(None, "abc").status, IndicatorStatus::Unverified);
    }

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
    fn critic_false_positive_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(
            &d,
            "fp1.json",
            "{\"commit\":\"abc\",\"findings_count\":0,\"suppressed_findings_count\":0,\
             \"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad = write(
            &d,
            "fp2.json",
            "{\"commit\":\"abc\",\"findings_count\":2,\"suppressed_findings_count\":0,\
             \"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn external_only_pass_and_warn() {
        let d = tempfile::tempdir().expect("tmp");
        let ok = write(&d, "e1.json", "{\"commit\":\"abc\",\"external_only_count\":0}");
        assert_eq!(formatter_perltidy_compat(Some(&ok), "abc").status, IndicatorStatus::Pass);
        assert_eq!(critic_perlcritic_compat(Some(&ok), "abc").status, IndicatorStatus::Pass);
        let bad = write(&d, "e2.json", "{\"commit\":\"abc\",\"external_only_count\":3}");
        assert_eq!(formatter_perltidy_compat(Some(&bad), "abc").status, IndicatorStatus::Warn);
    }

    #[test]
    fn stale_receipt_downgrades_pass_to_warn() {
        // A healthy receipt generated at an older commit is not evidence about
        // the current tree: it must not report a clean advisory pass.
        let d = tempfile::tempdir().expect("tmp");
        let corpus =
            write(&d, "s1.json", "{\"commit\":\"oldsha\",\"passed\":true,\"files_checked\":42}");
        let outcome = formatter_corpus_idempotent(Some(&corpus), "newsha");
        assert_eq!(outcome.status, IndicatorStatus::Warn);
        assert!(
            outcome.evidence.iter().any(|e| e.value.contains("stale receipt")),
            "stale receipt must be named in the evidence: {:?}",
            outcome.evidence
        );

        let fp = write(
            &d,
            "s2.json",
            "{\"commit\":\"oldsha\",\"findings_count\":0,\"suppressed_findings_count\":0,\
             \"files_with_parse_errors\":0}",
        );
        assert_eq!(critic_no_false_positives(Some(&fp), "newsha").status, IndicatorStatus::Warn);

        let ext = write(&d, "s3.json", "{\"commit\":\"oldsha\",\"external_only_count\":0}");
        assert_eq!(formatter_perltidy_compat(Some(&ext), "newsha").status, IndicatorStatus::Warn);
        assert_eq!(critic_perlcritic_compat(Some(&ext), "newsha").status, IndicatorStatus::Warn);
    }

    #[test]
    fn stale_receipt_remediation_says_regenerate() {
        // Healthy-but-stale is a provenance problem, so the remediation must
        // point at regenerating the receipt, not at fixing the formatter.
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "r.json", "{\"commit\":\"oldsha\",\"passed\":true,\"files_checked\":1}");
        let remediation =
            formatter_corpus_idempotent(Some(&p), "newsha").remediation.expect("remediation");
        assert!(remediation.contains("stale"), "{remediation}");
        assert!(remediation.contains("native-format corpus"), "{remediation}");
    }

    #[test]
    fn unhealthy_receipt_keeps_its_own_remediation() {
        // An unhealthy receipt at the current HEAD is a real finding; its
        // remediation must stay the substantive fix.
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "u.json", "{\"commit\":\"abc\",\"passed\":false,\"files_checked\":1}");
        let remediation =
            formatter_corpus_idempotent(Some(&p), "abc").remediation.expect("remediation");
        assert!(remediation.contains("idempotent"), "{remediation}");
    }

    #[test]
    fn unknown_or_absent_commit_does_not_downgrade() {
        // "unknown" is the wrapper's placeholder for an unresolvable HEAD, and
        // an empty expected commit means freshness was never asserted. Neither
        // is a real commit to compare against, so a healthy receipt still
        // passes — matching the readiness and quality-gate readers.
        let d = tempfile::tempdir().expect("tmp");
        let p = write(&d, "k.json", "{\"commit\":\"realsha\",\"passed\":true,\"files_checked\":7}");
        assert_eq!(formatter_corpus_idempotent(Some(&p), "unknown").status, IndicatorStatus::Pass);
        assert_eq!(formatter_corpus_idempotent(Some(&p), "").status, IndicatorStatus::Pass);

        // A receipt with no commit field recorded is likewise not comparable.
        let bare = write(&d, "b.json", "{\"passed\":true,\"files_checked\":7}");
        assert_eq!(
            formatter_corpus_idempotent(Some(&bare), "newsha").status,
            IndicatorStatus::Pass
        );
    }
}
