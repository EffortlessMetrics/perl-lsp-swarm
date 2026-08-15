//! Uniform quote operator parsing for the Perl parser.
//!
//! This module provides consistent parsing for quote-like operators,
//! properly extracting patterns, bodies, and modifiers.

use std::borrow::Cow;

/// Extract pattern and modifiers from a regex-like token (qr, m, or bare //)
pub fn extract_regex_parts(text: &str) -> (String, String, String) {
    // Handle different prefixes
    let content = if let Some(stripped) = text.strip_prefix("qr") {
        skip_paired_replacement_gap(stripped)
    } else if let Some(stripped) = strip_match_prefix(text) {
        skip_paired_replacement_gap(stripped)
    } else {
        text
    };

    // Get delimiter - content must be non-empty to have a delimiter
    let delimiter = match content.chars().next() {
        Some(d) => d,
        None => return (String::new(), String::new(), String::new()),
    };
    let closing = get_closing_delimiter(delimiter);

    // Extract body and modifiers
    let (body, modifiers) = extract_delimited_content(content, delimiter, closing);

    // Include delimiters in the pattern string for compatibility
    let pattern = format!("{}{}{}", delimiter, body, closing);

    (pattern, body, modifiers.to_string())
}

fn strip_match_prefix(text: &str) -> Option<&str> {
    let stripped = text.strip_prefix('m')?;
    let delimiter = skip_paired_replacement_gap(stripped).chars().next()?;
    (!delimiter.is_alphabetic()).then_some(stripped)
}

/// Strip the `tr` or `y` operator prefix from a transliteration token.
///
/// Checks `tr` before `y` so a hypothetical `tr` token is never misrouted
/// through a single-char `t`+`r` path. Returns the input unchanged when
/// neither prefix is present (e.g. when the caller already stripped it).
fn strip_transliteration_prefix(text: &str) -> &str {
    if let Some(s) = text.strip_prefix("tr") {
        s
    } else if let Some(s) = text.strip_prefix('y') {
        s
    } else {
        text
    }
}

/// Error type for substitution operator parsing failures
#[derive(Debug, Clone, PartialEq)]
pub enum SubstitutionError {
    /// Invalid modifier character found
    InvalidModifier(char),
    /// Invalid delimiter after `s` (alphanumeric or whitespace)
    InvalidDelimiter(char),
    /// Missing delimiter after 's'
    MissingDelimiter,
    /// Pattern is missing or empty (just `s/`)
    MissingPattern,
    /// Replacement section is missing (e.g., `s/pattern` without replacement part)
    MissingReplacement,
    /// Closing delimiter is missing after replacement (e.g., `s/pattern/replacement` without final `/`)
    MissingClosingDelimiter,
}

/// Error type for transliteration operator parsing failures
#[derive(Debug, Clone, PartialEq)]
pub enum TransliterationError {
    /// Invalid modifier character found
    InvalidModifier(char),
    /// Invalid delimiter after `tr`/`y`
    InvalidDelimiter(char),
    /// Missing delimiter after `tr`/`y`
    MissingDelimiter,
    /// Search list section is missing
    MissingSearch,
    /// Replacement list section is missing
    MissingReplacement,
    /// Closing delimiter is missing
    MissingClosingDelimiter,
}

/// Extract pattern, replacement, and modifiers from a substitution token with strict validation
///
/// This function parses substitution operators like s/pattern/replacement/flags
/// and handles various delimiter forms including:
/// - Non-paired delimiters: s/pattern/replacement/ (same delimiter for all parts)
/// - Paired delimiters: s{pattern}{replacement} (different open/close delimiters)
///
/// Unlike `extract_substitution_parts`, this function returns an error if invalid modifiers
/// are present instead of silently filtering them.
///
/// # Errors
///
/// Returns `Err(SubstitutionError::InvalidModifier(c))` if an invalid modifier character is found.
/// Valid modifiers are: g, i, m, s, x, o, e, r
///
/// Returns `Err(SubstitutionError::InvalidDelimiter(c))` if the delimiter after `s`
/// (or the paired replacement delimiter) is alphanumeric or whitespace, mirroring the
/// delimiter validation in `extract_transliteration_parts_strict`.
pub fn extract_substitution_parts_strict(
    text: &str,
) -> Result<(String, String, String), SubstitutionError> {
    // Skip 's' prefix
    let after_s = text.strip_prefix('s').unwrap_or(text);
    // Perl allows whitespace and line comments between `s` and a paired delimiter
    // (e.g. `s # comment\n [pattern] [replacement]` in upstream `t/base/lex.t`).
    let content = skip_paired_replacement_gap(after_s);

    // Get delimiter - check for missing delimiter (just 's' or 's' followed by nothing)
    let delimiter = match content.chars().next() {
        Some(d) => d,
        None => return Err(SubstitutionError::MissingDelimiter),
    };
    // Reject alphanumeric/whitespace delimiters as a distinct error rather than
    // misreporting a missing closing delimiter.
    if !is_valid_delimiter(delimiter) {
        return Err(SubstitutionError::InvalidDelimiter(delimiter));
    }
    let closing = get_closing_delimiter(delimiter);
    let is_paired = delimiter != closing;

    // Parse first body (pattern) with strict validation
    let (pattern, rest1, pattern_closed) =
        extract_delimited_content_strict(content, delimiter, closing);

    // Paired or not, an unclosed pattern is a missing closing delimiter.
    if !pattern_closed {
        return Err(SubstitutionError::MissingClosingDelimiter);
    }

    // Parse second body (replacement)
    // For paired delimiters, the replacement may use a different delimiter than the pattern
    // e.g., s[pattern]{replacement} is valid Perl
    let (replacement, modifiers_str, replacement_closed) = if !is_paired {
        // Non-paired delimiters: must have replacement section
        if rest1.is_empty() {
            return Err(SubstitutionError::MissingReplacement);
        }

        // Parse replacement, skipping string literals so that delimiter chars
        // inside "foo/bar" or 'a/b' don't terminate the replacement early.
        extract_unpaired_body_skip_strings(rest1, closing)
    } else {
        // Paired pattern delimiters still allow either paired or non-paired delimiters
        // for the replacement side (e.g. s{foo}/bar/ and s[foo]{bar}).
        let trimmed = skip_paired_replacement_gap(rest1);
        if let Some(rd) = trimmed.chars().next() {
            // After a paired pattern delimiter (e.g. `{...}`), the replacement
            // may use any non-whitespace character as a delimiter, INCLUDING
            // alphanumeric characters (which are self-delimiting: `s{foo}x...x`
            // is valid Perl). Only reject whitespace as a replacement delimiter.
            if rd.is_whitespace() {
                return Err(SubstitutionError::InvalidDelimiter(rd));
            }
            // For paired replacement delimiters (e.g. `s{foo}{bar}`), use the
            // paired extraction. For non-paired (including alphanumeric), the
            // character is its own open+close delimiter.
            let rd_closing = get_closing_delimiter(rd);
            if rd == rd_closing {
                // Self-delimiting: extract until the next occurrence of rd
                extract_delimited_content_strict(trimmed, rd, rd_closing)
            } else {
                // Paired: extract with balanced nesting
                extract_delimited_content_strict(trimmed, rd, rd_closing)
            }
        } else {
            // No more content - missing replacement
            return Err(SubstitutionError::MissingReplacement);
        }
    };

    // Paired or not, an unclosed replacement is a missing closing delimiter.
    if !replacement_closed {
        return Err(SubstitutionError::MissingClosingDelimiter);
    }

    // Validate modifiers strictly - reject if any invalid modifiers present
    let modifiers = validate_substitution_modifiers(modifiers_str)
        .map_err(SubstitutionError::InvalidModifier)?;

    Ok((pattern, replacement, modifiers))
}

