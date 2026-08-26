#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Tree-sitter Perl grammar binding (C FFI).
//!
//! This crate is the conventional C/tree-sitter reference implementation for
//! Perl, maintained alongside the native v3 Rust parser ([`perl-parser`]) for
//! compatibility testing and comparison. It vendors a snapshot of the upstream
//! [tree-sitter-perl] C grammar (`parser.c` + `scanner.c`) under `c-src/` and
//! exposes it via a hand-written FFI declaration — no `bindgen` or `libclang`
//! dependency is required to build.
//!
//! ## Relation to `tree-sitter-perl-rs`
//!
//! [`tree-sitter-perl-rs`] is a thin facade over the native v3 Rust parser and
//! is the recommended choice for new Rust projects. This crate (`tree-sitter-perl-c`)
//! should be preferred when:
//!
//! - **Compatibility testing** — comparing parse output against the upstream
//!   C reference grammar.
//! - **Non-Rust tree-sitter tooling** — the C grammar snapshot can be used as
//!   a build dependency for language bindings in other languages.
//! - **Baseline benchmarking** — measuring parse throughput of the C grammar
//!   against the native v3 parser.
//!
//! ## Build requirements
//!
//! Only a C compiler is required (e.g., `cc`/`gcc`/`clang` on Linux/macOS,
//! MSVC or MinGW on Windows). No `libclang` or `bindgen` toolchain is needed.
//!
//! ## Quick start
//!
//! ```rust
//! use tree_sitter_perl_c::parse_perl_code;
//!
//! let tree = parse_perl_code("my $x = 42;").unwrap();
//! assert!(!tree.root_node().has_error());
//! println!("{}", tree.root_node().to_sexp());
//! ```
//!
//! [tree-sitter-perl]: https://github.com/tree-sitter-perl/tree-sitter-perl
//! [`perl-parser`]: https://docs.rs/perl-parser
//! [`tree-sitter-perl-rs`]: https://docs.rs/tree-sitter-perl-rs
#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]

use std::{
    fmt,
    path::{Path, PathBuf},
};
use tree_sitter::{Language, Parser};

/// Reusable Perl parser for hot parse loops.
///
/// Construct once and call [`PerlParser::parse_bytes`] or
/// [`PerlParser::parse_code`] repeatedly to avoid parser setup overhead.
#[non_exhaustive]
pub struct PerlParser {
    parser: Parser,
}

/// Typed errors produced by Perl parse helpers in this crate.
#[non_exhaustive]
#[derive(Debug)]
pub enum ParsePerlError {
    /// Configuring the parser with the Perl language failed.
    LanguageSetup(tree_sitter::LanguageError),
    /// Tree-sitter returned `None` instead of a parse tree.
    ParseReturnedNone,
    /// Reading source bytes from disk failed.
    Io(std::io::Error),
    /// Parsing completed but the resulting tree contains syntax errors.
    ///
    /// Produced by the summary APIs ([`parse_perl_summary`],
    /// [`try_parse_perl_summary`]), which fail closed rather than hand back a
    /// summary of a broken tree. Recover the error-bearing tree through
    /// [`try_parse_perl_code`].
    MalformedSource,
}

impl fmt::Display for ParsePerlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LanguageSetup(error) => {
                write!(f, "failed to configure parser language: {error:?}")
            }
            Self::ParseReturnedNone => write!(f, "tree-sitter returned no parse tree"),
            Self::Io(error) => write!(f, "failed to read Perl source file: {error}"),
            Self::MalformedSource => write!(f, "parsed tree contains syntax errors"),
        }
    }
}

impl std::error::Error for ParsePerlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LanguageSetup(error) => Some(error),
            Self::ParseReturnedNone => None,
            Self::Io(error) => Some(error),
            Self::MalformedSource => None,
        }
    }
}

impl From<tree_sitter::LanguageError> for ParsePerlError {
    fn from(value: tree_sitter::LanguageError) -> Self {
        Self::LanguageSetup(value)
    }
}

impl From<std::io::Error> for ParsePerlError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Contextual parse error produced by [`parse_perl_file`].
///
/// Wraps the original [`ParsePerlError`] and records the source path so
/// callers surface actionable diagnostics without re-implementing path
/// tracking themselves.
#[non_exhaustive]
#[derive(Debug)]
pub struct ParsePerlFileError {
    path: PathBuf,
    source: ParsePerlError,
}

