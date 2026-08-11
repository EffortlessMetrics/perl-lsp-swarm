use crate::incremental::checkpoint::{LexCheckpoint, ParseCheckpoint, ScopeSnapshot};
use crate::incremental::lex::lex_source_with_checkpoints;
use perl_lexer::Token;
use perl_line_index::LineIndex;
use perl_parser_core::ast::{Node, NodeKind};
use perl_parser_core::error::ParseOutput;
use perl_parser_core::parser::Parser;
use ropey::Rope;

#[derive(Clone)]
pub struct IncrementalState {
    pub rope: Rope,
    pub line_index: LineIndex,
    /// Compact compatibility summaries of live lexer restart boundaries.
    ///
    /// These summaries are captured from `PerlLexer::checkpoint()` while
    /// lexing. The full live state is replayed and validated before any restart;
    /// this summary alone never authorizes restoration.
    pub lex_checkpoints: Vec<LexCheckpoint>,
    pub parse_checkpoints: Vec<ParseCheckpoint>,
    /// Authoritative native parser output for the current source.
    ///
    /// This is produced by `Parser::parse_with_recovery` and carries the AST,
    /// ordered parser diagnostics, recovery count, budget usage, and early
    /// termination state. Incremental consumers should use this field rather
    /// than reconstructing parser state from an AST alone.
    pub parse_output: ParseOutput,
    /// Parsed AST compatibility field.
    ///
    /// This field mirrors [`Self::parse_output`]'s AST after every supported
    /// state transition. It remains temporarily for compatibility with existing
    /// callers and parse-checkpoint code.
    #[deprecated(note = "Use parse_output.ast; this compatibility mirror will be removed.")]
    pub ast: Node,
    pub tokens: Vec<Token>,
    pub source: String,
}

impl IncrementalState {
    #[expect(deprecated, reason = "the compatibility AST field mirrors the native parse output")]
    pub fn new(source: String) -> Self {
        let rope = Rope::from_str(&source);
        let line_index = LineIndex::new(&source);
        let mut parser = Parser::new(&source);
        let parse_output = parser.parse_with_recovery();
        let ast = parse_output.ast.clone();
        let lexed = lex_source_with_checkpoints(&source, &line_index);
        let parse_checkpoints = Self::create_parse_checkpoints(&parse_output.ast);
        Self {
            rope,
            line_index,
            lex_checkpoints: lexed.checkpoints,
            parse_checkpoints,
            parse_output,
            ast,
            tokens: lexed.tokens,
            source,
        }
    }

    pub fn find_lex_checkpoint(&self, byte: usize) -> Option<&LexCheckpoint> {
        self.lex_checkpoints.iter().rev().find(|cp| cp.byte <= byte)
    }

    pub fn find_parse_checkpoint(&self, byte: usize) -> Option<&ParseCheckpoint> {
        self.parse_checkpoints.iter().rev().find(|cp| cp.byte <= byte)
    }

    /// Refresh the authoritative parser output from the current source.
    ///
    /// The compatibility AST and parse checkpoints are updated from the same
    /// recovered parse so the state cannot expose mixed parse generations.
    #[expect(deprecated, reason = "the compatibility AST field mirrors the native parse output")]
    pub(crate) fn refresh_parse_output(&mut self) {
        let mut parser = Parser::new(&self.source);
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
