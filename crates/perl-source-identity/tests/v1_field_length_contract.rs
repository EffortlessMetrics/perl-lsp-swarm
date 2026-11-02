//! Integration tests for the v1 field-length contract (#15064).

#![allow(clippy::expect_used)]

use perl_source_identity::{
    ContentDigest, LogicalSourceId, ProjectId, SOURCE_IDENTITY_V1_MAX_FIELD_BYTES, WorkspaceRootId,
};

#[test]
fn v1_max_field_bytes_is_u32_max() {
    assert_eq!(SOURCE_IDENTITY_V1_MAX_FIELD_BYTES, u32::MAX);
}

#[test]
fn representative_v1_wires_remain_byte_identical() {
    let digest = ContentDigest::of_bytes(b"package App;\n1;\n");
    assert_eq!(digest, ContentDigest::try_of_bytes(b"package App;\n1;\n").expect("same bound"));

    let project = ProjectId::from_canonical_name("acme/widget");
    assert_eq!(project, ProjectId::try_from_canonical_name("acme/widget").expect("same bound"));

    let root = WorkspaceRootId::from_project_and_root_key(&project, "main");
    assert_eq!(
        root,
        WorkspaceRootId::try_from_project_and_root_key(&project, "main").expect("same bound")
    );

    let src = LogicalSourceId::from_root_and_path(&root, "lib/Widget.pm");
    assert_eq!(
        src,
        LogicalSourceId::try_from_root_and_path(&root, "lib/Widget.pm").expect("same bound")
    );

    // Pinned on current v1 encoding (must not change when only adding bounds).
    assert_eq!(
        digest.as_wire(),
        "sha256:3e72a28a4048315561af696f9c5c7b2d663ee4fc0e8b486103f0962a882e85c8"
    );
}

#[test]
fn try_constructors_share_infallible_results_for_small_inputs() {
    assert!(ProjectId::try_from_canonical_name("x").is_ok());
}
