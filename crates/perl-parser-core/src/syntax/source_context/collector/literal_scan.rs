//! Line-oriented literal/comment scanner lifted from completion `lexical_context`.
//!
//! Emits line-comment regions with quote/heredoc awareness so `#` inside string
//! literals is not classified as comment.

use std::cmp::Ordering;

use crate::syntax::text_line::is_identifier_byte;

use super::super::kind::SourceRegionKind;
use super::super::region::{SourceRegion, last_char_start};

pub(super) fn scan_line_comments_and_open_literals(source: &str) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    let mut literal_state = LiteralScanState::default();
    let mut in_pod_block = false;
    let mut pod_start: Option<usize> = None;
    let mut line_start = 0usize;

    for raw_line in source.split_inclusive('\n') {
        let line_end = line_start + raw_line.len();
        let line = strip_line_ending(raw_line);

        if is_pod_end_marker(line) {
            if let Some(start) = pod_start.take() {
                push_region(&mut regions, start, line_end, SourceRegionKind::Pod);
            }
            in_pod_block = false;
            line_start = line_end;
            continue;
        }

        if in_pod_block {
            line_start = line_end;
            continue;
        }

        let started_in_literal = literal_state.is_active();
        if is_pod_start_marker(line) && !started_in_literal {
            in_pod_block = true;
            pod_start = Some(line_start);
            line_start = line_end;
            continue;
        }

        if let Some(comment_start) =
            find_line_comment_start(source, line_start, line_end, &literal_state)
        {
            push_region(&mut regions, comment_start, line_end, SourceRegionKind::LineComment);
        }

        literal_state.scan_segment(source.as_bytes(), line_start, line_end);
        line_start = line_end;
    }

    if literal_state.is_active() {
        // Anchor the recovery span to the start of the final character: using
        // `len - 1` lands mid-codepoint when the unterminated literal ends in
        // multibyte text, violating the char-boundary invariant callers rely on
        // when slicing the source.
        push_region(
            &mut regions,
            last_char_start(source, source.len()),
            source.len(),
            SourceRegionKind::RecoveryAmbiguous,
        );
    } else if let Some(start) = pod_start {
        push_region(&mut regions, start, source.len(), SourceRegionKind::Pod);
    }

    regions
}

/// Heredoc body regions between opener line and closing delimiter line.
pub(super) fn scan_heredoc_regions(source: &str) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    let mut active: Option<(usize, String, bool)> = None;
    let mut line_start = 0usize;

    for raw_line in source.split_inclusive('\n') {
        let line_end = line_start + raw_line.len();
        let line = strip_line_ending(raw_line);

        if let Some((body_start, label, allow_indented)) = active.take() {
            // Trailing spaces/tabs after the delimiter still close the region.
            // `PerlLexer` ends the heredoc body on such a line; comparing only
            // the untrimmed line left the collector scanning to EOF and
            // reclassifying every following statement as `Heredoc`, because
            // `Heredoc` outranks `Code` in `region_precedence`.
            //
            // The trimmed comparison is an *additional* way to close, never a
            // replacement: a quoted label may itself end in whitespace
            // (`<<"EOF "`), and trimming alone would stop that line matching.
            let candidate =
                if allow_indented { line.trim_start_matches([' ', '\t']) } else { line };
            let closes = candidate == label || candidate.trim_end_matches([' ', '\t']) == label;
            if closes {
                push_region(&mut regions, body_start, line_start, SourceRegionKind::Heredoc);
            } else {
                active = Some((body_start, label, allow_indented));
            }
        } else if let Some((label, allow_indented)) = heredoc_opener_on_line(line) {
            active = Some((line_end, label, allow_indented));
        }

        line_start = line_end;
    }

    if let Some((body_start, _, _)) = active {
        push_region(&mut regions, body_start, source.len(), SourceRegionKind::Heredoc);
    }

    regions
}

fn heredoc_opener_on_line(line: &str) -> Option<(String, bool)> {
    let marker = line.find("<<")?;
    let before = &line[..marker];
    if before.ends_with('<') {
        return None;
    }
    // Guard against `<<` appearing inside a line comment (#5456). A `#` in the
    // prefix that is not inside a simple quote pair means the rest of the line
    // (including the `<<`) is a comment, not a heredoc opener. Without this,
    // `# see <<EOF docs` would swallow the rest of the file as a heredoc body.
    if prefix_has_unquoted_comment(before) {
        return None;
    }
    let mut rest = &line[marker + 2..];
    let allow_indented = if let Some(stripped) = rest.strip_prefix('~') {
        rest = stripped;
        true
    } else {
        false
    };
    rest = rest.trim_start_matches([' ', '\t']);
    let first = rest.chars().next()?;
    let label = match first {
        '\'' | '"' | '`' => {
            let after = &rest[first.len_utf8()..];
            let end = after.find(first)?;
            after[..end].to_string()
        }
        '\\' => {
            let after = &rest[1..];
            if !starts_heredoc_label(after) {
                return None;
            }
            let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
            after[..end].to_string()
        }
        // An *unquoted* heredoc label is an identifier, so it must start with a
        // Unicode letter or `_`. Accepting a leading digit made `my $y = $x << 2;`
        // parse as a heredoc opener whose body then swallowed the rest of the
        // file.
        _ if starts_heredoc_label(rest) => {
            let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
            rest[..end].to_string()
        }
        _ => return None,
    };
    if label.is_empty() { None } else { Some((label, allow_indented)) }
}

/// Whether `rest` starts an unquoted heredoc label, i.e. a Perl identifier.
fn starts_heredoc_label(rest: &str) -> bool {
    rest.starts_with(|character: char| character.is_alphabetic() || character == '_')
}

