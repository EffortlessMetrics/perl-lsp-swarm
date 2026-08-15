use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use perl_parser_comparison::{
    CurrentUpstreamAdapter, CurrentUpstreamExecutionDisposition,
    CurrentUpstreamSubjectIdentity, PACKAGE_CHECKSUM, PACKAGE_REQUIREMENT, PACKAGE_VERSION,
    SUBJECT_IDENTITY_TOML, TREE_SITTER_LANGUAGE_VERSION, TREE_SITTER_RUNTIME_VERSION,
    UPSTREAM_COMMIT, UPSTREAM_TAG, validate_exact_package_requirement,
};
use tree_sitter::{InputEdit, Point};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn exact_subject_identity_matches_the_reviewed_release() {
    let identity = CurrentUpstreamSubjectIdentity::current();

    assert_eq!(identity.subject_role, "current_upstream_tree_sitter");
    assert_eq!(identity.package_version, PACKAGE_VERSION);
    assert_eq!(identity.package_requirement, PACKAGE_REQUIREMENT);
    assert_eq!(identity.package_checksum, PACKAGE_CHECKSUM);
    assert_eq!(identity.upstream_tag, UPSTREAM_TAG);
    assert_eq!(identity.upstream_commit, UPSTREAM_COMMIT);
    assert_eq!(
        identity.tree_sitter_runtime_version,
        TREE_SITTER_RUNTIME_VERSION
    );
    assert_eq!(
        identity.tree_sitter_language_version,
        TREE_SITTER_LANGUAGE_VERSION
    );

    let semantic_identity = identity.semantic_identity();
    assert!(semantic_identity.contains(PACKAGE_VERSION));
    assert!(semantic_identity.contains(PACKAGE_CHECKSUM));
    assert!(semantic_identity.contains(UPSTREAM_COMMIT));

    let receipt = identity.render_json_receipt();
    assert!(receipt.contains("\"subject_role\": \"current_upstream_tree_sitter\""));
    assert!(receipt.contains(PACKAGE_CHECKSUM));
}

#[test]
fn reviewed_identity_file_carries_the_same_exact_subject() {
    assert!(SUBJECT_IDENTITY_TOML.contains("package_requirement = \"=1.2.1\""));
    assert!(SUBJECT_IDENTITY_TOML.contains(PACKAGE_CHECKSUM));
    assert!(SUBJECT_IDENTITY_TOML.contains(UPSTREAM_COMMIT));
    assert!(SUBJECT_IDENTITY_TOML.contains("refresh_owner = \"#7255\""));
}

#[test]
fn floating_or_compatible_requirements_are_rejected() {
    assert!(validate_exact_package_requirement("=1.2.1").is_ok());

    for requirement in ["1.2.1", "^1.2.1", "~1.2.1", ">=1.2.1", "*"] {
        let error = validate_exact_package_requirement(requirement)
            .expect_err("non-exact requirement must fail");
        assert_eq!(error.expected(), "=1.2.1");
        assert_eq!(error.actual(), requirement);
    }
}

#[test]
fn parses_utf8_and_raw_bytes_through_the_exact_subject() -> Result<(), Box<dyn Error>> {
    let mut adapter = CurrentUpstreamAdapter::new()?;

    let utf8 = adapter.parse_str("my $snowman = \"☃\";\n", None)?;
    assert_eq!(
        utf8.disposition(),
        CurrentUpstreamExecutionDisposition::AcceptedClean
    );
    assert!(!utf8.tree().root_node().has_error());
    assert_eq!(utf8.subject(), CurrentUpstreamSubjectIdentity::current());

    let bytes = adapter.parse_bytes(b"sub answer { return 42; }\n", None)?;
    assert_eq!(
        bytes.disposition(),
        CurrentUpstreamExecutionDisposition::AcceptedClean
    );
    assert!(bytes.root_sexp().contains("sub"));
    Ok(())
}

#[test]
fn reuses_an_edited_old_tree_without_changing_subject_identity() -> Result<(), Box<dyn Error>> {
    let mut adapter = CurrentUpstreamAdapter::new()?;
    let initial = b"my $x = 1;\n";
    let updated = b"my $x = 10;\n";
    let first = adapter.parse_bytes(initial, None)?;

    let edit = InputEdit {
        start_byte: 9,
        old_end_byte: 9,
        new_end_byte: 10,
        start_position: Point { row: 0, column: 9 },
        old_end_position: Point { row: 0, column: 9 },
        new_end_position: Point { row: 0, column: 10 },
    };
    let second = adapter.parse_edited(first.tree(), &edit, updated)?;

    assert_eq!(
        second.disposition(),
        CurrentUpstreamExecutionDisposition::AcceptedClean
    );
    assert_eq!(first.subject(), second.subject());
    assert!(!second.tree().root_node().has_error());
    Ok(())
}

#[test]
fn exposes_the_pinned_query_and_node_type_surfaces() -> Result<(), Box<dyn Error>> {
    let adapter = CurrentUpstreamAdapter::new()?;

    let _highlights = adapter.highlight_query()?;
    let _injections = adapter.injection_query()?;
    assert!(!adapter.node_types().trim().is_empty());
    Ok(())
}

#[test]
fn manifest_and_lock_bind_the_exact_crates_io_subject() -> Result<(), Box<dyn Error>> {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))?;
    assert!(manifest.contains("ts-parser-perl = \"=1.2.1\""));
    assert!(!manifest.contains("ts-parser-perl = \"1.2.1\""));

    let lock = fs::read_to_string(workspace_root().join("Cargo.lock"))?;
    let package_marker = format!("name = \"ts-parser-perl\"\nversion = \"{PACKAGE_VERSION}\"");
    assert!(lock.contains(&package_marker));
    assert!(lock.contains(&format!("checksum = \"{PACKAGE_CHECKSUM}\"")));
    Ok(())
}

#[test]
fn native_tree_sitter_style_facade_remains_a_distinct_subject() -> Result<(), Box<dyn Error>> {
    let facade_manifest = fs::read_to_string(
        workspace_root().join("crates/tree-sitter-perl-rs/Cargo.toml"),
    )?;

    assert!(!facade_manifest.contains("ts-parser-perl"));
    assert!(facade_manifest.contains("perl-parser-core"));
    Ok(())
}
