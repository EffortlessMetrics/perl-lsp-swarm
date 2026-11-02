//! Accepted numeric ranges for formatting layout parameters (#7501).
//!
//! Values mirror `configuration_authority::catalog` and the native formatter's
//! `layout_bounds` module so schema, authority, and runtime agree.

use std::ops::RangeInclusive;

pub use perl_lsp_perltidy::{MAX_INDENT_WIDTH, MAX_LINE_WIDTH, MIN_INDENT_WIDTH, MIN_LINE_WIDTH};

/// Client/project `indentColumns` / `perltidy_indent_columns` product range.
pub const FORMATTING_INDENT_COLUMNS_RANGE: RangeInclusive<u32> =
    RangeInclusive::new(MIN_INDENT_WIDTH, MAX_INDENT_WIDTH);

/// Client/project `maximumLineLength` / `perltidy_maximum_line_length` product range.
pub const FORMATTING_MAXIMUM_LINE_LENGTH_RANGE: RangeInclusive<u32> = 20..=MAX_LINE_WIDTH;

/// `formatting.blockCommentIndentation` — bounded even though native formatting ignores it today.
pub const FORMATTING_BLOCK_COMMENT_INDENTATION_RANGE: RangeInclusive<u32> = 0..=32;

/// Editor `FormattingOptions.tab_size` when it feeds native layout or argv construction.
pub const FORMATTING_EDITOR_TAB_SIZE_RANGE: RangeInclusive<u32> =
    RangeInclusive::new(MIN_INDENT_WIDTH, MAX_INDENT_WIDTH);

/// Try to admit a client-supplied unsigned integer into a product range without `as u32` wrap.
#[must_use]
pub fn try_admit_formatting_u32(value: u64, range: &RangeInclusive<u32>) -> Option<u32> {
    let Ok(raw) = u32::try_from(value) else {
        return None;
    };
    range.contains(&raw).then_some(raw)
}

/// Clamp a trusted project-file value into a product range with a deterministic warning.
#[must_use]
pub fn clamp_project_formatting_u32(value: u32, range: &RangeInclusive<u32>, setting: &str) -> u32 {
    let clamped = value.clamp(*range.start(), *range.end());
    if clamped != value {
        tracing::warn!(
            target: "perl_lsp::config",
            setting,
            value,
            minimum = *range.start(),
            maximum = *range.end(),
            clamped,
            "formatting numeric out of product range; clamped",
        );
    }
    clamped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u64_max_does_not_wrap_into_indent_columns() {
        assert!(try_admit_formatting_u32(u64::MAX, &FORMATTING_INDENT_COLUMNS_RANGE).is_none());
        assert!(
            try_admit_formatting_u32(4_294_967_296, &FORMATTING_INDENT_COLUMNS_RANGE).is_none()
        );
    }

    #[test]
    fn indent_columns_range_matches_native_bounds() {
        assert_eq!(*FORMATTING_INDENT_COLUMNS_RANGE.start(), MIN_INDENT_WIDTH);
        assert_eq!(*FORMATTING_INDENT_COLUMNS_RANGE.end(), MAX_INDENT_WIDTH);
        assert_eq!(FORMATTING_EDITOR_TAB_SIZE_RANGE, FORMATTING_INDENT_COLUMNS_RANGE);
    }
}