fn skip_paired_replacement_gap(mut text: &str) -> &str {
    let mut comment_eligible = false;
    loop {
        let trimmed = text.trim_start_matches(char::is_whitespace);
        let saw_whitespace = trimmed.len() != text.len();
        text = trimmed;
        comment_eligible |= saw_whitespace;

        if comment_eligible && text.starts_with('#') {
            text = after_line_comment(text);
            comment_eligible = true;
            continue;
        }

        return text;
    }
}

fn after_line_comment(text: &str) -> &str {
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '\n' | '\r') {
            return &text[idx + ch.len_utf8()..];
        }
    }
    ""
}

/// Extract content between delimiters with strict tracking of whether closing was found.
/// Returns (content, rest, found_closing).
fn extract_delimited_content_strict(text: &str, open: char, close: char) -> (String, &str, bool) {
    let mut chars = text.char_indices();
    let is_paired = open != close;

    // Skip opening delimiter
    if let Some((_, c)) = chars.next() {
        if c != open {
            return (String::new(), text, false);
        }
    } else {
        return (String::new(), "", false);
    }

    let mut body = String::new();
    let mut depth = if is_paired { 1 } else { 0 };
    let mut escaped = false;
    let mut end_pos = text.len();
    let mut found_closing = false;

    for (i, ch) in chars {
        if escaped {
            body.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                body.push(ch);
                escaped = true;
            }
            c if c == open && is_paired => {
                body.push(ch);
                depth += 1;
            }
            c if c == close => {
                if is_paired {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = i + ch.len_utf8();
                        found_closing = true;
                        break;
                    }
                    body.push(ch);
                } else {
                    end_pos = i + ch.len_utf8();
                    found_closing = true;
                    break;
                }
            }
            _ => body.push(ch),
        }
    }

    (body, &text[end_pos..], found_closing)
}

/// Extract pattern, replacement, and modifiers from a substitution token
///
/// This function parses substitution operators like s/pattern/replacement/flags
/// and handles various delimiter forms including:
/// - Non-paired delimiters: s/pattern/replacement/ (same delimiter for all parts)
/// - Paired delimiters: s{pattern}{replacement} (different open/close delimiters)
///
/// For paired delimiters, properly handles nested delimiters within the pattern
/// or replacement parts. Returns (pattern, replacement, modifiers) as strings.
///
/// Note: This function silently filters invalid modifiers. For strict validation,
/// use `extract_substitution_parts_strict` instead.
pub fn extract_substitution_parts(text: &str) -> (String, String, String) {
    // Skip 's' prefix
    let content = skip_paired_replacement_gap(text.strip_prefix('s').unwrap_or(text));

    // Get delimiter - content must be non-empty to have a delimiter
    let delimiter = match content.chars().next() {
        Some(d) => d,
        None => return (String::new(), String::new(), String::new()),
    };
    if !is_valid_delimiter(delimiter) {
        if let Some((pattern, replacement, modifiers_str)) = split_on_last_paired_delimiter(content)
        {
            let modifiers = extract_substitution_modifiers(&modifiers_str);
            return (pattern, replacement, modifiers);
        }

        return (String::new(), String::new(), String::new());
    }
    let closing = get_closing_delimiter(delimiter);
    let is_paired = delimiter != closing;

    // Parse first body (pattern)
    let (mut pattern, rest1, pattern_closed) = if is_paired {
        extract_substitution_pattern_with_replacement_hint(content, delimiter, closing)
    } else {
        extract_delimited_content_strict(content, delimiter, closing)
    };

    // Parse second body (replacement)
    // For paired delimiters, the replacement may use a different delimiter than the pattern
    // e.g., s[pattern]{replacement} is valid Perl
    let (replacement, modifiers_str) = if !is_paired && !rest1.is_empty() {
        // Non-paired delimiters: manually parse the replacement, skipping string literals
        // so that delimiter chars inside "foo/bar" or 'a/b' don't end the replacement early.
        let (body, rest, _found) = extract_unpaired_body_skip_strings(rest1, closing);
        (body, Cow::Borrowed(rest))
    } else if !is_paired && !pattern_closed {
        if let Some((fallback_pattern, fallback_replacement, fallback_modifiers)) =
            split_unclosed_substitution_pattern(&pattern)
        {
            pattern = fallback_pattern;
            (fallback_replacement, Cow::Owned(fallback_modifiers))
        } else {
            (String::new(), Cow::Borrowed(rest1))
        }
    } else if is_paired {
        let trimmed = skip_paired_replacement_gap(rest1);
        if let Some(rd) = trimmed.chars().next() {
            if !is_valid_delimiter(rd) {
                (String::new(), Cow::Borrowed(trimmed))
            } else {
                let repl_closing = get_closing_delimiter(rd);
                let (body, rest) = extract_delimited_content(trimmed, rd, repl_closing);
                (body, Cow::Borrowed(rest))
            }
        } else {
            (String::new(), Cow::Borrowed(trimmed))
        }
    } else {
        (String::new(), Cow::Borrowed(rest1))
    };

    // Extract and validate only valid substitution modifiers
    let modifiers = extract_substitution_modifiers(modifiers_str.as_ref());

    (pattern, replacement, modifiers)
}

/// Extract search, replace, and modifiers from a transliteration token
pub fn extract_transliteration_parts(text: &str) -> (String, String, String) {
    let after_op = strip_transliteration_prefix(text);
    let content = skip_paired_replacement_gap(after_op);

    // Get delimiter - content must be non-empty to have a delimiter
    let delimiter = match content.chars().next() {
        Some(d) => d,
        None => return (String::new(), String::new(), String::new()),
    };
    if !is_valid_delimiter(delimiter) {
        return (String::new(), String::new(), String::new());
    }
    let closing = get_closing_delimiter(delimiter);
    let is_paired = delimiter != closing;

    // Parse first body (search pattern)
    let (search, rest1) = extract_delimited_content(content, delimiter, closing);

    // Parse second body (replacement pattern)
    let (replacement, modifiers_str) = if !is_paired && !rest1.is_empty() {
        let (body, rest, _found) = extract_unpaired_body(rest1, closing);
        (body, rest)
    } else if is_paired {
        // Skip whitespace and allow any valid delimiter for the replacement list.
        // Perl accepts forms like tr[abc]{xyz} in addition to tr[abc][xyz].
        let rest2 = skip_paired_replacement_gap(rest1);
        match rest2.chars().next() {
            Some(repl_delimiter) if is_valid_delimiter(repl_delimiter) => {
                extract_delimited_content(
                    rest2,
                    repl_delimiter,
                    get_closing_delimiter(repl_delimiter),
                )
            }
            _ => (String::new(), rest2),
        }
    } else {
        (String::new(), rest1)
    };

    // Extract and validate only valid transliteration modifiers
    // Security fix: Apply consistent validation for all delimiter types
    let modifiers = modifiers_str
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .filter(|&c| is_transliteration_modifier(c))
        .collect();

    (search, replacement, modifiers)
}

