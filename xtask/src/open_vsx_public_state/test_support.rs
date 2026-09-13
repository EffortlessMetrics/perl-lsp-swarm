use super::classify::classify;
use super::model::{Observation, PublicState, Receipt};
use color_eyre::eyre::{Result, WrapErr, bail};

pub(super) const INCIDENT: &str =
    include_str!("../../../fixtures/open_vsx_public_state/incident_shape_listing_absent.json");
pub(super) const AVAILABLE_EXACT: &str =
    include_str!("../../../fixtures/open_vsx_public_state/synthetic_available_exact.json");
pub(super) const LISTING_MISSING: &str = include_str!(
    "../../../fixtures/open_vsx_public_state/synthetic_listing_missing_version_retrievable.json"
);
pub(super) const RATE_LIMITED: &str =
    include_str!("../../../fixtures/open_vsx_public_state/synthetic_provider_rate_limited.json");
pub(super) const NAMESPACE_ABSENT: &str =
    include_str!("../../../fixtures/open_vsx_public_state/synthetic_namespace_absent.json");
pub(super) const DIGEST_MISMATCH: &str =
    include_str!("../../../fixtures/open_vsx_public_state/synthetic_public_digest_mismatch.json");
pub(super) const UNPLANNED_URL: &str =
    include_str!("../../../fixtures/open_vsx_public_state/synthetic_unplanned_request_url.json");
pub(super) const INSTRUMENT_INCOMPLETE: &str =
    include_str!("../../../fixtures/open_vsx_public_state/synthetic_instrument_incomplete.json");

/// Every observation fixture, for invariants that must hold across all of them.
pub(super) const ALL_FIXTURES: &[&str] = &[
    INCIDENT,
    AVAILABLE_EXACT,
    LISTING_MISSING,
    RATE_LIMITED,
    NAMESPACE_ABSENT,
    DIGEST_MISMATCH,
    UNPLANNED_URL,
    INSTRUMENT_INCOMPLETE,
];

pub(super) fn observation(raw: &str) -> Result<Observation> {
    serde_json::from_str(raw).wrap_err("parsing an Open VSX observation fixture")
}

pub(super) fn receipt(raw: &str) -> Result<Receipt> {
    Ok(classify(observation(raw)?))
}

/// Classify a fixture after applying one mutation to its parsed JSON.
pub(super) fn receipt_with(
    raw: &str,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<Receipt> {
    let mut document: serde_json::Value =
        serde_json::from_str(raw).wrap_err("parsing an Open VSX observation fixture")?;
    mutate(&mut document);
    let observation: Observation = serde_json::from_value(document)
        .wrap_err("re-parsing a mutated Open VSX observation fixture")?;
    Ok(classify(observation))
}

pub(super) fn expect_state(receipt: &Receipt, expected: PublicState, context: &str) -> Result<()> {
    if receipt.state != expected {
        bail!(
            "{context}: expected {}, found {} (blockers: {:?})",
            expected.key(),
            receipt.state.key(),
            receipt.blockers.iter().map(|blocker| blocker.code.as_str()).collect::<Vec<_>>()
        );
    }
    Ok(())
}

pub(super) fn expect_blocker(receipt: &Receipt, code: &str) -> Result<()> {
    if !receipt.blockers.iter().any(|blocker| blocker.code == code) {
        bail!(
            "expected blocker {code}, found {:?}",
            receipt.blockers.iter().map(|blocker| blocker.code.as_str()).collect::<Vec<_>>()
        );
    }
    Ok(())
}
