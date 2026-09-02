//! Deterministic heredoc body ownership for the Pest instrument (#8220).
//!
//! # Selected contract
//!
//! This crate **supports** heredoc bodies. A heredoc opener owns the physical
//! lines that follow its logical line up to — and excluding — its terminator
//! line. Those bytes become [`AstNode::Heredoc::content`] and are removed from
//! the text handed to Pest, so following code resumes at the line after the
//! terminator instead of the body being re-parsed as ordinary statements.
//!
//! The alternative contract offered by #8220 — reporting every heredoc as
//! unsupported — was rejected: the body extent must be scanned to resume
//! following code correctly at all, and once the extent is known the content is
//! already in hand.
//!
//! An *ordinary complete success carrying silently empty content* is no longer
//! reachable for a terminated heredoc: content is empty only when the body is
//! genuinely empty. Every case where this module cannot own a body truthfully —
//! a missing terminator, a body over budget, more queued openers than the depth
//! budget — produces a typed [`ParseDiagnostic`] and a non-`Complete`
//! [`ParseCompleteness`] through
//! [`PureRustPerlParser::parse_heredoc_outcome`].
//!
//! # Claim boundary
//!
//! This module claims the **heredoc** contract only. Whole-source accounting for
//! every construct remains #8093's `parse_strict`/`parse_recovering` row; a
//! `Complete` outcome here means "no heredoc opener lost or truncated a body",
//! not "every input byte is accounted for". This is a comparison instrument, not
//! the production parser: production heredoc lexing is owned by `perl-lexer`.
//!
//! # Ownership rules
//!
//! Openers are recognized in *term* position only, mirroring the production
//! lexer's `LexerMode::ExpectOperator` split, so `1 << 2` and `1 <<2` stay left
//! shifts. After a bareword, term position is decided by
//! [`BUILTIN_LIST_OP_NAMES`], which mirrors the grammar's `builtin_list_op_name`
//! — the scanner must agree with the grammar, because recognizing an opener the
//! grammar rejects deletes real source. A bare marker must follow `<<`/`<<~`
//! immediately, matching Perl's "use of bare `<<` to mean `<<\"\"` is
//! forbidden"; quoted and escaped markers may be separated by horizontal
//! whitespace.
//!
//! Non-code regions own no openers, and recognizing them needs context the
//! current line does not carry, so the walk tracks it: comments, single-line
//! strings and quote-like operators, quoted runs left open by a previous line,
//! POD blocks (`=word` through `=cut`), and everything after `__DATA__` or
//! `__END__`. `<<MARKER`-shaped text in any of them is data.
//!
//! A terminator line is the marker alone, followed immediately by a line ending
//! or end of input — a trailing space does *not* terminate, matching Perl. For
//! `<<~`, the terminator may be indented, its indentation is stripped from each
//! body line, and that indentation must be a prefix of every non-blank body
//! line's — Perl treats a mismatch as a fatal compile error, so a mismatch here
//! is not a terminator rather than a `Complete` heredoc with a fabricated body.

use std::collections::VecDeque;

use crate::outcome::{
    ParseAttempt, ParseCompleteness, ParseDiagnostic, ParseDiagnosticKind, ParseOutcome,
    ParserFailure, RecoveryAction, SourceRange, StrictParseError,
};
use crate::pure_rust_parser::{AstNode, PureRustPerlParser};

/// Maximum source bytes one heredoc body may own.
///
/// Mirrors `perl-lexer`'s `MAX_HEREDOC_BYTES` so both instruments degrade at the
/// same budget. A body over budget is truncated and reported, never silently
/// accepted.
pub const MAX_HEREDOC_BODY_BYTES: usize = 256 * 1024;

/// Maximum heredoc openers queued from one physical line.
///
/// Mirrors `perl-lexer`'s `MAX_HEREDOC_DEPTH`.
pub const MAX_HEREDOC_DEPTH: usize = 100;

/// How a heredoc marker was spelled on its opener.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeredocDelimiterForm {
    /// `<<EOF`
    Bare,
    /// `<<'EOF'`
    SingleQuoted,
    /// `<<"EOF"`
    DoubleQuoted,
    /// ``<<`EOF` ``
    Backtick,
    /// `<<\EOF`
    Escaped,
}

impl HeredocDelimiterForm {
    /// Stable machine name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bare => "bare",
            Self::SingleQuoted => "single-quoted",
            Self::DoubleQuoted => "double-quoted",
            Self::Backtick => "backtick",
            Self::Escaped => "escaped",
        }
    }

    /// Whether this form suppresses interpolation in real Perl.
    #[must_use]
    pub const fn is_non_interpolating(self) -> bool {
        matches!(self, Self::SingleQuoted | Self::Escaped)
    }
}