impl ParsePerlFileError {
    /// Returns the path that triggered the error.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for ParsePerlFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to parse Perl file {}: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for ParsePerlFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Returns the tree-sitter [`Language`] for Perl (C grammar).
///
/// Use this to configure a [`tree_sitter::Parser`] or to create query objects
/// that target the Perl grammar.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_c::language;
/// use tree_sitter::Parser;
///
/// let lang = language();
/// let mut parser = Parser::new();
/// parser.set_language(&lang).unwrap();
/// ```
pub fn language() -> Language {
    // SAFETY: `tree_sitter_perl` is generated by tree-sitter-cli and linked via `build.rs`.
    // It returns a raw pointer to a valid static TSLanguage struct for the Perl grammar. The function
    // has no preconditions on the calling thread, takes no arguments, holds no borrows, and
    // cannot cause aliasing or memory-safety issues. Soundness depends on the build script
    // linking the correct, ABI-compatible parser object, which cc::Build in build.rs guarantees.
    unsafe { tree_sitter_perl() }
}

/// The vendored upstream `queries/injections.scm` source.
///
/// Mirrors how upstream grammar bindings embed their query files as public
/// string constants; the copy lives inside this crate (like `c-src/`) so the
/// published package stays self-contained. Provenance is recorded in
/// [`UPSTREAM_SNAPSHOT.md`].
///
/// [`UPSTREAM_SNAPSHOT.md`]: crate documentation root
pub const INJECTIONS_QUERY: &str = include_str!("../queries/injections.scm");

/// The vendored upstream `queries/highlights.scm` source.
///
/// See [`INJECTIONS_QUERY`] for the embedding/provenance contract.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../queries/highlights.scm");

/// Compiles [`INJECTIONS_QUERY`] against the Perl language.
///
/// This is the public, typed entry point for consumers that previously had to
/// locate the repository's vendored `.scm` files and call
/// `tree_sitter::Query::new` themselves.
///
/// # Example
///
/// ```rust
/// let query = tree_sitter_perl_c::load_injections_query().unwrap();
/// assert!(query.capture_names().iter().any(|name| *name == "injection.content"));
/// ```
///
/// # Errors
///
/// Returns [`tree_sitter::QueryError`] if the embedded source stops matching
/// the compiled grammar (a snapshot consistency fault, not caller input).
pub fn load_injections_query() -> Result<tree_sitter::Query, tree_sitter::QueryError> {
    load_query(INJECTIONS_QUERY)
}

/// Compiles [`HIGHLIGHTS_QUERY`] against the Perl language.
///
/// # Known snapshot delta
///
/// The upstream `highlights.scm` copied at snapshot time references newer
/// grammar surface (`postfix_deref` literal-token children, `slices`
/// `hashref:`/`arrayref:` fields) than the frozen `c-src/` parser validates,
/// so compilation of the *full* file currently returns
/// [`tree_sitter::QueryError`] (kind `Structure`, first offender
/// `postfix_deref`). The embedded bytes are never patched silently; callers
/// needing working highlight rules today can compile extracted fragments
/// against [`language`] (the approach used by `tests/query_conformance.rs`).
/// A `c-src/`/queries snapshot refresh that removes the delta turns the
/// drift tripwire test green and unlocks this loader end-to-end.
///
/// # Errors
///
/// Returns [`tree_sitter::QueryError`] under the same conditions as
/// [`load_injections_query`]; see the known snapshot delta above.
pub fn load_highlights_query() -> Result<tree_sitter::Query, tree_sitter::QueryError> {
    load_query(HIGHLIGHTS_QUERY)
}

fn load_query(source: &str) -> Result<tree_sitter::Query, tree_sitter::QueryError> {
    tree_sitter::Query::new(&language(), source)
}

/// Creates a [`tree_sitter::Parser`] configured for Perl.
///
/// Returns an error if the language version is incompatible with the linked
/// tree-sitter runtime (this should not happen in practice).
///
/// Prefer this over [`create_parser`] in new code — it surfaces errors
/// explicitly.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_c::try_create_parser;
///
/// let mut parser = try_create_parser().unwrap();
/// let tree = parser.parse("my $x = 1;", None).unwrap();
/// assert!(!tree.root_node().has_error());
/// ```
pub fn try_create_parser() -> Result<Parser, tree_sitter::LanguageError> {
    let mut parser = Parser::new();
    parser.set_language(&language())?;
    Ok(parser)
}

impl PerlParser {
    /// Creates a reusable Perl parser instance.
    pub fn new() -> Result<Self, tree_sitter::LanguageError> {
        Ok(Self { parser: try_create_parser()? })
    }

