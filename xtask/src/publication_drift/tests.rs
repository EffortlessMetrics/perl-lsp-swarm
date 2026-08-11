use super::authority::{load_authority, sha256_hex};
use super::classify::classify;
use super::model::{
    AuthoritySource, LoadedManifest, ManifestVerificationStatus, Observation, PublicationManifest,
    Verdict,
};
use color_eyre::eyre::{Result, bail, eyre};
use std::fs;
use tempfile::TempDir;

const CLEAN: &str = include_str!("../../../fixtures/publication_drift/clean.json");
const AUTHORITY: &[u8] =
    include_bytes!("../../../fixtures/publication_drift/publication_manifest.v1.json");

#[test]
fn clean_translation_fixture_passes() -> Result<()> {
    let receipt = classify_fixture(CLEAN)?;
    if receipt.verdict != Verdict::Clean || !receipt.authority_valid {
        bail!("clean fixture returned {:?}: {:?}", receipt.verdict, receipt.blockers);
    }
    if receipt.comparison_version.as_deref() != Some("0.17.0") {
        bail!("unexpected comparison version: {:?}", receipt.comparison_version);
    }
    if receipt.manifest_verification.status != ManifestVerificationStatus::Verified {
        bail!("manifest was not verified");
    }
    Ok(())
}

#[test]
fn windows_arm64_incident_is_product_drift() -> Result<()> {
    let receipt = classify_fixture(include_str!(
        "../../../fixtures/publication_drift/windows_arm64_target_drift.json"
    ))?;
    if receipt.verdict != Verdict::Drift {
        bail!("incident fixture returned {:?}: {:?}", receipt.verdict, receipt.blockers);
    }
    assert_blocker(&receipt, "same_version_divergent_product")?;
    assert_blocker(&receipt, "product_drift")
}

#[test]
fn behavioral_translation_is_promoted_to_product_drift() -> Result<()> {
    let receipt = classify_fixture(include_str!(
        "../../../fixtures/publication_drift/behavioral_translation.json"
    ))?;
    if receipt.verdict != Verdict::Drift {
        bail!("behavioral translation returned {:?}", receipt.verdict);
    }
    if receipt.differences[0].effective_classification != "product_drift" {
        bail!("behavioral translation was not promoted to product drift");
    }
    assert_blocker(&receipt, "behavioral_translation_is_product_drift")
}