/// Why a heredoc body could not be owned truthfully.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeredocDefect {
    /// No terminator line exists; the remainder of the source was taken as body.
    MissingTerminator,
    /// `<< MARKER` with intervening whitespace before a bare marker.
    ///
    /// Perl rejects this outright ("Use of bare `<<` to mean `<<\"\"` is
    /// forbidden"), but this crate's grammar admits it because whitespace is
    /// implicit between `<<` and the delimiter. The opener owns no body.
    SeparatedBareMarker,
    /// The body exceeded [`MAX_HEREDOC_BODY_BYTES`] and was truncated.
    BodyOverBudget,
    /// More than [`MAX_HEREDOC_DEPTH`] openers were queued from one line.
    DepthOverBudget,
}

impl HeredocDefect {
    /// Stable machine name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingTerminator => "missing-terminator",
            Self::SeparatedBareMarker => "separated-bare-marker",
            Self::BodyOverBudget => "body-over-budget",
            Self::DepthOverBudget => "depth-over-budget",
        }
    }
}

/// One heredoc opener and the disposition of the body it owns.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeredocCapture {
    marker: String,
    form: HeredocDelimiterForm,
    indented: bool,
    content: String,
    opener: SourceRange,
    body: SourceRange,
    defect: Option<HeredocDefect>,
}

impl HeredocCapture {
    /// Terminator spelling, without its quotes.
    #[must_use]
    pub fn marker(&self) -> &str {
        &self.marker
    }

    /// How the marker was spelled on the opener.
    #[must_use]
    pub const fn form(&self) -> HeredocDelimiterForm {
        self.form
    }

    /// Whether the opener used the `<<~` indented form.
    #[must_use]
    pub const fn indented(&self) -> bool {
        self.indented
    }

    /// Owned body text, with `<<~` indentation already stripped.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Byte range of the `<<...` opener in the original source.
    #[must_use]
    pub const fn opener(&self) -> SourceRange {
        self.opener
    }

    /// Byte range of the owned body in the original source.
    ///
    /// Covers the body lines and the terminator line when one exists, so the
    /// range is exactly the text removed from the parsed source.
    #[must_use]
    pub const fn body(&self) -> SourceRange {
        self.body
    }

    /// Defect, when the body could not be owned truthfully.
    #[must_use]
    pub const fn defect(&self) -> Option<HeredocDefect> {
        self.defect
    }

    /// Whether a terminator line was found.
    #[must_use]
    pub const fn terminated(&self) -> bool {
        !matches!(self.defect, Some(HeredocDefect::MissingTerminator))
    }
}

/// Result of the deterministic heredoc pre-pass.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeredocScan {
    stripped: String,
    captures: Vec<HeredocCapture>,
    diagnostics: Vec<ParseDiagnostic>,
    recovery_ranges: Vec<SourceRange>,
}

impl HeredocScan {
    /// Source with every owned heredoc body removed.
    ///
    /// This is the text handed to Pest. Opener lines are preserved verbatim.
    #[must_use]
    pub fn stripped(&self) -> &str {
        &self.stripped
    }

    /// Captures in source order.
    #[must_use]
    pub fn captures(&self) -> &[HeredocCapture] {
        &self.captures
    }

    /// Diagnostics for bodies that could not be owned truthfully.
    #[must_use]
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Original-source ranges covered by a heredoc defect.
    #[must_use]
    pub fn recovery_ranges(&self) -> &[SourceRange] {
        &self.recovery_ranges
    }

    /// Completeness of the **heredoc** contract for this source.
    ///
    /// `Complete` when every opener owns a terminated, in-budget body.
    /// `Unsupported` when the depth budget was exceeded. `Recovered` otherwise.
    #[must_use]
    pub fn completeness(&self) -> ParseCompleteness {
        completeness_for(&self.diagnostics)
    }
}

/// Run the deterministic heredoc pre-pass over `source`.
///
/// Never panics and never allocates more than `source.len()` for the stripped
/// text plus the owned bodies. Output is a pure function of `source`.
#[must_use]
pub fn scan(source: &str) -> HeredocScan {
    let mut scanner = Scanner::new(source);
    scanner.run();
    scanner.finish()
}

/// A heredoc opener found on one logical line, before its body is consumed.
#[derive(Debug, Clone)]
struct PendingOpener {
    marker: String,
    form: HeredocDelimiterForm,
    indented: bool,
    opener: SourceRange,
}