/// Extract search, replace, and modifiers from a transliteration token with strict validation.
///
/// Supports both `tr///` and `y///` syntax, including optional whitespace between
/// the operator and delimiter (e.g. `tr /a/b/`).
///
/// # Errors
///
/// Returns `Err(TransliterationError::InvalidModifier(c))` if an invalid modifier
/// character is encountered. Valid modifiers are: `c`, `d`, `s`, `r`.
pub fn extract_transliteration_parts_strict(
    text: &str,
) -> Result<(String, String, String), TransliterationError> {
    // Strip `tr` or `y` prefix, then allow optional whitespace before delimiter.
    let after_op = strip_transliteration_prefix(text);
    let content = skip_paired_replacement_gap(after_op);

    // Get delimiter.
    let delimiter = match content.chars().next() {
        Some(d) => d,
        None => return Err(TransliterationError::MissingDelimiter),
    };
    if !is_valid_delimiter(delimiter) {
        return Err(TransliterationError::InvalidDelimiter(delimiter));
    }
    let closing = get_closing_delimiter(delimiter);
    let is_paired = delimiter != closing;

    // Parse first body (search).
    let (search, rest1, search_closed) =
        extract_delimited_content_strict(content, delimiter, closing);
    if !search_closed {
        return Err(TransliterationError::MissingClosingDelimiter);
    }

    // Parse second body (replacement).
    let (replacement, modifiers_str, replacement_closed) = if !is_paired {
        if rest1.is_empty() {
            return Err(TransliterationError::MissingReplacement);
        }
        extract_unpaired_body_skip_strings(rest1, closing)
    } else {
        let trimmed = skip_paired_replacement_gap(rest1);
        if let Some(repl_delimiter) = trimmed.chars().next() {
            // After a paired search delimiter (e.g. `{...}`), the replacement must
            // also start with a valid non-alphanumeric, non-whitespace delimiter.
            // An alphanumeric character here (e.g. `tr{abc}xyz`) is an invalid
            // delimiter, not merely a missing replacement section.
            if !is_valid_delimiter(repl_delimiter) {
                return Err(TransliterationError::InvalidDelimiter(repl_delimiter));
            }
            let repl_closing = get_closing_delimiter(repl_delimiter);
            let (body, rest, found_closing) =
                extract_delimited_content_strict(trimmed, repl_delimiter, repl_closing);
            (body, rest, found_closing)
        } else {
            return Err(TransliterationError::MissingReplacement);
        }
    };

    if !replacement_closed {
        return Err(TransliterationError::MissingClosingDelimiter);
    }

    // Note: an empty search list is valid Perl — `tr///` counts characters
    // (the "$count = ($str =~ tr///)" idiom). Do not reject empty search.

    // Validate transliteration modifiers strictly.
    let mut modifiers = String::new();
    for modifier in modifiers_str.chars().take_while(|c: &char| c.is_ascii_alphanumeric()) {
        if is_transliteration_modifier(modifier) {
            modifiers.push(modifier);
        } else {
            return Err(TransliterationError::InvalidModifier(modifier));
        }
    }

    Ok((search, replacement, modifiers))
}

/// Get the closing delimiter for a given opening delimiter
fn get_closing_delimiter(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        _ => open,
    }
}

fn is_paired_open(ch: char) -> bool {
    get_closing_delimiter(ch) != ch
}

/// Whether `ch` may open a quote-like operator body.
///
/// Mirrors the lexer's own delimiter gate (`!is_alphanumeric() && !is_whitespace()`
/// in perl-lexer's `is_quote_delim`), so this module's notion of a valid delimiter
/// matches what the lexer will actually tokenize.
fn is_valid_delimiter(ch: char) -> bool {
    !ch.is_alphanumeric() && !ch.is_whitespace()
}

/// Valid `tr///` / `y///` modifiers: complement, delete, squeeze, return.
fn is_transliteration_modifier(ch: char) -> bool {
    matches!(ch, 'c' | 'd' | 's' | 'r')
}

fn starts_with_paired_delimiter(text: &str) -> Option<char> {
    let trimmed = text.trim_start();
    match trimmed.chars().next() {
        Some(ch) if is_paired_open(ch) => Some(ch),
        _ => None,
    }
}

/// Extract content between delimiters and return (content, rest).
///
/// Thin wrapper over [`extract_delimited_content_strict`] for callers that do not
/// need to distinguish "closed properly" from "ran off the end of the input".
fn extract_delimited_content(text: &str, open: char, close: char) -> (String, &str) {
    let (body, rest, _found_closing) = extract_delimited_content_strict(text, open, close);
    (body, rest)
}

/// Scan an unpaired (self-closing) body starting *after* its opening delimiter,
/// stopping at the first unescaped `closing` char.
///
/// Returns `(body, rest, found_closing)`, where `rest` begins just past the closing
/// delimiter. Unlike [`extract_unpaired_body_skip_strings`], embedded string literals
/// are not treated specially: a `closing` char inside `"..."` still ends the body.
fn extract_unpaired_body(text: &str, closing: char) -> (String, &str, bool) {
    let mut body = String::new();
    let mut escaped = false;
    let mut end_pos = text.len();
    let mut found_closing = false;

    for (i, ch) in text.char_indices() {
        if escaped {
            body.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                body.push(ch);
                escaped = true;
            }
            c if c == closing => {
                end_pos = i + ch.len_utf8();
                found_closing = true;
                break;
            }
            _ => body.push(ch),
        }
    }

    (body, &text[end_pos..], found_closing)
}

/// Lookahead helper: determine whether a `quote` char at byte `pos` in `text` is the
/// opening of a genuine inner string literal that protects `closing` delimiter chars.
///
/// Returns `Some((end_pos, true))` when:
///   - A matching closing `quote` is found on the SAME LINE (no `\n` crossed), AND
///   - The content between the two `quote` chars contains `closing`.
///   - `end_pos` is the byte offset just after the closing `quote`.
///
/// Returns `None` (or `Some((_, false))`) when:
///   - A newline or end of `text` is reached before the matching closing `quote`, OR
///   - The string content does not contain `closing`.
///
/// Stopping at newlines prevents cross-statement false positives in multiline source.
fn scan_inner_string(
    text: &str,
    pos: usize,
    quote: char,
    delimiter: char,
) -> Option<(usize, bool)> {
    if is_word_apostrophe(text, pos, quote) {
        return None;
    }
    // Adjacent quotes are literal replacement text (for example s/"/""/g),
    // not a string literal to skip while hunting for the replacement delimiter.
    if text.get(..pos).and_then(|prefix| prefix.chars().next_back()) == Some(quote) {
        return None;
    }
    let start = pos + quote.len_utf8();
    let rest = text.get(start..)?;
    if rest.starts_with(quote) {
        return None;
    }
    let mut escaped = false;
    let mut contains_delim = false;
    let mut end_of_string = None;
    let mut local_pos = start;
    for ch in rest.chars() {
        if escaped {
            escaped = false;
            local_pos += ch.len_utf8();
            continue;
        }
        if ch == '\\' {
            escaped = true;
            local_pos += ch.len_utf8();
            continue;
        }
        // Newline terminates the scan: inner string literals don't span lines.
        if ch == '\n' {
            return None;
        }
        if ch == delimiter {
            contains_delim = true;
        }
        if ch == quote {
            end_of_string = Some(local_pos + ch.len_utf8());
            break;
        }
        local_pos += ch.len_utf8();
    }
    end_of_string.map(|end| (end, contains_delim))
}

