//! Statement splitting and `lib` pragma prefix recognition.

use std::collections::VecDeque;

/// Split Perl source into semicolon-terminated statements without treating
/// semicolons inside simple quoted strings, line comments, POD, or heredoc
/// bodies as terminators.
///
/// The compatibility scanner also exposes the first statement inside a leading
/// `BEGIN { ... }` block as a source subslice. Without that bounded prefix peel,
/// the block opener hides an otherwise ordinary `use lib` or `no lib` pragma
/// from the prefix recognizer.
pub(super) fn split_perl_statements(source: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    // Whether any non-whitespace, non-comment content has appeared in the
    // current statement since `start`.  When false and we hit a comment, we
    // can safely advance `start` past the comment so it doesn't pollute the
    // next statement slice.
    let mut has_content = false;
    let mut pending_heredocs = VecDeque::new();

    let chars: Vec<(usize, char)> = source.char_indices().collect();
    let mut i = 0;

    while i < chars.len() {
        let (idx, ch) = chars[i];

        if escaped {
            escaped = false;
            i += 1;
            continue;
        }

        if ch == '\\' && (in_single || in_double) {
            escaped = true;
            i += 1;
            continue;
        }

        if ch == '\'' && !in_double {
            in_single = !in_single;
            has_content = true;
            i += 1;
            continue;
        }

        if ch == '"' && !in_single {
            in_double = !in_double;
            has_content = true;
            i += 1;
            continue;
        }

        if ch == '='
            && !in_single
            && !in_double
            && (idx == 0 || source.as_bytes().get(idx - 1) == Some(&b'\n'))
            && chars.get(i + 1).is_some_and(|(_, next)| next.is_ascii_alphabetic())
        {
            let pod_end = skip_pod_section(source, idx);
            if !has_content {
                start = pod_end;
            }
            i = advance_char_index(&chars, i, pod_end);
            continue;
        }

        if ch == '<'
            && !in_single
            && !in_double
            && let Some((heredoc_end, tag, strip_indent, quoted)) =
                parse_heredoc_opener(source, idx)
            && (quoted || has_heredoc_terminator(source, heredoc_end, &tag, strip_indent))
        {
            pending_heredocs.push_back((tag, strip_indent));
            has_content = true;
            i = advance_char_index(&chars, i, heredoc_end);
            continue;
        }

        if ch == '\n' && !in_single && !in_double && !pending_heredocs.is_empty() {
            let body_end = skip_heredoc_bodies(source, idx + 1, &mut pending_heredocs);
            if !has_content {
                start = body_end;
            }
            i = advance_char_index(&chars, i, body_end);
            continue;
        }

        // Skip Perl line comments: # ... <newline>
        // A `#` is only a comment when outside of any string literal.
        if ch == '#' && !in_single && !in_double {
            // Skip to end of line (or end of source).
            let comment_end = match source[idx..].find('\n') {
                Some(nl_offset) => idx + nl_offset + 1,
                None => source.len(),
            };
            // If no statement content has been seen yet, advance `start` past
            // the comment so the comment text is not included in the next slice.
            if !has_content {
                start = comment_end;
            }
            // Skip the iterator past the comment.
            while i < chars.len() && chars[i].0 < comment_end {
                i += 1;
            }
            continue;
        }

        if ch == ';' && !in_single && !in_double {
            let end = idx + ch.len_utf8();
            push_statement(&mut statements, &source[start..end]);
            start = end;
            has_content = false;
        } else if !ch.is_whitespace() {
            has_content = true;
        }

        i += 1;
    }

    if start < source.len() {
        push_statement(&mut statements, &source[start..]);
    }

    statements
}

fn advance_char_index(chars: &[(usize, char)], mut index: usize, byte_end: usize) -> usize {
    while index < chars.len() && chars[index].0 < byte_end {
        index += 1;
    }
    index
}

fn skip_pod_section(source: &str, start: usize) -> usize {
    let Some(first_newline) = source[start..].find('\n') else {
        return source.len();
    };

    let mut line_start = start + first_newline + 1;
    while line_start < source.len() {
        let newline = source[line_start..].find('\n');
        let line_end = newline.map_or(source.len(), |offset| line_start + offset);
        let mut line = &source[line_start..line_end];
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
        if line == "=cut"
            || line
                .strip_prefix("=cut")
                .is_some_and(|suffix| suffix.chars().next().is_some_and(char::is_whitespace))
        {
            return newline.map_or(source.len(), |_| line_end + 1);
        }
        let Some(_offset) = newline else {
            return source.len();
        };
        line_start = line_end + 1;
    }

    source.len()
}