struct Scanner<'a> {
    source: &'a str,
    stripped: String,
    captures: Vec<HeredocCapture>,
    diagnostics: Vec<ParseDiagnostic>,
    recovery_ranges: Vec<SourceRange>,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            stripped: String::with_capacity(source.len()),
            captures: Vec::new(),
            diagnostics: Vec::new(),
            recovery_ranges: Vec::new(),
        }
    }

    fn finish(self) -> HeredocScan {
        let Self { stripped, captures, mut diagnostics, mut recovery_ranges, .. } = self;
        ParseDiagnostic::sort_slice(&mut diagnostics);
        recovery_ranges.sort_by_key(|range| (range.start(), range.end()));
        HeredocScan { stripped, captures, diagnostics, recovery_ranges }
    }

    /// Walk physical lines, emitting non-body lines and consuming owned bodies.
    ///
    /// Opener recognition carries lexical context across lines: POD blocks,
    /// `__DATA__`/`__END__` sections, and quoted runs left open by the previous
    /// line are all non-code, and `<<MARKER`-shaped text inside them is data.
    fn run(&mut self) {
        let lines = physical_lines(self.source);
        let mut index = 0;
        let mut region = Region::Code;
        let mut open_construct: Option<OpenConstruct> = None;
        while index < lines.len() {
            let line = lines[index];
            let text = &self.source[line.start..line.end];
            let content = &self.source[line.start..line.content_end];
            self.stripped.push_str(text);
            index += 1;

            match region {
                // Everything after the sentinel is data, never code.
                Region::Data => continue,
                Region::Pod => {
                    if content.starts_with("=cut") {
                        region = Region::Code;
                    }
                    continue;
                }
                Region::Code => {}
            }

            if open_construct.is_none() {
                if is_data_sentinel(content) {
                    region = Region::Data;
                    continue;
                }
                if is_pod_start(content) {
                    region = Region::Pod;
                    continue;
                }
            }

            let mut openers = Vec::new();
            open_construct = scan_line_openers(text, line.start, &mut openers, open_construct);
            if openers.is_empty() {
                continue;
            }

            if openers.len() > MAX_HEREDOC_DEPTH {
                self.record_depth_over_budget(line);
                openers.truncate(MAX_HEREDOC_DEPTH);
            }

            for opener in openers {
                match opener {
                    LineOpener::Owned(opener) => {
                        index = self.consume_body(&lines, index, opener);
                    }
                    LineOpener::Unowned(opener) => self.record_unowned(opener),
                }
            }
        }
    }

    /// Consume the body owned by `opener`, starting at line `index`.
    ///
    /// Returns the index of the first line after the terminator.
    fn consume_body(
        &mut self,
        lines: &[PhysicalLine],
        index: usize,
        opener: PendingOpener,
    ) -> usize {
        let body_start = lines.get(index).map_or(self.source.len(), |line| line.start);
        let mut cursor = index;
        let mut terminator: Option<usize> = None;
        let mut over_budget_at: Option<usize> = None;
        // Common indentation of the non-blank body lines seen so far. `<<~`
        // only accepts a terminator whose own indentation is a prefix of it.
        let mut body_indent: Option<&str> = None;
        let mut body_has_content = false;

        while cursor < lines.len() {
            let line = lines[cursor];
            if line.end.saturating_sub(body_start) > MAX_HEREDOC_BODY_BYTES {
                over_budget_at = Some(cursor);
                break;
            }
            let content = &self.source[line.start..line.content_end];
            if is_terminator_line(
                content,
                &opener.marker,
                opener.indented,
                body_indent,
                body_has_content,
            ) {
                terminator = Some(cursor);
                break;
            }
            let indent = leading_horizontal_whitespace(content);
            if indent.len() != content.len() {
                body_has_content = true;
                body_indent = Some(match body_indent {
                    Some(common) => common_indent(common, indent),
                    None => indent,
                });
            }
            cursor += 1;
        }

        let body_line_end = terminator.or(over_budget_at).unwrap_or(lines.len());
        let content_end = lines.get(body_line_end).map_or(self.source.len(), |line| line.start);
        let raw_body = self.source.get(body_start..content_end).unwrap_or_default();

        let indent = terminator
            .filter(|_| opener.indented)
            .and_then(|line_index| lines.get(line_index))
            .map(|line| leading_horizontal_whitespace(&self.source[line.start..line.content_end]))
            .unwrap_or("");
        let content = strip_body_indent(raw_body, indent);

        // The removed span covers the body and, when present, the terminator line.
        let removed_end = terminator
            .and_then(|line_index| lines.get(line_index))
            .map_or(content_end, |line| line.end);
        let resume = match terminator {
            Some(line_index) => line_index + 1,
            None => body_line_end,
        };
        let Some(body_range) = source_range(body_start, removed_end, self.source) else {
            // Unreachable: `source_range` clamps its bounds. Fail closed by
            // owning no body rather than recording an invented range.
            return resume;
        };

        let defect = if terminator.is_some() {
            None
        } else if over_budget_at.is_some() {
            Some(HeredocDefect::BodyOverBudget)
        } else {
            Some(HeredocDefect::MissingTerminator)
        };

        match defect {
            None => {}
            Some(HeredocDefect::BodyOverBudget) => {
                self.record_defect(
                    body_range,
                    ParseDiagnosticKind::SkippedSource,
                    RecoveryAction::Skip,
                    format!(
                        "heredoc `{}` body exceeds the {MAX_HEREDOC_BODY_BYTES}-byte budget; \
                         the body was truncated at that boundary and its terminator was not sought",
                        opener.marker
                    ),
                );
            }
            Some(_) => {
                self.record_defect(
                    body_range,
                    ParseDiagnosticKind::RecoveredFragment,
                    RecoveryAction::ResumeAfter,
                    format!(
                        "heredoc `{}` has no terminator line; the remainder of the source \
                         was taken as its body",
                        opener.marker
                    ),
                );
            }
        }

        self.captures.push(HeredocCapture {
            marker: opener.marker,
            form: opener.form,
            indented: opener.indented,
            content,
            opener: opener.opener,
            body: body_range,
            defect,
        });

        // Everything up to `removed_end` leaves the parsed source. When the body
        // ran over budget the remainder is still parsed, so resume there.
        resume
    }

    /// Record an opener this crate's grammar admits but Perl rejects.
    ///
    /// It is still a capture, so the queue stays aligned with the openers the
    /// grammar produces; its empty content is explained by the diagnostic.
    fn record_unowned(&mut self, opener: PendingOpener) {
        self.record_defect(
            opener.opener,
            ParseDiagnosticKind::UnsupportedSyntax,
            RecoveryAction::Skip,
            format!(
                "`<<` separated from bare marker `{}` by whitespace is not a heredoc in Perl; \
                 this opener owns no body",
                opener.marker
            ),
        );
        self.captures.push(HeredocCapture {
            marker: opener.marker,
            form: opener.form,
            indented: opener.indented,
            content: String::new(),
            opener: opener.opener,
            body: opener.opener,
            defect: Some(HeredocDefect::SeparatedBareMarker),
        });
    }

    fn record_depth_over_budget(&mut self, line: PhysicalLine) {
        let Some(range) = source_range(line.start, line.end, self.source) else {
            return;
        };
        self.record_defect(
            range,
            ParseDiagnosticKind::UnsupportedSyntax,
            RecoveryAction::Skip,
            format!(
                "more than {MAX_HEREDOC_DEPTH} heredoc openers on one line is not supported; \
                 openers beyond the budget own no body"
            ),
        );
    }

    fn record_defect(
        &mut self,
        range: SourceRange,
        kind: ParseDiagnosticKind,
        action: RecoveryAction,
        message: String,
    ) {
        self.diagnostics.push(ParseDiagnostic::new(kind, range, message, None, Some(action)));
        if !self.recovery_ranges.contains(&range) {
            self.recovery_ranges.push(range);
        }
    }
}

