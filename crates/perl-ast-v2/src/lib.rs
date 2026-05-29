//! Enhanced AST with full position tracking for incremental parsing
//!
//! This module provides an updated AST that uses Range instead of SourceLocation
//! to support incremental parsing and better error reporting.

use perl_position_tracking::Range;

/// A unique identifier for AST nodes to support incremental parsing.
pub type NodeId = usize;

/// Index into the diagnostics array in `ParseOutput`.
///
/// This type enables lightweight error nodes that reference diagnostics
/// stored separately from the AST, reducing memory overhead and decoupling
/// tree structure from human-readable messages.
pub type DiagnosticId = u32;

/// Kinds of missing syntax elements for error recovery.
///
/// This enum provides specific information about what was expected
/// but not found, enabling better IDE diagnostics and recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingKind {
    /// Missing expression (e.g., after `=` in assignment)
    Expression,
    /// Missing statement
    Statement,
    /// Missing identifier/name
    Identifier,
    /// Missing block `{ ... }`
    Block,
    /// Missing closing delimiter
    ClosingDelimiter(char),
    /// Missing semicolon
    Semicolon,
    /// Missing condition (e.g., in `if`)
    Condition,
    /// Missing argument
    Argument,
    /// Missing operator
    Operator,
}

/// Enhanced AST node with full position tracking
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique identifier for this node
    pub id: NodeId,
    /// The kind of syntax node
    pub kind: NodeKind,
    /// Source range with line/column information
    pub range: Range,
}

impl Node {
    /// Create a new AST node
    pub fn new(id: NodeId, kind: NodeKind, range: Range) -> Self {
        Node { id, kind, range }
    }

    /// Convert to tree-sitter compatible S-expression
    pub fn to_sexp(&self) -> String {
        // Delegate to existing implementation
        self.kind.to_sexp()
    }
}

/// The kinds of AST nodes used by the parser.
///
/// Each variant represents a specific syntactic construct in the Perl source
/// and carries the child nodes or data needed to represent that construct.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    // Program structure
    /// Top-level program containing a list of statements.
    Program {
        /// Statements contained by the program/root node.
        statements: Vec<Node>,
    },
    /// Block node containing a list of statements.
    Block {
        /// Statements inside a block.
        statements: Vec<Node>,
    },

    // Declarations
    /// A single variable declaration (`my`, `our`, `local`, `state`, ...).
    VariableDeclaration {
        /// The declarator keyword (e.g. `my`, `our`).
        declarator: String, // my, our, local, state
        /// The variable node being declared.
        variable: Box<Node>,
        /// Any attributes attached to the declaration.
        attributes: Vec<String>,
        /// Optional initializer expression.
        initializer: Option<Box<Node>>,
    },

    /// A list-style variable declaration (e.g. `my ($a, $b) = ...`).
    VariableListDeclaration {
        /// The declarator keyword.
        declarator: String,
        /// Variables declared in the list.
        variables: Vec<Node>,
        /// Any attributes attached to the declaration.
        attributes: Vec<String>,
        /// Optional initializer for the list.
        initializer: Option<Box<Node>>,
    },

    // Variables
    /// A variable usage with sigil and name (e.g. `$foo`, `@arr`).
    Variable {
        /// The sigil character (e.g. `$`, `@`, `%`).
        sigil: String, // $, @, %, *
        /// The identifier/name of the variable.
        name: String,
    },

    // Error recovery nodes
    /// An error/recovery node produced during parsing (legacy, rich payload).
    ///
    /// This variant embeds the error information directly in the AST node.
    /// For new code, prefer `ErrorRef` which stores only a diagnostic index.
    Error {
        /// Human readable error message.
        message: String,
        /// Tokens or node kinds that were expected at this location.
        expected: Vec<String>,
        /// Optional partially parsed node for recovery contexts.
        partial: Option<Box<Node>>,
    },

    /// Lightweight error node referencing a diagnostic by index.
    ///
    /// This is the preferred error representation for memory efficiency.
    /// The actual diagnostic information is stored in `ParseOutput.diagnostics`
    /// and can be looked up by the `diag_id`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let output = parser.parse_with_recovery();
    /// for node in output.ast.walk() {
    ///     if let NodeKind::ErrorRef { diag_id } = &node.kind {
    ///         let diagnostic = &output.diagnostics[*diag_id as usize];
    ///         println!("Error at {:?}: {}", node.range, diagnostic);
    ///     }
    /// }
    /// ```
    ErrorRef {
        /// Index into the diagnostics array in `ParseOutput`.
        diag_id: DiagnosticId,
    },

    /// Placeholder for a missing expression during error recovery.
    MissingExpression,
    /// Placeholder for a missing statement during error recovery.
    MissingStatement,
    /// Placeholder for a missing identifier during error recovery.
    MissingIdentifier,
    /// Placeholder for a missing block during error recovery.
    MissingBlock,

    /// Specific kind of missing syntax element.
    ///
    /// This provides more granular information about what's missing
    /// without embedding full diagnostic details in the AST.
    Missing(MissingKind),

    // Include all other variants from original AST...
    // (Abbreviated for example - would include all original variants)

    // Expressions
    /// A binary expression (e.g. `a + b`).
    Binary {
        /// The operator token as text.
        op: String,
        /// Left-hand side expression.
        left: Box<Node>,
        /// Right-hand side expression.
        right: Box<Node>,
    },

    /// A unary expression (e.g. `-x`, `!flag`).
    Unary {
        /// The operator token.
        op: String,
        /// The operand expression.
        operand: Box<Node>,
    },

    // Control flow
    /// An `if` control-flow construct, including `elsif` and `else` branches.
    If {
        /// The conditional expression.
        condition: Box<Node>,
        /// The then-branch block node.
        then_branch: Box<Node>,
        /// Zero or more `elsif` branches represented as (condition, block).
        elsif_branches: Vec<(Node, Node)>,
        /// Optional else branch.
        else_branch: Option<Box<Node>>,
    },

    // Literals
    /// Numeric literal node.
    Number {
        /// The literal text of the number.
        value: String,
    },
    /// String literal node; may be interpolated.
    String {
        /// The string contents.
        value: String,
        /// Whether the string contains interpolation.
        interpolated: bool,
    },
    /// An identifier token.
    Identifier {
        /// The identifier text.
        name: String,
    },
    // Other essential variants...
}

