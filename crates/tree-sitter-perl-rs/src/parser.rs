use crate::tree::tree_from_parts;
use crate::{InputEdit, ParseDiagnostic, Tree};
use perl_parser_core::{
    ParseOutput, Parser as CoreParser,
    incremental::{FallbackReason as CoreFallbackReason, IncrementalEdit, IncrementalState},
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
    /// catastrophic failure returns `tree: None` and a typed [`ParseFailure`]. Existing
    /// callers that only need the compatibility `Option` API can continue using
    /// [`parse`][Parser::parse].
    pub fn parse_detailed(&mut self, source: &str) -> ParseOutcome {
        let mut core = CoreParser::new(source);
        let output = core.parse_with_recovery();
        let terminated_early = output.terminated_early();
        let ParseOutput { ast, diagnostics, .. } = output;
        let failure = terminated_early
            .then(|| diagnostics.iter().find_map(ParseFailure::from_diagnostic))
            .flatten();
        let tree = failure.is_none().then(|| tree_from_parts(ast, source, diagnostics.clone()));

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
    fn from_diagnostic(diagnostic: &ParseDiagnostic) -> Option<Self> {
        match diagnostic {
            ParseDiagnostic::RecursionLimit => Some(Self::RecursionLimit),
            ParseDiagnostic::NestingTooDeep { depth, max_depth } => {
                Some(Self::NestingTooDeep { depth: *depth, max_depth: *max_depth })
            }
            ParseDiagnostic::Cancelled => Some(Self::Cancelled),
            _ => Some(Self::Other { diagnostic: diagnostic.clone() }),
        }
    }
}