/// Build a range clamped into `source`.
///
/// The clamp makes `start <= end <= source.len()` hold, so the `None` arm is
/// unreachable; callers fail closed on it rather than inventing a range.
fn source_range(start: usize, end: usize, source: &str) -> Option<SourceRange> {
    let end = end.min(source.len());
    let start = start.min(end);
    SourceRange::try_new(start, end).ok()
}

/// One physical line: `start..end` includes the line terminator,
/// `start..content_end` excludes it.
#[derive(Debug, Clone, Copy)]
struct PhysicalLine {
    start: usize,
    content_end: usize,
    end: usize,
}

fn physical_lines(source: &str) -> Vec<PhysicalLine> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let mut content_end = index;
        if content_end > start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        lines.push(PhysicalLine { start, content_end, end: index + 1 });
        start = index + 1;
    }
    if start < bytes.len() {
        lines.push(PhysicalLine { start, content_end: bytes.len(), end: bytes.len() });
    }
    lines
}

fn leading_horizontal_whitespace(line: &str) -> &str {
    let end = line.bytes().position(|byte| byte != b' ' && byte != b'\t').unwrap_or(line.len());
    line.get(..end).unwrap_or("")
}

/// A terminator line is the marker alone, with no trailing bytes.
///
/// For `<<~` the marker may be preceded by horizontal whitespace, but only when
/// that whitespace is a prefix of every non-blank body line's indentation —
/// mirroring `perl-lexer`'s `heredoc_terminator_line_end`. Perl treats a
/// mismatch as a fatal compile error ("Indentation on line N of here-doc
/// doesn't match delimiter"), so accepting one here would report a `Complete`
/// heredoc for source Perl refuses to compile, with a fabricated body.
fn is_terminator_line(
    line: &str,
    marker: &str,
    indented: bool,
    body_indent: Option<&str>,
    body_has_content: bool,
) -> bool {
    if !indented {
        return line == marker;
    }
    let indent = leading_horizontal_whitespace(line);
    let candidate = line.get(indent.len()..).unwrap_or("");
    if candidate != marker {
        return false;
    }
    if !body_has_content {
        return true;
    }
    body_indent.is_some_and(|common| common.starts_with(indent))
}

