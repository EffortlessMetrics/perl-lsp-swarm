use perl_parser_core::syntax::text_line::is_identifier_byte;
use std::cmp::Ordering;

/// Bounded local POD boundary for completion (#13241, HTTP-client scope).
///
/// Canonical broad boundary: any column-zero alphabetic `=command` directive
/// enters POD, and only an exact column-zero `=cut` returns the source to Perl
/// code. Like perl, `=cut` exits POD when followed by any non-word byte (or
/// end of line), so `=cut;` resumes code while `=cutlery` stays POD. `=end`
/// and the blank line ending a `=for` paragraph close an inner POD construct
/// only; they never resume executable code, and unknown alphabetic commands
/// stay opaque POD.
///
/// #13244 owns migrating completion onto the generation-bound canonical
/// `SourceRegionIndex` and deleting this local bridge.
#[derive(Default)]
enum PodState {
    #[default]
    Code,
    Pod,
}

fn advance_pod_state(state: &mut PodState, line: &str) -> bool {
    match state {
        PodState::Code => {
            if pod_directive(line).is_some() {
                *state = PodState::Pod;
                true
            } else {
                false
            }
        }
        PodState::Pod => {
            if is_pod_end_marker(line) {
                *state = PodState::Code;
            }
            true
        }
    }
}

/// Simple heuristic to check if position is in a string.
pub(super) fn is_in_string(source: &str, position: usize) -> bool {
    if invalid_string_position(source, position) {
        return false;
    }

    let mut active_delimiters: std::collections::VecDeque<HeredocDelimiter> =
        std::collections::VecDeque::new();
    let mut literal_state = LiteralScanState::default();
    let mut pod_state = PodState::default();
    let mut line_start = 0usize;

    for raw_line in source.split_inclusive('\n') {
        let line_end = line_start + raw_line.len();
        let line = strip_line_ending(raw_line);

        if let Some(delimiter) = active_delimiters.front() {
            if delimiter.matches_close(line) {
                if position_within_line(position, line_start, line_end) {
                    return false;
                }
                active_delimiters.pop_front();
            } else if position_within_line(position, line_start, line_end) {
                return false;
            }
            line_start = line_end;
            continue;
        }

        if !matches!(pod_state, PodState::Code) {
            if position_within_line(position, line_start, line_end) {
                return false;
            }
            advance_pod_state(&mut pod_state, line);
            line_start = line_end;
            continue;
        }

        if !literal_state.is_active() && advance_pod_state(&mut pod_state, line) {
            if position_within_line(position, line_start, line_end) {
                return false;
            }
            line_start = line_end;
            continue;
        }

        if position_within_line(position, line_start, line_end) {
            literal_state.scan_segment(source.as_bytes(), line_start, position);
            return literal_state.in_single_quote
                || literal_state.in_double_quote
                || literal_state.in_backtick
                || literal_state.literal.as_ref().is_some_and(|literal| {
                    // The replacement section of `s///`/`tr///` is string-like:
                    // it stays completion-eligible but must not read as
                    // executable constructor evidence.
                    literal.is_string_like() || !literal.in_pattern_section()
                });
        }

        let started_in_literal = literal_state.is_active();
        let resumed_code_index =
            literal_state.scan_segment(source.as_bytes(), line_start, line_end);
        if started_in_literal {
            if let Some(resumed_code_index) = resumed_code_index {
                active_delimiters.extend(extract_heredoc_delimiters_from_source_line(
                    source,
                    line,
                    line_end,
                    resumed_code_index - line_start,
                ));
            }
        } else {
            active_delimiters
                .extend(extract_heredoc_delimiters_from_source_line(source, line, line_end, 0));
        }

        line_start = line_end;
    }

    false
}

fn invalid_string_position(source: &str, position: usize) -> bool {
    position > source.len() || !source.is_char_boundary(position)
}

fn position_within_line(position: usize, line_start: usize, line_end: usize) -> bool {
    position >= line_start && position <= line_end
}

/// Heuristic to check if position is inside a regex literal.
///
/// Only the pattern section counts: the replacement section of `s///` and
/// `tr///` is string-like, so variable completion stays available there.
pub(super) fn is_in_regex(source: &str, position: usize) -> bool {
    if invalid_string_position(source, position) {
        return false;
    }

    let mut literal_state = LiteralScanState::default();
    scan_prefix_line_by_line(&mut literal_state, source, position);
    literal_state.literal.as_ref().is_some_and(|literal| {
        literal.kind == QuoteLikeLiteralKind::Regex && literal.in_pattern_section()
    })
}

/// Advance `state` over the source prefix `[0, position)` one line at a time.
///
/// [`LiteralScanState::scan_segment`] suspends at the first unquoted `#`, so
/// feeding it an entire multi-line prefix would permanently stop literal
/// tracking at the first line comment. Comment state ends with its newline,
/// so per-line segments resume literal detection on the following line.
///
/// The walk shares the heredoc/POD boundaries of the other lexical scanners:
/// heredoc bodies (and their closing lines) and POD lines never enter or
/// advance literal state, so regex-like text inside a non-code region cannot
/// leak literal state into later executable code.
fn scan_prefix_line_by_line(state: &mut LiteralScanState, source: &str, position: usize) {
    let prefix_end = position.min(source.len());
    let bytes = source.as_bytes();
    let mut pod_state = PodState::default();
    let mut active_delimiters: std::collections::VecDeque<HeredocDelimiter> =
        std::collections::VecDeque::new();
    let mut line_start = 0usize;

    while line_start < prefix_end {
        let line_end = bytes[line_start..prefix_end]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(prefix_end, |offset| line_start + offset + 1);
        let line = source.get(line_start..line_end).map(strip_line_ending).unwrap_or_default();

        if let Some(delimiter) = active_delimiters.front() {
            if delimiter.matches_close(line) {
                active_delimiters.pop_front();
            }
            line_start = line_end;
            continue;
        }

        if !matches!(pod_state, PodState::Code) {
            advance_pod_state(&mut pod_state, line);
            line_start = line_end;
            continue;
        }

        let started_in_literal = state.is_active();
        if !started_in_literal && advance_pod_state(&mut pod_state, line) {
            line_start = line_end;
            continue;
        }

        let resumed_code_index = state.scan_segment(bytes, line_start, line_end.min(prefix_end));
        if line_end <= prefix_end {
            if started_in_literal {
                if let Some(resumed_code_index) = resumed_code_index {
                    active_delimiters.extend(extract_heredoc_delimiters_from_source_line(
                        source,
                        line,
                        line_end,
                        resumed_code_index - line_start,
                    ));
                }
            } else {
                active_delimiters
                    .extend(extract_heredoc_delimiters_from_source_line(source, line, line_end, 0));
            }
        }

        line_start = line_end;
    }
}

/// True when `position` sits strictly inside an open regex pattern body.
///
/// Unlike [`is_in_regex`], this excludes the opening-delimiter position itself
/// (`m/|` is a pattern position, while probing at `m|/` is not yet inside the
/// literal body), so regex flag detection does not hijack pattern completion.
fn is_inside_entered_regex_body(source: &str, position: usize) -> bool {
    if invalid_string_position(source, position) {
        return false;
    }

    let mut literal_state = LiteralScanState::default();
    scan_prefix_line_by_line(&mut literal_state, source, position);
    let Some(literal) = literal_state.literal.as_ref() else {
        return false;
    };
    literal.kind == QuoteLikeLiteralKind::Regex
        && literal.in_pattern_section()
        && literal_state.current_literal_body_start.is_some_and(|body_start| body_start < position)
}

