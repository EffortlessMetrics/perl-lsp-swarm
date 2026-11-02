//! Pure derivation of settled invocation receipts from independent evidence planes.

use perl_core_harness_types::{
    HarnessMode, RUNNER_RECORD_SCHEMA_VERSION, RunnerRecord, RunnerStatus,
    result_axes::EvidenceValidity, validate_execution_mechanism,
};
use perl_test_facts::parse_tap;

use crate::normalization::sha256_digest_bytes;
use crate::normalize_test_path;

use super::model::{
    INVOCATION_RECEIPT_SCHEMA_VERSION, InvocationAuthority, InvocationBinding, InvocationDerived,
    InvocationDomainOutcome, InvocationProcessSummary, InvocationSemanticSummary,
    InvocationTapSummary, ReconcileInvocationInput, SemanticRecordRejection,
    SettledInvocationReceipt, TAP_PARSER_IDENTITY,
};

/// Recompute one invocation claim from explicit supplied evidence.
///
/// The producer's self-reported pass/fail status is compared but never trusted
/// as the final domain outcome.
#[must_use]
pub fn reconcile_invocation(input: ReconcileInvocationInput<'_>) -> SettledInvocationReceipt {
    let mut reasons = Vec::new();
    let tap = summarize_tap(input.stdout);
    let process = InvocationProcessSummary {
        terminal_outcome: input.terminal.label().to_string(),
        scoreable: input.terminal.is_scoreable(),
        stdout_truncated: input.stdout_truncated,
    };

    if input.stdout_truncated {
        reasons.push(input.terminal.not_proven_reason());
        reasons
            .push("stdout capture was truncated; TAP completeness cannot be trusted".to_string());
    }
    if !input.terminal.is_scoreable() {
        reasons.push(input.terminal.not_proven_reason());
    }

    let semantic =
        match validate_semantic_records(&input.binding, input.semantic_records, &mut reasons) {
            Ok(summary) => summary,
            Err(rejection) => {
                reasons.push(rejection.reason());
                None
            }
        };

    if input.authority == InvocationAuthority::DirectProbe {
        reasons.push(
            "direct_probe authority cannot settle upstream product evidence (#8173)".to_string(),
        );
    }

    let tap_valid = tap_is_authoritative(&tap, &mut reasons);
    let cross_plane_ok =
        cross_planes_agree(&process, &tap, semantic.as_ref(), &mut reasons, input.binding.mode);

    let evidence_result =
        derive_evidence_validity(&process, tap_valid, semantic.is_some(), cross_plane_ok, &reasons);
    let domain_outcome = derive_domain_outcome(
        evidence_result,
        &process,
        &tap,
        semantic.as_ref(),
        input.binding.mode,
    );

    SettledInvocationReceipt {
        schema_version: INVOCATION_RECEIPT_SCHEMA_VERSION.to_string(),
        run_id: input.binding.run_id.clone(),
        invocation_id: input.binding.invocation_id.clone(),
        authority: input.authority,
        binding: input.binding,
        process,
        tap,
        semantic_record: semantic,
        derived: InvocationDerived { evidence_result, domain_outcome, reasons },
    }
}

fn summarize_tap(stdout: &str) -> InvocationTapSummary {
    let report = parse_tap(stdout);
    let structurally_complete = report.bail_out.is_none()
        && report.diagnostics.is_empty()
        && report.unknown_count() == 0
        && report.plan.is_some();
    InvocationTapSummary {
        raw_digest: sha256_digest_bytes(stdout.as_bytes()),
        parser_identity: TAP_PARSER_IDENTITY.to_string(),
        plan_start: report.plan.as_ref().map(|plan| plan.start),
        plan_end: report.plan.as_ref().map(|plan| plan.end),
        assertions_passed: report.passed_count(),
        assertions_failed: report.failed_count(),
        skipped: report.skipped_count(),
        bailed_out: report.bail_out.is_some(),
        structurally_complete,
    }
}

fn tap_is_authoritative(tap: &InvocationTapSummary, reasons: &mut Vec<String>) -> bool {
    if tap.bailed_out {
        reasons.push("TAP bailed out before completion".to_string());
        return false;
    }
    if !tap.structurally_complete {
        reasons.push("TAP is malformed, incomplete, or lacks a valid plan".to_string());
        return false;
    }
    true
}

