// Allow dead_code because this helper is currently exercised by tests and schema fixtures before CLI wiring lands.
#![allow(dead_code)]

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewVerdict {
    Clean,
    NeedsBuilderFix,
    NeedsDiffFix,
    NeedsHuman,
    BlockedUnknown,
}

/// What kind of pass produced this review receipt.
///
/// This is the invariant-#4 binding: a pass that ALSO mutated the branch
/// (a fix-forward pass) is `FixResponder`, never `IndependentReview` — the
/// merge-ready computation in `merge_ready.rs` treats these differently.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewInstrument {
    /// An independent reviewer pass that did not mutate the branch.
    #[default]
    IndependentReview,
    /// The same principal that reviewed also holds/held a writer-lease
    /// mutation on this branch (fix-forward) — does NOT satisfy the
    /// independent-review requirement for computed merge-ready.
    FixResponder,
    /// A standards/lint-only pass (fast first pass, not correctness review).
    Standards,
    /// A narrow verification/proof-running pass (build/test output only).
    Verify,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReceipt {
    pub kind: String,
    pub producer: String,
    pub pr: u64,
    pub head_sha: String,
    pub base_sha: String,
    pub(crate) verdict: ReviewVerdict,
    pub material_observations: Vec<String>,
    pub negative_checks: Vec<String>,
    pub blockers: Vec<String>,
    pub next_routes: Vec<String>,
    pub supersedes: Option<String>,
    /// Which instrument produced this receipt. Defaults to `IndependentReview`
    /// so existing serialized receipts (emitted before this field existed)
    /// deserialize without panicking — back-compat is required.
    #[serde(default)]
    pub instrument: ReviewInstrument,
    /// Free-text scope of what this review was bounded to verify. Defaults
    /// to empty for back-compat with pre-existing receipts.
    #[serde(default)]
    pub claim_boundary: String,
}

/// Validate review receipt invariants that are easy to keep in sync with tests.
pub fn validate_review_receipt(value: &Value) -> Result<()> {
    let receipt: ReviewReceipt = serde_json::from_value(value.clone())?;

    if receipt.kind != "review" {
        bail!("kind must be 'review'");
    }

    if receipt.producer.is_empty() {
        bail!("producer must be non-empty");
    }

    if receipt.pr == 0 {
        bail!("pr must be >= 1");
    }

    if !is_sha1_hex(&receipt.head_sha) {
        bail!("head_sha must be a 40-char lowercase hex commit sha");
    }

    if !is_sha1_hex(&receipt.base_sha) {
        bail!("base_sha must be a 40-char lowercase hex commit sha");
    }

    if receipt.next_routes.is_empty() {
        bail!("next_routes must not be empty");
    }

    let has_signoff_intent = receipt.next_routes.iter().any(|route| route == "signoff_clean");

    if matches!(receipt.verdict, ReviewVerdict::Clean) {
        if receipt.material_observations.is_empty() {
            bail!("clean verdict requires at least one material observation");
        }
        if receipt.negative_checks.is_empty() {
            bail!("clean verdict requires at least one negative check");
        }
        if !has_signoff_intent {
            bail!("clean verdict must include signoff_clean in next_routes");
        }
    }

    if matches!(
        receipt.verdict,
        ReviewVerdict::NeedsBuilderFix
            | ReviewVerdict::NeedsDiffFix
            | ReviewVerdict::NeedsHuman
            | ReviewVerdict::BlockedUnknown
    ) && has_signoff_intent
    {
        bail!("needs-fix/human/blocked verdicts must not emit clean sign-off intent");
    }

    // Evidence-only receipt: route hints are allowed, mutation commands are not.
    if receipt.next_routes.iter().any(|route| route.starts_with("label:")) {
        bail!("review receipts are evidence-only and may not include label mutations");
    }

    let _ = receipt.blockers;
    let _ = receipt.supersedes;

    Ok(())
}

fn is_sha1_hex(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

/// Load and validate a review receipt from a JSON file on disk.
///
/// Used by the merge-ready computation (`merge_ready.rs`) to bind computed
/// merge-readiness to an actual, schema-valid review receipt rather than a
/// bare label name.
pub fn load_review_receipt(path: &Path) -> Result<ReviewReceipt> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read review receipt: {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse review receipt JSON: {}", path.display()))?;
    validate_review_receipt(&value)
        .with_context(|| format!("review receipt failed validation: {}", path.display()))?;
    serde_json::from_value(value)
        .with_context(|| format!("failed to deserialize review receipt: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{ReviewInstrument, load_review_receipt, validate_review_receipt};
    use color_eyre::eyre::{Result, eyre};
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> Result<PathBuf> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = base.join("tests").join("fixtures").join("review-receipts").join(name);
        if !path.exists() {
            return Err(eyre!("missing fixture: {}", path.display()));
        }
        Ok(path)
    }

    fn load_fixture(name: &str) -> Result<Value> {
        let raw = fs::read_to_string(fixture_path(name)?)?;
        let parsed: Value = serde_json::from_str(&raw)?;
        Ok(parsed)
    }

    #[test]
    fn clean_receipt_with_observations_passes() -> Result<()> {
        let value = load_fixture("clean-with-observations.json")?;
        validate_review_receipt(&value)
    }

    #[test]
    fn clean_receipt_without_observations_fails() -> Result<()> {
        let value = load_fixture("clean-without-observations.json")?;
        let err = validate_review_receipt(&value)
            .err()
            .ok_or_else(|| eyre!("fixture should fail validation"))?;
        let message = err.to_string();
        if !message.contains("material observation") {
            return Err(eyre!("expected material observation failure, got: {message}"));
        }
        Ok(())
    }

    #[test]
    fn needs_builder_fix_with_clean_signoff_intent_fails() -> Result<()> {
        let value = load_fixture("needs-builder-fix-with-clean-signoff-intent.json")?;
        let err = validate_review_receipt(&value)
            .err()
            .ok_or_else(|| eyre!("fixture should fail validation"))?;
        let message = err.to_string();
        if !message.contains("must not emit clean sign-off intent") {
            return Err(eyre!("expected signoff intent failure, got: {message}"));
        }
        Ok(())
    }

    /// Back-compat: a receipt serialized before `instrument`/`claim_boundary`
    /// existed (the on-disk fixture predates this change) must still
    /// deserialize via the serde defaults, not panic or error.
    #[test]
    fn receipt_without_instrument_field_deserializes_with_default() -> Result<()> {
        let receipt = load_review_receipt(&fixture_path("clean-with-observations.json")?)?;
        if receipt.instrument != ReviewInstrument::IndependentReview {
            return Err(eyre!(
                "expected default instrument IndependentReview, got {:?}",
                receipt.instrument
            ));
        }
        if !receipt.claim_boundary.is_empty() {
            return Err(eyre!(
                "expected default empty claim_boundary, got {:?}",
                receipt.claim_boundary
            ));
        }
        Ok(())
    }
}
