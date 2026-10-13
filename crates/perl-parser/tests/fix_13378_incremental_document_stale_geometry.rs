//! Shift-left proof for #13378: experimental `IncrementalDocument` must fail
//! closed to a full fresh parse instead of publishing leaf-patched trees or
//! stale range-cache keys.
//!
//! These tests fail if a mutation:
//! - restores leaf-only `location.end` repair (`fast_token_update`);
//! - skips cache invalidation after a length-changing edit;
//! - restores the partial `NodeKind` traversal / `adjust_node_position`;
//! - treats an invalid edit as a successful new version;
//! - counts a clone or cache match as retained identity (`nodes_reused` /
//!   `cache_hits`);
//! - leaves this historical engine marked production-eligible contrary to #7292.
//!
//! CI feature detection: `feature_cfgs_in_source` scans for inner `#![cfg(...]`
//! lines to auto-add `--features incremental`.
#![cfg(feature = "incremental")]

use perl_parser::incremental::ParseSnapshotStrategy;
use perl_parser::incremental::incremental_document::{
    IncrementalDocument, IncrementalDocumentError, IncrementalEditRefusal,
};
use perl_parser::incremental::incremental_edit::{IncrementalEdit, IncrementalEditSet};
use perl_parser_core::ast::{Node, NodeKind};
use perl_parser_core::parser::Parser;
use perl_tdd_support::must_err_with;
use std::collections::HashSet;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn edit(start: usize, old_end: usize, text: &str) -> IncrementalEdit {
    IncrementalEdit::new(start, old_end, text.to_string())
}

fn fresh_parse(source: &str) -> Result<Node, Box<dyn std::error::Error>> {
    let mut parser = Parser::new(source);
    Ok(parser.parse()?)
}

fn collect_ranges(node: &Node, out: &mut HashSet<(usize, usize)>) {
    out.insert((node.location.start, node.location.end));
    node.for_each_child(|child| collect_ranges(child, out));
}

fn collect_error_found(node: &Node, out: &mut Vec<(usize, usize, Option<String>)>) {
    if let NodeKind::Error { found, .. } = &node.kind {
        let found_text = found.as_ref().map(|token| token.text.to_string());
        out.push((node.location.start, node.location.end, found_text));
    }
    node.for_each_child(|child| collect_error_found(child, out));
}

fn assert_fresh_equivalent(doc: &IncrementalDocument) -> TestResult {
    let fresh = fresh_parse(&doc.source)?;
    assert_eq!(
        *doc.root, fresh,
        "retained IncrementalDocument result must match a fresh parse of the current source"
    );

    let mut live_ranges = HashSet::new();
    collect_ranges(&doc.root, &mut live_ranges);
    for key in doc.subtree_cache.by_range.keys() {
        assert!(
            live_ranges.contains(key),
            "subtree cache retained stale range key {key:?} after edit; live ranges: {live_ranges:?}"
        );
    }

    assert_eq!(
        doc.last_strategy,
        ParseSnapshotStrategy::IncrementalFullFallback,
        "retained edits must report explicit full-fallback identity, not a leaf-patch or subtree-shift strategy"
    );
    assert_eq!(
        doc.metrics.nodes_reused, 0,
        "full fallback must not count clones or cache matches as retained identity"
    );
    assert_eq!(
        doc.metrics.cache_hits, 0,
        "full fallback must not report cache hits as parser work avoided"
    );

    let mut expected_errors = Vec::new();
    let mut actual_errors = Vec::new();
    collect_error_found(&fresh, &mut expected_errors);
    collect_error_found(&doc.root, &mut actual_errors);
    assert_eq!(
        actual_errors, expected_errors,
        "Error::found recovery geometry must match the fresh parse"
    );
    Ok(())
}

