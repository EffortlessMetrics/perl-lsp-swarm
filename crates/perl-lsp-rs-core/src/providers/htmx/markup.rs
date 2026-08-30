//! Bounded raw-markup context recognition for htmx attribute names.

/// Maximum number of source bytes inspected before the completion position.
pub const MAX_MARKUP_LOOKBACK_BYTES: usize = 8 * 1024;

/// Proven htmx attribute-name slot in raw markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmxAttributeNameContext<'a> {
    /// Attribute prefix already typed by the user.
    pub prefix: &'a str,
    /// Byte offset where the active attribute name starts.
    pub prefix_start: usize,
    /// Byte offset of the completion position.
    pub position: usize,
}

/// Return an htmx attribute-name context when `position` is inside a proven
/// open HTML start tag.
///
/// The recognizer examines only a bounded suffix of the document. It rejects
/// comments, closing and declaration tags, processing instructions, quoted and
/// unquoted values, completed tags, common Perl-template code regions, and
/// malformed start tags.
#[must_use]
pub fn htmx_attribute_name_context(
    source: &str,
    position: usize,
) -> Option<HtmxAttributeNameContext<'_>> {
    if position > source.len() || !source.is_char_boundary(position) {
        return None;
    }

    let window_start = bounded_window_start(source, position);
    let window = source.get(window_start..position)?;
    if current_line_is_template_code(window) {
        return None;
    }

    let tag_start = open_start_tag_offset(window)?;
    let tag_body = window.get(tag_start + 1..)?;
    let token_start = active_attribute_name_start(tag_body)?;
    let prefix = tag_body.get(token_start..)?;
    if !is_htmx_attribute_prefix(prefix) {
        return None;
    }

    Some(HtmxAttributeNameContext {
        prefix,
        prefix_start: window_start + tag_start + 1 + token_start,
        position,
    })
}

fn bounded_window_start(source: &str, position: usize) -> usize {
    let mut start = position.saturating_sub(MAX_MARKUP_LOOKBACK_BYTES);
    while start < position && !source.is_char_boundary(start) {
        start += 1;
    }
    start
}

