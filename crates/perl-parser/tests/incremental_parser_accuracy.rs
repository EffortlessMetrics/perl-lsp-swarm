#![cfg(feature = "incremental")]
//! Manifest-backed and ambiguity-boundary incremental parser equivalence checks.

use perl_parser::edit::Edit;
use perl_parser::incremental_v2::IncrementalParserV2;
use perl_parser::position::Position;
use perl_parser::{
    Parser,
    ast::{Node, NodeKind},
};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ParserAccuracyManifest {
    fixtures: Vec<ParserAccuracyFixture>,
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyFixture {
    id: String,
    source_path: PathBuf,
    #[serde(default)]
    incremental_expectations: Vec<IncrementalExpectation>,
}

#[derive(Debug, Deserialize)]
struct IncrementalExpectation {
    id: String,
    edits: Vec<IncrementalEditExpectation>,
}

#[derive(Debug, Deserialize)]
struct IncrementalEditExpectation {
    old_text: String,
    new_text: String,
}

type TestError = Box<dyn std::error::Error>;
type TestResult = Result<(), TestError>;

fn assert_incremental_edit_matches_fresh(
    source: &str,
    old_text: &str,
    new_text: &str,
    expectation_id: &str,
    require_reuse: bool,
) -> Result<Node, TestError> {
    if old_text.contains(['\n', '\r']) || new_text.contains(['\n', '\r']) {
        return Err("incremental equivalence slices currently expect a single-line edit".into());
    }
    if source.matches(old_text).count() != 1 {
        return Err(format!("{expectation_id}: old_text must occur exactly once").into());
    }

    let statement_start = source
        .find(old_text)
        .ok_or_else(|| format!("{expectation_id}: edit old_text is absent from source"))?;
    let old_chars: Vec<char> = old_text.chars().collect();
    let new_chars: Vec<char> = new_text.chars().collect();
    let common_prefix = old_chars
        .iter()
        .zip(&new_chars)
        .take_while(|(old, new)| old == new)
        .count();
    let common_suffix = old_chars[common_prefix..]
        .iter()
        .rev()
        .zip(new_chars[common_prefix..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    let old_changed_chars = old_chars.len().saturating_sub(common_prefix + common_suffix);
    let new_changed_chars = new_chars.len().saturating_sub(common_prefix + common_suffix);
    if old_changed_chars == 0 && new_changed_chars == 0 {
        return Err(format!("{expectation_id}: edit has no changed character range").into());
    }

    let old_prefix_bytes: usize = old_chars[..common_prefix]
        .iter()
        .map(|character| character.len_utf8())
        .sum();
    let old_changed_end = common_prefix + old_changed_chars;
    let new_changed_end = common_prefix + new_changed_chars;
    let old_changed_bytes: usize = old_chars[common_prefix..old_changed_end]
        .iter()
        .map(|character| character.len_utf8())
        .sum();
    let new_changed_bytes: usize = new_chars[common_prefix..new_changed_end]
        .iter()
        .map(|character| character.len_utf8())
        .sum();
    let start = statement_start + old_prefix_bytes;
    let old_end = start + old_changed_bytes;
    let new_source = source.replacen(old_text, new_text, 1);
    if new_source == source {
        return Err(format!("{expectation_id}: edit did not change the source").into());
    }
    let new_end = start + new_changed_bytes;

    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line = (source[..start].bytes().filter(|byte| *byte == b'\n').count() + 1) as u32;
    let character = (start - line_start) as u32;

    let mut incremental = IncrementalParserV2::new();
    incremental.parse(source)?;
    incremental.edit(Edit::new(
        start,
        old_end,
        new_end,
        Position::new(start, line, character),
        Position::new(old_end, line, character + old_changed_bytes as u32),
        Position::new(new_end, line, character + new_changed_bytes as u32),
    ));
    let incremental_ast = incremental.parse(&new_source)?;
    assert!(
        !contains_error(&incremental_ast),
        "{expectation_id}: incremental parse produced a recovery Error node"
    );

    if require_reuse {
        assert!(
            incremental.reused_nodes > 0,
            "{expectation_id}: edit should exercise incremental reuse rather than a full fallback"
        );
        assert!(
            incremental.get_last_reuse_analysis().is_some(),
            "{expectation_id}: edit must take the advanced incremental-reuse path"
        );
    }

    let mut fresh_parser = Parser::new(&new_source);
    let fresh_ast = fresh_parser.parse()?;
    assert!(
        fresh_parser.errors().is_empty(),
        "{expectation_id}: fresh parse produced diagnostics: {:?}",
        fresh_parser.errors()
    );
    assert!(
        !contains_error(&fresh_ast),
        "{expectation_id}: fresh parse produced a recovery Error node"
    );
    assert_eq!(
        incremental_ast.to_sexp(),
        fresh_ast.to_sexp(),
        "incremental result diverged from fresh parse for expectation {expectation_id}"
    );
    Ok(fresh_ast)
}

fn contains_match(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Match { .. })
        || node.children().into_iter().any(contains_match)
}

fn contains_division(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Binary { op, .. } if op == "/")
        || node.children().into_iter().any(contains_division)
}

fn contains_error(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Error { .. })
        || node.children().into_iter().any(contains_error)
}

