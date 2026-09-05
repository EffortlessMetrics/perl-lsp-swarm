//! Statement splitting and `lib` pragma prefix recognition.

use std::collections::VecDeque;

/// Split Perl source into semicolon-terminated statements without treating
/// semicolons inside simple quoted strings, line comments, POD, or heredoc
/// bodies as terminators.
///
/// Because the split is semicolon-driven, each slice is then normalized by
/// [`strip_statement_prefix`]: leading closing braces from blocks that already
/// ended are dropped, and one leading `BEGIN { ... }` header is peeled. Without
/// the peel the block opener hides an otherwise ordinary `use lib` / `no lib`
/// pragma; without the brace trim the *next* top-level pragma stays hidden, so
/// a block-scoped root would be reported while the later file-level root that
/// should outrank it is not.
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
            && let Some((heredoc_end, tag, strip_indent, requires_terminator)) =
                parse_heredoc_opener(source, idx)
            && (!requires_terminator
                || has_heredoc_terminator(source, heredoc_end, &tag, strip_indent))
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
            // A trailing comment swallows the newline that would otherwise
            // trigger the pending heredoc bodies, so drain them at the same
            // line boundary instead of scanning those bodies as code.
            let resume = if pending_heredocs.is_empty() {
                comment_end
            } else {
                skip_heredoc_bodies(source, comment_end, &mut pending_heredocs)
            };
            // If no statement content has been seen yet, advance `start` past
            // the comment so the comment text is not included in the next slice.
            if !has_content {
                start = resume;
            }
            i = advance_char_index(&chars, i, resume);
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

/// Walk the lines starting at `from`, yielding each line without its `\r\n`
/// terminator together with the offset just past that terminator.
///
/// A final line with no trailing newline yields `source.len()`, so callers can
/// use the yielded offset as a resume point without re-deriving line ends.
fn lines_from(source: &str, from: usize) -> impl Iterator<Item = (&str, usize)> {
    let mut line_start = from;
    std::iter::from_fn(move || {
        if line_start >= source.len() {
            return None;
        }
        let newline = source[line_start..].find('\n');
        let line_end = newline.map_or(source.len(), |offset| line_start + offset);
        let raw = &source[line_start..line_end];
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let next = newline.map_or(source.len(), |_| line_end + 1);
        line_start = next;
        Some((line, next))
    })
}

/// Whether `line` is the `=cut` that ends a POD section.
///
/// `=cutlery` is ordinary POD prose, so the terminator must be the whole line
/// or be followed by whitespace.
fn is_pod_terminator(line: &str) -> bool {
    line.strip_prefix("=cut")
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(char::is_whitespace))
}

/// Whether `line` is the terminator for a heredoc opened with `tag`.
fn closes_heredoc(line: &str, tag: &str, strip_indent: bool) -> bool {
    if strip_indent { line.trim_start() == tag } else { line == tag }
}

fn skip_pod_section(source: &str, start: usize) -> usize {
    let Some(first_newline) = source[start..].find('\n') else {
        return source.len();
    };

    lines_from(source, start + first_newline + 1)
        .find(|(line, _)| is_pod_terminator(line))
        .map_or(source.len(), |(_, next)| next)
}

