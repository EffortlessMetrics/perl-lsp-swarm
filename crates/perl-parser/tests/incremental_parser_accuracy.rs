#![cfg(feature = "incremental")]
//! Manifest-backed incremental parser equivalence checks.

use perl_parser::Parser;
use perl_parser::edit::Edit;
use perl_parser::incremental_v2::IncrementalParserV2;
use perl_parser::position::Position;
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

type TestResult = Result<(), Box<dyn std::error::Error>>;

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
    let statement_start = source
        .find(&edit.old_text)
        .ok_or("incremental edit old_text is absent from fixture source")?;
    if edit.old_text.contains(['\n', '\r']) || edit.new_text.contains(['\n', '\r']) {
        return Err("the first incremental slice expects a single-line edit".into());
    }
    let old_chars: Vec<char> = edit.old_text.chars().collect();
    let new_chars: Vec<char> = edit.new_text.chars().collect();
    let common_prefix =
        old_chars.iter().zip(&new_chars).take_while(|(old, new)| old == new).count();
    let common_suffix = old_chars[common_prefix..]
        .iter()
        .rev()
        .zip(new_chars[common_prefix..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    if common_prefix + common_suffix >= old_chars.len()
        || common_prefix + common_suffix >= new_chars.len()
    {
        return Err("incremental edit has no changed character range".into());
    }

    let old_prefix_bytes: usize =
        old_chars[..common_prefix].iter().map(|character| character.len_utf8()).sum();
    let old_changed_bytes: usize = old_chars[common_prefix..old_chars.len() - common_suffix]
        .iter()
        .map(|character| character.len_utf8())
        .sum();
    let new_changed_bytes: usize = new_chars[common_prefix..new_chars.len() - common_suffix]
        .iter()
        .map(|character| character.len_utf8())
        .sum();
    let start = statement_start + old_prefix_bytes;
    let old_end = start + old_changed_bytes;
    let new_source = source.replacen(&edit.old_text, &edit.new_text, 1);
    if new_source == source {
        return Err("incremental edit did not change the fixture source".into());
    }
    let new_end = start + new_changed_bytes;

    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line = (source[..start].bytes().filter(|byte| *byte == b'\n').count() + 1) as u32;
    let character = (start - line_start) as u32;

    let mut incremental = IncrementalParserV2::new();
    incremental.parse(&source)?;
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
        incremental.reused_nodes > 0,
        "literal edit should exercise incremental reuse rather than a full fallback"
    );
    assert!(
        incremental.get_last_reuse_analysis().is_some(),
        "literal edit must take the advanced incremental-reuse path"
    );

    let fresh_ast = Parser::new(&new_source).parse()?;

    assert_eq!(
        incremental_ast.to_sexp(),
        fresh_ast.to_sexp(),
        "incremental result diverged from fresh parse for expectation {}",
        expectation.id
    );
    Ok(())
}