/// Longest common leading-whitespace prefix of `left` and `right`.
fn common_indent<'a>(left: &'a str, right: &str) -> &'a str {
    let shared = left.bytes().zip(right.bytes()).take_while(|(a, b)| a == b).count();
    left.get(..shared).unwrap_or("")
}

/// Strip `indent` from the front of every line of `body`.
///
/// `is_terminator_line` has already established that `indent` is a prefix of
/// every non-blank body line, so only blank lines can come up short.
fn strip_body_indent(body: &str, indent: &str) -> String {
    if indent.is_empty() {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    for line in split_inclusive_lines(body) {
        out.push_str(line.strip_prefix(indent).unwrap_or_else(|| {
            let matched =
                line.bytes().zip(indent.bytes()).take_while(|(left, right)| left == right).count();
            line.get(matched..).unwrap_or(line)
        }));
    }
    out
}

fn split_inclusive_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive('\n')
}

// --- Opener recognition ----------------------------------------------------

/// Words after which a `<<` still starts a term rather than a left shift.
///
/// Public so the drift guard in the contract suite can compare it with the
/// grammar without reaching into private state.
///
/// This must agree with the grammar, because the scanner removes body lines the
/// grammar would otherwise parse: recognizing an opener the grammar rejects
/// deletes real source. The grammar admits a heredoc as an argument of
/// `builtin_list_op`, so its `builtin_list_op_name` alternation is the
/// authority — `BUILTIN_LIST_OP_NAMES` mirrors it and
/// `grammar_builtin_list_ops_match_scanner` in `heredoc_body_contract.rs`
/// parses `grammar.pest` and fails if the two ever drift.
///
/// The remaining entries are control-flow and low-precedence operators after
/// which Perl also expects a term. A `<<` after any other bareword is treated
/// as a shift, matching the grammar's `shift_expression`; a heredoc opened
/// after a user-defined sub name therefore owns no body and reports empty
/// content rather than removing source.
pub const BUILTIN_LIST_OP_NAMES: [&str; 108] = [
    "print",
    "say",
    "warn",
    "die",
    "printf",
    "bless",
    "open",
    "close",
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "grep",
    "map",
    "sort",
    "join",
    "split",
    "substr",
    "sprintf",
    "chomp",
    "chop",
    "defined",
    "undef",
    "ref",
    "scalar",
    "keys",
    "values",
    "each",
    "delete",
    "exists",
    "length",
    "reverse",
    "index",
    "rindex",
    "ord",
    "chr",
    "lc",
    "uc",
    "lcfirst",
    "ucfirst",
    "abs",
    "int",
    "hex",
    "oct",
    "sqrt",
    "exp",
    "log",
    "sin",
    "cos",
    "atan2",
    "rand",
    "srand",
    "time",
    "localtime",
    "gmtime",
    "stat",
    "lstat",
    "glob",
    "readdir",
    "telldir",
    "seekdir",
    "rewinddir",
    "closedir",
    "opendir",
    "rename",
    "unlink",
    "chmod",
    "chown",
    "mkdir",
    "rmdir",
    "symlink",
    "readlink",
    "link",
    "truncate",
    "pack",
    "unpack",
    "vec",
    "binmode",
    "eof",
    "fileno",
    "flock",
    "getc",
    "read",
    "readline",
    "seek",
    "tell",
    "sysopen",
    "sysread",
    "syswrite",
    "sysseek",
    "syscall",
    "select",
    "eval",
    "exit",
    "fork",
    "wait",
    "waitpid",
    "system",
    "exec",
    "kill",
    "sleep",
    "alarm",
    "getpgrp",
    "getppid",
    "getpriority",
    "setpgrp",
    "setpriority",
];

/// Control-flow and operator keywords after which Perl expects a term.
const TERM_POSITION_KEYWORDS: [&str; 8] =
    ["return", "and", "or", "not", "if", "unless", "while", "until"];

fn is_term_position_word(word: &str) -> bool {
    BUILTIN_LIST_OP_NAMES.contains(&word) || TERM_POSITION_KEYWORDS.contains(&word)
}

/// Non-code region the line walk is currently inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    /// Ordinary Perl code.
    Code,
    /// A POD block, from `=word` to `=cut`.
    Pod,
    /// After `__DATA__` or `__END__`; never code again.
    Data,
}

/// A quoted or quote-like run left open at the end of a line.
#[derive(Debug, Clone, Copy)]
struct OpenConstruct {
    open: u8,
    close: u8,
    depth: usize,
}

