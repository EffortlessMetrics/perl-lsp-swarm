//! Fail-closed validation of equivalence receipts (#12286): digest, verdict,
//! and plane consistency are re-proven from the receipt alone.

use super::compare::equivalence_payload_digest;
use super::model::{
    EquivalenceVerdict, TRACE_EQUIVALENCE_CLAIM_BOUNDARY,
    UPSTREAM_TRACE_EQUIVALENCE_SCHEMA_VERSION, UpstreamTraceEquivalenceV1,
};

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
    if payload.verdict.grants_transfer() && payload.transferable.is_empty() {
        return Err("passing verdict must name its transferable proposition".to_string());
    }
    if !payload.verdict.grants_transfer() && !payload.transferable.is_empty() {
        return Err("non-passing verdict must not transfer propositions".to_string());
    }
    if payload.verdict.grants_transfer()
        && (payload.planes.is_empty()
            || payload.planes.iter().any(|plane| !plane.matched)
            || !payload.differences.is_empty())
    {
        return Err("passing verdict requires every declared plane matched".to_string());
    }
    if payload.verdict == EquivalenceVerdict::MeasuredDifference && payload.differences.is_empty() {
        return Err("measured difference must retain its differences".to_string());
    }
    if payload.subjects.equivalence_identity.instrumentation_id.is_empty() {
        return Err("equivalence identity must bind an instrumentation subject".to_string());
    }
    Ok(())
}