    /// Parses Perl source bytes using this parser instance.
    pub fn parse_bytes(&mut self, code: &[u8]) -> Result<tree_sitter::Tree, ParsePerlError> {
        try_parse_with_parser(&mut self.parser, code)
    }

    /// Parses Perl source text using this parser instance.
    pub fn parse_code(&mut self, code: &str) -> Result<tree_sitter::Tree, ParsePerlError> {
        self.parse_bytes(code.as_bytes())
    }
}

fn try_parse_with_parser(
    parser: &mut Parser,
    code: &[u8],
) -> Result<tree_sitter::Tree, ParsePerlError> {
    match parser.parse(code, None) {
        Some(tree) => Ok(tree),
        None => Err(ParsePerlError::ParseReturnedNone),
    }
}

/// Creates a [`tree_sitter::Parser`] configured for Perl, silently ignoring
/// language-set errors.
///
/// This is a compatibility shim. Prefer [`try_create_parser`] in new code so
/// that version mismatches are not swallowed.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_c::create_parser;
///
/// let parser = create_parser();
/// assert!(parser.language().is_some());
/// ```
pub fn create_parser() -> Parser {
    let mut parser = Parser::new();
    let _ = parser.set_language(&language());
    parser
}

/// Parses Perl source bytes and returns the resulting [`tree_sitter::Tree`].
///
/// This accepts arbitrary bytes so callers can parse non-UTF-8 source files,
/// for example Perl scripts with Latin-1 encoded strings or binary data
/// embedded in `__DATA__` sections.
///
/// # Notes
///
/// The tree-sitter C grammar receives the raw bytes as-is. A UTF-8 BOM
/// (`\xEF\xBB\xBF`) at the start of the file is not stripped automatically
/// and may produce an error node in the resulting tree.  Strip it before
/// calling this function if strict grammar compliance is required.
///
/// # Errors
///
/// Returns an error if the parser cannot be initialised (version mismatch) or
/// if tree-sitter returns `None` from `parse` (cancelled or timed out).
pub fn parse_perl_bytes(code: &[u8]) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    try_parse_perl_bytes(code).map_err(Into::into)
}

/// Parses Perl source bytes and returns the resulting [`tree_sitter::Tree`].
///
/// This typed variant allows callers to distinguish parser setup failures from
/// parse cancellation/timeouts (`None` from tree-sitter).
pub fn try_parse_perl_bytes(code: &[u8]) -> Result<tree_sitter::Tree, ParsePerlError> {
    let mut parser = try_create_parser().map_err(ParsePerlError::LanguageSetup)?;
    try_parse_with_parser(&mut parser, code)
}

/// Parses Perl source bytes using a caller-provided configured [`tree_sitter::Parser`].
///
/// This helper is intended for performance-sensitive code paths where a single
/// parser is reused across many snippets. The parser must already be configured
/// with [`language`].
///
/// # Errors
///
/// Returns an error if tree-sitter returns `None` from `parse` (cancelled or
/// timed out).
pub fn parse_perl_bytes_with_parser(
    parser: &mut Parser,
    code: &[u8],
) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    try_parse_perl_bytes_with_parser(parser, code).map_err(Into::into)
}

/// Parses Perl source bytes using a caller-provided configured [`tree_sitter::Parser`].
///
/// This typed variant allows callers to explicitly handle parse cancellation/timeouts
/// (`None` from tree-sitter) as [`ParsePerlError::ParseReturnedNone`].
pub fn try_parse_perl_bytes_with_parser(
    parser: &mut Parser,
    code: &[u8],
) -> Result<tree_sitter::Tree, ParsePerlError> {
    try_parse_with_parser(parser, code)
}

