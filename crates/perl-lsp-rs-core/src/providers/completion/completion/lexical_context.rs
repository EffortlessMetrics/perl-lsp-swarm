/// Simple heuristic to check if position is in a string.
pub(super) fn is_in_string(source: &str, position: usize) -> bool {
    let before = &source[..position];
    let single_quotes = before.matches('\'').count();
    let double_quotes = before.matches('"').count();

    single_quotes % 2 == 1 || double_quotes % 2 == 1
}

/// Heuristic to check if position is inside a regex literal.
pub(super) fn is_in_regex(source: &str, position: usize) -> bool {
    let before = &source[..position];

    let Some(last_slash) = before.rfind('/') else {
        return false;
    };

    let pre_slash = before[..last_slash].trim_end();
    if pre_slash.ends_with("=~") || pre_slash.ends_with("!~") {
        return true;
    }

    if pre_slash_has_regex_op(pre_slash) {
        return true;
    }

    if matches!(
        pre_slash.split_ascii_whitespace().next_back(),
        Some("or") | Some("and") | Some("not")
    ) {
        return true;
    }

    if let Some(last_char) = pre_slash.chars().next_back()
        && matches!(last_char, '(' | ',' | '=' | '!' | '&' | '|' | ';' | '{' | '~')
    {
        return true;
    }

    pre_slash.is_empty()
}

/// Return true when the text immediately before a `/` is one of the explicit regex operators.
fn pre_slash_has_regex_op(pre_slash: &str) -> bool {
    let trimmed = pre_slash.trim_end();
    for op in &["qr", "m", "s", "tr", "y"] {
        if let Some(before_op) = trimmed.strip_suffix(op) {
            let boundary_ok = before_op
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
            if boundary_ok {
                return true;
            }
        }
    }
    false
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
    if close_pos >= 2 && without_flags.ends_with('/') && is_in_regex(source, close_pos - 1) {
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

/// Simple heuristic to check if position is in a comment.
pub(super) fn is_in_comment(source: &str, position: usize) -> bool {
    let line_start = source[..position].rfind('\n').map_or(0, |p| p + 1);
    let line = &source[line_start..position];
    line.contains('#')
}

/// Check if position is inside a heredoc literal.
///
/// A heredoc starts with `<<DELIMITER` or `<<'DELIMITER'` or `<<"DELIMITER"` and
/// ends when a line contains only the delimiter (and optional trailing whitespace/newline).
pub(super) fn is_in_heredoc(source: &str, position: usize) -> bool {
    if position == 0 {
        return false;
    }

    let before = &source[..position];

    // Find the last occurrence of << which could start a heredoc
    let Some(heredoc_start) = before.rfind("<<") else {
        return false;
    };

    // Extract the delimiter
    let after_heredoc = &source[heredoc_start + 2..];
    let Some(delimiter) = extract_heredoc_delimiter(after_heredoc) else {
        return false;
    };

    if delimiter.is_empty() {
        return false;
    }

    // Find the opening line (where << appears)
    let open_line_end =
        source[heredoc_start..].find('\n').map(|p| heredoc_start + p).unwrap_or(source.len());

    // After the opening line, look for the closing delimiter.
    // We walk the lines manually with a byte-offset tracker so we can compute
    // the exact start position of the closing line without the off-by-N errors
    // that arise from `after_open_line.len() - line.len()` (which is wrong when
    // the closing line is not the very last bytes of the slice).
    let after_open_line = &source[open_line_end..];

    let mut byte_offset = 0usize;
    for line in after_open_line.lines() {
        let trimmed = line.trim_end();
        if trimmed == delimiter {
            // `byte_offset` is now the start of the closing delimiter line
            // within `after_open_line`.  The heredoc body is the region
            // strictly between `open_line_end` (the opening `\n`) and the
            // first byte of the closing delimiter line.
            let closing_line_start = open_line_end + byte_offset;
            return position > open_line_end && position < closing_line_start;
        }
        // Advance past this line and its line separator (1 byte for `\n`; the
        // lines() iterator strips the separator, so we add it back).
        byte_offset += line.len() + 1;
    }

    // If no closing delimiter found, assume we're in the heredoc if position is after the opening
    position > open_line_end
}

/// Extract the heredoc delimiter from text immediately after `<<`
fn extract_heredoc_delimiter(text: &str) -> Option<String> {
    let first_char = text.chars().next()?;

    match first_char {
        // Quoted forms: <<'EOF', <<"EOF", etc.
        '\'' | '"' => {
            let close_quote = text[first_char.len_utf8()..]
                .find(first_char)
                .map(|p| first_char.len_utf8() + p)?;
            Some(text[first_char.len_utf8()..close_quote].to_string())
        }
        // Bare form: <<EOF
        _ if first_char.is_ascii_alphabetic() || first_char == '_' => {
            let end =
                text.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(text.len());
            Some(text[..end].to_string())
        }
        _ => None,
    }
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
pub(super) fn is_in_pod(source: &str, position: usize) -> bool {
    if position == 0 {
        return false;
    }

    let before = &source[..position];

    let mut in_pod_block = false;
    // Track whether we are inside a heredoc body while scanning lines.
    // We open a heredoc when we see <<DELIM and close it when we see the
    // delimiter alone on a line.  This prevents heredoc bodies that happen
    // to contain `=pod` from poisoning the POD state machine.
    let mut heredoc_delimiter: Option<String> = None;

    for line in before.lines() {
        // If we are inside a heredoc, check for the closing delimiter first.
        if let Some(ref delim) = heredoc_delimiter {
            if line.trim_end() == delim.as_str() {
                heredoc_delimiter = None;
            }
            // Either way, skip POD checks for this line — it is heredoc content.
            continue;
        }

        // Check whether this line opens a heredoc.
        if let Some(hd_pos) = line.find("<<") {
            let after = &line[hd_pos + 2..];
            if let Some(delim) = extract_heredoc_delimiter(after) {
                if !delim.is_empty() {
                    heredoc_delimiter = Some(delim);
                    // The opening line itself is code, not heredoc body — fall
                    // through to POD checks (unlikely to be =pod, but correct).
                }
            }
        }

        // POD commands must start at column 0 (no leading whitespace).
        if is_pod_start_marker(line) {
            in_pod_block = true;
        }
        if is_pod_end_marker(line) {
            in_pod_block = false;
        }
    }

    in_pod_block
}

fn is_pod_start_marker(line: &str) -> bool {
    // Per perlpod: the command paragraph must begin at column 0.
    // Do NOT use trim_start() here — indented `=pod` is not a POD directive.
    line.starts_with("=pod")
        || line.starts_with("=head1")
        || line.starts_with("=head2")
        || line.starts_with("=head3")
        || line.starts_with("=head4")
        || line.starts_with("=over")
        || line.starts_with("=item")
        || line.starts_with("=back")
        || line.starts_with("=for")
        || line.starts_with("=begin")
        || line.starts_with("=encoding")
        || line.starts_with("=attr")
        || line.starts_with("=method")
        || line.starts_with("=func")
}

fn is_pod_end_marker(line: &str) -> bool {
    // Per perlpod: `=cut` must also appear at column 0.
    line.starts_with("=cut")
}
