#![cfg(feature = "incremental")]
//! Manifest-backed incremental parser equivalence checks.

use perl_parser::edit::Edit;
use perl_parser::incremental_v2::IncrementalParserV2;
use perl_parser::position::Position;
use perl_parser::Parser;
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
    let edit = expectation
        .edits
        .first()
        .ok_or("incremental_small_edit expectation has no edit")?;

    let source = fs::read_to_string(root.join(&fixture.source_path))?;
    let statement_start = source
        .find(&edit.old_text)
        .ok_or("incremental edit old_text is absent from fixture source")?;
    let literal_offset = edit
        .old_text
        .find('1')
        .ok_or("incremental edit old_text has no numeric literal")?;
    let start = statement_start + literal_offset;
    let old_end = start + 1;
    let new_source = source.replacen(&edit.old_text, &edit.new_text, 1);
    let new_end = start + 1;

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
        Position::new(old_end, line, character + 1),
        Position::new(new_end, line, character + 1),
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
