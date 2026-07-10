//! Regression tests for document-highlight caret-at-trailing-edge boundary.
//!
//! Defect: `find_node_at_offset` (and its `extract_symbol_at_offset` fallback)
//! used a half-open containment check (`offset >= node.location.end`), so a
//! caret resting at the trailing edge of an identifier — the common "caret just
//! after the word" position produced by double-click-select in VS Code — yielded
//! zero highlights. The sibling references provider
//! (`navigation/references.rs`) already treats the trailing edge as on-token
//! (`offset > node.location.end`); this pins document-highlight to the same
//! inclusive-end boundary.

use perl_lsp_rs_core::providers::document_highlight::DocumentHighlightProvider;

/// Parse source into an AST node using the same parser the live LSP path uses.
fn parse(source: &str) -> perl_ast::Node {
    let mut parser = perl_parser_core::Parser::new(source);
    match parser.parse() {
        Ok(ast) => ast,
        Err(e) => unreachable!("fixture must parse: {e:?}"),
    }
}

/// Fixture: `$total` spans bytes 3..9 (`my $total = 0;\n$total += 5;\n`).
/// It appears twice (declaration + compound assignment), so a caret on the
/// symbol must yield exactly two highlights.
const FIXTURE: &str = "my $total = 0;\n$total += 5;\n";

#[test]
fn highlight_at_trailing_edge_returns_occurrences() -> Result<(), Box<dyn std::error::Error>> {
    let ast = parse(FIXTURE);
    let provider = DocumentHighlightProvider::new();

    // Byte 9 is immediately after the 'l' of the first `$total` — i.e. exactly
    // at `node.location.end`. This is the position a double-click-select caret
    // lands on. Pre-fix this returned 0 (the half-open `offset >= end` check
    // rejected 9 >= 9); post-fix it must return both occurrences.
    let highlights = provider.find_highlights(&ast, FIXTURE, 9);
    assert_eq!(
        highlights.len(),
        2,
        "caret at trailing edge (byte==node.end) must highlight both $total occurrences, got {highlights:?}"
    );

    Ok(())
}

#[test]
fn highlight_inside_token_still_returns_occurrences() -> Result<(), Box<dyn std::error::Error>> {
    // Sanity anchor: a caret in the middle of the token was always correct and
    // must remain so after the boundary change.
    let ast = parse(FIXTURE);
    let provider = DocumentHighlightProvider::new();

    let highlights = provider.find_highlights(&ast, FIXTURE, 6);
    assert_eq!(
        highlights.len(),
        2,
        "caret inside $total must highlight both occurrences, got {highlights:?}"
    );

    Ok(())
}

#[test]
fn highlight_past_token_returns_nothing() -> Result<(), Box<dyn std::error::Error>> {
    // Guard the other side of the boundary: byte 10 is past `$total` (on the
    // space) and must not highlight the variable. This ensures the fix widens
    // the inclusive end by exactly one byte, not further.
    let ast = parse(FIXTURE);
    let provider = DocumentHighlightProvider::new();

    let highlights = provider.find_highlights(&ast, FIXTURE, 10);
    assert!(
        highlights.is_empty(),
        "caret past $total (byte 10, on the space) must not highlight it, got {highlights:?}"
    );

    Ok(())
}
