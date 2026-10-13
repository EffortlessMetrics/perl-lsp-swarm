//! Format-body region recognition for the local-subroutine prepass.
//!
//! This is not a second format grammar. It only answers whether a line is a
//! single-line `format` opener or a `.` terminator, matching measured perl
//! 5.38.2 so picture-line text cannot enter [`super::LocalSymbolTable`].

use crate::unicode::{is_perl_identifier_continue, is_perl_identifier_start};

use super::{line_bounds, parse_qualified_name, skip_horizontal_whitespace};

/// Return `true` if `line` terminates a `format` body.
///
/// Perl accepts trailing horizontal whitespace after the terminating `.` but
/// rejects an indented one, so leading whitespace deliberately keeps the body
/// open rather than releasing it back to code.
pub(super) fn is_format_terminator(line: &str) -> bool {
    line.trim_end_matches([' ', '\t']) == "."
}

/// Return the start offset of the last format terminator in `input`.
///
/// A body terminates after `line_start` exactly when this offset is greater
/// than `line_start`, so one scan answers every opener. Walking the remaining
/// source per opener instead would be quadratic on input carrying many
/// opener-shaped lines, which is the cost class #10210 already tracks for
/// unclosed quote-like openers.
pub(super) fn last_format_terminator_line_start(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut cursor = 0usize;
    let mut last = None;

    while cursor < bytes.len() {
        let (line_end, next_line_start) = line_bounds(bytes, cursor);
        if is_format_terminator(&input[cursor..line_end]) {
            last = Some(cursor);
        }
        cursor = next_line_start;
    }

    last
}

/// Return `true` if a `format` body opened on the line at `line_start` is
/// terminated later in `input`.
///
/// An opener is only armed when its body closes. Otherwise a
/// format-opener-shaped line that is really prose would consume every remaining
/// line and hide the real declarations after it.
pub(super) fn format_body_terminates(
    terminator_scan: &mut Option<Option<usize>>,
    input: &str,
    line_start: usize,
) -> bool {
    let last = *terminator_scan.get_or_insert_with(|| last_format_terminator_line_start(input));
    last.is_some_and(|terminator| terminator > line_start)
}

/// Return `true` if a statement can begin immediately after `prefix`.
///
/// A statement begins at the start of a line and after `;`, `{` or `}`. It also
/// begins after Perl statement labels, which `format` accepts: `LABEL: format
/// STDOUT =` is valid and does declare a format. Verified against perl 5.38.2,
/// which also accepts a space before the colon (`L : format`) and more than one
/// label (`A: B: format`), so labels are peeled in a loop rather than once.
///
/// The label case must not admit the `::` package separator: `$Report::format`
/// also ends in a colon but is an ordinary name, not a labelled statement. Only
/// one colon is stripped per turn, so a `::` prefix still ends in a colon, which
/// is not an identifier character and therefore yields no label word.
pub(super) fn starts_a_statement(prefix: &str) -> bool {
    let mut prefix = prefix.trim_end_matches([' ', '\t']);

    loop {
        if prefix.is_empty() || prefix.ends_with([';', '{', '}']) {
            return true;
        }

        let Some(head) = prefix.strip_suffix(':') else {
            return false;
        };
        let head = head.trim_end_matches([' ', '\t']);

        let Some(label_start) = head
            .char_indices()
            .rev()
            .take_while(|(_, ch)| is_perl_identifier_continue(*ch))
            .last()
            .map(|(index, _)| index)
        else {
            return false;
        };

        let (before_label, label) = head.split_at(label_start);
        if !label.starts_with(is_perl_identifier_start) {
            return false;
        }

        // Each turn removes at least the colon, so this terminates.
        prefix = before_label.trim_end_matches([' ', '\t']);
    }
}

/// Return `true` if the `format` keyword at `offset` sits where a statement can
/// begin and is not part of a longer name, a method call, or a hash subscript.
pub(super) fn is_format_keyword_boundary(line: &str, offset: usize) -> bool {
    if !starts_a_statement(line[..offset].trim_end_matches([' ', '\t'])) {
        return false;
    }

    // `'` is already an identifier continuation here (legacy package
    // separator), so only `:` needs naming alongside.
    let after_offset = offset + "format".len();
    let after = line[after_offset..].chars().next();
    !after.is_some_and(|ch| is_perl_identifier_continue(ch) || ch == ':')
}

/// Return `true` if a `format` opener beginning at `after_keyword` completes on
/// this line, i.e. an optional name followed by `=` as the last non-comment
/// token.
///
/// The trailing check is what separates a real opener from an operator: `=>`,
/// `==` and `=~` all leave their second character in `rest`, which is neither
/// empty nor a comment, so no separate operator test is needed.
pub(super) fn format_opener_completes_line(line: &str, after_keyword: usize) -> bool {
    let mut offset = skip_horizontal_whitespace(line, after_keyword);

    if let Some((_, end)) = parse_qualified_name(line, offset) {
        offset = skip_horizontal_whitespace(line, end);
    }

    if !line[offset..].starts_with('=') {
        return false;
    }
    offset += '='.len_utf8();

    let rest = line[offset..].trim_start_matches([' ', '\t']);
    rest.is_empty() || rest.starts_with('#')
}

/// Return `true` if `offset` in `line` is a `format` keyword that opens a body
/// terminated later in `input`.
pub(super) fn opens_terminated_format_body(
    input: &str,
    line_start: usize,
    line: &str,
    offset: usize,
    terminator_scan: &mut Option<Option<usize>>,
) -> bool {
    line[offset..].starts_with("format")
        && is_format_keyword_boundary(line, offset)
        && format_opener_completes_line(line, offset + "format".len())
        && format_body_terminates(terminator_scan, input, line_start)
}

#[cfg(test)]
mod tests {
    use super::{format_opener_completes_line, is_format_keyword_boundary, is_format_terminator};

    #[test]
    fn terminator_matches_perl_dot_line_rules() {
        assert!(is_format_terminator("."));
        assert!(is_format_terminator(".  \t"));
        assert!(!is_format_terminator("  ."));
        assert!(!is_format_terminator(".......... @<<<"));
        assert!(!is_format_terminator(""));
        assert!(!is_format_terminator(".x"));
    }

    #[test]
    fn opener_requires_statement_position_and_end_of_line_equals() {
        assert!(is_format_keyword_boundary("format STDOUT =", 0));
        assert!(is_format_keyword_boundary("LABEL: format STDOUT =", 7));
        assert!(!is_format_keyword_boundary("$obj->format STDOUT =", 6));
        assert!(!is_format_keyword_boundary("format_width =", 0));
        assert!(format_opener_completes_line("format STDOUT =", "format".len()));
        assert!(format_opener_completes_line("format =", "format".len()));
        assert!(!format_opener_completes_line("format => 1", "format".len()));
        assert!(!format_opener_completes_line("format STDOUT", "format".len()));
    }
}
