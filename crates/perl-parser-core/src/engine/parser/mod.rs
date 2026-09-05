//! Recursive descent Perl parser.
//!
//! Consumes tokens from `perl-lexer` and produces AST nodes with error recovery.
//! The parser handles operator precedence, quote-like operators, and heredocs,
//! while tracking recursion depth to prevent stack overflows on malformed input.
//!
//! # IDE-Friendly Error Recovery
//!
//! This parser uses an **IDE-friendly error recovery model**:
//!
//! - **Returns `Ok(ast)` with ERROR nodes** for most parse failures (recovered errors)
//! - **Returns `Err`** only for catastrophic failures (recursion limits, etc.)
//!
//! This means `result.is_err()` is **not** the correct way to check for parse errors.
//! Instead, check for ERROR nodes in the AST or use `parser.errors()`:
//!
//! ```rust,ignore
//! let mut parser = Parser::new(code);
//! match parser.parse() {
//!     Err(_) => println!("Catastrophic parse failure"),
//!     Ok(ast) => {
//!         // Check for recovered errors via ERROR nodes
//!         if ast.to_sexp().contains("ERROR") {
//!             println!("Parse errors recovered: {:?}", parser.errors());
//!         }
//!     }
//! }
//! ```
//!
//! ## Why IDE-Friendly?
//!
//! Traditional compilers return `Err` on any syntax error. This prevents:
//! - Code completion in incomplete code
//! - Go-to-definition while typing
//! - Hover information in files with errors
//!
//! By returning partial ASTs with ERROR nodes, editors can provide useful
//! features even when code is incomplete or contains errors.
//!
//! # Performance
//!
//! - **Time complexity**: O(n) for typical token streams
//! - **Space complexity**: O(n) for AST storage with bounded recursion memory usage
//! - **Optimizations**: Fast-path parsing and efficient recovery to maintain performance
//! - **Benchmarks**: ~150µs–1ms for typical files; low ms for large file inputs
//! - **Large-scale notes**: Tuned to scale for large workspaces (50GB PST-style scans)
//!
//! # Usage
//!
//! ```rust
//! use perl_parser_core::Parser;
//!
//! let mut parser = Parser::new("my $var = 42; sub hello { print $var; }");
//! let ast = parser.parse();
//! ```

use crate::{
    ast::{GotoTargetForm, Node, NodeKind, SourceLocation},
    error::{ParseError, ParseOutput, ParseResult, ParseStopCause, RecoveryKind, RecoverySite},
    heredoc_collector::{self, HeredocContent, PendingHeredoc, collect_at_declaration_offsets},
    quote_parser,
    token_stream::{ContextualOpResult, ContextualTokenOp, Token, TokenKind, TokenStream},
};
use perl_lexer::LexerMode;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

mod operation;
use operation::ParserOperationContext;
pub use operation::{ParserConfigIdentity, ParserOperationId};

