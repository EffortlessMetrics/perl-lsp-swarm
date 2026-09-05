//! Bounded raw-markup context recognition for htmx attribute names.

use super::catalog::starts_with_ignore_ascii_case;

/// Maximum source prefix scanned before a completion position.
///
/// Positions beyond this cap fail closed. Scanning from the document start
/// preserves comment, template-block, and raw-text element state; clipping an
/// arbitrary tail would make those states unknowable.
pub const MAX_MARKUP_SCAN_BYTES: usize = 256 * 1024;

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
/// The recognizer scans a capped source prefix so lexical state is never
/// invented at an arbitrary clipped boundary. It rejects comments, closing and
/// declaration tags, processing instructions, raw-text elements, quoted and
/// unquoted values, completed tags, common Perl-template code regions, and
/// malformed start tags.
#[must_use]
pub fn htmx_attribute_name_context(
    source: &str,
    position: usize,
) -> Option<HtmxAttributeNameContext<'_>> {
    if position > source.len()
        || position > MAX_MARKUP_SCAN_BYTES
        || !source.is_char_boundary(position)
    {
        return None;
    }

    let source_prefix = source.get(..position)?;
    if current_line_is_template_code(source_prefix) {
        return None;
    }

    let tag_start = open_start_tag_offset(source_prefix)?;
    let tag_body = source_prefix.get(tag_start + 1..)?;
    let token_start = active_attribute_name_start(tag_body)?;
    let prefix = tag_body.get(token_start..)?;
    if !is_htmx_attribute_prefix(prefix) {
        return None;
    }

    Some(HtmxAttributeNameContext { prefix, prefix_start: tag_start + 1 + token_start, position })
}

fn current_line_is_template_code(source_prefix: &str) -> bool {
    let line = source_prefix.rsplit_once('\n').map_or(source_prefix, |(_, suffix)| suffix);
    line.trim_start_matches([' ', '\t']).starts_with('%')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawTextKind {
    Script,
    Style,
    Title,
    Textarea,
    Xmp,
    Iframe,
    Noembed,
    Noframes,
    Noscript,
    Plaintext,
}

impl RawTextKind {
    const fn name(self) -> &'static [u8] {
        match self {
            Self::Script => b"script",
            Self::Style => b"style",
            Self::Title => b"title",
            Self::Textarea => b"textarea",
            Self::Xmp => b"xmp",
            Self::Iframe => b"iframe",
            Self::Noembed => b"noembed",
            Self::Noframes => b"noframes",
            Self::Noscript => b"noscript",
            Self::Plaintext => b"plaintext",
        }
    }
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
    MasonNamed(MasonBlockKind),
    RawText(RawTextKind),
    InvalidTag { quote: Option<u8> },
}

/// Mason named blocks (`<%method greet>` ... `</%method>` and friends).
///
/// Unlike the inline `<% ... %>` template regions, a named block closes with
/// `</%name>` instead of `%>`. Recognizing them keeps markup after the block
/// reachable; without this, the first named block would swallow the rest of
/// the document and permanently suppress completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MasonBlockKind {
    Args,
    Attr,
    Class,
    Cleanup,
    Def,
    Doc,
    Filter,
    Flags,
    Init,
    Method,
    Once,
    Perl,
    Shared,
    Sub,
    Text,
}

impl MasonBlockKind {
    const ALL: [Self; 15] = [
        Self::Args,
        Self::Attr,
        Self::Class,
        Self::Cleanup,
        Self::Def,
        Self::Doc,
        Self::Filter,
        Self::Flags,
        Self::Init,
        Self::Method,
        Self::Once,
        Self::Perl,
        Self::Shared,
        Self::Sub,
        Self::Text,
    ];

    const fn name(self) -> &'static [u8] {
        match self {
            Self::Args => b"args",
            Self::Attr => b"attr",
            Self::Class => b"class",
            Self::Cleanup => b"cleanup",
            Self::Def => b"def",
            Self::Doc => b"doc",
            Self::Filter => b"filter",
            Self::Flags => b"flags",
            Self::Init => b"init",
            Self::Method => b"method",
            Self::Once => b"once",
            Self::Perl => b"perl",
            Self::Shared => b"shared",
            Self::Sub => b"sub",
            Self::Text => b"text",
        }
    }
}