fn is_word_apostrophe(text: &str, pos: usize, quote: char) -> bool {
    quote == '\''
        && text
            .get(..pos)
            .and_then(|prefix| prefix.chars().next_back())
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Like `extract_unpaired_body` but skips over string literals (`"..."` / `'...'`)
/// so that the closing delimiter character inside a string is not mistaken for the
/// end of the replacement section.  Returns `(body, rest, found_closing)`.
///
/// Uses lookahead to determine whether a `'` or `"` is actually an inner string:
/// only enters string-skip mode when the candidate string (a) has a matching closing
/// quote on the same line AND (b) contains the closing delimiter in its content.
/// This prevents lone apostrophes (e.g. the `'` in `s/''/'/g`) from triggering
/// string-skip, which would cause replacement scanning to cross statement boundaries.
fn extract_unpaired_body_skip_strings(text: &str, closing: char) -> (String, &str, bool) {
    let mut body = String::new();
    let mut end_pos = text.len();
    let mut found_closing = false;
    let mut pos = 0usize;
    let mut escaped = false;

    while let Some(ch) = text.get(pos..).and_then(|s| s.chars().next()) {
        if escaped {
            body.push(ch);
            escaped = false;
            pos += ch.len_utf8();
            continue;
        }

        match ch {
            '\\' => {
                body.push(ch);
                escaped = true;
                pos += ch.len_utf8();
            }
            // Skip over string literals to avoid treating delimiter chars inside
            // "foo/bar" or 'a/b' as the closing delimiter of the replacement.
            //
            // Guard: only enter string-skip when lookahead confirms a matching closing
            // quote exists on the same line AND the content contains the closing delimiter.
            '"' | '\'' if ch != closing => {
                let quote = ch;
                match scan_inner_string(text, pos, quote, closing) {
                    Some((string_end, true)) => {
                        // String content contains the closing delimiter → skip the string.
                        let string_text = &text[pos..string_end];
                        body.push_str(string_text);
                        pos = string_end;
                    }
                    _ => {
                        // No closing quote on same line, or content has no delimiter:
                        // treat the opening quote as a literal character.
                        body.push(ch);
                        pos += ch.len_utf8();
                    }
                }
            }
            c if c == closing => {
                end_pos = pos + ch.len_utf8();
                found_closing = true;
                break;
            }
            _ => {
                body.push(ch);
                pos += ch.len_utf8();
            }
        }
    }

    (body, &text[end_pos..], found_closing)
}

fn extract_substitution_pattern_with_replacement_hint(
    text: &str,
    open: char,
    close: char,
) -> (String, &str, bool) {
    let mut chars = text.char_indices();

    // Skip opening delimiter
    if let Some((_, c)) = chars.next() {
        if c != open {
            return (String::new(), text, false);
        }
    } else {
        return (String::new(), "", false);
    }

    let mut body = String::new();
    let mut depth = 1usize;
    let mut escaped = false;
    let mut first_close_pos: Option<usize> = None;
    let mut first_body_len: usize = 0;

    for (i, ch) in chars {
        if escaped {
            body.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                body.push(ch);
                escaped = true;
            }
            c if c == open => {
                body.push(ch);
                depth += 1;
            }
            c if c == close => {
                if depth > 1 {
                    depth -= 1;
                    body.push(ch);
                    continue;
                }

                let rest = &text[i + ch.len_utf8()..];
                if first_close_pos.is_none() {
                    first_close_pos = Some(i + ch.len_utf8());
                    first_body_len = body.len();
                }

                if starts_with_paired_delimiter(rest).is_some() {
                    return (body, rest, true);
                }

                body.push(ch);
            }
            _ => body.push(ch),
        }
    }

    if let Some(pos) = first_close_pos {
        body.truncate(first_body_len);
        return (body, &text[pos..], true);
    }

    (body, "", false)
}

/// Byte offsets of every unescaped paired opening delimiter in `text`, in source order.
fn paired_open_positions(text: &str) -> Vec<(usize, char)> {
    let mut escaped = false;
    let mut positions = Vec::new();

    for (idx, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if is_paired_open(ch) {
            positions.push((idx, ch));
        }
    }

    positions
}

/// Split `text` at the paired group opening at `idx`, yielding
/// `(leading, group_body, trailing)` when that group is actually closed.
fn split_at_paired_open(text: &str, idx: usize, open: char) -> Option<(String, String, String)> {
    let (body, rest, found_closing) =
        extract_delimited_content_strict(&text[idx..], open, get_closing_delimiter(open));
    found_closing.then(|| (text[..idx].to_string(), body, rest.to_string()))
}

/// Recover a replacement from an unclosed non-paired pattern by treating the
/// **first** closed paired group as the replacement (e.g. `s/foo{bar}`).
fn split_unclosed_substitution_pattern(pattern: &str) -> Option<(String, String, String)> {
    paired_open_positions(pattern)
        .into_iter()
        .find_map(|(idx, ch)| split_at_paired_open(pattern, idx, ch))
}

/// Recover pattern/replacement/modifiers when the delimiter position held an
/// invalid (alphanumeric or whitespace) char, using the **last** closed paired group.
fn split_on_last_paired_delimiter(text: &str) -> Option<(String, String, String)> {
    paired_open_positions(text)
        .into_iter()
        .rev()
        .find_map(|(idx, ch)| split_at_paired_open(text, idx, ch))
}

/// Extract and validate substitution modifiers, returning only valid ones
///
/// Valid Perl substitution modifiers include:
/// - Core modifiers: g, i, m, s, x, o, e, r
/// - Charset modifiers (Perl 5.14+): a, d, l, u
/// - Additional modifiers: n (5.22+), p, c
///
/// This function provides panic-safe modifier validation for substitution operators,
/// filtering out invalid modifiers to prevent security vulnerabilities.
fn extract_substitution_modifiers(text: &str) -> String {
    text.chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .filter(|&c| {
            matches!(
                c,
                'g' | 'i'
                    | 'm'
                    | 's'
                    | 'x'
                    | 'o'
                    | 'e'
                    | 'r'
                    | 'a'
                    | 'd'
                    | 'l'
                    | 'u'
                    | 'n'
                    | 'p'
                    | 'c'
            )
        })
        .collect()
}

