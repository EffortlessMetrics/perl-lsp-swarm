//! Exercise the provider's UTF-16 input and full-file output projection (#17564).

use lsp_types::{Position, Range};
use perl_lsp_rs_core::providers::selection_range::SelectionRangeProvider;

fn assert_projection(source: &str, line: u32, line_end: u32, eof: Position) {
    // The supplementary character precedes $tail: its UTF-16 columns are 10..15,
    // not its byte columns 12..17 or Unicode scalar columns 9..14.
    let cursor = Position::new(line, 12);
    let results = SelectionRangeProvider::new().get_selection_ranges(source, &[cursor, eof]);
    assert_eq!(results.len(), 2, "{source:?}");
    assert_eq!(
        results[0].range,
        Range::new(Position::new(line, 10), Position::new(line, 15)),
        "token after supplementary Unicode: {source:?}"
    );

    for (selection, position) in results.iter().zip([cursor, eof]) {
        let mut current = selection;
        let mut has_full_line = false;
        loop {
            assert!(current.range.start <= position && position <= current.range.end, "{source:?}");
            has_full_line |=
                current.range == Range::new(Position::new(line, 0), Position::new(line, line_end));
            match current.parent.as_deref() {
                Some(parent) => {
                    assert!(parent.range.start <= current.range.start, "{source:?}");
                    assert!(parent.range.end >= current.range.end, "{source:?}");
                    current = parent;
                }
                None => break,
            }
        }
        assert_eq!(
            current.range,
            Range::new(Position::new(0, 0), eof),
            "full-file endpoint: {source:?}"
        );
        if position == cursor {
            assert!(has_full_line, "full line missing: {source:?}");
        }
    }
}

#[test]
fn terminal_bare_cr_remains_in_selection_range_endpoints() {
    assert_projection("say \"💖\"; $tail\r", 0, 16, Position::new(0, 16));
    assert_projection("# lead\r\nsay \"💖\"; $tail\r", 1, 16, Position::new(1, 16));
}

#[test]
fn crlf_separates_the_final_empty_line() {
    assert_projection("say \"💖\"; $tail\r\n", 0, 15, Position::new(1, 0));
    assert_projection("# lead\r\nsay \"💖\"; $tail\r\n", 1, 15, Position::new(2, 0));
}

#[test]
fn crlf_preserves_the_preceding_bare_cr() {
    assert_projection("say \"💖\"; $tail\r\r\n", 0, 16, Position::new(1, 0));
    assert_projection("# lead\r\nsay \"💖\"; $tail\r\r\n", 1, 16, Position::new(2, 0));
}
