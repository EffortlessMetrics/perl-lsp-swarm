use crate::tree::tree_from_parts;
use crate::{InputEdit, ParseDiagnostic, Tree};
pub use perl_parser_core::incremental::IncrementalMetrics;
pub(crate) use perl_parser_core::incremental::IncrementalState;
use perl_parser_core::{
    ParseOutput, ParseStopCause, Parser as CoreParser,
    incremental::{FallbackReason as CoreFallbackReason, IncrementalEdit},
};

/// A Perl parser with tree-sitter-style ergonomics.
///
/// Wraps the v3 recursive-descent Perl parser. Create one parser instance and call
/// [`parse`][Parser::parse] for each source file you need to process.
///
/// # Example
///
/// ```rust
/// use tree_sitter_perl_rs::Parser;
///
/// let mut parser = Parser::new();
/// let tree = parser.parse("sub greet { print \"hello\"; }");
/// assert!(tree.is_some());
/// ```
pub struct Parser {
    // Stateless currently; the v3 CoreParser takes source at construction time.
    // Stored as a unit struct for forward compatibility (e.g. future options).
    _priv: (),
}

impl Parser {
    /// Create a new parser instance.
    pub fn new() -> Self {
        Parser { _priv: () }
    }

    /// Parse a Perl source string and return a [`Tree`], or `None` on complete failure.
    ///
    /// The v3 parser is highly error-tolerant — even malformed input usually produces a
    /// partial tree. `None` is reserved for extreme edge cases where no AST can be built
    /// at all.
    ///
    /// # Example
    ///
    /// ```rust
    /// use tree_sitter_perl_rs::Parser;
    ///
    /// let mut parser = Parser::new();
    /// let tree = parser.parse("my $x = 42;");
    /// assert!(tree.is_some());
    /// ```
    pub fn parse(&mut self, source: &str) -> Option<Tree> {
        let mut core = CoreParser::new(source);
        match core.parse() {
            Ok(root) => Some(tree_from_parts(root, source, core.errors().to_vec())),
            Err(_) => None,
        }
    }

    /// Parse `source` and preserve recovery diagnostics and catastrophic failures.
    ///
    /// A recovered parse returns `tree: Some(_)` with one or more diagnostics. A
    /// terminal failure returns `tree: None`. `failure` contains a typed
    /// [`ParseFailure`] when the facade can represent the terminal cause. Existing
    /// callers that only need the compatibility `Option` API can continue using
    /// [`parse`][Parser::parse].
    pub fn parse_detailed(&mut self, source: &str) -> ParseOutcome {
        let mut core = CoreParser::new(source);
        let output = core.parse_with_recovery();
        // `stop_cause` is the authority: `perl-parser-core` sets it at the exact
        // branch that terminates the parse and documents that "the diagnostic
        // population never determines the stop cause". Scanning `diagnostics`
        // instead mis-reports a recovered-then-terminated parse as whatever was
        // recovered first.
        let stop_cause = output.stop_cause();
        let ParseOutput { ast, diagnostics, .. } = output;
        let failure =
            stop_cause.and_then(|cause| ParseFailure::from_stop_cause(cause, &diagnostics));
        // Withhold the tree on the invariant (`stop_cause.is_some() ==
        // terminated_early()`), not on whether the cause could be classified, so
        // an unclassifiable terminal cause can never publish a partial tree.
        let tree = stop_cause.is_none().then(|| tree_from_parts(ast, source, diagnostics.clone()));

        ParseOutcome { tree, diagnostics, failure }
    }

    /// Parse `source` using `old_tree` as a hint for incremental re-parsing.
    ///
    /// A single validated edit uses the lower-tier checkpoint-bounded token replay
    /// kernel. The AST is rebuilt from the resulting token stream; this facade does
    /// not claim AST subtree reuse. Multiple, invalid, or missing edits use a safe
    /// full-parse fallback and record the reason on the returned tree.
    ///
    /// Returns `None` on complete parse failure (same semantics as `parse`).
    pub fn parse_with_old_tree(&mut self, source: &str, old_tree: &Tree) -> Option<Tree> {
        // Fast path: if source is unchanged and no edits were recorded, reuse the old tree
        // instead of re-parsing. This mirrors tree-sitter's incremental no-op behavior.
        if source == old_tree.source() && old_tree.pending_edits.is_empty() {
            let mut unchanged = old_tree.clone();
            unchanged.reparse_mode = Some(ReparseMode::Unchanged);
            return Some(unchanged);
        }

        let fallback_reason = match old_tree.pending_edits.as_slice() {
            [edit] => {
                let Some(incremental_edit) =
                    validated_incremental_edit(old_tree.source(), source, edit)
                else {
                    return self.parse_with_fallback(source, FallbackReason::InvalidEdit);
                };

                let mut state = old_tree.incremental_state.as_ref().cloned().unwrap_or_else(|| {
                    IncrementalState::with_diagnostics(old_tree.source(), old_tree.diagnostics())
                });
                match state.reparse(source, &incremental_edit) {
                    Ok(root) => {
                        let mode =
                            state.metrics().fallback.map_or(ReparseMode::TokenReplay, |reason| {
                                ReparseMode::FullParseFallback(FallbackReason::TokenReplay(reason))
                            });
                        return Some(Tree {
                            root,
                            source: source.to_string(),
                            pending_edits: Vec::new(),
                            diagnostics: state.diagnostics().to_vec(),
                            incremental_state: Some(state),
                            reparse_mode: Some(mode),
                        });
                    }
                    Err(_) => FallbackReason::TokenReplay(CoreFallbackReason::TokenReplayFailed),
                }
            }
            [] => FallbackReason::NoPendingEdit,
            _ => FallbackReason::MultipleEdits,
        };

        self.parse_with_fallback(source, fallback_reason)
    }