/// Strip Perl-style line comments from `qw()` content.
///
/// In Perl, `#` inside `qw()` begins a comment that extends to the end of the
/// line (see perlop: "A # character within the list is treated as a comment
/// character"). This function removes those comment segments so that
/// `split_whitespace()` sees only the actual list elements.
fn strip_qw_comments(content: &str) -> String {
    content
        .lines()
        .map(|line| if let Some(pos) = line.find('#') { &line[..pos] } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Upper bound on retained test-only decision events, so a large source cannot
/// grow the trace without bound.
#[cfg(test)]
pub(crate) const MAX_DECISION_TRACE: usize = 256;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParserDecision {
    UnknownLowercaseBarewordCall,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParserDecisionTrace {
    pub(crate) decision: ParserDecision,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Parser state for a single Perl source input.
///
/// Construct with [`Parser::new`] and call [`Parser::parse`] to obtain an AST.
/// Non-fatal syntax errors are collected and can be accessed via [`Parser::errors`].
///
/// Every ordinary strict or recovery-aware parse owns one
/// [`ParserConfigIdentity`] and one live operation context (budget identity,
/// live [`crate::BudgetTracker`], cancellation-probe handle, operation
/// identity, and terminal-state accumulator). Production recursion depth is
/// checked, recorded, and unwound through that context. [`parse_with_recovery`]
/// returns the live tracker, not a post-hoc reconstruction.
///
/// [`crate::parser_context::ParserContext`] is a parallel AST-v2 helper and is
/// not this production authority (#8700 B04 / #7105).
pub struct Parser<'a> {
    /// Token stream providing access to lexed Perl script content
    tokens: TokenStream<'a>,
    /// Syntactic block nesting for `NestingTooDeep`. This is not the
    /// production resource-control depth; that lives on the operation
    /// context tracker and is governed by [`Parser::with_depth`].
    block_depth: usize,
    /// Position tracking for error reporting and AST location information
    last_end_position: usize,
    /// Context flag for disambiguating for-loop initialization syntax
    in_for_loop_init: bool,
    /// Depth of nested class bodies for context-sensitive class-body constructs
    in_class_body: usize,
    /// Statement boundary tracking for indirect object syntax detection
    at_stmt_start: bool,
    /// FIFO queue of pending heredoc declarations awaiting content collection
    pending_heredocs: VecDeque<PendingHeredoc>,
    /// Custom attributes registered by Attribute::Handlers declarations in this file.
    custom_attribute_handlers: HashSet<String>,
    /// Whether `use Attribute::Handlers;` has been seen in this file.
    attribute_handlers_enabled: bool,
    /// Source bytes for heredoc content collection (shared with token stream)
    src_bytes: &'a [u8],
    /// Byte cursor tracking position for heredoc content collection
    byte_cursor: usize,
    /// Delimiter from an unrecognised heredoc introducer whose body leaked into
    /// the ordinary token stream.  Only the matching bareword may be exempted.
    heredoc_recovery_tag: Option<String>,
    /// Start time of parsing for timeout enforcement (specifically heredocs)
    heredoc_start_time: Option<Instant>,
    /// Collection of parse errors encountered during parsing (for error recovery)
    errors: Vec<ParseError>,
    /// Live production operation context. Fresh counters, terminal state, and
    /// operation identity start at each [`Parser::parse`] / [`Parser::parse_with_recovery`]
    /// entry; configuration and the cancellation-probe handle are retained.
    operation: ParserOperationContext,
    /// Semantic decision events emitted by the actual production route in unit tests,
    /// capped at [`MAX_DECISION_TRACE`] entries.
    #[cfg(test)]
    decision_trace: Vec<ParserDecisionTrace>,
    /// Test-only mutation control that preserves the AST while bypassing route evidence.
    #[cfg(test)]
    bypass_unknown_lowercase_bareword_decision: bool,
}

// Recursion limit is set conservatively to prevent stack overflow
// before the limit triggers. The actual stack usage depends on the
// number of function frames between recursion checks (about 20-30
// for the precedence parsing chain). 128 * 30 = ~3840 frames which
// is safe. Real Perl code rarely exceeds 20-30 nesting levels.
pub(crate) const MAX_RECURSION_DEPTH: usize = 128;
pub(crate) const MAX_BLOCK_NESTING_DEPTH: usize = 512;

impl<'a> Parser<'a> {
    /// Create a new parser for the provided Perl source.
    ///
    /// # Arguments
    ///
    /// * `input` - Perl source code to be parsed
    ///
    /// # Returns
    ///
    /// A configured parser ready to parse the provided source.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::Parser;
    ///
    /// let script = "use strict; my $filter = qr/important/;";
    /// let mut parser = Parser::new(script);
    /// // Parser ready to parse the source
    /// ```
    pub fn new(input: &'a str) -> Self {
        Self::with_production_config(input, ParserConfigIdentity::production_default())
    }

    /// Construct a parser with an explicit production configuration identity.
    ///
    /// [`Parser::new`] and [`Parser::new_with_recovery_config`] select
    /// [`ParserConfigIdentity::production_default`] through this same path.
    pub fn with_production_config(input: &'a str, config: ParserConfigIdentity) -> Self {
        Self::assemble(TokenStream::new(input), input, config, None)
    }

    /// Immutable configuration identity selected for this parser.
    pub fn config_identity(&self) -> ParserConfigIdentity {
        self.operation.config()
    }

    /// Identity of the current (or last-started) parse operation.
    pub fn operation_id(&self) -> ParserOperationId {
        self.operation.operation_id()
    }

    fn assemble(
        tokens: TokenStream<'a>,
        source: &'a str,
        config: ParserConfigIdentity,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Self {
        Parser {
            tokens,
            block_depth: 0,
            last_end_position: 0,
            in_for_loop_init: false,
            in_class_body: 0,
            at_stmt_start: true,
            pending_heredocs: VecDeque::new(),
            custom_attribute_handlers: HashSet::new(),
            attribute_handlers_enabled: false,
            src_bytes: source.as_bytes(),
            byte_cursor: 0,
            heredoc_recovery_tag: None,
            heredoc_start_time: None,
            errors: Vec::new(),
            operation: ParserOperationContext::new(config, cancellation),
            #[cfg(test)]
            decision_trace: Vec::new(),
            #[cfg(test)]
            bypass_unknown_lowercase_bareword_decision: false,
        }
    }

    /// Create a new parser with a cancellation flag for cooperative cancellation.
    ///
    /// When the flag is set to `true`, the parser will return `Err(ParseError::Cancelled)`
    /// at the next cancellation check point (every 64 statements).
    ///
    /// # Arguments
    ///
    /// * `input` - Perl source code to parse.
    /// * `cancellation_flag` - Shared flag used to request cancellation.
    ///
    /// # Returns
    ///
    /// A parser configured with cooperative cancellation checks.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::Parser;
    /// use std::sync::{
    ///     atomic::AtomicBool,
    ///     Arc,
    /// };
    ///
    /// let cancellation_flag = Arc::new(AtomicBool::new(false));
    /// let mut parser = Parser::new_with_cancellation("my $x = 1;", cancellation_flag);
    /// let _ = parser.parse();
    /// ```
    ///
    /// # Arguments
    ///
    /// `input` and `cancellation_flag` configure source + cancellation.
    ///
    /// # Returns
    ///
    /// A parser configured with cooperative cancellation checks.
    ///
    /// # Examples
    ///
    /// See the cancellation usage example above.
    pub fn new_with_cancellation(input: &'a str, cancellation_flag: Arc<AtomicBool>) -> Self {
        Self::assemble(
            TokenStream::new(input),
            input,
            ParserConfigIdentity::production_default(),
            Some(cancellation_flag),
        )
    }

    /// Create a parser from pre-lexed tokens, skipping the lexer pass.
    ///
    /// This constructor is the integration point for the incremental parsing
    /// pipeline: when cached tokens are available for an unchanged region of
    /// source, they can be fed directly into the parser without re-lexing.
    ///
    /// # Arguments
    ///
    /// * `tokens` — Pre-lexed `Token` values produced by a prior [`TokenStream`]
    ///   pass. Trivia tokens (whitespace, comments) should already be filtered
    ///   out, as [`TokenStream::from_vec`] does not apply trivia skipping.
    ///   An `Eof` token does **not** need to be included; the stream synthesises
    ///   one when the buffer is exhausted.
    /// * `source` — The original Perl source text. This is still required for
    ///   heredoc content collection which operates directly on byte offsets in
    ///   the source rather than on the token stream.
    ///
    /// # Returns
    ///
    /// A configured parser that will consume `tokens` in order without invoking
    /// the lexer. The resulting AST is structurally identical to one produced by
    /// [`Parser::new`] with the same source, provided the token list is complete
    /// and accurate.
    ///
    /// # Context-sensitive token disambiguation
    ///
    /// The standard parser directs contextual token operations (issue #8128) to
    /// re-classify ambiguous tokens (e.g. `/` as division vs. regex) in
    /// context-sensitive positions. A buffered stream cannot re-derive
    /// classifications: each request returns a typed fallback requirement that
    /// this parser records as an [`ParseError::Advisory`] diagnostic while
    /// continuing with the cached classification. In practice this means
    /// `from_tokens` is safe to use when the token stream comes from a previous
    /// successful parse of the same source, where the cached kinds already
    /// reflect every parser-directed correction; advisory diagnostics for
    /// fallback requirements indicate a misaligned or stale token cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use perl_parser_core::{Parser, Token, TokenKind, TokenStream};
    ///
    /// let source = "my $x = 42;";
    ///
    /// // Collect pre-lexed tokens (normally cached from a prior parse)
    /// let mut stream = TokenStream::new(source);
    /// let mut tokens = Vec::new();
    /// loop {
    ///     match stream.next() {
    ///         Ok(t) if t.kind() == TokenKind::Eof => break,
    ///         Ok(t) => tokens.push(t),
    ///         Err(_) => break,
    ///     }
    /// }
    ///
    /// let mut parser = Parser::from_tokens(tokens, source);
    /// let ast = parser.parse()?;
    /// assert!(matches!(ast.kind, perl_parser_core::NodeKind::Program { .. }));
    /// # Ok::<(), perl_parser_core::ParseError>(())
    /// ```
    ///
    /// # Arguments
    ///
    /// * `tokens` - Pre-lexed non-trivia tokens.
    /// * `source` - Original source text used by heredoc processing.
    ///
    /// # Returns
    ///
    /// A parser that consumes the provided token vector.
    ///
    /// # Examples
    ///
    /// See the pre-lexed token example above.
    pub fn from_tokens(tokens: Vec<Token>, source: &'a str) -> Self {
        Self::assemble(
            // Retain the exact source identity so contextual operation
            // fallbacks distinguish missing source from missing checkpoint
            // authority (#8128).
            TokenStream::from_vec_with_source(tokens, source),
            source,
            ParserConfigIdentity::production_default(),
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn decision_trace(&self) -> &[ParserDecisionTrace] {
        &self.decision_trace
    }

    #[cfg(test)]
    pub(crate) fn set_unknown_lowercase_bareword_decision_bypass_for_test(&mut self, bypass: bool) {
        self.bypass_unknown_lowercase_bareword_decision = bypass;
    }

    #[cfg(test)]
    fn unknown_lowercase_bareword_decision_is_bypassed(&self) -> bool {
        self.bypass_unknown_lowercase_bareword_decision
    }

    #[cfg(test)]
    fn record_unknown_lowercase_bareword_call_decision(&mut self, start: usize, end: usize) {
        if self.decision_trace.len() >= MAX_DECISION_TRACE {
            return;
        }
        self.decision_trace.push(ParserDecisionTrace {
            decision: ParserDecision::UnknownLowercaseBarewordCall,
            start,
            end,
        });
    }

    /// Check for cooperative cancellation, amortised over every 64 calls.
    ///
    /// Returns `Err(ParseError::Cancelled)` if the cancellation flag has been set.
    #[inline]
    fn check_cancelled(&mut self) -> ParseResult<()> {
        self.operation.probe_cancellation()
    }

    /// Create a new parser with custom enhanced recovery configuration.
    ///
    /// This constructor exists for API compatibility while enhanced recovery
    /// configuration is being phased in.
    ///
    /// # Arguments
    ///
    /// * `input` - Perl source text to tokenize and parse.
    /// * `_config` - Placeholder recovery configuration parameter.
    ///
    /// # Returns
    ///
    /// A parser instance initialized for the provided source text.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::Parser;
    ///
    /// let parser = Parser::new_with_recovery_config("my $x = 1;", ());
    /// assert_eq!(parser.errors().len(), 0);
    /// ```
    pub fn new_with_recovery_config(input: &'a str, _config: ()) -> Self {
        Self::with_production_config(input, ParserConfigIdentity::production_default())
    }

    /// Parse the source and return the AST for the Parse stage.
    ///
    /// # Returns
    ///
    /// * `Ok(Node)` - Parsed AST with a `Program` root node.
    /// * `Err(ParseError)` - Non-recoverable parsing failure.
    ///
    /// # Errors
    ///
    /// Returns `ParseError` for non-recoverable conditions such as recursion limits.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::Parser;
    ///
    /// let mut parser = Parser::new("my $count = 1;");
    /// let ast = parser.parse()?;
    /// assert!(matches!(ast.kind, perl_parser_core::NodeKind::Program { .. }));
    /// # Ok::<(), perl_parser_core::ParseError>(())
    /// ```
    pub fn parse(&mut self) -> ParseResult<Node> {
        // Fresh operation-local state: no counter, depth, terminal, or
        // cancellation-check state leaks from a previous parse on this instance.
        self.begin_operation();
        // Check cancellation before starting — handles pre-set flags immediately.
        if self.operation.is_pre_cancelled() {
            return Err(ParseError::Cancelled);
        }
        self.parse_program()
    }

    fn begin_operation(&mut self) {
        self.operation.begin();
        self.block_depth = 0;
    }

    /// Get all parse errors collected during parsing
    ///
    /// When error recovery is enabled, the parser continues after syntax errors
    /// and collects them for later retrieval. This is useful for IDE integration
    /// where you want to show all errors at once.
    ///
    /// # Returns
    ///
    /// A slice of all `ParseError`s encountered during parsing
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::Parser;
    ///
    /// let mut parser = Parser::new("my $x = ; sub foo {");
    /// let _ast = parser.parse(); // Parse with recovery
    /// let errors = parser.errors();
    /// // errors will contain details about syntax errors
    /// ```
    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    /// Observe a parser-directed contextual token operation (issue #8128).
    ///
    /// Applied, replayed, and not-required outcomes continue silently: the
    /// requested classification is in force. A buffered stream that cannot
    /// honor the request records an [`ParseError::Advisory`] so the
    /// conservative continuation with cached classification is observable and
    /// never reported as an application, and returns `Ok(())` — callers keep
    /// parsing with the tokens the stream still holds.
    fn observe_contextual_operation(
        &mut self,
        operation: ContextualTokenOp,
        location: usize,
    ) -> ParseResult<()> {
        let label = operation.label();
        match self.tokens.apply_contextual(operation) {
            ContextualOpResult::AppliedLive
            | ContextualOpResult::AppliedReplay
            | ContextualOpResult::NotRequired => Ok(()),
            ContextualOpResult::FallbackRequired { reason } => {
                self.errors.push(ParseError::Advisory {
                    message: format!(
                        "{label} requires a rebuild through a live lexer ({reason:?}); \
                         continuing with cached classification"
                    ),
                    location,
                });
                Ok(())
            }
            ContextualOpResult::Unsupported => {
                self.errors.push(ParseError::Advisory {
                    message: format!(
                        "{label} is not supported for this stream state; \
                         continuing with cached classification"
                    ),
                    location,
                });
                Ok(())
            }
        }
    }

    /// Reclassify the head lookahead token as a term-context token (issue
    /// #8128). Used where the parser knows a `/` classified as division must
    /// become a regex delimiter; on a buffered stream that refuses the
    /// operation an advisory records the conservative continuation.
    fn reclassify_head_as_term(&mut self) -> ParseResult<()> {
        let location = self.tokens.peek()?.start();
        self.observe_contextual_operation(
            ContextualTokenOp::ReclassifyFromBoundary { expected_context: LexerMode::ExpectTerm },
            location,
        )
    }

    /// Parse with error recovery and return comprehensive output.
    ///
    /// This method is preferred for LSP Analyze workflows and always returns
    /// a `ParseOutput` containing the AST and any collected diagnostics.
    /// `budget_usage` is the live operation tracker, not a post-hoc
    /// reconstruction from diagnostics.
    ///
    /// # Returns
    ///
    /// `ParseOutput` with the AST, diagnostics, and the live budget tracker.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use perl_parser_core::Parser;
    ///
    /// let mut parser = Parser::new("my $x = ;");
    /// let output = parser.parse_with_recovery();
    /// assert!(!output.diagnostics.is_empty() || matches!(output.ast.kind, perl_parser_core::NodeKind::Program { .. }));
    /// ```
    pub fn parse_with_recovery(&mut self) -> ParseOutput {
        let (ast, stop_cause) = match self.parse() {
            // An Ok result is a completed parse unless a parser branch
            // recorded a terminal stop while returning Ok (the lexer-budget
            // `UnknownRest` stop leaves a partial AST): consume that recorded
            // cause here so truncation is never reported as clean completion.
            Ok(node) => (node, self.operation.take_terminal()),
            Err(e) => {
                // If parse() returned Err, it was a non-recoverable error (e.g. cancellation,
                // recursion limit, or nesting limit). Record the typed stop cause at this branch —
                // the cause is not reconstructed from diagnostics later. Any operation-scoped
                // cause stored before the terminal error is superseded by it and dropped, so
                // nothing leaks into a later operation on this same parser.
                let cause = ParseStopCause::from_parse_error(&e);
                let _ = self.operation.take_terminal();

                // Ensure the terminal error is recorded in the diagnostic vector, but only
                // once — `Cancelled` in particular can already be present from prior work.
                if !self.errors.contains(&e) {
                    self.errors.push(e);
                }

                // Return a partial Program node so consumers always receive a usable AST.
                (
                    Node::new(
                        NodeKind::Program { statements: vec![] },
                        SourceLocation { start: 0, end: 0 },
                    ),
                    Some(cause),
                )
            }
        };

        // Return the live tracker that governed this operation. Uncharged
        // dimensions (tokens/nodes/diagnostics) stay at zero until B02.
        ParseOutput::finish(ast, self.errors.clone(), self.operation.take_tracker(), stop_cause)
    }
}

include!("helpers.rs");
include!("heredoc.rs");
include!("statements.rs");
include!("variables.rs");
include!("control_flow.rs");
include!("declarations.rs");
include!("expressions/mod.rs");
include!("expressions/precedence.rs");
include!("expressions/unary.rs");
include!("expressions/postfix.rs");
include!("expressions/primary.rs");
include!("expressions/calls.rs");
include!("expressions/hashes.rs");
include!("expressions/quotes.rs");

#[cfg(test)]
mod builtin_block_list_tests;
#[cfg(test)]
mod builtin_expansion_tests;
#[cfg(test)]
mod chained_deref_method_tests;
#[cfg(test)]
mod coderef_invocation_tests;
#[cfg(test)]
mod complex_args_tests;
#[cfg(test)]
mod control_flow_expr_tests;
#[cfg(test)]
mod declaration_in_args_tests;
#[cfg(test)]
mod error_recovery_tests;
#[cfg(test)]
mod eval_goto_tests;
#[cfg(test)]
mod for_builtin_block_tests;
#[cfg(test)]
mod format_comprehensive_tests;
#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod forward_declaration_tests;
#[cfg(test)]
mod from_tokens_tests;
#[cfg(test)]
mod glob_assignment_tests;
#[cfg(test)]
mod glob_tests;
#[cfg(test)]
mod hash_vs_block_tests;
#[cfg(test)]
mod heredoc_security_tests;
#[cfg(test)]
mod indirect_call_tests;
#[cfg(test)]
mod indirect_object_tests;
#[cfg(test)]
mod loop_control_tests;
#[cfg(test)]
mod qualified_variable_subscript_tests;
#[cfg(test)]
mod regex_delimiter_tests;
#[cfg(test)]
mod slash_ambiguity_tests;
#[cfg(test)]
mod statement_modifier_tests;
#[cfg(test)]
mod statement_terminator_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tie_tests;
#[cfg(test)]
mod typed_variable_declaration_tests;
#[cfg(test)]
mod unclosed_block_recovery_tests;
#[cfg(test)]
mod use_overload_tests;
#[cfg(test)]
mod x_repetition_tests;

#[cfg(test)]
mod strip_qw_comments_unit_tests {
    use super::strip_qw_comments;

    #[test]
    fn test_strip_basic() {
        let result = strip_qw_comments("foo # comment\n bar");
        assert_eq!(result.split_whitespace().collect::<Vec<_>>(), vec!["foo", "bar"]);
    }
}

#[cfg(test)]
mod operation_context_unit_tests {
    use super::*;

    #[test]
    fn with_depth_unwinds_on_injected_early_return() {
        let mut parser = Parser::new("1");
        let result: ParseResult<()> = parser.with_depth(|p| {
            assert_eq!(p.operation.tracker().current_depth, 1);
            Err(ParseError::Cancelled)
        });
        assert!(result.is_err());
        assert_eq!(parser.operation.tracker().current_depth, 0);
        assert_eq!(parser.operation.tracker().max_depth_reached, 1);
    }

    #[test]
    fn with_depth_unwinds_on_success() {
        let mut parser = Parser::new("1");
        let value = parser
            .with_depth(|p| {
                assert_eq!(p.operation.tracker().current_depth, 1);
                Ok(7_u8)
            })
            .expect("success path");
        assert_eq!(value, 7);
        assert_eq!(parser.operation.tracker().current_depth, 0);
        assert_eq!(parser.operation.tracker().max_depth_reached, 1);
    }

    #[test]
    fn nested_with_depth_retains_max() {
        let mut parser = Parser::new("1");
        parser
            .with_depth(|p| {
                p.with_depth(|inner| {
                    assert_eq!(inner.operation.tracker().current_depth, 2);
                    Ok(())
                })
            })
            .expect("nested depth");
        assert_eq!(parser.operation.tracker().current_depth, 0);
        assert_eq!(parser.operation.tracker().max_depth_reached, 2);
    }
}

/// Drift-guard tests for reserved `Missing*` NodeKind variants.
///
/// These tests document the **current parser emission contract**:
///
/// - `MissingExpression` IS emitted (by `recover_missing_infix_rhs`).
/// - `MissingStatement`, `MissingIdentifier`, `MissingBlock` are RESERVED —
///   the parser never emits them today.
///
/// If a future recovery change starts emitting any of the reserved variants,
/// the guard test will fail, signalling that real parser fixture tests must
/// be added before the change ships.
#[cfg(test)]
mod recovery_node_drift_guard {
    use super::Parser;
    use crate::ast::NodeKind;

    /// Walk the AST tree and collect all s-expression tokens by flattening
    /// the sexp string — cheap and sufficient for kind-name presence checks.
    fn sexp(source: &str) -> String {
        let mut parser = Parser::new(source);
        let output = parser.parse_with_recovery();
        output.ast.to_sexp()
    }

    // -----------------------------------------------------------------------
    // Assertion helpers
    // -----------------------------------------------------------------------

    /// Assert that the given s-expression does NOT contain a reserved variant.
    fn assert_no_reserved_missing(label: &str, src: &str) {
        let s = sexp(src);
        assert!(
            !s.contains("(missing_statement)"),
            "{label}: `MissingStatement` must not appear in parse output for {:?}, sexp={s}",
            src
        );
        assert!(
            !s.contains("(missing_identifier)"),
            "{label}: `MissingIdentifier` must not appear in parse output for {:?}, sexp={s}",
            src
        );
        assert!(
            !s.contains("(missing_block)"),
            "{label}: `MissingBlock` must not appear in parse output for {:?}, sexp={s}",
            src
        );
    }

    // -----------------------------------------------------------------------
    // Guard: reserved variants are NOT emitted by the current parser
    // -----------------------------------------------------------------------

    #[test]
    fn test_reserved_missing_variants_not_emitted_on_truncated_input() {
        // Inputs that are aggressively malformed or truncated — high-value
        // probes for recovery paths.
        let probes = [
            // Truncated after keyword
            "if",
            "while",
            "for",
            "sub",
            "my",
            "our",
            "local",
            // Missing block bodies
            "sub foo",
            "if (1)",
            "while (1)",
            "for (my $i = 0; $i < 10; $i++)",
            // Missing identifiers in common positions
            "sub { }",
            "package",
            "use",
            // Incomplete expressions
            "$x->{",
            "@arr[",
            "%hash{",
            // Operator with no operands
            "++",
            "--",
            // Realistic partial-file snippets
            "my $x = { key =>",
            "print STDERR",
            "die \"message\" if",
        ];
        for src in &probes {
            assert_no_reserved_missing(src, src);
        }
    }

    // -----------------------------------------------------------------------
    // Positive: MissingExpression IS emitted for trailing infix with no RHS
    // -----------------------------------------------------------------------

    #[test]
    fn test_missing_expression_is_still_emitted_for_trailing_infix() {
        // A trailing binary operator with no RHS triggers `recover_missing_infix_rhs`,
        // which is the one code path that emits `MissingExpression`.
        let s = sexp("1 +");
        assert!(
            s.contains("(missing_expression)"),
            "Expected `MissingExpression` in sexp for `1 +`, got: {s}"
        );
        // Confirm the reserved three are still absent even in this case.
        assert!(
            !s.contains("(missing_statement)"),
            "MissingStatement must not appear alongside MissingExpression"
        );
        assert!(
            !s.contains("(missing_identifier)"),
            "MissingIdentifier must not appear alongside MissingExpression"
        );
        assert!(
            !s.contains("(missing_block)"),
            "MissingBlock must not appear alongside MissingExpression"
        );
    }

    #[test]
    fn test_missing_expression_emitted_for_assignment_no_rhs() {
        // `my $x = ;` — another known trigger for MissingExpression via infix recovery.
        let s = sexp("my $x = ;");
        assert!(
            s.contains("(missing_expression)"),
            "Expected `MissingExpression` for `my $x = ;`, got: {s}"
        );
    }

    // -----------------------------------------------------------------------
    // Sanity: the NodeKind enum variants exist (compile-time check)
    // -----------------------------------------------------------------------

    #[test]
    fn test_reserved_variant_names_compile() {
        // If anyone removes or renames the reserved variants, this will fail to compile.
        let _variants: &[NodeKind] =
            &[NodeKind::MissingStatement, NodeKind::MissingIdentifier, NodeKind::MissingBlock];
    }
}