fn open_start_tag_offset(source_prefix: &str) -> Option<usize> {
    let bytes = source_prefix.as_bytes();
    let mut state = MarkupState::Text;
    let mut line_start = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'\n' {
            state = scan_markup_segment(bytes, line_start, index, state);
            line_start = index + 1;
        }
        index += 1;
    }
    state = scan_markup_segment(bytes, line_start, bytes.len(), state);

    match state {
        MarkupState::StartTag { start, quote: None } => Some(start),
        _ => None,
    }
}

/// Advance markup state across `[start, end)`, which never contains a newline.
///
/// Mojolicious- and Mason-style template-code lines whose first
/// non-whitespace byte is `%` are removed before the document is rendered, so
/// their bytes are excluded from the markup scan in every state. A `%` line
/// therefore cannot mutate markup state anywhere: its Perl operators and
/// string contents can neither close a start tag or comment, nor end a
/// raw-text element, nor open a false one.
///
/// The one exception is a line that begins with the `%>` or `%]` closing
/// delimiter: it is scanned so an open `<%`/`[%` region closes at its real
/// boundary and markup after it stays reachable.
fn scan_markup_segment(
    bytes: &[u8],
    start: usize,
    end: usize,
    mut state: MarkupState,
) -> MarkupState {
    let code_line = template_code_line(bytes, start, end);
    let closer_line = code_line && template_closer_line(bytes, start, end);
    if code_line && !closer_line {
        return state;
    }

    let mut index = start;
    while index < end {
        state = step_markup_state(bytes, &mut index, state);
    }
    state
}

/// Whether `[start, end)` is a template-code line (leading `%` after blanks).
fn template_code_line(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut index = start;
    while index < end && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    index < end && bytes[index] == b'%'
}

/// Whether a template-code line begins with a region-closing delimiter.
fn template_closer_line(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut index = start;
    while index < end && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    starts_with(bytes, index, b"%>") || starts_with(bytes, index, b"%]")
}

fn step_markup_state(bytes: &[u8], index: &mut usize, state: MarkupState) -> MarkupState {
    match state {
        MarkupState::Text => scan_text(bytes, index),
        MarkupState::Comment => scan_comment(bytes, index),
        MarkupState::TemplatePercent => {
            scan_delimited(bytes, index, b"%>", MarkupState::TemplatePercent)
        }
        MarkupState::TemplateBracket => {
            scan_delimited(bytes, index, b"%]", MarkupState::TemplateBracket)
        }
        MarkupState::MasonComponent => {
            scan_delimited(bytes, index, b"&>", MarkupState::MasonComponent)
        }
        MarkupState::MasonNamed(kind) => scan_mason_named(bytes, index, kind),
        MarkupState::StartTag { start, quote } => scan_start_tag(bytes, index, start, quote),
        MarkupState::IgnoredTag { quote } => scan_ignored_tag(bytes, index, quote),
        MarkupState::RawText(kind) => scan_raw_text(bytes, index, kind),
        MarkupState::InvalidTag { quote } => scan_invalid_tag(bytes, index, quote),
    }
}

/// Close an HTML comment at the standard `-->` ending or HTML5's admitted
/// `--!>` ending, so markup after either is reachable again.
fn scan_comment(bytes: &[u8], index: &mut usize) -> MarkupState {
    if starts_with(bytes, *index, b"-->") {
        *index += 3;
        MarkupState::Text
    } else if starts_with(bytes, *index, b"--!>") {
        *index += 4;
        MarkupState::Text
    } else {
        *index += 1;
        MarkupState::Comment
    }
}

fn scan_text(bytes: &[u8], index: &mut usize) -> MarkupState {
    if starts_with(bytes, *index, b"<!--") {
        *index += 4;
        MarkupState::Comment
    } else if starts_with(bytes, *index, b"<%") {
        if let Some(kind) = mason_named_block_kind(bytes, *index + 2) {
            *index += 2 + kind.name().len();
            MarkupState::MasonNamed(kind)
        } else {
            *index += 2;
            MarkupState::TemplatePercent
        }
    } else if starts_with(bytes, *index, b"[%") {
        *index += 2;
        MarkupState::TemplateBracket
    } else if starts_with(bytes, *index, b"<&") {
        *index += 2;
        MarkupState::MasonComponent
    } else if bytes.get(*index) == Some(&b'<') {
        let next = bytes.get(*index + 1).copied();
        *index += 1;
        match next {
            Some(byte) if byte.is_ascii_alphabetic() => {
                MarkupState::StartTag { start: *index - 1, quote: None }
            }
            Some(b'/' | b'!' | b'?') => MarkupState::IgnoredTag { quote: None },
            _ => MarkupState::Text,
        }
    } else {
        *index += 1;
        MarkupState::Text
    }
}

