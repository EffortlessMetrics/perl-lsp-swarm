//! Editor Intelligence Scorecard — Gold Corpus Harness
//!
//! Replays hover, goto-definition, and completion requests against real
//! gold-corpus fixtures and reports pass rates.  This is the headless
//! scorecard described in issue #4066.
//!
//! ## How it works
//!
//! 1. Discover all `test_corpus/gold/<fixture-name>/` directories.
//! 2. For each fixture directory that has `expected_hover.json`,
//!    `expected_goto.json`, or `expected_completion.json`, load the
//!    assertions and replay LSP requests.
//! 3. Print per-kind pass rates to stdout (`--nocapture`).
//! 4. Fail the test if *any* assertion fails (CI gate).
//!
//! ## Verify
//!
//! ```bash
//! RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test editor_intelligence_scorecard -- --nocapture
//! ```

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout/print_stderr don't
// apply the way they do to production code.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod common;

use common::test_utils::TestServerBuilder;
use perl_corpus::gold::{
    CompletionAssertionKind, CompletionGoldFixture, GoldAssertion, GoldFixture, GotoAssertionKind,
    GotoGoldFixture, HoverAssertionRigor, HoverGoldFixture, RenameAssertion, RenameAssertionKind,
    RenameExpectedEdit, RenameGoldFixture, load_completion_gold_fixtures,
    load_document_symbol_gold_fixtures, load_gold_fixtures, load_goto_gold_fixtures,
    load_hover_gold_fixtures, load_rename_gold_fixtures, match_hover_assertion,
    observe_hover_response,
};
use perl_corpus::{DocumentSymbolAssertionKind, DocumentSymbolGoldFixture};
use serde_json::Value;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn gold_corpus_root() -> PathBuf {
    // Walk up from the test binary location to find the workspace root,
    // then resolve test_corpus/gold/.
    // CARGO_MANIFEST_DIR is set to the crate directory during tests.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let crate_dir = PathBuf::from(manifest);
    // crates/perl-lsp-rs → workspace root is two levels up
    let workspace_root = crate_dir
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| crate_dir.clone());
    workspace_root.join("test_corpus").join("gold")
}

fn completion_labels_from_response(resp: &Value) -> Vec<String> {
    let items = match resp["result"]["items"].as_array() {
        Some(arr) => arr,
        None => match resp["result"].as_array() {
            Some(arr) => arr,
            None => return Vec::new(),
        },
    };
    items.iter().filter_map(|item| item["label"].as_str().map(|s| s.to_string())).collect()
}

fn first_goto_line(resp: &Value) -> Option<u32> {
    let arr = resp.get("result")?.as_array()?;
    let first = arr.first()?;
    let line = first["range"]["start"]["line"].as_u64()? as u32;
    Some(line)
}

fn diagnostic_codes_from_response(resp: &Value) -> Vec<String> {
    resp["result"]["items"].as_array().map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|item| {
                let code = item.get("code")?;
                code.as_str()
                    .map(ToString::to_string)
                    .or_else(|| code.as_i64().map(|n| n.to_string()))
            })
            .collect()
    })
}

fn document_symbol_names(resp: &Value) -> Vec<String> {
    fn collect_document_symbols(symbols: &Value, out: &mut Vec<String>) {
        if let Some(array) = symbols.as_array() {
            for symbol in array {
                if let Some(name) = symbol.get("name").and_then(Value::as_str) {
                    out.push(name.to_string());
                }

                if let Some(children) = symbol.get("children") {
                    collect_document_symbols(children, out);
                }
            }
        }
    }

    let mut names = Vec::new();
    if let Some(result) = resp.get("result") {
        collect_document_symbols(result, &mut names);
    }
    names
}

// ---------------------------------------------------------------------------
// Hover correctness test
// ---------------------------------------------------------------------------

