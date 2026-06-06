//! Selection range normalization for enhanced refactor actions.

/// Normalize a selected byte range so trailing statement punctuation does not
/// block expression-oriented refactor actions.
pub(super) fn normalize_range_for_refactors(source: &str, range: (usize, usize)) -> (usize, usize) {
    if source.is_empty() {
        return (0, 0);
    }

    let start = range.0.min(source.len());
    let mut end = range.1.min(source.len());

    if start >= end {
        return (start, end);
    }

    while end > start {
        // Use .get(..end) to avoid panicking on a non-char-boundary `end` value
        // that a stale or externally-sourced byte range might supply.
        let Some(ch) = source.get(..end).and_then(|s| s.chars().next_back()) else {
            // `end` is mid-char — snap to the nearest lower char boundary by
            // decrementing one byte at a time until we land on a boundary.
            end -= 1;
            while end > start && !source.is_char_boundary(end) {
                end -= 1;
            }
            continue;
        };

        if ch.is_whitespace() || ch == ';' {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (start, end.max(start))
}