fn scan_delimited(
    bytes: &[u8],
    index: &mut usize,
    delimiter: &[u8],
    current: MarkupState,
) -> MarkupState {
    if starts_with(bytes, *index, delimiter) {
        *index += delimiter.len();
        MarkupState::Text
    } else {
        *index += 1;
        current
    }
}

fn scan_start_tag(bytes: &[u8], index: &mut usize, start: usize, quote: Option<u8>) -> MarkupState {
    if starts_template_region(bytes, *index) {
        *index += 1;
        return MarkupState::InvalidTag { quote };
    }

    match quote {
        Some(delimiter) => {
            let closes_quote = bytes.get(*index) == Some(&delimiter);
            *index += 1;
            MarkupState::StartTag {
                start,
                quote: if closes_quote { None } else { Some(delimiter) },
            }
        }
        None => match bytes.get(*index).copied() {
            Some(delimiter @ (b'"' | b'\'')) => {
                *index += 1;
                MarkupState::StartTag { start, quote: Some(delimiter) }
            }
            Some(b'>') => {
                let raw_text = raw_text_kind_for_start_tag(bytes, start, *index);
                *index += 1;
                raw_text.map_or(MarkupState::Text, MarkupState::RawText)
            }
            Some(b'<') => {
                *index += 1;
                MarkupState::InvalidTag { quote: None }
            }
            Some(_) => {
                *index += 1;
                MarkupState::StartTag { start, quote: None }
            }
            None => MarkupState::StartTag { start, quote: None },
        },
    }
}

fn scan_ignored_tag(bytes: &[u8], index: &mut usize, quote: Option<u8>) -> MarkupState {
    match quote {
        Some(delimiter) => {
            let closes_quote = bytes.get(*index) == Some(&delimiter);
            *index += 1;
            MarkupState::IgnoredTag { quote: if closes_quote { None } else { Some(delimiter) } }
        }
        None => match bytes.get(*index).copied() {
            Some(delimiter @ (b'"' | b'\'')) => {
                *index += 1;
                MarkupState::IgnoredTag { quote: Some(delimiter) }
            }
            Some(b'>') => {
                *index += 1;
                MarkupState::Text
            }
            Some(_) => {
                *index += 1;
                MarkupState::IgnoredTag { quote: None }
            }
            None => MarkupState::IgnoredTag { quote: None },
        },
    }
}

fn scan_invalid_tag(bytes: &[u8], index: &mut usize, quote: Option<u8>) -> MarkupState {
    match quote {
        Some(delimiter) => {
            let closes_quote = bytes.get(*index) == Some(&delimiter);
            *index += 1;
            MarkupState::InvalidTag { quote: if closes_quote { None } else { Some(delimiter) } }
        }
        None => match bytes.get(*index).copied() {
            Some(delimiter @ (b'"' | b'\'')) => {
                *index += 1;
                MarkupState::InvalidTag { quote: Some(delimiter) }
            }
            Some(b'>') => {
                *index += 1;
                MarkupState::Text
            }
            Some(_) => {
                *index += 1;
                MarkupState::InvalidTag { quote: None }
            }
            None => MarkupState::InvalidTag { quote: None },
        },
    }
}

fn scan_raw_text(bytes: &[u8], index: &mut usize, kind: RawTextKind) -> MarkupState {
    if kind != RawTextKind::Plaintext
        && let Some(name_end) = raw_text_end_tag_name_end(bytes, *index, kind)
    {
        *index = name_end;
        MarkupState::IgnoredTag { quote: None }
    } else {
        *index += 1;
        MarkupState::RawText(kind)
    }
}