/// Run all hover gold fixtures and assert every assertion passes.
/// Reports a pass-rate summary to stdout under --nocapture.
#[test]
fn test_hover_gold_corpus() -> TestResult {
    let root = gold_corpus_root();
    let fixtures: Vec<HoverGoldFixture> = match load_hover_gold_fixtures(&root) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("SKIP: no hover gold fixtures found in {}", root.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let server = TestServerBuilder::new().build();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut compatibility_total = 0usize;
    let mut compatibility_passed = 0usize;
    let mut exact_total = 0usize;
    let mut exact_passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let code = std::fs::read_to_string(&fixture.fixture_path)?;

        let uri = format!("file:///gold/{}.pl", fixture.name);
        server.open_document(&uri, &code);

        for assertion in &fixture.hover_assertions {
            total += 1;
            let resp = server.get_hover(&uri, assertion.line, assertion.character);
            let observation = observe_hover_response(&resp);
            let match_result = match_hover_assertion(&observation, assertion);
            let ok = match_result.is_ok();
            match assertion.kind.rigor() {
                HoverAssertionRigor::Compatibility => {
                    compatibility_total += 1;
                    if ok {
                        compatibility_passed += 1;
                    }
                }
                HoverAssertionRigor::Exact => {
                    exact_total += 1;
                    if ok {
                        exact_passed += 1;
                    }
                }
            }

            if let Err(failure) = match_result {
                failures.push(format!(
                    "  FAIL [{}] {:?} at line:{} char:{} — {}",
                    fixture.name,
                    assertion.kind,
                    assertion.line,
                    assertion.character,
                    failure.reason
                ));
            } else {
                passed += 1;
            }
        }
    }

    println!(
        "\nHover gold corpus: {}/{} assertions passed ({:.0}%)",
        passed,
        total,
        if total > 0 { passed as f64 / total as f64 * 100.0 } else { 100.0 }
    );
    println!(
        "Hover gold corpus compatibility: {}/{} ({:.0}%)",
        compatibility_passed,
        compatibility_total,
        if compatibility_total > 0 {
            compatibility_passed as f64 / compatibility_total as f64 * 100.0
        } else {
            100.0
        }
    );
    println!(
        "Hover gold corpus exact: {}/{} ({:.0}%)",
        exact_passed,
        exact_total,
        if exact_total > 0 { exact_passed as f64 / exact_total as f64 * 100.0 } else { 100.0 }
    );
    for f in &failures {
        println!("{f}");
    }

    assert!(
        failures.is_empty(),
        "Hover gold corpus: {} assertion(s) failed out of {}:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Goto-definition correctness test
// ---------------------------------------------------------------------------

/// Run all goto-definition gold fixtures and assert every assertion passes.
#[test]
fn test_goto_gold_corpus() -> TestResult {
    let root = gold_corpus_root();
    let fixtures: Vec<GotoGoldFixture> = match load_goto_gold_fixtures(&root) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("SKIP: no goto gold fixtures found in {}", root.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let server = TestServerBuilder::new().build();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let code = std::fs::read_to_string(&fixture.fixture_path)?;

        let uri = format!("file:///gold/{}.pl", fixture.name);
        server.open_document(&uri, &code);

        for assertion in &fixture.goto_assertions {
            total += 1;
            let resp = server.get_definition(&uri, assertion.line, assertion.character);
            let def_line = first_goto_line(&resp);

            let ok = match &assertion.kind {
                GotoAssertionKind::GotoNonNull => def_line.is_some(),
                GotoAssertionKind::GotoNull => def_line.is_none(),
                GotoAssertionKind::GotoLine { expected_line } => def_line == Some(*expected_line),
            };

            if ok {
                passed += 1;
            } else {
                failures.push(format!(
                    "  FAIL [{}] {:?} at line:{} char:{} — got def_line: {:?}",
                    fixture.name, assertion.kind, assertion.line, assertion.character, def_line
                ));
            }
        }
    }

    println!(
        "\nGoto gold corpus: {}/{} assertions passed ({:.0}%)",
        passed,
        total,
        if total > 0 { passed as f64 / total as f64 * 100.0 } else { 100.0 }
    );
    for f in &failures {
        println!("{f}");
    }

    assert!(
        failures.is_empty(),
        "Goto gold corpus: {} assertion(s) failed out of {}:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Completion relevance test
// ---------------------------------------------------------------------------

/// Run all completion gold fixtures and assert every assertion passes.
/// Reports top-1 accuracy, top-5 accuracy, non-empty rate, and noise-free rate.
#[test]
fn test_completion_gold_corpus() -> TestResult {
    let root = gold_corpus_root();
    let fixtures: Vec<CompletionGoldFixture> = match load_completion_gold_fixtures(&root) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("SKIP: no completion gold fixtures found in {}", root.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let server = TestServerBuilder::new().build();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let code = std::fs::read_to_string(&fixture.fixture_path)?;

        let uri = format!("file:///gold/{}.pl", fixture.name);
        server.open_document(&uri, &code);

        for assertion in &fixture.completion_assertions {
            total += 1;
            let resp = server.get_completion(&uri, assertion.line, assertion.character);
            let labels = completion_labels_from_response(&resp);

            let ok = match &assertion.kind {
                CompletionAssertionKind::CompletionNonEmpty => !labels.is_empty(),
                CompletionAssertionKind::CompletionTop1 { expected_label } => {
                    labels.first().map(|l| l.as_str()) == Some(expected_label.as_str())
                }
                CompletionAssertionKind::CompletionTop5 { expected_label } => {
                    labels.iter().take(5).any(|l| l == expected_label)
                }
                CompletionAssertionKind::CompletionPresent { expected_label } => {
                    labels.iter().any(|l| l == expected_label)
                }
                CompletionAssertionKind::CompletionNoiseAbsent { forbidden_label } => {
                    !labels.iter().any(|l| l == forbidden_label)
                }
            };

            if ok {
                passed += 1;
            } else {
                failures.push(format!(
                    "  FAIL [{}] {:?} at line:{} char:{} — labels: {:?}",
                    fixture.name,
                    assertion.kind,
                    assertion.line,
                    assertion.character,
                    &labels[..labels.len().min(10)]
                ));
            }
        }
    }

    println!(
        "\nCompletion gold corpus: {}/{} assertions passed ({:.0}%)",
        passed,
        total,
        if total > 0 { passed as f64 / total as f64 * 100.0 } else { 100.0 }
    );
    for f in &failures {
        println!("{f}");
    }

    assert!(
        failures.is_empty(),
        "Completion gold corpus: {} assertion(s) failed out of {}:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Diagnostics correctness test
// ---------------------------------------------------------------------------

/// Run all diagnostics gold fixtures (`expected.json`) and assert every assertion passes.
#[test]
fn test_diagnostics_gold_corpus() -> TestResult {
    let root = gold_corpus_root();
    let fixtures: Vec<GoldFixture> = match load_gold_fixtures(&root) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("SKIP: no diagnostics gold fixtures found in {}", root.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let server = TestServerBuilder::new().build();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let code = std::fs::read_to_string(&fixture.fixture_path)?;

        let uri = format!("file:///gold/{}.pl", fixture.name);
        server.open_document(&uri, &code);

        let diagnostics = server.get_diagnostics(&uri);
        let diagnostic_codes = diagnostic_codes_from_response(&diagnostics);

        for assertion in &fixture.expected.diagnostics {
            total += 1;
            let ok = match assertion {
                GoldAssertion::NoDiagnostics => diagnostic_codes.is_empty(),
                GoldAssertion::NoDiagnostic { code } => !diagnostic_codes.iter().any(|c| c == code),
                GoldAssertion::DiagnosticPresent { code, message_contains, .. } => {
                    let has_code = diagnostic_codes.iter().any(|c| c == code);
                    if !has_code {
                        false
                    } else if let Some(needle) = message_contains {
                        diagnostics["result"]["items"].as_array().is_some_and(|items| {
                            items.iter().any(|item| {
                                // Use the same normalization as diagnostic_codes_from_response:
                                // accept both string and integer coded diagnostics.
                                let item_code = item.get("code").and_then(|v| {
                                    v.as_str()
                                        .map(ToString::to_string)
                                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                                });
                                let item_code_matches =
                                    item_code.as_deref().is_some_and(|c| c == code);
                                let message_matches = item["message"]
                                    .as_str()
                                    .is_some_and(|message| message.contains(needle));
                                item_code_matches && message_matches
                            })
                        })
                    } else {
                        true
                    }
                }
                GoldAssertion::DiagnosticCount { code, count } => {
                    diagnostic_codes.iter().filter(|c| *c == code).count() == *count
                }
            };

            if ok {
                passed += 1;
            } else {
                failures.push(format!(
                    "  FAIL [{}] {:?} — diagnostic codes: {:?}",
                    fixture.name, assertion, diagnostic_codes
                ));
            }
        }
    }

    println!(
        "\nDiagnostics gold corpus: {}/{} assertions passed ({:.0}%)",
        passed,
        total,
        if total > 0 { passed as f64 / total as f64 * 100.0 } else { 100.0 }
    );
    for f in &failures {
        println!("{f}");
    }

    assert!(
        failures.is_empty(),
        "Diagnostics gold corpus: {} assertion(s) failed out of {}:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Document symbols correctness test
// ---------------------------------------------------------------------------

/// Run all document-symbol gold fixtures and assert every assertion passes.
#[test]
fn test_document_symbols_gold_corpus() -> TestResult {
    let root = gold_corpus_root();
    let fixtures: Vec<DocumentSymbolGoldFixture> = match load_document_symbol_gold_fixtures(&root) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("SKIP: no document-symbol gold fixtures found in {}", root.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let server = TestServerBuilder::new().build();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let code = std::fs::read_to_string(&fixture.fixture_path)?;

        let uri = format!("file:///gold/{}.pl", fixture.name);
        server.open_document(&uri, &code);

        let resp = server.get_symbols(&uri);
        let names = document_symbol_names(&resp);

        for assertion in &fixture.symbol_assertions {
            total += 1;
            let ok = match &assertion.kind {
                DocumentSymbolAssertionKind::SymbolNonEmpty => !names.is_empty(),
                DocumentSymbolAssertionKind::SymbolPresent { name } => {
                    names.iter().any(|candidate| candidate == name)
                }
                DocumentSymbolAssertionKind::SymbolAbsent { name } => {
                    names.iter().all(|candidate| candidate != name)
                }
                DocumentSymbolAssertionKind::SymbolCount { count } => names.len() == *count,
            };

            if ok {
                passed += 1;
            } else {
                failures.push(format!(
                    "  FAIL [{}] {:?} — symbols: {:?}",
                    fixture.name, assertion.kind, names
                ));
            }
        }
    }

    eprintln!("\nDocument symbols gold corpus: {passed}/{total} passed");

    assert!(
        failures.is_empty(),
        "Document symbols gold corpus: {} assertion(s) failed out of {}:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Rename correctness test
// ---------------------------------------------------------------------------

fn rename_total_edit_count(resp: &Value) -> usize {
    observed_rename_edits(resp).map_or(0, |edits| edits.len())
}

fn rename_has_structured_error(resp: &Value) -> bool {
    let Some(error) = resp.get("error").and_then(Value::as_object) else {
        return false;
    };
    error.get("code").and_then(Value::as_i64).is_some()
        && error.get("message").and_then(Value::as_str).is_some()
}

fn rename_protocol_rejection(resp: &Value) -> bool {
    // RenameNull represents a protocol-level refusal for an otherwise valid
    // request.  Invalid parameters is the only standard JSON-RPC error this
    // oracle treats as that outcome; server lifecycle, cancellation, and
    // implementation errors must remain visible as test failures.
    resp.get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64)
        == Some(-32602)
}

/// Return whether a replacement name is sufficiently well formed for the
/// rename oracle to interpret a `-32602` response as a semantic refusal.
///
/// The scorecard deliberately does not implement Perl name resolution.  It
/// does, however, reject empty, whitespace-containing, and punctuation-only
/// requests so that an invalid request cannot masquerade as a symbol that the
/// server refused to rename.
fn rename_replacement_name_is_well_formed(name: &str) -> bool {
    let mut chars = name.chars();
    let first = match chars.next() {
        Some('$' | '@' | '%') => chars.next(),
        Some(character) => Some(character),
        None => None,
    };

    first.is_some_and(|character| character == '_' || unicode_ident::is_xid_start(character))
        && chars.all(|character| character == '_' || unicode_ident::is_xid_continue(character))
}

/// Verify that an LSP position addresses an actual non-whitespace source
/// character.  This is deliberately a small request-shape check, not a Perl
/// parser: it prevents an InvalidParams response for an out-of-range request
/// from masquerading as a semantic RenameNull result.
fn rename_position_is_well_formed(source: &str, line: u32, character: u32) -> bool {
    let Some(source_line) = source.lines().nth(line as usize) else {
        return false;
    };

    let mut offset = 0u32;
    for source_character in source_line.chars() {
        let width = source_character.len_utf16() as u32;
        // An LSP UTF-16 position may not point into the middle of a surrogate
        // pair. Accept only the scalar's starting offset.
        if character == offset {
            return !source_character.is_whitespace();
        }
        offset = offset.saturating_add(width);
    }
    false
}

fn rename_is_null(resp: &Value, source: &str, line: u32, character: u32) -> bool {
    match (resp.get("result"), resp.get("error")) {
        (Some(Value::Null), None) => true,
        (None, Some(_)) => {
            rename_position_is_well_formed(source, line, character)
                && rename_has_structured_error(resp)
                && rename_protocol_rejection(resp)
        }
        _ => false,
    }
}

/// Extract the JSON-RPC error message from a rename response, if any, for
/// failure diagnostics. Without this, a failed assertion only reports "0
/// edits" — indistinguishable from a request that legitimately found
/// nothing to rename — hiding the actual server error.
fn rename_error_message(resp: &Value) -> Option<&str> {
    resp.get("error").and_then(|error| error["message"].as_str())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ObservedRenameEdit {
    uri: String,
    line: u32,
    character: u32,
    end_line: u32,
    end_character: u32,
    new_text: String,
}

fn json_u32(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|number| u32::try_from(number).ok())
}

fn observed_rename_edits(resp: &Value) -> Option<Vec<ObservedRenameEdit>> {
    if resp.get("error").is_some() {
        return None;
    }
    let mut edits = Vec::new();
    let result = resp.get("result")?;

    if let Some(changes) = result.get("changes") {
        for (uri, entries) in changes.as_object()? {
            append_text_edits(&mut edits, uri, entries.as_array()?)?;
        }
    }

    if let Some(document_changes) = result.get("documentChanges") {
        for document_change in document_changes.as_array()? {
            let document_change = document_change.as_object()?;
            let uri = document_change.get("textDocument")?.get("uri")?.as_str()?;
            append_text_edits(&mut edits, uri, document_change.get("edits")?.as_array()?)?;
        }
    }
    edits.sort();
    Some(edits)
}

fn append_text_edits(
    edits: &mut Vec<ObservedRenameEdit>,
    uri: &str,
    entries: &[Value],
) -> Option<()> {
    for entry in entries {
        let range = entry.get("range")?.as_object()?;
        let start = range.get("start")?.as_object()?;
        let end = range.get("end")?.as_object()?;
        let new_text = entry.get("newText")?.as_str()?;
        let line = start.get("line").and_then(json_u32)?;
        let character = start.get("character").and_then(json_u32)?;
        let end_line = end.get("line").and_then(json_u32)?;
        let end_character = end.get("character").and_then(json_u32)?;
        edits.push(ObservedRenameEdit {
            uri: uri.to_owned(),
            line,
            character,
            end_line,
            end_character,
            new_text: new_text.to_owned(),
        });
    }
    Some(())
}

fn rename_expected_edits_match(
    resp: &Value,
    expected_uri: &str,
    expected: Option<&[RenameExpectedEdit]>,
) -> bool {
    let Some(expected) = expected else {
        return observed_rename_edits(resp).is_some();
    };

    let mut expected_edits: Vec<ObservedRenameEdit> = expected
        .iter()
        .map(|edit| ObservedRenameEdit {
            uri: edit.uri.clone().unwrap_or_else(|| expected_uri.to_owned()),
            line: edit.line,
            character: edit.character,
            end_line: edit.end_line,
            end_character: edit.end_character,
            new_text: edit.new_text.clone(),
        })
        .collect();
    expected_edits.sort();
    observed_rename_edits(resp).is_some_and(|observed| observed == expected_edits)
}

fn rename_edit_count_at_least_passes(
    resp: &Value,
    expected_uri: &str,
    min: usize,
    expected: Option<&[RenameExpectedEdit]>,
    source: &str,
    line: u32,
    character: u32,
) -> bool {
    // The schema requires min >= 1. Keep the helper defensive so a direct
    // caller cannot turn a successful-rename assertion into a shape-only
    // zero-edit check.
    min > 0
        && !rename_is_null(resp, source, line, character)
        && observed_rename_edits(resp).is_some()
        && rename_total_edit_count(resp) >= min
        && rename_expected_edits_match(resp, expected_uri, expected)
}

fn rename_assertion_passes(
    assertion: &RenameAssertion,
    resp: &Value,
    uri: &str,
    source: &str,
) -> bool {
    let expected_edits_ok =
        rename_expected_edits_match(resp, uri, assertion.expected_edits.as_deref());
    let response_edits_are_well_formed =
        rename_is_null(resp, source, assertion.line, assertion.character)
            || observed_rename_edits(resp).is_some();

    match &assertion.kind {
        RenameAssertionKind::RenameSucceeds => {
            !rename_is_null(resp, source, assertion.line, assertion.character)
                && rename_total_edit_count(resp) >= 1
                && response_edits_are_well_formed
                && expected_edits_ok
        }
        RenameAssertionKind::RenameNull => {
            rename_replacement_name_is_well_formed(&assertion.new_name)
                && rename_is_null(resp, source, assertion.line, assertion.character)
                && assertion.expected_edits.is_none()
        }
        RenameAssertionKind::RenameEditCountAtLeast { min } => rename_edit_count_at_least_passes(
            resp,
            uri,
            *min,
            assertion.expected_edits.as_deref(),
            source,
            assertion.line,
            assertion.character,
        ),
    }
}

/// Run all rename gold fixtures and assert every assertion passes.
/// Reports rename success rate to stdout under --nocapture.
#[test]
fn test_rename_gold_corpus() -> TestResult {
    let root = gold_corpus_root();
    let fixtures: Vec<RenameGoldFixture> = match load_rename_gold_fixtures(&root) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            eprintln!("SKIP: no rename gold fixtures found in {}", root.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let server = TestServerBuilder::new().build();

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for fixture in &fixtures {
        let code = std::fs::read_to_string(&fixture.fixture_path)?;

        let uri = format!("file:///gold/{}.pl", fixture.name);
        server.open_document(&uri, &code);

        for assertion in &fixture.rename_assertions {
            total += 1;
            let resp =
                server.get_rename(&uri, assertion.line, assertion.character, &assertion.new_name);

            let ok = rename_assertion_passes(assertion, &resp, &uri, &code);

            if ok {
                passed += 1;
            } else {
                failures.push(format!(
                    "  FAIL [{}] {:?} at line:{} char:{} new_name:{:?} — edits: {}, error: {}",
                    fixture.name,
                    assertion.kind,
                    assertion.line,
                    assertion.character,
                    assertion.new_name,
                    rename_total_edit_count(&resp),
                    rename_error_message(&resp).unwrap_or("<none>"),
                ));
            }
        }
    }

    println!(
        "\nRename gold corpus: {}/{} assertions passed ({:.0}%)",
        passed,
        total,
        if total > 0 { passed as f64 / total as f64 * 100.0 } else { 100.0 }
    );
    for f in &failures {
        println!("{f}");
    }

    assert!(
        failures.is_empty(),
        "Rename gold corpus: {} assertion(s) failed out of {}:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );

    Ok(())
}

#[cfg(test)]
mod rename_oracle_tests {
    use super::*;
    use serde_json::json;

    const VALID_RENAME_SOURCE: &str = "sub unused { return 1; }\n";

    fn expected() -> Vec<RenameExpectedEdit> {
        vec![RenameExpectedEdit {
            uri: None,
            line: 4,
            character: 4,
            end_line: 4,
            end_character: 19,
            new_text: "sum_values".to_string(),
        }]
    }

    fn expected_basic() -> Vec<RenameExpectedEdit> {
        vec![
            RenameExpectedEdit {
                uri: None,
                line: 5,
                character: 7,
                end_line: 5,
                end_character: 13,
                new_text: "$total".to_string(),
            },
            RenameExpectedEdit {
                uri: None,
                line: 6,
                character: 4,
                end_line: 6,
                end_character: 10,
                new_text: "$total".to_string(),
            },
            RenameExpectedEdit {
                uri: None,
                line: 7,
                character: 18,
                end_line: 7,
                end_character: 24,
                new_text: "$total".to_string(),
            },
            RenameExpectedEdit {
                uri: None,
                line: 8,
                character: 11,
                end_line: 8,
                end_character: 17,
                new_text: "$total".to_string(),
            },
        ]
    }

    fn basic_response_with_entries(entries: Value) -> Value {
        json!({
            "result": {
                "changes": {
                    "file:///gold/rename_basic.pl": entries
                }
            }
        })
    }

    fn response(range: Value, new_text: &str) -> Value {
        json!({
            "result": {
                "changes": {
                    "file:///gold/rename_subroutine.pl": [{
                        "range": range,
                        "newText": new_text
                    }]
                }
            }
        })
    }

    fn response_with_entries(entries: Value) -> Value {
        json!({
            "result": {
                "changes": {
                    "file:///gold/rename_subroutine.pl": entries
                }
            }
        })
    }

    fn rename_null_assertion() -> RenameAssertion {
        RenameAssertion {
            kind: RenameAssertionKind::RenameNull,
            line: 0,
            character: 0,
            new_name: "unused".to_string(),
            expected_edits: None,
            rationale: String::new(),
        }
    }

    fn rename_success_assertion(kind: RenameAssertionKind) -> RenameAssertion {
        RenameAssertion {
            kind,
            line: 4,
            character: 4,
            new_name: "sum_values".to_string(),
            expected_edits: Some(expected()),
            rationale: String::new(),
        }
    }

    #[test]
    fn rename_oracle_rejects_wrong_range() -> TestResult {
        let resp = response(
            json!({"start":{"line":4,"character":5},"end":{"line":4,"character":20}}),
            "sum_values",
        );
        if rename_expected_edits_match(
            &resp,
            "file:///gold/rename_subroutine.pl",
            Some(expected().as_slice()),
        ) {
            return Err("wrong-range rename edit passed the oracle".into());
        }
        Ok(())
    }

    #[test]
    fn rename_oracle_rejects_wrong_replacement_text() -> TestResult {
        let resp = response(
            json!({"start":{"line":4,"character":4},"end":{"line":4,"character":19}}),
            "calculate_total",
        );
        if rename_expected_edits_match(
            &resp,
            "file:///gold/rename_subroutine.pl",
            Some(expected().as_slice()),
        ) {
            return Err("wrong-text rename edit passed the oracle".into());
        }
        Ok(())
    }

    #[test]
    fn rename_basic_oracle_rejects_wrong_occurrence() -> TestResult {
        let wrong = json!([
            {
                "range": {
                    "start": {"line": 5, "character": 7},
                    "end": {"line": 5, "character": 13}
                },
                "newText": "$total"
            },
            {
                "range": {
                    "start": {"line": 6, "character": 4},
                    "end": {"line": 6, "character": 10}
                },
                "newText": "$total"
            },
            {
                "range": {
                    "start": {"line": 7, "character": 17},
                    "end": {"line": 7, "character": 23}
                },
                "newText": "$total"
            },
            {
                "range": {
                    "start": {"line": 8, "character": 11},
                    "end": {"line": 8, "character": 17}
                },
                "newText": "$total"
            }
        ]);
        let resp = basic_response_with_entries(wrong);
        if rename_expected_edits_match(
            &resp,
            "file:///gold/rename_basic.pl",
            Some(expected_basic().as_slice()),
        ) {
            return Err("wrong rename_basic occurrence passed the exact oracle".into());
        }
        Ok(())
    }

    #[test]
    fn rename_oracle_rejects_malformed_extra_edit() -> TestResult {
        let valid = json!({
            "range": {
                "start": {"line": 4, "character": 4},
                "end": {"line": 4, "character": 19}
            },
            "newText": "sum_values"
        });
        let malformed = json!({
            "range": {
                "start": {"line": 5, "character": "not-a-number"},
                "end": {"line": 5, "character": 8}
            },
            "newText": "sum_values"
        });
        let resp = response_with_entries(json!([valid, malformed]));
        if rename_expected_edits_match(
            &resp,
            "file:///gold/rename_subroutine.pl",
            Some(expected().as_slice()),
        ) {
            return Err("malformed extra rename edit passed the oracle".into());
        }
        Ok(())
    }

    #[test]
    fn rename_oracle_rejects_malformed_edit_without_expected_edits() -> TestResult {
        let malformed = json!({
            "range": {
                "start": {"line": 5, "character": "not-a-number"},
                "end": {"line": 5, "character": 8}
            },
            "newText": "sum_values"
        });
        let resp = response_with_entries(json!([malformed]));
        if rename_expected_edits_match(&resp, "file:///gold/rename_subroutine.pl", None) {
            return Err("malformed rename edit passed an empty expected-edit oracle".into());
        }
        Ok(())
    }

    #[test]
    fn rename_oracle_preserves_count_only_and_exact_modes() -> TestResult {
        let valid = json!({
            "range": {
                "start": {"line": 4, "character": 4},
                "end": {"line": 4, "character": 19}
            },
            "newText": "sum_values"
        });
        let resp = response_with_entries(json!([valid]));

        if !rename_expected_edits_match(&resp, "file:///gold/rename_subroutine.pl", None) {
            return Err("well-formed rename edit failed count-only mode".into());
        }
        if !rename_expected_edits_match(
            &resp,
            "file:///gold/rename_subroutine.pl",
            Some(expected().as_slice()),
        ) {
            return Err("matching rename edit failed exact mode".into());
        }
        if rename_expected_edits_match(&resp, "file:///gold/rename_subroutine.pl", Some(&[])) {
            return Err("non-empty rename edit passed explicit empty exact mode".into());
        }
        Ok(())
    }

    #[test]
    fn rename_oracle_normalizes_document_changes_text_document_edits() -> TestResult {
        let response = json!({
            "result": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///gold/rename_subroutine.pl", "version": null},
                    "edits": expected().iter().map(|edit| json!({
                        "range": {
                            "start": {"line": edit.line, "character": edit.character},
                            "end": {"line": edit.end_line, "character": edit.end_character}
                        },
                        "newText": edit.new_text
                    })).collect::<Vec<_>>()
                }]
            }
        });

        if !rename_expected_edits_match(
            &response,
            "file:///gold/rename_subroutine.pl",
            Some(expected().as_slice()),
        ) {
            return Err("TextDocumentEdit workspace edit did not match expected edits".into());
        }
        if rename_total_edit_count(&response) != expected().len() {
            return Err("TextDocumentEdit workspace edit count was not normalized".into());
        }
        Ok(())
    }

    #[test]
    fn rename_oracle_matches_expected_edits_by_target_uri() -> TestResult {
        let other_uri = "file:///gold/rename_other.pl";
        let response = json!({
            "result": {
                "changes": {
                    "file:///gold/rename_subroutine.pl": [{
                        "range": {"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 19}},
                        "newText": "sum_values"
                    }],
                    (other_uri): [{
                        "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 3}},
                        "newText": "sum"
                    }]
                }
            }
        });
        let expected = vec![
            RenameExpectedEdit {
                uri: None,
                line: 4,
                character: 4,
                end_line: 4,
                end_character: 19,
                new_text: "sum_values".to_string(),
            },
            RenameExpectedEdit {
                uri: Some(other_uri.to_string()),
                line: 1,
                character: 0,
                end_line: 1,
                end_character: 3,
                new_text: "sum".to_string(),
            },
        ];
        if !rename_expected_edits_match(
            &response,
            "file:///gold/rename_subroutine.pl",
            Some(expected.as_slice()),
        ) {
            return Err("multi-file rename edits did not match target URIs".into());
        }
        let mut wrong_target = expected.clone();
        wrong_target[1].uri = Some("file:///gold/wrong.pl".to_string());
        if rename_expected_edits_match(
            &response,
            "file:///gold/rename_subroutine.pl",
            Some(wrong_target.as_slice()),
        ) {
            return Err("rename edit with wrong target URI passed exact oracle".into());
        }
        Ok(())
    }

    #[test]
    fn rename_count_assertion_rejects_null_and_error_at_zero_minimum() -> TestResult {
        let null_response = json!({"result": null});
        if rename_edit_count_at_least_passes(
            &null_response,
            "file:///gold/rename_subroutine.pl",
            0,
            None,
            VALID_RENAME_SOURCE,
            0,
            0,
        ) {
            return Err("null rename response passed a zero-minimum count assertion".into());
        }

        let error_response = json!({
            "error": {"code": -32603, "message": "rename failed"}
        });
        if rename_edit_count_at_least_passes(
            &error_response,
            "file:///gold/rename_subroutine.pl",
            0,
            None,
            VALID_RENAME_SOURCE,
            0,
            0,
        ) {
            return Err("error rename response passed a zero-minimum count assertion".into());
        }

        Ok(())
    }

    #[test]
    fn rename_count_assertion_rejects_empty_success_at_zero_minimum() -> TestResult {
        let empty_success = json!({"result": {"changes": {}}});
        if rename_edit_count_at_least_passes(
            &empty_success,
            "file:///gold/rename_subroutine.pl",
            0,
            None,
            VALID_RENAME_SOURCE,
            0,
            0,
        ) {
            return Err(
                "empty successful WorkspaceEdit passed a zero-minimum rename assertion".into()
            );
        }

        Ok(())
    }

    #[test]
    fn rename_null_requires_explicit_null_or_structured_error() -> TestResult {
        let assertion = rename_null_assertion();
        let uri = "file:///gold/rename_subroutine.pl";

        for passing in [
            json!({"result": null}),
            json!({"error": {"code": -32602, "message": "not renamable"}}),
        ] {
            if !rename_assertion_passes(&assertion, &passing, uri, VALID_RENAME_SOURCE) {
                return Err(format!("valid rename-null outcome was rejected: {passing}").into());
            }
        }

        for malformed in [
            json!({}),
            json!({"id": 1}),
            json!({"error": null}),
            json!({"error": {}}),
            json!({"error": {"code": -32602}}),
            json!({"error": {"message": "not renamable"}}),
            json!({
                "result": null,
                "error": {"code": -32602, "message": "not renamable"}
            }),
            json!({"error": {"code": -32000, "message": "test harness timeout"}}),
            json!({"error": {"code": -32050, "message": "Connection closed"}}),
            json!({"error": {"code": -32051, "message": "transport failure"}}),
            json!({"error": {"code": -32601, "message": "method not found"}}),
            json!({"error": {"code": -32603, "message": "internal error"}}),
            json!({"error": {"code": -32800, "message": "request cancelled"}}),
        ] {
            if rename_assertion_passes(&assertion, &malformed, uri, VALID_RENAME_SOURCE) {
                return Err(
                    format!("malformed rename response passed RenameNull: {malformed}").into()
                );
            }
        }

        for invalid_name in ["", "not a name", "!"] {
            let invalid_request =
                RenameAssertion { new_name: invalid_name.to_string(), ..assertion.clone() };
            let invalid_params = json!({
                "error": {"code": -32602, "message": "invalid replacement name"}
            });
            if rename_assertion_passes(&invalid_request, &invalid_params, uri, VALID_RENAME_SOURCE)
            {
                return Err(format!(
                    "invalid replacement name was accepted as RenameNull: {invalid_name:?}"
                )
                .into());
            }
        }

        for unicode_name in ["Δelta", "$名前"] {
            let unicode_request =
                RenameAssertion { new_name: unicode_name.to_string(), ..assertion.clone() };
            let invalid_params = json!({
                "error": {"code": -32602, "message": "not renamable"}
            });
            if !rename_assertion_passes(&unicode_request, &invalid_params, uri, VALID_RENAME_SOURCE)
            {
                return Err(format!(
                    "valid Unicode replacement name was rejected: {unicode_name:?}"
                )
                .into());
            }
        }

        if rename_replacement_name_is_well_formed("\u{0301}name") {
            return Err("a leading combining mark was accepted as a rename name".into());
        }

        let invalid_position = RenameAssertion { line: 99, character: 0, ..assertion.clone() };
        let invalid_params = json!({
            "error": {"code": -32602, "message": "invalid source position"}
        });
        if rename_assertion_passes(&invalid_position, &invalid_params, uri, VALID_RENAME_SOURCE) {
            return Err("InvalidParams at an out-of-range position passed RenameNull".into());
        }

        if rename_position_is_well_formed("😀name\n", 0, 1) {
            return Err("a position inside a UTF-16 surrogate pair was accepted".into());
        }

        Ok(())
    }

    #[test]
    fn successful_rename_assertions_reject_result_plus_error() -> TestResult {
        let mut contradictory = response(
            json!({"start":{"line":4,"character":4},"end":{"line":4,"character":19}}),
            "sum_values",
        );
        contradictory["error"] = json!({"code": -32603, "message": "contradictory response"});
        let uri = "file:///gold/rename_subroutine.pl";

        for assertion in [
            rename_success_assertion(RenameAssertionKind::RenameSucceeds),
            rename_success_assertion(RenameAssertionKind::RenameEditCountAtLeast { min: 1 }),
        ] {
            if rename_assertion_passes(&assertion, &contradictory, uri, VALID_RENAME_SOURCE) {
                return Err(format!(
                    "result-plus-error response passed successful rename mode: {:?}",
                    assertion.kind
                )
                .into());
            }
        }

        Ok(())
    }

    #[test]
    fn rename_parser_round_trips_omission_and_rejects_explicit_null() -> TestResult {
        let omitted: RenameAssertion = serde_json::from_str(
            r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values"}"#,
        )?;
        let serialized = serde_json::to_value(&omitted)?;
        if serialized.get("expected_edits").is_some() {
            return Err("omitted expected_edits must serialize as omission".into());
        }
        let round_tripped: RenameAssertion = serde_json::from_value(serialized)?;
        if round_tripped.expected_edits.is_some() {
            return Err("omitted expected_edits must round-trip as count-only mode".into());
        }

        let null = serde_json::from_str::<RenameAssertion>(
            r#"{"kind":"rename_succeeds","line":4,"character":4,"new_name":"sum_values","expected_edits":null}"#,
        );
        if null.is_ok() {
            return Err("explicit null expected_edits passed the parser".into());
        }

        let valid = json!({
            "range": {
                "start": {"line": 4, "character": 4},
                "end": {"line": 4, "character": 19}
            },
            "newText": "sum_values"
        });
        let response = response_with_entries(json!([valid]));
        if round_tripped.expected_edits.is_some()
            || !rename_expected_edits_match(
                &response,
                "file:///gold/rename_subroutine.pl",
                round_tripped.expected_edits.as_deref(),
            )
        {
            return Err("omitted expected_edits must use count-only scorecard mode".into());
        }

        Ok(())
    }
}