impl NodeKind {
    /// Convert to S-expression format
    pub fn to_sexp(&self) -> String {
        use NodeKind::*;

        match self {
            Program { statements } => {
                let stmts = statements.iter().map(|s| s.to_sexp()).collect::<Vec<_>>().join(" ");
                format!("(source_file {})", stmts)
            }

            Block { statements } => {
                let stmts = statements.iter().map(|s| s.to_sexp()).collect::<Vec<_>>().join(" ");
                format!("(block {})", stmts)
            }

            Variable { sigil, name } => {
                format!("(variable {} {})", sigil, name)
            }

            Number { value } => format!("(number {})", value),

            String { value, interpolated } => {
                if *interpolated {
                    format!("(string_interpolated {:?})", value)
                } else {
                    format!("(string {:?})", value)
                }
            }

            Binary { op, left, right } => {
                format!("(binary_{} {} {})", op, left.to_sexp(), right.to_sexp())
            }

            Error { message, .. } => format!("(ERROR {})", message),
            ErrorRef { diag_id } => format!("(ERROR_REF #{})", diag_id),

            MissingExpression => "(MISSING_EXPRESSION)".to_string(),
            MissingStatement => "(MISSING_STATEMENT)".to_string(),
            MissingIdentifier => "(MISSING_IDENTIFIER)".to_string(),
            MissingBlock => "(MISSING_BLOCK)".to_string(),
            Missing(kind) => format!("(MISSING {:?})", kind),

            // Add other variants...
            _ => format!("({:?})", self),
        }
    }
}

/// Generator for producing unique `NodeId` values used across the AST.
///
/// This utility ensures each constructed `Node` receives a distinct identifier
/// which is useful for incremental parsing, diffing and node references.
pub struct NodeIdGenerator {
    /// The next identifier to hand out.
    next_id: NodeId,
}

impl NodeIdGenerator {
    /// Create a new `NodeIdGenerator` starting at zero.
    pub fn new() -> Self {
        NodeIdGenerator { next_id: 0 }
    }