fn scan_mason_named(bytes: &[u8], index: &mut usize, kind: MasonBlockKind) -> MarkupState {
    if let Some(name_end) = mason_named_end_tag_name_end(bytes, *index, kind) {
        *index = name_end;
        MarkupState::IgnoredTag { quote: None }
    } else {
        *index += 1;
        MarkupState::MasonNamed(kind)
    }
}

/// Recognize a Mason named-block opener at the byte after `<%`.
///
/// A named opener is `<%keyword` (with no space between `<%` and the keyword,
/// unlike a `<% expr %>` substitution) followed only by names, blanks, and
/// `.`/`-`/`_` characters before the closing `>` of the opener tag. Anything
/// else — including `<%ident(...)` code — stays an ordinary `%` template
/// region and fails closed at its `%>` delimiter.
fn mason_named_block_kind(bytes: &[u8], index: usize) -> Option<MasonBlockKind> {
    let kind = MasonBlockKind::ALL.into_iter().find(|kind| {
        bytes
            .get(index..index + kind.name().len())
            .is_some_and(|name| bytes_eq_ignore_ascii_case(name, kind.name()))
    })?;

    let mut cursor = index + kind.name().len();
    // A longer identifier (`<%methods>`) is not this keyword.
    if bytes.get(cursor).is_some_and(|byte| is_tag_name_byte(*byte)) {
        return None;
    }
    while let Some(byte) = bytes.get(cursor).copied() {
        match byte {
            b'>' => return Some(kind),
            b' ' | b'\t' => cursor += 1,
            byte if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') => {
                cursor += 1;
            }
            _ => return None,
        }
    }
    None
}

fn mason_named_end_tag_name_end(bytes: &[u8], index: usize, kind: MasonBlockKind) -> Option<usize> {
    if !starts_with(bytes, index, b"</%") {
        return None;
    }

    let name_start = index + 3;
    let name_end = name_start.checked_add(kind.name().len())?;
    let name = bytes.get(name_start..name_end)?;
    if !bytes_eq_ignore_ascii_case(name, kind.name()) {
        return None;
    }

    bytes
        .get(name_end)
        .is_some_and(|byte| is_html_space(*byte) || *byte == b'>')
        .then_some(name_end)
}

fn starts_template_region(bytes: &[u8], index: usize) -> bool {
    starts_with(bytes, index, b"<%")
        || starts_with(bytes, index, b"[%")
        || starts_with(bytes, index, b"<&")
}

fn raw_text_kind_for_start_tag(bytes: &[u8], start: usize, end: usize) -> Option<RawTextKind> {
    let body = bytes.get(start + 1..end)?;
    // HTML ignores a self-closing solidus on non-void elements, so a raw-text
    // start tag such as `<script />` still opens raw-text content. Name
    // extraction already stops at the solidus because it is not a tag-name
    // byte, so no special handling is needed beyond not rejecting it.
    let name_end = body.iter().position(|byte| !is_tag_name_byte(*byte)).unwrap_or(body.len());
    let name = body.get(..name_end)?;

    [
        RawTextKind::Script,
        RawTextKind::Style,
        RawTextKind::Title,
        RawTextKind::Textarea,
        RawTextKind::Xmp,
        RawTextKind::Iframe,
        RawTextKind::Noembed,
        RawTextKind::Noframes,
        RawTextKind::Noscript,
        RawTextKind::Plaintext,
    ]
    .into_iter()
    .find(|kind| bytes_eq_ignore_ascii_case(name, kind.name()))
}

fn raw_text_end_tag_name_end(bytes: &[u8], index: usize, kind: RawTextKind) -> Option<usize> {
    if !starts_with(bytes, index, b"</") {
        return None;
    }

    let name_start = index + 2;
    let name_end = name_start.checked_add(kind.name().len())?;
    let name = bytes.get(name_start..name_end)?;
    if !bytes_eq_ignore_ascii_case(name, kind.name()) {
        return None;
    }

    bytes
        .get(name_end)
        .is_some_and(|byte| is_html_space(*byte) || matches!(*byte, b'/' | b'>'))
        .then_some(name_end)
}

fn starts_with(bytes: &[u8], index: usize, needle: &[u8]) -> bool {
    bytes.get(index..).is_some_and(|suffix| suffix.starts_with(needle))
}