/// Whether `prefix` (the text before a candidate `<<` heredoc opener) contains
/// an unquoted `#` line-comment marker. A simple single/double-quote state
/// machine tracks whether the `#` is inside a string literal.
fn prefix_has_unquoted_comment(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && (in_single || in_double) {
            i += 2; // skip escaped char
            continue;
        }
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

fn push_region(regions: &mut Vec<SourceRegion>, start: usize, end: usize, kind: SourceRegionKind) {
    if let Some(region) = SourceRegion::new(start, end, kind) {
        regions.push(region);
    }
}

fn find_line_comment_start(
    source: &str,
    line_start: usize,
    line_end: usize,
    state: &LiteralScanState,
) -> Option<usize> {
    let mut probe = state.clone();
    let bytes = source.as_bytes();
    let mut index = line_start;
    while index < line_end {
        if probe.escaped {
            probe.escaped = false;
            index += 1;
            continue;
        }
        if let Some(active_literal) = probe.literal.as_mut() {
            if active_literal.advance(bytes[index], &mut probe.escaped) {
                probe.literal = None;
            }
            index += 1;
            continue;
        }
        match bytes[index] {
            b'\\' if probe.in_single_quote || probe.in_double_quote || probe.in_backtick => {
                probe.escaped = true;
            }
            b'\'' if !probe.in_double_quote && !probe.in_backtick => {
                probe.in_single_quote = !probe.in_single_quote;
            }
            b'"' if !probe.in_single_quote && !probe.in_backtick => {
                probe.in_double_quote = !probe.in_double_quote;
            }
            b'`' if !probe.in_single_quote && !probe.in_double_quote => {
                probe.in_backtick = !probe.in_backtick;
            }
            b'#' if !probe.in_single_quote && !probe.in_double_quote && !probe.in_backtick => {
                return Some(index);
            }
            _ if !probe.in_single_quote && !probe.in_double_quote && !probe.in_backtick => {
                if let Some(literal_start) = quote_like_literal_start(bytes, index) {
                    let consumed = literal_start.consumed;
                    probe.literal = Some(ActiveLiteral::new(literal_start));
                    index += consumed;
                    continue;
                }
                if let Some(literal_start) = slash_regex_literal_start(bytes, index) {
                    let consumed = literal_start.consumed;
                    probe.literal = Some(ActiveLiteral::new(literal_start));
                    index += consumed;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn strip_line_ending(line: &str) -> &str {
    let without_lf = line.strip_suffix('\n').unwrap_or(line);
    without_lf.strip_suffix('\r').unwrap_or(without_lf)
}

fn is_pod_start_marker(line: &str) -> bool {
    if is_pod_end_marker(line) {
        return false;
    }
    line.strip_prefix('=')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|command| command.is_ascii_alphabetic())
}

fn is_pod_end_marker(line: &str) -> bool {
    line.strip_prefix("=cut")
        .is_some_and(|rest| rest.chars().next().is_none_or(char::is_whitespace))
}

#[derive(Clone, Default)]
struct LiteralScanState {
    in_single_quote: bool,
    in_double_quote: bool,
    in_backtick: bool,
    literal: Option<ActiveLiteral>,
    pending_literal_body_start: Option<usize>,
    escaped: bool,
}

impl LiteralScanState {
    fn is_active(&self) -> bool {
        self.in_single_quote || self.in_double_quote || self.in_backtick || self.literal.is_some()
    }

    fn scan_segment(&mut self, bytes: &[u8], mut index: usize, end: usize) -> Option<usize> {
        let started_active = self.is_active();
        let mut resumed_code_index = None;

        loop {
            if let Some(body_start) = self.pending_literal_body_start {
                match index.cmp(&body_start) {
                    Ordering::Less => match body_start.cmp(&end) {
                        Ordering::Less => index = body_start,
                        Ordering::Equal | Ordering::Greater => break,
                    },
                    Ordering::Equal | Ordering::Greater => {}
                }
                self.pending_literal_body_start = None;
            }

            let Some(byte) = bytes.get(index..end).and_then(|remaining| remaining.first()).copied()
            else {
                break;
            };

            if self.escaped {
                self.escaped = false;
                index += 1;
                continue;
            }

            if let Some(active_literal) = self.literal.as_mut() {
                if active_literal.advance(byte, &mut self.escaped) {
                    self.literal = None;
                    if started_active && resumed_code_index.is_none() {
                        resumed_code_index = Some(index + 1);
                    }
                }
                index += 1;
                continue;
            }

            match byte {
                b'\\' if self.in_single_quote || self.in_double_quote || self.in_backtick => {
                    self.escaped = true
                }
                b'\'' if !self.in_double_quote && !self.in_backtick => {
                    self.in_single_quote = !self.in_single_quote;
                    if started_active && !self.in_single_quote && resumed_code_index.is_none() {
                        resumed_code_index = Some(index + 1);
                    }
                }
                b'"' if !self.in_single_quote && !self.in_backtick => {
                    self.in_double_quote = !self.in_double_quote;
                    if started_active && !self.in_double_quote && resumed_code_index.is_none() {
                        resumed_code_index = Some(index + 1);
                    }
                }
                b'`' if !self.in_single_quote && !self.in_double_quote => {
                    self.in_backtick = !self.in_backtick;
                    if started_active && !self.in_backtick && resumed_code_index.is_none() {
                        resumed_code_index = Some(index + 1);
                    }
                }
                b'#' if !self.in_single_quote && !self.in_double_quote && !self.in_backtick => {
                    break;
                }
                _ if !self.in_single_quote && !self.in_double_quote && !self.in_backtick => {
                    if let Some(literal_start) = quote_like_literal_start(bytes, index) {
                        let consumed = literal_start.consumed;
                        self.literal = Some(ActiveLiteral::new(literal_start));
                        let body_start = index + consumed;
                        match body_start.cmp(&end) {
                            Ordering::Greater => {
                                self.pending_literal_body_start = Some(body_start);
                                index = end;
                            }
                            Ordering::Less | Ordering::Equal => index = body_start,
                        }
                        continue;
                    }
                    if let Some(literal_start) = slash_regex_literal_start(bytes, index) {
                        let consumed = literal_start.consumed;
                        self.literal = Some(ActiveLiteral::new(literal_start));
                        index += consumed;
                        continue;
                    }
                }
                _ => {}
            }

            index += 1;
        }

        resumed_code_index
    }
}

#[derive(Clone, Copy)]
struct QuoteLikeLiteral {
    opener: u8,
    closer: u8,
    sections: usize,
    consumed: usize,
    kind: QuoteLikeLiteralKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuoteLikeLiteralKind {
    String,
    Regex,
}

#[derive(Clone)]
struct ActiveLiteral {
    opener: u8,
    closer: u8,
    sections_remaining: usize,
    depth: usize,
    awaiting_section_opener: bool,
    #[expect(dead_code, reason = "policy:5003-pr1: reserved for regex/string kind dispatch")]
    kind: QuoteLikeLiteralKind,
}

impl ActiveLiteral {
    fn new(literal: QuoteLikeLiteral) -> Self {
        Self {
            opener: literal.opener,
            closer: literal.closer,
            sections_remaining: literal.sections,
            depth: 1,
            awaiting_section_opener: false,
            kind: literal.kind,
        }
    }

    fn advance(&mut self, byte: u8, escaped: &mut bool) -> bool {
        if *escaped {
            *escaped = false;
            return false;
        }

        if byte == b'\\' {
            *escaped = true;
            return false;
        }

        if self.awaiting_section_opener {
            if byte == b';' {
                return true;
            } else if let Some(closer) = quote_like_closer(byte) {
                self.opener = byte;
                self.closer = closer;
                self.awaiting_section_opener = false;
                self.depth = 1;
            }
            return false;
        }

        if self.opener != self.closer && byte == self.opener {
            self.depth += 1;
            return false;
        }

        if byte != self.closer {
            return false;
        }

        self.depth = self.depth.saturating_sub(1);
        if self.depth > 0 {
            return false;
        }

        self.sections_remaining = self.sections_remaining.saturating_sub(1);
        if self.sections_remaining == 0 {
            return true;
        }

        if self.opener == self.closer {
            self.depth = 1;
        } else {
            self.awaiting_section_opener = true;
        }
        false
    }
}

fn quote_like_literal_start(bytes: &[u8], index: usize) -> Option<QuoteLikeLiteral> {
    if !quote_like_operator_boundary(bytes, index) {
        return None;
    }
    if quote_like_follows_sub_declaration(bytes, index) {
        return None;
    }
    if quote_like_follows_method_or_qualified_name(bytes, index) {
        return None;
    }
    if quote_like_is_file_test_s_operator(bytes, index) {
        return None;
    }

    let (delimiter_offset, sections, allow_space, kind) =
        quote_like_operator_parameters(bytes.get(index).copied()?, bytes.get(index + 1).copied())?;

    let delimiter_index = index + delimiter_offset;
    let delimiter_index =
        if allow_space { skip_ascii_space(bytes, delimiter_index) } else { delimiter_index };
    if quote_like_is_braced_bareword_key(bytes, index, delimiter_index) {
        return None;
    }
    if bytes.get(delimiter_index..delimiter_index + 2) == Some(b"=>") {
        return None;
    }

    let opener = bytes.get(delimiter_index).copied()?;
    let closer = quote_like_closer(opener)?;
    Some(QuoteLikeLiteral { opener, closer, sections, consumed: delimiter_index + 1 - index, kind })
}

fn quote_like_operator_parameters(
    byte: u8,
    next: Option<u8>,
) -> Option<(usize, usize, bool, QuoteLikeLiteralKind)> {
    match (byte, next) {
        (b'q', Some(b'r')) => Some((2, 1, true, QuoteLikeLiteralKind::Regex)),
        (b'q', Some(b'q' | b'w' | b'x')) => Some((2, 1, true, QuoteLikeLiteralKind::String)),
        (b't', Some(b'r')) => Some((2, 2, true, QuoteLikeLiteralKind::Regex)),
        (b'q', _) => Some((1, 1, true, QuoteLikeLiteralKind::String)),
        (b'm', _) => Some((1, 1, true, QuoteLikeLiteralKind::Regex)),
        (b's' | b'y', _) => Some((1, 2, true, QuoteLikeLiteralKind::Regex)),
        _ => None,
    }
}

fn quote_like_is_file_test_s_operator(bytes: &[u8], index: usize) -> bool {
    if bytes.get(index) != Some(&b's') {
        return false;
    }

    let mut before = index;
    while before > 0 && matches!(bytes.get(before - 1), Some(b' ' | b'\t')) {
        before -= 1;
    }

    if bytes.get(before.saturating_sub(1)) != Some(&b'-') {
        return false;
    }

    before <= 1
        || bytes.get(before - 2).is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
}

fn quote_like_is_braced_bareword_key(bytes: &[u8], index: usize, delimiter_index: usize) -> bool {
    let mut before = index;
    while before > 0 && matches!(bytes.get(before - 1), Some(b' ' | b'\t')) {
        before -= 1;
    }

    if bytes.get(before.saturating_sub(1)) != Some(&b'{') {
        return false;
    }

    let after_operator = skip_ascii_space(bytes, delimiter_index);
    bytes.get(after_operator) == Some(&b'}')
}

fn quote_like_follows_sub_declaration(bytes: &[u8], index: usize) -> bool {
    let mut before = index;
    while before > 0 && matches!(bytes.get(before - 1), Some(b' ' | b'\t')) {
        before -= 1;
    }

    let word_end = before;
    while before > 0
        && bytes.get(before - 1).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        before -= 1;
    }

    before < word_end && bytes.get(before..word_end) == Some(b"sub")
}

fn quote_like_follows_method_or_qualified_name(bytes: &[u8], index: usize) -> bool {
    let mut before = index;
    while before > 0 && matches!(bytes.get(before - 1), Some(b' ' | b'\t')) {
        before -= 1;
    }

    before >= 2 && matches!(bytes.get(before - 2..before), Some(b"->") | Some(b"::"))
}

fn slash_regex_literal_start(bytes: &[u8], index: usize) -> Option<QuoteLikeLiteral> {
    if bytes.get(index) != Some(&b'/')
        || !(slash_follows_binding_operator(bytes, index)
            || slash_starts_bare_regex_literal(bytes, index))
    {
        return None;
    }

    Some(QuoteLikeLiteral {
        opener: b'/',
        closer: b'/',
        sections: 1,
        consumed: 1,
        kind: QuoteLikeLiteralKind::Regex,
    })
}

fn slash_follows_binding_operator(bytes: &[u8], index: usize) -> bool {
    let mut before = index;
    while before > 0 && matches!(bytes.get(before - 1), Some(b' ' | b'\t')) {
        before -= 1;
    }

    before >= 2
        && matches!(bytes.get(before - 2), Some(b'=' | b'!'))
        && bytes.get(before - 1) == Some(&b'~')
}

fn slash_starts_bare_regex_literal(bytes: &[u8], index: usize) -> bool {
    let before = bytes[..index]
        .iter()
        .rposition(|byte| !matches!(byte, b' ' | b'\t'))
        .map_or(0, |position| position + 1);

    if before == 0 {
        return true;
    }

    if bytes.get(before - 1).is_some_and(|byte| {
        matches!(*byte, b'(' | b',' | b'=' | b'!' | b'&' | b'|' | b';' | b'{' | b'~')
    }) {
        return true;
    }

    let word_start = bytes[..before]
        .iter()
        .rposition(|byte| !is_identifier_byte(*byte))
        .map_or(0, |position| position + 1);
    matches!(
        bytes.get(word_start..before),
        Some(
            b"and"
                | b"do"
                | b"eval"
                | b"for"
                | b"foreach"
                | b"given"
                | b"grep"
                | b"if"
                | b"map"
                | b"not"
                | b"or"
                | b"return"
                | b"split"
                | b"unless"
                | b"until"
                | b"when"
                | b"while"
        )
    )
}

fn quote_like_operator_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0
        || bytes.get(index.saturating_sub(1)).is_none_or(|byte| {
            !byte.is_ascii_alphanumeric()
                && *byte != b'_'
                && !matches!(*byte, b'$' | b'@' | b'%' | b'&' | b'*')
        })
}

fn skip_ascii_space(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

fn quote_like_closer(opener: u8) -> Option<u8> {
    match opener {
        b'/' => Some(b'/'),
        b'{' => Some(b'}'),
        b'[' => Some(b']'),
        b'(' => Some(b')'),
        b'<' => Some(b'>'),
        _ if opener.is_ascii_punctuation() => Some(opener),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte offset of `needle` in `haystack`, as an assertion-friendly `Option`.
    fn offset_of(haystack: &str, needle: &str) -> Option<usize> {
        haystack.find(needle)
    }

    fn regions_of_kind(regions: &[SourceRegion], kind: SourceRegionKind) -> Vec<SourceRegion> {
        regions.iter().copied().filter(|region| region.kind == kind).collect()
    }

    // ---- strip_line_ending -------------------------------------------------

    #[test]
    fn strip_line_ending_removes_lf_crlf_and_leaves_bare_lines() {
        assert_eq!(strip_line_ending("code\n"), "code", "a trailing LF must be removed");
        assert_eq!(strip_line_ending("code\r\n"), "code", "a trailing CRLF must be removed");
        assert_eq!(
            strip_line_ending("code"),
            "code",
            "a final line without a terminator must be unchanged"
        );
        assert_eq!(
            strip_line_ending("code\r"),
            "code",
            "a trailing CR is stripped even without a following LF"
        );
        assert_eq!(
            strip_line_ending("co\rde"),
            "co\rde",
            "an interior CR is content, not a terminator"
        );
    }

    // ---- POD markers -------------------------------------------------------

    #[test]
    fn pod_start_marker_requires_an_alphabetic_command() {
        assert!(is_pod_start_marker("=head1 NAME"), "=head1 opens a POD block");
        assert!(is_pod_start_marker("=pod"), "=pod opens a POD block");
        assert!(!is_pod_start_marker("=1"), "a digit command is not a POD opener");
        assert!(!is_pod_start_marker("="), "a bare = is not a POD opener");
        assert!(!is_pod_start_marker(" =head1"), "an indented = is not a POD opener");
        assert!(!is_pod_start_marker("=cut"), "=cut closes POD, it never opens it");
    }

    #[test]
    fn pod_end_marker_requires_a_word_boundary_after_cut() {
        assert!(is_pod_end_marker("=cut"), "a bare =cut closes POD");
        assert!(is_pod_end_marker("=cut  trailing"), "=cut followed by space closes POD");
        assert!(!is_pod_end_marker("=cutlery"), "=cutlery is a POD command, not a terminator");
        assert!(!is_pod_end_marker("=head1"), "=head1 is not a terminator");
    }

    // ---- scan_line_comments_and_open_literals -------------------------------

    #[test]
    fn line_comment_region_starts_at_the_hash_and_runs_to_end_of_line() -> Result<(), String> {
        let source = "my $x = 1; # trailing note\nmy $y = 2;\n";
        let regions = scan_line_comments_and_open_literals(source);
        let comments = regions_of_kind(&regions, SourceRegionKind::LineComment);
        let hash = offset_of(source, "#").ok_or("fixture must contain a hash")?;
        assert_eq!(comments.len(), 1, "exactly one line comment expected: {regions:?}");
        assert_eq!(comments[0].start, hash, "the comment must start at the hash");
        assert_eq!(
            comments[0].end,
            offset_of(source, "my $y").ok_or("fixture must contain a second line")?,
            "the comment must end at the start of the next line"
        );
        Ok(())
    }

    #[test]
    fn hash_inside_quotes_is_not_a_comment() {
        for source in [
            "my $x = \"not # a comment\";\n",
            "my $x = 'not # a comment';\n",
            "my $x = `echo not # a comment`;\n",
            "my $x = q{not # a comment};\n",
            "my $x =~ /not # a comment/;\n",
        ] {
            let regions = scan_line_comments_and_open_literals(source);
            assert!(
                regions_of_kind(&regions, SourceRegionKind::LineComment).is_empty(),
                "a hash inside a literal must not open a comment: {source:?} -> {regions:?}"
            );
        }
    }

    #[test]
    fn hash_after_a_closed_literal_still_opens_a_comment() -> Result<(), String> {
        let source = "my $x = \"value\"; # real comment\n";
        let regions = scan_line_comments_and_open_literals(source);
        let comments = regions_of_kind(&regions, SourceRegionKind::LineComment);
        assert_eq!(comments.len(), 1, "the trailing comment must be found: {regions:?}");
        assert_eq!(
            comments[0].start,
            offset_of(source, "# real").ok_or("fixture must contain the comment")?,
            "the comment must start after the closed string"
        );
        Ok(())
    }

    #[test]
    fn pod_block_spans_from_opener_through_the_cut_line() -> Result<(), String> {
        let source = "code();\n=head1 NAME\n\nbody\n\n=cut\nmore();\n";
        let regions = scan_line_comments_and_open_literals(source);
        let pods = regions_of_kind(&regions, SourceRegionKind::Pod);
        assert_eq!(pods.len(), 1, "exactly one POD region expected: {regions:?}");
        assert_eq!(
            pods[0].start,
            offset_of(source, "=head1").ok_or("fixture must contain =head1")?,
            "the POD region must start at the opener line"
        );
        assert_eq!(
            pods[0].end,
            offset_of(source, "more()").ok_or("fixture must contain trailing code")?,
            "the POD region must end after the =cut line"
        );
        Ok(())
    }

    #[test]
    fn pod_body_lines_do_not_produce_comment_regions() {
        let source = "=head1 NAME\n# not code, inside pod\n=cut\n";
        let regions = scan_line_comments_and_open_literals(source);
        assert!(
            regions_of_kind(&regions, SourceRegionKind::LineComment).is_empty(),
            "a hash inside a POD body must not be a line comment: {regions:?}"
        );
    }

    #[test]
    fn unterminated_pod_runs_to_end_of_source() -> Result<(), String> {
        let source = "code();\n=head1 NAME\nbody\n";
        let regions = scan_line_comments_and_open_literals(source);
        let pods = regions_of_kind(&regions, SourceRegionKind::Pod);
        assert_eq!(pods.len(), 1, "an unterminated POD block still yields one region: {regions:?}");
        assert_eq!(
            pods[0].start,
            offset_of(source, "=head1").ok_or("fixture must contain =head1")?,
            "the POD region must start at the opener"
        );
        assert_eq!(pods[0].end, source.len(), "an unterminated POD block runs to EOF");
        Ok(())
    }

    #[test]
    fn pod_opener_inside_an_open_literal_is_not_a_pod_block() {
        let source = "my $x = \"open\n=head1 still string\n";
        let regions = scan_line_comments_and_open_literals(source);
        assert!(
            regions_of_kind(&regions, SourceRegionKind::Pod).is_empty(),
            "an =head1 line inside an open string must not open POD: {regions:?}"
        );
    }

    #[test]
    fn unterminated_literal_recovery_region_lands_on_a_char_boundary() {
        let source = "my $x = \"unterminated é";
        let regions = scan_line_comments_and_open_literals(source);
        let recovery = regions_of_kind(&regions, SourceRegionKind::RecoveryAmbiguous);
        assert_eq!(recovery.len(), 1, "an unterminated literal must recover: {regions:?}");
        assert_eq!(recovery[0].end, source.len(), "recovery must extend to EOF");
        assert!(
            source.is_char_boundary(recovery[0].start),
            "recovery start {} must be a char boundary in {source:?}",
            recovery[0].start
        );
        assert_eq!(
            &source[recovery[0].start..recovery[0].end],
            "é",
            "recovery must anchor on the final character, not the final byte"
        );
    }

    #[test]
    fn balanced_source_produces_no_recovery_region() {
        let source = "my $x = \"closed\";\n";
        let regions = scan_line_comments_and_open_literals(source);
        assert!(
            regions_of_kind(&regions, SourceRegionKind::RecoveryAmbiguous).is_empty(),
            "balanced source must not recover: {regions:?}"
        );
    }

    // ---- scan_heredoc_regions ----------------------------------------------

    #[test]
    fn heredoc_body_spans_opener_line_end_through_terminator_line_start() -> Result<(), String> {
        let source = "my $x = <<EOF;\nbody line\nEOF\ntail();\n";
        let regions = scan_heredoc_regions(source);
        assert_eq!(regions.len(), 1, "one heredoc body expected: {regions:?}");
        assert_eq!(
            regions[0].start,
            offset_of(source, "body").ok_or("fixture must contain a body")?,
            "the body starts on the line after the opener"
        );
        assert_eq!(
            regions[0].end,
            offset_of(source, "EOF\ntail").ok_or("fixture must contain a terminator")?,
            "the body ends at the start of the terminator line"
        );
        Ok(())
    }

    #[test]
    fn indented_terminator_only_closes_a_tilde_heredoc() {
        let indented = "my $x = <<~EOF;\nbody\n    EOF\ntail();\n";
        let plain = "my $x = <<EOF;\nbody\n    EOF\ntail();\n";

        let indented_regions = scan_heredoc_regions(indented);
        assert_eq!(indented_regions.len(), 1, "<<~ closes on an indented terminator");
        assert!(
            indented_regions[0].end < indented.len(),
            "<<~ must close before EOF: {indented_regions:?}"
        );

        let plain_regions = scan_heredoc_regions(plain);
        assert_eq!(plain_regions.len(), 1, "plain heredoc still opens a body");
        assert_eq!(
            plain_regions[0].end,
            plain.len(),
            "an indented terminator must not close a plain heredoc"
        );
    }

    /// Names the `trim_end_matches` closer seam directly rather than reaching it
    /// through `SourceRegionIndex::build`. `PerlLexer` closes a heredoc on a
    /// delimiter line padded with spaces or tabs; comparing the untrimmed line
    /// left the body open to EOF, so every following statement was reclassified
    /// as `Heredoc` (which outranks `Code` in `region_precedence`).
    #[test]
    fn terminator_closes_despite_trailing_spaces_or_tabs() {
        for source in [
            "my $x = <<EOF;\nbody\nEOF  \ntail();\n",
            "my $x = <<EOF;\nbody\nEOF\t\ntail();\n",
            "my $x = <<~EOF;\nbody\n    EOF \ntail();\n",
        ] {
            let regions = scan_heredoc_regions(source);
            assert_eq!(regions.len(), 1, "one heredoc body expected in {source:?}: {regions:?}");
            assert!(
                regions[0].end < source.len(),
                "a whitespace-padded terminator must close the body before EOF in {source:?}: \
                 {regions:?}"
            );
        }
    }

    /// The trimmed comparison must be additive, not a replacement: a quoted
    /// label that itself ends in whitespace still closes on its exact line.
    #[test]
    fn quoted_label_ending_in_whitespace_still_closes_exactly() {
        let source = "my $x = <<\"EOF \";\nbody\nEOF \ntail();\n";
        let regions = scan_heredoc_regions(source);
        assert_eq!(regions.len(), 1, "one heredoc body expected: {regions:?}");
        assert!(
            regions[0].end < source.len(),
            "a label ending in whitespace must still close on its exact line: {regions:?}"
        );
    }

    /// The negative half: trailing *non*-whitespace still does not close, so the
    /// trim is not a blanket relaxation of delimiter matching.
    #[test]
    fn terminator_does_not_close_on_trailing_non_whitespace() {
        let source = "my $x = <<EOF;\nbody\nEOF;\ntail();\n";
        let regions = scan_heredoc_regions(source);
        assert_eq!(regions.len(), 1, "one heredoc body expected: {regions:?}");
        assert_eq!(
            regions[0].end,
            source.len(),
            "`EOF;` is not the delimiter and must not close the body: {regions:?}"
        );
    }

    #[test]
    fn left_shift_expression_is_not_a_heredoc_opener() {
        for source in ["my $y = $x << 2;\n", "my $y = $x <<< 2;\n"] {
            assert!(
                scan_heredoc_regions(source).is_empty(),
                "a shift expression must not open a heredoc: {source:?}"
            );
        }
    }

    #[test]
    fn heredoc_opener_label_forms() {
        assert_eq!(
            heredoc_opener_on_line("my $x = <<EOF;"),
            Some(("EOF".to_string(), false)),
            "a bare label is an unindented heredoc"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<~  EOF;"),
            Some(("EOF".to_string(), true)),
            "<<~ allows an indented terminator and skips spaces before the label"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<~\tEOF;"),
            Some(("EOF".to_string(), true)),
            "<<~ allows tabs before an indented terminator label"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<~;"),
            None,
            "<<~ without a label is not a heredoc opener"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<'END OF';"),
            Some(("END OF".to_string(), false)),
            "a single-quoted label may contain spaces"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<\"E O\";"),
            Some(("E O".to_string(), false)),
            "a double-quoted label may contain spaces"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<`CMD`;"),
            Some(("CMD".to_string(), false)),
            "a backtick label is a command heredoc"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<\\EOF;"),
            Some(("EOF".to_string(), false)),
            "a backslash-escaped label is accepted"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<\\1;"),
            None,
            "a backslash followed by a non-identifier is not a label"
        );
        assert_eq!(heredoc_opener_on_line("my $x = <<2;"), None, "a digit cannot start a label");
        assert_eq!(heredoc_opener_on_line("my $x = <<;"), None, "an empty label is rejected");
        assert_eq!(heredoc_opener_on_line("my $x = <<<EOF;"), None, "<<< is not a heredoc opener");
        assert_eq!(heredoc_opener_on_line("my $x = 1;"), None, "no << marker, no heredoc");
        assert_eq!(
            heredoc_opener_on_line("my $x = <<'unterminated;"),
            None,
            "an unterminated quoted label is rejected"
        );
        assert_eq!(
            heredoc_opener_on_line("my $x = <<é;"),
            Some(("é".to_string(), false)),
            "a Unicode identifier can start an unquoted label"
        );
    }

    #[test]
    fn starts_heredoc_label_accepts_identifier_starts_only() {
        assert!(starts_heredoc_label("EOF"), "an uppercase letter starts a label");
        assert!(starts_heredoc_label("_private"), "an underscore starts a label");
        assert!(!starts_heredoc_label("1EOF"), "a digit does not start a label");
        assert!(!starts_heredoc_label(""), "an empty rest does not start a label");
    }

    // ---- push_region -------------------------------------------------------

    #[test]
    fn push_region_rejects_inverted_spans_and_keeps_ordered_ones() {
        let mut regions = Vec::new();
        push_region(&mut regions, 5, 2, SourceRegionKind::Pod);
        assert!(regions.is_empty(), "an inverted span must not be pushed");
        push_region(&mut regions, 2, 5, SourceRegionKind::Pod);
        assert_eq!(
            regions,
            vec![SourceRegion { start: 2, end: 5, kind: SourceRegionKind::Pod }],
            "an ordered span must be pushed unchanged"
        );
    }

    // ---- find_line_comment_start -------------------------------------------

    #[test]
    fn find_line_comment_start_reports_the_first_bare_hash() {
        let source = "code(); # note\n";
        let state = LiteralScanState::default();
        assert_eq!(
            find_line_comment_start(source, 0, source.len(), &state),
            offset_of(source, "#"),
            "the first bare hash starts the comment"
        );
    }

    #[test]
    fn find_line_comment_start_honours_an_inherited_open_literal() {
        // The line begins inside a string opened on a previous line, so the
        // hash is literal text until the string closes.
        let source = "still string # not a comment\" # yes a comment\n";
        let state = LiteralScanState { in_double_quote: true, ..LiteralScanState::default() };
        let found = find_line_comment_start(source, 0, source.len(), &state);
        assert_eq!(
            found,
            offset_of(source, "# yes"),
            "only the hash after the closing quote is a comment"
        );
    }

    #[test]
    fn find_line_comment_start_skips_escaped_quotes() {
        let source = "my $x = \"a \\\" # b\"; # real\n";
        let state = LiteralScanState::default();
        assert_eq!(
            find_line_comment_start(source, 0, source.len(), &state),
            offset_of(source, "# real"),
            "an escaped quote must not close the string early"
        );
    }

    #[test]
    fn find_line_comment_start_returns_none_without_a_bare_hash() {
        let source = "code();\n";
        let state = LiteralScanState::default();
        assert_eq!(
            find_line_comment_start(source, 0, source.len(), &state),
            None,
            "a line with no hash has no comment start"
        );
    }

    // ---- LiteralScanState --------------------------------------------------

    #[test]
    fn literal_scan_state_is_active_tracks_every_open_form() {
        let mut state = LiteralScanState::default();
        assert!(!state.is_active(), "a fresh state is inactive");

        state.in_single_quote = true;
        assert!(state.is_active(), "an open single quote is active");
        state.in_single_quote = false;

        state.in_double_quote = true;
        assert!(state.is_active(), "an open double quote is active");
        state.in_double_quote = false;

        state.in_backtick = true;
        assert!(state.is_active(), "an open backtick is active");
        state.in_backtick = false;

        state.literal = Some(ActiveLiteral::new(QuoteLikeLiteral {
            opener: b'{',
            closer: b'}',
            sections: 1,
            consumed: 2,
            kind: QuoteLikeLiteralKind::String,
        }));
        assert!(state.is_active(), "an open quote-like literal is active");
    }

    #[test]
    fn scan_segment_leaves_state_open_across_a_multi_line_string() {
        let source = "my $x = \"open\n";
        let mut state = LiteralScanState::default();
        state.scan_segment(source.as_bytes(), 0, source.len());
        assert!(state.is_active(), "an unclosed string keeps the state active");
    }

    #[test]
    fn scan_segment_reports_where_code_resumes_after_an_inherited_literal() {
        let source = "tail\" + 1;\n";
        let mut state = LiteralScanState { in_double_quote: true, ..LiteralScanState::default() };
        let resumed = state.scan_segment(source.as_bytes(), 0, source.len());
        assert_eq!(
            resumed,
            offset_of(source, "\"").map(|index| index + 1),
            "code resumes immediately after the closing quote"
        );
        assert!(!state.is_active(), "the inherited string is closed by this segment");
    }

    #[test]
    fn scan_segment_stops_at_a_bare_hash() {
        let source = "code(); # \"not a string\n";
        let mut state = LiteralScanState::default();
        state.scan_segment(source.as_bytes(), 0, source.len());
        assert!(!state.is_active(), "a quote inside a trailing comment must not open a literal");
    }

    #[test]
    fn scan_segment_defers_a_literal_body_that_starts_past_the_segment_end() {
        // `q{` is split across the segment boundary: the operator is consumed on
        // this segment but the body starts on the next one.
        let source = "my $x = q{\nbody}\n";
        let first_line_end = source.len() - "body}\n".len();
        let mut state = LiteralScanState::default();
        state.scan_segment(source.as_bytes(), 0, first_line_end);
        assert!(state.is_active(), "the quote-like literal stays open across the newline");
        state.scan_segment(source.as_bytes(), first_line_end, source.len());
        assert!(!state.is_active(), "the literal closes once its body is scanned");
    }

    // ---- ActiveLiteral::advance --------------------------------------------

    fn advance_all(literal: &mut ActiveLiteral, text: &str) -> Option<usize> {
        let mut escaped = false;
        for (index, byte) in text.bytes().enumerate() {
            if literal.advance(byte, &mut escaped) {
                return Some(index);
            }
        }
        None
    }

    #[test]
    fn active_literal_closes_only_after_balanced_nested_delimiters() {
        let mut literal = ActiveLiteral::new(QuoteLikeLiteral {
            opener: b'{',
            closer: b'}',
            sections: 1,
            consumed: 2,
            kind: QuoteLikeLiteralKind::String,
        });
        assert_eq!(
            advance_all(&mut literal, "a{b}c}rest"),
            Some("a{b}c".len()),
            "the nested open brace must be balanced before the literal closes"
        );
    }

    #[test]
    fn active_literal_ignores_an_escaped_closer() {
        let mut literal = ActiveLiteral::new(QuoteLikeLiteral {
            opener: b'/',
            closer: b'/',
            sections: 1,
            consumed: 1,
            kind: QuoteLikeLiteralKind::Regex,
        });
        assert_eq!(
            advance_all(&mut literal, "a\\/b/rest"),
            Some("a\\/b".len()),
            "an escaped closer must not end the literal"
        );
    }

    #[test]
    fn active_literal_requires_both_sections_of_a_same_delimiter_substitution() {
        let mut literal = ActiveLiteral::new(QuoteLikeLiteral {
            opener: b'/',
            closer: b'/',
            sections: 2,
            consumed: 1,
            kind: QuoteLikeLiteralKind::Regex,
        });
        assert_eq!(
            advance_all(&mut literal, "pat/rep/rest"),
            Some("pat/rep".len()),
            "s/// closes only after the replacement section"
        );
    }

    #[test]
    fn active_literal_accepts_a_rebracketed_second_section() {
        let mut literal = ActiveLiteral::new(QuoteLikeLiteral {
            opener: b'{',
            closer: b'}',
            sections: 2,
            consumed: 2,
            kind: QuoteLikeLiteralKind::Regex,
        });
        assert_eq!(
            advance_all(&mut literal, "pat} [rep]rest"),
            Some("pat} [rep]".len() - 1),
            "s{{}}[] must pick up the second section's own delimiter pair"
        );
    }

    #[test]
    fn active_literal_terminates_on_a_semicolon_while_awaiting_a_second_section() {
        let mut literal = ActiveLiteral::new(QuoteLikeLiteral {
            opener: b'{',
            closer: b'}',
            sections: 2,
            consumed: 2,
            kind: QuoteLikeLiteralKind::Regex,
        });
        assert_eq!(
            advance_all(&mut literal, "pat};"),
            Some("pat}".len()),
            "a semicolon ends recovery instead of swallowing the rest of the file"
        );
    }

    // ---- quote_like_literal_start ------------------------------------------

    fn literal_start(source: &str, needle: &str) -> Option<QuoteLikeLiteral> {
        let index = source.find(needle)?;
        quote_like_literal_start(source.as_bytes(), index)
    }

    #[test]
    fn quote_like_operators_are_recognized_with_their_section_counts() {
        for (source, needle, sections) in [
            ("my $x = q{body};", "q{", 1usize),
            ("my $x = qq{body};", "qq{", 1),
            ("my $x = qw(a b);", "qw(", 1),
            ("my $x = qx{cmd};", "qx{", 1),
            ("my $x = qr{pat};", "qr{", 1),
            ("my $x = m{pat};", "m{", 1),
            ("$x =~ s{pat}{rep};", "s{", 2),
            ("$x =~ y{a}{b};", "y{", 2),
            ("$x =~ tr{a}{b};", "tr{", 2),
        ] {
            let found = literal_start(source, needle);
            assert!(found.is_some(), "{needle} must open a quote-like literal in {source:?}");
            assert_eq!(
                found.map(|literal| literal.sections),
                Some(sections),
                "{needle} must declare {sections} section(s)"
            );
        }
    }

    #[test]
    fn quote_like_literal_start_reports_the_delimiter_pair_and_consumed_width() {
        let found = literal_start("my $x = qq  [body];", "qq");
        assert_eq!(
            found.map(|literal| (literal.opener, literal.closer)),
            Some((b'[', b']')),
            "the bracket pair must be resolved from the delimiter"
        );
        assert_eq!(
            found.map(|literal| literal.consumed),
            Some("qq  [".len()),
            "consumed must span the operator, the skipped spaces, and the opener"
        );
    }

    #[test]
    fn sub_named_like_a_quote_operator_is_not_a_literal() {
        assert!(
            literal_start("sub q { 1 }", "q ").is_none(),
            "a sub declaration named q is not a quote-like literal"
        );
    }

    #[test]
    fn method_and_qualified_names_are_not_quote_operators() {
        assert!(
            literal_start("$object->s(1);", "s(").is_none(),
            "a method call named s is not a substitution"
        );
        assert!(
            literal_start("Some::Package::q(1);", "q(").is_none(),
            "a qualified function named q is not a quote-like literal"
        );
    }

    #[test]
    fn file_test_s_operator_is_not_a_substitution() {
        assert!(
            literal_start("if (-s $file) { 1 }", "s ").is_none(),
            "-s is the file-size test, not s///"
        );
        assert_eq!(
            literal_start("my $x = $n-s{a}{b};", "s{").map(|literal| literal.sections),
            Some(2),
            "a minus glued to an identifier is subtraction, so s/// still applies"
        );
    }

    #[test]
    fn braced_bareword_key_is_not_a_quote_operator() {
        assert!(
            literal_start("my $v = $hash{q};", "q}").is_none(),
            "{{q}} is a bareword hash key, not a quote-like literal"
        );
    }

    #[test]
    fn fat_comma_after_a_quote_operator_is_a_bareword() {
        assert!(
            literal_start("my %h = (q => 1);", "q ").is_none(),
            "q => is a bareword key, not a quote-like literal"
        );
    }

    #[test]
    fn quote_operator_needs_a_word_boundary_before_it() {
        assert!(
            literal_start("my $xq{a};", "q{").is_none(),
            "q glued to an identifier is part of that identifier"
        );
        assert!(
            literal_start("$q{a};", "q{").is_none(),
            "a sigil before q makes it a variable name, not an operator"
        );
    }

    #[test]
    fn quote_like_operator_parameters_maps_each_operator_form() {
        assert_eq!(
            quote_like_operator_parameters(b'q', Some(b'r')),
            Some((2, 1, true, QuoteLikeLiteralKind::Regex)),
            "qr is a one-section regex with a two-byte operator"
        );
        assert_eq!(
            quote_like_operator_parameters(b'q', Some(b'w')),
            Some((2, 1, true, QuoteLikeLiteralKind::String)),
            "qw is a one-section string list"
        );
        assert_eq!(
            quote_like_operator_parameters(b't', Some(b'r')),
            Some((2, 2, true, QuoteLikeLiteralKind::Regex)),
            "tr is a two-section transliteration"
        );
        assert_eq!(
            quote_like_operator_parameters(b'q', None),
            Some((1, 1, true, QuoteLikeLiteralKind::String)),
            "bare q is a one-section string"
        );
        assert_eq!(
            quote_like_operator_parameters(b'm', None),
            Some((1, 1, true, QuoteLikeLiteralKind::Regex)),
            "m is a one-section regex"
        );
        assert_eq!(
            quote_like_operator_parameters(b's', None),
            Some((1, 2, true, QuoteLikeLiteralKind::Regex)),
            "s is a two-section regex"
        );
        assert_eq!(
            quote_like_operator_parameters(b't', Some(b'x')),
            None,
            "t is only an operator as part of tr"
        );
        assert_eq!(quote_like_operator_parameters(b'z', None), None, "z is not a quote operator");
    }

    #[test]
    fn quote_like_is_file_test_s_operator_requires_a_standalone_minus() {
        assert!(
            quote_like_is_file_test_s_operator(b"if (-s $file)", 5),
            "-s after an open paren is the file test"
        );
        assert!(
            quote_like_is_file_test_s_operator(b"-  s $file", 3),
            "spaces between - and s are allowed"
        );
        assert!(
            !quote_like_is_file_test_s_operator(b"$n-s{a}{b}", 3),
            "a minus glued to an identifier is subtraction"
        );
        assert!(
            !quote_like_is_file_test_s_operator(b"m{a}", 0),
            "a non-s byte is never the file test"
        );
    }

    #[test]
    fn quote_like_is_braced_bareword_key_requires_a_closing_brace() {
        assert!(
            quote_like_is_braced_bareword_key(b"$h{q}", 3, 4),
            "{{q}} with a closing brace is a bareword key"
        );
        assert!(
            !quote_like_is_braced_bareword_key(b"$h{q{a}}", 3, 4),
            "{{q{{...}}}} opens a real literal"
        );
        assert!(
            !quote_like_is_braced_bareword_key(b"my q{a}", 3, 4),
            "without a preceding brace there is no bareword key"
        );
    }

    #[test]
    fn quote_like_follows_sub_declaration_matches_only_the_sub_keyword() {
        assert!(quote_like_follows_sub_declaration(b"sub q {}", 4), "sub q is a declaration");
        assert!(quote_like_follows_sub_declaration(b"sub   q {}", 6), "spaces after sub are fine");
        assert!(!quote_like_follows_sub_declaration(b"subq {}", 4), "subq is one identifier");
        assert!(!quote_like_follows_sub_declaration(b"my q{a}", 3), "my is not sub");
        assert!(!quote_like_follows_sub_declaration(b"q{a}", 0), "nothing precedes the operator");
    }

    #[test]
    fn quote_like_follows_method_or_qualified_name_matches_arrow_and_colons() {
        assert!(
            quote_like_follows_method_or_qualified_name(b"$o->s(1)", 4),
            "-> precedes a method"
        );
        assert!(
            quote_like_follows_method_or_qualified_name(b"Pkg::q(1)", 5),
            ":: precedes a qualified name"
        );
        assert!(
            !quote_like_follows_method_or_qualified_name(b"$o = s(1)", 5),
            "= is not a method arrow"
        );
        assert!(!quote_like_follows_method_or_qualified_name(b"s(1)", 0), "nothing precedes it");
    }

    // ---- slash regex literals ----------------------------------------------

    #[test]
    fn slash_regex_literal_start_accepts_binding_and_bare_positions() {
        assert_eq!(
            slash_regex_literal_start(b"$x =~ /pat/", 6).map(|literal| literal.sections),
            Some(1),
            "a slash after =~ opens a regex"
        );
        assert_eq!(
            slash_regex_literal_start(b"$x !~ /pat/", 6).map(|literal| literal.sections),
            Some(1),
            "a slash after !~ opens a regex"
        );
        assert!(
            slash_regex_literal_start(b"my $x = $a / $b", 11).is_none(),
            "a slash after an identifier is division"
        );
        assert!(
            slash_regex_literal_start(b"$x =~ mpat/", 6).is_none(),
            "a non-slash byte never opens a slash regex"
        );
    }

    #[test]
    fn slash_follows_binding_operator_requires_the_two_byte_operator() {
        assert!(slash_follows_binding_operator(b"$x =~ /p/", 6), "=~ binds a regex");
        assert!(slash_follows_binding_operator(b"$x !~ /p/", 6), "!~ binds a regex");
        assert!(!slash_follows_binding_operator(b"$x ~ /p/", 5), "a bare ~ does not bind");
        assert!(!slash_follows_binding_operator(b"/p/", 0), "nothing precedes the slash");
    }

    #[test]
    fn slash_starts_bare_regex_literal_recognizes_operator_and_keyword_positions() {
        assert!(slash_starts_bare_regex_literal(b"/pat/", 0), "a leading slash starts a regex");
        assert!(slash_starts_bare_regex_literal(b"grep /pat/, @x", 5), "grep takes a bare regex");
        assert!(slash_starts_bare_regex_literal(b"split /,/, $x", 6), "split takes a bare regex");
        assert!(slash_starts_bare_regex_literal(b"if (/pat/)", 4), "an open paren allows a regex");
        assert!(slash_starts_bare_regex_literal(b"$x = /pat/", 5), "= allows a regex");
        assert!(
            !slash_starts_bare_regex_literal(b"$total / $count", 7),
            "an identifier means division"
        );
        assert!(!slash_starts_bare_regex_literal(b"foo() / 2", 6), "a close paren means division");
    }

    // ---- small helpers -----------------------------------------------------

    #[test]
    fn quote_like_operator_boundary_rejects_identifier_and_sigil_prefixes() {
        assert!(quote_like_operator_boundary(b"q{a}", 0), "index 0 is always a boundary");
        assert!(quote_like_operator_boundary(b"= q{a}", 2), "a space is a boundary");
        assert!(!quote_like_operator_boundary(b"xq{a}", 1), "a letter is not a boundary");
        assert!(!quote_like_operator_boundary(b"_q{a}", 1), "an underscore is not a boundary");
        for sigil in [b'$', b'@', b'%', b'&', b'*'] {
            let source = [sigil, b'q', b'{', b'a', b'}'];
            assert!(
                !quote_like_operator_boundary(&source, 1),
                "sigil {} must not be a boundary",
                char::from(sigil)
            );
        }
    }

    #[test]
    fn skip_ascii_space_advances_past_whitespace_only() {
        assert_eq!(skip_ascii_space(b"  x", 0), 2, "leading spaces are skipped");
        assert_eq!(skip_ascii_space(b"\t\n x", 0), 3, "tabs and newlines are skipped");
        assert_eq!(skip_ascii_space(b"x  ", 0), 0, "a non-space start does not advance");
        assert_eq!(skip_ascii_space(b"  ", 0), 2, "an all-space input stops at the end");
    }

    #[test]
    fn quote_like_closer_pairs_brackets_and_mirrors_punctuation() {
        assert_eq!(quote_like_closer(b'{'), Some(b'}'), "braces pair");
        assert_eq!(quote_like_closer(b'['), Some(b']'), "square brackets pair");
        assert_eq!(quote_like_closer(b'('), Some(b')'), "parens pair");
        assert_eq!(quote_like_closer(b'<'), Some(b'>'), "angle brackets pair");
        assert_eq!(quote_like_closer(b'/'), Some(b'/'), "a slash closes itself");
        assert_eq!(quote_like_closer(b'|'), Some(b'|'), "punctuation mirrors itself");
        assert_eq!(quote_like_closer(b'a'), None, "a letter is not a delimiter");
        assert_eq!(quote_like_closer(b' '), None, "a space is not a delimiter");
    }

    #[test]
    fn heredoc_opener_ignored_inside_comment() {
        // #5456: `<<` inside a line comment must not be treated as a heredoc
        // opener. Without the guard, `# see <<EOF docs` would swallow the rest
        // of the file as a heredoc body.
        assert_eq!(
            super::heredoc_opener_on_line("# see <<EOF docs"),
            None,
            "heredoc opener inside a comment must be ignored"
        );
        assert_eq!(
            super::heredoc_opener_on_line("my $x = 1; # heredoc: <<END"),
            None,
            "heredoc opener after code + comment must be ignored"
        );
    }

    #[test]
    fn heredoc_opener_still_found_in_code() {
        // Positive guard: a real heredoc opener in code (no comment) must work.
        assert_eq!(
            super::heredoc_opener_on_line("my $x = <<EOF;"),
            Some(("EOF".to_string(), false)),
            "real heredoc opener must still be found"
        );
    }

    #[test]
    fn heredoc_opener_with_hash_inside_string_not_treated_as_comment() {
        // A `#` inside a string literal is not a comment, so the `<<` after it
        // is still a valid heredoc opener.
        assert_eq!(
            super::heredoc_opener_on_line("my $m = '#'; print <<END;"),
            Some(("END".to_string(), false)),
            "heredoc opener after a quoted # must still be found"
        );
        assert_eq!(
            super::heredoc_opener_on_line("my $m = \"a\\\"# still quoted\"; print <<END;"),
            Some(("END".to_string(), false)),
            "an escaped quote must not end the string before its #"
        );
    }
}
