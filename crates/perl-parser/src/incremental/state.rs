use crate::incremental::checkpoint::{LexCheckpoint, ParseCheckpoint, ScopeSnapshot};
use crate::incremental::lex::create_lex_checkpoints;
use crate::incremental::snapshot::{ParseGeneration, ParseSnapshot, ParseSnapshotStrategy};
use perl_lexer::{PerlLexer, Token, TokenType};
use perl_line_index::LineIndex;
use perl_parser_core::ast::{Node, NodeKind};
use perl_parser_core::error::ParseOutput;
use perl_parser_core::parser::Parser;
use ropey::Rope;
use std::ops::Deref;

/// Read-only compatibility view for legacy field-style access.
///
/// `IncrementalState` intentionally implements `Deref` but not `DerefMut` for
/// this view. Existing consumers may continue to read the committed generation
/// through legacy fields, while every state transition remains private to the
/// incremental parser.
#[doc(hidden)]
#[derive(Clone)]
pub struct IncrementalStateReadView {
    /// Current committed source text.
    pub source: String,
    /// Rope view for the current committed source.
    pub rope: Rope,
    /// Line index for the current committed source.
    pub line_index: LineIndex,
    /// Monotonic identity for the committed generation.
    pub generation: ParseGeneration,
    /// Lexer restart summaries for the current committed token stream.
    pub lex_checkpoints: Vec<LexCheckpoint>,
    /// Parser restart summaries for the current committed parse output.
    pub parse_checkpoints: Vec<ParseCheckpoint>,
    /// Generation-bound authoritative parser snapshot.
    pub snapshot: ParseSnapshot,
    /// Authoritative recovery-aware parser output compatibility mirror.
    pub parse_output: ParseOutput,
    /// Parsed AST compatibility mirror.
    pub ast: Node,
    /// Current committed lexer token stream.
    pub tokens: Vec<Token>,
}

/// One internally consistent incremental parser generation.
///
/// Generation-bearing fields are held by an immutable compatibility view so
/// external callers can read the legacy fields but cannot mutate source,
/// tokens, checkpoints, AST, or parser output independently. Use
/// [`IncrementalState::new`], read-only accessors, and [`super::apply_edits`] to
/// move between committed generations.
///
/// Legacy field-style reads remain available:
///
/// ```
/// use perl_parser::incremental::IncrementalState;
///
/// let state = IncrementalState::new("my $x = 1;".to_string());
/// assert_eq!(state.source.len(), state.source().len());
/// assert_eq!(state.tokens.len(), state.tokens().len());
/// assert_eq!(state.lex_checkpoints.len(), state.lex_checkpoints().len());
/// assert_eq!(state.generation, state.generation());
/// ```
///
/// The view does not grant mutation authority:
///
/// ```compile_fail
/// use perl_parser::incremental::IncrementalState;
///
/// let mut state = IncrementalState::new("my $x = 1;".to_string());
/// state.source.push_str("\n");
/// ```
///
/// ```compile_fail
/// use perl_parser::incremental::IncrementalState;
///
/// let mut state = IncrementalState::new("my $x = 1;".to_string());
/// state.tokens.clear();
/// ```
#[derive(Clone)]
#[non_exhaustive]
pub struct IncrementalState {
    pub(super) read_view: IncrementalStateReadView,
}

impl Deref for IncrementalState {
    type Target = IncrementalStateReadView;

    fn deref(&self) -> &Self::Target {
        &self.read_view
    }
}

impl IncrementalState {
    /// Build the initial committed generation from source text.
    #[must_use]
    pub fn new(source: String) -> Self {
        let rope = Rope::from_str(&source);
        let line_index = LineIndex::new(&source);
        let mut parser = Parser::new(&source);
        let parse_output = parser.parse_with_recovery();
        let generation = ParseGeneration::INITIAL;
        let snapshot = ParseSnapshot::from_output(
            &source,
            generation,
            ParseSnapshotStrategy::Fresh,
            parse_output.clone(),
        );
        let ast = parse_output.ast.clone();
        let mut lexer = PerlLexer::new(&source);
        let mut tokens = Vec::new();
        while let Some(token) = lexer.next_token() {
            if token.token_type == TokenType::EOF {
                break;
            }
            tokens.push(token);
        }
        let lex_checkpoints = create_lex_checkpoints(&tokens, &line_index);
        let parse_checkpoints = Self::create_parse_checkpoints(&parse_output.ast);

        Self {
            read_view: IncrementalStateReadView {
                source,
                rope,
                line_index,
                generation,
                lex_checkpoints,
                parse_checkpoints,
                snapshot,
                parse_output,
                ast,
                tokens,
            },
        }
    }

