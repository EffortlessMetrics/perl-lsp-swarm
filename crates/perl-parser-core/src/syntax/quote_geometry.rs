//! Source-backed geometry for Perl regex-family quote operators.
//!
//! The parser's legacy quote helpers return detached strings. This module keeps
//! the same source spellings while recording the exact byte ranges for pattern,
//! replacement, delimiter, and modifier regions. Ranges are absolute offsets in
//! the original source when callers provide the enclosing token start.

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
    let delimiter_offset = if prefix.len == 0 {
        0
    } else {
        skip_operator_gap(text, prefix.len)
    };
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
        RegexFamilyOperator::Transliteration
        | RegexFamilyOperator::TransliterationAlias => {
            scan_second_body(text, pattern_scan, SecondBodyKind::Transliteration)
        }
        RegexFamilyOperator::BareMatch
        | RegexFamilyOperator::Match
        | RegexFamilyOperator::QuoteRegex => None,
    };

    let replacement = replacement_scan
        .map(|scan| body_geometry(text, source_start, scan))
        .transpose()?;
    let modifiers_start = replacement_scan.map_or(pattern_scan.rest_offset, |scan| scan.rest_offset);
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
    for (relative, ch) in text.get(body_start..).unwrap_or_default().char_indices() {
        let Some(offset) = body_start.checked_add(relative) else {
            break;
        };
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
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

fn body_geometry(
    text: &str,
    source_start: usize,
    scan: ScannedBody,
) -> Option<DelimitedBodyGeometry> {
    let delimiter_len = scan.delimiter.len_utf8();
    let opening_delimiter_range =
        absolute_range(source_start, scan.open_offset, scan.open_offset.checked_add(delimiter_len)?)?;
    let closing_delimiter_range = scan
        .close_offset
        .map(|offset| {
            absolute_range(source_start, offset, offset.checked_add(delimiter_len)?)
        })
        .transpose()?;

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
