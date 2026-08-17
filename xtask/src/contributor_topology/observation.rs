use super::{ChannelState, Observation, ObservationStatus, PublicationStage, StaticTopology};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedObservation {
    status: ObservationStatus,
    source: String,
    observed_at: String,
    limitation: Option<String>,
    development_repository: String,
    development_branch: String,
    development_sha: Option<String>,
    publication_repository: String,
    publication_branch: String,
    publication_sha: Option<String>,
    prepared_swarm_sha: Option<String>,
    publication_join_sha: Option<String>,
    public_release_tag: Option<String>,
    #[serde(default)]
    channels: BTreeMap<String, ChannelState>,
}

pub(super) fn load_observation(
    path: Option<&Path>,
    static_topology: &StaticTopology,
) -> Result<Observation> {
    let Some(path) = path else {
        return Ok(Observation {
            status: ObservationStatus::NotProven,
            source: None,
            observed_at: None,
            limitation: Some("live topology observation was not supplied".to_string()),
            development_sha: None,
            publication_sha: None,
            prepared_swarm_sha: None,
            publication_join_sha: None,
            public_release_tag: None,
            stage: PublicationStage::NotProven,
            channels: not_proven_channels(&static_topology.primary_channels),
        });
    };
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let captured: CapturedObservation =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    normalize_observation(captured, static_topology)
}

fn normalize_observation(
    captured: CapturedObservation,
    static_topology: &StaticTopology,
) -> Result<Observation> {
    if captured.development_repository != static_topology.development_repository
        || captured.development_branch != static_topology.development_default_branch
        || captured.publication_repository != static_topology.publication_repository
        || captured.publication_branch != static_topology.publication_branch
    {
        bail!("captured observation disagrees with static repository topology");
    }
    validate_observation_metadata(Some(&captured.source), Some(&captured.observed_at))?;
    validate_sha(captured.development_sha.as_deref(), "development_sha")?;
    validate_sha(captured.publication_sha.as_deref(), "publication_sha")?;
    validate_sha(captured.prepared_swarm_sha.as_deref(), "prepared_swarm_sha")?;
    validate_sha(captured.publication_join_sha.as_deref(), "publication_join_sha")?;

    if captured.public_release_tag.as_deref().is_some_and(|value| value.trim().is_empty()) {
        bail!("public_release_tag cannot be empty");
    }
    let channels = complete_channels(captured.channels, &static_topology.primary_channels)?;

    match captured.status {
        ObservationStatus::Proven => {
            if captured.limitation.is_some() {
                bail!("PROVEN observation cannot carry a limitation");
            }
            if captured.development_sha.is_none() || captured.publication_sha.is_none() {
                bail!("PROVEN observation requires both repository SHAs");
            }
        }
        ObservationStatus::NotProven => {
            if captured.limitation.as_deref().is_none_or(|value| value.trim().is_empty()) {
                bail!("NOT_PROVEN observation requires a limitation");
            }
        }
    }
    validate_publication_evidence(
        captured.prepared_swarm_sha.as_deref(),
        captured.publication_join_sha.as_deref(),
        captured.public_release_tag.as_deref(),
        &channels,
    )?;

    let stage = stage_for(
        captured.status,
        captured.prepared_swarm_sha.as_deref(),
        captured.publication_join_sha.as_deref(),
        captured.public_release_tag.as_deref(),
    );
    let observation = Observation {
        status: captured.status,
        source: Some(captured.source),
        observed_at: Some(captured.observed_at),
        limitation: captured.limitation,
        development_sha: captured.development_sha,
        publication_sha: captured.publication_sha,
        prepared_swarm_sha: captured.prepared_swarm_sha,
        publication_join_sha: captured.publication_join_sha,
        public_release_tag: captured.public_release_tag,
        stage,
        channels,
    };
    validate_normalized_observation(&observation, &static_topology.primary_channels)?;
    Ok(observation)
}