/// Validate substitution modifiers and return an error if any are invalid
///
/// Valid Perl substitution modifiers include:
/// - Core modifiers: g, i, m, s, x, o, e, r
/// - Charset modifiers (Perl 5.14+): a, d, l, u
/// - Additional modifiers: n (5.22+), p, c
///
/// # Arguments
///
/// * `modifiers_str` - The raw modifier string following the substitution operator
///
/// # Returns
///
/// * `Ok(String)` - The validated modifiers if all are valid
/// * `Err(char)` - The first invalid modifier character encountered
///
/// # Examples
///
/// ```ignore
/// assert!(validate_substitution_modifiers("gi").is_ok());
/// assert!(validate_substitution_modifiers("gia").is_ok());  // 'a' for ASCII mode
/// assert!(validate_substitution_modifiers("giz").is_err()); // 'z' is invalid
/// ```
pub fn validate_substitution_modifiers(modifiers_str: &str) -> Result<String, char> {
    let mut valid_modifiers = String::new();

    for c in modifiers_str.chars() {
        // Stop at non-alphabetic characters (end of modifiers)
        if !c.is_ascii_alphabetic() {
            // If it's whitespace or end of input, that's ok
            if c.is_whitespace() || c == ';' || c == '\n' || c == '\r' {
                break;
            }
            // Non-alphabetic, non-whitespace character in modifier position is invalid
            return Err(c);
        }

        // Check if it's a valid substitution modifier
        if matches!(
            c,
            'g' | 'i' | 'm' | 's' | 'x' | 'o' | 'e' | 'r' | 'a' | 'd' | 'l' | 'u' | 'n' | 'p' | 'c'
        ) {
            valid_modifiers.push(c);
        } else {
            // Invalid alphabetic modifier
            return Err(c);
        }
    }

    Ok(valid_modifiers)
}

// ============================================================================
// Canonical qw / q / qq operator content extractor (Wave D centralization)
// ============================================================================

/// Extract the inner content of a Perl quote-like operator expression.
///
/// This is the **canonical shared implementation** for all qw/q/qq delimiter
/// parsing in the workspace. Every consumer crate (perl-semantic-analyzer,
/// perl-workspace, perl-module, and this crate's own HIR model) delegates here.
///
/// # Behaviour
///
/// Strips `operator` from the start of `s`, skips optional whitespace and
/// line comments before its opening delimiter (Perl allows `qw (a b)` and
/// upstream `t/base/lex.t` uses `q # comment\n "b"#`),
/// reads the opening delimiter, maps it to its paired closing delimiter
/// (`(` → `)`, `{` → `}`, `[` → `]`, `<` → `>`; all others self-close),
/// rejects an alphanumeric or underscore character in delimiter position
/// (i.e. `qwfoo` → `None`), verifies the string ends with the closing
/// delimiter, and returns the interior slice.
///
/// # Examples
///
/// ```ignore
/// // Basic qw
/// assert_eq!(parse_quote_operator_content("qw(foo bar)", "qw"), Some("foo bar"));
/// // Space before delimiter
/// assert_eq!(parse_quote_operator_content("qw (foo bar)", "qw"), Some("foo bar"));
/// // Self-closing delimiter
/// assert_eq!(parse_quote_operator_content("qw/foo bar/", "qw"), Some("foo bar"));
/// // Bareword — rejected
/// assert_eq!(parse_quote_operator_content("qwfoo", "qw"), None);
/// ```
pub fn parse_quote_operator_content<'a>(s: &'a str, operator: &str) -> Option<&'a str> {
    let (open, content) = quote_operator_open_and_content(s, operator)?;
    let close = get_closing_delimiter(open);
    if !content.ends_with(close) {
        return None;
    }
    let end = content.len().checked_sub(close.len_utf8())?;
    Some(&content[..end])
}

pub(crate) fn quote_operator_open_and_content<'a>(
    s: &'a str,
    operator: &str,
) -> Option<(char, &'a str)> {
    // Perl allows whitespace and line comments between a quote-like operator
    // and its opening delimiter, e.g. `qw [a b]` or `q # comment\n "x"#`.
    let rest = skip_paired_replacement_gap(s.strip_prefix(operator)?);
    let open = rest.chars().next()?;
    if open.is_ascii_alphanumeric() || open == '_' {
        return None;
    }
    Some((open, rest.get(open.len_utf8()..)?))
}

/// Parse a `qw(...)` expression and return the whitespace-split word list.
///
/// This is a convenience wrapper around [`parse_quote_operator_content`] that
/// additionally splits the inner content on whitespace. Returns `None` when
/// the input is not a valid `qw` expression.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(
///     parse_qw_words("qw(Foo Bar Baz)"),
///     Some(vec!["Foo".to_string(), "Bar".to_string(), "Baz".to_string()])
/// );
/// assert_eq!(parse_qw_words("qwfoo"), None);
/// ```
pub fn parse_qw_words(s: &str) -> Option<Vec<String>> {
    let inner = parse_quote_operator_content(s, "qw")?;
    Some(inner.split_whitespace().map(str::to_string).collect())
}

// ============================================================================
// shared_scanner_invariants — pins the contracts the shared quote-scanning
// helpers must keep, so a future edit to one cannot silently diverge from the
// callers that now delegate to it.
// ============================================================================
#[cfg(test)]
mod shared_scanner_invariants {
    use super::*;

    /// Delimiter/body shapes exercised by the substitution, transliteration and
    /// regex extractors.
    const BODIES: &[(&str, char)] = &[
        ("/abc/rest", '/'),
        ("{a{b}c}rest", '{'),
        ("[a]rest", '['),
        ("(a)rest", '('),
        ("<a>rest", '<'),
        ("/a\\/b/rest", '/'),
        ("/unterminated", '/'),
        ("{unterminated", '{'),
        ("{a{b}", '{'),
        ("", '/'),
        ("x/mismatched/", '/'),
        ("//", '/'),
        ("{}", '{'),
    ];

    /// `extract_delimited_content` is a projection of the strict scanner: it must
    /// return exactly the strict body and rest, differing only by dropping the flag.
    #[test]
    fn extract_delimited_content_projects_strict_scanner() {
        for &(text, open) in BODIES {
            let close = get_closing_delimiter(open);
            let (body, rest) = extract_delimited_content(text, open, close);
            let (strict_body, strict_rest, _closed) =
                extract_delimited_content_strict(text, open, close);
            assert_eq!(
                (body.as_str(), rest),
                (strict_body.as_str(), strict_rest),
                "extract_delimited_content({text:?}, {open:?}) diverged from the strict scanner"
            );
        }
    }

    /// The unpaired scanner starts *after* the opening delimiter, honours escapes,
    /// and reports whether it actually found the closing delimiter.
    #[test]
    fn extract_unpaired_body_reports_closure_and_honours_escapes() {
        assert_eq!(extract_unpaired_body("abc/tail", '/'), ("abc".to_string(), "tail", true));
        // An escaped delimiter does not close the body, and the backslash is kept.
        assert_eq!(extract_unpaired_body("a\\/b/tail", '/'), ("a\\/b".to_string(), "tail", true));
        // No closing delimiter: whole input is body, rest is empty, found_closing false.
        assert_eq!(extract_unpaired_body("abc", '/'), ("abc".to_string(), "", false));
        // Immediate close yields an empty body.
        assert_eq!(extract_unpaired_body("/tail", '/'), (String::new(), "tail", true));
    }