    fn parse_with_fallback(&mut self, source: &str, reason: FallbackReason) -> Option<Tree> {
        let mut tree = self.parse(source)?;
        tree.reparse_mode = Some(ReparseMode::FullParseFallback(reason));
        tree.incremental_state =
            Some(IncrementalState::with_diagnostics(source, tree.diagnostics()));
        Some(tree)
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

fn validated_incremental_edit(
    old_source: &str,
    new_source: &str,
    edit: &InputEdit,
) -> Option<IncrementalEdit> {
    if edit.start_byte > edit.old_end_byte
        || edit.old_end_byte > old_source.len()
        || edit.new_end_byte < edit.start_byte
        || edit.new_end_byte > new_source.len()
        || !old_source.is_char_boundary(edit.start_byte)
        || !old_source.is_char_boundary(edit.old_end_byte)
        || !new_source.is_char_boundary(edit.start_byte)
        || !new_source.is_char_boundary(edit.new_end_byte)
    {
        return None;
    }

    let removed = edit.old_end_byte.checked_sub(edit.start_byte)?;
    let inserted = edit.new_end_byte.checked_sub(edit.start_byte)?;
    let expected_len = old_source.len().checked_sub(removed)?.checked_add(inserted)?;
    if expected_len != new_source.len() {
        return None;
    }

    let old_prefix = old_source.get(..edit.start_byte)?;
    let new_prefix = new_source.get(..edit.start_byte)?;
    let old_suffix = old_source.get(edit.old_end_byte..)?;
    let new_suffix = new_source.get(edit.new_end_byte..)?;
    if old_prefix != new_prefix || old_suffix != new_suffix {
        return None;
    }

    let new_text = new_source.get(edit.start_byte..edit.new_end_byte)?;
    Some(IncrementalEdit::new(edit.start_byte, edit.old_end_byte, new_text))
}

/// The operation used to produce a tree from an old tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReparseMode {
    /// The source was byte-identical and the old tree was reused.
    Unchanged,
    /// One validated edit used checkpoint-bounded token replay.
    TokenReplay,
    /// The source was parsed from scratch after replay was not safe or usable.
    FullParseFallback(FallbackReason),
}

/// Why the facade used a complete parse instead of token replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackReason {
    /// The pending [`InputEdit`] did not describe `source`.
    InvalidEdit,
    /// More than one pending edit was recorded on the old tree.
    MultipleEdits,
    /// The source changed but no pending edit was recorded on the old tree.
    NoPendingEdit,
    /// The lower-tier token replay kernel rejected the incremental operation.
    TokenReplay(CoreFallbackReason),
}

/// The result of [`Parser::parse_detailed`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ParseOutcome {
    /// The recovered syntax tree, when parsing did not fail catastrophically.
    pub tree: Option<Tree>,
    /// Diagnostics collected during parsing, including recoverable errors.
    pub diagnostics: Vec<ParseDiagnostic>,
    /// The typed reason parsing could not produce a usable tree, if any.
    pub failure: Option<ParseFailure>,
}

impl ParseOutcome {
    /// Returns `true` when diagnostics or an explicit error node were observed.
    pub fn has_error(&self) -> bool {
        self.diagnostics.iter().any(ParseDiagnostic::blocks_clean_parse)
            || self.tree.as_ref().is_some_and(Tree::has_error)
    }

    /// Returns `true` when a tree was produced with recovery diagnostics.
    pub fn is_recovered(&self) -> bool {
        self.tree.is_some() && self.has_error()
    }
}

/// Typed catastrophic parse failures surfaced by [`Parser::parse_detailed`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ParseFailure {
    /// The parser recursion budget was exceeded.
    RecursionLimit,
    /// The parser's structural nesting budget was exceeded.
    NestingTooDeep {
        /// Observed nesting depth.
        depth: usize,
        /// Configured maximum nesting depth.
        max_depth: usize,
    },
    /// Parsing was cancelled by the caller.
    Cancelled,
    /// A future or currently unclassified catastrophic failure.
    Other {
        /// The original parser diagnostic.
        diagnostic: ParseDiagnostic,
    },
}