/// Parses a Perl source string and returns the resulting [`tree_sitter::Tree`].
///
/// # Errors
///
/// Returns an error if the parser cannot be initialised (version mismatch) or
/// if tree-sitter returns `None` from `parse` (cancelled or timed out).
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_c::parse_perl_code;
///
/// let tree = parse_perl_code("my $x = 42;").unwrap();
/// assert!(!tree.root_node().has_error());
/// ```
pub fn parse_perl_code(code: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    try_parse_perl_code(code).map_err(Into::into)
}

/// Parses a Perl source string and returns the resulting [`tree_sitter::Tree`].
///
/// This typed variant allows callers to inspect whether parser setup failed or
/// tree-sitter returned no parse tree.
pub fn try_parse_perl_code(code: &str) -> Result<tree_sitter::Tree, ParsePerlError> {
    try_parse_perl_bytes(code.as_bytes())
}

/// Parses a Perl source string using a caller-provided configured [`tree_sitter::Parser`].
///
/// This helper avoids creating and configuring a new parser for each parse call.
/// The parser must already be configured with [`language`].
///
/// # Errors
///
/// Returns an error if tree-sitter returns `None` from `parse` (cancelled or
/// timed out).
pub fn parse_perl_code_with_parser(
    parser: &mut Parser,
    code: &str,
) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    try_parse_perl_code_with_parser(parser, code).map_err(Into::into)
}

/// Parses a Perl source string using a caller-provided configured [`tree_sitter::Parser`].
///
/// This typed variant allows callers to explicitly handle parse cancellation/timeouts
/// (`None` from tree-sitter) as [`ParsePerlError::ParseReturnedNone`].
pub fn try_parse_perl_code_with_parser(
    parser: &mut Parser,
    code: &str,
) -> Result<tree_sitter::Tree, ParsePerlError> {
    try_parse_perl_bytes_with_parser(parser, code.as_bytes())
}

/// Reads a file from `path` and parses it as Perl source.
///
/// # Errors
///
/// Returns a [`ParsePerlFileError`] (boxed) if the file cannot be read or if
/// the parser cannot be initialised or returns no tree. The error message
/// includes the file path to aid diagnostics.
pub fn parse_perl_file<P: AsRef<Path>>(
    path: P,
) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    let path_ref = path.as_ref();
    let code = std::fs::read(path_ref).map_err(|e| {
        Box::new(ParsePerlFileError { path: path_ref.to_path_buf(), source: ParsePerlError::Io(e) })
            as Box<dyn std::error::Error>
    })?;
    try_parse_perl_bytes(&code).map_err(|source| {
        Box::new(ParsePerlFileError { path: path_ref.to_path_buf(), source })
            as Box<dyn std::error::Error>
    })
}

/// Reads a file from `path` and parses it as Perl source.
///
/// This typed variant allows callers to distinguish IO failures from parser
/// setup and parse-`None` outcomes.
pub fn try_parse_perl_file<P: AsRef<Path>>(path: P) -> Result<tree_sitter::Tree, ParsePerlError> {
    let code = std::fs::read(path).map_err(ParsePerlError::Io)?;
    try_parse_perl_bytes(&code)
}

/// Ergonomic summary of a clean Perl parse.
///
/// Returned by [`parse_perl_summary`] / [`try_parse_perl_summary`] instead of a
/// bare [`tree_sitter::Tree`]. The raw tree stays reachable through
/// [`ParseResult::tree`] / [`ParseResult::into_tree`], so power users lose no
/// capability — this is an additive wrapper, not an AST replacement.
#[non_exhaustive]
#[derive(Debug)]
pub struct ParseResult {
    tree: tree_sitter::Tree,
    node_count: usize,
}

impl ParseResult {
    /// Whether the underlying tree contains syntax errors.
    ///
    /// Summaries handed out by [`try_parse_perl_summary`] fail closed on
    /// malformed source, so callers receiving one always see `false` here;
    /// the value is derived from the live tree rather than baked in so the
    /// type stays honest independent of how it was obtained.
    pub fn has_error(&self) -> bool {
        self.tree.root_node().has_error()
    }

    /// Total number of syntax nodes in the parsed tree, including unnamed tokens.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// S-expression rendering of the root node, computed from the live tree
    /// each call (never cached).
    pub fn root_sexp(&self) -> String {
        self.tree.root_node().to_sexp()
    }

    /// Borrows the underlying parse tree for advanced inspection.
    pub fn tree(&self) -> &tree_sitter::Tree {
        &self.tree
    }

