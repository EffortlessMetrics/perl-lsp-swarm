//! Table-driven contradiction controls for invocation reconciliation (#8191).

use perl_core_harness_types::{
    HarnessMode, RUNNER_RECORD_SCHEMA_VERSION, RunnerRecord, RunnerStatus,
    result_axes::EvidenceValidity,
};

use color_eyre::eyre::Context;

use crate::invocation_reconcile::{
    InvocationAuthority, InvocationBinding, InvocationDomainOutcome, ReconcileInvocationInput,
    reconcile_invocation,
};
use crate::transition::TerminalProcessOutcome;

fn binding(mode: HarnessMode, path: &str) -> InvocationBinding {
    InvocationBinding {
        run_id: "run-fixture".to_string(),
        invocation_id: path.to_string(),
        expected_path: path.to_string(),
        mode,
        plan_payload_digest: "sha256:plan".to_string(),
        source_logical_id: "subject-fixture".to_string(),
        source_raw_digest: "sha256:raw".to_string(),
        source_effective_digest: "sha256:effective".to_string(),
        runner_artifact_identity: "artifact-fixture".to_string(),
    }
}

fn record(
    mode: HarnessMode,
    path: &str,
    status: RunnerStatus,
    passed: usize,
    total: usize,
) -> RunnerRecord {
    RunnerRecord {
        schema_version: RUNNER_RECORD_SCHEMA_VERSION.to_string(),
        mode: mode.as_str().to_string(),
        path: path.to_string(),
        status,
        assertions_passed: passed,
        assertions_total: total,
        bucket: None,
        first_diagnostic: None,
        mechanism: None,
        scaffold_stamp: None,
        semantic_boundaries: Vec::new(),
    }
}

fn reconcile<'a>(
    mode: HarnessMode,
    path: &str,
    terminal: TerminalProcessOutcome,
    stdout: &'a str,
    stdout_truncated: bool,
    records: &'a [RunnerRecord],
) -> crate::invocation_reconcile::SettledInvocationReceipt {
    reconcile_invocation(ReconcileInvocationInput {
        binding: binding(mode, path),
        authority: InvocationAuthority::UpstreamTest,
        terminal,
        stdout_truncated,
        stdout,
        semantic_records: records,
    })
}

#[test]
fn complete_valid_pass_is_authoritative() {
    let stdout = "1..1\nok 1 - fine\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Valid);
    assert_eq!(rec.derived.domain_outcome, InvocationDomainOutcome::CompletedCleanly);
}

#[test]
fn complete_valid_product_failure_is_distinct_from_not_proven() {
    let stdout = "1..1\nnot ok 1 - broke\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Fail, 0, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Valid);
    assert_eq!(rec.derived.domain_outcome, InvocationDomainOutcome::CompletedWithFailures);
}

#[test]
fn pass_record_with_not_ok_tap_is_invalid() {
    let stdout = "1..1\nnot ok 1 - broke\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Pass, 1, 1)],
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
    assert!(rec.derived.reasons.iter().any(|reason| {
        reason.contains("pass while TAP") || reason.contains("disagree with TAP")
    }));
}

#[test]
fn pass_record_with_bailout_is_not_proven() {
    let stdout = "1..2\nok 1\nBail out! stopped\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Pass, 1, 2)],
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn pass_record_with_truncated_tap_is_not_proven() {
    let stdout = "1..2\nok 1 - only one\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Pass, 1, 2)],
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn pass_record_with_nonzero_exit_is_not_proven() {
    let stdout = "1..1\nok 1\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::NonZeroExit { code: 9 },
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Pass, 1, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::NotProven);
}

#[test]
fn zero_exit_with_tap_record_disagreement_is_invalid() {
    let stdout = "1..1\nok 1\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Fail, 0, 1)],
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn assertion_total_mismatch_is_invalid() {
    let stdout = "1..2\nok 1 - a\nok 2 - b\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Pass, 2, 1)],
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn wrong_mode_is_rejected() {
    let stdout = "1..1\nok 1\n";
    let mut wrong = record(HarnessMode::Compile, "base/bad.t", RunnerStatus::Pass, 1, 1);
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        std::slice::from_ref(&wrong),
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Invalid);
    wrong.mode = HarnessMode::Parse.as_str().to_string();
    let _ = wrong;
}

#[test]
fn duplicate_pass_records_fail_closed() {
    let stdout = "1..1\nok 1\n";
    let records = [
        record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1),
        record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1),
    ];
    let rec = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &records,
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn pass_and_fail_records_for_one_invocation_fail_closed() {
    let stdout = "1..1\nok 1\n";
    let records = [
        record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1),
        record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Fail, 0, 1),
    ];
    let rec = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &records,
    );
    assert_ne!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn missing_record_with_successful_tap_is_not_proven() {
    let stdout = "1..1\nok 1\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::NotProven);
}