/// Return true when the cursor is positioned in the flag region after a closing regex delimiter.
pub(crate) fn is_in_regex_flags(source: &str, position: usize) -> bool {
    if position == 0 || position > source.len() {
        return false;
    }
    let before = &source[..position];
    let flag_chars: &[char] = &['g', 'i', 'm', 's', 'x', 'e', 'r', 'a', 'd', 'u', 'p', 'l', 'c'];
    let without_flags = before.trim_end_matches(|c: char| flag_chars.contains(&c));
    let close_pos = without_flags.len();
    if close_pos >= 2
        && without_flags.ends_with('/')
        && is_inside_entered_regex_body(source, close_pos - 1)
    {
        return true;
    }

    let body = without_flags.trim();
    is_multi_delim_regex_at_close(body)
}

fn is_multi_delim_regex_at_close(text: &str) -> bool {
    let (op_len, required_slashes) = if text.starts_with("tr/") || text.starts_with("y/") {
        let op = if text.starts_with("tr/") { 2 } else { 1 };
        (op, 3usize)
    } else if text.starts_with("s/") {
        (1, 3usize)
    } else if text.starts_with("m") || text.starts_with("qr") {
        return is_m_or_qr_closed(text);
    } else {
        let stripped = text
            .find("=~")
            .map(|p| text[p + 2..].trim_start())
            .or_else(|| text.find("!~").map(|p| text[p + 2..].trim_start()));
        if let Some(rhs) = stripped {
            return is_multi_delim_regex_at_close(rhs);
        }
        return false;
    };
    let body_after_op = &text[op_len..];
    let slash_count = count_unescaped_slashes(body_after_op);
    slash_count == required_slashes
}

fn is_m_or_qr_closed(text: &str) -> bool {
    let (op_len, delim_pos) =
        if text.starts_with("qr") { (2usize, 2usize) } else { (1usize, 1usize) };
    let Some(delim) = text[delim_pos..].chars().next() else {
        return false;
    };
    if delim.is_ascii_alphanumeric() || delim.is_ascii_whitespace() {
        return false;
    }
    let close_delim = matching_close_delimiter(delim);
    let body = &text[(op_len + delim.len_utf8())..];
    body.ends_with(close_delim)
}

fn matching_close_delimiter(open: char) -> char {
    match open {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        _ => open,
    }
}

fn count_unescaped_slashes(s: &str) -> usize {
    let mut count = 0usize;
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '/' {
            count += 1;
        }
    }
    count
}

