//! File-local symbol table for bareword/regex disambiguation.
//!
//! Enables the lexer to identify known subroutine names so that
//! `identifier /regex/` can be lexed as a function call with a regex argument
//! rather than `identifier / expr` division.
//!
//! # How it works
//!
//! Before context-sensitive lexing begins, a bounded phase-0 scan collects
//! source-level `sub NAME` declarations. The scanner deliberately has no symbol
//! table dependency, so it cannot recurse back into the lexer whose ambiguity it
//! helps resolve.
//!
//! The phase-0 scan shares the lexer's Unicode identifier policy and excludes
//! line comments, ordinary quoted strings, POD, heredoc bodies, and data
//! sections. Quote-like operators, regex bodies, formats, imports, generated
//! code, and source filters remain explicit follow-up boundaries under #6732.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::unicode::{is_perl_identifier_continue, is_perl_identifier_start};

/// File-local subroutine symbol table built from a source pre-pass.
///
/// # Forward references
///
/// Because this is a whole-file pre-pass, subroutines declared *after* their
/// first call site are recognized correctly. A forward reference like:
///
/// ```text
/// builder /pattern/;
/// sub builder { ... }
/// ```
///
/// can therefore take the known-sub/regex path.
///
/// Qualified declarations are stored using their exact source spelling. For
/// example, `sub Foo::bar` records `Foo::bar`, not the unqualified alias `bar`.
#[derive(Debug, Clone, Default)]
pub struct LocalSymbolTable {
    known_subs: Arc<HashSet<Box<str>>>,
}

impl LocalSymbolTable {
    /// Scan Perl source for statically declared `sub NAME` forms.
    ///
    /// The scanner is O(n) over source bytes and deliberately source-only. It
    /// recognizes the same Unicode identifier starts/continuations as the
    /// native lexer, including `::` and legacy apostrophe package separators.
    /// It excludes declarations spelled inside line comments, ordinary quoted
    /// strings, POD, recognized heredoc bodies, and `__DATA__`/`__END__`.
    ///
    /// Imported symbols, dynamic declarations, quote-like/regex/format bodies,
    /// `eval`, `AUTOLOAD`, source filters, and workspace symbols are not inferred.
    pub fn scan_subs(input: &str) -> Self {
        let mut known_subs = HashSet::new();
        let mut state = ScanState::default();
        let bytes = input.as_bytes();
        let mut line_start = 0usize;

        while line_start < bytes.len() {
            let (line_end, next_line_start) = line_bounds(bytes, line_start);
            let line = &input[line_start..line_end];

            if let Some(pending) = state.pending_heredocs.front() {
                if pending.matches_terminator(line) {
                    state.pending_heredocs.pop_front();
                }
                line_start = next_line_start;
                continue;
            }

            if state.in_pod {
                if line.starts_with("=cut") {
                    state.in_pod = false;
                }
                line_start = next_line_start;
                continue;
            }

            if state.quote == QuoteState::Code && starts_pod(line) {
                state.in_pod = true;
                line_start = next_line_start;
                continue;
            }

            if state.quote == QuoteState::Code && is_data_marker(line) {
                break;
            }

            scan_code_line(line, &mut state, &mut known_subs);
            line_start = next_line_start;
        }

        Self { known_subs: Arc::new(known_subs) }
    }

    /// Return `true` if `name` was declared as a subroutine in this file.
    pub fn is_known_sub(&self, name: &str) -> bool {
        self.known_subs.contains(name)
    }

    /// Return the number of subroutine names recorded.
    pub fn len(&self) -> usize {
        self.known_subs.len()
    }

