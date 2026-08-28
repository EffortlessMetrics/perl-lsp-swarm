//! Shared primitives for Perl module token parsing and boundary detection.

use unicode_ident::{is_xid_continue, is_xid_start};

/// Byte span for a parsed module token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleTokenSpan {
    /// Inclusive byte start offset in the source text.
    pub start: usize,
    /// Exclusive byte end offset in the source text.
    pub end: usize,
}

/// Parse a module token that starts at `start` in `text`.
///
/// A module token is one or more identifier segments separated by either
/// `::` (canonical) or `'` (legacy) separators. Segment starts and
/// continuations follow the lexer’s Unicode XID authority; continuations also
/// accept Perl’s emoji and join-control extensions. Returned offsets remain
/// exact UTF-8 byte spans.
#[must_use]
pub fn parse_module_token(text: &str, start: usize) -> Option<ModuleTokenSpan> {
    if start >= text.len() || !text.is_char_boundary(start) {
        return None;
    }

    let token_start = start;
    let mut index = parse_identifier_segment(text, start)?;

    while let Some(separator_len) = separator_len_at(text, index) {
        index += separator_len;
        index = parse_identifier_segment(text, index)?;
    }

    Some(ModuleTokenSpan { start: token_start, end: index })
}

/// Check if a span from `start` to `end` is bounded as a standalone token.
///
/// Empty, invalid, reversed, out-of-bounds, or mid-codepoint spans are rejected.
#[must_use]
pub fn has_standalone_module_token_boundaries(line: &str, start: usize, end: usize) -> bool {
    if start >= end
        || end > line.len()
        || !line.is_char_boundary(start)
        || !line.is_char_boundary(end)
    {
        return false;
    }

    let left_ok = !left_context_is_module_char(line, start);
    let right_ok = !right_context_is_module_char(line, end);

    left_ok && right_ok
}

/// Check whether `ch` belongs to the module token character class.
#[must_use]
pub fn is_module_token_char(ch: char) -> bool {
    is_identifier_continue(ch) || ch == ':'
}

/// Check whether `ch` belongs to Perl module identifier characters.
#[must_use]
pub fn is_module_identifier_char(ch: char) -> bool {
    is_identifier_continue(ch)
}

pub(crate) fn is_module_identifier_segment(segment: &str) -> bool {
    parse_identifier_segment(segment, 0).is_some_and(|end| end == segment.len())
}

fn separator_len_at(text: &str, index: usize) -> Option<usize> {
    let rest = text.get(index..)?;
    if rest.starts_with("::") {
        Some(2)
    } else if rest.starts_with('\'') {
        Some(1)
    } else {
        None
    }
}

fn parse_identifier_segment(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if !is_identifier_start(first) {
        return None;
    }

    let mut end = start + first.len_utf8();
    for (relative, ch) in chars {
        if !is_identifier_continue(ch) {
            break;
        }
        end = start + relative + ch.len_utf8();
    }

    Some(end)
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || is_xid_start(ch) || is_emoji_codepoint(ch)
}

fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch)
        || is_xid_continue(ch)
        || matches!(
            ch as u32,
            0x200C | 0x200D |
            0x20E3 |
            0xFE00..=0xFE0F |
            0xE0100..=0xE01EF |
            0x1F3FB..=0x1F3FF |
            0xE0020..=0xE007F
        )
}

fn is_emoji_codepoint(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1F000..=0x1F02F |
        0x1F0A0..=0x1F0FF |
        0x1F100..=0x1F1FF |
        0x1F200..=0x1F2FF |
        0x1F300..=0x1F6FF |
        0x1F700..=0x1F77F |
        0x1F780..=0x1F7FF |
        0x1F800..=0x1F8FF |
        0x1F900..=0x1F9FF |
        0x1FA00..=0x1FA6F |
        0x1FA70..=0x1FAFF |
        0x2600..=0x26FF |
        0x2700..=0x27BF
    )
}

fn left_context_is_module_char(line: &str, start: usize) -> bool {
    if start == 0 {
        return false;
    }

    let mut left = line[..start].char_indices();
    let Some((left_idx, ch)) = left.next_back() else {
        return false;
    };

    if ch != '\'' {
        return is_module_token_char(ch);
    }

    if left_idx == 0 {
        return false;
    }

    line[..left_idx].chars().next_back().is_some_and(is_module_identifier_char)
}

fn right_context_is_module_char(line: &str, end: usize) -> bool {
    if end >= line.len() {
        return false;
    }

    let mut right = line[end..].chars();
    let Some(ch) = right.next() else {
        return false;
    };

    if ch != '\'' {
        return is_module_token_char(ch);
    }

    right.next().is_some_and(is_module_identifier_char)
}