fn apply_and_check(
    doc: &mut IncrementalDocument,
    start: usize,
    old_end: usize,
    text: &str,
) -> TestResult {
    let old_keys: HashSet<(usize, usize)> = doc.subtree_cache.by_range.keys().copied().collect();
    let old_len = doc.source.len();
    doc.apply_edit(edit(start, old_end, text))?;
    assert_fresh_equivalent(doc)?;

    let length_changed = (old_end - start) != text.len() || old_len != doc.source.len();
    if length_changed {
        for key in &old_keys {
            if key.0 >= old_end || key.1 > start {
                assert!(
                    !doc.subtree_cache.by_range.contains_key(key) || {
                        let mut live = HashSet::new();
                        collect_ranges(&doc.root, &mut live);
                        live.contains(key)
                    },
                    "old-generation range key {key:?} survived a length-changing edit"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn incremental_document_remains_experimental_non_production() -> TestResult {
    let ledger = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("incremental_authority.json"),
    )?;
    let value: serde_json::Value = serde_json::from_str(&ledger)?;
    let modules = value["modules"].as_array().ok_or("modules array")?;
    let document = modules
        .iter()
        .find(|module| module["module"] == "incremental_document")
        .ok_or("incremental_document ledger row")?;
    assert_eq!(document["status"], "experimental");
    assert_eq!(document["production_eligible"], false);
    assert_eq!(value["canonical"]["production_eligible"], false);
    Ok(())
}

#[test]
fn stale_geometry_helpers_are_absent_from_incremental_document() {
    let src = include_str!("../src/incremental/incremental_document.rs");
    for needle in [
        "fn fast_token_update",
        "fn is_single_token_edit",
        "fn update_token_in_tree",
        "fn update_token_in_children",
        "fn adjust_node_position",
        "fn find_reusable_subtrees",
        "fn find_reusable_for_ranges",
        "fn parse_with_reuse",
        "fn insert_reusable",
        "used_fast_path",
    ] {
        assert!(
            !src.contains(needle),
            "historical stale-geometry helper {needle} must stay removed rather than become a second production engine"
        );
    }
}

#[test]
fn construction_reports_fresh_strategy() -> TestResult {
    let doc = IncrementalDocument::new("my $x = 1;".to_string())?;
    assert_eq!(doc.last_strategy, ParseSnapshotStrategy::Fresh);
    assert_eq!(doc.version, 0);
    Ok(())
}

#[test]
fn length_changing_number_edit_matches_fresh_parse() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let one = source.find('1').ok_or("1")?;
    apply_and_check(&mut doc, one, one + 1, "1000")?;
    assert!(doc.source.contains("1000"));
    assert!(doc.source.contains("my $y = 2;"));
    Ok(())
}

#[test]
fn length_changing_string_edit_matches_fresh_parse() -> TestResult {
    let source = "my $s = 'a'; my $y = 2;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let a = source.find('a').ok_or("a")?;
    apply_and_check(&mut doc, a, a + 1, "abc")?;
    Ok(())
}

#[test]
fn identifier_spelling_and_width_edit_matches_fresh_parse() -> TestResult {
    let source = "my $name = 1; $name;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let name = source.find("name").ok_or("name")?;
    apply_and_check(&mut doc, name, name + 4, "n")?;
    Ok(())
}

#[test]
fn malformed_recovery_geometry_matches_fresh_parse() -> TestResult {
    let source = "my $x = ; my $y = 2;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let y = source.find("$y").ok_or("$y")?;
    apply_and_check(&mut doc, y, y + 2, "$z")?;
    Ok(())
}

#[test]
fn insertion_deletion_and_replacement_inside_a_leaf_match_fresh_parse() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let one = source.find('1').ok_or("1")?;

    let mut insert = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut insert, one + 1, one + 1, "0")?;

    let mut delete = IncrementalDocument::new("my $x = 10; my $y = 2;".to_string())?;
    let ten = delete.source.find("10").ok_or("10")?;
    apply_and_check(&mut delete, ten + 1, ten + 2, "")?;

    let mut replace = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut replace, one, one + 1, "9")?;
    Ok(())
}

#[test]
fn edit_range_extending_beyond_selected_leaf_matches_fresh_parse() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let one = source.find('1').ok_or("1")?;
    let mut doc = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut doc, one, one + 4, "9; m")?;
    Ok(())
}

#[test]
fn replacement_that_changes_lexical_token_class_matches_fresh_parse() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let one = source.find('1').ok_or("1")?;
    let mut doc = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut doc, one, one + 1, "q")?;
    Ok(())
}

#[test]
fn unicode_before_inside_and_after_the_edit_matches_fresh_parse() -> TestResult {
    let source = "my $msg = \"café\"; my $y = 2;";
    let cafe = source.find("café").ok_or("café")?;
    let y = source.find("$y").ok_or("$y")?;

    let mut before = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut before, cafe, cafe, "naïve ")?;

    let mut inside = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut inside, cafe + "caf".len(), cafe + "café".len(), "éé")?;

    let mut after = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut after, y, y + 2, "$z")?;
    Ok(())
}

#[test]
fn crlf_and_following_line_geometry_match_fresh_parse() -> TestResult {
    let source = "my $x = 1;\r\nmy $y = 2;";
    let one = source.find('1').ok_or("1")?;
    let mut doc = IncrementalDocument::new(source.to_string())?;
    apply_and_check(&mut doc, one, one + 1, "1000")?;
    assert!(doc.source.contains("\r\n"));
    Ok(())
}

