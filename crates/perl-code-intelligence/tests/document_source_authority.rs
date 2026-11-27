//! Integration tests for document source authority (#7218).

use perl_code_intelligence::{
    DocumentAuthorityError, DocumentSourceAuthority, DocumentSourceKind, ResolvedDocumentAuthority,
};
use perl_source_identity::{
    ContentDigest, ContentRevision, LogicalSourceId, ProjectId, RootRelativeLogicalPath,
    SourceGeneration, WorkspaceRootId,
};
use perl_test_must::must_with;

fn fixture_revision(content: &[u8]) -> ContentRevision {
    let project = ProjectId::from_canonical_name("https://example.com/demo");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root-a");
    let path = must_with(RootRelativeLogicalPath::parse("lib/Demo.pm"), "path");
    let logical = LogicalSourceId::from_root_and_logical_path(&root, &path);
    ContentRevision::new(logical, ContentDigest::of_bytes(content))
}

#[test]
fn open_snapshot_differs_from_disk_observation() {
    let open = fixture_revision(b"open\n");
    let disk = fixture_revision(b"disk\n");
    let authority = must_with(
        DocumentSourceAuthority::open_retained(
            open.logical_source_id.clone(),
            open.clone(),
            SourceGeneration::known("1"),
        ),
        "open authority",
    );
    assert!(!authority.accepts_revision(&disk));
}

#[test]
fn supplied_snapshot_differs_from_open_and_disk() {
    let supplied = fixture_revision(b"supplied\n");
    let open = fixture_revision(b"open\n");
    let disk = fixture_revision(b"disk\n");
    let authority = must_with(
        DocumentSourceAuthority::caller_supplied(
            supplied.logical_source_id.clone(),
            supplied.clone(),
            SourceGeneration::known("9"),
        ),
        "supplied authority",
    );
    assert_eq!(
        DocumentAuthorityError::SuppliedRevisionMismatch,
        ResolvedDocumentAuthority::admit_caller_supplied(
            authority.clone(),
            &open,
            SourceGeneration::known("9"),
        )
        .unwrap_err()
    );
    assert_eq!(
        DocumentAuthorityError::SilentDiskFallbackRefused,
        ResolvedDocumentAuthority::refuse_disk_for_supplied(&authority, &disk).unwrap_err()
    );
}

#[test]
fn document_source_kinds_remain_distinct_in_serialization() {
    let authority = must_with(
        DocumentSourceAuthority::caller_supplied(
            fixture_revision(b"a").logical_source_id,
            fixture_revision(b"a"),
            SourceGeneration::known("1"),
        ),
        "authority",
    );
    let json = must_with(serde_json::to_string(&authority), "json");
    assert!(json.contains("CallerSupplied"));
    let decoded: DocumentSourceAuthority = must_with(serde_json::from_str(&json), "decode");
    assert_eq!(decoded.kind, DocumentSourceKind::CallerSupplied);
}