/// Check if position is inside a line comment.
///
/// Scans the portion of the current line before `position` with a simple
/// quote-state machine so that a `#` inside a string literal is NOT treated as
/// a comment start (#4956 bug 4). Previously this was a naive `line.contains('#')`
/// check, which suppressed completion whenever a `#` appeared anywhere on the
/// line — including inside strings like `my $x = "a # b";`.
pub(super) fn is_in_comment(source: &str, position: usize) -> bool {
    let line_start = source[..position].rfind('\n').map_or(0, |p| p + 1);
    let line = &source[line_start..position];
    let bytes = line.as_bytes();
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

/// Check if position is inside a heredoc literal.
///
/// A heredoc starts with `<<DELIMITER` or `<<'DELIMITER'` or `<<"DELIMITER"` and
/// ends when a line contains only the delimiter (and optional trailing whitespace/newline).
pub(super) fn is_in_heredoc(source: &str, position: usize) -> bool {
    is_in_heredoc_with_boundary(source, position, false)
}

fn is_in_heredoc_with_boundary(source: &str, position: usize, include_closing_line: bool) -> bool {
    if position == 0 {
        return false;
    }

    let position = position.min(source.len());
    let mut active_delimiters: std::collections::VecDeque<HeredocDelimiter> =
        std::collections::VecDeque::new();
    let mut pod_state = PodState::default();
    let mut literal_state = LiteralScanState::default();
    let mut line_start = 0usize;

    for raw_line in source.split_inclusive('\n') {
        let line_end = line_start + raw_line.len();
        let line = strip_line_ending(raw_line);

        if let Some(delimiter) = active_delimiters.front() {
            if delimiter.matches_close(line) {
                if position >= line_start && position < line_end {
                    return include_closing_line;
                }
                active_delimiters.pop_front();
            } else if position >= line_start && position < line_end {
                return true;
            }
        } else {
            let started_in_literal = literal_state.is_active();
            if !started_in_literal && advance_pod_state(&mut pod_state, line) {
                if position >= line_start && position < line_end {
                    return false;
                }
                line_start = line_end;
                continue;
            }

            if position >= line_start && position < line_end {
                return false;
            }

            let resumed_code_index =
                literal_state.scan_segment(source.as_bytes(), line_start, line_end);
            if started_in_literal {
                if let Some(resumed_code_index) = resumed_code_index {
                    active_delimiters.extend(extract_heredoc_delimiters_from_source_line(
                        source,
                        line,
                        line_end,
                        resumed_code_index - line_start,
                    ));
                }
            } else {
                active_delimiters
                    .extend(extract_heredoc_delimiters_from_source_line(source, line, line_end, 0));
            }
        }

        line_start = line_end;
    }

    !active_delimiters.is_empty() && position >= line_start
}

fn strip_line_ending(line: &str) -> &str {
    let without_lf = line.strip_suffix('\n').unwrap_or(line);
    without_lf.strip_suffix('\r').unwrap_or(without_lf)
}

fn extract_heredoc_delimiters_from_code_line_from(
    line: &str,
    start_index: usize,
) -> Vec<HeredocDelimiter> {
    let bytes = line.as_bytes();
    let mut delimiters = Vec::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut literal: Option<ActiveLiteral> = None;
    let mut escaped = false;
    let mut index = start_index;

    while index + 1 < bytes.len() {
        let byte = bytes[index];

        if escaped {
            escaped = false;
            index += 1;
            continue;
        }

        if let Some(active_literal) = literal.as_mut() {
            if active_literal.advance(byte, &mut escaped) {
                literal = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'\\' if in_single_quote || in_double_quote || in_backtick => {
                escaped = true;
            }
            b'\'' if !in_double_quote && !in_backtick => {
                in_single_quote = !in_single_quote;
            }
            b'"' if !in_single_quote && !in_backtick => {
                in_double_quote = !in_double_quote;
            }
            b'`' if !in_single_quote && !in_double_quote => {
                in_backtick = !in_backtick;
            }
            b'#' if !in_single_quote && !in_double_quote && !in_backtick => {
                break;
            }
            _ if !in_single_quote && !in_double_quote && !in_backtick => {
                if let Some(literal_start) = quote_like_literal_start(bytes, index) {
                    let consumed = literal_start.consumed;
                    literal = Some(ActiveLiteral::new(literal_start));
                    index += consumed;
                    continue;
                }
                if let Some(literal_start) = slash_regex_literal_start(bytes, index) {
                    let consumed = literal_start.consumed;
                    literal = Some(ActiveLiteral::new(literal_start));
                    index += consumed;
                    continue;
                }

                if byte == b'<' && bytes[index + 1] == b'<' {
                    let after_marker = &line[index + 2..];
                    if is_heredoc_operator_context(line, index)
                        && let Some(mut delimiter) = extract_heredoc_delimiter(after_marker)
                    {
                        let no_space_bareword = no_space_bareword_before_marker(line, index);
                        let spaced_keyword_bareword =
                            spaced_keyword_bareword_before_marker(line, index);
                        let no_space_requires_future =
                            is_ambiguous_no_space_output_filehandle_context(line, index)
                                || no_space_bareword
                                    .is_some_and(is_ambiguous_no_space_bareword_word);
                        delimiter.requires_future_close =
                            no_space_requires_future || spaced_keyword_bareword.is_some();
                        delimiter.ignore_future_body_heredocs =
                            is_no_space_candidate_body_context(line, index);
                        delimiter.constant_probe_bareword =
                            no_space_bareword.or(spaced_keyword_bareword).map(str::to_string);
                        delimiters.push(delimiter);
                    }
                    index += 2;
                    continue;
                }
            }
            _ => {}
        }

        index += 1;
    }

    delimiters
}

fn extract_heredoc_delimiters_from_source_line(
    source: &str,
    line: &str,
    line_end: usize,
    start_index: usize,
) -> Vec<HeredocDelimiter> {
    extract_heredoc_delimiters_from_code_line_from(line, start_index)
        .into_iter()
        .filter(|delimiter| {
            !delimiter.requires_future_close
                || (!delimiter
                    .constant_probe_bareword
                    .as_deref()
                    .is_some_and(|word| has_constant_declaration_before(source, line_end, word))
                    && has_future_heredoc_close(source, line_end, delimiter))
        })
        .collect()
}

fn has_future_heredoc_close(source: &str, line_end: usize, delimiter: &HeredocDelimiter) -> bool {
    let Some(rest) = source.get(line_end..) else {
        return false;
    };

    let mut active_delimiters: std::collections::VecDeque<HeredocDelimiter> =
        std::collections::VecDeque::new();
    let mut literal_state = LiteralScanState::default();
    let mut pod_state = PodState::default();
    let mut line_start = line_end;

    for raw_line in rest.split_inclusive('\n') {
        let next_line_start = line_start + raw_line.len();
        let line = strip_line_ending(raw_line);

        if let Some(active_delimiter) = active_delimiters.front() {
            if active_delimiter.matches_close(line) {
                active_delimiters.pop_front();
            }
            line_start = next_line_start;
            continue;
        }

        let started_in_literal = literal_state.is_active();
        let ignored_pod_line = !started_in_literal && advance_pod_state(&mut pod_state, line);

        if !started_in_literal && !ignored_pod_line && delimiter.matches_close(line) {
            return true;
        }

        let resumed_code_index =
            literal_state.scan_segment(source.as_bytes(), line_start, next_line_start);
        if !ignored_pod_line && !delimiter.ignore_future_body_heredocs {
            if started_in_literal {
                if let Some(resumed_code_index) = resumed_code_index {
                    active_delimiters.extend(extract_heredoc_delimiters_from_source_line(
                        source,
                        line,
                        next_line_start,
                        resumed_code_index - line_start,
                    ));
                }
            } else {
                active_delimiters.extend(extract_heredoc_delimiters_from_source_line(
                    source,
                    line,
                    next_line_start,
                    0,
                ));
            }
        }
        line_start = next_line_start;
    }

    false
}

fn is_heredoc_operator_context(line: &str, marker_index: usize) -> bool {
    let raw_before_marker = &line[..marker_index];
    let has_space_before_marker = raw_before_marker.ends_with([' ', '\t']);
    let before_marker = raw_before_marker.trim_end_matches([' ', '\t']);
    let Some(last_char) = before_marker.chars().next_back() else {
        return true;
    };

    if is_expression_prefix_char(last_char) {
        return true;
    }

    match last_char {
        '}' => is_braced_output_filehandle_context(before_marker),
        ')' | ']' | '\'' | '"' | '`' | '0'..='9' => false,
        'A'..='Z' | 'a'..='z' | '_' => {
            let word_start = ascii_word_start(before_marker);
            let before_word = &before_marker[..word_start];
            let word = &before_marker[word_start..];

            if before_word
                .chars()
                .next_back()
                .is_some_and(|ch| matches!(ch, '$' | '@' | '%' | '&' | '*'))
            {
                return is_output_filehandle_context(before_word);
            }

            if is_bareword_output_filehandle_context(before_word) {
                return true;
            }

            if before_word_has_method_arrow(before_word) {
                return false;
            }

            if has_space_before_marker && is_bareword_shift_operand_context(before_word) {
                return false;
            }

            !word.is_empty()
        }
        _ => true,
    }
}

fn before_word_has_method_arrow(before_word: &str) -> bool {
    before_word.trim_end_matches([' ', '\t']).ends_with("->")
}

fn is_ambiguous_no_space_output_filehandle_context(line: &str, marker_index: usize) -> bool {
    let raw_before_marker = &line[..marker_index];
    if raw_before_marker.ends_with([' ', '\t']) {
        return false;
    }

    let before_marker = raw_before_marker.trim_end_matches([' ', '\t']);
    let word_start = before_marker
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    let before_word = &before_marker[..word_start];
    let word = &before_marker[word_start..];

    is_bareword_output_filehandle_context(before_word) && !word.is_empty()
}

fn is_ambiguous_no_space_bareword_context(line: &str, marker_index: usize) -> bool {
    no_space_bareword_before_marker(line, marker_index)
        .is_some_and(is_ambiguous_no_space_bareword_word)
}

fn no_space_bareword_before_marker(line: &str, marker_index: usize) -> Option<&str> {
    let raw_before_marker = &line[..marker_index];
    if raw_before_marker.ends_with([' ', '\t']) {
        return None;
    }

    let before_marker = raw_before_marker.trim_end_matches([' ', '\t']);
    let word_start = ascii_word_start(before_marker);
    let word = &before_marker[word_start..];

    (!word.is_empty()).then_some(word)
}

fn is_ambiguous_no_space_bareword_word(word: &str) -> bool {
    !is_no_space_heredoc_keyword(word)
}

fn is_no_space_candidate_body_context(line: &str, marker_index: usize) -> bool {
    if is_ambiguous_no_space_output_filehandle_context(line, marker_index) {
        return true;
    }
    if !is_ambiguous_no_space_bareword_context(line, marker_index) {
        return false;
    }

    let raw_before_marker = &line[..marker_index];
    let before_marker = raw_before_marker.trim_end_matches([' ', '\t']);
    let word_start = before_marker
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && *ch != '_')
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    let before_word = before_marker[..word_start].trim_end_matches([' ', '\t']);

    before_word.is_empty()
        || before_word.chars().next_back().is_some_and(|ch| matches!(ch, ';' | '{' | '}'))
}

fn spaced_keyword_bareword_before_marker(line: &str, marker_index: usize) -> Option<&str> {
    let raw_before_marker = &line[..marker_index];
    if !raw_before_marker.ends_with([' ', '\t']) {
        return None;
    }

    let before_marker = raw_before_marker.trim_end_matches([' ', '\t']);
    let word_start = ascii_word_start(before_marker);
    let before_word = before_marker[..word_start].trim_end_matches([' ', '\t']);
    let word = &before_marker[word_start..];

    if !word.is_empty()
        && matches!(
            before_word.split_ascii_whitespace().next_back(),
            Some("return" | "if" | "unless" | "while" | "until" | "for" | "foreach")
        )
    {
        Some(word)
    } else {
        None
    }
}

fn is_braced_output_filehandle_context(before_marker: &str) -> bool {
    let Some(open_brace_index) = before_marker.rfind('{') else {
        return false;
    };
    let prefix = before_marker[..open_brace_index].trim_end_matches([' ', '\t']);
    is_output_filehandle_keyword(prefix.split_ascii_whitespace().next_back())
}

fn is_output_filehandle_context(before_word: &str) -> bool {
    let prefix = before_word.trim_end_matches([' ', '\t']);
    let Some((sigil_index, _)) = prefix
        .char_indices()
        .next_back()
        .filter(|(_, ch)| matches!(ch, '$' | '@' | '%' | '&' | '*'))
    else {
        return false;
    };
    let prefix = &prefix[..sigil_index];
    is_output_filehandle_keyword(prefix.split_ascii_whitespace().next_back())
}

fn is_output_filehandle_keyword(keyword: Option<&str>) -> bool {
    matches!(keyword, Some("print" | "say" | "printf"))
}

fn is_bareword_output_filehandle_context(before_word: &str) -> bool {
    is_output_filehandle_keyword(before_word.split_ascii_whitespace().next_back())
}

fn is_no_space_heredoc_keyword(word: &str) -> bool {
    matches!(word, "print" | "say" | "warn" | "die")
}

fn is_bareword_shift_operand_context(before_word: &str) -> bool {
    let prefix = before_word.trim_end_matches([' ', '\t']);
    if prefix.is_empty() || prefix.split_ascii_whitespace().next_back() == Some("print") {
        return false;
    }

    prefix.chars().next_back().is_some_and(is_expression_prefix_char)
}

fn is_expression_prefix_char(ch: char) -> bool {
    matches!(
        ch,
        '=' | '('
            | '['
            | '{'
            | ','
            | ';'
            | ':'
            | '?'
            | '!'
            | '~'
            | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '&'
            | '|'
            | '^'
    )
}

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

struct ActiveLiteral {
    opener: u8,
    closer: u8,
    /// Total delimiter-separated sections the literal was opened with
    /// (`1` for `m//`/`qr//`, `2` for `s///` and `tr///`).
    sections: usize,
    sections_remaining: usize,
    depth: usize,
    awaiting_section_opener: bool,
    kind: QuoteLikeLiteralKind,
}

impl ActiveLiteral {
    fn new(literal: QuoteLikeLiteral) -> Self {
        Self {
            opener: literal.opener,
            closer: literal.closer,
            sections: literal.sections,
            sections_remaining: literal.sections,
            depth: 1,
            awaiting_section_opener: false,
            kind: literal.kind,
        }
    }

    /// True while the scan is inside the pattern (first) section. The
    /// replacement section of `s///`/`tr///` is string-like, not regex.
    fn in_pattern_section(&self) -> bool {
        self.sections_remaining == self.sections
    }

    fn is_string_like(&self) -> bool {
        self.kind == QuoteLikeLiteralKind::String
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

#[derive(Default)]
struct LiteralScanState {
    in_single_quote: bool,
    in_double_quote: bool,
    in_backtick: bool,
    literal: Option<ActiveLiteral>,
    pending_literal_body_start: Option<usize>,
    /// Source offset where the currently open literal's body begins, used to
    /// distinguish "at the opening delimiter" from "inside the body".
    current_literal_body_start: Option<usize>,
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
                    self.current_literal_body_start = None;
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
                        self.current_literal_body_start = Some(index + consumed);
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
                        self.current_literal_body_start = Some(index + consumed);
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

    if let 0 = before {
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

pub(super) fn ascii_word_start(text: &str) -> usize {
    text.as_bytes()
        .iter()
        .rposition(|byte| !is_identifier_byte(*byte))
        .map_or(0, |position| position + 1)
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

struct HeredocDelimiter {
    label: String,
    allow_indented_close: bool,
    requires_future_close: bool,
    ignore_future_body_heredocs: bool,
    constant_probe_bareword: Option<String>,
}

impl HeredocDelimiter {
    fn matches_close(&self, line: &str) -> bool {
        if self.allow_indented_close {
            line.trim_start_matches([' ', '\t']) == self.label
        } else {
            line == self.label
        }
    }
}

/// Extract the heredoc delimiter from text immediately after `<<`
fn extract_heredoc_delimiter(text: &str) -> Option<HeredocDelimiter> {
    let (text, has_leading_space) = strip_horizontal_space(text);
    let (text, allow_indented_close, has_label_leading_space) = if has_leading_space {
        (text, false, true)
    } else if let Some(rest) = text.strip_prefix('~') {
        let (rest, has_space_after_tilde) = strip_horizontal_space(rest);
        (rest, true, has_space_after_tilde)
    } else {
        (text, false, false)
    };
    let first_char = text.chars().next()?;

    let label = match first_char {
        // Quoted forms: <<'EOF', <<"EOF", <<`EOF`, etc.
        '\'' | '"' | '`' => parse_quoted_heredoc_label(text, first_char)?,
        // Non-interpolating bare form: <<\EOF
        '\\' if !has_label_leading_space => {
            let rest = &text[first_char.len_utf8()..];
            let first_rest_char = rest.chars().next()?;
            if !first_rest_char.is_ascii_alphanumeric() && first_rest_char != '_' {
                return None;
            }
            let end =
                rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
            rest[..end].to_string()
        }
        // Bare form: <<EOF
        _ if !has_label_leading_space
            && (first_char.is_ascii_alphanumeric() || first_char == '_') =>
        {
            let end =
                text.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(text.len());
            text[..end].to_string()
        }
        _ => return None,
    };

    Some(HeredocDelimiter {
        label,
        allow_indented_close,
        requires_future_close: false,
        ignore_future_body_heredocs: false,
        constant_probe_bareword: None,
    })
}

fn has_constant_declaration_before(source: &str, position: usize, name: &str) -> bool {
    source[..position.min(source.len())].lines().any(|line| line_declares_constant(line, name))
}

fn line_declares_constant(line: &str, name: &str) -> bool {
    let line = line.split_once('#').map_or(line, |(code, _)| code);
    let Some(rest) = line.trim_start().strip_prefix("use") else {
        return false;
    };
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let Some(rest) = rest.trim_start().strip_prefix("constant") else {
        return false;
    };
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();

    constant_statement_declares_name(rest, name)
}

fn constant_statement_declares_name(rest: &str, name: &str) -> bool {
    if let Some(map_body) =
        rest.strip_prefix('{').and_then(|after_open| after_open.split('}').next())
    {
        return map_body.split(',').any(|entry| constant_map_entry_declares_name(entry, name));
    }

    strip_constant_name_prefix(rest, name)
        .is_some_and(|after_name| after_name.chars().next().is_none_or(is_constant_name_boundary))
}

fn constant_map_entry_declares_name(entry: &str, name: &str) -> bool {
    strip_constant_name_prefix(entry.trim_start(), name).is_some_and(|after_name| {
        after_name
            .trim_start()
            .strip_prefix("=>")
            .is_some_and(|_| after_name.chars().next().is_none_or(is_constant_name_boundary))
    })
}

fn strip_constant_name_prefix<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    if let Some(after_name) = text.strip_prefix(name) {
        return Some(after_name);
    }

    for quote in ['"', '\''] {
        if let Some(quoted_name) = text
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_prefix(name))
            .and_then(|rest| rest.strip_prefix(quote))
        {
            return Some(quoted_name);
        }
    }

    None
}

fn is_constant_name_boundary(ch: char) -> bool {
    !ch.is_ascii_alphanumeric() && ch != '_'
}

fn parse_quoted_heredoc_label(text: &str, quote: char) -> Option<String> {
    let mut label = String::new();
    let mut escaped = false;

    for ch in text[quote.len_utf8()..].chars() {
        if escaped {
            if ch == quote || ch == '\\' {
                label.push(ch);
            } else {
                label.push('\\');
                label.push(ch);
            }
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(label);
        } else {
            label.push(ch);
        }
    }

    None
}

fn strip_horizontal_space(text: &str) -> (&str, bool) {
    let trimmed = text.trim_start_matches([' ', '\t']);
    (trimmed, trimmed.len() != text.len())
}

/// Check if position is inside a POD block.
///
/// POD blocks start with a POD directive like `=pod`, `=head1`, `=head2`, etc.
/// at the **beginning** of a line (column 0) and end with `=cut` at the beginning
/// of a line.  Per the perlpod spec, leading whitespace disqualifies a line from
/// being a POD command — `  =pod` is plain text, not a POD directive.
///
/// Lines that fall inside a heredoc are skipped so that heredoc bodies containing
/// `=pod`-like content cannot corrupt the POD state machine.
///
/// The scan is one incremental forward pass sharing the heredoc/literal state
/// machine with [`is_in_string`]: the pod decision for each line only needs the
/// heredoc and literal state at that line's start, so recomputing either state
/// from the source start per line (an O(lines × prefix) rescan) is avoided on
/// the per-keystroke completion path.
pub(super) fn is_in_pod(source: &str, position: usize) -> bool {
    if position == 0 {
        return false;
    }

    let prefix_end = position.min(source.len());
    let bytes = source.as_bytes();
    let mut state = PodState::default();
    let mut literal_state = LiteralScanState::default();
    let mut active_delimiters: std::collections::VecDeque<HeredocDelimiter> =
        std::collections::VecDeque::new();
    let mut line_start = 0usize;

    while line_start < prefix_end {
        let line_end = bytes[line_start..prefix_end]
            .iter()
            .position(|candidate| *candidate == b'\n')
            .map_or(prefix_end, |newline_offset| line_start + newline_offset + 1);
        let line = source.get(line_start..line_end).map(strip_line_ending).unwrap_or_default();

        if let Some(delimiter) = active_delimiters.front() {
            // Heredoc body and its closing line never advance POD state.
            if delimiter.matches_close(line) {
                active_delimiters.pop_front();
            }
            line_start = line_end;
            continue;
        }

        if !matches!(state, PodState::Code) {
            advance_pod_state(&mut state, line);
            line_start = line_end;
            continue;
        }

        let started_in_literal = literal_state.is_active();
        if !started_in_literal && advance_pod_state(&mut state, line) {
            line_start = line_end;
            continue;
        }

        let resumed_code_index =
            literal_state.scan_segment(bytes, line_start, line_end.min(prefix_end));
        if line_end <= prefix_end {
            if started_in_literal {
                if let Some(resumed_code_index) = resumed_code_index {
                    active_delimiters.extend(extract_heredoc_delimiters_from_source_line(
                        source,
                        line,
                        line_end,
                        resumed_code_index - line_start,
                    ));
                }
            } else {
                active_delimiters
                    .extend(extract_heredoc_delimiters_from_source_line(source, line, line_end, 0));
            }
        }

        line_start = line_end;
    }

    !matches!(state, PodState::Code)
}

fn is_pod_end_marker(line: &str) -> bool {
    // perl exits POD at a column-zero `=cut` followed by any non-word byte
    // (including none): `=cut;` and `=cut-lt` resume code, while `=cutlery`,
    // `=cut1`, and `=cut_lt` remain POD commands/paragraph text.
    let Some(after_cut) = line.strip_prefix("=cut") else {
        return false;
    };
    after_cut.bytes().next().is_none_or(|byte| !byte.is_ascii_alphanumeric() && byte != b'_')
}

fn pod_directive(line: &str) -> Option<&str> {
    let token = line.split_ascii_whitespace().next()?;
    (line.starts_with('=') && token.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic))
        .then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_quote_like_start(
        source: &[u8],
        index: usize,
        consumed: usize,
        sections: usize,
        kind: QuoteLikeLiteralKind,
    ) {
        let literal = quote_like_literal_start(source, index);
        assert!(literal.is_some(), "expected quote-like literal for {source:?} at {index}");

        if let Some(literal) = literal {
            assert_eq!(literal.consumed, consumed);
            assert_eq!(literal.sections, sections);
            assert_eq!(literal.kind, kind);
        }
    }

    #[test]
    fn future_close_probe_rejects_out_of_range_start() {
        let delimiter = HeredocDelimiter {
            label: "EOF".to_string(),
            allow_indented_close: false,
            requires_future_close: true,
            ignore_future_body_heredocs: false,
            constant_probe_bareword: None,
        };

        assert!(!has_future_heredoc_close("<<EOF", 99, &delimiter));
    }

    #[test]
    fn invalid_string_position_boundary_discriminator() {
        let source = "é";

        assert_eq!(invalid_string_position(source, 0), false);
        assert_eq!(invalid_string_position(source, source.len()), false);
        assert_eq!(invalid_string_position(source, 1), true);
        assert_eq!(invalid_string_position(source, source.len() + 1), true);
    }

    #[test]
    fn position_within_line_boundary_discriminator() {
        assert_eq!(position_within_line(9, 10, 15), false);
        assert_eq!(position_within_line(10, 10, 15), true);
        assert_eq!(position_within_line(12, 10, 15), true);
        assert_eq!(position_within_line(15, 10, 15), true);
        assert_eq!(position_within_line(16, 10, 15), false);
    }

    #[test]
    fn is_in_string_rejects_out_of_range_and_non_boundary_positions() {
        assert_eq!(is_in_string("abc", 3), false);
        assert_eq!(is_in_string("abc", 4), false);
        assert_eq!(is_in_string("\"é", 1), true);
        assert_eq!(is_in_string("\"é", 2), false);
        assert_eq!(is_in_string("\"é", 3), true);
        assert_eq!(is_in_string("\"é", 4), false);
        assert_eq!(is_in_string("\"a", 2), true);
    }

    #[test]
    fn is_in_string_respects_line_start_boundary() {
        let source = "my $x = 1;\n\"open\nstill_open";

        assert_eq!(is_in_string(source, 10), false);
        assert_eq!(is_in_string(source, 11), false);
        assert_eq!(is_in_string(source, 12), true);
        assert_eq!(is_in_string(source, 17), true);
        assert_eq!(is_in_string(source, 27), true);
    }

    #[test]
    fn is_in_string_tracks_quote_parity_and_escapes() {
        assert_eq!(is_in_string("\"", 1), true);
        assert_eq!(is_in_string("\"\"", 2), false);
        assert_eq!(is_in_string("'", 1), true);
        assert_eq!(is_in_string("''", 2), false);
        assert_eq!(is_in_string("my $x = 'open", 13), true);
        assert_eq!(is_in_string("my $x = 'closed'", 16), false);
        assert_eq!(is_in_string("my $x = \"open", 13), true);
        assert_eq!(is_in_string("my $x = \"closed\"", 16), false);
        assert_eq!(is_in_string("my $x = 'single' . \"double", 26), true);
        assert_eq!(is_in_string("`", 1), true);
    }

    #[test]
    fn literal_scan_quote_parity_boundary_discriminator() {
        let mut no_quote = LiteralScanState::default();
        assert_eq!(no_quote.scan_segment(b"", 0, 0), None);
        assert_eq!(no_quote.is_active(), false);

        let mut one_single_quote = LiteralScanState::default();
        assert_eq!(one_single_quote.scan_segment(b"'", 0, 1), None);
        assert_eq!(one_single_quote.in_single_quote, true);
        assert_eq!(one_single_quote.in_double_quote, false);
        assert_eq!(one_single_quote.is_active(), true);

        let mut two_single_quotes = LiteralScanState::default();
        assert_eq!(two_single_quotes.scan_segment(b"''", 0, 2), None);
        assert_eq!(two_single_quotes.in_single_quote, false);
        assert_eq!(two_single_quotes.in_double_quote, false);
        assert_eq!(two_single_quotes.is_active(), false);

        let mut one_double_quote = LiteralScanState::default();
        assert_eq!(one_double_quote.scan_segment(br#"""#, 0, 1), None);
        assert_eq!(one_double_quote.in_single_quote, false);
        assert_eq!(one_double_quote.in_double_quote, true);
        assert_eq!(one_double_quote.is_active(), true);

        let mut two_double_quotes = LiteralScanState::default();
        assert_eq!(two_double_quotes.scan_segment(br#""""#, 0, 2), None);
        assert_eq!(two_double_quotes.in_single_quote, false);
        assert_eq!(two_double_quotes.in_double_quote, false);
        assert_eq!(two_double_quotes.is_active(), false);
    }

    #[test]
    fn is_in_string_tracks_quote_like_string_literals() {
        assert_eq!(is_in_string("my $text = qq{Hello $me", 23), true);
        assert_eq!(is_in_string("my $text = q($me", 16), true);
        assert_eq!(is_in_string("my $rx = qr{$me", 15), false);
    }

    #[test]
    fn is_in_string_does_not_treat_hash_keys_as_quote_like_literals() {
        let after_q_key = "$h{q}; my $name";
        let after_qq_key = "$h{qq}; my $name";
        let after_qw_key = "$h{qw}; my $name";
        let after_qx_key = "$h{qx}; my $name";
        let after_qr_key = "$h{qr}; my $name";
        let after_arrow_q_key = "$h->{ q }; my $name";
        let after_s_key = "$h{s}; my $name";
        let after_tr_key = "$h{tr}; my $name";
        let after_y_key = "$h{y}; my $name";
        let inside_string_after_m_key = "$h{m}; my $text = \"Hello $na";

        assert_eq!(is_in_string(after_q_key, after_q_key.len()), false);
        assert_eq!(is_in_string(after_qq_key, after_qq_key.len()), false);
        assert_eq!(is_in_string(after_qw_key, after_qw_key.len()), false);
        assert_eq!(is_in_string(after_qx_key, after_qx_key.len()), false);
        assert_eq!(is_in_string(after_qr_key, after_qr_key.len()), false);
        assert_eq!(is_in_string(after_arrow_q_key, after_arrow_q_key.len()), false);
        assert_eq!(is_in_string(after_s_key, after_s_key.len()), false);
        assert_eq!(is_in_string(after_tr_key, after_tr_key.len()), false);
        assert_eq!(is_in_string(after_y_key, after_y_key.len()), false);
        assert_eq!(is_in_string(inside_string_after_m_key, inside_string_after_m_key.len()), true);
    }

    #[test]
    fn is_in_string_skips_pod_q_like_text() {
        let source = "=pod\nq($cursor\n=cut\nmy $after = ";

        assert_eq!(is_in_string(source, 2), false);
        assert_eq!(is_in_string(source, source.len()), false);
    }

    #[test]
    fn indented_pod_directives_are_plain_code_text() {
        for directive in ["=begin comment", "=for comment", "=cut"] {
            let source = format!("  {directive}\nmy $after = ");
            assert!(!is_in_pod(&source, source.len()), "{directive} must be column-zero");
        }

        let source = "=pod\ndocumentation\n  =cut\nstill documentation";
        assert!(is_in_pod(&source, source.len()), "indented =cut must not close POD");
    }

    #[test]
    fn recognized_modern_pod_commands_start_pod() {
        for directive in ["=encoding utf8", "=head5 Deep", "=head6 Deeper"] {
            let source = format!("{directive}\ndocumentation\n$http->po");
            assert!(is_in_pod(&source, source.len()), "{directive} must start POD");
            assert!(!is_in_string(&source, source.len()));
            assert!(!is_in_heredoc(&source, source.len()));
        }
    }

    #[test]
    fn unmatched_end_does_not_cut_pod() {
        let source = "=pod\ndocumentation\n=end comment\nstill documentation\n$http->po";
        assert!(is_in_pod(source, source.len()));
    }

    #[test]
    fn begin_end_region_stays_pod_until_cut() {
        let source =
            "=begin comment\ndocumentation\n=end comment\n\n$http = HTTP::Tiny->new;\n$http->po";
        assert!(is_in_pod(source, source.len()), "=end must not resume code without =cut");

        let cut_source = "=begin comment\ndocs\n=end comment\n=cut\nmy $after = ";
        assert!(!is_in_pod(cut_source, cut_source.len()), "=cut must resume code");
    }

    #[test]
    fn for_paragraph_blank_line_stays_pod_until_cut() {
        let source = "use HTTP::Tiny;\n=for comment\ndocumentation\n\nmy $http = HTTP::Tiny->new;\n$http->po";
        assert!(
            is_in_pod(source, source.len()),
            "a =for paragraph's blank line must not resume code without =cut"
        );
        assert!(!is_in_string(source, source.len()));
        assert!(!is_in_heredoc(source, source.len()));

        let cut_source = "=for comment\ndocs\n\n=cut\nmy $after = ";
        assert!(!is_in_pod(cut_source, cut_source.len()), "=cut must resume code");
    }

    #[test]
    fn cutlery_and_unknown_commands_stay_pod() {
        let cutlery = "=pod\ndocs\n=cutlery\nstill documentation";
        assert!(is_in_pod(cutlery, cutlery.len()), "=cutlery is not =cut");

        let unknown = "=foobar custom\nmy $http = HTTP::Tiny->new;\n$http->po";
        assert!(
            is_in_pod(unknown, unknown.len()),
            "unknown column-zero alphabetic commands must stay opaque POD"
        );

        let eof_before_cut = "=for comment\ndocumentation\n";
        assert!(is_in_pod(eof_before_cut, eof_before_cut.len()));
    }

    #[test]
    fn is_in_string_handles_escaped_quote() {
        let source = r#"my $text = "Hello \" $name"#;

        assert!(is_in_string(source, source.len()));
    }

    #[test]
    fn is_in_string_skips_heredoc_body_and_closing_line_quotes() {
        let source = r#"my $text = <<EOF;
literal
EOF
my $after = "op"#;

        assert_eq!(is_in_string(source, 17), false);
        assert_eq!(is_in_string(source, 18), false);
        assert_eq!(is_in_string(source, 25), false);
        assert_eq!(is_in_string(source, 26), false);
        assert_eq!(is_in_string(source, 27), false);
        assert_eq!(is_in_string(source, 30), false);
        assert_eq!(is_in_string(source, 31), false);
        assert_eq!(is_in_string(source, 45), true);
    }

    #[test]
    fn future_close_probe_ignores_pod_blocks() {
        let delimiter = HeredocDelimiter {
            label: "bar".to_string(),
            allow_indented_close: false,
            requires_future_close: true,
            ignore_future_body_heredocs: false,
            constant_probe_bareword: None,
        };
        let source = "return foo <<bar;\n=pod\nbar\n=cut\n";
        let line_end = "return foo <<bar;\n".len();

        assert!(!has_future_heredoc_close(source, line_end, &delimiter));
    }

    #[test]
    fn heredoc_operator_context_accepts_start_of_line_marker() {
        assert!(is_heredoc_operator_context("<<EOF", 0));
    }

    #[test]
    fn is_heredoc_operator_context_boundary_discriminator() {
        assert_eq!(is_heredoc_operator_context("$obj->method <<EOF", 13), false);
    }

    #[test]
    fn before_word_has_method_arrow_boundary_discriminator() {
        assert_eq!(before_word_has_method_arrow("$obj->"), true);
        assert_eq!(before_word_has_method_arrow("$obj-> \t"), true);
        assert_eq!(before_word_has_method_arrow("$obj -"), false);
    }

    #[test]
    fn constant_declaration_probe_finds_scalar_and_map_constants() {
        let source = "use constant FOO => 2;\nuse constant \"QUOTED\" => 4;\nuse constant 'SINGLE' => 5;\nuse constant { BAR => 1, BAZ => 3, \"MAPQ\" => 6, 'MAPS' => 7 };\n";

        assert!(has_constant_declaration_before(source, source.len(), "FOO"));
        assert!(has_constant_declaration_before(source, source.len(), "QUOTED"));
        assert!(has_constant_declaration_before(source, source.len(), "SINGLE"));
        assert!(has_constant_declaration_before(source, source.len(), "BAR"));
        assert!(has_constant_declaration_before(source, source.len(), "BAZ"));
        assert!(has_constant_declaration_before(source, source.len(), "MAPQ"));
        assert!(has_constant_declaration_before(source, source.len(), "MAPS"));
        assert!(!has_constant_declaration_before(source, source.len(), "BA"));
    }

    #[test]
    fn constant_declaration_probe_rejects_prefix_lookalikes() {
        assert!(!line_declares_constant("useful constant FOO => 1", "FOO"));
        assert!(!line_declares_constant("use other FOO => 1", "FOO"));
        assert!(!line_declares_constant("use constantish FOO => 1", "FOO"));
    }

    #[test]
    fn heredoc_delimiter_parser_rejects_invalid_labels() {
        assert!(extract_heredoc_delimiter(" EOF").is_none());
        assert!(extract_heredoc_delimiter(r"\!EOF").is_none());
        assert!(parse_quoted_heredoc_label(r#""EOF"#, '"').is_none());
    }

    #[test]
    fn heredoc_delimiter_parser_accepts_underscore_labels() {
        let escaped = extract_heredoc_delimiter(r"\_EOF");
        assert_eq!(escaped.as_ref().map(|delimiter| delimiter.label.as_str()), Some("_EOF"));

        let escaped_embedded = extract_heredoc_delimiter(r"\EO_F;");
        assert_eq!(
            escaped_embedded.as_ref().map(|delimiter| delimiter.label.as_str()),
            Some("EO_F")
        );

        let bare = extract_heredoc_delimiter("_EOF");
        assert_eq!(bare.as_ref().map(|delimiter| delimiter.label.as_str()), Some("_EOF"));

        let bare_embedded = extract_heredoc_delimiter("EO_F;");
        assert_eq!(bare_embedded.as_ref().map(|delimiter| delimiter.label.as_str()), Some("EO_F"));
    }

    #[test]
    fn quoted_heredoc_label_preserves_quote_and_backslash_escapes() {
        assert_eq!(parse_quoted_heredoc_label(r#""EO\"F""#, '"'), Some("EO\"F".to_string()));
        assert_eq!(parse_quoted_heredoc_label(r#""EO\\F""#, '"'), Some(r"EO\F".to_string()));
    }

    #[test]
    fn future_close_probe_skips_nested_heredoc_bodies() {
        let delimiter = HeredocDelimiter {
            label: "bar".to_string(),
            allow_indented_close: false,
            requires_future_close: true,
            ignore_future_body_heredocs: false,
            constant_probe_bareword: None,
        };
        let source = "return foo <<bar;\nmy $h = <<EOF;\nbar\nEOF\nbar\n";
        let line_end = "return foo <<bar;\n".len();

        assert!(has_future_heredoc_close(source, line_end, &delimiter));
    }

    #[test]
    fn future_close_probe_resumes_after_multiline_literal() {
        let delimiter = HeredocDelimiter {
            label: "bar".to_string(),
            allow_indented_close: false,
            requires_future_close: true,
            ignore_future_body_heredocs: false,
            constant_probe_bareword: None,
        };
        let source = "return foo <<bar;\nmy $literal = '\ntext'; my $h = <<EOF;\nbar\nEOF\nbar\n";
        let line_end = "return foo <<bar;\n".len();

        assert!(has_future_heredoc_close(source, line_end, &delimiter));
    }

    #[test]
    fn heredoc_operator_context_accepts_braced_filehandle() {
        assert_eq!(is_heredoc_operator_context("print {$fh} <<EOF", 12), true);
        assert_eq!(is_heredoc_operator_context("my $value = {$fh} <<EOF", 18), false);
    }

    #[test]
    fn heredoc_operator_context_accepts_underscore_filehandle_word() {
        assert_eq!(is_heredoc_operator_context("print OUT_FH <<EOF", 13), true);
    }

    #[test]
    fn heredoc_operator_context_rejects_sigiled_underscore_term_outside_print() {
        assert_eq!(is_heredoc_operator_context("my $_ <<EOF", 6), false);
        assert_eq!(is_heredoc_operator_context("my $out_fh <<EOF", 11), false);
    }

    #[test]
    fn filehandle_context_helpers_reject_missing_shapes() {
        assert!(!is_braced_output_filehandle_context("print }"));
        assert!(!is_output_filehandle_context("print"));
    }

    #[test]
    fn active_literal_escape_consumes_next_byte() {
        let literal = QuoteLikeLiteral {
            opener: b'!',
            closer: b'!',
            sections: 1,
            consumed: 2,
            kind: QuoteLikeLiteralKind::String,
        };
        let mut active = ActiveLiteral::new(literal);
        let mut escaped = true;

        assert!(!active.advance(b'!', &mut escaped));
        assert!(!escaped);
    }

    #[test]
    fn pending_literal_body_past_segment_waits_for_next_scan() {
        let mut state = LiteralScanState {
            pending_literal_body_start: Some(10),
            ..LiteralScanState::default()
        };

        assert_eq!(state.scan_segment(b"q", 0, 1), None);
        assert_eq!(state.pending_literal_body_start, Some(10));
    }

    #[test]
    fn pending_literal_body_at_segment_start_resumes_scanning() {
        let mut state =
            LiteralScanState { pending_literal_body_start: Some(0), ..LiteralScanState::default() };

        assert_eq!(state.scan_segment(b"'", 0, 1), None);
        assert_eq!(state.pending_literal_body_start, None);
        assert!(state.in_single_quote);
    }

    #[test]
    fn pending_literal_body_at_segment_end_waits_for_next_scan() {
        let mut state =
            LiteralScanState { pending_literal_body_start: Some(1), ..LiteralScanState::default() };

        assert_eq!(state.scan_segment(b"q", 0, 1), None);
        assert_eq!(state.pending_literal_body_start, Some(1));
    }

    #[test]
    fn quote_like_literal_body_start_eq_segment_end_stays_active_without_pending() {
        let mut state = LiteralScanState::default();

        assert_eq!(state.scan_segment(b"q!", 0, 2), None);
        assert!(state.literal.is_some());
        assert_eq!(state.pending_literal_body_start, None);
    }

    fn assert_scan_starts_quote_like_literal(
        source: &[u8],
        kind: QuoteLikeLiteralKind,
        sections_remaining: usize,
        opener: u8,
        closer: u8,
    ) {
        let mut state = LiteralScanState::default();

        assert_eq!(state.scan_segment(source, 0, source.len()), None);
        assert_eq!(state.pending_literal_body_start, None);
        assert!(state.literal.is_some(), "expected active literal for {source:?}");
        if let Some(literal) = state.literal.as_ref() {
            assert_eq!(literal.kind, kind);
            assert_eq!(literal.sections_remaining, sections_remaining);
            assert_eq!(literal.opener, opener);
            assert_eq!(literal.closer, closer);
        }
    }

    #[test]
    fn literal_scan_quote_like_operator_boundary_discriminator() {
        assert_scan_starts_quote_like_literal(b"qr{", QuoteLikeLiteralKind::Regex, 1, b'{', b'}');
        assert_scan_starts_quote_like_literal(b"qq{", QuoteLikeLiteralKind::String, 1, b'{', b'}');
        assert_scan_starts_quote_like_literal(b"qw(", QuoteLikeLiteralKind::String, 1, b'(', b')');
        assert_scan_starts_quote_like_literal(b"qx/", QuoteLikeLiteralKind::String, 1, b'/', b'/');
        assert_scan_starts_quote_like_literal(b"tr/", QuoteLikeLiteralKind::Regex, 2, b'/', b'/');
    }

    #[test]
    fn quote_like_literal_start_discriminates_qr_from_q_strings() {
        assert_eq!(Some(&b'r'), b"qr{abc}".get(1));
        assert_eq!(
            quote_like_operator_parameters(b'q', Some(b'r')),
            Some((2, 1, true, QuoteLikeLiteralKind::Regex))
        );

        let qr_literal = quote_like_literal_start(b"qr{abc}", 0);

        assert!(qr_literal.is_some());
        if let Some(literal) = qr_literal {
            assert_eq!(literal.consumed, 3);
            assert_eq!(literal.sections, 1);
            assert_eq!(literal.kind, QuoteLikeLiteralKind::Regex);
        }
        assert_quote_like_start(b"q{abc}", 0, 2, 1, QuoteLikeLiteralKind::String);
        assert!(quote_like_literal_start(b"qa{abc}", 0).is_none());
    }

    #[test]
    fn quote_like_literal_start_discriminates_q_string_variants() {
        assert_eq!(
            quote_like_operator_parameters(b'q', Some(b'q')),
            Some((2, 1, true, QuoteLikeLiteralKind::String))
        );
        assert_eq!(
            quote_like_operator_parameters(b'q', Some(b'w')),
            Some((2, 1, true, QuoteLikeLiteralKind::String))
        );
        assert_eq!(
            quote_like_operator_parameters(b'q', Some(b'x')),
            Some((2, 1, true, QuoteLikeLiteralKind::String))
        );

        assert_quote_like_start(b"qq{abc}", 0, 3, 1, QuoteLikeLiteralKind::String);
        assert_quote_like_start(b"qw(foo)", 0, 3, 1, QuoteLikeLiteralKind::String);
        assert_quote_like_start(b"qx/path/", 0, 3, 1, QuoteLikeLiteralKind::String);
        assert_quote_like_start(b"qr{abc}", 0, 3, 1, QuoteLikeLiteralKind::Regex);
    }

    #[test]
    fn quote_like_literal_start_discriminates_tr_operator() {
        assert_eq!(Some(&b'r'), b"tr/a/b".get(1));
        assert_eq!(
            quote_like_operator_parameters(b't', Some(b'r')),
            Some((2, 2, true, QuoteLikeLiteralKind::Regex))
        );
        assert_eq!(quote_like_operator_parameters(b't', Some(b'/')), None);

        let tr_literal = quote_like_literal_start(b"tr/a/b", 0);

        assert!(tr_literal.is_some());
        if let Some(literal) = tr_literal {
            assert_eq!(literal.consumed, 3);
            assert_eq!(literal.sections, 2);
            assert_eq!(literal.kind, QuoteLikeLiteralKind::Regex);
        }
        assert!(quote_like_literal_start(b"t/a", 0).is_none());
        assert!(quote_like_literal_start(b"try { 1 }", 0).is_none());
    }

    #[test]
    fn literal_scan_index_eq_end_is_noop() {
        let mut state = LiteralScanState { in_single_quote: true, ..LiteralScanState::default() };

        assert_eq!(state.scan_segment(b"'", 1, 1), None);
        assert!(state.in_single_quote);
    }

    #[test]
    fn literal_scan_records_single_quote_resume() {
        let mut state = LiteralScanState { in_single_quote: true, ..LiteralScanState::default() };

        assert_eq!(state.scan_segment(b"abc' <<EOF", 0, 10), Some(4));
        assert!(!state.in_single_quote);
    }

    #[test]
    fn slash_regex_before_eq_zero_starts_literal() {
        assert_eq!(slash_starts_bare_regex_literal(b"/<<EOF/", 0), true);
    }

    #[test]
    fn slash_regex_can_start_after_leading_spaces() {
        assert_eq!(slash_starts_bare_regex_literal(b"   /<<EOF/", 3), true);
    }

    #[test]
    fn slash_regex_keyword_probe_keeps_underscore_inside_word() {
        assert_eq!(slash_starts_bare_regex_literal(b"not_if /<<EOF/", 7), false);
    }

    #[test]
    fn angle_quote_like_delimiter_uses_angle_closer() {
        assert_eq!(quote_like_closer(b'<'), Some(b'>'));
    }

    #[test]
    fn quoted_heredoc_label_preserves_non_quote_escape() {
        assert_eq!(parse_quoted_heredoc_label(r#""EO\nF""#, '"'), Some(r"EO\nF".to_string()));
    }

    #[test]
    fn pod_exits_on_cut_followed_by_any_non_word_byte() {
        // perl exits POD at column-zero `=cut` plus any non-word byte.
        let semicolon = "=pod\ndocs\n=cut;\nmy $code = 1;";
        assert!(!is_in_pod(semicolon, semicolon.len()));

        let hyphenated = "=pod\ndocs\n=cut-lt\nmy $code = 1;";
        assert!(!is_in_pod(hyphenated, hyphenated.len()));

        let plain = "=pod\ndocs\n=cut\nmy $code = 1;";
        assert!(!is_in_pod(plain, plain.len()));

        let spaced = "=pod\ndocs\n=cut \nmy $code = 1;";
        assert!(!is_in_pod(spaced, spaced.len()));
    }

    #[test]
    fn pod_word_continuations_after_cut_stay_pod() {
        for line in ["=cutlery", "=cut1", "=cut_lt"] {
            let source = format!("=pod\ndocs\n{line}\nmy $code = 1;");
            assert!(is_in_pod(&source, source.len()), "{line} must stay POD");
        }
    }

    #[test]
    fn is_in_pod_ignores_pod_lines_inside_multiline_literal() {
        let source = "my $text = \"\n=pod\n=cut\n\";\nmy $code = 1;";
        assert!(!is_in_pod(source, source.len()));

        let pod_after_literal = "my $text = \"\n=pod\n\";\n=pod\ndocs\nmy $code = 1;";
        assert!(is_in_pod(pod_after_literal, pod_after_literal.len()));
    }

    #[test]
    fn is_in_pod_ignores_cut_inside_unclosed_heredoc() {
        let source = "<<'EOF'\n=cut\nEOF\nmy $code = 1;";
        assert!(!is_in_pod(source, source.len()));
        assert!(!is_in_heredoc(source, source.find("my $code").unwrap()));
    }

    #[test]
    fn is_in_regex_resumes_after_line_comment() {
        let source = "my $http; # prior comment\nmy $pattern = qr{$http = HTTP::Tiny->new()};\n";
        let regex_body = source.find("HTTP::Tiny").unwrap();
        assert!(
            is_in_regex(source, regex_body),
            "a regex opened after a line comment must still be detected as a regex position"
        );

        let before_pattern = source.find("my $pattern").unwrap();
        assert!(!is_in_regex(source, before_pattern));
    }

    #[test]
    fn substitution_replacement_section_is_string_like() {
        let source = "my $x = s;foo;replacement;;
";
        let replacement = source.find("replacement").unwrap();
        assert!(is_in_string(source, replacement));
        assert!(!is_in_regex(source, replacement));

        let pattern = source.find("foo").unwrap();
        assert!(is_in_regex(source, pattern));
        assert!(!is_in_string(source, pattern));
    }

    #[test]
    fn transliteration_replacement_section_is_string_like() {
        // `tr///` and `y///` share the two-section regex-kind literal path
        // with `s///`, so the replacement side pins the same classification.
        for operator in ["tr", "y"] {
            let source = format!("my $x = {operator};abc;replacement;;\n");
            let replacement = source.find("replacement").unwrap();
            assert!(is_in_string(&source, replacement), "{operator} replacement is string-like");
            assert!(!is_in_regex(&source, replacement), "{operator} replacement is not regex");
        }
    }

    #[test]
    fn is_in_regex_ignores_regex_like_text_in_non_code_regions_after_comment() {
        let heredoc = "# docs\nmy $text = <<'END';\nqr{ unmatched\nEND\nmy $code = 1;\n";
        assert!(
            !is_in_regex(heredoc, heredoc.find("my $code").unwrap()),
            "regex-like text inside a heredoc body must not leave literal state active"
        );

        let pod = "# docs\n=pod\nqr{ unmatched\n=cut\nmy $code = 1;\n";
        assert!(
            !is_in_regex(pod, pod.find("my $code").unwrap()),
            "regex-like text inside a POD body must not leave literal state active"
        );
    }
}