pub(super) fn validate_normalized_observation(
    observation: &Observation,
    primary_channels: &[String],
) -> Result<()> {
    validate_sha(observation.development_sha.as_deref(), "development_sha")?;
    validate_sha(observation.publication_sha.as_deref(), "publication_sha")?;
    validate_sha(observation.prepared_swarm_sha.as_deref(), "prepared_swarm_sha")?;
    validate_sha(observation.publication_join_sha.as_deref(), "publication_join_sha")?;
    validate_observation_metadata(
        observation.source.as_deref(),
        observation.observed_at.as_deref(),
    )?;

    match observation.status {
        ObservationStatus::Proven => {
            if observation.source.is_none() || observation.observed_at.is_none() {
                bail!("PROVEN observation requires source and observed_at");
            }
            if observation.limitation.is_some() {
                bail!("PROVEN observation cannot carry a limitation");
            }
            if observation.development_sha.is_none() || observation.publication_sha.is_none() {
                bail!("PROVEN observation requires both repository SHAs");
            }
        }
        ObservationStatus::NotProven => {
            if observation.limitation.as_deref().is_none_or(|value| value.trim().is_empty()) {
                bail!("NOT_PROVEN observation requires a limitation");
            }
        }
    }
    if observation.public_release_tag.as_deref().is_some_and(|value| value.trim().is_empty()) {
        bail!("public_release_tag cannot be empty");
    }
    validate_publication_evidence(
        observation.prepared_swarm_sha.as_deref(),
        observation.publication_join_sha.as_deref(),
        observation.public_release_tag.as_deref(),
        &observation.channels,
    )?;
    validate_channels(&observation.channels, primary_channels)?;
    let expected_stage = stage_for(
        observation.status,
        observation.prepared_swarm_sha.as_deref(),
        observation.publication_join_sha.as_deref(),
        observation.public_release_tag.as_deref(),
    );
    if observation.stage != expected_stage {
        bail!("observation stage does not match its evidence");
    }
    Ok(())
}

fn validate_observation_metadata(source: Option<&str>, observed_at: Option<&str>) -> Result<()> {
    match (source, observed_at) {
        (None, None) => Ok(()),
        (Some(source), Some(observed_at))
            if !source.trim().is_empty() && !observed_at.trim().is_empty() =>
        {
            Ok(())
        }
        (Some(_), Some(_)) => {
            bail!("captured observation source and observed_at must be non-empty")
        }
        _ => bail!("partial observation source and observed_at must move together"),
    }
}

fn complete_channels(
    mut channels: BTreeMap<String, ChannelState>,
    primary_channels: &[String],
) -> Result<BTreeMap<String, ChannelState>> {
    validate_channels(&channels, primary_channels)?;
    for channel in primary_channels {
        channels.entry(channel.clone()).or_insert(ChannelState::NotProven);
    }
    Ok(channels)
}

fn not_proven_channels(primary_channels: &[String]) -> BTreeMap<String, ChannelState> {
    primary_channels.iter().map(|channel| (channel.clone(), ChannelState::NotProven)).collect()
}

fn validate_channels(
    channels: &BTreeMap<String, ChannelState>,
    primary_channels: &[String],
) -> Result<()> {
    if let Some(unknown) =
        channels.keys().find(|channel| !primary_channels.iter().any(|known| known == *channel))
    {
        bail!("unknown primary release channel {unknown:?}");
    }
    if let Some(missing) = primary_channels.iter().find(|channel| !channels.contains_key(*channel))
    {
        bail!("primary release channel {missing:?} is missing");
    }
    Ok(())
}

fn stage_for(
    status: ObservationStatus,
    prepared_sha: Option<&str>,
    join_sha: Option<&str>,
    release_tag: Option<&str>,
) -> PublicationStage {
    match status {
        ObservationStatus::NotProven => PublicationStage::NotProven,
        ObservationStatus::Proven if release_tag.is_some() => PublicationStage::PublicRelease,
        ObservationStatus::Proven if join_sha.is_some() => PublicationStage::PostJoinPreRelease,
        ObservationStatus::Proven if prepared_sha.is_some() => PublicationStage::PreparedCandidate,
        ObservationStatus::Proven => PublicationStage::DevelopmentOnly,
    }
}

fn validate_publication_evidence(
    prepared_sha: Option<&str>,
    join_sha: Option<&str>,
    release_tag: Option<&str>,
    channels: &BTreeMap<String, ChannelState>,
) -> Result<()> {
    if join_sha.is_some() && prepared_sha.is_none() {
        bail!("publication join requires prepared swarm SHA");
    }
    if release_tag.is_some() && join_sha.is_none() {
        bail!("public release requires publication join SHA");
    }
    if channels.values().any(|state| *state == ChannelState::Available) && release_tag.is_none() {
        bail!("AVAILABLE channel requires public release tag");
    }
    Ok(())
}

fn validate_sha(value: Option<&str>, label: &str) -> Result<()> {
    if let Some(value) = value
        && (value.len() != 40
            || !value
                .chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()))
    {
        bail!("{label} must be a full lowercase commit SHA");
    }
    Ok(())
}
