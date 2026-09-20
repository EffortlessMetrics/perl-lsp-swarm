use super::authority::sha256_hex;
use super::model::{
    AuthoritySource, LoadedManifest, Observation, ObservedDifference, PublicationManifest,
};
use color_eyre::eyre::{Result, eyre};

pub(super) const CLEAN: &str = include_str!("../../../fixtures/publication_drift/clean.json");
pub(super) const AUTHORITY: &[u8] =
    include_bytes!("../../../fixtures/publication_drift/publication_manifest.v1.json");

pub(super) fn clean_observation() -> Result<Observation> {
    Ok(serde_json::from_str(CLEAN)?)
}

pub(super) fn first_difference_mut(
    observation: &mut Observation,
) -> Result<&mut ObservedDifference> {
    observation
        .differences
        .as_mut()
        .and_then(|differences| differences.first_mut())
        .ok_or_else(|| eyre!("clean fixture has no difference"))
}

pub(super) fn fixture_authority() -> Result<AuthoritySource> {
    let document: PublicationManifest = serde_json::from_slice(AUTHORITY)?;
    Ok(AuthoritySource::Loaded(LoadedManifest { document, actual_sha256: manifest_digest() }))
}

pub(super) fn manifest_digest() -> String {
    sha256_hex(AUTHORITY)
}