fn validate_semantic_records(
    binding: &InvocationBinding,
    records: &[RunnerRecord],
    reasons: &mut Vec<String>,
) -> Result<Option<InvocationSemanticSummary>, SemanticRecordRejection> {
    if records.is_empty() {
        return Err(SemanticRecordRejection::MissingRecord);
    }
    if records.len() > 1 {
        let mut saw_pass = false;
        let mut saw_fail = false;
        for record in records {
            match record.status {
                RunnerStatus::Pass => saw_pass = true,
                RunnerStatus::Fail => saw_fail = true,
            }
        }
        if saw_pass && saw_fail {
            return Err(SemanticRecordRejection::ConflictingStatuses);
        }
        return Err(SemanticRecordRejection::DuplicateRecords);
    }
    let record = &records[0];
    if record.schema_version != RUNNER_RECORD_SCHEMA_VERSION {
        return Err(SemanticRecordRejection::WrongSchema {
            observed: record.schema_version.clone(),
        });
    }
    if record.mode != binding.mode.as_str() {
        return Err(SemanticRecordRejection::WrongMode {
            observed: record.mode.clone(),
            expected: binding.mode.as_str().to_string(),
        });
    }
    let normalized =
        normalize_test_path(&record.path).ok_or_else(|| SemanticRecordRejection::WrongPath {
            observed: record.path.clone(),
            expected: binding.expected_path.clone(),
        })?;
    if normalized != binding.expected_path {
        return Err(SemanticRecordRejection::WrongPath {
            observed: normalized,
            expected: binding.expected_path.clone(),
        });
    }
    if record.assertions_passed > record.assertions_total {
        return Err(SemanticRecordRejection::ImpossibleAssertions {
            passed: record.assertions_passed,
            total: record.assertions_total,
        });
    }
    if let Err(violation) = validate_execution_mechanism(&record.mode, record.mechanism) {
        return Err(SemanticRecordRejection::MechanismViolation { detail: violation.to_string() });
    }
    let encoded = match serde_json::to_vec(record) {
        Ok(bytes) => bytes,
        Err(err) => {
            reasons.push(format!("semantic record could not be canonicalized: {err}"));
            return Err(SemanticRecordRejection::MechanismViolation {
                detail: "semantic record serialization failed".to_string(),
            });
        }
    };
    Ok(Some(InvocationSemanticSummary {
        payload_digest: sha256_digest_bytes(&encoded),
        mode: record.mode.clone(),
        path: record.path.clone(),
        reported_status: record.status,
        assertions_passed: record.assertions_passed,
        assertions_total: record.assertions_total,
    }))
}

fn cross_planes_agree(
    process: &InvocationProcessSummary,
    tap: &InvocationTapSummary,
    semantic: Option<&InvocationSemanticSummary>,
    reasons: &mut Vec<String>,
    mode: HarnessMode,
) -> bool {
    let Some(semantic) = semantic else {
        if tap.structurally_complete && tap.assertions_failed == 0 && process.scoreable {
            reasons.push(
                "missing semantic runner record despite apparently successful TAP".to_string(),
            );
        }
        return false;
    };

    if semantic.assertions_passed != tap.assertions_passed
        || semantic.assertions_total != tap_count_total(tap)
    {
        reasons.push(format!(
            "semantic record assertions ({}/{}) disagree with TAP ({}/{})",
            semantic.assertions_passed,
            semantic.assertions_total,
            tap.assertions_passed,
            tap_count_total(tap)
        ));
        return false;
    }

    let tap_pass = tap.assertions_failed == 0 && !tap.bailed_out;
    let record_pass = semantic.reported_status == RunnerStatus::Pass;
    if record_pass && !tap_pass {
        reasons.push(
            "semantic record reports pass while TAP contains failures or bailout".to_string(),
        );
        return false;
    }
    if !record_pass && tap_pass && !mode_allows_tap_pass_record_fail(mode) {
        reasons.push(
            "semantic record reports fail while TAP reports a clean pass without mode allowance"
                .to_string(),
        );
        return false;
    }

    if record_pass && !process.scoreable {
        reasons.push(
            "semantic record reports pass while terminal process evidence is not scoreable"
                .to_string(),
        );
        return false;
    }

    if process.stdout_truncated {
        return false;
    }

    true
}

fn tap_count_total(tap: &InvocationTapSummary) -> usize {
    tap.assertions_passed.saturating_add(tap.assertions_failed).saturating_add(tap.skipped)
}

fn mode_allows_tap_pass_record_fail(mode: HarnessMode) -> bool {
    // Parse/compile synthetic one-test TAP can represent failure outside TAP lines.
    matches!(mode, HarnessMode::Parse | HarnessMode::Compile)
}

fn derive_evidence_validity(
    process: &InvocationProcessSummary,
    tap_valid: bool,
    has_semantic: bool,
    cross_plane_ok: bool,
    reasons: &[String],
) -> EvidenceValidity {
    if reasons.iter().any(|reason| reason.contains("direct_probe authority")) {
        return EvidenceValidity::Invalid;
    }
    if !has_semantic || !tap_valid || !process.scoreable || process.stdout_truncated {
        return EvidenceValidity::NotProven;
    }
    if cross_plane_ok { EvidenceValidity::Valid } else { EvidenceValidity::Invalid }
}

fn derive_domain_outcome(
    evidence: EvidenceValidity,
    process: &InvocationProcessSummary,
    tap: &InvocationTapSummary,
    semantic: Option<&InvocationSemanticSummary>,
    mode: HarnessMode,
) -> InvocationDomainOutcome {
    if evidence != EvidenceValidity::Valid {
        return InvocationDomainOutcome::NotAssessed;
    }
    let semantic = match semantic {
        Some(value) => value,
        None => return InvocationDomainOutcome::NotAssessed,
    };
    if tap.skipped > 0
        && tap.assertions_failed == 0
        && semantic.reported_status == RunnerStatus::Pass
    {
        return InvocationDomainOutcome::RecognizedSkip;
    }
    if semantic.reported_status == RunnerStatus::Fail || tap.assertions_failed > 0 {
        if semantic.reported_status == RunnerStatus::Fail || tap.assertions_failed > 0 {
            return InvocationDomainOutcome::CompletedWithFailures;
        }
    }
    if !process.scoreable {
        return InvocationDomainOutcome::NotAssessed;
    }
    if semantic.reported_status == RunnerStatus::Pass
        && tap.assertions_failed == 0
        && tap.assertions_passed > 0
    {
        return InvocationDomainOutcome::CompletedCleanly;
    }
    if mode_allows_tap_pass_record_fail(mode) && semantic.reported_status == RunnerStatus::Fail {
        return InvocationDomainOutcome::CompletedWithFailures;
    }
    InvocationDomainOutcome::UnsupportedOrBounded
}