    /// Unlike the strict substitution path, the transliteration replacement scan
    /// deliberately does NOT treat embedded quotes as protected string literals —
    /// `tr` operates on character lists, not on Perl expressions.
    #[test]
    fn transliteration_replacement_does_not_protect_inner_strings() {
        let (body, _rest, closed) = extract_unpaired_body("\"a/b\"/tail", '/');
        assert_eq!(body, "\"a", "tr replacement must stop at the first unescaped delimiter");
        assert!(closed);
    }

    /// `is_valid_delimiter` mirrors the lexer's quote-delimiter gate.
    #[test]
    fn is_valid_delimiter_matches_lexer_gate() {
        for ch in ['/', '{', '[', '(', '<', '#', '|', '!', '\'', '"', '@', '-'] {
            assert!(is_valid_delimiter(ch), "{ch:?} should be a valid quote delimiter");
        }
        for ch in ['a', 'Z', '0', '9', ' ', '\t', '\n'] {
            assert!(!is_valid_delimiter(ch), "{ch:?} must be rejected in delimiter position");
        }
    }

    /// `is_paired_open` must stay derived from the single delimiter table.
    #[test]
    fn is_paired_open_agrees_with_closing_delimiter_table() {
        for ch in ['(', '[', '{', '<', ')', ']', '}', '>', '/', '#', '|', 'a', ' '] {
            assert_eq!(
                is_paired_open(ch),
                get_closing_delimiter(ch) != ch,
                "is_paired_open({ch:?}) must agree with get_closing_delimiter"
            );
        }
    }

    /// Escaped opening delimiters are not split candidates.
    #[test]
    fn paired_open_positions_skips_escaped_openers() {
        assert_eq!(paired_open_positions("a{b}c"), vec![(1, '{')]);
        assert_eq!(paired_open_positions("a\\{b}c"), vec![]);
        assert_eq!(paired_open_positions("{a}[b]"), vec![(0, '{'), (3, '[')]);
        assert_eq!(paired_open_positions("no openers here"), vec![]);
    }

    /// The two recovery splitters differ only in which closed group they select.
    #[test]
    fn recovery_splitters_select_first_and_last_closed_group() {
        let text = "a{one}b{two}c";
        assert_eq!(
            split_unclosed_substitution_pattern(text),
            Some(("a".to_string(), "one".to_string(), "b{two}c".to_string()))
        );
        assert_eq!(
            split_on_last_paired_delimiter(text),
            Some(("a{one}b".to_string(), "two".to_string(), "c".to_string()))
        );
        // An unclosed group is not a split candidate for either splitter.
        assert_eq!(split_unclosed_substitution_pattern("a{unclosed"), None);
        assert_eq!(split_on_last_paired_delimiter("a{unclosed"), None);
    }

    /// `parse_quote_operator_content` must use the shared delimiter table rather
    /// than an inlined copy of it.
    #[test]
    fn quote_operator_content_uses_shared_delimiter_table() {
        for (input, expected) in [
            ("qw(a b)", Some("a b")),
            ("qw[a b]", Some("a b")),
            ("qw{a b}", Some("a b")),
            ("qw<a b>", Some("a b")),
            ("qw/a b/", Some("a b")),
            ("qw|a b|", Some("a b")),
            // Mismatched close for a paired opener is rejected.
            ("qw(a b]", None),
            // Alphanumeric in delimiter position is a bareword, not a quote op.
            ("qwfoo", None),
        ] {
            assert_eq!(
                parse_quote_operator_content(input, "qw"),
                expected,
                "parse_quote_operator_content({input:?})"
            );
        }
    }
}

// ============================================================================
// paired_delimiter_conformance — inline tests for get_closing_delimiter
//
// This test block is one of THREE that together form the conformance matrix
// for paired-delimiter implementations across the workspace (#1320).
//
// The same input set is tested in:
//   - crates/perl-lexer/src/quote_handler.rs                   (paired_close)
//   - crates/perl-dap/src/inline_values/code_mask.rs           (matching_delimiter)
//
// Normalized contract: each impl maps to (close_char: char, is_paired: bool).
//   - This impl: get_closing_delimiter(c) → cl; is_paired = (cl != c)
// ============================================================================

// Source-backed regex-family geometry kept inside quote.rs so the
// existing RIPR suppression on this path covers the seam without a
// new no_static_path module surface.
mod regex_family_geometry {
    use perl_ast::SourceLocation;

