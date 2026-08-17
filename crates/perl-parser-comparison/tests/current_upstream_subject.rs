#![cfg(feature = "current-upstream")]

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use perl_parser_comparison::{
    CURRENT_UPSTREAM_SUBJECT, CurrentUpstreamAdapter, CurrentUpstreamParseMode,
    SUBJECT_MANIFEST_TOML, validate_exact_package_requirement,
};
use tree_sitter::{InputEdit, Point};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn exact_subject_manifest_is_one_complete_authority() {
    let subject = &CURRENT_UPSTREAM_SUBJECT;

    assert_eq!(subject.schema_version(), "parser-comparison-subject.v1");
    assert_eq!(subject.subject_role(), "current_upstream_tree_sitter");
    assert_eq!(subject.package_name(), "ts-parser-perl");
    assert_eq!(subject.package_version(), "1.2.1");
    assert_eq!(subject.package_requirement(), "=1.2.1");
    assert_eq!(
        subject.package_checksum(),
        "d125f7bfdd1fd82a7e87d2e85793f486ad1b5f465144e9e22132dbe5bd80e694"
    );
    assert_eq!(subject.upstream_tag(), "v1.2.1");
    assert_eq!(subject.upstream_commit(), "c3e17b31179bf8f658c9f37c7a3ea6a202212d5a");
    assert_eq!(subject.tree_sitter_runtime_version(), "0.26.12");
    assert_eq!(subject.tree_sitter_language_version(), "0.1.7");
    assert_eq!(subject.upstream_rust_version(), "1.77");
    assert_eq!(
        subject.semantic_digest(),
        "sha256:750bf42fd1190088c649e5c0ab50995b8895a8002ac15d6bbe560721a97134b2"
    );
    assert_eq!(
        subject.semantic_identity(),
        format!("current_upstream_tree_sitter:{}", subject.semantic_digest())
    );
    assert_eq!(subject.refresh_owner(), "#7255");
    assert!(subject.claim_boundary().contains("no consumer migration"));
}