#[test]
fn invalid_edits_are_refused_without_version_or_cache_change() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let version = doc.version;
    let root = (*doc.root).clone();
    let cache = doc.subtree_cache.by_range.clone();
    let strategy = doc.last_strategy;
    let source_before = doc.source.clone();

    let cases: &[(&str, IncrementalEdit, IncrementalEditRefusal)] = &[
        ("backward", edit(4, 2, "x"), IncrementalEditRefusal::BackwardRange),
        ("out_of_range", edit(0, source.len() + 1, "x"), IncrementalEditRefusal::OutOfRange),
        (
            "mid_codepoint",
            {
                let cafe_source = "my $x = \"é\";";
                let accent = cafe_source.find('é').ok_or("é")?;
                let mut unicode_doc = IncrementalDocument::new(cafe_source.to_string())?;
                let err = must_err_with(
                    unicode_doc.apply_edit(edit(accent + 1, accent + 1, "x")),
                    "mid-codepoint must refuse",
                );
                assert!(
                    matches!(
                        err,
                        IncrementalDocumentError::InvalidEdit {
                            reason: IncrementalEditRefusal::NotCharBoundary,
                            ..
                        }
                    ),
                    "mid-codepoint refusal: {err:?}"
                );
                assert_eq!(unicode_doc.version, 0);
                assert_eq!(unicode_doc.source, cafe_source);
                edit(0, 0, "")
            },
            IncrementalEditRefusal::NotCharBoundary,
        ),
    ];

    for (label, candidate, _reason) in cases {
        if *label == "mid_codepoint" {
            continue;
        }
        let err = must_err_with(doc.apply_edit(candidate.clone()), *label);
        match err {
            IncrementalDocumentError::InvalidEdit { reason, .. } => {
                if *label == "backward" {
                    assert_eq!(reason, IncrementalEditRefusal::BackwardRange);
                } else {
                    assert_eq!(reason, IncrementalEditRefusal::OutOfRange);
                }
            }
            other => return Err(format!("{label}: expected InvalidEdit, got {other:?}").into()),
        }
        assert_eq!(doc.version, version, "{label} advanced version");
        assert_eq!(doc.source, source_before, "{label} mutated source");
        assert_eq!(*doc.root, root, "{label} mutated tree");
        assert_eq!(doc.subtree_cache.by_range, cache, "{label} mutated cache");
        assert_eq!(doc.last_strategy, strategy, "{label} changed strategy");
    }
    Ok(())
}

#[test]
fn reversed_and_non_boundary_batch_edits_are_atomic() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let version = doc.version;

    let mut reversed = IncrementalEditSet::new();
    reversed.add(edit(4, 2, "x"));
    let err = must_err_with(doc.apply_edits(&reversed), "reversed batch");
    assert!(matches!(
        err,
        IncrementalDocumentError::InvalidEdit { reason: IncrementalEditRefusal::BackwardRange, .. }
    ));
    assert_eq!(doc.version, version);
    assert_eq!(doc.source, source);

    let cafe = "my $x = \"é\";";
    let accent = cafe.find('é').ok_or("é")?;
    let mut unicode_doc = IncrementalDocument::new(cafe.to_string())?;
    let mut mixed = IncrementalEditSet::new();
    mixed.add(edit(4, 6, "$value"));
    mixed.add(edit(accent + 1, accent + 1, "x"));
    let err = must_err_with(unicode_doc.apply_edits(&mixed), "mixed unmappable batch");
    assert!(matches!(
        err,
        IncrementalDocumentError::InvalidEdit {
            reason: IncrementalEditRefusal::NotCharBoundary,
            ..
        }
    ));
    assert_eq!(unicode_doc.version, 0);
    assert_eq!(unicode_doc.source, cafe);
    Ok(())
}

#[test]
fn edit_then_undo_and_reopen_match() -> TestResult {
    let source = "my $x = 1; my $y = 2;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let original_root = (*doc.root).clone();
    let one = source.find('1').ok_or("1")?;
    apply_and_check(&mut doc, one, one + 1, "1000")?;
    let thousand = doc.source.find("1000").ok_or("1000")?;
    apply_and_check(&mut doc, thousand, thousand + 4, "1")?;
    assert_eq!(doc.source, source);
    assert_eq!(*doc.root, original_root);

    let reopened = IncrementalDocument::new(doc.source.clone())?;
    assert_eq!(*doc.root, *reopened.root);
    Ok(())
}

#[test]
fn unless_payload_survives_without_hand_traversal_helpers() -> TestResult {
    let source = "unless (1) { 2 } elsif (3) { 4 } else { 5 }";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let one = source.find('1').ok_or("1")?;
    apply_and_check(&mut doc, one, one + 1, "10")?;
    Ok(())
}

#[test]
fn empty_edit_set_does_not_advance_version() -> TestResult {
    let source = "my $x = 42;";
    let mut doc = IncrementalDocument::new(source.to_string())?;
    let root = (*doc.root).clone();
    doc.apply_edits(&IncrementalEditSet::new())?;
    assert_eq!(doc.version, 0);
    assert_eq!(doc.source, source);
    assert_eq!(*doc.root, root);
    assert_eq!(doc.last_strategy, ParseSnapshotStrategy::Fresh);
    Ok(())
}
