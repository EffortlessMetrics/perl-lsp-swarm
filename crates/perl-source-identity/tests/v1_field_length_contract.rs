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
    assert_eq!(
        project.as_wire(),
        "project:sha256:63a294744a9a79203fb98384c20901a5c6edf8fb8b49b8b4e43e1bc60d1967f5"
    );
    assert_eq!(
        root.as_wire(),
        "root:sha256:accea0a1360d4f4239c89a77e2d87df37d86fa1a1f1332a23284c6b5cedb1955"
    );
    assert_eq!(
        src.as_wire(),
        "src:sha256:69548d78dbfaf02aa895fd250b3b65fda58a7d17d8922c6dc6e6e44534e876a5"
    );
}

#[test]
fn try_constructors_share_infallible_results_for_small_inputs() {
    assert_eq!(
        ProjectId::from_canonical_name("x"),
        ProjectId::try_from_canonical_name("x").expect("small input")
    );
}