    /// Return `true` if no subroutines have been recorded.
    pub fn is_empty(&self) -> bool {
        self.known_subs.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum QuoteState {
    #[default]
    Code,
    Single,
    Double,
    Backtick,
}

impl QuoteState {
    fn delimiter(self) -> Option<char> {
        match self {
            Self::Code => None,
            Self::Single => Some('\''),
            Self::Double => Some('"'),
            Self::Backtick => Some('`'),
        }
    }
}

#[derive(Debug, Default)]
struct ScanState {
    quote: QuoteState,
    awaiting_sub_name: bool,
    in_pod: bool,
    pending_heredocs: VecDeque<PendingHeredoc>,
}

#[derive(Debug)]
struct PendingHeredoc {
    label: Box<str>,
    allow_indent: bool,
}

impl PendingHeredoc {
    fn matches_terminator(&self, line: &str) -> bool {
        let line = line.trim_end_matches([' ', '\t']);
        if self.allow_indent {
            line.trim_start_matches([' ', '\t']) == self.label.as_ref()
        } else {
            line == self.label.as_ref()
        }
    }
}

fn line_bounds(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut end = start;
    while end < bytes.len() && !matches!(bytes[end], b'\n' | b'\r') {
        end += 1;
    }

    let mut next = end;
    if next < bytes.len() {
        if bytes[next] == b'\r' {
            next += 1;
            if next < bytes.len() && bytes[next] == b'\n' {
                next += 1;
            }
        } else {
            next += 1;
        }
    }
    (end, next)
}

fn starts_pod(line: &str) -> bool {
    [
        "=pod",
        "=head",
        "=over",
        "=item",
        "=back",
        "=begin",
        "=end",
        "=for",
        "=encoding",
    ]
    .iter()
    .any(|directive| line.starts_with(directive))
}

fn is_data_marker(line: &str) -> bool {
    matches!(line.trim_end_matches([' ', '\t']), "__DATA__" | "__END__")
}

fn scan_code_line(line: &str, state: &mut ScanState, known_subs: &mut HashSet<Box<str>>) {
    let mut offset = 0usize;

    while offset < line.len() {
        if state.quote != QuoteState::Code {
            offset = scan_quoted_character(line, offset, state);
            continue;
        }

        if state.awaiting_sub_name {
            offset = skip_horizontal_whitespace(line, offset);
            if offset >= line.len() || line[offset..].starts_with('#') {
                return;
            }

            if let Some((name, end)) = parse_qualified_name(line, offset) {
                known_subs.insert(name.into());
                state.awaiting_sub_name = false;
                offset = end;
                continue;
            }

            state.awaiting_sub_name = false;
        }

        let Some(ch) = line[offset..].chars().next() else {
            return;
        };

        if ch == '#' {
            return;
        }

        if ch == '\'' {
            if apostrophe_is_package_separator(line, offset) {
                offset += ch.len_utf8();
            } else {
                state.quote = QuoteState::Single;
                offset += ch.len_utf8();
            }
            continue;
        }
        if ch == '"' {
            state.quote = QuoteState::Double;
            offset += ch.len_utf8();
            continue;
        }
        if ch == '`' {
            state.quote = QuoteState::Backtick;
            offset += ch.len_utf8();
            continue;
        }

        if line[offset..].starts_with("<<")
            && heredoc_allowed_before(line, offset)
            && let Some((pending, end)) = parse_heredoc_opener(line, offset)
        {
            state.pending_heredocs.push_back(pending);
            offset = end;
            continue;
        }

        if line[offset..].starts_with("sub") && is_sub_keyword_boundary(line, offset) {
            state.awaiting_sub_name = true;
            offset += "sub".len();
            continue;
        }

        offset += ch.len_utf8();
    }
}

fn scan_quoted_character(line: &str, offset: usize, state: &mut ScanState) -> usize {
    let Some(ch) = line[offset..].chars().next() else {
        return line.len();
    };

    if ch == '\\' {
        let after_slash = offset + ch.len_utf8();
        return line[after_slash..]
            .chars()
            .next()
            .map_or(after_slash, |escaped| after_slash + escaped.len_utf8());
    }

    if state.quote.delimiter() == Some(ch) {
        state.quote = QuoteState::Code;
    }
    offset + ch.len_utf8()
}

fn skip_horizontal_whitespace(line: &str, mut offset: usize) -> usize {
    while let Some(ch) = line[offset..].chars().next() {
        if matches!(ch, ' ' | '\t') {
            offset += ch.len_utf8();
        } else {
            break;
        }
    }
    offset
}

fn apostrophe_is_package_separator(line: &str, offset: usize) -> bool {
    let before = line[..offset].chars().next_back();
    let after = line[offset + '\''.len_utf8()..].chars().next();
    before.is_some_and(is_name_segment_continue) && after.is_some_and(is_perl_identifier_start)
}

fn is_sub_keyword_boundary(line: &str, offset: usize) -> bool {
    if line[..offset].ends_with("->") {
        return false;
    }

    let before = line[..offset].chars().next_back();
    if before.is_some_and(|ch| {
        is_perl_identifier_continue(ch) || matches!(ch, '$' | '@' | '%' | '&' | '*' | ':' | '\'')
    }) {
        return false;
    }

    let after_offset = offset + "sub".len();
    let after = line[after_offset..].chars().next();
    !after.is_some_and(|ch| {
        is_perl_identifier_continue(ch) || matches!(ch, ':' | '\'')
    })
}

fn parse_qualified_name(text: &str, start: usize) -> Option<(&str, usize)> {
    let mut offset = start;
    if text[offset..].starts_with("::") {
        offset += 2;
    }

    let first = text[offset..].chars().next()?;
    if !is_perl_identifier_start(first) {
        return None;
    }
    offset += first.len_utf8();
    offset = consume_name_segment(text, offset);

    loop {
        if text[offset..].starts_with("::") {
            let segment_start = offset + 2;
            let Some(first) = text[segment_start..].chars().next() else {
                break;
            };
            if !is_perl_identifier_start(first) {
                break;
            }
            offset = consume_name_segment(text, segment_start + first.len_utf8());
            continue;
        }

        if text[offset..].starts_with('\'') {
            let segment_start = offset + '\''.len_utf8();
            let Some(first) = text[segment_start..].chars().next() else {
                break;
            };
            if !is_perl_identifier_start(first) {
                break;
            }
            offset = consume_name_segment(text, segment_start + first.len_utf8());
            continue;
        }
        break;
    }

    Some((&text[start..offset], offset))
}

fn consume_name_segment(text: &str, mut offset: usize) -> usize {
    while let Some(ch) = text[offset..].chars().next() {
        if is_name_segment_continue(ch) {
            offset += ch.len_utf8();
        } else {
            break;
        }
    }
    offset
}

fn is_name_segment_continue(ch: char) -> bool {
    ch != '\'' && is_perl_identifier_continue(ch)
}

fn heredoc_allowed_before(line: &str, offset: usize) -> bool {
    let prefix = line[..offset].trim_end_matches([' ', '\t']);
    if prefix.is_empty() {
        return true;
    }

    if prefix
        .chars()
        .next_back()
        .is_some_and(|ch| matches!(ch, '=' | '(' | '[' | '{' | ',' | ';' | ':' | '?'))
    {
        return true;
    }

    prefix
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'))
        .next_back()
        .is_some_and(|word| matches!(word, "print" | "say" | "warn" | "die" | "return"))
}

fn parse_heredoc_opener(line: &str, start: usize) -> Option<(PendingHeredoc, usize)> {
    let mut offset = start + 2;
    let allow_indent = if line[offset..].starts_with('~') {
        offset += '~'.len_utf8();
        true
    } else {
        false
    };
    offset = skip_horizontal_whitespace(line, offset);

    if line[offset..].starts_with('\\') {
        offset += '\\'.len_utf8();
    }

    let Some(first) = line[offset..].chars().next() else {
        return None;
    };

    let (label, end) = if matches!(first, '\'' | '"' | '`') {
        parse_quoted_heredoc_label(line, offset, first)?
    } else {
        if !is_perl_identifier_start(first) {
            return None;
        }
        let label_start = offset;
        offset += first.len_utf8();
        while let Some(ch) = line[offset..].chars().next() {
            if is_name_segment_continue(ch) {
                offset += ch.len_utf8();
            } else {
                break;
            }
        }
        (line[label_start..offset].to_string(), offset)
    };

    if label.is_empty() {
        return None;
    }

    Some((PendingHeredoc { label: label.into_boxed_str(), allow_indent }, end))
}

fn parse_quoted_heredoc_label(
    line: &str,
    start: usize,
    quote: char,
) -> Option<(String, usize)> {
    let mut offset = start + quote.len_utf8();
    let mut label = String::new();

    while let Some(ch) = line[offset..].chars().next() {
        if ch == quote {
            return Some((label, offset + ch.len_utf8()));
        }
        if ch == '\\' {
            offset += ch.len_utf8();
            let escaped = line[offset..].chars().next()?;
            label.push(escaped);
            offset += escaped.len_utf8();
            continue;
        }
        label.push(ch);
        offset += ch.len_utf8();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::LocalSymbolTable;
    use crate::{LexerConfig, PerlLexer, TokenType};

    fn tokens_with_table(source: &str, table: LocalSymbolTable) -> Vec<crate::Token> {
        let config = LexerConfig { symbol_table: Some(table), ..LexerConfig::default() };
        PerlLexer::with_config(source, config).collect_tokens()
    }

    #[test]
    fn empty_and_default_tables_are_empty() {
        assert!(LocalSymbolTable::scan_subs("").is_empty());
        assert!(LocalSymbolTable::default().is_empty());
    }

    #[test]
    fn ordinary_forward_and_prototyped_declarations_are_recognized() {
        let source = "sub alpha { }\nsub beta;\nsub transform ($$) { }\nsub _private { }\nsub process2 { }";
        let table = LocalSymbolTable::scan_subs(source);

        for name in ["alpha", "beta", "transform", "_private", "process2"] {
            assert!(table.is_known_sub(name), "missing {name:?}");
        }
        assert_eq!(table.len(), 5);
    }

    #[test]
    fn unicode_qualified_legacy_and_keyword_names_use_exact_spelling() {
        let source = "sub café { }\nsub Foo::bar { }\nsub Foo'legacy { }\nsub ::root { }\nsub q { }";
        let table = LocalSymbolTable::scan_subs(source);

        for name in ["café", "Foo::bar", "Foo'legacy", "::root", "q"] {
            assert!(table.is_known_sub(name), "missing exact declaration key {name:?}");
        }
        assert!(!table.is_known_sub("bar"));
        assert!(!table.is_known_sub("legacy"));
        assert!(!table.is_known_sub("root"));
    }

    #[test]
    fn comments_and_ordinary_strings_cannot_create_known_subs() {
        let source = concat!(
            "# sub commented_out { }\n",
            "my $a = 'sub single_quoted { }';\n",
            "my $b = \"sub double_quoted { }\";\n",
            "my $c = `echo sub backtick { }`;\n",
            "sub real { } # sub inline_comment { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        for name in ["commented_out", "single_quoted", "double_quoted", "backtick", "inline_comment"] {
            assert!(!table.is_known_sub(name), "non-code name leaked: {name:?}");
        }
    }

    #[test]
    fn multiline_ordinary_string_state_is_preserved_between_lines() {
        let source = "my $text = \"sub first_fake { }\nstill text with sub second_fake { }\";\nsub real { }";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("first_fake"));
        assert!(!table.is_known_sub("second_fake"));
    }

    #[test]
    fn pod_sections_cannot_create_known_subs() {
        let source = "=pod\nsub documented { }\n=cut\nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("documented"));
    }

    #[test]
    fn quoted_and_indented_heredoc_bodies_cannot_create_known_subs() {
        let source = concat!(
            "my $a = <<'ONE';\n",
            "sub first_fake { }\n",
            "ONE\n",
            "my $b = <<~TWO;\n",
            "  sub second_fake { }\n",
            "  TWO\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("first_fake"));
        assert!(!table.is_known_sub("second_fake"));
    }

    #[test]
    fn multiple_heredocs_are_excluded_in_fifo_order() {
        let source = concat!(
            "my ($a, $b) = (<<A, <<'B');\n",
            "sub first_fake { }\n",
            "A\n",
            "sub second_fake { }\n",
            "B\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("first_fake"));
        assert!(!table.is_known_sub("second_fake"));
    }

    #[test]
    fn data_and_end_markers_stop_declaration_collection() {
        for marker in ["__DATA__", "__END__"] {
            let source = format!("sub before {{ }}\n{marker}\nsub after {{ }}\n");
            let table = LocalSymbolTable::scan_subs(&source);
            assert!(table.is_known_sub("before"));
            assert!(!table.is_known_sub("after"));
        }
    }

    #[test]
    fn substrings_variables_methods_and_anonymous_subs_do_not_invent_names() {
        let source = concat!(
            "my $sub = 1;\n",
            "my $x = substring(1, 2);\n",
            "$obj->sub();\n",
            "my $code = sub { 1 };\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert_eq!(table.len(), 1);
        assert!(table.is_known_sub("real"));
        for name in ["string", "anonymous", "obj"] {
            assert!(!table.is_known_sub(name));
        }
    }

    #[test]
    fn fake_heredoc_declaration_cannot_change_slash_disambiguation() {
        let source = concat!(
            "my $doc = <<'END';\n",
            "sub builder { }\n",
            "END\n",
            "builder /pattern/;\n",
        );
        let table = LocalSymbolTable::scan_subs(source);
        assert!(!table.is_known_sub("builder"));

        let tokens = tokens_with_table(source, table);
        assert!(tokens.iter().any(|token| matches!(&token.token_type, TokenType::Division)));
        assert!(!tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    }

    #[test]
    fn real_forward_and_unicode_declarations_change_only_the_known_sub_path() {
        for source in [
            "builder /pattern/; sub builder { 1 }",
            "café /pattern/; sub café { 1 }",
        ] {
            let table = LocalSymbolTable::scan_subs(source);
            let tokens = tokens_with_table(source, table);
            assert!(tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
        }
    }

    #[test]
    fn crlf_and_cr_only_lines_preserve_boundaries() {
        for source in [
            "=pod\r\nsub fake { }\r\n=cut\r\nsub real { }\r\n",
            "=pod\rsub fake { }\r=cut\rsub real { }\r",
        ] {
            let table = LocalSymbolTable::scan_subs(source);
            assert!(table.is_known_sub("real"));
            assert!(!table.is_known_sub("fake"));
        }
    }
}
