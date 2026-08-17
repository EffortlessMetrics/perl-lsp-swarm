//! Fixture-driven projection tests; localized expect calls keep setup readable.
#![allow(clippy::expect_used)]

#[path = "contributor_topology/support.rs"]
mod support;

use serde_json::{Value, json};
use support::contributor_topology::{
    ObservationStatus, PublicationStage, build_projection, validate_projection,
};
use support::{captured_observation, fixture_root, write_observation};

#[test]
fn observation_repository_mismatch_fails() {
    let temp = fixture_root();
    let value =
        captured_observation(&[("development_repository", json!("EffortlessMetrics/perl-lsp"))]);
    let path = write_observation(temp.path(), "mismatch.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}

#[test]
fn available_channel_without_release_tag_fails() {
    let temp = fixture_root();
    let value = captured_observation(&[("channels", json!({"crates_io": "AVAILABLE"}))]);
    let path = write_observation(temp.path(), "bad-channel.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}

#[test]
fn join_without_prepared_candidate_fails() {
    let temp = fixture_root();
    let value = captured_observation(&[(
        "publication_join_sha",
        json!("dddddddddddddddddddddddddddddddddddddddd"),
    )]);
    let path = write_observation(temp.path(), "bad-join.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}

#[test]
fn not_proven_can_retain_partial_observation_without_stage_claim() {
    let temp = fixture_root();
    let value = captured_observation(&[
        ("status", json!("NOT_PROVEN")),
        ("limitation", json!("publication ruleset API unavailable")),
        ("publication_sha", Value::Null),
    ]);
    let path = write_observation(temp.path(), "partial.json", &value);
    let projection = build_projection(temp.path(), Some(&path)).expect("build projection");
    assert_eq!(projection.observation.status, ObservationStatus::NotProven);
    assert_eq!(projection.observation.stage, PublicationStage::NotProven);
    assert!(projection.observation.development_sha.is_some());
    assert!(projection.observation.publication_sha.is_none());
}

#[test]
fn channel_input_order_does_not_change_projection_digest() {
    let temp = fixture_root();
    let shared = [
        ("prepared_swarm_sha", json!("cccccccccccccccccccccccccccccccccccccccc")),
        ("publication_join_sha", json!("dddddddddddddddddddddddddddddddddddddddd")),
        ("public_release_tag", json!("v0.18.0")),
    ];
    let mut first = captured_observation(&shared);
    first.as_object_mut().expect("first object").insert(
        "channels".to_string(),
        serde_json::from_str(r#"{"open_vsx":"NOT_PROVEN","crates_io":"AVAILABLE"}"#)
            .expect("first channels"),
    );
    let mut second = captured_observation(&shared);
    second.as_object_mut().expect("second object").insert(
        "channels".to_string(),
        serde_json::from_str(r#"{"crates_io":"AVAILABLE","open_vsx":"NOT_PROVEN"}"#)
            .expect("second channels"),
    );
    let first_path = write_observation(temp.path(), "first.json", &first);
    let second_path = write_observation(temp.path(), "second.json", &second);
    let first_projection =
        build_projection(temp.path(), Some(&first_path)).expect("first projection");
    let second_projection =
        build_projection(temp.path(), Some(&second_path)).expect("second projection");
    assert_eq!(first_projection.projection_digest, second_projection.projection_digest);
}

#[test]
fn unknown_observation_field_fails_closed() {
    let temp = fixture_root();
    let mut value = captured_observation(&[]);
    value
        .as_object_mut()
        .expect("observation object")
        .insert("unexpected".to_string(), json!(true));
    let path = write_observation(temp.path(), "unknown.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}

#[test]
fn unknown_channel_fails_closed() {
    let temp = fixture_root();
    let value = captured_observation(&[("channels", json!({"future_channel": "NOT_PROVEN"}))]);
    let path = write_observation(temp.path(), "unknown-channel.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}

#[test]
fn omitted_channels_become_explicit_not_proven() {
    let temp = fixture_root();
    let path = write_observation(temp.path(), "channels.json", &captured_observation(&[]));
    let projection = build_projection(temp.path(), Some(&path)).expect("build projection");
    assert_eq!(projection.observation.channels.len(), 4);
    assert!(
        projection
            .observation
            .channels
            .values()
            .all(|state| *state == support::contributor_topology::ChannelState::NotProven)
    );
}

#[test]
fn empty_not_proven_provenance_is_rejected_by_check_validation() {
    let temp = fixture_root();
    let mut projection = build_projection(temp.path(), None).expect("build projection");
    projection.observation.source = Some(String::new());
    projection.observation.observed_at = Some(String::new());
    assert!(validate_projection(temp.path(), &projection).is_err());
}

#[test]
fn missing_channel_is_rejected_by_check_validation() {
    let temp = fixture_root();
    let mut projection = build_projection(temp.path(), None).expect("build projection");
    projection.observation.channels.remove("open_vsx");
    assert!(validate_projection(temp.path(), &projection).is_err());
}

#[test]
fn tampered_stage_is_rejected_by_check_validation() {
    let temp = fixture_root();
    let mut projection = build_projection(temp.path(), None).expect("build projection");
    projection.observation.stage = PublicationStage::DevelopmentOnly;
    assert!(validate_projection(temp.path(), &projection).is_err());
}

#[test]
fn malformed_observation_sha_fails_closed() {
    let temp = fixture_root();
    let value = captured_observation(&[("development_sha", json!("not-a-sha"))]);
    let path = write_observation(temp.path(), "bad-sha.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}

#[test]
fn proven_observation_cannot_carry_a_limitation() {
    let temp = fixture_root();
    let value = captured_observation(&[("limitation", json!("not actually proven"))]);
    let path = write_observation(temp.path(), "bad-limitation.json", &value);
    assert!(build_projection(temp.path(), Some(&path)).is_err());
}