#[test]
fn missing_manifest_is_not_proven() -> Result<()> {
    let observation: Observation = serde_json::from_str(include_str!(
        "../../../fixtures/publication_drift/missing_manifest.json"
    ))?;
    let receipt = classify(observation, AuthoritySource::Missing);
    if receipt.verdict != Verdict::NotProven {
        bail!("missing manifest returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "comparison_manifest_missing")
}

#[test]
fn invalid_authority_dominates_observed_drift() -> Result<()> {
    let receipt = classify_fixture(include_str!(
        "../../../fixtures/publication_drift/invalid_authority_with_drift.json"
    ))?;
    if receipt.verdict != Verdict::NotProven {
        bail!("invalid authority returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_public_basis_mismatch")
}

#[test]
fn windows_paths_are_rejected_on_every_host() -> Result<()> {
    let receipt =
        classify_fixture(include_str!("../../../fixtures/publication_drift/windows_path.json"))?;
    if receipt.verdict != Verdict::NotProven {
        bail!("Windows path returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "invalid_difference_path")
}

#[test]
fn omitted_required_collections_are_not_proven() -> Result<()> {
    let mut observation = clean_observation()?;
    observation.differences = None;
    observation.invariants = None;
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("omitted collections returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "differences_collection_missing")?;
    assert_blocker(&receipt, "invariants_collection_missing")
}

#[test]
fn cross_version_comparison_is_not_proven() -> Result<()> {
    let mut observation = clean_observation()?;
    observation.public.version = "0.18.0".to_string();
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("cross-version comparison returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "cross_version_comparison")
}

#[test]
fn optional_tag_prefix_normalizes_to_same_release() -> Result<()> {
    let mut observation = clean_observation()?;
    observation.swarm.version = "v0.17.0".to_string();
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::Clean {
        bail!("tag-prefixed version returned {:?}: {:?}", receipt.verdict, receipt.blockers);
    }
    if receipt.comparison_version.as_deref() != Some("0.17.0") {
        bail!("tag-prefixed comparison did not normalize: {:?}", receipt.comparison_version);
    }
    Ok(())
}

#[test]
fn unknown_manifest_rule_is_not_proven() -> Result<()> {
    let mut observation = clean_observation()?;
    let difference = first_difference_mut(&mut observation)?;
    difference.manifest_rule = Some("unknown.rule".to_string());
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("unknown manifest rule returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_rule_unknown")
}

#[test]
fn manifest_rule_cannot_authorize_another_path() -> Result<()> {
    let mut observation = clean_observation()?;
    first_difference_mut(&mut observation)?.path = "docs/README.md".to_string();
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("rule path mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_rule_path_mismatch")
}

#[test]
fn manifest_rule_cannot_authorize_another_classification() -> Result<()> {
    let mut observation = clean_observation()?;
    first_difference_mut(&mut observation)?.classification = "release_metadata_only".to_string();
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("rule classification mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_rule_classification_mismatch")
}

#[test]
fn manifest_rule_cannot_authorize_another_owner() -> Result<()> {
    let mut observation = clean_observation()?;
    first_difference_mut(&mut observation)?.owner = "documentation".to_string();
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("rule owner mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_rule_owner_mismatch")
}

#[test]
fn digest_mismatch_is_not_proven() -> Result<()> {
    let mut observation = clean_observation()?;
    observation.manifest.as_mut().ok_or_else(|| eyre!("clean fixture manifest missing"))?.sha256 =
        "f".repeat(64);
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("digest mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_digest_mismatch")
}

#[test]
fn manifest_repository_identity_must_match_subjects() -> Result<()> {
    let observation = clean_observation()?;
    let mut document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    document.swarm_repository = "other/repository".to_string();
    let source =
        AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: manifest_digest() });
    let receipt = classify(observation, source);
    if receipt.verdict != Verdict::NotProven {
        bail!("manifest repository mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_swarm_repository_mismatch")
}

#[test]
fn manifest_tree_digest_must_match_subjects() -> Result<()> {
    let observation = clean_observation()?;
    let mut document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    document.public_tree_digest = "f".repeat(64);
    let source =
        AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: manifest_digest() });
    let receipt = classify(observation, source);
    if receipt.verdict != Verdict::NotProven {
        bail!("manifest tree mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_public_tree_digest_mismatch")
}

#[test]
fn manifest_version_must_match_comparison_version() -> Result<()> {
    let observation = clean_observation()?;
    let mut document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    document.version = "0.18.0".to_string();
    let source =
        AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: manifest_digest() });
    let receipt = classify(observation, source);
    if receipt.verdict != Verdict::NotProven {
        bail!("manifest version mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_version_mismatch")
}

#[test]
fn manifest_cannot_omit_minimum_invariant_authority() -> Result<()> {
    let observation = clean_observation()?;
    let mut document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    document
        .required_invariants
        .retain(|invariant| invariant.id != "artifact_traceable_to_public_sha");
    let source =
        AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: manifest_digest() });
    let receipt = classify(observation, source);
    if receipt.verdict != Verdict::NotProven {
        bail!("incomplete manifest invariants returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "manifest_required_invariant_missing")
}

#[test]
fn observed_invariant_owner_is_bound_to_manifest() -> Result<()> {
    let mut observation = clean_observation()?;
    let invariant = observation
        .invariants
        .as_mut()
        .and_then(|invariants| invariants.first_mut())
        .ok_or_else(|| eyre!("clean fixture invariants missing"))?;
    invariant.owner = "other-owner".to_string();
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("invariant owner mismatch returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "invariant_owner_mismatch")
}

#[test]
fn unknown_observed_invariant_is_not_proven() -> Result<()> {
    let mut observation = clean_observation()?;
    let mut unknown = observation
        .invariants
        .as_ref()
        .and_then(|invariants| invariants.first())
        .cloned()
        .ok_or_else(|| eyre!("clean fixture invariants missing"))?;
    unknown.id = "unmanifested_invariant".to_string();
    observation
        .invariants
        .as_mut()
        .ok_or_else(|| eyre!("clean fixture invariants missing"))?
        .push(unknown);
    let receipt = classify(observation, fixture_authority()?);
    if receipt.verdict != Verdict::NotProven {
        bail!("unknown invariant returned {:?}", receipt.verdict);
    }
    assert_blocker(&receipt, "unknown_invariant")
}

#[test]
fn receipt_collections_are_deterministically_ordered() -> Result<()> {
    let mut observation = clean_observation()?;
    observation
        .invariants
        .as_mut()
        .ok_or_else(|| eyre!("clean fixture invariants missing"))?
        .reverse();
    let receipt = classify(observation, fixture_authority()?);
    if !receipt.invariants.windows(2).all(|window| window[0].id <= window[1].id) {
        bail!("invariants were not sorted in the receipt");
    }
    Ok(())
}

#[test]
fn file_loader_hashes_and_parses_manifest_bytes() -> Result<()> {
    let temp = TempDir::new()?;
    let relative = "authority.json";
    fs::write(temp.path().join(relative), AUTHORITY)?;
    let mut observation = clean_observation()?;
    let manifest =
        observation.manifest.as_mut().ok_or_else(|| eyre!("clean fixture manifest missing"))?;
    manifest.path = relative.to_string();

    match load_authority(temp.path(), Some(manifest)) {
        AuthoritySource::Loaded(loaded) => {
            if loaded.actual_sha256 != manifest_digest() {
                bail!("loader produced an unexpected digest");
            }
        }
        other => bail!("loader did not return authority: {other:?}"),
    }
    Ok(())
}

#[test]
fn schemas_are_well_formed_json_documents() -> Result<()> {
    for raw in [
        include_str!("../../../schemas/publication_manifest.v1.schema.json"),
        include_str!("../../../schemas/publication_drift.v1.schema.json"),
        include_str!("../../../schemas/publication_drift_receipt.v1.schema.json"),
    ] {
        let _: serde_json::Value = serde_json::from_str(raw)?;
    }
    Ok(())
}

fn clean_observation() -> Result<Observation> {
    Ok(serde_json::from_str(CLEAN)?)
}

fn first_difference_mut(
    observation: &mut Observation,
) -> Result<&mut super::model::ObservedDifference> {
    observation
        .differences
        .as_mut()
        .and_then(|differences| differences.first_mut())
        .ok_or_else(|| eyre!("clean fixture difference missing"))
}

fn fixture_authority() -> Result<AuthoritySource> {
    let document = serde_json::from_slice(AUTHORITY)?;
    Ok(AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: manifest_digest() }))
}

fn manifest_digest() -> String {
    sha256_hex(AUTHORITY)
}

fn classify_fixture(raw: &str) -> Result<super::model::Receipt> {
    let observation: Observation = serde_json::from_str(raw)?;
    Ok(classify(observation, fixture_authority()?))
}

fn assert_blocker(receipt: &super::model::Receipt, code: &str) -> Result<()> {
    if !receipt.blockers.iter().any(|blocker| blocker.code == code) {
        bail!("missing blocker {code:?}: {:?}", receipt.blockers);
    }
    Ok(())
}
