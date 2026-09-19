//! Regression tests for the document-highlight text-fallback scan (#5409).
//!
//! Defect: `find_highlights` only scanned AST nodes, so a variable name that
//! also appeared in a comment, POD block, `__END__`/`__DATA__` section, or a
//! non-interpolated (single-quoted) string was never highlighted — even though
//! LSP document-highlight semantics expect textual occurrences to be marked
//! everywhere. The `find_references` handler already carried such a text
//! fallback; document-highlight did not, and the two disagreed.
//!
//! The fix adds a bounded raw-text scan for the variable's `sigil+name` (e.g.
//! `$count`), classifying text-only hits as `DocumentHighlightKind::Text`. Hits
//! that coincide with a real AST occurrence are coalesced by the dedup pass,
//! which prefers the more specific `Read`/`Write` kind.

use perl_lsp_rs_core::providers::document_highlight::{
    DocumentHighlightKind, DocumentHighlightProvider,
};

/// Parse source into an AST node using the same parser the live LSP path uses.
fn parse(source: &str) -> perl_ast::Node {
    let mut parser = perl_parser_core::Parser::new(source);
    match parser.parse() {
        Ok(ast) => ast,
        Err(e) => unreachable!("fixture must parse: {e:?}"),
    }
}

/// Fixture exercising every region the AST cannot see at once:
/// a comment, a single-quoted (non-interpolated) string, a POD block, and an
/// `__END__` section — plus two real code occurrences of `$count`.
const COMMENT_FIXTURE: &str = "\
my $count = 0;          # increment $count here
$count += 1;
my $note = 'see $count for the total';
=pod
Document the $count variable.
=cut
__END__
$count is not code here.
";

#[test]
fn text_fallback_highlights_comment_and_string_occurrences()
-> Result<(), Box<dyn std::error::Error>> {
    let ast = parse(COMMENT_FIXTURE);
    let provider = DocumentHighlightProvider::new();

    let decl = COMMENT_FIXTURE.find("$count").ok_or("fixture must contain $count declaration")?;
    let highlights = provider.find_highlights(&ast, COMMENT_FIXTURE, decl + 2);

    // Each textual `$count` occurrence must be represented:
    //   1. declaration (`my $count`)
    //   2. comment (`# increment $count here`)
    //   3. assignment (`$count += 1`)
    //   4. single-quoted string (`'see $count for the total'`)
    //   5. POD block (`$count variable.`)
    //   6. __END__ section (`$count is not code here.`)
    let expected_count = COMMENT_FIXTURE.matches("$count").count();
    assert_eq!(
        highlights.len(),
        expected_count,
        "every textual `$count` occurrence (comment, string, POD, __END__) must be highlighted, \
         got {highlights:?}"
    );

    // The comment occurrence (index 1) must be present as a Text highlight.
    let comment_occurrence =
        COMMENT_FIXTURE.find("# increment $count").ok_or("missing comment occurrence")?;
    let comment_hit = COMMENT_FIXTURE[comment_occurrence..].find("$count").map(|p| {
        let abs = comment_occurrence + p;
        (abs, abs + "$count".len())
    });
    if let Some((start, end)) = comment_hit {
        assert!(
            highlights.iter().any(|h| h.location.start() == start && h.location.end() == end),
            "comment occurrence at {start}..{end} must be highlighted, got {highlights:?}"
        );
    }

    // The real code occurrences must keep their more specific kind (Read/Write),
    // not be downgraded to Text by the fallback.
    let assignment = COMMENT_FIXTURE.find("$count += 1").ok_or("missing assignment occurrence")?;
    let assign_start = COMMENT_FIXTURE[assignment..]
        .find("$count")
        .map(|p| assignment + p)
        .ok_or("must locate $count in assignment")?;
    let assign_hit = highlights
        .iter()
        .find(|h| h.location.start() == assign_start)
        .ok_or("assignment occurrence must be among highlights")?;
    assert_ne!(
        assign_hit.kind,
        DocumentHighlightKind::Text,
        "real code occurrence must retain Read/Write kind, not be downgraded to Text"
    );

    Ok(())
}

#[test]
fn text_fallback_does_not_match_prefix_of_longer_variable_name()
-> Result<(), Box<dyn std::error::Error>> {
    // The target is `$count`, and `$counter` also appears. The text fallback
    // must respect the right-side word boundary: `$count` must not match the
    // `$count` prefix inside `$counter`.
    //
    // Two real `$count` occurrences (declaration + use) plus one real
    // `$counter` (declaration only — `$counter += 1` is a second occurrence).
    // The text fallback scanning for `$count` must NOT emit a spurious hit on
    // the `$counter` prefix.
    let source = "my $count = 0;\nmy $counter = 1;\n$count += $counter;\n";
    let ast = parse(source);
    let provider = DocumentHighlightProvider::new();

    // Cursor on the first `$count` declaration.
    let decl = source.find("$count").ok_or("fixture must contain $count")?;
    let highlights = provider.find_highlights(&ast, source, decl + 2);

    // `$count` appears exactly twice in the source (lines 1 and 3). The text
    // fallback must not add a third hit matching the `$count` prefix of
    // `$counter`.
    let count_hits: Vec<_> = highlights
        .iter()
        .filter(|h| {
            let text = &source[h.location.start()..h.location.end()];
            text == "$count"
        })
        .collect();
    assert_eq!(
        count_hits.len(),
        2,
        "`$count` must match exactly its 2 real occurrences, not the prefix of \
         `$counter`; got {highlights:?}"
    );
    // None of the `$count` hits may overlap with `$counter`.
    let counter_pos = source.find("$counter").ok_or("fixture must contain $counter")?;
    assert!(
        !highlights.iter().any(|h| h.location.start() == counter_pos),
        "no highlight may start at `$counter`'s position ({counter_pos}); got {highlights:?}"
    );
    Ok(())
}

#[test]
fn text_fallback_respects_utf8_word_boundary() -> Result<(), Box<dyn std::error::Error>> {
    // `$caf` must not match inside `$café` — the next char after `$caf` in
    // `$café` is `é` (a multi-byte UTF-8 letter), which is a word character.
    // The ASCII-only byte check would incorrectly accept it (#5409 review).
    let source = "my $caf = 1;\nmy $café = 2;\nprint $caf;\n";
    let ast = parse(source);
    let provider = DocumentHighlightProvider::new();

    let decl = source.find("$caf ").ok_or("fixture must contain $caf ")?;
    let highlights = provider.find_highlights(&ast, source, decl + 2);

    // `$caf` appears twice (declaration + print). It must NOT match the `$caf`
    // prefix inside `$café`.
    let caf_hits: Vec<_> = highlights
        .iter()
        .filter(|h| &source[h.location.start()..h.location.end()] == "$caf")
        .collect();
    assert_eq!(
        caf_hits.len(),
        2,
        "`$caf` must match exactly 2 occurrences, not the prefix of `$café`; got {highlights:?}"
    );
    Ok(())
}
