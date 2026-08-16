use super::observation::{load_observation, validate_normalized_observation};
use super::static_sources::load_static_topology;
use super::{Observation, Projection, SCHEMA, SourceDigest, StaticTopology};
use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize)]
struct ProjectionBody<'a> {
    schema: u32,
    #[serde(rename = "static")]
    static_topology: &'a StaticTopology,
    observation: &'a Observation,
    sources: &'a BTreeMap<String, SourceDigest>,
}

pub fn build_projection(root: &Path, observation_path: Option<&Path>) -> Result<Projection> {
    let (static_topology, sources) = load_static_topology(root)?;
    let observation = load_observation(observation_path, &static_topology)?;
    let projection_digest = projection_digest(&static_topology, &observation, &sources)?;
    Ok(Projection { schema: SCHEMA, static_topology, observation, sources, projection_digest })
}

pub fn validate_projection(root: &Path, projection: &Projection) -> Result<()> {
    if projection.schema != SCHEMA {
        bail!("unsupported contributor topology schema {}; expected {SCHEMA}", projection.schema);
    }
    let (static_topology, sources) = load_static_topology(root)?;
    if projection.static_topology != static_topology {
        bail!("projection static topology is stale or contradictory");
    }
    if projection.sources != sources {
        bail!("projection source digests are stale");
    }
    validate_normalized_observation(&projection.observation)?;
    let expected = projection_digest(
        &projection.static_topology,
        &projection.observation,
        &projection.sources,
    )?;
    if projection.projection_digest != expected {
        bail!("projection digest does not match its semantic content");
    }
    Ok(())
}

pub fn render_human(projection: &Projection) -> String {
    let static_topology = &projection.static_topology;
    let observation = &projection.observation;
    let development_sha = observation.development_sha.as_deref().unwrap_or("NOT_PROVEN");
    let publication_sha = observation.publication_sha.as_deref().unwrap_or("NOT_PROVEN");
    let mut lines = vec![
        format!("contributor-topology: {}", observation.status.as_str()),
        format!(
            "development: {}/{} @ {}",
            static_topology.development_repository,
            static_topology.development_default_branch,
            development_sha
        ),
        format!(
            "publication: {}/{} @ {}",
            static_topology.publication_repository,
            static_topology.publication_branch,
            publication_sha
        ),
        format!("issues/prs: {}", static_topology.issue_repository),
        format!("promotion: {}", static_topology.promotion_protocol),
        format!("stage: {}", observation.stage.as_str()),
    ];
    if let Some(limitation) = &observation.limitation {
        lines.push(format!("limitation: {limitation}"));
    }
    for (channel, state) in &observation.channels {
        lines.push(format!("channel {channel}: {}", state.as_str()));
    }
    lines.join("\n")
}

fn projection_digest(
    static_topology: &StaticTopology,
    observation: &Observation,
    sources: &BTreeMap<String, SourceDigest>,
) -> Result<String> {
    let body = ProjectionBody { schema: SCHEMA, static_topology, observation, sources };
    let bytes = serde_json::to_vec(&body)?;
    Ok(Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect())
}