#[test]
fn checked_in_manifest_is_an_exact_generated_projection() {
    assert_eq!(SUBJECT_MANIFEST_TOML, CURRENT_UPSTREAM_SUBJECT.render_toml());
    assert_eq!(CURRENT_UPSTREAM_SUBJECT.canonical_semantic_json().len(), 542);
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
fn parses_utf8_and_raw_bytes_as_factual_subject_results() -> Result<(), Box<dyn Error>> {
    let mut adapter = CurrentUpstreamAdapter::new()?;

    let utf8 = adapter.parse_str("my $snowman = \"☃\";\n", None)?;
    assert_eq!(utf8.mode(), CurrentUpstreamParseMode::Fresh);
    assert!(!utf8.root_has_error());
    assert_eq!(utf8.source_len(), "my $snowman = \"☃\";\n".len());
    assert_eq!(utf8.subject(), &CURRENT_UPSTREAM_SUBJECT);
    assert_eq!(utf8.root_byte_range(), 0..utf8.source_len());

    let bytes = adapter.parse_bytes(b"sub answer { return 42; }\n", None)?;
    assert_eq!(bytes.mode(), CurrentUpstreamParseMode::Fresh);
    assert!(!bytes.root_kind().is_empty());
    assert!(bytes.root_named_child_count() > 0);
    assert!(bytes.bounded_root_sexp().as_str().contains("sub"));
    Ok(())
}

#[test]
fn recovery_markers_remain_facts_not_correctness_outcomes() -> Result<(), Box<dyn Error>> {
    let mut adapter = CurrentUpstreamAdapter::new()?;
    let result = adapter.parse_str("my $x = ;\n@@@\n", None)?;

    assert_eq!(result.mode(), CurrentUpstreamParseMode::Fresh);
    assert!(result.root_has_error());
    assert_eq!(result.subject(), &CURRENT_UPSTREAM_SUBJECT);
    Ok(())
}

#[test]
fn reuses_an_edited_old_tree_with_explicit_lifecycle_identity() -> Result<(), Box<dyn Error>> {
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

    assert_eq!(first.mode(), CurrentUpstreamParseMode::Fresh);
    assert_eq!(second.mode(), CurrentUpstreamParseMode::EditedOldTree);
    assert_eq!(first.subject(), second.subject());
    assert_eq!(second.source_len(), updated.len());
    assert!(!second.root_has_error());
    Ok(())
}

#[test]
fn caller_supplied_old_tree_is_not_relabelled_fresh() -> Result<(), Box<dyn Error>> {
    let mut adapter = CurrentUpstreamAdapter::new()?;
    let source = b"my $x = 1;\n";
    let first = adapter.parse_bytes(source, None)?;
    let second = adapter.parse_bytes(source, Some(first.tree()))?;

    assert_eq!(second.mode(), CurrentUpstreamParseMode::ReusedOldTree);
    Ok(())
}

#[test]
fn root_projection_is_bounded_and_reports_omissions() -> Result<(), Box<dyn Error>> {
    let mut adapter = CurrentUpstreamAdapter::new()?;
    let source = "my $x = 1;\n".repeat(1_000);
    let result = adapter.parse_str(&source, None)?;
    let projection = result.bounded_root_sexp();

    assert!(projection.as_str().len() <= 4_096);
    assert!(projection.original_bytes() >= projection.as_str().len());
    assert_eq!(projection.omitted_bytes(), projection.original_bytes() - projection.as_str().len());
    assert!(projection.is_truncated());
    Ok(())
}

#[test]
fn exposes_only_the_exact_pinned_query_and_node_type_surfaces() -> Result<(), Box<dyn Error>> {
    let adapter = CurrentUpstreamAdapter::new()?;

    let _highlights = adapter.highlight_query()?;
    let _injections = adapter.injection_query()?;
    assert!(!adapter.node_types().trim().is_empty());
    assert_eq!(adapter.subject(), &CURRENT_UPSTREAM_SUBJECT);
    Ok(())
}

#[test]
fn manifest_and_lock_bind_the_exact_crates_io_subject() -> Result<(), Box<dyn Error>> {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))?;
    assert!(manifest.contains(&format!(
        "ts-parser-perl = {{ version = \"{}\", optional = true }}",
        CURRENT_UPSTREAM_SUBJECT.package_requirement()
    )));
    assert!(!manifest.contains("ts-parser-perl = \"1.2.1\""));

    let lock = fs::read_to_string(workspace_root().join("Cargo.lock"))?;
    let package_marker = format!(
        "name = \"{}\"\nversion = \"{}\"",
        CURRENT_UPSTREAM_SUBJECT.package_name(),
        CURRENT_UPSTREAM_SUBJECT.package_version()
    );
    assert!(lock.contains(&package_marker));
    assert!(
        lock.contains(&format!("checksum = \"{}\"", CURRENT_UPSTREAM_SUBJECT.package_checksum()))
    );
    assert!(lock.contains(&format!(
        "name = \"tree-sitter\"\nversion = \"{}\"",
        CURRENT_UPSTREAM_SUBJECT.tree_sitter_runtime_version()
    )));
    assert!(lock.contains(&format!(
        "name = \"tree-sitter-language\"\nversion = \"{}\"",
        CURRENT_UPSTREAM_SUBJECT.tree_sitter_language_version()
    )));
    Ok(())
}

#[test]
fn adapter_does_not_reintroduce_a_private_comparison_outcome_model() -> Result<(), Box<dyn Error>> {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/current_upstream.rs"))?;

    for forbidden in [
        "CurrentUpstreamExecutionDisposition",
        "AcceptedClean",
        "AcceptedRecovered",
        "ScoredComparison",
        "Verdict::Correct",
    ] {
        assert!(
            !source.contains(forbidden),
            "current-upstream adapter must remain factual: found {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn native_tree_sitter_style_facade_remains_a_distinct_subject() -> Result<(), Box<dyn Error>> {
    let facade_manifest =
        fs::read_to_string(workspace_root().join("crates/tree-sitter-perl-rs/Cargo.toml"))?;

    assert!(!facade_manifest.contains("ts-parser-perl"));
    assert!(facade_manifest.contains("perl-parser-core"));
    Ok(())
}
