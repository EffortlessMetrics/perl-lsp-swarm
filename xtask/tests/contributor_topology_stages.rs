//! Fixture-driven projection tests; localized expect calls keep setup readable.
#![allow(clippy::expect_used)]

#[path = "contributor_topology/support.rs"]
mod support;

use serde_json::json;
use support::contributor_topology::{
    ChannelState, ObservationStatus, PublicationStage, build_projection, render_human,
    validate_projection,
};
use support::{captured_observation, fixture_root, write_observation};

#[test]
fn missing_observation_is_not_proven() {
    let temp = fixture_root();
    let projection = build_projection(temp.path(), None).expect("build projection");
    assert_eq!(projection.observation.status, ObservationStatus::NotProven);
    assert_eq!(projection.observation.stage, PublicationStage::NotProven);
    assert!(projection.observation.development_sha.is_none());
    assert_eq!(
        projection.static_topology.development_repository,
        "EffortlessMetrics/perl-lsp-swarm"
    );
    validate_projection(temp.path(), &projection).expect("validate projection");
}

#[test]
fn development_only_does_not_imply_public_availability() {
    let temp = fixture_root();
    let path = write_observation(temp.path(), "development.json", &captured_observation(&[]));
    let projection = build_projection(temp.path(), Some(&path)).expect("build projection");
    assert_eq!(projection.observation.stage, PublicationStage::DevelopmentOnly);
    assert!(projection.observation.public_release_tag.is_none());
    assert!(projection.observation.channels.is_empty());
}

#[test]
fn prepared_candidate_is_distinct() {
    let temp = fixture_root();
    let value = captured_observation(&[(
        "prepared_swarm_sha",
        json!("cccccccccccccccccccccccccccccccccccccccc"),
    )]);
    let path = write_observation(temp.path(), "prepared.json", &value);
    let projection = build_projection(temp.path(), Some(&path)).expect("build projection");
    assert_eq!(projection.observation.stage, PublicationStage::PreparedCandidate);
    assert!(projection.observation.publication_join_sha.is_none());
}

#[test]
fn post_join_pre_release_is_distinct() {
    let temp = fixture_root();
    let value = captured_observation(&[
        ("prepared_swarm_sha", json!("cccccccccccccccccccccccccccccccccccccccc")),
        ("publication_join_sha", json!("dddddddddddddddddddddddddddddddddddddddd")),
    ]);
    let path = write_observation(temp.path(), "joined.json", &value);
    let projection = build_projection(temp.path(), Some(&path)).expect("build projection");
    assert_eq!(projection.observation.stage, PublicationStage::PostJoinPreRelease);
    assert!(projection.observation.public_release_tag.is_none());
}

#[test]
fn public_release_keeps_channel_state_separate() {
    let temp = fixture_root();
    let value = captured_observation(&[
        ("prepared_swarm_sha", json!("cccccccccccccccccccccccccccccccccccccccc")),
        ("publication_join_sha", json!("dddddddddddddddddddddddddddddddddddddddd")),
        ("public_release_tag", json!("v0.18.0")),
        ("channels", json!({"crates_io": "AVAILABLE", "open_vsx": "NOT_PROVEN"})),
    ]);
    let path = write_observation(temp.path(), "released.json", &value);
    let projection = build_projection(temp.path(), Some(&path)).expect("build projection");
    assert_eq!(projection.observation.stage, PublicationStage::PublicRelease);
    assert_eq!(projection.observation.channels.get("crates_io"), Some(&ChannelState::Available));
    assert_eq!(projection.observation.channels.get("open_vsx"), Some(&ChannelState::NotProven));
    assert!(render_human(&projection).contains("stage: public_release"));
}