/// `__DATA__` / `__END__` end the code region for the rest of the source.
fn is_data_sentinel(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed == "__DATA__" || trimmed == "__END__"
}

/// POD starts at a line beginning `=` followed by an identifier, except `=cut`.
fn is_pod_start(line: &str) -> bool {
    line.strip_prefix('=').is_some_and(|rest| rest.starts_with(|ch: char| ch.is_ascii_alphabetic()))
        && !line.starts_with("=cut")
}

/// Find every heredoc opener on one physical line.
///
/// `line_start` is the line's byte offset in the original source, so recorded
/// ranges are original-source ranges. `carried` is a quoted run left open by the
/// previous line; the return value is the run left open by this one, so a
/// string spanning several lines never has its interior scanned for openers.
fn scan_line_openers(
    line: &str,
    line_start: usize,
    out: &mut Vec<LineOpener>,
    carried: Option<OpenConstruct>,
) -> Option<OpenConstruct> {
    let bytes = line.as_bytes();
    let mut index = 0;

    if let Some(construct) = carried {
        let (next, still_open) = continue_construct(bytes, construct);
        if let Some(open) = still_open {
            return Some(open);
        }
        index = next;
    }

    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'#' if !is_length_sigil(bytes, index) => return None,
            b'\'' | b'"' | b'`' => {
                let (next, open) = skip_quoted(bytes, index, byte);
                if let Some(open) = open {
                    return Some(open);
                }
                index = next;
            }
            b'<' if bytes.get(index + 1) == Some(&b'<') => {
                match parse_opener(line, bytes, index, line_start) {
                    Some((opener, next)) => {
                        out.push(opener);
                        index = next;
                    }
                    None => index += 2,
                }
            }
            _ => match skip_quote_like(line, bytes, index) {
                Some((_next, Some(open))) => return Some(open),
                Some((next, None)) => index = next,
                None => index += 1,
            },
        }
    }
    None
}

/// Resume a construct left open by a previous line.
///
/// Returns the index just past its close, or the construct still open.
fn continue_construct(bytes: &[u8], construct: OpenConstruct) -> (usize, Option<OpenConstruct>) {
    let balanced = construct.close != construct.open;
    let mut depth = construct.depth;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            index += 2;
            continue;
        }
        if balanced && byte == construct.open {
            depth += 1;
        } else if byte == construct.close {
            depth -= 1;
            if depth == 0 {
                return (index + 1, None);
            }
        }
        index += 1;
    }
    (bytes.len(), Some(OpenConstruct { depth, ..construct }))
}

/// `$#array` and `$#{...}` are not comments.
fn is_length_sigil(bytes: &[u8], index: usize) -> bool {
    index > 0 && bytes[index - 1] == b'$'
}

/// Skip a `'`/`"`/`` ` ``-delimited run starting at `open`. Returns the index
/// after the closing delimiter, or the line end when unterminated.
fn skip_quoted(bytes: &[u8], open: usize, delimiter: u8) -> (usize, Option<OpenConstruct>) {
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == delimiter => return (index + 1, None),
            _ => index += 1,
        }
    }
    // Unterminated on this line: the run continues on the next one.
    (bytes.len(), Some(OpenConstruct { open: delimiter, close: delimiter, depth: 1 }))
}

/// Quote-like operators whose contents must not be scanned for openers.
const QUOTE_LIKE_OPERATORS: [&str; 9] = ["qq", "qw", "qx", "qr", "tr", "q", "m", "s", "y"];

/// Skip a quote-like operator (`q{...}`, `qq(...)`, `m/.../`, `s{...}{...}`)
/// starting at `index`. Returns `None` when no operator starts here.
#[allow(clippy::type_complexity)]
fn skip_quote_like(
    line: &str,
    bytes: &[u8],
    index: usize,
) -> Option<(usize, Option<OpenConstruct>)> {
    if index > 0 && is_word_byte(bytes[index - 1]) {
        return None;
    }
    let rest = line.get(index..)?;
    let name = QUOTE_LIKE_OPERATORS
        .into_iter()
        .find(|op| rest.starts_with(op) && !rest[op.len()..].starts_with(is_word_char))?;

    let mut cursor = index + name.len();
    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        cursor += 1;
    }
    let open = *bytes.get(cursor)?;
    // A bare word followed by `=>` or `,` is a hash key, not an operator.
    if open.is_ascii_alphanumeric() || open == b'_' || open == b'=' || open == b',' || open == b';'
    {
        return None;
    }

    let sections = if matches!(name, "s" | "tr" | "y") { 2 } else { 1 };
    let mut end = cursor;
    for section in 0..sections {
        let opener = if section == 0 || closing_delimiter(open) != open {
            // Bracketing delimiters restart with their own opener for section 2.
            if section == 0 { open } else { *bytes.get(end)? }
        } else {
            open
        };
        let (next, still_open) = skip_delimited(bytes, end, opener);
        if let Some(open) = still_open {
            return Some((bytes.len(), Some(open)));
        }
        end = next;
    }
    // Trailing flags (`m/x/gi`).
    while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
        end += 1;
    }
    Some((end, None))
}

