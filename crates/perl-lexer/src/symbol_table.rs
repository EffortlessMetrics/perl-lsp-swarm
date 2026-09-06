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
//! line comments, ordinary and quote-like strings, pattern bodies, POD,
//! heredoc bodies, `format` bodies, and data sections. Imports, generated code,
//! and source filters remain explicit follow-up boundaries under #6732.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::lexer::helpers::is_builtin_function;
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
    /// The scanner is deliberately source-only. It recognizes the same Unicode
    /// identifier starts/continuations as the native lexer, including `::` and
    /// legacy apostrophe package separators. It excludes declarations spelled
    /// inside line comments, ordinary and quote-like strings, pattern bodies,
    /// POD, recognized heredoc bodies, `format` bodies, and
    /// `__DATA__`/`__END__`.
    ///
    /// Imported symbols, dynamic declarations, `eval`, `AUTOLOAD`, source
    /// filters, and workspace symbols are not inferred. A heredoc opener is
    /// therefore still missed when its callable is only imported, never
    /// declared in this file.
    ///
    /// # Format bodies
    ///
    /// A `format` body is excluded whole, and only when a terminator is found:
    /// an opener whose body never closes is left unarmed, so opener-shaped prose
    /// cannot hide the declarations after it.
    ///
    /// Perl alternates picture lines and argument lines inside a body, and a
    /// `sub NAME {}` on an *argument* line is genuinely declared, so excluding
    /// the region drops that declaration. The trade is deliberate: picture lines
    /// carry arbitrary report text, which makes a false declaration realistic,
    /// whereas a named declaration on an argument line is pathological. Both
    /// directions can mis-tokenize — a missing declaration turns a real regex
    /// into division just as a false one turns division into a regex — so this
    /// is a judgement about which case occurs in practice, not a claim that one
    /// direction is harmless. Missing an argument-line declaration also matches
    /// the standing behavior for imported subs. Distinguishing the two line
    /// kinds would put a second Perl grammar inside the prepass. See #14916.
    ///
    /// A bare `/.../` regex body is still scanned as code, because recognizing
    /// an unprefixed slash requires the regex-versus-division decision this
    /// table exists to inform.
    ///
    /// # Cost
    ///
    /// Two linear passes over source bytes on well-formed input, including
    /// input carrying many *closed* quote-like constructs. Input carrying many
    /// *unclosed* quote-like openers is quadratic, because each opener
    /// pre-validates closure against the remaining source; see #6732 for the
    /// bounded-lookahead follow-up.
    pub fn scan_subs(input: &str) -> Self {
        // Two passes. Whether `NAME <<TERM` opens a heredoc depends on whether
        // `NAME` is a callable, but a single forward pass only knows the
        // declarations it has already walked past, so an opener whose callable
        // is declared further down the file is missed and its body is scanned
        // as code. The first pass therefore only collects candidate declaration
        // names; the second pass consults them from the first line onward.
        let hints = scan_pass(input, &HashSet::new());
        let known_subs = scan_pass(input, &hints);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteLikePhase {
    InBody,
    AwaitingPairedBody,
}

#[derive(Debug, Clone, Copy)]
struct QuoteLikeState {
    opener: char,
    closer: char,
    paired: bool,
    depth: usize,
    bodies_remaining: u8,
    phase: QuoteLikePhase,
}

impl QuoteLikeState {
    fn new(opener: char, bodies_remaining: u8) -> Self {
        let (closer, paired) =
            paired_delimiter(opener).map_or((opener, false), |closer| (closer, true));
        Self {
            opener,
            closer,
            paired,
            depth: usize::from(paired),
            bodies_remaining,
            phase: QuoteLikePhase::InBody,
        }
    }

    fn begin_paired_body(&mut self, opener: char) {
        let (closer, paired) =
            paired_delimiter(opener).map_or((opener, false), |closer| (closer, true));
        self.opener = opener;
        self.closer = closer;
        self.paired = paired;
        self.depth = usize::from(paired);
        self.phase = QuoteLikePhase::InBody;
    }
}

#[derive(Debug, Default)]
struct ScanState {
    quote: QuoteState,
    quote_like: Option<QuoteLikeState>,
    awaiting_sub_name: bool,
    in_pod: bool,
    in_format: bool,
    pending_heredocs: VecDeque<PendingHeredoc>,
}

#[derive(Debug)]
struct PendingHeredoc {
    label: Box<str>,
    allow_indent: bool,
}

impl PendingHeredoc {
    fn matches_terminator(&self, line: &str) -> bool {
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

fn pod_command(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('=')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest.get(..end)
}

fn starts_pod(line: &str) -> bool {
    let Some(command) = pod_command(line) else {
        return false;
    };
    matches!(command, "pod" | "over" | "item" | "back" | "begin" | "end" | "for" | "encoding")
        || command == "head"
        || command.strip_prefix("head").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn is_pod_cut(line: &str) -> bool {
    pod_command(line) == Some("cut")
}

fn is_data_marker(line: &str) -> bool {
    matches!(line.trim_end_matches([' ', '\t']), "__DATA__" | "__END__")
}

/// Return `true` if `line` terminates a `format` body.
///
/// Perl accepts trailing horizontal whitespace after the terminating `.` but
/// rejects an indented one, so leading whitespace deliberately keeps the body
/// open rather than releasing it back to code.
fn is_format_terminator(line: &str) -> bool {
    line.trim_end_matches([' ', '\t']) == "."
}

/// Return `true` if a `format` body opened on the line at `line_start` is
/// terminated later in `input`.
///
/// An opener is only armed when its body closes. Otherwise a
/// format-opener-shaped line that is really prose — inside a heredoc this
/// prepass does not recognize, or in a partially typed buffer — would consume
/// every remaining line and hide the real declarations after it. That failure
/// is worse than the leak this region exists to close, and it contradicts the
/// rule the prepass already applies to malformed quote-like constructs: keep the
/// damage local rather than erasing the rest of the file.
///
/// Cost is one extra line walk per opener that reaches this check. Openers are
/// rare and the walk stops at the first terminator, so this does not change the
/// prepass's behavior on input without `format`.
fn format_body_terminates(input: &str, line_start: usize) -> bool {
    let bytes = input.as_bytes();
    let (_, mut cursor) = line_bounds(bytes, line_start);

    while cursor < bytes.len() {
        let (line_end, next_line_start) = line_bounds(bytes, cursor);
        if is_format_terminator(&input[cursor..line_end]) {
            return true;
        }
        cursor = next_line_start;
    }

    false
}

/// Return `true` if a statement can begin immediately after `prefix`.
///
/// A statement begins at the start of a line and after `;`, `{` or `}`. It also
/// begins after Perl statement labels, which `format` accepts: `LABEL: format
/// STDOUT =` is valid and does declare a format. Verified against perl 5.38.2,
/// which also accepts a space before the colon (`L : format`) and more than one
/// label (`A: B: format`), so labels are peeled in a loop rather than once.
///
/// The label case must not admit the `::` package separator: `$Report::format`
/// also ends in a colon but is an ordinary name, not a labelled statement. Only
/// one colon is stripped per turn, so a `::` prefix still ends in a colon, which
/// is not an identifier character and therefore yields no label word. The
/// package-scalar controls pin that.
fn starts_a_statement(prefix: &str) -> bool {
    let mut prefix = prefix.trim_end_matches([' ', '\t']);

    loop {
        if prefix.is_empty() || prefix.ends_with([';', '{', '}']) {
            return true;
        }

        let Some(head) = prefix.strip_suffix(':') else {
            return false;
        };
        let head = head.trim_end_matches([' ', '\t']);

        let Some(label_start) = head
            .char_indices()
            .rev()
            .take_while(|(_, ch)| is_perl_identifier_continue(*ch))
            .last()
            .map(|(index, _)| index)
        else {
            return false;
        };

        let (before_label, label) = head.split_at(label_start);
        if !label.starts_with(is_perl_identifier_start) {
            return false;
        }

        // Each turn removes at least the colon, so this terminates.
        prefix = before_label.trim_end_matches([' ', '\t']);
    }
}

/// Return `true` if the `format` keyword at `offset` sits where a statement can
/// begin and is not part of a longer name, a method call, or a hash subscript.
fn is_format_keyword_boundary(line: &str, offset: usize) -> bool {
    if !starts_a_statement(line[..offset].trim_end_matches([' ', '\t'])) {
        return false;
    }

    // `'` is already an identifier continuation here (legacy package
    // separator), so only `:` needs naming alongside.
    let after_offset = offset + "format".len();
    let after = line[after_offset..].chars().next();
    !after.is_some_and(|ch| is_perl_identifier_continue(ch) || ch == ':')
}

/// Return `true` if a `format` opener beginning at `after_keyword` completes on
/// this line, i.e. an optional name followed by `=` as the last non-comment
/// token.
///
/// The trailing check is what separates a real opener from an operator: `=>`,
/// `==` and `=~` all leave their second character in `rest`, which is neither
/// empty nor a comment, so no separate operator test is needed.
fn format_opener_completes_line(line: &str, after_keyword: usize) -> bool {
    let mut offset = skip_horizontal_whitespace(line, after_keyword);

    if let Some((_, end)) = parse_qualified_name(line, offset) {
        offset = skip_horizontal_whitespace(line, end);
    }

    if !line[offset..].starts_with('=') {
        return false;
    }
    offset += '='.len_utf8();

    let rest = line[offset..].trim_start_matches([' ', '\t']);
    rest.is_empty() || rest.starts_with('#')
}

/// Run one full forward scan, treating `hints` as additional callable names.
///
/// `hints` is empty on the first pass and carries the first pass's declaration
/// names on the second, which is what lets a heredoc opener recognize a
/// callable declared later in the file.
fn scan_pass(input: &str, hints: &HashSet<Box<str>>) -> HashSet<Box<str>> {
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

        if state.in_format {
            if is_format_terminator(line) {
                state.in_format = false;
            }
            line_start = next_line_start;
            continue;
        }

        if state.in_pod {
            if is_pod_cut(line) {
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

        scan_code_line(input, line_start, line, &mut state, &mut known_subs, hints);
        line_start = next_line_start;
    }

    known_subs
}

/// Return `true` if `word` names something callable for prepass purposes.
///
/// `known_subs` holds declarations already seen in this pass; `hints` holds the
/// previous pass's declarations, which cover callables declared further down.
fn is_callable_word(word: &str, known_subs: &HashSet<Box<str>>, hints: &HashSet<Box<str>>) -> bool {
    is_builtin_function(word) || known_subs.contains(word) || hints.contains(word)
}

fn scan_code_line(
    input: &str,
    line_start: usize,
    line: &str,
    state: &mut ScanState,
    known_subs: &mut HashSet<Box<str>>,
    hints: &HashSet<Box<str>>,
) {
    let mut offset = 0usize;

    while offset < line.len() {
        if state.quote_like.is_some() {
            offset = scan_quote_like_character(line, offset, state);
            continue;
        }

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

        if let Some(end) = start_quote_like(input, line_start, line, offset, state) {
            offset = end;
            continue;
        }

        if ch == '\'' {
            if apostrophe_is_package_separator(line, offset, known_subs, hints) {
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
            && heredoc_allowed_before(line, offset, known_subs, hints)
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

        if line[offset..].starts_with("format")
            && is_format_keyword_boundary(line, offset)
            && format_opener_completes_line(line, offset + "format".len())
            && format_body_terminates(input, line_start)
        {
            // The picture body starts on the next line; nothing after the `=`
            // on this line is code.
            state.in_format = true;
            return;
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

fn apostrophe_is_package_separator(
    line: &str,
    offset: usize,
    known_subs: &HashSet<Box<str>>,
    hints: &HashSet<Box<str>>,
) -> bool {
    let before = line[..offset].chars().next_back();
    let after = line[offset + '\''.len_utf8()..].chars().next();
    if !before.is_some_and(is_name_segment_continue) || !after.is_some_and(is_perl_identifier_start)
    {
        return false;
    }

    previous_word_before(line, offset).is_none_or(|word| !is_callable_word(word, known_subs, hints))
}

fn paired_delimiter(opener: char) -> Option<char> {
    match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '<' => Some('>'),
        _ => None,
    }
}

fn quote_like_operator_at(line: &str, offset: usize) -> Option<(&'static str, u8)> {
    let prefix = line[..offset].trim_end_matches([' ', '\t']);
    let separated_from_prefix =
        line[..offset].chars().next_back().is_some_and(|ch| matches!(ch, ' ' | '\t'));
    if prefix.ends_with("->")
        || (!separated_from_prefix
            && prefix.chars().next_back().is_some_and(|ch| {
                is_perl_identifier_continue(ch)
                    || matches!(ch, ':' | '\'' | '$' | '@' | '%' | '&' | '*' | '-')
            }))
    {
        return None;
    }

    for (operator, bodies) in [
        ("tr", 2),
        ("qq", 1),
        ("qw", 1),
        ("qx", 1),
        ("qr", 1),
        ("q", 1),
        ("m", 1),
        ("s", 2),
        ("y", 2),
    ] {
        if !line[offset..].starts_with(operator) {
            continue;
        }
        let after = offset + operator.len();
        if line[after..]
            .chars()
            .next()
            .is_some_and(|ch| is_perl_identifier_continue(ch) || matches!(ch, ':' | '\''))
        {
            continue;
        }
        return Some((operator, bodies));
    }
    None
}

fn start_quote_like(
    input: &str,
    line_start: usize,
    line: &str,
    offset: usize,
    state: &mut ScanState,
) -> Option<usize> {
    let (operator, bodies) = quote_like_operator_at(line, offset)?;
    let mut delimiter_offset = skip_horizontal_whitespace(line, offset + operator.len());
    if delimiter_offset >= line.len() || line[delimiter_offset..].starts_with("=>") {
        return None;
    }

    let delimiter = line[delimiter_offset..].chars().next()?;
    if delimiter.is_alphanumeric() || delimiter == '_' {
        return None;
    }

    let prefix = line[..offset].trim_end_matches([' ', '\t']);
    if prefix.ends_with('{') && delimiter == '}' {
        return None;
    }

    delimiter_offset += delimiter.len_utf8();
    let quote_like = QuoteLikeState::new(delimiter, bodies);
    let absolute_body_start = line_start + delimiter_offset;
    if input.get(absolute_body_start..).is_some_and(|suffix| quote_like_closes(suffix, quote_like))
    {
        state.quote_like = Some(quote_like);
        Some(delimiter_offset)
    } else {
        // A malformed quote-like construct must not hide every declaration in
        // the rest of the file. Keep its current physical line opaque, then
        // resume the declaration prepass on the following line.
        state.quote_like = None;
        Some(line.len())
    }
}

fn quote_like_closes(source: &str, mut quote_like: QuoteLikeState) -> bool {
    let mut chars = source.chars();

    while let Some(ch) = chars.next() {
        if quote_like.phase == QuoteLikePhase::AwaitingPairedBody {
            if ch.is_whitespace() {
                continue;
            }
            if ch.is_alphanumeric() || ch == '_' {
                return false;
            }
            quote_like.begin_paired_body(ch);
            continue;
        }

        if ch == '\\' {
            let _ = chars.next();
            continue;
        }

        if quote_like.paired && ch == quote_like.opener {
            quote_like.depth = quote_like.depth.saturating_add(1);
            continue;
        }
        if ch != quote_like.closer {
            continue;
        }

        if quote_like.paired && quote_like.depth > 1 {
            quote_like.depth -= 1;
            continue;
        }

        quote_like.bodies_remaining = quote_like.bodies_remaining.saturating_sub(1);
        if quote_like.bodies_remaining == 0 {
            return true;
        }
        if quote_like.paired {
            quote_like.phase = QuoteLikePhase::AwaitingPairedBody;
        }
    }

    false
}

fn scan_quote_like_character(line: &str, mut offset: usize, state: &mut ScanState) -> usize {
    let Some(mut quote_like) = state.quote_like else {
        return offset;
    };

    if quote_like.phase == QuoteLikePhase::AwaitingPairedBody {
        offset = skip_horizontal_whitespace(line, offset);
        let Some(opener) = line[offset..].chars().next() else {
            state.quote_like = Some(quote_like);
            return line.len();
        };
        if opener.is_alphanumeric() || opener == '_' {
            state.quote_like = None;
            return offset;
        }
        quote_like.begin_paired_body(opener);
        state.quote_like = Some(quote_like);
        return offset + opener.len_utf8();
    }

    let Some(ch) = line[offset..].chars().next() else {
        state.quote_like = Some(quote_like);
        return line.len();
    };

    if ch == '\\' {
        let after_slash = offset + ch.len_utf8();
        state.quote_like = Some(quote_like);
        return line[after_slash..]
            .chars()
            .next()
            .map_or(after_slash, |escaped| after_slash + escaped.len_utf8());
    }

    if quote_like.paired && ch == quote_like.opener {
        quote_like.depth = quote_like.depth.saturating_add(1);
    }

    if ch == quote_like.closer {
        if quote_like.paired && quote_like.depth > 1 {
            quote_like.depth -= 1;
        } else {
            quote_like.bodies_remaining = quote_like.bodies_remaining.saturating_sub(1);
            if quote_like.bodies_remaining == 0 {
                state.quote_like = None;
                return offset + ch.len_utf8();
            }
            if quote_like.paired {
                quote_like.phase = QuoteLikePhase::AwaitingPairedBody;
            }
        }
    }

    state.quote_like = Some(quote_like);
    offset + ch.len_utf8()
}

fn previous_word_before(text: &str, end: usize) -> Option<&str> {
    let prefix = text[..end].trim_end_matches([' ', '\t']);
    let mut start = prefix.len();
    while let Some(ch) = prefix[..start].chars().next_back() {
        if is_perl_identifier_continue(ch) || matches!(ch, ':' | '\'') {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }
    (start < prefix.len()).then(|| &prefix[start..])
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
    !after.is_some_and(|ch| is_perl_identifier_continue(ch) || matches!(ch, ':' | '\''))
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

fn heredoc_allowed_before(
    line: &str,
    offset: usize,
    known_subs: &HashSet<Box<str>>,
    hints: &HashSet<Box<str>>,
) -> bool {
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

    previous_word_before(line, offset).is_some_and(|word| is_callable_word(word, known_subs, hints))
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

    let first = line[offset..].chars().next()?;

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

    Some((PendingHeredoc { label: label.into_boxed_str(), allow_indent }, end))
}

fn parse_quoted_heredoc_label(line: &str, start: usize, quote: char) -> Option<(String, usize)> {
    let mut offset = start + quote.len_utf8();
    let mut label = String::new();

    while let Some(ch) = line[offset..].chars().next() {
        if ch == quote {
            return Some((label, offset + ch.len_utf8()));
        }
        if ch == '\\' {
            offset += ch.len_utf8();
            let escaped = line[offset..].chars().next()?;
            if quote == '\'' && !matches!(escaped, '\'' | '\\') {
                label.push('\\');
            }
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
        let source =
            "sub alpha { }\nsub beta;\nsub transform ($$) { }\nsub _private { }\nsub process2 { }";
        let table = LocalSymbolTable::scan_subs(source);

        for name in ["alpha", "beta", "transform", "_private", "process2"] {
            assert!(table.is_known_sub(name), "missing {name:?}");
        }
        assert_eq!(table.len(), 5);
    }

    #[test]
    fn unicode_qualified_legacy_and_keyword_names_use_exact_spelling() {
        let source =
            "sub café { }\nsub Foo::bar { }\nsub Foo'legacy { }\nsub ::root { }\nsub q { }";
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
        for name in
            ["commented_out", "single_quoted", "double_quoted", "backtick", "inline_comment"]
        {
            assert!(!table.is_known_sub(name), "non-code name leaked: {name:?}");
        }
    }

    #[test]
    fn multiline_ordinary_string_state_is_preserved_between_lines() {
        let source =
            "my $text = \"sub first_fake { }\nstill text with sub second_fake { }\";\nsub real { }";
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
        let source =
            concat!("my $doc = <<'END';\n", "sub builder { }\n", "END\n", "builder /pattern/;\n",);
        let table = LocalSymbolTable::scan_subs(source);
        assert!(!table.is_known_sub("builder"));

        let tokens = tokens_with_table(source, table);
        assert!(tokens.iter().any(|token| matches!(&token.token_type, TokenType::Division)));
        assert!(!tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    }

    #[test]
    fn real_forward_and_unicode_declarations_change_only_the_known_sub_path() {
        for source in ["builder /pattern/; sub builder { 1 }", "café /pattern/; sub café { 1 }"] {
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

    #[test]
    fn pod_cut_requires_the_exact_command_name() {
        let source = "=pod\nsub fake_one { }\n=cutlery\nsub fake_two { }\n=cut\nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake_one"));
        assert!(!table.is_known_sub("fake_two"));
    }

    #[test]
    fn declared_callable_heredoc_excludes_its_body() {
        let source = concat!(
            "sub sink { }\n",
            "sink <<END;\n",
            "sub fake { }\n",
            "END\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("sink"));
        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn later_declared_callable_heredoc_excludes_its_body() {
        // The declaration order that a single forward pass cannot see: `sink`
        // is only declared after the opener that uses it.
        let source = concat!("sink <<END;\n", "sub fake { }\n", "END\n", "sub sink { }\n",);
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("sink"), "the real declaration must still be recorded");
        assert!(
            !table.is_known_sub("fake"),
            "a heredoc body must not declare a sub even when its callable is declared later"
        );
    }

    #[test]
    fn later_declared_callable_heredoc_preserves_the_division_path() {
        // The consequence the exclusion exists for: a name spelled only inside
        // the heredoc body must not flip a later slash to a regex.
        let source =
            concat!("sink <<END;\n", "sub fake { }\n", "END\n", "sub sink { }\n", "fake /x/;\n",);
        let table = LocalSymbolTable::scan_subs(source);
        let tokens = tokens_with_table(source, table);

        assert!(
            !tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)),
            "a heredoc-only name must keep the division path"
        );
    }

    #[test]
    fn adjacent_builtin_string_does_not_hide_following_declarations() {
        let source = "print'hello';\nsub builder { }\nbuilder /x/;";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("builder"));
        let tokens = tokens_with_table(source, table);
        assert!(tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    }

    #[test]
    fn single_quoted_heredoc_labels_preserve_literal_backslashes() {
        let source =
            concat!("my $doc = <<'A\\B';\n", "sub fake { }\n", "A\\B\n", "sub real { }\n",);
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn ordinary_heredoc_terminators_reject_trailing_whitespace() {
        let source = concat!(
            "my $doc = <<END;\n",
            "sub fake_one { }\n",
            "END \n",
            "sub fake_two { }\n",
            "END\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake_one"));
        assert!(!table.is_known_sub("fake_two"));
    }

    #[test]
    fn empty_quoted_heredoc_labels_are_excluded_until_a_blank_line() {
        let source = concat!("my $doc = <<\"\";\n", "sub fake { }\n", "\n", "sub real { }\n",);
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn quote_like_and_pattern_bodies_cannot_create_known_subs() {
        let source = concat!(
            "my $q = q{sub q_fake { }};\n",
            "my $qq = qq{sub qq_fake { }};\n",
            "my $qw = qw(sub qw_fake);\n",
            "my $qx = qx{sub qx_fake};\n",
            "my $qr = qr{sub qr_fake { }};\n",
            "my $m = m{sub m_fake { }};\n",
            "my $s = s{sub s_fake { }}{replacement};\n",
            "my $tr = tr{sub tr_fake}{replacement};\n",
            "my $y = y{sub y_fake}{replacement};\n",
            "sub q { }\n",
            "sub Foo::s { }\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        for real in ["q", "Foo::s", "real"] {
            assert!(table.is_known_sub(real), "missing real declaration {real:?}");
        }
        for fake in [
            "q_fake", "qq_fake", "qw_fake", "qx_fake", "qr_fake", "m_fake", "s_fake", "tr_fake",
            "y_fake",
        ] {
            assert!(!table.is_known_sub(fake), "quote-like body leaked {fake:?}");
        }
    }

    #[test]
    fn quote_like_operators_after_print_return_and_callable_barewords_are_opaque() {
        let cases = [
            (
                "print",
                "print q{sub fake_print { }};\nsub real_print { }\nfake_print /x/;\n",
                "fake_print",
                "real_print",
            ),
            (
                "return",
                "sub caller_return { return q{sub fake_return { }}; }\nsub real_return { }\nfake_return /x/;\n",
                "fake_return",
                "real_return",
            ),
            (
                "callable bareword",
                "sub sink { }\nsink q{sub fake_callable { }};\nsub real_callable { }\nfake_callable /x/;\n",
                "fake_callable",
                "real_callable",
            ),
        ];

        for (context, source, fake, real) in cases {
            let table = LocalSymbolTable::scan_subs(source);
            assert!(!table.is_known_sub(fake), "{context} quote-like body leaked {fake:?}");
            assert!(table.is_known_sub(real), "{context} lost real declaration {real:?}");

            let tokens = tokens_with_table(source, table);
            assert!(
                tokens.iter().any(|token| matches!(&token.token_type, TokenType::Division)),
                "{context} fake call did not retain division path"
            );
            assert!(
                !tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)),
                "{context} fake call unexpectedly took regex path"
            );
        }
    }

    #[test]
    fn repeated_non_heredoc_shifts_do_not_invent_names() {
        let mut source = String::from("my $x = 1");
        for _ in 0..4096 {
            source.push_str(" << 1");
        }
        source.push_str(";\nsub real { }\n");
        let table = LocalSymbolTable::scan_subs(&source);

        assert_eq!(table.len(), 1);
        assert!(table.is_known_sub("real"));
    }

    #[test]
    fn unclosed_quote_like_does_not_hide_later_declarations() {
        let source = concat!(
            "my @items = qw[word1 word2\n",
            "emit \"bad\";\n",
            "print 1;\n",
            "sub emit { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("emit"));
    }

    #[test]
    fn closed_multiline_quote_like_still_excludes_declaration_text() {
        let source = concat!("my $doc = q{\n", "sub fake { }\n", "};\n", "sub real { }\n",);
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn closed_multiline_two_body_quote_like_excludes_both_bodies() {
        let source = concat!(
            "my $value = 'before';\n",
            "$value =~ s{\n",
            "sub fake_pattern { }\n",
            "}{\n",
            "sub fake_replacement { }\n",
            "};\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake_pattern"));
        assert!(!table.is_known_sub("fake_replacement"));
    }

    #[test]
    fn format_bodies_cannot_create_known_subs() {
        let source = concat!(
            "sub before { }\n",
            "format STDOUT =\n",
            "sub fake_picture { }\n",
            ".\n",
            "sub after { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("before"), "declaration before the format was lost");
        assert!(table.is_known_sub("after"), "declaration after the format was lost");
        assert!(!table.is_known_sub("fake_picture"), "format body leaked a declaration");
    }

    #[test]
    fn a_format_body_name_retains_the_division_path() {
        let source = concat!(
            "format STDOUT =\n",
            "sub fake_fmt { }\n",
            ".\n",
            "sub real_fmt { }\n",
            "fake_fmt /x/;\n",
        );
        let table = LocalSymbolTable::scan_subs(source);
        assert!(!table.is_known_sub("fake_fmt"));
        assert!(table.is_known_sub("real_fmt"));

        let tokens = tokens_with_table(source, table);
        assert!(
            tokens.iter().any(|token| matches!(&token.token_type, TokenType::Division)),
            "format-body name did not retain the division path"
        );
        assert!(
            !tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)),
            "format-body name unexpectedly took the regex path"
        );
    }

    #[test]
    fn format_opener_spellings_are_recognized() {
        let cases = [
            ("named", "format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
            ("unnamed", "format =\nsub fake { }\n.\nsub real { }\n"),
            ("qualified", "format Foo::BAR =\nsub fake { }\n.\nsub real { }\n"),
            ("indented keyword", "  format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
            ("after a statement", "my $x = 1; format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
            ("trailing comment", "format STDOUT = # pic\nsub fake { }\n.\nsub real { }\n"),
            ("padded equals", "format STDOUT =\t \nsub fake { }\n.\nsub real { }\n"),
            // `LABEL: format STDOUT =` is valid Perl and does declare a format;
            // verified by running it under perl 5.38.2.
            ("statement label", "LABEL: format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
            ("label in a block", "{ FMT: format STDOUT =\nsub fake { }\n.\n}\nsub real { }\n"),
            // perl 5.38.2 accepts a space before the label's colon, and accepts
            // more than one label on a statement; both were run to confirm.
            ("spaced label", "L : format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
            ("chained labels", "A: B: format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
            ("chained spaced labels", "A : B : format STDOUT =\nsub fake { }\n.\nsub real { }\n"),
        ];

        for (context, source) in cases {
            let table = LocalSymbolTable::scan_subs(source);
            assert!(!table.is_known_sub("fake"), "{context}: format body leaked a declaration");
            assert!(table.is_known_sub("real"), "{context}: lost the declaration after the body");
        }
    }

    #[test]
    fn format_terminator_semantics_match_perl() {
        // `perl -c` accepts trailing horizontal whitespace after the terminating
        // `.` and rejects an indented one, so an indented `.` must keep the body
        // open rather than releasing it back to code.
        let trailing_space = "format STDOUT =\nsub fake { }\n.  \nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(trailing_space);
        assert!(!table.is_known_sub("fake"));
        assert!(table.is_known_sub("real"), "a padded terminator failed to close the body");

        let crlf = "format STDOUT =\r\nsub fake { }\r\n.\r\nsub real { }\r\n";
        let table = LocalSymbolTable::scan_subs(crlf);
        assert!(!table.is_known_sub("fake"));
        assert!(table.is_known_sub("real"), "a CRLF terminator failed to close the body");

        // A real terminator follows, so the region does arm; the indented `.`
        // in between must not release it early, because perl rejects an
        // indented terminator ("Format not terminated").
        let indented_dot = concat!(
            "format STDOUT =\n",
            "sub fake { }\n",
            "  .\n",
            "sub also_fake { }\n",
            ".\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(indented_dot);
        assert!(!table.is_known_sub("fake"));
        assert!(
            !table.is_known_sub("also_fake"),
            "an indented `.` released the body, but perl does not accept it as a terminator"
        );
        assert!(table.is_known_sub("real"), "the real terminator failed to close the body");

        let empty_body = "format STDOUT =\n.\nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(empty_body);
        assert!(table.is_known_sub("real"), "an empty format body swallowed following code");

        // A dot-leader picture line begins with `.` but is ordinary report
        // formatting, not a terminator. perl does not end the body there.
        let dot_leader = concat!(
            "format STDOUT =\n",
            ".......... @<<<\n",
            "$name\n",
            "sub hidden_in_picture { 1 }\n",
            ".\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(dot_leader);
        assert!(table.is_known_sub("real"));
        assert!(
            !table.is_known_sub("hidden_in_picture"),
            "a dot-leader picture line was treated as the terminator"
        );
    }

    #[test]
    fn an_unterminated_format_opener_does_not_consume_the_rest_of_the_file() {
        // An opener is armed only when its body closes. The alternative — run to
        // EOF — loses every later declaration whenever a format-opener-shaped
        // line is really prose, which is a worse failure than the leak the
        // region exists to close, and contradicts the rule the prepass already
        // applies to malformed quote-like constructs.
        let unterminated = "format STDOUT =\nsub fake { }\nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(unterminated);
        assert!(
            table.is_known_sub("real"),
            "an unterminated opener swallowed a later real declaration"
        );

        // The motivating case, and valid Perl: this prepass does not recognize
        // `print $fh <<'END';` as a heredoc, so the opener-shaped prose inside
        // the body is reached as code. It must not erase the file.
        let prose_in_unrecognized_heredoc = concat!(
            "open(my $fh, '>', '/dev/null') or die;\n",
            "print $fh <<'END';\n",
            "Usage: declare a report with a line such as\n",
            "format STDOUT =\n",
            "followed by picture and argument lines.\n",
            "END\n",
            "sub helper { return 2 }\n",
        );
        let table = LocalSymbolTable::scan_subs(prose_in_unrecognized_heredoc);
        assert!(
            table.is_known_sub("helper"),
            "opener-shaped prose erased the declarations after it"
        );
    }

    #[test]
    fn format_keyword_lookalikes_do_not_open_a_region() {
        // Each case must be defeated by exactly one live guard — the
        // statement-position rule, the end-of-line completion check, or the
        // keyword's trailing-character check — so that dropping any one of them
        // leaves a case red. Cases that several guards reject prove none of them.
        let cases = [
            // Only the trailing-character check rejects this: `_width` would
            // otherwise parse as the format's name and leave `=` ending the
            // line. The terminator below is deliberate — without it the
            // arming check alone would refuse, and the guard would go unproven.
            ("longer name starting with the keyword", "format_width =\nsub real { }\n.\n"),
            // Only the statement-position guard rejects these: the keyword is
            // followed by a bare `=` that does end the line.
            ("scalar assignment", "my $format =\n    1;\nsub real { }\n"),
            ("package scalar assignment", "$Report::format =\n    1;\nsub real { }\n"),
            // Only the fat-comma guard rejects this: the key starts its line, so
            // the keyword genuinely sits where a statement could begin.
            ("fat comma at line start", "my %h = (\n    format =>\n    1,\n);\nsub real { }\n"),
            // Only the label rule's single-colon check rejects these. Accepting
            // a statement label must not accept the `::` package separator or a
            // ternary's `:`, both of which also leave a colon before `format`.
            ("nested package scalar", "$Deep::Report::format =\n    1;\nsub real { }\n"),
            // A label word only introduces a statement when it starts one. This
            // is a mid-edit buffer — an unfinished space-free ternary — which an
            // LSP lexes routinely, and which is the one realistic shape that
            // puts `IDENT:` in front of `format` away from a statement start.
            ("label word mid-expression", "my $x = $a ?$b: format =\nsub real { }\n"),
            // Peeling labels in a loop must not turn a ternary into a label
            // chain: a bareword branch also leaves `word :` before the keyword.
            ("ternary bareword branch", "my $x = $c ? foo : format =\nsub real { }\n"),
            ("ternary sigil branch", "my $x = $c ? $b : format =\nsub real { }\n"),
            // A Perl label is an identifier, so it cannot begin with a digit.
            ("numeric label word", "123: format =\nsub real { }\n"),
            // Rejected before either guard, but kept as ordinary regressions.
            ("fat comma inline", "my %h = (format => 1);\nsub real { }\n"),
            ("method call", "my $o; $o->format();\nsub real { }\n"),
            ("prefixed name", "sub format_it { }\nsub real { }\n"),
            ("hash subscript", "my %h; $h{format} = 1;\nsub real { }\n"),
        ];

        for (context, source) in cases {
            let table = LocalSymbolTable::scan_subs(source);
            assert!(
                table.is_known_sub("real"),
                "{context}: opened a format region and swallowed real code"
            );
        }

        // `sub format { }` declares a subroutine named `format`; the opener must
        // not consume it as a region introducer.
        let table = LocalSymbolTable::scan_subs("sub format { }\nsub real { }\n");
        assert!(table.is_known_sub("format"));
        assert!(table.is_known_sub("real"));
    }

    #[test]
    fn format_opener_text_inside_excluded_regions_does_not_open_a_region() {
        let cases = [
            ("string", "my $t = \"format STDOUT =\";\nsub real { }\n"),
            ("comment", "# format STDOUT =\nsub real { }\n"),
            ("pod", "=pod\nformat STDOUT =\n=cut\nsub real { }\n"),
            ("heredoc", "my $d = <<'END';\nformat STDOUT =\nEND\nsub real { }\n"),
            ("quote-like", "my $q = q{format STDOUT =};\nsub real { }\n"),
        ];

        for (context, source) in cases {
            let table = LocalSymbolTable::scan_subs(source);
            assert!(
                table.is_known_sub("real"),
                "{context}: opener text inside an excluded region opened a format body"
            );
        }
    }

    #[test]
    fn format_argument_line_declarations_are_a_documented_limitation() {
        // Measured with perl 5.38.2: a `sub NAME {}` on a format *argument*
        // line is genuinely declared, while the same text on a picture line is
        // not. The prepass excludes the whole body rather than growing a second
        // format grammar, so this declaration is deliberately not recorded.
        // Missing a declaration degrades to the division path, which is already
        // the standing behavior for imported subs; a false declaration would
        // instead actively mis-tokenize. See #14916.
        let source = "format STDOUT =\n@<<<\nsub argument_line { 1 }\n.\nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(
            !table.is_known_sub("argument_line"),
            "argument-line handling changed; revisit the limitation recorded on scan_subs"
        );
    }
}