/// Whether the `<<` at `start` follows a complete term, which makes it the
/// left-shift operator rather than a heredoc opener.
///
/// Perl resolves this by lexer position, and the distinction is observable:
/// `perl -c` accepts `my $x = 1 <<'EOF';` with no `EOF` line anywhere, so `<<`
/// after a number is a shift. A preceding bareword is a function call
/// (`print <<'EOF'`), which leaves `<<` in term position.
fn follows_complete_term(source: &str, start: usize) -> bool {
    let before = source[..start].trim_end_matches([' ', '\t']);
    let Some(last) = before.chars().next_back() else {
        return false;
    };
    if matches!(last, ')' | ']' | '}') {
        return true;
    }
    if !(last.is_ascii_alphanumeric() || last == '_') {
        return false;
    }

    let run_start = before
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_')
        .last()
        .map_or(0, |(index, _)| index);

    // A bare number ends a term.
    if before[run_start..].chars().all(|ch| ch.is_ascii_digit()) {
        return true;
    }

    let before_run = before[..run_start].trim_end_matches([' ', '\t']);
    let Some(sigil) = before_run.chars().next_back().filter(|ch| "$@%&".contains(*ch)) else {
        // A bareword here is a function call, which leaves `<<` in term position.
        return false;
    };

    // A sigiled variable usually ends a term — but not when it is itself an
    // argument to a preceding bareword. Perl's indirect filehandle syntax puts
    // `<<` in term position there: `print $fh <<'EOF'` is a heredoc, which
    // `perl -c` confirms by demanding the terminator.
    let before_sigil =
        before_run[..before_run.len() - sigil.len_utf8()].trim_end_matches([' ', '\t']);
    !before_sigil.chars().next_back().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Recognize a heredoc opener at `start`, returning
/// `(end, tag, strip_indent, requires_terminator)`.
///
/// Position decides first: after a complete term this is the left-shift
/// operator and no heredoc is reported at all.
///
/// In term position, `requires_terminator` is set only for a bareword tag
/// (`<<EOF`), which is indistinguishable from incidental text such as a regex
/// body and so must be confirmed by a matching terminator line. A quoted or
/// backslash-escaped tag is honored immediately, even while the body is still
/// being typed and no terminator exists yet. That direction matters: an
/// unconfirmed opener suppresses later text, whereas refusing to recognize one
/// lets heredoc prose be scanned as code and *invent* an `@INC` root the
/// program never adds.
fn parse_heredoc_opener(source: &str, start: usize) -> Option<(usize, String, bool, bool)> {
    if source.get(start..start + 2)? != "<<" {
        return None;
    }

    // Perl decides heredoc-versus-shift by lexer position, not by the delimiter
    // form, and never revisits that choice. `perl -c` accepts `my $x = 1 <<'EOF';`
    // and `my $x = 1 <<\EOF;` with no terminator anywhere, and running the latter
    // shows the body parsed as code. So a terminator that happens to appear later
    // must not be allowed to reclassify a shift as a heredoc.
    if follows_complete_term(source, start) {
        return None;
    }

    let mut tag_start = start + 2;
    let strip_indent = source.as_bytes().get(tag_start) == Some(&b'~');
    if strip_indent {
        tag_start += 1;
    }

    // Perl allows whitespace between `<<`/`<<~` and a *quoted* delimiter, but
    // not before a bareword one: `<< EOF` is the left-shift operator while
    // `<< 'EOF'` is a heredoc.
    let spaced_tag_start = tag_start
        + source.as_bytes()[tag_start..]
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();

    let first = *source.as_bytes().get(spaced_tag_start)?;

    // `<<\EOF` is a heredoc whose bareword delimiter carries single-quote
    // semantics. The backslash removes the left-shift ambiguity of a bare
    // `<<EOF`, so it is confirmed on the same rule as a quoted delimiter.
    if first == b'\\' {
        let tag_start_after_escape = spaced_tag_start + 1;
        let mut tag_end = tag_start_after_escape;
        while let Some(byte) = source.as_bytes().get(tag_end) {
            if !(byte.is_ascii_alphanumeric() || *byte == b'_') {
                break;
            }
            tag_end += 1;
        }
        if tag_end == tag_start_after_escape {
            return None;
        }
        return Some((
            tag_end,
            source[tag_start_after_escape..tag_end].to_string(),
            strip_indent,
            false,
        ));
    }

    if first == b'\'' || first == b'"' || first == b'`' {
        let quote = first as char;
        let content_start = spaced_tag_start + 1;
        let quote_offset = source[content_start..].find(quote)?;
        let quote_end = content_start + quote_offset;
        let tag = &source[content_start..quote_end];
        if tag.contains('\n') {
            return None;
        }
        return Some((quote_end + 1, tag.to_string(), strip_indent, false));
    }

    if spaced_tag_start != tag_start || !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }

    let mut tag_end = tag_start + 1;
    while let Some(byte) = source.as_bytes().get(tag_end) {
        if !(byte.is_ascii_alphanumeric() || *byte == b'_') {
            break;
        }
        tag_end += 1;
    }

    Some((tag_end, source[tag_start..tag_end].to_string(), strip_indent, true))
}

fn has_heredoc_terminator(source: &str, start: usize, tag: &str, strip_indent: bool) -> bool {
    let Some(first_newline) = source[start..].find('\n') else {
        return false;
    };

    lines_from(source, start + first_newline + 1)
        .any(|(line, _)| closes_heredoc(line, tag, strip_indent))
}

fn skip_heredoc_bodies(
    source: &str,
    mut line_start: usize,
    pending: &mut VecDeque<(String, bool)>,
) -> usize {
    if pending.is_empty() {
        return line_start;
    }

    for (line, next) in lines_from(source, line_start) {
        line_start = next;
        let closes_front = pending
            .front()
            .is_some_and(|(tag, strip_indent)| closes_heredoc(line, tag, *strip_indent));
        if closes_front {
            pending.pop_front();
            if pending.is_empty() {
                break;
            }
        }
    }

    line_start
}

fn push_statement<'a>(statements: &mut Vec<&'a str>, statement: &'a str) {
    statements.push(strip_statement_prefix(statement));
}

/// Trim block punctuation that belongs to *preceding* code off the front of a
/// statement slice.
///
/// `split_perl_statements` cuts on semicolons only, so a slice routinely opens
/// with the closing braces of blocks that ended before it (`}\nuse lib 'x';`),
/// and a block-leading pragma opens with its own `BEGIN {` header. Neither is
/// part of the statement the prefix recognizers are asked about, and both are
/// trimmed here so the same normalization applies whether or not a `BEGIN`
/// block is present.
///
/// The result stays a subslice of the original source, so the statement end —
/// the activation rail used by `activation_offset` — is unchanged.
fn strip_statement_prefix(statement: &str) -> &str {
    let mut rest = skip_leading_whitespace_and_comments(statement);
    while let Some(after_close) = rest.strip_prefix('}') {
        rest = skip_leading_whitespace_and_comments(after_close);
    }
    strip_leading_begin_block_prefix(rest).unwrap_or(rest)
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