fn bytes_eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttributeState {
    Between,
    Name { start: usize },
    AfterName,
    BeforeValue,
    QuotedValue { delimiter: u8 },
    AfterValue,
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
        let byte = bytes.get(index).copied()?;
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
                    AttributeState::AfterValue
                } else {
                    AttributeState::QuotedValue { delimiter }
                }
            }
            AttributeState::AfterValue => {
                if is_html_space(byte) {
                    AttributeState::Between
                } else {
                    AttributeState::Invalid
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

/// HTML tag-name bytes: ASCII alphanumerics, `-`, `_`, `:`, and `.` (the
/// namespaced custom-element separator), plus non-ASCII bytes, which the HTML
/// specification admits in custom-element names.
fn is_tag_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte >= 0x80 || matches!(byte, b'-' | b'_' | b':' | b'.')
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

#[cfg(test)]
mod tests {
    use super::{MAX_MARKUP_SCAN_BYTES, htmx_attribute_name_context};

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
            "<div title=\"value\"hx-",
            "<div foo=hx-",
            "<div <span hx-",
            "<div <span title=\"> <button hx-",
            "<% my $x = '<div hx-'",
            "[% '<div hx-'",
            "<& component, value => '<div hx-'",
            "% my $x = '<div hx-'",
            "<div [% IF enabled %] hx-",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_none(),
                "unexpected context for {source:?}"
            );
        }
    }

    #[test]
    fn recovers_after_a_closed_invalid_tag_without_using_a_quoted_gt() {
        let source = "<div <span title=\">\"> <button hx-";

        assert!(htmx_attribute_name_context(source, source.len()).is_some());
    }

    #[test]
    fn accepts_attribute_names_after_static_values() {
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
    fn rejects_html_like_text_inside_raw_text_elements() {
        for source in [
            "<script>const x = '<div hx-'",
            "<style>.x::before { content: '<div hx-' }",
            "<title><div hx-",
            "<textarea><div hx-",
            "<xmp><div hx-",
            "<iframe><div hx-",
            "<noembed><div hx-",
            "<noframes><div hx-",
            "<noscript><div hx-",
            "<plaintext><div hx-",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_none(),
                "raw-text content admitted for {source:?}"
            );
        }
    }

    #[test]
    fn resumes_markup_after_a_matching_raw_text_end_tag() {
        let source = "<SCRIPT>const x = '<div hx-';</script><button hx-re";
        let context = htmx_attribute_name_context(source, source.len());

        assert!(context.is_some_and(|context| context.prefix == "hx-re"));
    }

    #[test]
    fn preserves_long_comment_and_template_state_from_document_start() {
        for source in [
            format!("<!-- {} <div hx-", "comment".repeat(2_000)),
            format!("<% {} <div hx-", "template".repeat(2_000)),
            format!("<script>{}<div hx-", "script".repeat(2_000)),
        ] {
            assert!(source.len() < MAX_MARKUP_SCAN_BYTES);
            assert!(
                htmx_attribute_name_context(&source, source.len()).is_none(),
                "clipped lexical state admitted markup"
            );
        }
    }

    #[test]
    fn rejects_positions_beyond_the_bounded_scan_budget() {
        let source = format!("{}<div hx-", "text".repeat(MAX_MARKUP_SCAN_BYTES / 4));

        assert!(source.len() > MAX_MARKUP_SCAN_BYTES);
        assert!(htmx_attribute_name_context(&source, source.len()).is_none());

        // The cap is inclusive: a proven slot ending exactly at the budget is
        // still admitted (a `>` to `>=` regression must fail here).
        let at_limit = format!("{}<div hx-", "x".repeat(MAX_MARKUP_SCAN_BYTES - 8));
        assert_eq!(at_limit.len(), MAX_MARKUP_SCAN_BYTES);
        assert!(
            htmx_attribute_name_context(&at_limit, at_limit.len())
                .is_some_and(|context| context.prefix == "hx-")
        );
    }

    #[test]
    fn rejects_positions_inside_utf8_code_points() {
        let source = "<div éhx-";

        assert!(htmx_attribute_name_context(source, 6).is_none());
    }

    #[test]
    fn self_closing_raw_text_start_tags_still_open_raw_text() {
        for source in [
            "<script/><div hx-",
            "<script />const x = '<div hx-'",
            "<style />.x { content: '<div hx-' }",
            "<textarea /><div hx-",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_none(),
                "self-closed raw-text content admitted for {source:?}"
            );
        }

        let resumes = "<script />var ready = true;</script><button hx-re";
        assert!(
            htmx_attribute_name_context(resumes, resumes.len())
                .is_some_and(|context| context.prefix == "hx-re")
        );
    }

    #[test]
    fn custom_element_tag_names_are_recognized() {
        for source in ["<plugin.foo hx-", "<my-élément hx-", "<ns:widget.hx-boost hx-"] {
            assert!(
                htmx_attribute_name_context(source, source.len())
                    .is_some_and(|context| context.prefix == "hx-"),
                "custom-element tag rejected for {source:?}"
            );
        }
    }

    #[test]
    fn earlier_template_code_lines_are_not_markup() {
        for source in [
            "% my $x = '<div hx-';\n<button hx-",
            "% my $x = '<div foo=bar>';\n<button hx-",
            "  % in = ('<script>');\n<button hx-get",
            "\t% layout 'main', title => '<style>';\n<div hx-",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_some(),
                "earlier template-code line suppressed a valid slot for {source:?}"
            );
        }
    }

    #[test]
    fn a_percent_line_carrying_a_region_closer_is_still_scanned() {
        for source in ["<% if ($x) {\n%>\n<div hx-", "[% IF x\n%]\n<div hx-"] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_some(),
                "closer on a %-line was skipped for {source:?}"
            );
        }
    }

    #[test]
    fn html_comments_close_on_the_standard_and_the_incorrect_ending() {
        for source in ["<!-- note --><div hx-", "<!-- note --!><div hx-"] {
            assert!(
                htmx_attribute_name_context(source, source.len())
                    .is_some_and(|context| context.prefix == "hx-"),
                "comment ending suppressed a valid slot for {source:?}"
            );
        }
    }

    #[test]
    fn template_code_lines_are_inert_inside_markup_states() {
        for source in [
            // A %-line cannot close a comment: it renders as nothing, so the
            // comment stays open and the later slot is not proven.
            "<!--\n% note -->\n<div hx-",
            // A quoted fake end tag on a %-line cannot end a raw-text element.
            "<script>\n% q(</script>);\n<div hx-",
            // A `>` on a %-line cannot close an open start tag either.
            "<div\n% if ($x > 1) {\nhx-",
        ] {
            assert!(
                htmx_attribute_name_context(source, source.len()).is_none(),
                "template-code line mutated markup state for {source:?}"
            );
        }
    }

    #[test]
    fn named_mason_blocks_do_not_suppress_later_markup() {
        let source = "<%method greet>\n<p>hello</p>\n</%method>\n<div hx-";
        assert!(
            htmx_attribute_name_context(source, source.len())
                .is_some_and(|context| context.prefix == "hx-")
        );

        let sub_block = "<%sub render>\n<p>x</p>\n</%sub>\n<button hx-post";
        assert!(
            htmx_attribute_name_context(sub_block, sub_block.len())
                .is_some_and(|context| context.prefix == "hx-post")
        );

        let def_block = "<%def footer>\n<span>x</span>\n</%def>\n<div hx-get";
        assert!(
            htmx_attribute_name_context(def_block, def_block.len())
                .is_some_and(|context| context.prefix == "hx-get")
        );

        let args_block = "<%args>\n  $x => 1\n</%args>\n<div hx-get";
        assert!(
            htmx_attribute_name_context(args_block, args_block.len())
                .is_some_and(|context| context.prefix == "hx-get")
        );

        let flags_block = "<%flags>\n  inherit => 1\n</%flags>\n<button hx-";
        assert!(
            htmx_attribute_name_context(flags_block, flags_block.len())
                .is_some_and(|context| context.prefix == "hx-")
        );

        let with_substitution = "<%attr>\n  id => 'x'\n</%attr>\n<% \"inline\" %>\n<button hx-post";
        assert!(
            htmx_attribute_name_context(with_substitution, with_substitution.len())
                .is_some_and(|context| context.prefix == "hx-post")
        );

        // Non-keyword `<%ident>` stays an ordinary `%` template region and
        // fails closed when its `%>` never arrives.
        let not_a_block = "<%unknown>...\n<div hx-";
        assert!(htmx_attribute_name_context(not_a_block, not_a_block.len()).is_none());
    }
}