fn current_line_is_template_code(window: &str) -> bool {
    let line = window.rsplit_once('\n').map_or(window, |(_, suffix)| suffix);
    line.trim_start_matches([' ', '\t']).starts_with('%')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkupState {
    Text,
    Comment,
    StartTag { start: usize, quote: Option<u8> },
    IgnoredTag { quote: Option<u8> },
    TemplatePercent,
    TemplateBracket,
    MasonComponent,
    InvalidTag,
}

fn open_start_tag_offset(window: &str) -> Option<usize> {
    let bytes = window.as_bytes();
    let mut state = MarkupState::Text;
    let mut index = 0usize;

    while index < bytes.len() {
        state = match state {
            MarkupState::Text => {
                if starts_with(bytes, index, b"<!--") {
                    index += 4;
                    MarkupState::Comment
                } else if starts_with(bytes, index, b"<%") {
                    index += 2;
                    MarkupState::TemplatePercent
                } else if starts_with(bytes, index, b"[%") {
                    index += 2;
                    MarkupState::TemplateBracket
                } else if starts_with(bytes, index, b"<&") {
                    index += 2;
                    MarkupState::MasonComponent
                } else if bytes.get(index) == Some(&b'<') {
                    let next = bytes.get(index + 1).copied();
                    index += 1;
                    match next {
                        Some(byte) if byte.is_ascii_alphabetic() => {
                            MarkupState::StartTag { start: index - 1, quote: None }
                        }
                        Some(b'/' | b'!' | b'?') => MarkupState::IgnoredTag { quote: None },
                        _ => MarkupState::Text,
                    }
                } else {
                    index += 1;
                    MarkupState::Text
                }
            }
            MarkupState::Comment => {
                if starts_with(bytes, index, b"-->") {
                    index += 3;
                    MarkupState::Text
                } else {
                    index += 1;
                    MarkupState::Comment
                }
            }
            MarkupState::TemplatePercent => {
                if starts_with(bytes, index, b"%>") {
                    index += 2;
                    MarkupState::Text
                } else {
                    index += 1;
                    MarkupState::TemplatePercent
                }
            }
            MarkupState::TemplateBracket => {
                if starts_with(bytes, index, b"%]") {
                    index += 2;
                    MarkupState::Text
                } else {
                    index += 1;
                    MarkupState::TemplateBracket
                }
            }
            MarkupState::MasonComponent => {
                if starts_with(bytes, index, b"&>") {
                    index += 2;
                    MarkupState::Text
                } else {
                    index += 1;
                    MarkupState::MasonComponent
                }
            }
            MarkupState::StartTag { start, quote } => match quote {
                Some(delimiter) => {
                    let closes_quote = bytes.get(index) == Some(&delimiter);
                    index += 1;
                    MarkupState::StartTag {
                        start,
                        quote: if closes_quote { None } else { Some(delimiter) },
                    }
                }
                None => match bytes.get(index).copied() {
                    Some(delimiter @ (b'"' | b'\'')) => {
                        index += 1;
                        MarkupState::StartTag { start, quote: Some(delimiter) }
                    }
                    Some(b'>') => {
                        index += 1;
                        MarkupState::Text
                    }
                    Some(b'<') => {
                        index += 1;
                        MarkupState::InvalidTag
                    }
                    Some(_) => {
                        index += 1;
                        MarkupState::StartTag { start, quote: None }
                    }
                    None => MarkupState::StartTag { start, quote: None },
                },
            },
            MarkupState::IgnoredTag { quote } => match quote {
                Some(delimiter) => {
                    let closes_quote = bytes.get(index) == Some(&delimiter);
                    index += 1;
                    MarkupState::IgnoredTag {
                        quote: if closes_quote { None } else { Some(delimiter) },
                    }
                }
                None => match bytes.get(index).copied() {
                    Some(delimiter @ (b'"' | b'\'')) => {
                        index += 1;
                        MarkupState::IgnoredTag { quote: Some(delimiter) }
                    }
                    Some(b'>') => {
                        index += 1;
                        MarkupState::Text
                    }
                    Some(_) => {
                        index += 1;
                        MarkupState::IgnoredTag { quote: None }
                    }
                    None => MarkupState::IgnoredTag { quote: None },
                },
            },
            MarkupState::InvalidTag => {
                if bytes.get(index) == Some(&b'>') {
                    index += 1;
                    MarkupState::Text
                } else {
                    index += 1;
                    MarkupState::InvalidTag
                }
            }
        };
    }

    match state {
        MarkupState::StartTag { start, quote: None } => Some(start),
        _ => None,
    }
}

fn starts_with(bytes: &[u8], index: usize, needle: &[u8]) -> bool {
    bytes.get(index..).is_some_and(|suffix| suffix.starts_with(needle))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttributeState {
    Between,
    Name { start: usize },
    AfterName,
    BeforeValue,
    QuotedValue { delimiter: u8 },
    UnquotedValue,
    Invalid,
}

fn active_attribute_name_start(tag_body: &str) -> Option<usize> {
    let bytes = tag_body.as_bytes();
    let mut index = 0usize;

    while bytes.get(index).is_some_and(|byte| is_tag_name_byte(*byte)) {
        index += 1;
    }
    if index == 0 || !bytes.get(index).is_some_and(|byte| is_html_space(*byte)) {
        return None;
    }

    let mut state = AttributeState::Between;
    while index < bytes.len() {
        let Some(byte) = bytes.get(index).copied() else {
            return None;
        };
        state = match state {
            AttributeState::Between => {
                if is_html_space(byte) {
                    AttributeState::Between
                } else if is_attribute_name_byte(byte) {
                    AttributeState::Name { start: index }
                } else {
                    AttributeState::Invalid
                }
            }
            AttributeState::Name { start } => {
                if is_attribute_name_byte(byte) {
                    AttributeState::Name { start }
                } else if is_html_space(byte) {
                    AttributeState::AfterName
                } else if byte == b'=' {
                    AttributeState::BeforeValue
                } else {
                    AttributeState::Invalid
                }
            }
            AttributeState::AfterName => {
                if is_html_space(byte) {
                    AttributeState::AfterName
                } else if byte == b'=' {
                    AttributeState::BeforeValue
                } else if is_attribute_name_byte(byte) {
                    AttributeState::Name { start: index }
                } else {
                    AttributeState::Invalid
                }
            }
            AttributeState::BeforeValue => {
                if is_html_space(byte) {
                    AttributeState::BeforeValue
                } else if matches!(byte, b'"' | b'\'') {
                    AttributeState::QuotedValue { delimiter: byte }
                } else if is_unquoted_value_byte(byte) {
                    AttributeState::UnquotedValue
                } else {
                    AttributeState::Invalid
                }
            }
            AttributeState::QuotedValue { delimiter } => {
                if byte == delimiter {
                    AttributeState::Between
                } else {
                    AttributeState::QuotedValue { delimiter }
                }
            }
            AttributeState::UnquotedValue => {
                if is_html_space(byte) {
                    AttributeState::Between
                } else if is_unquoted_value_byte(byte) {
                    AttributeState::UnquotedValue
                } else {
                    AttributeState::Invalid
                }
            }
            AttributeState::Invalid => AttributeState::Invalid,
        };
        index += 1;
    }

    match state {
        AttributeState::Name { start } => Some(start),
        _ => None,
    }
}

fn is_tag_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
}

fn is_html_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

fn is_attribute_name_byte(byte: u8) -> bool {
    byte >= 0x20
        && !is_html_space(byte)
        && !matches!(byte, b'"' | b'\'' | b'>' | b'/' | b'=' | b'<')
}

fn is_unquoted_value_byte(byte: u8) -> bool {
    byte >= 0x20
        && !is_html_space(byte)
        && !matches!(byte, b'"' | b'\'' | b'<' | b'=' | b'>' | b'`')
}

fn is_htmx_attribute_prefix(prefix: &str) -> bool {
    prefix.bytes().all(is_htmx_attribute_byte)
        && (prefix.eq_ignore_ascii_case("hx")
            || starts_with_ignore_ascii_case(prefix, "hx-")
            || prefix.eq_ignore_ascii_case("data-hx")
            || starts_with_ignore_ascii_case(prefix, "data-hx-"))
}

fn is_htmx_attribute_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::{MAX_MARKUP_LOOKBACK_BYTES, htmx_attribute_name_context};

    #[test]
    fn recognizes_multiline_attribute_name_and_exact_range() {
        let source = "<my-component\n  class=\"x>y\"\n  data-hx-re";
        let context = htmx_attribute_name_context(source, source.len());

        assert!(context.is_some_and(|context| {
            context.prefix == "data-hx-re"
                && context.prefix_start == source.len() - "data-hx-re".len()
                && context.position == source.len()
        }));
    }

    #[test]
    fn rejects_non_attribute_positions() {
        for source in [
            "hx-",
            "<div> hx-",
            "<!-- <div hx-",
            "</div hx-",
            "<!doctype html hx-",
            "<?xml hx-",
            "<div title=\"hx-",
            "<div foo=hx-",
            "<div <span hx-",
            "<% my $x = '<div hx-'",
            "[% '<div hx-'",
            "<& component, value => '<div hx-'",
            "% my $x = '<div hx-'",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_none(),
                "unexpected context for {source:?}"
            );
        }
    }

    #[test]
    fn accepts_attribute_names_after_quoted_and_unquoted_values() {
        for source in [
            "<div title=\"x>y\" hx-",
            "<div title='x>y' hx-",
            "<div class=primary hx-",
            "<div disabled hx-",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_some(),
                "missing context for {source:?}"
            );
        }
    }

    #[test]
    fn rejects_start_tags_beyond_the_bounded_window() {
        let source = format!("<div {} hx-", "a".repeat(MAX_MARKUP_LOOKBACK_BYTES));

        assert!(htmx_attribute_name_context(&source, source.len()).is_none());
    }

    #[test]
    fn rejects_positions_inside_utf8_code_points() {
        let source = "<div éhx-";

        assert!(htmx_attribute_name_context(source, 6).is_none());
    }
}