const fn closing_delimiter(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    }
}

/// Skip one delimited section beginning at `open_index`. Balanced when the
/// delimiter is a bracket pair.
fn skip_delimited(bytes: &[u8], open_index: usize, open: u8) -> (usize, Option<OpenConstruct>) {
    let close = closing_delimiter(open);
    let balanced = close != open;
    let mut depth = 1usize;
    let mut index = open_index + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            index += 2;
            continue;
        }
        if balanced && byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
            if depth == 0 {
                return (index + 1, None);
            }
        }
        index += 1;
    }
    // Unterminated on this line: the construct continues on the next one.
    (bytes.len(), Some(OpenConstruct { open, close, depth }))
}

const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// A `<<` the scanner recognized as an opener.
#[derive(Debug, Clone)]
enum LineOpener {
    /// Owns the body lines below its logical line.
    Owned(PendingOpener),
    /// Admitted by this crate's grammar but rejected by Perl, so it owns no
    /// body.
    Unowned(PendingOpener),
}

/// Parse a heredoc opener at `index` (`bytes[index..index + 2] == "<<"`).
///
/// Returns the opener and the index just past it, or `None` when this `<<` is a
/// left shift or carries no valid marker.
fn parse_opener(
    line: &str,
    bytes: &[u8],
    index: usize,
    line_start: usize,
) -> Option<(LineOpener, usize)> {
    if !is_term_position(line, bytes, index) {
        return None;
    }
    let mut cursor = index + 2;
    let indented = if bytes.get(cursor) == Some(&b'~') {
        cursor += 1;
        true
    } else {
        false
    };

    let after_sigils = cursor;
    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        cursor += 1;
    }
    let had_space = cursor != after_sigils;

    let (form, marker, end) = match *bytes.get(cursor)? {
        b'\'' => read_quoted_marker(bytes, cursor, b'\'', HeredocDelimiterForm::SingleQuoted)?,
        b'"' => read_quoted_marker(bytes, cursor, b'"', HeredocDelimiterForm::DoubleQuoted)?,
        b'`' => read_quoted_marker(bytes, cursor, b'`', HeredocDelimiterForm::Backtick)?,
        b'\\' => {
            let (marker, end) = read_bare_marker(bytes, cursor + 1)?;
            (HeredocDelimiterForm::Escaped, marker, end)
        }
        _ => {
            let (marker, end) = read_bare_marker(bytes, cursor)?;
            (HeredocDelimiterForm::Bare, marker, end)
        }
    };

    let opener = SourceRange::try_new(line_start + index, line_start + end).ok()?;
    let pending = PendingOpener { marker, form, indented, opener };

    // Perl forbids bare `<< EOF`. This crate's grammar admits it because
    // whitespace is implicit between `<<` and the delimiter, so the opener is
    // recorded but owns no body — its empty content is explained, not silent.
    if had_space && form == HeredocDelimiterForm::Bare {
        return Some((LineOpener::Unowned(pending), end));
    }
    Some((LineOpener::Owned(pending), end))
}

fn read_bare_marker(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    let mut end = start;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    if end == start {
        return None;
    }
    let marker = std::str::from_utf8(bytes.get(start..end)?).ok()?;
    Some((marker.to_string(), end))
}

fn read_quoted_marker(
    bytes: &[u8],
    open: usize,
    delimiter: u8,
    form: HeredocDelimiterForm,
) -> Option<(HeredocDelimiterForm, String, usize)> {
    let (marker, end) = read_bare_marker(bytes, open + 1)?;
    if bytes.get(end) != Some(&delimiter) {
        return None;
    }
    Some((form, marker, end + 1))
}

/// Whether `<<` at `index` sits where a term may start.
///
/// Mirrors the production lexer's `ExpectOperator` split: after a value —
/// an identifier that is not a list operator, a number, a closing bracket, a
/// variable, or a string — `<<` is a left shift.
fn is_term_position(line: &str, bytes: &[u8], index: usize) -> bool {
    let mut cursor = index;
    while cursor > 0 && matches!(bytes[cursor - 1], b' ' | b'\t') {
        cursor -= 1;
    }
    let Some(previous) = cursor.checked_sub(1).and_then(|at| bytes.get(at)) else {
        return true;
    };
    match *previous {
        b')' | b']' | b'}' | b'\'' | b'"' | b'`' => false,
        byte if byte.is_ascii_digit() => false,
        byte if is_word_byte(byte) => {
            let word_start = line
                .get(..cursor)
                .map(|head| head.trim_end_matches(is_word_char).len())
                .unwrap_or(cursor);
            line.get(word_start..cursor).is_some_and(is_term_position_word)
        }
        _ => true,
    }
}