fn parse_heredoc_opener(source: &str, start: usize) -> Option<(usize, String, bool, bool)> {
    if source.get(start..start + 2)? != "<<" {
        return None;
    }

    let mut tag_start = start + 2;
    let strip_indent = source.as_bytes().get(tag_start) == Some(&b'~');
    if strip_indent {
        tag_start += 1;
    }

    let first = *source.as_bytes().get(tag_start)?;
    if first == b'\'' || first == b'"' || first == b'`' {
        let quote = first as char;
        let content_start = tag_start + 1;
        let quote_offset = source[content_start..].find(quote)?;
        let quote_end = content_start + quote_offset;
        let tag = &source[content_start..quote_end];
        if tag.is_empty() || tag.contains('\n') {
            return None;
        }
        return Some((quote_end + 1, tag.to_string(), strip_indent, true));
    }

    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }

    let mut tag_end = tag_start + 1;
    while let Some(byte) = source.as_bytes().get(tag_end) {
        if !(byte.is_ascii_alphanumeric() || *byte == b'_') {
            break;
        }
        tag_end += 1;
    }

    Some((tag_end, source[tag_start..tag_end].to_string(), strip_indent, false))
}

fn has_heredoc_terminator(source: &str, start: usize, tag: &str, strip_indent: bool) -> bool {
    let Some(first_newline) = source[start..].find('\n') else {
        return false;
    };

    let mut line_start = start + first_newline + 1;
    while line_start < source.len() {
        let newline = source[line_start..].find('\n');
        let line_end = newline.map_or(source.len(), |offset| line_start + offset);
        let mut line = &source[line_start..line_end];
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
        let content = if strip_indent { line.trim_start() } else { line };
        if content == tag {
            return true;
        }

        let Some(_offset) = newline else {
            return false;
        };
        line_start = line_end + 1;
    }

    false
}

fn skip_heredoc_bodies(
    source: &str,
    mut line_start: usize,
    pending: &mut VecDeque<(String, bool)>,
) -> usize {
    while !pending.is_empty() && line_start < source.len() {
        let newline = source[line_start..].find('\n');
        let line_end = newline.map_or(source.len(), |offset| line_start + offset);
        let mut line = &source[line_start..line_end];
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
        let closes_front = pending.front().is_some_and(|(tag, strip_indent)| {
            let content = if *strip_indent { line.trim_start() } else { line };
            content == tag
        });
        if closes_front {
            pending.pop_front();
        }

        let Some(_offset) = newline else {
            return source.len();
        };
        line_start = line_end + 1;
    }

    line_start
}

fn push_statement<'a>(statements: &mut Vec<&'a str>, statement: &'a str) {
    let trimmed = statement.trim_start();
    let statement = strip_leading_begin_block_prefix(trimmed).unwrap_or(statement);
    statements.push(statement);
}

/// Return the first statement body inside a leading `BEGIN { ... }` block.
///
/// This is intentionally narrower than Perl block parsing: it only removes the
/// phase keyword, optional line comments, and the opening brace before the
/// first semicolon-delimited statement. The returned value remains a subslice
/// of the original source, so the statement end (the activation rail used by
/// `activation_offset`) is preserved while the slice start moves past the
/// `BEGIN` prefix.
fn strip_leading_begin_block_prefix(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("BEGIN")?;
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '{' || c == '#') {
        return None;
    }

    let rest = skip_leading_whitespace_and_comments(rest);
    let rest = rest.strip_prefix('{')?;
    Some(skip_leading_whitespace_and_comments(rest))
}

fn skip_leading_whitespace_and_comments(mut source: &str) -> &str {
    loop {
        source = source.trim_start();
        let Some(comment) = source.strip_prefix('#') else {
            return source;
        };
        let Some(newline) = comment.find('\n') else {
            return &source[source.len()..];
        };
        source = &comment[newline + 1..];
    }
}

pub(super) fn strip_use_lib_prefix(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("use")?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("lib")?;
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '(' || c == ';') {
        return None;
    }
    Some(rest.trim_start())
}

pub(super) fn strip_no_lib_prefix(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("no")?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("lib")?;
    if !rest.starts_with(|c: char| c.is_whitespace() || c == '(' || c == ';') {
        return None;
    }
    Some(rest.trim_start())
}