    /// Regex-family operator recognized by the geometry scanner.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum RegexFamilyOperator {
        /// Bare match form, such as `/pattern/`.
        BareMatch,
        /// Explicit match form, such as `m/pattern/`.
        Match,
        /// Compiled regex form, such as `qr/pattern/`.
        QuoteRegex,
        /// Substitution form, such as `s/pattern/replacement/`.
        Substitution,
        /// Transliteration form using `tr`.
        Transliteration,
        /// Transliteration form using the `y` alias.
        TransliterationAlias,
    }

    /// Exact source geometry for one delimiter-bounded body.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub struct DelimitedBodyGeometry {
        /// Body text without its surrounding delimiters.
        pub text: String,
        /// Exact body range in the original source.
        pub range: SourceLocation,
        /// Opening delimiter range. For unpaired two-body operators, the pattern's
        /// closing delimiter is also the replacement's opening delimiter.
        pub opening_delimiter_range: SourceLocation,
        /// Closing delimiter range, or `None` for a partial/unclosed body.
        pub closing_delimiter_range: Option<SourceLocation>,
    }

    impl DelimitedBodyGeometry {
        /// Whether this body has a source-backed closing delimiter.
        #[must_use]
        pub const fn is_closed(&self) -> bool {
            self.closing_delimiter_range.is_some()
        }
    }

    /// Exact source geometry for the raw modifier sequence.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub struct ModifierGeometry {
        /// Raw adjacent alphabetic modifier spelling.
        pub text: String,
        /// Exact modifier range in the original source.
        pub range: SourceLocation,
    }

    /// Source-backed geometry for one regex-family operator occurrence.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub struct RegexFamilyGeometry {
        /// Operator family.
        pub operator: RegexFamilyOperator,
        /// Operator prefix range. Bare matches use an empty range at the opening delimiter.
        pub operator_range: SourceLocation,
        /// Exact parsed operator extent, excluding unrelated following source.
        pub full_range: SourceLocation,
        /// Pattern or transliteration search-list body.
        pub pattern: DelimitedBodyGeometry,
        /// Substitution replacement or transliteration replacement-list body.
        pub replacement: Option<DelimitedBodyGeometry>,
        /// Raw modifier spelling and range.
        pub modifiers: ModifierGeometry,
    }

    #[derive(Debug, Clone, Copy)]
    struct OperatorPrefix {
        operator: RegexFamilyOperator,
        len: usize,
    }

    #[derive(Debug, Clone, Copy)]
    struct ScannedBody {
        open_offset: usize,
        body_start: usize,
        body_end: usize,
        close_offset: Option<usize>,
        rest_offset: usize,
        delimiter: char,
    }

    /// Extract exact source geometry for a regex, match, substitution, or
    /// transliteration spelling.
    ///
    /// `source_start` is the absolute byte offset where `text` begins. The function
    /// accepts trailing source after the operator and stops `full_range` after the
    /// adjacent modifier run. It returns `None` when `text` does not begin with a
    /// recognizable regex-family operator or when absolute offsets overflow.
    #[must_use]
    pub fn extract_regex_family_geometry(
        text: &str,
        source_start: usize,
    ) -> Option<RegexFamilyGeometry> {
        let prefix = identify_operator(text)?;
        let delimiter_offset =
            if prefix.len == 0 { 0 } else { skip_operator_gap(text, prefix.len) };
        let delimiter = text.get(delimiter_offset..)?.chars().next()?;
        if !is_valid_delimiter(delimiter) {
            return None;
        }

        let pattern_scan = scan_delimited(text, delimiter_offset, delimiter)?;
        let pattern = body_geometry(text, source_start, pattern_scan)?;

        let replacement_scan = match prefix.operator {
            RegexFamilyOperator::Substitution => {
                scan_second_body(text, pattern_scan, SecondBodyKind::Substitution)
            }
            RegexFamilyOperator::Transliteration | RegexFamilyOperator::TransliterationAlias => {
                scan_second_body(text, pattern_scan, SecondBodyKind::Transliteration)
            }
            RegexFamilyOperator::BareMatch
            | RegexFamilyOperator::Match
            | RegexFamilyOperator::QuoteRegex => None,
        };

        let replacement = match replacement_scan {
            Some(scan) => Some(body_geometry(text, source_start, scan)?),
            None => None,
        };
        let modifiers_start =
            replacement_scan.map_or(pattern_scan.rest_offset, |scan| scan.rest_offset);
        let modifiers_end = modifier_end(text, modifiers_start);
        let operator_range = absolute_range(source_start, 0, prefix.len)?;
        let full_range = absolute_range(source_start, 0, modifiers_end)?;
        let modifiers = ModifierGeometry {
            text: text.get(modifiers_start..modifiers_end)?.to_string(),
            range: absolute_range(source_start, modifiers_start, modifiers_end)?,
        };

        Some(RegexFamilyGeometry {
            operator: prefix.operator,
            operator_range,
            full_range,
            pattern,
            replacement,
            modifiers,
        })
    }

    fn identify_operator(text: &str) -> Option<OperatorPrefix> {
        if text.starts_with("qr") {
            return Some(OperatorPrefix { operator: RegexFamilyOperator::QuoteRegex, len: 2 });
        }
        if text.starts_with("tr") {
            return Some(OperatorPrefix { operator: RegexFamilyOperator::Transliteration, len: 2 });
        }
        if text.starts_with('m') {
            return Some(OperatorPrefix { operator: RegexFamilyOperator::Match, len: 1 });
        }
        if text.starts_with('s') {
            return Some(OperatorPrefix { operator: RegexFamilyOperator::Substitution, len: 1 });
        }
        if text.starts_with('y') {
            return Some(OperatorPrefix {
                operator: RegexFamilyOperator::TransliterationAlias,
                len: 1,
            });
        }

        let delimiter = text.chars().next()?;
        is_valid_delimiter(delimiter)
            .then_some(OperatorPrefix { operator: RegexFamilyOperator::BareMatch, len: 0 })
    }

    fn skip_operator_gap(text: &str, mut offset: usize) -> usize {
        let mut comment_eligible = false;

        loop {
            let before_whitespace = offset;
            while let Some(ch) = text.get(offset..).and_then(|rest| rest.chars().next()) {
                if !ch.is_whitespace() {
                    break;
                }
                offset = offset.saturating_add(ch.len_utf8());
            }
            comment_eligible |= offset != before_whitespace;

            if comment_eligible && text.get(offset..).is_some_and(|rest| rest.starts_with('#')) {
                offset = after_line_comment_offset(text, offset);
                comment_eligible = true;
                continue;
            }

            return offset;
        }
    }

    fn after_line_comment_offset(text: &str, offset: usize) -> usize {
        let Some(rest) = text.get(offset..) else {
            return text.len();
        };
        for (index, ch) in rest.char_indices() {
            if matches!(ch, '\n' | '\r') {
                return offset.saturating_add(index).saturating_add(ch.len_utf8());
            }
        }
        text.len()
    }

    fn scan_delimited(text: &str, open_offset: usize, delimiter: char) -> Option<ScannedBody> {
        let open = text.get(open_offset..)?.chars().next()?;
        if open != delimiter {
            return None;
        }
        let close = closing_delimiter(delimiter);
        let body_start = open_offset.checked_add(delimiter.len_utf8())?;
        let paired = delimiter != close;
        let mut depth = 1usize;
        let mut escaped = false;

        for (relative, ch) in text.get(body_start..)?.char_indices() {
            let offset = body_start.checked_add(relative)?;
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if paired && ch == delimiter {
                depth = depth.saturating_add(1);
                continue;
            }
            if ch == close {
                if paired {
                    depth = depth.saturating_sub(1);
                    if depth != 0 {
                        continue;
                    }
                }
                let rest_offset = offset.checked_add(ch.len_utf8())?;
                return Some(ScannedBody {
                    open_offset,
                    body_start,
                    body_end: offset,
                    close_offset: Some(offset),
                    rest_offset,
                    delimiter,
                });
            }
        }

        Some(ScannedBody {
            open_offset,
            body_start,
            body_end: text.len(),
            close_offset: None,
            rest_offset: text.len(),
            delimiter,
        })
    }

    #[derive(Debug, Clone, Copy)]
    enum SecondBodyKind {
        Substitution,
        Transliteration,
    }

    fn scan_second_body(
        text: &str,
        pattern: ScannedBody,
        kind: SecondBodyKind,
    ) -> Option<ScannedBody> {
        let pattern_close = pattern.close_offset?;
        let pattern_close_end = pattern.rest_offset;
        let pattern_is_paired = closing_delimiter(pattern.delimiter) != pattern.delimiter;

        if !pattern_is_paired {
            return Some(scan_unpaired_body(
                text,
                pattern_close,
                pattern_close_end,
                pattern.delimiter,
            ));
        }

        let replacement_open = skip_operator_gap(text, pattern.rest_offset);
        let delimiter = text.get(replacement_open..)?.chars().next()?;
        let delimiter_allowed = match kind {
            SecondBodyKind::Substitution => !delimiter.is_whitespace(),
            SecondBodyKind::Transliteration => is_valid_delimiter(delimiter),
        };
        if !delimiter_allowed {
            return None;
        }
        scan_delimited(text, replacement_open, delimiter)
    }

    fn scan_unpaired_body(
        text: &str,
        shared_open_offset: usize,
        body_start: usize,
        delimiter: char,
    ) -> ScannedBody {
        let mut escaped = false;
        let Some(rest) = text.get(body_start..) else {
            return ScannedBody {
                open_offset: shared_open_offset,
                body_start,
                body_end: body_start,
                close_offset: None,
                rest_offset: body_start,
                delimiter,
            };
        };

        let mut relative = 0usize;
        while let Some(ch) = rest.get(relative..).and_then(|tail| tail.chars().next()) {
            let Some(offset) = body_start.checked_add(relative) else {
                break;
            };
            if escaped {
                escaped = false;
                relative = relative.saturating_add(ch.len_utf8());
                continue;
            }
            if ch == '\\' {
                escaped = true;
                relative = relative.saturating_add(ch.len_utf8());
                continue;
            }
            if matches!(ch, '\'' | '"')
                && ch != delimiter
                && !is_word_apostrophe(text, offset, ch)
                && let Some(string_end) = scan_inner_string(text, offset, ch, delimiter)
            {
                let Some(next_relative) = string_end.checked_sub(body_start) else {
                    break;
                };
                relative = next_relative;
                continue;
            }
            if ch == delimiter {
                let rest_offset = offset.saturating_add(ch.len_utf8());
                return ScannedBody {
                    open_offset: shared_open_offset,
                    body_start,
                    body_end: offset,
                    close_offset: Some(offset),
                    rest_offset,
                    delimiter,
                };
            }
            relative = relative.saturating_add(ch.len_utf8());
        }

        ScannedBody {
            open_offset: shared_open_offset,
            body_start,
            body_end: text.len(),
            close_offset: None,
            rest_offset: text.len(),
            delimiter,
        }
    }

    fn scan_inner_string(text: &str, start: usize, quote: char, delimiter: char) -> Option<usize> {
        let content_start = start.checked_add(quote.len_utf8())?;
        let mut escaped = false;
        let mut contains_delimiter = false;

        for (relative, ch) in text.get(content_start..)?.char_indices() {
            let offset = content_start.checked_add(relative)?;
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '\n' {
                return None;
            }
            if ch == delimiter {
                contains_delimiter = true;
            }
            if ch == quote {
                return contains_delimiter.then_some(offset.checked_add(ch.len_utf8())?);
            }
        }

        None
    }

    fn is_word_apostrophe(text: &str, position: usize, quote: char) -> bool {
        quote == '\''
            && text
                .get(..position)
                .and_then(|prefix| prefix.chars().next_back())
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    }

    fn body_geometry(
        text: &str,
        source_start: usize,
        scan: ScannedBody,
    ) -> Option<DelimitedBodyGeometry> {
        let delimiter_len = scan.delimiter.len_utf8();
        let opening_delimiter_range = absolute_range(
            source_start,
            scan.open_offset,
            scan.open_offset.checked_add(delimiter_len)?,
        )?;
        let closing_delimiter_range = match scan.close_offset {
            Some(offset) => {
                Some(absolute_range(source_start, offset, offset.checked_add(delimiter_len)?)?)
            }
            None => None,
        };

        Some(DelimitedBodyGeometry {
            text: text.get(scan.body_start..scan.body_end)?.to_string(),
            range: absolute_range(source_start, scan.body_start, scan.body_end)?,
            opening_delimiter_range,
            closing_delimiter_range,
        })
    }

    fn modifier_end(text: &str, start: usize) -> usize {
        let mut end = start;
        let Some(rest) = text.get(start..) else {
            return start;
        };
        for ch in rest.chars() {
            if !ch.is_ascii_alphabetic() {
                break;
            }
            end = end.saturating_add(ch.len_utf8());
        }
        end
    }

    fn absolute_range(
        source_start: usize,
        relative_start: usize,
        relative_end: usize,
    ) -> Option<SourceLocation> {
        if relative_start > relative_end {
            return None;
        }
        Some(SourceLocation {
            start: source_start.checked_add(relative_start)?,
            end: source_start.checked_add(relative_end)?,
        })
    }

    fn closing_delimiter(open: char) -> char {
        match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '<' => '>',
            _ => open,
        }
    }

    fn is_valid_delimiter(ch: char) -> bool {
        !ch.is_alphanumeric() && !ch.is_whitespace()
    }
}

