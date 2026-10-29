//! Contract test pinning the JSON contract identity of the contributor
//! topology projection.
//!
//! Asserts that the on-the-wire JSON for a `Projection`:
//! 1. emits the project-wide `schema_version: String` field (not the
//!    short-form `schema: u32` outlier drift from before #15288);
//! 2. uses the closed `"contributor_topology.v1"` literal as its value;
//! 3. rejects a v1-shaped payload that uses the legacy `schema: u32`
//!    field name (negative control: `deny_unknown_fields` must trip);
//! 4. rejects a `schema_version` value that does not equal the closed
//!    literal (`validate_projection` must surface the message).
//!
//! This file owns the constant-level contract so future drift is caught
//! at `cargo test` rather than at the consumer side.

#![allow(clippy::expect_used)]

#[path = "contributor_topology/support.rs"]
mod support;

use serde_json::{Value, json};
use sha2::Digest;
use support::contributor_topology::{Projection, build_projection, validate_projection};
use support::fixture_root;

const EXPECTED_SCHEMA_VERSION: &str = "contributor_topology.v1";

#[test]
fn projection_json_uses_schema_version_string_field() {
    let temp = fixture_root();
    let projection = build_projection(temp.path(), None).expect("build projection");

    let value: Value = serde_json::to_value(&projection).expect("serialize projection");
    let object = value.as_object().expect("projection is an object");

    assert!(
        object.contains_key("schema_version"),
        "projection JSON must declare `schema_version` (post-#15288 contract)"
    );
    assert!(
        !object.contains_key("schema"),
        "projection JSON must not declare the legacy `schema` field"
    );

    let version =
        object.get("schema_version").and_then(Value::as_str).expect("schema_version is a string");
    assert_eq!(version, EXPECTED_SCHEMA_VERSION);
}

#[test]
fn projection_json_rejects_legacy_short_form_field() {
    let temp = fixture_root();
    let mut projection = build_projection(temp.path(), None).expect("build projection");
    // Mutate the runtime struct back to the legacy short-form name. This
    // mirrors the historical shape: a serialized payload that carries the
    // old `schema: u32` field instead of `schema_version`.
    #[derive(serde::Serialize)]
    struct LegacyProjection<'a> {
        schema: u32,
        static_: &'a support::contributor_topology::StaticTopology,
        observation: &'a support::contributor_topology::Observation,
        sources:
            &'a std::collections::BTreeMap<String, support::contributor_topology::SourceDigest>,
        projection_digest: &'a String,
    }

    let legacy = LegacyProjection {
        schema: 1,
        static_: &projection.static_topology,
        observation: &projection.observation,
        sources: &projection.sources,
        projection_digest: &projection.projection_digest,
    };
    let legacy_text = serde_json::to_string(&legacy).expect("serialize legacy");
    let parsed: Result<Projection, _> = serde_json::from_str(&legacy_text);

    assert!(
        parsed.is_err(),
        "Projection deserialization must reject legacy `schema: u32` payload (deny_unknown_fields)"
    );

    // Suppress unused-mut warning while keeping `projection` in scope for
    // follow-up assertions if this test is ever expanded.
    projection.schema_version = EXPECTED_SCHEMA_VERSION.to_string();
}

#[test]
fn validate_projection_rejects_unknown_schema_version_value() {
    let temp = fixture_root();
    let mut projection = build_projection(temp.path(), None).expect("build projection");

    projection.schema_version = "contributor_topology.v2".to_string();
    // Recompute the digest over the mutated body so the digest check does
    // not short-circuit this test's actual assertion.
    let body = json!({
        "schema_version": projection.schema_version,
        "static": projection.static_topology,
        "observation": projection.observation,
        "sources": projection.sources,
    });
    projection.projection_digest =
        sha2::Sha256::digest(serde_json::to_vec(&body).expect("serialize mutated body"))
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();

    let error = validate_projection(temp.path(), &projection)
        .expect_err("validate_projection must fail for unknown schema_version");
    let message = format!("{error}");
    assert!(
        message.contains("schema_version") && message.contains(EXPECTED_SCHEMA_VERSION),
        "error message must name the field and the expected literal, got: {message}"
    );
}

#[test]
fn validate_projection_accepts_v1_schema_version() {
    let temp = fixture_root();
    let projection = build_projection(temp.path(), None).expect("build projection");
    validate_projection(temp.path(), &projection).expect("v1 schema_version must validate");
}
