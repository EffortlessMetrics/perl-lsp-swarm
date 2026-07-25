//! Line-oriented literal/comment scanner lifted from completion `lexical_context`.
//!
//! Emits line-comment regions with quote/heredoc awareness so `#` inside string
//! literals is not classified as comment.

use std::cmp::Ordering;

use crate::syntax::text_line::is_identifier_byte;

use super::super::kind::SourceRegionKind;
use super::super::region::SourceRegion;

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
        push_region(
            &mut regions,
            source.len().saturating_sub(1),
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
            let closes = if allow_indented {
                line.trim_start_matches([' ', '\t']) == label
            } else {
                line == label
            };
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
            let end =
                after.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(after.len());
            after[..end].to_string()
        }
        _ if first.is_ascii_alphanumeric() || first == '_' => {
            let end =
                rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
            rest[..end].to_string()
        }
        _ => return None,
    };
    if label.is_empty() {
        None
    } else {
        Some((label, allow_indented))
    }
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
