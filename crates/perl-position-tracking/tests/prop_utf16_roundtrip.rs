#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! Property-based tests for UTF-8 ↔ UTF-16 position roundtrip.
//!
//! Verifies that `offset_to_utf16_line_col` and `utf16_line_col_to_offset`
//! form a stable roundtrip for arbitrary Unicode strings.

use perl_position_tracking::{offset_to_utf16_line_col, utf16_line_col_to_offset};
use perl_test_generators::unicode_string;
use proptest::prelude::*;

// Keep CR coverage local: other users of unicode_string() are unaffected.
fn source_text() -> impl Strategy<Value = String> {
    prop_oneof![
        unicode_string(),
        (unicode_string(), prop::sample::select(vec!["\r", "\r\n", "\r\r\n", "\n", "\r\nx\r"]))
            .prop_map(|(mut text, ending)| {
                text.push_str(ending);
                text
            }),
        prop::sample::select(vec![
            "\r",
            "a\r",
            "a💖\r",
            "a\r\n",
            "a\r\r\n",
            "x\r\na💖\r",
            "\n",
            ""
        ])
        .prop_map(str::to_owned),
    ]
}

// Independent raw-byte oracle: LF ends a row; only its immediately preceding
// CR belongs to the separator. Include the empty terminal row.
fn row_bounds(text: &str) -> Vec<(usize, usize, usize)> {
    let mut rows = Vec::new();
    let mut start = 0;
    for (lf, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            let end = if lf > start && text.as_bytes()[lf - 1] == b'\r' { lf - 1 } else { lf };
            rows.push((start, end, lf + 1));
            start = lf + 1;
        }
    }
    rows.push((start, text.len(), text.len()));
    rows
}

fn normalized_boundary(text: &str, offset: usize) -> usize {
    for (_, content_end, separator_end) in row_bounds(text) {
        if offset >= content_end && offset < separator_end {
            return content_end;
        }
    }
    offset
}

proptest! {
    /// For any string and any byte offset within it, converting to
    /// UTF-16 line/col and back yields the same offset (or the nearest
    /// valid position).
    #[test]
    fn roundtrip_offset_to_utf16_and_back(
        s in source_text(),
        offset in 0usize..200usize,
    ) {
        let offset = offset.min(s.len());
        let (line, col) = offset_to_utf16_line_col(&s, offset);
        let back = utf16_line_col_to_offset(&s, line, col);
        // The roundtrip must land on the same line (may differ by ≤ 1 col
        // due to multi-byte char boundary snapping).
        let (line2, _col2) = offset_to_utf16_line_col(&s, back);
        prop_assert_eq!(line, line2, "line mismatch: {} → ({},{}) → {} → ({},{})", offset, line, col, back, line2, _col2);
    }

    /// Content boundaries roundtrip exactly; separator interiors normalize to content end.
    #[test]
    fn char_boundaries_roundtrip_with_separator_normalization(s in source_text()) {
        let mut boundaries: Vec<usize> = s.char_indices().map(|(idx, _)| idx).collect();
        boundaries.push(s.len());

        for offset in boundaries {
            let (line, col) = offset_to_utf16_line_col(&s, offset);
            let back = utf16_line_col_to_offset(&s, line, col);
            prop_assert_eq!(
                back,
                normalized_boundary(&s, offset),
                "char-boundary roundtrip mismatch: {} → ({},{}) → {} for {:?}",
                offset,
                line,
                col,
                back,
                s
            );
        }
    }

    /// For any string, every valid UTF-16 column on every line maps
    /// to a byte offset within bounds.
    #[test]
    fn utf16_col_stays_in_bounds(
        s in source_text(),
    ) {
        for (line, (line_start, line_end, _)) in row_bounds(&s).into_iter().enumerate() {
            let line = line as u32;
            let line_content = &s[line_start..line_end];
            let utf16_len = line_content.encode_utf16().count() as u32;
            // Test col = 0, midpoint, and end
            for col in [0, utf16_len / 2, utf16_len, utf16_len + 1] {
                let offset = utf16_line_col_to_offset(&s, line, col);
                prop_assert!(offset <= s.len(), "offset {} out of bounds for len {} (line={}, col={})", offset, s.len(), line, col);
            }
        }
    }

    /// Offsets must be monotonic as UTF-16 columns increase within a line.
    #[test]
    fn utf16_columns_map_to_monotonic_offsets(s in source_text()) {
        for (line, (line_start, line_end, _)) in row_bounds(&s).into_iter().enumerate() {
            let line = line as u32;
            let line_content = &s[line_start..line_end];
            let utf16_len = line_content.encode_utf16().count() as u32;

            let mut previous = utf16_line_col_to_offset(&s, line, 0);
            for col in 1..=(utf16_len + 2) {
                let current = utf16_line_col_to_offset(&s, line, col);
                prop_assert!(
                    current >= previous,
                    "non-monotonic offset mapping on line {} at col {}: {} -> {}",
                    line,
                    col,
                    previous,
                    current
                );
                prop_assert!(
                    current >= line_start && current <= line_end,
                    "offset {} escaped line bounds [{}, {}] on line {} col {}",
                    current,
                    line_start,
                    line_end,
                    line,
                    col
                );
                previous = current;
            }

        }
    }

    /// Empty string edge case.
    #[test]
    fn empty_string(s in Just(String::new())) {
        let (line, col) = offset_to_utf16_line_col(&s, 0);
        prop_assert_eq!((line, col), (0, 0));
        let back = utf16_line_col_to_offset(&s, line, col);
        prop_assert_eq!(back, 0);
    }
}

#[test]
fn literal_cr_rows_preserve_content_and_normalize_only_separators() {
    for (text, expected_rows) in [
        ("\r", vec![(0, 1, 1)]),
        ("a\r", vec![(0, 2, 2)]),
        ("a💖\r", vec![(0, 6, 6)]),
        ("a\r\n", vec![(0, 1, 3), (3, 3, 3)]),
        ("a\r\r\n", vec![(0, 2, 4), (4, 4, 4)]),
        ("x\r\na💖\r", vec![(0, 1, 3), (3, 9, 9)]),
        ("\n", vec![(0, 0, 1), (1, 1, 1)]),
        ("", vec![(0, 0, 0)]),
    ] {
        assert_eq!(row_bounds(text), expected_rows, "{text:?}");
        for (line, &(start, end, separator_end)) in expected_rows.iter().enumerate() {
            let column = text[start..end].encode_utf16().count() as u32;
            assert_eq!(utf16_line_col_to_offset(text, line as u32, column), end);
            assert_eq!(utf16_line_col_to_offset(text, line as u32, column + 20), end);
            for offset in end..separator_end {
                assert_eq!(offset_to_utf16_line_col(text, offset), (line as u32, column));
            }
        }
        for offset in text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())) {
            let (line, column) = offset_to_utf16_line_col(text, offset);
            assert_eq!(
                utf16_line_col_to_offset(text, line, column),
                normalized_boundary(text, offset)
            );
        }
        let last_line = expected_rows.len() - 1;
        let (start, end, _) = expected_rows[last_line];
        let eof = (last_line as u32, text[start..end].encode_utf16().count() as u32);
        assert_eq!(offset_to_utf16_line_col(text, text.len() + 20), eof);
        assert_eq!(utf16_line_col_to_offset(text, eof.0 + 20, 20), text.len());
    }
}
