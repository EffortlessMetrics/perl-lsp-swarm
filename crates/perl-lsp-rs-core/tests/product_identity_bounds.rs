use perl_lsp_rs_core::product_identity::{
    ArtifactRole, BinaryIdentityInput, BinaryIdentityPacketV1, BuildIdentityState,
};
use serde_json::Value;

fn revision(character: char) -> String {
    character.to_string().repeat(40)
}

fn digest(character: char) -> String {
    character.to_string().repeat(64)
}

fn externally_bound_input(artifact_role: Option<ArtifactRole>) -> BinaryIdentityInput {
    BinaryIdentityInput {
        source_revision: Some(revision('a')),
        source_tree_digest: Some(digest('b')),
        target: Some("x86_64-unknown-linux-gnu".to_owned()),
        profile: Some("release".to_owned()),
        artifact_role,
        artifact_digest: Some(digest('c')),
        candidate_identity: Some("rc1".to_owned()),
    }
}

#[test]
fn unknown_artifact_role_is_equivalent_to_missing_role() {
    let missing = BinaryIdentityPacketV1::server("0.18.0", externally_bound_input(None));
    let unknown = BinaryIdentityPacketV1::server(
        "0.18.0",
        externally_bound_input(Some(ArtifactRole::Unknown)),
    );

    assert_eq!(unknown.artifact, missing.artifact);
    assert_eq!(unknown.limitations, missing.limitations);
    assert!(unknown.limitations.iter().any(|limitation| limitation == "artifact_role_not_proven"));
}

#[test]
fn packet_deserialization_rejects_unknown_envelope_fields() -> Result<(), serde_json::Error> {
    let mut packet: Value = serde_json::from_str(
        &BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default()).to_json()?,
    )?;
    packet["futureIdentityField"] = Value::Bool(true);

    assert!(serde_json::from_value::<BinaryIdentityPacketV1>(packet).is_err());
    Ok(())
}

#[test]
fn packet_deserialization_rejects_unknown_nested_fields() -> Result<(), serde_json::Error> {
    let mut packet: Value = serde_json::from_str(
        &BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default()).to_json()?,
    )?;
    packet["artifact"]["futureArtifactField"] = Value::String("ignored".to_owned());

    assert!(serde_json::from_value::<BinaryIdentityPacketV1>(packet).is_err());
    Ok(())
}

#[test]
fn packet_deserialization_accepts_declared_optional_fields_when_omitted()
-> Result<(), serde_json::Error> {
    let packet = BinaryIdentityPacketV1::server("0.18.0", BinaryIdentityInput::default());
    let decoded: BinaryIdentityPacketV1 = serde_json::from_str(&packet.to_json()?)?;

    assert_eq!(decoded.build.source_revision, None);
    assert_eq!(decoded.artifact.digest, None);
    assert!(decoded.limitations.iter().any(|limitation| limitation == "artifact_role_not_proven"));
    Ok(())
}

#[test]
fn oversized_build_and_candidate_inputs_fail_closed() {
    let oversized_target = BinaryIdentityPacketV1::server(
        "0.18.0",
        BinaryIdentityInput {
            source_revision: Some(revision('a')),
            source_tree_digest: Some(digest('b')),
            target: Some(format!("x86_64-{}", "x".repeat(129))),
            profile: Some("release".to_owned()),
            artifact_role: Some(ArtifactRole::Archive),
            artifact_digest: Some(digest('c')),
            candidate_identity: Some("rc1".to_owned()),
        },
    );
    assert_eq!(
        oversized_target.build.identity_state,
        BuildIdentityState::NotProven,
        "oversized target must invalidate build authority"
    );
    assert_eq!(oversized_target.build.target, None, "oversized target must not be emitted");
    assert!(
        oversized_target.limitations.iter().any(|limitation| limitation == "target_triple_invalid"),
        "limitations={:?}",
        oversized_target.limitations
    );

    let oversized_candidate = BinaryIdentityPacketV1::server(
        "0.18.0",
        BinaryIdentityInput {
            source_revision: Some(revision('a')),
            source_tree_digest: Some(digest('b')),
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            profile: Some("release".to_owned()),
            artifact_role: Some(ArtifactRole::Archive),
            artifact_digest: Some(digest('c')),
            candidate_identity: Some("x".repeat(129)),
        },
    );
    assert_eq!(
        oversized_candidate.build.identity_state,
        BuildIdentityState::Exact,
        "invalid artifact metadata must not rewrite valid build evidence"
    );
    assert_eq!(
        oversized_candidate.artifact.candidate_identity, None,
        "oversized candidate must not be emitted"
    );
    assert!(
        oversized_candidate
            .limitations
            .iter()
            .any(|limitation| limitation == "candidate_identity_invalid"),
        "limitations={:?}",
        oversized_candidate.limitations
    );
}
