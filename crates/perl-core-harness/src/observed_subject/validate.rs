//! Fail-closed structural validation for observed runner subject receipts.
//!
//! The subject is a join, not a capture: it retains no raw streams to
//! re-decode. Its standalone validation therefore re-proves the laws that
//! travel with the receipt itself — exact schema identity, sorted mandatory
//! limitations, fixed claim boundary, recomputed row fingerprints (covering
//! every row field including the normalized source identity), state and work
//! coherence re-derived from the retained rows, recomputed payload digest,
//! evidence-class vocabulary, and every structural-zero work invariant — and
//! rejects any nonzero counterfeit outright. Full input-side agreement is
//! proven by [`crate::observed_subject::build::check_observed_runner_subject`],
//! which rebuilds the receipt from its exact inputs and compares.

use crate::observed_subject::build::{observed_subject_payload_digest, required_limitations};
use crate::observed_subject::model::{
    JoinWork, OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION, OBSERVED_SUBJECT_CLAIM_BOUNDARY,
    ObservedRunnerSubjectV1,
};

/// Structural-zero fields of [`JoinWork`] in declaration order.
const STRUCTURAL_ZEROS: [&str; 5] = [
    "source_reads",
    "filesystem_scans",
    "runner_processes",
    "direct_probe_inputs",
    "reconstructed_fields",
];

/// Validate every self-contained law of one observed runner subject receipt.
pub fn validate_observed_runner_subject_shape(
    receipt: &ObservedRunnerSubjectV1,
) -> Result<(), String> {
    if receipt.schema_version != OBSERVED_RUNNER_SUBJECT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported observed runner subject schema {}",
            receipt.schema_version
        ));
    }
    if receipt.payload.claim_boundary != OBSERVED_SUBJECT_CLAIM_BOUNDARY {
        return Err(
            "observed runner subject claim boundary does not match the fixed contract".to_string()
        );
    }
    let mut expected_limitations = required_limitations();
    expected_limitations.sort();
    if receipt.payload.limitations != expected_limitations {
        return Err(
            "observed runner subject limitations are not exactly the mandatory set".to_string()
        );
    }
    if receipt.payload.evidence_classes.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(
            "observed runner subject evidence classes must be sorted and unique".to_string()
        );
    }
    for (index, row) in receipt.payload.rows.iter().enumerate() {
        let expected = crate::observed_subject::build::row_fingerprint(row)?;
        if expected != row.row_fingerprint {
            return Err(format!("joined row {index} fingerprint does not cover its own content"));
        }
    }
    reject_counterfeit_work(&receipt.payload.work)?;
    // State/accounting coherence is re-derived from the retained rows so a
    // re-digested relabel cannot pass standalone validation.
    crate::observed_subject::build::coherence_error(&receipt.payload)?;
    let digest = observed_subject_payload_digest(&receipt.payload)?;
    if digest != receipt.payload_digest {
        return Err(format!(
            "observed runner subject payload digest {digest} does not bind the payload",
        ));
    }
    Ok(())
}

/// Re-prove each structural-zero invariant; unknown work is never numeric zero.
fn reject_counterfeit_work(work: &JoinWork) -> Result<(), String> {
    for field in STRUCTURAL_ZEROS {
        let value = match field {
            "source_reads" => work.source_reads,
            "filesystem_scans" => work.filesystem_scans,
            "runner_processes" => work.runner_processes,
            "direct_probe_inputs" => work.direct_probe_inputs,
            _ => work.reconstructed_fields,
        };
        if value != 0 {
            return Err(format!(
                "structural invariant {field} recorded a nonzero counterfeit value {value}; \
                 the pure join never reads source, scans the filesystem, runs processes, \
                 admits direct probes, or reconstructs missing fields"
            ));
        }
    }
    Ok(())
}