    /// Consumes the summary and returns the underlying parse tree.
    pub fn into_tree(self) -> tree_sitter::Tree {
        self.tree
    }
}

/// Counts every node under `root` (named and unnamed) with an explicit cursor
/// walk, so pathological nesting depth cannot exhaust the stack through
/// recursion.
fn count_tree_nodes(root: tree_sitter::Node<'_>) -> usize {
    let mut count = 0usize;
    let mut cursor = root.walk();
    'descend: loop {
        count += 1;
        if cursor.goto_first_child() || cursor.goto_next_sibling() {
            continue 'descend;
        }
        while cursor.goto_parent() {
            if cursor.goto_next_sibling() {
                continue 'descend;
            }
        }
        break 'descend;
    }
    count
}

/// Parses a Perl source string and returns an ergonomic [`ParseResult`] summary.
///
/// Unlike [`parse_perl_code`], this fails closed on malformed source: a parse
/// that completes but contains syntax error nodes yields
/// [`ParsePerlError::MalformedSource`] instead of a summary dressed over a
/// broken tree.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_c::parse_perl_summary;
///
/// let summary = parse_perl_summary("my $x = 42;").unwrap();
/// assert!(!summary.has_error());
/// assert!(summary.node_count() > 1);
/// assert!(summary.root_sexp().starts_with("(source_file"));
/// ```
///
/// # Errors
///
/// Returns a boxed [`ParsePerlError`] when the parser cannot be initialised,
/// tree-sitter returns no tree, or the source is malformed.
pub fn parse_perl_summary(code: &str) -> Result<ParseResult, Box<dyn std::error::Error>> {
    try_parse_perl_summary(code).map_err(Into::into)
}

/// Typed variant of [`parse_perl_summary`].
///
/// Returns [`ParsePerlError::MalformedSource`] when the parse completes but the
/// tree contains syntax error nodes; use [`try_parse_perl_code`] when you need
/// the error-bearing tree itself.
///
/// # Errors
///
/// Surfaces every failure mode as its typed [`ParsePerlError`] variant.
pub fn try_parse_perl_summary(code: &str) -> Result<ParseResult, ParsePerlError> {
    let tree = try_parse_perl_code(code)?;
    if tree.root_node().has_error() {
        return Err(ParsePerlError::MalformedSource);
    }
    Ok(ParseResult { node_count: count_tree_nodes(tree.root_node()), tree })
}

