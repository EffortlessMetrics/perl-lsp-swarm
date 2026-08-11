use super::authority::sha256_hex;
use super::classify::classify;
use super::model::{
    AuthoritySource, LoadedManifest, NOT_PROVEN_CLASS, Observation, PublicationManifest, Verdict,
};
use color_eyre::eyre::{Result, bail, eyre};

const CLEAN: &str = include_str!("../../../fixtures/publication_drift/clean.json");
const AUTHORITY: &[u8] =
    include_bytes!("../../../fixtures/publication_drift/publication_manifest.v1.json");

#[test]
fn missing_authority_downgrades_clean_difference_in_receipt() -> Result<()> {
    let observation: Observation = serde_json::from_str(CLEAN)?;
    let receipt = classify(observation, AuthoritySource::Missing);
    assert_not_proven_difference(&receipt, "missing authority")
}

#[test]
fn unknown_manifest_rule_downgrades_clean_difference_in_receipt() -> Result<()> {
    let mut observation: Observation = serde_json::from_str(CLEAN)?;
    first_difference_mut(&mut observation)?.manifest_rule = Some("unknown-rule".to_string());
    let receipt = classify(observation, fixture_authority()?);
    assert_not_proven_difference(&receipt, "unknown rule")
}

#[test]
fn mismatched_manifest_rule_downgrades_clean_difference_in_receipt() -> Result<()> {
    let mut observation: Observation = serde_json::from_str(CLEAN)?;
    first_difference_mut(&mut observation)?.owner = "different-owner".to_string();
    let receipt = classify(observation, fixture_authority()?);
    assert_not_proven_difference(&receipt, "mismatched rule")
}

fn first_difference_mut(
    observation: &mut Observation,
) -> Result<&mut super::model::ObservedDifference> {
    observation
        .differences
        .as_mut()
        .and_then(|differences| differences.first_mut())
        .ok_or_else(|| eyre!("clean fixture has no difference"))
}

fn fixture_authority() -> Result<AuthoritySource> {
    let document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    Ok(AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: sha256_hex(AUTHORITY) }))
}

fn assert_not_proven_difference(receipt: &super::model::Receipt, case: &str) -> Result<()> {
    if receipt.verdict != Verdict::NotProven {
        bail!("{case}: expected not_proven, found {:?}", receipt.verdict);
    }
    let effective = receipt
        .differences
        .first()
        .map(|difference| difference.effective_classification.as_str())
        .ok_or_else(|| eyre!("{case}: receipt has no classified difference"))?;
    if effective != NOT_PROVEN_CLASS {
        bail!(
            "{case}: expected effective classification {NOT_PROVEN_CLASS:?}, found {effective:?}"
        );
    }
    Ok(())
}