impl ParseFailure {
    /// Classify the parser's terminal stop cause.
    ///
    /// Returns `None` when the terminal cause explicitly forbids facade
    /// classification, or when an uncategorized cause has no diagnostic to
    /// attach. Callers must still treat the parse as terminated; therefore
    /// [`Parser::parse_detailed`] can return `tree: None, failure: None` while
    /// preserving diagnostics.
    fn from_stop_cause(cause: ParseStopCause, diagnostics: &[ParseDiagnostic]) -> Option<Self> {
        match cause {
            ParseStopCause::Cancelled => Some(Self::Cancelled),
            // Both the unit `RecursionLimit` and the fielded
            // `RecursionDepthExhausted` budget paths arrive here. This is
            // deliberately NOT `NestingTooDeep`: that belongs to the structural
            // guards, and `ParseError::RecursionDepthExhausted` forbids
            // "relabeling expression-recursion exhaustion as structural
            // nesting" (#12952; taxonomy settled by #14342).
            ParseStopCause::RecursionBudgetExhausted { .. } => Some(Self::RecursionLimit),
            ParseStopCause::NestingOrDepthBudgetExhausted { limit, usage } => {
                Some(Self::NestingTooDeep { depth: usage, max_depth: limit })
            }
            // This sentinel deliberately carries no facade classification and
            // forbids callers from inferring one from preserved diagnostics.
            ParseStopCause::FutureTypedTerminal => None,
            // Other uncategorized terminal causes report the terminal diagnostic,
            // which the parser appends last, rather than the first recovered one.
            _ => {
                diagnostics.last().map(|diagnostic| Self::Other { diagnostic: diagnostic.clone() })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Focused discriminators for `ParseFailure::from_stop_cause` (the #12952
    // RIPR seams). `tests/parse_failure_taxonomy.rs` activates only the
    // recursion and nesting arms through `parse_detailed`; the cancellation
    // and catch-all arms have no public-API activation because the facade
    // exposes no cancellation token, so they are pinned here directly.
    // Failures propagate rather than panic, per the repository lint policy.
    type StopCauseResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn cancellation_classifies_as_cancelled() -> StopCauseResult {
        let failure = ParseFailure::from_stop_cause(ParseStopCause::Cancelled, &[]);

        match failure {
            Some(ParseFailure::Cancelled) => Ok(()),
            other => Err(format!("cancellation must classify as Cancelled, got {other:?}").into()),
        }
    }

    #[test]
    fn recursion_budget_exhaustion_classifies_as_recursion_limit() -> StopCauseResult {
        let cause = ParseStopCause::RecursionBudgetExhausted { limit: Some(128), usage: Some(129) };
        let failure = ParseFailure::from_stop_cause(cause, &[]);

        match failure {
            Some(ParseFailure::RecursionLimit) => Ok(()),
            other => Err(format!(
                "recursion budget exhaustion must classify as RecursionLimit, not {other:?}"
            )
            .into()),
        }
    }

    #[test]
    fn nesting_budget_exhaustion_classifies_as_nesting_too_deep() -> StopCauseResult {
        let cause = ParseStopCause::NestingOrDepthBudgetExhausted { limit: 64, usage: 65 };
        let failure = ParseFailure::from_stop_cause(cause, &[]);

        match failure {
            Some(ParseFailure::NestingTooDeep { depth: 65, max_depth: 64 }) => Ok(()),
            other => Err(format!(
                "nesting budget exhaustion must classify as NestingTooDeep, got {other:?}"
            )
            .into()),
        }
    }

    #[test]
    fn uncategorized_cause_reports_the_terminal_diagnostic() -> StopCauseResult {
        let diagnostic =
            ParseDiagnostic::SyntaxError { message: "terminal".to_string(), location: 7 };
        let failure = ParseFailure::from_stop_cause(
            ParseStopCause::HeredocBudgetExhausted { limit: 512, usage: 600 },
            std::slice::from_ref(&diagnostic),
        );

        match failure {
            Some(ParseFailure::Other { diagnostic: reported }) => {
                assert_eq!(reported.to_string(), diagnostic.to_string());
                Ok(())
            }
            other => Err(format!(
                "an uncategorized cause must report the terminal diagnostic, got {other:?}"
            )
            .into()),
        }
    }

    #[test]
    fn future_typed_terminal_does_not_infer_failure_from_diagnostics() -> StopCauseResult {
        let diagnostic =
            ParseDiagnostic::SyntaxError { message: "recovered".to_string(), location: 7 };
        let failure = ParseFailure::from_stop_cause(
            ParseStopCause::FutureTypedTerminal,
            std::slice::from_ref(&diagnostic),
        );

        match failure {
            None => Ok(()),
            other => Err(format!(
                "FutureTypedTerminal must not infer a failure from diagnostics; \
                 parse_detailed withholds the tree independently, got {other:?}"
            )
            .into()),
        }
    }

    #[test]
    fn uncategorized_cause_without_diagnostics_withholds_the_failure() -> StopCauseResult {
        let failure = ParseFailure::from_stop_cause(ParseStopCause::LexerBudgetExhausted, &[]);

        match failure {
            None => Ok(()),
            other => Err(format!(
                "an uncategorized cause with no recorded diagnostic must yield no failure; the \
                 tree is withheld on the stop cause instead, got {other:?}"
            )
            .into()),
        }
    }
}
