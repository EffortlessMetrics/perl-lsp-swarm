//! Product bounds for native layout parameters (#7501 / #9963).
//!
//! These limits apply before indentation or line-width values participate in
//! allocation or repetition in the native formatter pipeline.

/// Maximum native line width used for layout decisions.
pub const MAX_LINE_WIDTH: u32 = 1_000;

/// Minimum native line width.
pub const MIN_LINE_WIDTH: u32 = 1;

/// Maximum indentation step width (spaces per indent level when not using tabs).
pub const MAX_INDENT_WIDTH: u32 = 16;

/// Minimum indentation step width.
pub const MIN_INDENT_WIDTH: u32 = 1;

/// Maximum rendered indent columns for one line prefix (defense in depth at allocation seams).
pub const MAX_RENDER_INDENT_COLUMNS: usize = 4_096;

/// Normalize a configured line width for native layout.
#[must_use]
pub fn effective_line_width(raw: u32) -> u32 {
    raw.clamp(MIN_LINE_WIDTH, MAX_LINE_WIDTH)
}

/// Normalize a configured indent step for native layout and `indent_unit`.
#[must_use]
pub fn effective_indent_width(raw: u32) -> u32 {
    raw.clamp(MIN_INDENT_WIDTH, MAX_INDENT_WIDTH)
}

/// Bounded indent prefix column count for one rendered line.
#[must_use]
pub fn render_indent_columns(indent_level: usize, indent_width: u32, use_tabs: bool) -> usize {
    if use_tabs {
        indent_level.min(MAX_RENDER_INDENT_COLUMNS)
    } else {
        let step = usize::try_from(effective_indent_width(indent_width))
            .unwrap_or(MAX_INDENT_WIDTH as usize);
        indent_level
            .checked_mul(step)
            .unwrap_or(MAX_RENDER_INDENT_COLUMNS)
            .min(MAX_RENDER_INDENT_COLUMNS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_indent_width_does_not_request_huge_unit() {
        let columns = render_indent_columns(1, u32::MAX, false);
        assert_eq!(columns, MAX_INDENT_WIDTH as usize);
    }
}
