use crate::incremental::checkpoint::{LexCheckpoint, ParseCheckpoint, ScopeSnapshot};
use crate::incremental::lex::create_lex_checkpoints;
use perl_lexer::{PerlLexer, Token, TokenType};
use perl_line_index::LineIndex;
use perl_parser_core::ast::{Node, NodeKind};
use perl_parser_core::error::ParseOutput;
use perl_parser_core::parser::Parser;
use ropey::Rope;
use std::ops::Deref;

/// Read-only compatibility view for legacy field-style source access.
///
/// `IncrementalState` intentionally implements `Deref` but not `DerefMut` for
/// this view. Existing consumers may continue to read `state.source`, while
/// source replacement remains private to the committed-generation machinery.
#[doc(hidden)]
#[derive(Clone)]
pub struct IncrementalStateReadView {
    pub source: String,
}

/// One internally consistent incremental parser generation.
///
/// Generation-bearing fields are crate-private so external callers cannot
/// mutate source, tokens, checkpoints, AST, or parser output independently.
/// Use [`IncrementalState::new`], read-only accessors, and [`super::apply_edits`]
/// to move between committed generations.
///
/// Legacy field-style source reads remain available through an immutable
/// compatibility view:
///
/// ```
/// use perl_parser::incremental::IncrementalState;
///
/// let state = IncrementalState::new("my $x = 1;".to_string());
/// assert_eq!(state.source.len(), state.source().len());
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
#[derive(Clone)]
#[non_exhaustive]
pub struct IncrementalState {
    pub(super) rope: Rope,
    pub(super) line_index: LineIndex,
    pub(super) lex_checkpoints: Vec<LexCheckpoint>,
    pub(super) parse_checkpoints: Vec<ParseCheckpoint>,
    /// Authoritative native parser output for the current source.
    pub(super) parse_output: ParseOutput,
    /// Parsed AST compatibility mirror.
    #[deprecated(note = "Use parse_output(); this compatibility mirror will be removed.")]
    pub(super) ast: Node,
    pub(super) tokens: Vec<Token>,
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
    #[expect(deprecated, reason = "the compatibility AST field mirrors the native parse output")]
    #[must_use]
    pub fn new(source: String) -> Self {
        let rope = Rope::from_str(&source);
        let line_index = LineIndex::new(&source);
        let mut parser = Parser::new(&source);
        let parse_output = parser.parse_with_recovery();
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
            rope,
            line_index,
            lex_checkpoints,
            parse_checkpoints,
            parse_output,
            ast,
            tokens,
            read_view: IncrementalStateReadView { source },
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
        &self.rope
    }

    /// Line index for the current committed source.
    #[must_use]
    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    /// Lexer restart summaries for the current committed token stream.
    #[must_use]
    pub fn lex_checkpoints(&self) -> &[LexCheckpoint] {
        &self.lex_checkpoints
    }

    /// Parser restart summaries for the current committed parse output.
    #[must_use]
    pub fn parse_checkpoints(&self) -> &[ParseCheckpoint] {
        &self.parse_checkpoints
    }

    /// Authoritative recovery-aware parser output for this generation.
    #[must_use]
    pub fn parse_output(&self) -> &ParseOutput {
        &self.parse_output
    }

    /// Compatibility AST view for the current generation.
    #[deprecated(note = "Use parse_output().ast; this compatibility view will be removed.")]
    #[must_use]
    pub fn ast(&self) -> &Node {
        &self.ast
    }

    /// Current committed lexer token stream.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Find the nearest lexer checkpoint at or before `byte`.
    #[must_use]
    pub fn find_lex_checkpoint(&self, byte: usize) -> Option<&LexCheckpoint> {
        self.lex_checkpoints.iter().rev().find(|cp| cp.byte <= byte)
    }

    /// Find the nearest parser checkpoint at or before `byte`.
    #[must_use]
    pub fn find_parse_checkpoint(&self, byte: usize) -> Option<&ParseCheckpoint> {
        self.parse_checkpoints.iter().rev().find(|cp| cp.byte <= byte)
    }

    /// Replace the text-bearing portion of a staged generation.
    ///
    /// This remains crate-private so source, rope, and line-index identity cannot
    /// be changed independently by consumers. Callers finish rebuilding tokens,
    /// checkpoints, and parser output before publishing the staged state.
    pub(super) fn replace_source_text(&mut self, source: String) {
        self.rope = Rope::from_str(&source);
        self.line_index = LineIndex::new(&source);
        self.read_view.source = source;
    }

    /// Refresh the authoritative parser output from the current source.
    ///
    /// The compatibility AST and parse checkpoints are updated from the same
    /// recovered parse so the state cannot expose mixed parse generations.
    #[expect(deprecated, reason = "the compatibility AST field mirrors the native parse output")]
    pub(crate) fn refresh_parse_output(&mut self) {
        let mut parser = Parser::new(self.source());
        let parse_output = parser.parse_with_recovery();
        self.parse_checkpoints = Self::create_parse_checkpoints(&parse_output.ast);
        self.ast = parse_output.ast.clone();
        self.parse_output = parse_output;
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