#[test]
fn output_truncation_after_valid_prefix_is_not_proven() {
    let stdout = "1..1\nok 1 - looks fine\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        true,
        &[record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::NotProven);
}

#[test]
fn direct_probe_authority_cannot_settle_upstream_product_evidence() {
    let stdout = "1..1\nok 1\n";
    let receipt = reconcile_invocation(ReconcileInvocationInput {
        binding: binding(HarnessMode::Parse, "base/ok.t"),
        authority: InvocationAuthority::DirectProbe,
        terminal: TerminalProcessOutcome::CleanExit,
        stdout_truncated: false,
        stdout,
        semantic_records: &[record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1)],
    });
    assert_eq!(receipt.derived.evidence_result, EvidenceValidity::Invalid);
}

#[test]
fn recognized_skip_is_distinct_from_invalid_evidence() {
    let stdout = "1..1\nok 1 - later # SKIP platform\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/skip.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/skip.t", RunnerStatus::Pass, 0, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Valid);
    assert_eq!(rec.derived.domain_outcome, InvocationDomainOutcome::RecognizedSkip);
}

#[test]
fn repeated_settlement_is_byte_identical() -> color_eyre::Result<()> {
    let stdout = "1..1\nok 1\n";
    let records = [record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1)];
    let first = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &records,
    );
    let second = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &records,
    );
    let first_json = serde_json::to_string(&first).context("serialize first receipt")?;
    let second_json = serde_json::to_string(&second).context("serialize second receipt")?;
    assert_eq!(first_json, second_json);
    Ok(())
}

#[test]
fn atomic_publication_writes_only_complete_receipt() -> color_eyre::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = crate::invocation_reconcile::invocation_receipt_path(
        temp.path(),
        "run-fixture",
        "base/ok.t",
    )?;
    let stdout = "1..1\nok 1\n";
    let receipt = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1)],
    );
    crate::invocation_reconcile::publish_settled_invocation_receipt(&path, &receipt)?;
    assert!(path.is_file());
    assert!(!path.with_extension("json.part").exists());
    let decoded: crate::invocation_reconcile::SettledInvocationReceipt =
        serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    assert_eq!(decoded, receipt);
    Ok(())
}

#[test]
fn fail_record_with_clean_matching_tap_is_invalid() {
    let stdout = "1..1\nok 1\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/bad.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/bad.t", RunnerStatus::Fail, 1, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Invalid);
}

#[test]
fn unparsed_stdout_lines_are_not_authoritative() {
    let stdout = "1..1\nok 1 - fine\nfuture record: opaque\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/ok.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/ok.t", RunnerStatus::Pass, 1, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::NotProven);
}

#[test]
fn skip_all_plan_is_recognized_skip() {
    let stdout = "1..0 # SKIP database unavailable\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/skip.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/skip.t", RunnerStatus::Pass, 0, 0)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Valid);
    assert_eq!(rec.derived.domain_outcome, InvocationDomainOutcome::RecognizedSkip);
}

#[test]
fn nested_subtest_counts_use_top_level_only() {
    let stdout = "    ok 1 - inner\nok 1 - outer\n1..1\n";
    let rec = reconcile(
        HarnessMode::Parse,
        "base/sub.t",
        TerminalProcessOutcome::CleanExit,
        stdout,
        false,
        &[record(HarnessMode::Parse, "base/sub.t", RunnerStatus::Pass, 1, 1)],
    );
    assert_eq!(rec.derived.evidence_result, EvidenceValidity::Valid);
}

#[test]
fn invocation_id_traversal_is_rejected_for_receipt_paths() {
    let err = crate::invocation_reconcile::invocation_receipt_path(
        std::path::Path::new("/tmp/run"),
        "run-fixture",
        "../escape",
    )
    .expect_err("parent segments must be rejected");
    assert!(err.to_string().contains("traversal"));
}