fn contains_variable_declaration(node: &Node, expected_name: &str) -> bool {
    matches!(
        &node.kind,
        NodeKind::VariableDeclaration { variable, .. }
            if matches!(
                &variable.kind,
                NodeKind::Variable { sigil, name } if sigil == "$" && name == expected_name
            )
    ) || node
        .children()
        .into_iter()
        .any(|child| contains_variable_declaration(child, expected_name))
}

#[test]
fn manifest_incremental_edits_match_a_fresh_parse() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let manifest_path = root
        .join("crates")
        .join("perl-corpus")
        .join("fixtures")
        .join("parser_accuracy")
        .join("manifest.json");
    let manifest: ParserAccuracyManifest =
        serde_json::from_str(&fs::read_to_string(manifest_path)?)?;

    let fixture = manifest
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "incremental_small_edit")
        .ok_or("incremental_small_edit fixture is missing")?;
    let expectation = fixture
        .incremental_expectations
        .first()
        .ok_or("incremental_small_edit has no incremental expectation")?;
    let edit = expectation.edits.first().ok_or("incremental_small_edit expectation has no edit")?;

    let source = fs::read_to_string(root.join(&fixture.source_path))?;
    let _fresh_ast = assert_incremental_edit_matches_fresh(
        &source,
        &edit.old_text,
        &edit.new_text,
        &expectation.id,
        true,
    )?;
    Ok(())
}

#[test]
fn pure_insertion_edit_matches_fresh_parse() -> TestResult {
    let source = concat!(
        "my $before = 1;\n",
        "my $value = 20;\n",
        "my $after = 3;\n",
    );
    let fresh_ast = assert_incremental_edit_matches_fresh(
        source,
        "$value = 20",
        "$value = 200",
        "pure_digit_insertion",
        false,
    )?;

    assert!(contains_variable_declaration(&fresh_ast, "before"));
    assert!(contains_variable_declaration(&fresh_ast, "value"));
    assert!(contains_variable_declaration(&fresh_ast, "after"));
    Ok(())
}

#[test]
fn pure_deletion_edit_matches_fresh_parse() -> TestResult {
    let source = concat!(
        "my $before = 1;\n",
        "my $value = 200;\n",
        "my $after = 3;\n",
    );
    let fresh_ast = assert_incremental_edit_matches_fresh(
        source,
        "$value = 200",
        "$value = 20",
        "pure_digit_deletion",
        false,
    )?;

    assert!(contains_variable_declaration(&fresh_ast, "before"));
    assert!(contains_variable_declaration(&fresh_ast, "value"));
    assert!(contains_variable_declaration(&fresh_ast, "after"));
    Ok(())
}

#[test]
fn slash_reclassification_edit_matches_fresh_parse() -> TestResult {
    let source = concat!(
        "my $before = 1;\n",
        "my $value = $left / 2;\n",
        "my $after = 3;\n",
    );
    let before_ast = Parser::new(source).parse()?;
    assert!(contains_division(&before_ast), "the pre-edit source must contain division");
    assert!(!contains_match(&before_ast), "the pre-edit source must not contain a regex Match");

    let fresh_ast = assert_incremental_edit_matches_fresh(
        source,
        "$left / 2",
        "$left =~ /two/",
        "division_to_regex_match",
        false,
    )?;

    assert!(
        contains_match(&fresh_ast),
        "the edited source must be reclassified as a regex Match expression"
    );
    assert!(
        !contains_division(&fresh_ast),
        "the edited source must not retain the obsolete division node"
    );
    assert!(contains_variable_declaration(&fresh_ast, "before"));
    assert!(contains_variable_declaration(&fresh_ast, "after"));
    Ok(())
}
