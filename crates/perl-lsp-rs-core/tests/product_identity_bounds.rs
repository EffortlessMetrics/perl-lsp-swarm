use perl_lsp_rs_core::product_identity::{
    ArtifactRole, BinaryIdentityInput, BinaryIdentityPacketV1, BuildIdentityState,
};

fn revision(character: char) -> String {
    character.to_string().repeat(40)
}

fn digest(character: char) -> String {
    character.to_string().repeat(64)
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
        oversized_target
            .limitations
            .iter()
            .any(|limitation| limitation == "target_triple_invalid"),
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
        oversized_candidate.artifact.candidate_identity,
        None,
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
