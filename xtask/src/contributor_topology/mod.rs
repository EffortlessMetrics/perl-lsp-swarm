//! Read-only contributor projection over the canonical development/publication topology.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod observation;
mod projection;
mod static_sources;

pub use projection::{build_projection, render_human, validate_projection};

pub(super) const SCHEMA: u32 = 1;
pub(super) const PRODUCT_IDENTITY_PATH: &str = "policy/product-identity.toml";
pub(super) const SYNC_PROTOCOL_PATH: &str = "docs/swarm/sync-protocol.md";
pub(super) const RELEASE_TOPOLOGY_SCHEMA_PATH: &str = "schemas/release_topology.v1.schema.json";
pub(super) const PROMOTION_PROTOCOL: &str =
    "docs/swarm/sync-protocol.md#mechanics-history-preserving-complete-tree-merge";
pub(super) const EXPECTED_DEVELOPMENT_BRANCH: &str = "main";
pub(super) const EXPECTED_PUBLICATION_BRANCH: &str = "master";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDigest {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StaticTopology {
    pub development_repository: String,
    pub development_default_branch: String,
    pub publication_repository: String,
    pub publication_branch: String,
    pub issue_repository: String,
    pub pull_request_repository: String,
    pub promotion_protocol: String,
    pub primary_channels: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationStatus {
    Proven,
    NotProven,
}

impl ObservationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "PROVEN",
            Self::NotProven => "NOT_PROVEN",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStage {
    NotProven,
    DevelopmentOnly,
    PreparedCandidate,
    PostJoinPreRelease,
    PublicRelease,
}

impl PublicationStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotProven => "not_proven",
            Self::DevelopmentOnly => "development_only",
            Self::PreparedCandidate => "prepared_candidate",
            Self::PostJoinPreRelease => "post_join_pre_release",
            Self::PublicRelease => "public_release",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelState {
    Available,
    Unavailable,
    NotProven,
}

impl ChannelState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "AVAILABLE",
            Self::Unavailable => "UNAVAILABLE",
            Self::NotProven => "NOT_PROVEN",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub status: ObservationStatus,
    pub source: Option<String>,
    pub observed_at: Option<String>,
    pub limitation: Option<String>,
    pub development_sha: Option<String>,
    pub publication_sha: Option<String>,
    pub prepared_swarm_sha: Option<String>,
    pub publication_join_sha: Option<String>,
    pub public_release_tag: Option<String>,
    pub stage: PublicationStage,
    pub channels: BTreeMap<String, ChannelState>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub schema: u32,
    #[serde(rename = "static")]
    pub static_topology: StaticTopology,
    pub observation: Observation,
    pub sources: BTreeMap<String, SourceDigest>,
    pub projection_digest: String,
}