/// Returns the scanner backend identifier for this crate.
///
/// Always returns `"c-scanner"`. Useful when code needs to distinguish between
/// this crate and the Rust-native [`tree-sitter-perl-rs`] backend.
///
/// [`tree-sitter-perl-rs`]: https://docs.rs/tree-sitter-perl-rs
pub fn get_scanner_config() -> &'static str {
    "c-scanner"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Query, QueryCursor, StreamingIterator};

    fn capture_text<'a>(
        query: &'a Query,
        code: &'a str,
        capture: tree_sitter::QueryCapture<'a>,
    ) -> Option<(&'a str, &'a str)> {
        let name = query.capture_names().get(capture.index as usize)?;
        let text = capture.node.utf8_text(code.as_bytes()).ok()?;
        Some((*name, text))
    }

    #[test]
    fn test_language_loading() {
        let lang = language();
        let count = lang.node_kind_count();
        println!("C implementation node kind count: {}", count);
        // Language is valid if we can get its node kind count
        assert!(count > 0);
    }

    #[test]
    fn test_basic_parsing() -> Result<(), Box<dyn std::error::Error>> {
        let code = "my $var = 'hello';";
        let tree = parse_perl_code(code)?;
        assert!(!tree.root_node().has_error());
        Ok(())
    }

    #[test]
    fn test_parse_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let code = b"my $var = 'hello';";
        let tree = parse_perl_bytes(code)?;
        assert!(!tree.root_node().has_error());
        Ok(())
    }

    #[test]
    fn test_parse_bytes_with_reused_parser() -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = try_create_parser()?;

        let first = parse_perl_bytes_with_parser(&mut parser, b"my $x = 1;")?;
        assert!(!first.root_node().has_error());

        let second = parse_perl_bytes_with_parser(&mut parser, b"my $y = 2;")?;
        assert!(!second.root_node().has_error());

        Ok(())
    }

    #[test]
    fn test_parse_code_with_reused_parser() -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = try_create_parser()?;

        let first = parse_perl_code_with_parser(&mut parser, "my $name = 'Perl';")?;
        assert!(!first.root_node().has_error());

        let second = parse_perl_code_with_parser(&mut parser, "print $name;")?;
        assert!(!second.root_node().has_error());

        Ok(())
    }

    #[test]
    fn test_typed_parse_none_error_variant_is_emitted() {
        let mut parser = Parser::new();
        let result = try_parse_with_parser(&mut parser, b"my $var = 'hello';");
        assert!(matches!(result, Err(ParsePerlError::ParseReturnedNone)));
    }

    #[test]
    fn test_typed_language_setup_error_variant_mapping() {
        let error = ParsePerlError::from(tree_sitter::LanguageError::Version(0));
        assert!(matches!(error, ParsePerlError::LanguageSetup(_)));
    }

    #[test]
    fn test_parser_creation() {
        let parser = create_parser();
        assert!(parser.language().is_some());
    }

    #[test]
    fn test_reusable_parser_parses_multiple_inputs() -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = PerlParser::new()?;
        let first = parser.parse_code("my $x = 1;")?;
        let second = parser.parse_code("my $y = 2;")?;
        assert!(!first.root_node().has_error());
        assert!(!second.root_node().has_error());
        Ok(())
    }

    /// Verify that error state from one parse does not bleed into the next.
    /// A parser reused after parsing invalid Perl must produce a clean tree
    /// for the subsequent valid input.
    #[test]
    fn test_reusable_parser_error_state_does_not_bleed() -> Result<(), Box<dyn std::error::Error>> {
        let mut parser = PerlParser::new()?;
        // First parse: syntactically invalid Perl — tree must exist but have error nodes.
        let bad_tree = parser.parse_code("my $x = @@@@@@;")?;
        assert!(bad_tree.root_node().has_error(), "invalid Perl should produce error nodes");
        // Second parse: valid Perl — must produce a clean tree despite the previous error.
        let good_tree = parser.parse_code("my $y = 42;")?;
        assert!(!good_tree.root_node().has_error(), "valid Perl after error parse must be clean");
        Ok(())
    }

    #[test]
    fn test_inline_cpp_injection_query_matches_heredoc_body()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = "use Inline CPP => <<'END_CPP';\n#include <string>\nclass Greet {};\nEND_CPP\n";
        let tree = parse_perl_code(code)?;
        let query = Query::new(&language(), INJECTIONS_QUERY)?;
        let mut cursor = QueryCursor::new();
        let mut matched = false;
        let mut matches = cursor.matches(&query, tree.root_node(), code.as_bytes());
        while let Some(m) = matches.next() {
            let mut saw_inline_package = false;
            let mut saw_inline_language = false;
            let mut saw_injection_content = false;

            for capture in m.captures {
                if let Some((name, text)) = capture_text(&query, code, *capture) {
                    match name {
                        "inline.package" => saw_inline_package = text == "Inline",
                        "inline.language" => saw_inline_language = text == "CPP",
                        "injection.content" => {
                            saw_injection_content = capture.node.kind() == "heredoc_content"
                                && text.contains("#include <string>");
                        }
                        _ => {}
                    }
                }
            }

            if saw_inline_package && saw_inline_language && saw_injection_content {
                matched = true;
                break;
            }
        }

        assert!(matched, "expected Inline::CPP heredoc to match the injection query");
        Ok(())
    }

    /// Verify that `parse_perl_bytes` returns a tree (possibly with error nodes) for
    /// input prefixed with a UTF-8 BOM.  The BOM is NOT stripped; callers are responsible
    /// for removing it if the grammar produces undesired error nodes.
    #[test]
    fn test_parse_bytes_with_utf8_bom_returns_tree() -> Result<(), Box<dyn std::error::Error>> {
        // UTF-8 BOM (\xEF\xBB\xBF) followed by valid Perl
        let bom_source = b"\xEF\xBB\xBFmy $x = 1;";
        let tree = parse_perl_bytes(bom_source)?;
        // The tree must be returned even if the BOM causes an error node
        assert_eq!(tree.root_node().kind(), "source_file");
        Ok(())
    }

    /// Verify that `parse_perl_bytes` handles a completely empty input.
    #[test]
    fn test_parse_bytes_empty_source() -> Result<(), Box<dyn std::error::Error>> {
        let tree = parse_perl_bytes(b"")?;
        assert_eq!(tree.root_node().kind(), "source_file");
        Ok(())
    }

    fn query_has_capture(query: &Query, expected: &str) -> bool {
        query.capture_names().contains(&expected)
    }

    #[test]
    fn load_injections_query_compiles_and_matches_inline_cpp_heredoc()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = "use Inline CPP => <<'END_CPP';\n#include <string>\nclass Greet {};\nEND_CPP\n";
        let tree = parse_perl_code(code)?;
        let query = load_injections_query()?;
        assert!(query_has_capture(&query, "injection.content"));
        assert!(query_has_capture(&query, "inline.package"));

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), code.as_bytes());
        let mut saw_injection_content_heredoc = false;
        while let Some(m) = matches.next() {
            for capture in m.captures {
                if capture.node.kind() == "heredoc_content" {
                    saw_injection_content_heredoc = true;
                }
            }
        }
        assert!(
            saw_injection_content_heredoc,
            "expected the public injection loader to yield heredoc_content captures"
        );
        Ok(())
    }

    #[test]
    fn load_highlights_query_fails_closed_on_snapshot_drift()
    -> Result<(), Box<dyn std::error::Error>> {
        // Tripwire: the upstream highlights.scm copied at snapshot time does
        // not fully validate against the frozen c-src/ parser (first
        // offender: `postfix_deref` literal children at row 136). When a
        // snapshot refresh resolves the delta, flip this test to the
        // positive-capture form used by the injections loader and record the
        // refreshed fingerprints in UPSTREAM_SNAPSHOT.md.
        let Err(error) = load_highlights_query() else {
            return Err(
                "snapshot drift resolved: flip this tripwire to positive-capture assertions".into(),
            );
        };
        let rendered = error.to_string();
        assert!(
            rendered.contains("postfix_deref"),
            "expected the pinned first-offender pattern in the query error, got: {rendered}"
        );

        // Positive discrimination: the language + query machinery itself is
        // healthy; fragments of the same file compile and capture normally
        // (same technique as tests/query_conformance.rs).
        let fragment_query = Query::new(&language(), "(comment) @comment")?;
        let code = "# just a comment\nmy $x = 1;\n";
        let tree = parse_perl_code(code)?;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&fragment_query, tree.root_node(), code.as_bytes());
        let mut saw_comment_capture = false;
        while let Some(m) = matches.next() {
            for capture in m.captures {
                if capture.node.kind() == "comment" {
                    saw_comment_capture = true;
                }
            }
        }
        assert!(saw_comment_capture, "expected highlight fragment query to capture comments");
        Ok(())
    }

    #[test]
    fn parse_perl_summary_reports_clean_tree_facts() -> Result<(), Box<dyn std::error::Error>> {
        let summary = parse_perl_summary("my $x = 42;\n")?;
        assert!(!summary.has_error());
        assert!(summary.node_count() > 1, "a source tree has more than its root node");
        assert_eq!(summary.tree().root_node().kind(), "source_file");
        assert!(summary.root_sexp().starts_with("(source_file"));
        Ok(())
    }

    #[test]
    fn parse_perl_summary_fails_closed_on_malformed_source() {
        let result = try_parse_perl_summary("my $x = @@@@@@;");
        assert!(matches!(result, Err(ParsePerlError::MalformedSource)));
    }

    #[test]
    fn parse_perl_summary_boxed_variant_propagates_malformed_source() {
        let result = parse_perl_summary("my $x = @@@@@@;");
        assert!(result.is_err(), "boxed variant must also fail closed on error trees");
    }

    #[test]
    fn parse_result_into_tree_preserves_root() -> Result<(), Box<dyn std::error::Error>> {
        let summary = try_parse_perl_summary("print 1;\n")?;
        let tree = summary.into_tree();
        assert!(!tree.root_node().has_error());
        Ok(())
    }
}

// SAFETY: See the SAFETY comment on the `language()` function above.
// This is the only unsafe code in the crate — the single FFI symbol we need
// from the compiled C grammar. No bindgen is used; the declaration is
// hand-written to avoid a libclang build dependency.
unsafe extern "C" {
    fn tree_sitter_perl() -> Language;
}
