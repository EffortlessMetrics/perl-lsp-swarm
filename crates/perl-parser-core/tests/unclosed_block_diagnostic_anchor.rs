//! Unclosed-block diagnostics must point at the brace the user has to close.
//!
//! The parser used to anchor "Unclosed block" at the position it stopped at, which is
//! end-of-input. That offset is useless in an editor: it lands on the last (usually
//! blank) line, it moves whenever trailing whitespace changes anywhere in the file, and
//! every unclosed block in a file collapses onto the same offset, so nested unclosed
//! braces produce identical diagnostics with nothing to distinguish them.
//!
//! These tests pin the opening `{` as the anchor.

use perl_parser_core::{ParseError, Parser};

const UNCLOSED_BLOCK_MESSAGE: &str = "Unclosed block: expected '}' but reached end of input";

/// Byte offsets of every "Unclosed block" diagnostic, in the order the parser reports them.
fn unclosed_block_offsets(source: &str) -> Vec<usize> {
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    parser
        .errors()
        .iter()
        .filter_map(|error| match error {
            ParseError::SyntaxError { message, location } if message == UNCLOSED_BLOCK_MESSAGE => {
                Some(*location)
            }
            _ => None,
        })
        .collect()
}

/// Byte offsets of every `{` in `source` — the set of legitimate anchors.
fn brace_offsets(source: &str) -> Vec<usize> {
    source.match_indices('{').map(|(offset, _)| offset).collect()
}

#[test]
fn unclosed_sub_block_anchors_at_its_opening_brace() {
    let source = "package P;\n\nsub partial {\n    return 1;\n";

    assert_eq!(
        unclosed_block_offsets(source),
        brace_offsets(source),
        "the diagnostic should sit on the `{{` of `sub partial`, not at end of input"
    );
}

#[test]
fn nested_unclosed_blocks_get_distinct_anchors() {
    let source = "sub outer {\n    if ($x) {\n        my $y = 1;\n";

    let offsets = unclosed_block_offsets(source);
    assert_eq!(offsets.len(), 2, "both the inner and the outer block are unclosed: {offsets:?}");

    let mut sorted = offsets.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        brace_offsets(source),
        "each unclosed block should be reported at its own opening brace"
    );
}

#[test]
fn unclosed_block_anchor_survives_trailing_whitespace() {
    let source = "package P;\n\nsub partial {\n    return 1;\n";
    let padded: String = source
        .split_inclusive('\n')
        .map(|line| match line.strip_suffix('\n') {
            Some(body) if !body.trim().is_empty() => format!("{body}  \n"),
            _ => line.to_string(),
        })
        .collect();

    assert_ne!(source, padded, "the padded variant must actually differ");
    // Padding every line shifts absolute offsets, so compare the reported line instead:
    // the anchor must stay on the `sub partial {` line rather than sliding to the last
    // line of the file the way the end-of-input anchor did.
    assert_eq!(
        anchor_lines(source),
        anchor_lines(&padded),
        "trailing whitespace must not move the diagnostic to another line"
    );
    assert_eq!(anchor_lines(source), vec![3], "the anchor belongs on `sub partial {{`");
}

/// 1-based lines carrying an "Unclosed block" diagnostic.
fn anchor_lines(source: &str) -> Vec<usize> {
    unclosed_block_offsets(source)
        .into_iter()
        .map(|offset| source[..offset].matches('\n').count() + 1)
        .collect()
}

#[test]
fn unclosed_block_anchor_is_inside_the_source() {
    // The end-of-input anchor pushed the diagnostic to `source.len()`, which the LSP
    // layer then widened to `len + 1` — a range with no character in it.
    let source = "sub a {\n  my $x = 1;\n\nsub b { 2 }\n";

    for offset in unclosed_block_offsets(source) {
        assert!(
            offset < source.len(),
            "anchor {offset} must address a real byte of the {} byte source",
            source.len()
        );
        assert_eq!(
            source.as_bytes().get(offset).copied(),
            Some(b'{'),
            "anchor {offset} should land on an opening brace"
        );
    }
}

#[test]
fn closed_blocks_report_no_unclosed_diagnostic() {
    assert!(
        unclosed_block_offsets("sub ok { 1 }\nif ($x) { 2 }\n").is_empty(),
        "well-formed blocks must not report an unclosed block"
    );
}
