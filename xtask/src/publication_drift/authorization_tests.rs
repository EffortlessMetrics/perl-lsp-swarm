use super::classify::classify;
use super::model::{AuthoritySource, NOT_PROVEN_CLASS, Observation, Verdict};
use super::test_support::{CLEAN, first_difference_mut, fixture_authority};
use color_eyre::eyre::{Result, bail, eyre};

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
