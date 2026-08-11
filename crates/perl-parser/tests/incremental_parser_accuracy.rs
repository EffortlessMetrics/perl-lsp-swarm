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
    let start = source
        .find(&edit.old_text)
        .ok_or("incremental edit old_text is absent from fixture source")?;
    let old_end = start + edit.old_text.len();
    let new_source = source.replacen(&edit.old_text, &edit.new_text, 1);
    let new_end = start + edit.new_text.len();

    let mut incremental = IncrementalParserV2::new();
    incremental.parse(&source)?;
    incremental.edit(Edit::new(
        start,
        old_end,
        new_end,
        Position::new(start, 3, 1),
        Position::new(old_end, 3, 15),
        Position::new(new_end, 3, 15),
    ));
    let incremental_ast = incremental.parse(&new_source)?;

    let fresh_ast = Parser::new(&new_source).parse()?;

    assert_eq!(
        incremental_ast.to_sexp(),
        fresh_ast.to_sexp(),
        "incremental result diverged from fresh parse for expectation {}",
        expectation.id
    );
    Ok(())
}
