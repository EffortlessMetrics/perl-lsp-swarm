//! Fail-closed validation of equivalence receipts (#12286): digest, verdict,
//! plane, and proposition consistency are re-proven from the receipt alone.

use super::compare::equivalence_payload_digest;
use super::model::{
    ComparisonPlane, EquivalenceVerdict, NON_TRANSFERABLE_PROPOSITIONS,
    TRACE_EQUIVALENCE_CLAIM_BOUNDARY, TRANSFERABLE_PROPOSITION,
    UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION, UpstreamTraceEquivalenceV1,
};

const DECLARED_PLANES: [ComparisonPlane; 5] = [
    ComparisonPlane::DiscoveryBytes,
    ComparisonPlane::MembershipOrder,
    ComparisonPlane::TerminalProcess,
    ComparisonPlane::Cleanup,
    ComparisonPlane::InvocationRows,
];

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Validate one equivalence receipt without trusting its verdict.
pub fn validate_trace_equivalence(receipt: &UpstreamTraceEquivalenceV1) -> Result<(), String> {
    if receipt.schema_version != UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION {
        return Err(format!("unexpected equivalence schema {}", receipt.schema_version));
    }
    let expected = equivalence_payload_digest(&receipt.payload)?;
    if expected != receipt.payload_digest {
        return Err("equivalence payload digest mismatch".to_string());
    }
    let payload = &receipt.payload;
    if payload.claim_boundary != TRACE_EQUIVALENCE_CLAIM_BOUNDARY {
        return Err("equivalence claim boundary drifted".to_string());
    }
    if payload.verdict == EquivalenceVerdict::Stale {
        return Err("stale is a consumer judgment and is never written".to_string());
    }
    let expected_transferable: Vec<String> = if payload.verdict.grants_transfer() {
        vec![TRANSFERABLE_PROPOSITION.to_string()]
    } else {
        Vec::new()
    };
    if payload.transferable != expected_transferable {
        return Err("transferable propositions do not match the verdict".to_string());
    }
    let expected_non_transferable: Vec<String> =
        NON_TRANSFERABLE_PROPOSITIONS.iter().map(|value| value.to_string()).collect();
    if payload.non_transferable != expected_non_transferable {
        return Err("non-transferable propositions drifted".to_string());
    }
    if payload.verdict.grants_transfer()
        || payload.verdict == EquivalenceVerdict::MeasuredDifference
    {
        let planes: Vec<ComparisonPlane> = payload.planes.iter().map(|plane| plane.plane).collect();
        if planes != DECLARED_PLANES {
            return Err("judged verdicts require the five declared planes in order".to_string());
        }
    }
    for plane in &payload.planes {
        if plane.matched != plane.differences.is_empty() {
            return Err("plane matched flag contradicts its differences".to_string());
        }
        if plane.matched && plane.ordinary_digest != plane.instrumented_digest {
            return Err("matched plane must carry equal plane identities".to_string());
        }
        if !is_digest(&plane.ordinary_digest) || !is_digest(&plane.instrumented_digest) {
            return Err("plane digests must be lowercase SHA-256".to_string());
        }
    }
    if payload.verdict.grants_transfer()
        && (payload.planes.iter().any(|plane| !plane.matched) || !payload.differences.is_empty())
    {
        return Err("passing verdict requires every declared plane matched".to_string());
    }
    if payload.verdict == EquivalenceVerdict::MeasuredDifference && payload.differences.is_empty() {
        return Err("measured difference must retain its differences".to_string());
    }
    if payload.subjects.equivalence_identity.instrumentation_id.is_empty() {
        return Err("equivalence identity must bind an instrumentation subject".to_string());
    }
    for digest in [
        &payload.subjects.instrumented_parent_receipt_digest,
        &payload.subjects.trace_receipt_digest,
        &payload.subjects.work_receipt_digest,
        &payload.subjects.equivalence_identity.ordinary_runner_artifact_sha256,
        &payload.subjects.equivalence_identity.instrumented_runner_artifact_sha256,
        &payload.subjects.equivalence_identity.patch_subject_digest,
    ] {
        if !is_digest(digest) {
            return Err("subject digests must be lowercase SHA-256".to_string());
        }
    }
    if payload.verdict.grants_transfer() && !is_digest(&payload.subjects.ordinary_receipt_digest) {
        return Err("passing verdict requires an ordinary receipt digest".to_string());
    }
    Ok(())
}