    /// Current committed source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.read_view.source
    }

    /// Rope view for the current committed source.
    #[must_use]
    pub fn rope(&self) -> &Rope {
        &self.read_view.rope
    }

    /// Line index for the current committed source.
    #[must_use]
    pub fn line_index(&self) -> &LineIndex {
        &self.read_view.line_index
    }

    /// Monotonic identity for the committed source generation.
    #[must_use]
    pub const fn generation(&self) -> ParseGeneration {
        self.read_view.generation
    }

    /// Lexer restart summaries for the current committed token stream.
    #[must_use]
    pub fn lex_checkpoints(&self) -> &[LexCheckpoint] {
        &self.read_view.lex_checkpoints
    }

    /// Parser restart summaries for the current committed parse output.
    #[must_use]
    pub fn parse_checkpoints(&self) -> &[ParseCheckpoint] {
        &self.read_view.parse_checkpoints
    }

    /// Generation-bound authoritative parser snapshot.
    #[must_use]
    pub fn snapshot(&self) -> &ParseSnapshot {
        &self.read_view.snapshot
    }

    /// Authoritative recovery-aware parser output for this generation.
    #[must_use]
    pub fn parse_output(&self) -> &ParseOutput {
        &self.read_view.snapshot.parse_output
    }

    /// Compatibility AST view for the current generation.
    #[deprecated(note = "Use snapshot().parse_output.ast; this compatibility view will be removed.")]
    #[must_use]
    pub fn ast(&self) -> &Node {
        &self.read_view.snapshot.parse_output.ast
    }

    /// Current committed lexer token stream.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.read_view.tokens
    }

    /// Find the nearest lexer checkpoint at or before `byte`.
    #[must_use]
    pub fn find_lex_checkpoint(&self, byte: usize) -> Option<&LexCheckpoint> {
        self.read_view.lex_checkpoints.iter().rev().find(|cp| cp.byte <= byte)
    }

    /// Find the nearest parser checkpoint at or before `byte`.
    #[must_use]
    pub fn find_parse_checkpoint(&self, byte: usize) -> Option<&ParseCheckpoint> {
        self.read_view.parse_checkpoints.iter().rev().find(|cp| cp.byte <= byte)
    }

    /// Replace the text-bearing portion of a staged generation.
    ///
    /// This remains crate-private so source, rope, and line-index identity cannot
    /// be changed independently by consumers. Callers finish rebuilding tokens,
    /// checkpoints, and parser output before publishing the staged state.
    pub(super) fn replace_source_text(&mut self, source: String) {
        self.read_view.rope = Rope::from_str(&source);
        self.read_view.line_index = LineIndex::new(&source);
        self.read_view.source = source;
    }

    /// Replace the staged token stream and rebuild its lexer checkpoints.
    pub(super) fn replace_tokens(&mut self, tokens: Vec<Token>) {
        self.read_view.tokens = tokens;
        self.refresh_lex_checkpoints();
    }

    /// Replace the suffix of the staged token stream.
    pub(super) fn splice_tokens(&mut self, start: usize, tokens: Vec<Token>) {
        self.read_view.tokens.splice(start.., tokens);
        self.refresh_lex_checkpoints();
    }

    /// Rebuild lexer checkpoints from the staged token stream and line index.
    fn refresh_lex_checkpoints(&mut self) {
        self.read_view.lex_checkpoints =
            create_lex_checkpoints(&self.read_view.tokens, &self.read_view.line_index);
    }

    /// Refresh the authoritative parser output from the current source.
    ///
    /// The snapshot, compatibility AST, and parse checkpoints are updated from
    /// the same recovered parse so the state cannot expose mixed generations.
    pub(crate) fn refresh_parse_output(&mut self, strategy: ParseSnapshotStrategy) {
        let mut parser = Parser::new(self.source());
        let parse_output = parser.parse_with_recovery();
        let generation = self.generation().next();
        let snapshot =
            ParseSnapshot::from_output(self.source(), generation, strategy, parse_output.clone());
        self.read_view.parse_checkpoints = Self::create_parse_checkpoints(&parse_output.ast);
        self.read_view.ast = parse_output.ast.clone();
        self.read_view.parse_output = parse_output;
        self.read_view.snapshot = snapshot;
        self.read_view.generation = generation;
    }

    pub(crate) fn create_parse_checkpoints(ast: &Node) -> Vec<ParseCheckpoint> {
        let mut checkpoints = vec![];
        let mut scope = ScopeSnapshot::default();
        walk_ast_for_checkpoints(ast, &mut checkpoints, &mut scope, 0);
        checkpoints
    }
}

fn walk_ast_for_checkpoints(
    node: &Node,
    checkpoints: &mut Vec<ParseCheckpoint>,
    scope: &mut ScopeSnapshot,
    node_id: usize,
) {
    match &node.kind {
        NodeKind::Package { name, .. } => {
            scope.package_name = name.clone();
            checkpoints.push(ParseCheckpoint {
                byte: node.location.start,
                scope_snapshot: scope.clone(),
                node_id,
            });
        }
        NodeKind::Subroutine { .. } | NodeKind::Block { .. } => checkpoints.push(ParseCheckpoint {
            byte: node.location.start,
            scope_snapshot: scope.clone(),
            node_id,
        }),
        NodeKind::VariableDeclaration { variable, .. } => {
            if let NodeKind::Variable { name, sigil, .. } = &variable.kind {
                scope.locals.push(format!("{}{}", sigil, name));
            }
        }
        NodeKind::VariableListDeclaration { variables, .. } => {
            for var in variables {
                if let NodeKind::Variable { name, sigil, .. } = &var.kind {
                    scope.locals.push(format!("{}{}", sigil, name));
                }
            }
        }
        _ => {}
    }
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for (i, stmt) in statements.iter().enumerate() {
                walk_ast_for_checkpoints(
                    stmt,
                    checkpoints,
                    scope,
                    node_id.wrapping_mul(101).wrapping_add(i),
                );
            }
        }
        _ => {}
    }
}