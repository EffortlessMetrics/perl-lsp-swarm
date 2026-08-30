//! Statement splitting and `lib` pragma prefix recognition.

/// Split Perl source into semicolon-terminated statements without treating
/// semicolons inside simple quoted strings or line comments as terminators.
///
/// The compatibility scanner also exposes the first statement inside a leading
/// `BEGIN { ... }` block as a source subslice. Without that bounded prefix peel,
/// the block opener hides an otherwise ordinary `use lib` or `no lib` pragma
/// from the prefix recognizer. Later statements in the block already begin at
/// their own semicolon boundary and need no special handling.
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