pub use regex_family_geometry::*;

#[cfg(test)]
mod paired_delimiter_conformance {
    use super::get_closing_delimiter;

    /// Normalize `get_closing_delimiter` to the shared `(close_char, is_paired)` shape.
    fn normalize(open: char) -> (char, bool) {
        let close = get_closing_delimiter(open);
        (close, close != open)
    }

    /// The shared conformance matrix.
    /// Each entry is `(open_char, expected_close, expected_is_paired)`.
    const MATRIX: &[(char, char, bool)] = &[
        // --- Paired openers -------------------------------------------
        ('(', ')', true),
        ('[', ']', true),
        ('{', '}', true),
        ('<', '>', true),
        // --- Self-delimiting: common punctuation ----------------------
        ('/', '/', false),
        ('#', '#', false),
        ('|', '|', false),
        ('!', '!', false),
        (',', ',', false),
        ('%', '%', false),
        ('~', '~', false),
        ('.', '.', false),
        (':', ':', false),
        (';', ';', false),
        // --- Self-delimiting: quote-adjacent chars --------------------
        ('\'', '\'', false),
        ('"', '"', false),
        // --- Self-delimiting: less-common punctuation ----------------
        ('@', '@', false),
        ('$', '$', false),
        ('^', '^', false),
        ('&', '&', false),
        ('*', '*', false),
        ('+', '+', false),
        ('-', '-', false),
        ('=', '=', false),
        ('?', '?', false),
        // --- Closing chars used as openers (not paired) --------------
        // Note: Perl does NOT auto-pair ) ] } > as openers.
        (')', ')', false),
        (']', ']', false),
        ('}', '}', false),
        ('>', '>', false),
    ];

    #[test]
    fn get_closing_delimiter_agrees_with_conformance_matrix() {
        for &(open, expected_close, expected_paired) in MATRIX {
            let (got_close, got_paired) = normalize(open);
            assert_eq!(
                got_close, expected_close,
                "perl-parser-core get_closing_delimiter({open:?}): close char mismatch \
                 (got {got_close:?}, expected {expected_close:?})"
            );
            assert_eq!(
                got_paired, expected_paired,
                "perl-parser-core get_closing_delimiter({open:?}): is_paired mismatch \
                 (got {got_paired}, expected {expected_paired})"
            );
        }
    }

    #[test]
    fn get_closing_delimiter_paired_openers_return_distinct_close() {
        // The four Perl auto-paired delimiters must map to a different close char.
        for (open, expected_close) in [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')] {
            let close = get_closing_delimiter(open);
            assert_eq!(
                close, expected_close,
                "get_closing_delimiter({open:?}) should return {expected_close:?}"
            );
        }
    }

    #[test]
    fn get_closing_delimiter_self_delimiters_return_self() {
        // Self-delimiting chars must return themselves unchanged.
        let self_delims = ['/', '#', '|', '!', ',', '%', '~', '.', ':', ';', '\'', '"', '@'];
        for open in self_delims {
            let close = get_closing_delimiter(open);
            assert_eq!(
                close, open,
                "get_closing_delimiter({open:?}) should return self for a self-delimiting char"
            );
        }
    }

    #[test]
    fn get_closing_delimiter_closing_chars_used_as_openers_return_self() {
        // Perl does NOT auto-pair ) ] } > as openers; they must return self.
        for close in [')', ']', '}', '>'] {
            let result = get_closing_delimiter(close);
            assert_eq!(
                result, close,
                "get_closing_delimiter({close:?}) should return self — \
                 closing chars are not themselves paired openers"
            );
        }
    }
}