// --- Parser integration ----------------------------------------------------

impl PureRustPerlParser {
    /// Parse `source` and return the **heredoc-scoped** typed outcome (#8220).
    ///
    /// Completeness reports the heredoc contract only: `Complete` means every
    /// opener owned a terminated, in-budget body. Whole-source accounting for
    /// every construct remains #8093's row and is not claimed here.
    ///
    /// A Pest rejection that recovery cannot turn into an AST is returned as
    /// [`ParseAttempt::Rejected`]; an outcome that cannot satisfy the vocabulary
    /// invariants is returned as [`ParseAttempt::Failed`] rather than being
    /// downgraded to a success.
    pub fn parse_heredoc_outcome(&mut self, source: &str) -> ParseAttempt<AstNode> {
        let scan = scan(source);
        let mut diagnostics = scan.diagnostics().to_vec();
        let mut recovery_ranges = scan.recovery_ranges().to_vec();

        let ast = match self.parse_scanned(&scan) {
            Ok(ast) => ast,
            Err(error) => {
                let Some(range) = source_range(0, source.len(), source) else {
                    return ParseAttempt::failed(ParserFailure::instrument(
                        "could not bind the rejection range to the source",
                    ));
                };
                return ParseAttempt::rejected(StrictParseError::new(
                    range,
                    "pest rejected the source and recovery produced no statements",
                    error.to_string(),
                ));
            }
        };

        // A captured body whose opener never reached an AST node would vanish
        // without this: the bytes left the parsed source but nothing represents
        // them. Report it rather than returning a quietly lossy success.
        for capture in self.unattached_heredocs(&scan) {
            diagnostics.push(ParseDiagnostic::new(
                ParseDiagnosticKind::SkippedSource,
                capture.body(),
                format!(
                    "heredoc `{}` owned a body but its opener produced no node; \
                     the body is not represented in the AST",
                    capture.marker()
                ),
                None,
                Some(RecoveryAction::Skip),
            ));
            if !recovery_ranges.contains(&capture.body()) {
                recovery_ranges.push(capture.body());
            }
        }

        let completeness = completeness_for(&diagnostics);
        match ParseOutcome::try_new(ast, completeness, diagnostics, recovery_ranges, source) {
            Ok(outcome) => ParseAttempt::outcome(outcome),
            Err(error) => ParseAttempt::failed(ParserFailure::instrument(error.to_string())),
        }
    }

    /// Captures still queued after a parse, i.e. bodies no opener node claimed.
    fn unattached_heredocs<'a>(&self, scan: &'a HeredocScan) -> Vec<&'a HeredocCapture> {
        let remaining = self.queued_heredoc_bodies();
        let claimed = scan.captures().len().saturating_sub(remaining);
        scan.captures().get(claimed..).unwrap_or_default().iter().collect()
    }
}

/// Heredoc-contract completeness implied by `diagnostics`.
fn completeness_for(diagnostics: &[ParseDiagnostic]) -> ParseCompleteness {
    if diagnostics.is_empty() {
        return ParseCompleteness::Complete;
    }
    if diagnostics.iter().any(|d| matches!(d.kind(), ParseDiagnosticKind::UnsupportedSyntax)) {
        return ParseCompleteness::Unsupported;
    }
    ParseCompleteness::Recovered
}

/// Queue of bodies awaiting attachment to their opener nodes.
///
/// Consumed front-to-back in source order; a body is attached only when its
/// marker matches the opener the grammar produced, so a scanner/grammar
/// disagreement leaves content empty instead of attaching the wrong body.
#[derive(Debug, Default)]
pub(crate) struct HeredocQueue {
    pending: VecDeque<(String, String)>,
}

impl HeredocQueue {
    pub(crate) fn from_scan(scan: &HeredocScan) -> Self {
        Self {
            pending: scan
                .captures()
                .iter()
                .map(|capture| (capture.marker().to_string(), capture.content().to_string()))
                .collect(),
        }
    }

    /// Take the body for `marker` when it is the next queued capture.
    pub(crate) fn take(&mut self, marker: &str) -> Option<String> {
        if self.pending.front().is_some_and(|(queued, _)| queued == marker) {
            return self.pending.pop_front().map(|(_, content)| content);
        }
        None
    }

    /// Bodies no opener node has claimed yet. Always a suffix of the scan's
    /// captures, because entries are only ever taken from the front.
    pub(crate) fn remaining(&self) -> usize {
        self.pending.len()
    }
}