    /// Return the next unique `NodeId` and advance the generator.
    pub fn next_id(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

impl Default for NodeIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_position_tracking::{Position, Range};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn test_range() -> Range {
        Range::new(Position::new(0, 1, 1), Position::new(5, 1, 6))
    }

    fn node(id: NodeId, kind: NodeKind) -> Node {
        Node::new(id, kind, test_range())
    }

    #[test]
    fn test_node_creation() -> TestResult {
        let mut id_gen = NodeIdGenerator::new();

        let node = node(id_gen.next_id(), NodeKind::Number { value: "42".to_string() });

        assert_eq!(node.id, 0);
        assert_eq!(node.to_sexp(), "(number 42)");
        Ok(())
    }

    #[test]
    fn test_error_nodes() -> TestResult {
        let mut id_gen = NodeIdGenerator::new();

        let error = node(
            id_gen.next_id(),
            NodeKind::Error {
                message: "Unexpected token".to_string(),
                expected: vec!["identifier".to_string()],
                partial: None,
            },
        );

        assert_eq!(error.to_sexp(), "(ERROR Unexpected token)");
        Ok(())
    }

    #[test]
    fn node_id_generator_default_continues_from_zero() -> TestResult {
        let mut id_gen = NodeIdGenerator::default();

        assert_eq!(id_gen.next_id(), 0);
        assert_eq!(id_gen.next_id(), 1);
        assert_eq!(id_gen.next_id(), 2);
        Ok(())
    }

    #[test]
    fn program_and_block_sexps_include_child_nodes_in_order() -> TestResult {
        let first = node(1, NodeKind::Number { value: "42".to_string() });
        let second =
            node(2, NodeKind::Variable { sigil: "$".to_string(), name: "answer".to_string() });

        let program = NodeKind::Program { statements: vec![first.clone(), second.clone()] };
        let block = NodeKind::Block { statements: vec![first, second] };

        assert_eq!(program.to_sexp(), "(source_file (number 42) (variable $ answer))");
        assert_eq!(block.to_sexp(), "(block (number 42) (variable $ answer))");
        Ok(())
    }

    #[test]
    fn string_sexp_distinguishes_plain_and_interpolated_values() -> TestResult {
        let plain = NodeKind::String { value: "literal $name".to_string(), interpolated: false };
        let interpolated =
            NodeKind::String { value: "hello $name".to_string(), interpolated: true };

        assert_eq!(plain.to_sexp(), "(string \"literal $name\")");
        assert_eq!(interpolated.to_sexp(), "(string_interpolated \"hello $name\")");
        Ok(())
    }

    #[test]
    fn binary_sexp_recurses_into_operands() -> TestResult {
        let left = node(1, NodeKind::Variable { sigil: "$".to_string(), name: "lhs".to_string() });
        let right = node(2, NodeKind::Number { value: "1".to_string() });

        let binary =
            NodeKind::Binary { op: "+".to_string(), left: Box::new(left), right: Box::new(right) };

        assert_eq!(binary.to_sexp(), "(binary_+ (variable $ lhs) (number 1))");
        Ok(())
    }

    #[test]
    fn recovery_nodes_render_stable_sexps() -> TestResult {
        let cases = [
            (NodeKind::ErrorRef { diag_id: 7 }, "(ERROR_REF #7)"),
            (NodeKind::MissingExpression, "(MISSING_EXPRESSION)"),
            (NodeKind::MissingStatement, "(MISSING_STATEMENT)"),
            (NodeKind::MissingIdentifier, "(MISSING_IDENTIFIER)"),
            (NodeKind::MissingBlock, "(MISSING_BLOCK)"),
            (NodeKind::Missing(MissingKind::Semicolon), "(MISSING Semicolon)"),
            (
                NodeKind::Missing(MissingKind::ClosingDelimiter('}')),
                "(MISSING ClosingDelimiter('}'))",
            ),
        ];

        for (kind, expected) in cases {
            assert_eq!(kind.to_sexp(), expected);
        }
        Ok(())
    }

    #[test]
    fn fallback_debug_sexp_covers_unhandled_variants() -> TestResult {
        let operand = node(1, NodeKind::Identifier { name: "flag".to_string() });
        let unary = NodeKind::Unary { op: "!".to_string(), operand: Box::new(operand) };

        let sexp = unary.to_sexp();

        assert!(sexp.starts_with("(Unary {"));
        assert!(sexp.contains("op: \"!\""));
        Ok(())
    }
}
