//! Abstract Syntax Tree definitions for Perl within the parsing and LSP workflow.
//!
//! This module defines the comprehensive AST node types that represent parsed Perl code
//! during the Parse → Index → Navigate → Complete → Analyze stages. The design is optimized
//! for both direct use in Rust analysis and for a native debug S-expression projection
//! (`Node::to_sexp`) during diagnostics. That projection is not Tree-sitter compatibility.
//!
//! # LSP Workflow Integration
//!
//! The AST structures support Perl tooling workflows by:
//! - **Parse**: Produced by the parser as the canonical syntax tree
//! - **Index**: Traversed to build symbol and reference tables
//! - **Navigate**: Provides locations for definition and reference lookups
//! - **Complete**: Supplies context for completion, hover, and signature help
//! - **Analyze**: Feeds semantic analysis, diagnostics, and refactoring
//!
//! # Performance and ownership
//!
//! AST structures are optimized for large codebases with:
//! - Memory-efficient node representation using `Box<Node>` for recursive structures
//! - Fast pattern matching via enum variants for common Perl constructs
//! - Location tracking for precise error reporting in large files
//!
//! Ownership stays recursively owned (`Box`, `Vec`, optional children). [`Node`]
//! destruction is iterative and depth-independent. [`Clone`], [`PartialEq`],
//! and [`Debug`] are iterative; overflow is proven on a 50,000-node chain with
//! a 256 KiB worker. [`Debug`] is a bounded human projection, not machine
//! identity. See [`Node`] for the depth-safety contract.
//!
//! # Usage Examples
//!
//! ## Basic AST Construction
//!
//! ```rust
//! use perl_ast::{Node, NodeKind, SourceLocation};
//!
//! // Create a simple variable declaration node
//! let location = SourceLocation::new(0, 10);
//! let node = Node::new(
//!     NodeKind::VariableDeclaration {
//!         declarator: "my".to_string(),
//!         variable: Box::new(Node::new(
//!             NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
//!             location,
//!         )),
//!         attributes: vec![],
//!         initializer: None,
//!     },
//!     location,
//! );
//! assert_eq!(node.kind.kind_name(), "VariableDeclaration");
//! ```
//!
//! ## Native debug S-expression
//!
//! ```rust
//! use perl_ast::{Node, NodeKind, SourceLocation};
//!
//! let loc = SourceLocation::new(0, 2);
//! let num = Node::new(NodeKind::Number { value: "42".to_string() }, loc);
//! let program = Node::new(NodeKind::Program { statements: vec![num] }, loc);
//!
//! let sexp = program.to_sexp();
//! assert!(sexp.starts_with("(source_file"));
//! ```
//!
//! ## AST Traversal and Analysis
//!
//! ```rust
//! use perl_ast::{Node, NodeKind, SourceLocation};
//!
//! fn count_variables(node: &Node) -> usize {
//!     let mut count = 0;
//!     match &node.kind {
//!         NodeKind::Variable { .. } => count += 1,
//!         NodeKind::Program { statements } => {
//!             for stmt in statements {
//!                 count += count_variables(stmt);
//!             }
//!         }
//!         _ => {} // Handle other node types as needed
//!     }
//!     count
//! }
//!
//! let loc = SourceLocation::new(0, 5);
//! let var = Node::new(
//!     NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
//!     loc,
//! );
//! let program = Node::new(NodeKind::Program { statements: vec![var] }, loc);
//! assert_eq!(count_variables(&program), 1);
//! ```
//!
//! ## Parsing Integration
//!
//! In practice the AST is produced by the parser rather than built by hand
//! (requires `perl-parser-core`):
//!
//! ```rust,ignore
//! use perl_parser_core::Parser;
//! use perl_ast::NodeKind;
//!
//! let mut parser = Parser::new("my $x = 42;");
//! let ast = parser.parse().expect("should parse");
//! assert!(matches!(ast.kind, NodeKind::Program { .. }));
//! ```

// Re-export SourceLocation from perl-position-tracking for unified span handling
pub use perl_position_tracking::SourceLocation;
// Re-export Token and TokenKind from perl-token for AST error nodes
pub use perl_token::{Token, TokenKind};
use std::fmt;
#[cfg(test)]
use std::ops::ControlFlow;
use strum::VariantNames as _;

/// Maximum AST traversal depth for recursive *read* operations that still
/// use a call-stack guard.
///
/// Historical recursive-render ceiling. [`Node::to_sexp`] and
/// [`Node::render_debug_sexp`] no longer consult it; those walks are iterative
/// and use caller-selected [`NativeDebugSexpLimits`]. Exact whole-tree reads
/// also ignore this constant.
///
/// Chosen at 512: typical Perl code nests fewer than 100 levels deep;
/// 512 provides a comfortable safety margin while staying well within
/// Rust's default 8 MB stack.
///
/// This constant does **not** bound destruction, clone, equality, debug
/// formatting, [`Node::count_nodes`], or
/// [`Node::find_deepest_containing_offset`]. Those exact whole-tree reads are
/// iterative. Bounded variants expose [`AstReadResult`] instead of consulting
/// this ceiling. [`Debug`] uses its own conservative budgets
/// (`NODE_DEBUG_MAX_*`). See [`Node`] for the full depth-safety disposition.
pub const MAX_AST_DEPTH: usize = 512;

/// Discriminant for the three semantically distinct forms of Perl's `goto` statement.
///
/// Perl's `goto` is overloaded across three fundamentally different operations:
///
/// | Form | Example | Semantics |
/// |------|---------|-----------|
/// | `Label` | `goto LABEL` | Jump to a named label in the current program |
/// | `Sub` | `goto &sub` | **Frame replacement** — tail-call with same `@_`; even `caller()` cannot distinguish |
/// | `Expr` | `goto $expr` | Dynamic target — computed at run time |
///
/// The `Sub` form (`goto &NAME`) is semantically different from a normal call: it replaces
/// the current stack frame with the called subroutine, so the called sub sees the same `@_`
/// and `caller` context. Semantic analysis and DAP must treat it as a tail-call, not a jump.
///
/// This enum is always populated at parse time (never `None`); the parser detects the form
/// by examining the first token of the target expression before consuming the full target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GotoTargetForm {
    /// `goto LABEL` — transfer control to a named label (plain identifier).
    Label,
    /// `goto &sub`, `goto &Pkg::sub`, `goto &$coderef` — frame replacement (tail call).
    ///
    /// The `&` sigil is the distinguishing marker. The target may be:
    /// - A bare name: `goto &helper`
    /// - A package-qualified name: `goto &Pkg::helper`
    /// - A variable coderef: `goto &$dispatch_table{$key}`
    Sub,
    /// `goto $expr` or `goto EXPR` where the target is a computed scalar expression.
    ///
    /// This includes `goto $label_var` (dynamic label) and other computed forms
    /// that are not a plain identifier (Label) or an ampersand-prefixed coderef (Sub).
    Expr,
}

/// Stable identifier for a named child relationship in the syntax tree.
///
/// Field identifiers are represented by canonical static names so the AST,
/// facade, and future query engine share one vocabulary without allocating at
/// traversal time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FieldId(&'static str);

macro_rules! define_field_ids {
    ($(($constant:ident, $name:literal)),+ $(,)?) => {
        impl FieldId {
            $(pub const $constant: Self = Self($name);)+

/// All field identifiers named by the structural registry.
            ///
            /// Order is the public compatibility inventory. Set membership is
            /// the unique [`crate::kind_schema::NODE_KIND_STRUCTURAL_REGISTRY`]
            /// fields; unused or missing names fail the parity checker.
            pub const ALL: &'static [Self] = &[$(Self::$constant),+];

            /// Return the canonical external name for this field.
            pub const fn name(self) -> &'static str {
                self.0
            }

            /// Resolve a canonical field name without allocating.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$constant),)+
                    _ => None,
                }
            }
        }
    };
}

define_field_ids! {
    (STATEMENTS, "statements"),
    (EXPRESSION, "expression"),
    (VARIABLE, "variable"),
    (PACKAGE, "package"),
    (STATEMENT, "statement"),
    (INITIALIZER, "initializer"),
    (ITEMS, "items"),
    (LHS, "lhs"),
    (RHS, "rhs"),
    (LEFT, "left"),
    (RIGHT, "right"),
    (CONDITION, "condition"),
    (THEN_BRANCH, "then_branch"),
    (THEN_EXPR, "then_expr"),
    (ELSE_EXPR, "else_expr"),
    (ELSE_BRANCH, "else_branch"),
    (OPERAND, "operand"),
    (ELEMENTS, "elements"),
    (KEY, "key"),
    (VALUE, "value"),
    (BLOCK, "block"),
    (BODY, "body"),
    (CATCH, "catch"),
    (FINALLY, "finally"),
    (CONTINUE_BLOCK, "continue_block"),
    (INIT, "init"),
    (UPDATE, "update"),
    (LIST, "list"),
    (EXPR, "expr"),
    (PROTOTYPE, "prototype"),
    (SIGNATURE, "signature"),
    (PARAMETERS, "parameters"),
    (DEFAULT_VALUE, "default_value"),
    (TARGET, "target"),
    (OBJECT, "object"),
    (ARGS, "args"),
    (PARTIAL, "partial"),
}

/// Core AST node representing any Perl language construct within parsing workflows.
///
/// This is the fundamental building block for representing parsed Perl code. Each node
/// contains both the semantic information (kind) and positional information (location)
/// necessary for comprehensive script analysis.
///
/// # LSP Workflow Role
///
/// Nodes flow through tooling stages:
/// - **Parse**: Created by the parser as it builds the syntax tree
/// - **Index**: Visited to build symbol and reference tables
/// - **Navigate**: Used to resolve definitions, references, and call hierarchy
/// - **Complete**: Provides contextual information for completion and hover
/// - **Analyze**: Drives semantic analysis and diagnostics
///
/// # Memory and ownership
///
/// The structure is designed for efficient memory usage during large-scale parsing:
/// - `SourceLocation` uses compact position encoding for large files
/// - `NodeKind` enum variants minimize memory overhead for common constructs
/// - Child relationships stay recursively owned (`Box<Node>`, `Vec<Node>`,
///   optional children, pair/clause records). Public node geometry is unchanged
///   from that model; destruction, clone, equality, and debug formatting, not
///   representation, are iterative.
///
/// # Depth safety
///
/// - **[`Drop`]**: iterative. Children are detached through
///   [`Node::for_each_child_mut`] into a heap work stack before each node's
///   remaining fields are released. A 50,000-node chain on a 256 KiB worker
///   completes without overflowing the thread stack. Construct/destroy
///   equality is proven at 10,000-node cycle depth, not on the overflow
///   fixture.
/// - **[`Clone`]**: iterative. Canonical child fields are cloned on an
///   explicit heap stack and each parent is rebuilt only after its children
///   exist. A 50,000-node chain on a 256 KiB worker completes without
///   overflowing the thread stack. This is a full owned duplication, not a
///   cheap share.
/// - **[`PartialEq`]**: iterative exact structural equality. Canonical child
///   fields are compared on an explicit heap stack. A 50,000-node chain on a
///   256 KiB worker completes without overflowing the thread stack. This is
///   the current `==` proposition (location, variant, every non-child
///   payload, optional/repeated cardinality, child order). It is not
///   S-expression, fingerprint, or source-text equality.
/// - **[`Debug`]**: iterative bounded human projection. Kind, range, a
///   selected payload summary, and a bounded child projection are rendered
///   on an explicit heap stack. Depth, width, node, payload, and byte
///   budgets are fixed conservative internals
///   ([`NODE_DEBUG_MAX_DEPTH`], [`NODE_DEBUG_MAX_CHILDREN`],
///   [`NODE_DEBUG_MAX_NODES`], [`NODE_DEBUG_MAX_PAYLOAD_CHARS`],
///   [`NODE_DEBUG_MAX_BYTES`]). Truncation is marked with
///   [`NODE_DEBUG_TRUNCATION_MARKER`]. A 50,000-node chain on a 256 KiB
///   worker completes without overflowing the thread stack, and the
///   rendering stays at or under the byte bound. This is not machine
///   identity, equality, serialization, or a durable metric oracle.
///   Configured complete/truncated native debug rendering is
///   [`Node::render_debug_sexp`].
///
/// Exact whole-tree reads such as [`Node::count_nodes`] and
/// [`Node::find_deepest_containing_offset`] are iterative over the canonical
/// child visit table and do not silently truncate. Bounded variants return
/// [`AstReadResult`]. [`Node::render_debug_sexp`] is iterative and returns
/// [`NativeDebugSexpResult`]. [`Node::to_sexp`] is a `String` wrapper over that
/// engine and cannot prove completeness.
///
/// # Examples
///
/// Construct a variable declaration node manually:
///
/// ```
/// use perl_ast::{Node, NodeKind, SourceLocation};
///
/// let loc = SourceLocation::new(0, 11);
/// let var = Node::new(
///     NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() },
///     loc,
/// );
/// let decl = Node::new(
///     NodeKind::VariableDeclaration {
///         declarator: "my".to_string(),
///         variable: Box::new(var),
///         attributes: vec![],
///         initializer: None,
///     },
///     loc,
/// );
/// assert_eq!(decl.kind.kind_name(), "VariableDeclaration");
/// ```
///
/// Typically you obtain nodes from the parser rather than constructing them by hand:
///
/// ```ignore
/// use perl_parser::Parser;
///
/// let mut parser = Parser::new("my $x = 42;");
/// let ast = parser.parse()?;
/// println!("AST: {}", ast.to_sexp());
/// ```
///
/// [`Clone`], [`PartialEq`], and [`Debug`] are iterative. See the
/// depth-safety table above.
#[non_exhaustive]
pub struct Node {
    /// The specific type and semantic content of this AST node
    pub kind: NodeKind,
    /// Source position information for error reporting and code navigation
    pub location: SourceLocation,
}

impl Node {
    /// Create a new AST node with the given kind and source location.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let node = Node::new(
    ///     NodeKind::Number { value: "42".to_string() },
    ///     SourceLocation::new(0, 2),
    /// );
    /// assert_eq!(node.kind.kind_name(), "Number");
    /// assert_eq!(node.location.start, 0);
    /// ```
    pub fn new(kind: NodeKind, location: SourceLocation) -> Self {
        #[cfg(test)]
        drop_audit::record_construct();
        Node { kind, location }
    }

    /// Decompose this node into its kind and source location.
    ///
    /// Pattern matching cannot move fields out of a type implementing
    /// [`Drop`], so consumers that previously destructured an owned `Node`
    /// call this instead. The returned kind is the original payload; the
    /// dropped shell retains only a childless placeholder.
    ///
    /// The returned [`NodeKind`] still owns every former child. Dropping that
    /// payload remains stack-safe: each owned [`Node`] runs the same iterative
    /// [`Drop`] implementation.
    pub fn into_parts(mut self) -> (NodeKind, SourceLocation) {
        let kind = std::mem::replace(&mut self.kind, NodeKind::MissingExpression);
        (kind, self.location)
    }

    // Native debug S-expression projection (`to_sexp`, `render_debug_sexp`,
    // `Display`) lives in `node_sexp`. Typed terminality is
    // `NativeDebugSexpResult`; `to_sexp` cannot prove completeness.

    /// Collect direct child nodes into a vector for convenience APIs.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let loc = SourceLocation::new(0, 1);
    /// let stmt = Node::new(NodeKind::Number { value: "1".to_string() }, loc);
    /// let program = Node::new(
    ///     NodeKind::Program { statements: vec![stmt] },
    ///     loc,
    /// );
    /// assert_eq!(program.children().len(), 1);
    /// ```
    #[inline]
    pub fn children(&self) -> Vec<&Node> {
        let mut children = Vec::new();
        self.for_each_child(|child| children.push(child));
        children
    }

    /// Count direct child nodes without allocating an intermediate vector.
    ///
    /// This is more efficient than `children().len()` when callers only need
    /// cardinality.
    #[inline]
    pub fn child_count(&self) -> usize {
        let mut count = 0;
        self.for_each_child(|_| count += 1);
        count
    }

    /// Get the first direct child node, if any.
    ///
    /// Optimized to avoid allocating the children vector.
    #[inline]
    pub fn first_child(&self) -> Option<&Node> {
        let mut result = None;
        self.for_each_child(|child| {
            if result.is_none() {
                result = Some(child);
            }
        });
        result
    }

    /// Returns `true` when this node's source span contains `offset`.
    ///
    /// The start position is inclusive and the end position is exclusive.
    #[inline]
    pub fn contains_offset(&self, offset: usize) -> bool {
        self.location.start() <= offset && offset < self.location.end()
    }

    /// Returns the byte length of this node's source span.
    ///
    /// Uses saturating subtraction so malformed spans never underflow.
    #[inline]
    pub fn span_len(&self) -> usize {
        self.location.end().saturating_sub(self.location.start())
    }

    /// Get the last direct child node, if any.
    ///
    /// Optimized to avoid allocating the children vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::{Node, NodeKind, SourceLocation};
    ///
    /// let loc = SourceLocation::new(0, 1);
    /// let first = Node::new(NodeKind::Number { value: "1".to_string() }, loc);
    /// let second = Node::new(NodeKind::Number { value: "2".to_string() }, loc);
    /// let program = Node::new(
    ///     NodeKind::Program { statements: vec![first, second] },
    ///     loc,
    /// );
    ///
    /// assert_eq!(program.last_child().map(|n| n.kind.kind_name()), Some("Number"));
    /// assert_eq!(Node::new(NodeKind::Block { statements: vec![] }, loc).last_child(), None);
    /// ```
    #[inline]
    pub fn last_child(&self) -> Option<&Node> {
        let mut result = None;
        self.for_each_child(|child| {
            result = Some(child);
        });
        result
    }

    /// Move every owned child node onto `stack`, leaving structural
    /// placeholders in their fields.
    ///
    /// Detachment runs through [`Self::for_each_child_mut`], the canonical
    /// mutable child traversal, so every registered child relationship is
    /// drained exactly once and new variants inherit destruction safety from
    /// that exhaustive match instead of a second hand-maintained table.
    fn detach_children(&mut self, stack: &mut Vec<Node>) {
        self.for_each_child_mut(|child| {
            stack.push(std::mem::replace(child, Self::detached_placeholder()));
        });
    }

    /// Leaf placeholder written into child slots during detachment.
    ///
    /// Only observable between detachment and field destruction; carries no
    /// heap payload and no children of its own.
    fn detached_placeholder() -> Self {
        Node::new(NodeKind::Ellipsis, SourceLocation::new(0, 0))
    }
}

/// Destroy an owned [`Node`] tree without unbounded stack growth.
///
/// Children are detached into an explicit work stack before each node's own
/// fields are dropped, so destructor depth is constant regardless of tree
/// depth. Payload destructors for non-node fields still run exactly once per
/// value; only the relative order of child destruction changes.
impl Drop for Node {
    fn drop(&mut self) {
        #[cfg(test)]
        drop_audit::record_destroy();

        let mut stack = Vec::new();
        self.detach_children(&mut stack);
        while let Some(mut node) = stack.pop() {
            node.detach_children(&mut stack);

            // Consume the detached payload explicitly.  If `node` were left
            // to the loop-scope drop, its `Node::drop` would run again for
            // every item and allocate another work stack per item.  After
            // detachment, `into_parts` leaves only the childless placeholder
            // in the shell, so its normal drop is constant-time and cannot
            // recurse into the original tree.
            let (kind, _) = node.into_parts();
            drop(kind);
        }
    }
}

mod node_clone;
mod node_debug;
mod node_eq;
mod node_sexp;
mod read_cursor;

pub use node_debug::{
    NODE_DEBUG_MAX_BYTES, NODE_DEBUG_MAX_CHILDREN, NODE_DEBUG_MAX_DEPTH, NODE_DEBUG_MAX_NODES,
    NODE_DEBUG_MAX_PAYLOAD_CHARS, NODE_DEBUG_TRUNCATION_MARKER,
};
pub use node_sexp::{
    NATIVE_DEBUG_SEXP_DEPTH_LIMIT_MARKER, NATIVE_DEBUG_SEXP_GRAMMAR,
    NativeDebugSexpInstrumentCause, NativeDebugSexpLimits, NativeDebugSexpOmitted,
    NativeDebugSexpResult, NativeDebugSexpTruncation, NativeDebugSexpWork,
};
pub use read_cursor::{
    AstReadExact, AstReadInstrumentCause, AstReadLimits, AstReadPath, AstReadPathStep,
    AstReadResult, AstReadTruncation, AstReadWork, DeepestContainingMatch,
};

#[cfg(test)]
mod drop_audit {
    use std::cell::Cell;

    thread_local! {
        static CONSTRUCTED: Cell<u64> = const { Cell::new(0) };
        static DESTROYED: Cell<u64> = const { Cell::new(0) };
    }

    pub(crate) fn record_construct() {
        CONSTRUCTED.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn record_destroy() {
        DESTROYED.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn take_counts() -> (u64, u64) {
        (CONSTRUCTED.take(), DESTROYED.take())
    }
}

/// Comprehensive enumeration of all Perl language constructs supported by the parser.
///
/// This enum represents every possible AST node type that can be parsed from Perl code
/// during the Parse → Index → Navigate → Complete → Analyze workflow. Each variant captures
/// the semantic meaning and structural relationships needed for complete script analysis
/// and transformation.
///
/// # LSP Workflow Integration
///
/// Node kinds are processed differently across workflow stages:
/// - **Parse**: All variants are produced by the parser
/// - **Index**: Symbol-bearing variants feed workspace indexing
/// - **Navigate**: Call and reference variants support navigation features
/// - **Complete**: Expression variants provide completion context
/// - **Analyze**: Semantic variants drive diagnostics and refactoring
///
/// # Examples
///
/// Pattern-match on node kinds to extract semantic information:
///
/// ```
/// use perl_ast::{Node, NodeKind, SourceLocation};
///
/// let loc = SourceLocation::new(0, 5);
/// let node = Node::new(
///     NodeKind::Variable { sigil: "$".to_string(), name: "foo".to_string() },
///     loc,
/// );
///
/// assert!(matches!(
///     &node.kind,
///     NodeKind::Variable { sigil, name } if sigil == "$" && name == "foo"
/// ));
/// ```
///
/// Use [`kind_name()`](NodeKind::kind_name) for debugging and diagnostics:
///
/// ```
/// use perl_ast::NodeKind;
///
/// let kind = NodeKind::Number { value: "99".to_string() };
/// assert_eq!(kind.kind_name(), "Number");
///
/// let kind = NodeKind::Variable { sigil: "@".to_string(), name: "list".to_string() };
/// assert_eq!(kind.kind_name(), "Variable");
/// ```
///
/// # Performance Considerations
///
/// The enum design optimizes for large codebases:
/// - Box pointers minimize stack usage for recursive structures
/// - Vector storage enables efficient bulk operations on child nodes
/// - [`Node`] clone duplicates the owned tree iteratively; [`NodeKind`] clone
///   still goes through [`Node::clone`] for child slots
/// - Pattern matching performance tuned for common Perl constructs
///
/// Dropping a [`NodeKind`] that still owns [`Node`] children is stack-safe
/// because each child uses [`Node`]'s iterative [`Drop`]. Derived [`Clone`]
/// on this enum goes through those same iterative [`Node::clone`] child slots.
/// Derived [`PartialEq`] likewise compares child slots through iterative
/// [`Node::eq`]. [`Debug`] is non-recursive: it shows the kind name and a
/// bounded payload summary without dumping child trees. Tree projection is
/// owned by [`Node`]'s bounded [`Debug`].
#[derive(Clone, PartialEq, strum::VariantNames)]
#[non_exhaustive]
pub enum NodeKind {
    /// Top-level program containing all statements in an Perl script
    ///
    /// This is the root node for any parsed Perl script content, containing all
    /// top-level statements found during the Parse stage of LSP workflow.
    Program {
        /// All top-level statements in the Perl script
        statements: Vec<Node>,
    },

    /// Statement wrapper for expressions that appear at statement level
    ///
    /// Used during Analyze stage to distinguish between expressions used as
    /// statements versus expressions within other contexts during Perl parsing.
    ExpressionStatement {
        /// The expression being used as a statement
        expression: Box<Node>,
    },

    /// Variable declaration with scope declarator in Perl script processing
    ///
    /// Represents declarations like `my $var`, `our $global`, `local $dynamic`, etc.
    /// Critical for Analyze stage symbol table construction during Perl parsing.
    VariableDeclaration {
        /// Scope declarator: "my", "our", "local", "state"
        declarator: String,
        /// The variable being declared
        variable: Box<Node>,
        /// Variable attributes (e.g., ":shared", ":locked")
        attributes: Vec<String>,
        /// Optional initializer expression
        initializer: Option<Box<Node>>,
    },

    /// Multiple variable declaration in a single statement
    ///
    /// Handles constructs like `my ($x, $y) = @values` common in Perl script processing.
    /// Supports efficient bulk variable analysis during Navigate stage operations.
    VariableListDeclaration {
        /// Scope declarator for all variables in the list
        declarator: String,
        /// All variables being declared in the list
        variables: Vec<Node>,
        /// Attributes applied to the variable list
        attributes: Vec<String>,
        /// Optional initializer for the entire variable list
        initializer: Option<Box<Node>>,
    },

    /// Nested variable list within a lexical list declaration.
    ///
    /// Represents a parenthesised group of variables inside a `my`/`our`/`state`
    /// list declaration, such as the `($b, $c)` in `my ($a, ($b, $c)) = ...`.
    /// A nested group with exactly one item is returned unwrapped (as the item
    /// itself), so this variant only appears for two-or-more-item groups.
    NestedVariableList {
        /// The variables or nested lists inside the inner parentheses.
        items: Vec<Node>,
    },

    /// Perl variable reference (scalar, array, hash, etc.) in Perl parsing workflow
    Variable {
        /// Variable sigil indicating type: $, @, %, &, *
        sigil: String, // $, @, %, &, *
        /// Variable name without sigil
        name: String,
    },

    /// Variable with additional attributes for enhanced LSP workflow
    VariableWithAttributes {
        /// The base variable node
        variable: Box<Node>,
        /// List of attribute names applied to the variable
        attributes: Vec<String>,
    },

    /// Assignment operation for LSP data processing workflows
    Assignment {
        /// Left-hand side of assignment
        lhs: Box<Node>,
        /// Right-hand side of assignment
        rhs: Box<Node>,
        /// Assignment operator: =, +=, -=, etc.
        op: String, // =, +=, -=, etc.
    },

    // Expressions
    /// Binary operation for Perl parsing workflow calculations
    Binary {
        /// Binary operator
        op: String,
        /// Left operand
        left: Box<Node>,
        /// Right operand
        right: Box<Node>,
    },

    /// Array slice: `@arr[1, 3, 5]` — returns a list of array elements.
    ///
    /// Distinct from scalar element access (`$arr[idx]`, which stays `Binary { op: "[]" }`)
    /// in that the `@` sigil signals list context and multiple indices.
    ArraySlice {
        /// Target array expression (typically `Variable { sigil: "@", .. }` or a dereference)
        target: Box<Node>,
        /// Index expression (single index or `ArrayLiteral` for multiple indices)
        indices: Box<Node>,
    },

    /// Hash slice: `@hash{qw(a b c)}` — returns a list of hash values.
    ///
    /// The `@` sigil means the result is a list of values for the given keys.
    /// The underlying storage is the `%hash` variable (not `@hash`).
    HashSlice {
        /// Target hash expression (typically `Variable { sigil: "@", .. }` or a dereference)
        target: Box<Node>,
        /// Key expression
        keys: Box<Node>,
    },

    /// Key-value slice: `%hash{qw(a b)}` — returns an interleaved list of key-value pairs.
    ///
    /// The `%` sigil means the result preserves key-value pairing, suitable for
    /// constructing hash subsets or passing to functions expecting key-value lists.
    KeyValueSlice {
        /// Target hash expression (typically `Variable { sigil: "%", .. }` or a dereference)
        target: Box<Node>,
        /// Key expression
        keys: Box<Node>,
    },

    /// Chained comparison expression (Perl 5.32+): `1 < $x < 10`
    ///
    /// Represents two or more consecutive comparison operators at the same
    /// precedence level. Semantically equivalent to `($a op1 $b) && ($b op2 $c)`
    /// with each intermediate operand evaluated only once.
    ///
    /// A single comparison (`$x < 10`) always produces [`Binary`](Self::Binary) instead.
    ChainedComparison {
        /// The N+1 operands in declaration order, where N is the number of operators (N >= 2).
        operands: Vec<Node>,
        /// The N comparison operators between adjacent operand pairs.
        ops: Vec<String>,
    },

    /// Ternary conditional expression for Perl parsing workflow logic
    Ternary {
        /// Condition to evaluate
        condition: Box<Node>,
        /// Expression when condition is true
        then_expr: Box<Node>,
        /// Expression when condition is false
        else_expr: Box<Node>,
    },

    /// Unary operation for Perl parsing workflow
    Unary {
        /// Unary operator
        op: String,
        /// Operand to apply operator to
        operand: Box<Node>,
    },

    // I/O operations
    /// Diamond operator for file input in Perl parsing workflow
    Diamond, // <>

    /// Ellipsis operator for Perl parsing workflow
    Ellipsis, // ...

    /// Undef value for Perl parsing workflow
    Undef, // undef

    /// Readline operation for LSP file processing
    Readline {
        /// Optional filehandle: `<STDIN>`, `<$fh>`, etc.
        filehandle: Option<String>, // <STDIN>, <$fh>, etc.
    },

    /// Glob pattern for LSP workspace file matching
    Glob {
        /// Pattern string for file matching
        pattern: String, // <*.txt>
    },

    /// Typeglob expression: `*foo` or `*main::bar`
    ///
    /// Provides access to all symbol table entries for a given name.
    Typeglob {
        /// Name of the symbol (including package qualification)
        name: String,
    },

    /// Numeric literal in Perl code (integer, float, hex, octal, binary)
    ///
    /// Represents all numeric literal forms: `42`, `3.14`, `0x1A`, `0o755`, `0b1010`.
    Number {
        /// String representation preserving original format
        value: String,
    },

    /// String literal with optional interpolation
    ///
    /// Handles both single-quoted (`'literal'`) and double-quoted (`"$interpolated"`) strings.
    String {
        /// String content (after quote processing)
        value: String,
        /// Whether the string supports variable interpolation
        interpolated: bool,
    },

    /// Version string literal (v-string) like `v1.2.3` or `v5.10.0`
    ///
    /// Semantically distinct from regular strings to support version checking
    /// and special handling in contexts like `use v5.10` and `require v5.8.0`.
    VString {
        /// Version string content (e.g., "v1.2.3")
        value: String,
    },

    /// Heredoc string literal for multi-line content
    ///
    /// Supports all heredoc forms: `<<EOF`, `<<'EOF'`, `<<"EOF"`, `<<~EOF` (indented).
    Heredoc {
        /// Delimiter marking heredoc boundaries
        delimiter: String,
        /// Content between delimiters
        content: String,
        /// Whether content supports variable interpolation
        interpolated: bool,
        /// Whether leading whitespace is stripped (<<~ form)
        indented: bool,
        /// Whether this is a command execution heredoc (<<`EOF`)
        command: bool,
        /// Body span for breakpoint detection (populated by drain_pending_heredocs)
        body_span: Option<SourceLocation>,
    },

    /// Array literal expression: `(1, 2, 3)` or `[1, 2, 3]`
    ArrayLiteral {
        /// Elements in the array
        elements: Vec<Node>,
    },

    /// Hash literal expression: `(key => 'value')` or `{key => 'value'}`
    HashLiteral {
        /// Key-value pairs in the hash
        pairs: Vec<(Node, Node)>,
    },

    /// Block of statements: `{ ... }`
    ///
    /// Used for control structures, subroutine bodies, and bare blocks.
    Block {
        /// Statements within the block
        statements: Vec<Node>,
    },

    /// Eval block for exception handling: `eval { ... }`
    Eval {
        /// Block to evaluate with exception trapping
        block: Box<Node>,
    },

    /// Do block for file inclusion or expression evaluation: `do { ... }` or `do "file"`
    Do {
        /// Block to execute or file expression
        block: Box<Node>,
    },

    /// Defer block for deferred cleanup on scope exit (Perl 5.36+ experimental, stable in 5.40)
    Defer {
        /// Block to execute on scope exit
        block: Box<Node>,
    },

    /// Try-catch-finally for modern exception handling (Syntax::Keyword::Try style)
    Try {
        /// Try block body
        body: Box<Node>,
        /// Catch blocks: (optional exception variable name with its source
        /// location, handler block).  The source location is the precise byte
        /// range of the catch variable as reported by the parser; when there
        /// is no catch variable it is `None`.
        catch_blocks: Vec<(Option<(String, SourceLocation)>, Box<Node>)>,
        /// Optional finally block
        finally_block: Option<Box<Node>>,
    },

    /// If-elsif-else conditional statement
    If {
        /// Condition expression
        condition: Box<Node>,
        /// Then branch block
        then_branch: Box<Node>,
        /// Elsif branches: (condition, block) pairs
        elsif_branches: Vec<(Box<Node>, Box<Node>)>,
        /// Optional else branch
        else_branch: Option<Box<Node>>,
        /// Original keyword: None for 'if', Some("unless") for 'unless' block form.
        keyword: Option<String>,
    },

    /// Statement with a label for loop control: `LABEL: while (...)`
    LabeledStatement {
        /// Label name (e.g., "OUTER", "LINE")
        label: String,
        /// Labeled statement (typically a loop)
        statement: Box<Node>,
    },

    /// While loop: `while (condition) { ... }`
    While {
        /// Loop condition
        condition: Box<Node>,
        /// Loop body
        body: Box<Node>,
        /// Optional continue block
        continue_block: Option<Box<Node>>,
        /// Original keyword: None for 'while', Some("until") for 'until' block form.
        keyword: Option<String>,
    },

    /// Tie operation for binding variables to objects: `tie %hash, 'Package', @args`
    Tie {
        /// Variable being tied
        variable: Box<Node>,
        /// Class/package name to tie to
        package: Box<Node>,
        /// Arguments passed to TIE* method
        args: Vec<Node>,
    },

    /// Untie operation for unbinding variables: `untie %hash`
    Untie {
        /// Variable being untied
        variable: Box<Node>,
    },

    /// C-style for loop: `for (init; cond; update) { ... }`
    For {
        /// Initialization expression
        init: Option<Box<Node>>,
        /// Loop condition
        condition: Option<Box<Node>>,
        /// Update expression
        update: Option<Box<Node>>,
        /// Loop body
        body: Box<Node>,
        /// Optional continue block
        continue_block: Option<Box<Node>>,
    },

    /// Foreach loop: `foreach my $item (@list) { ... }`
    Foreach {
        /// Iterator variable
        variable: Box<Node>,
        /// List to iterate
        list: Box<Node>,
        /// Loop body
        body: Box<Node>,
        /// Optional continue block
        continue_block: Option<Box<Node>>,
    },

    /// Given statement for switch-like matching (Perl 5.10+)
    Given {
        /// Expression to match against
        expr: Box<Node>,
        /// Body containing when/default blocks
        body: Box<Node>,
    },

    /// When clause in given/switch: `when ($pattern) { ... }`
    When {
        /// Pattern to match
        condition: Box<Node>,
        /// Handler block
        body: Box<Node>,
    },

    /// Default clause in given/switch: `default { ... }`
    Default {
        /// Handler block for unmatched cases
        body: Box<Node>,
    },

    /// Statement modifier syntax: `print "ok" if $condition`
    StatementModifier {
        /// Statement to conditionally execute
        statement: Box<Node>,
        /// Modifier keyword: if, unless, while, until, for, foreach
        modifier: String,
        /// Modifier condition
        condition: Box<Node>,
    },

    // Functions
    /// Subroutine declaration (function) including name, prototype, signature and body.
    Subroutine {
        /// Name of the subroutine
        ///
        /// # Precise Navigation Support
        /// - Added name_span for exact LSP navigation
        /// - Enables precise go-to-definition and hover behavior
        /// - O(1) span lookup in workspace symbols
        ///
        /// ## Integration Points
        /// - Semantic token providers
        /// - Cross-reference generation
        /// - Symbol renaming
        name: Option<String>,

        /// Source location span of the subroutine name
        ///
        /// ## Usage Notes
        /// - Always corresponds to the name field
        /// - Provides constant-time position information
        /// - Essential for precise editor interactions
        name_span: Option<SourceLocation>,

        /// Optional scope declarator: "my", "our", or "state" for lexical/package-scoped subs
        ///
        /// # Lexical Subroutines
        /// - Perl 5.18+ feature: `my sub helper { ... }`, `our sub global { ... }`, `state sub memo { ... }`
        /// - Distinguishes lexical scope binding from package-scoped subroutines
        /// - Essential for scope tracking, renaming, and dead code detection
        ///
        /// # Values
        /// - `None` — package-scoped subroutine (no declarator)
        /// - `Some("my")` — lexical subroutine with lexical binding
        /// - `Some("our")` — package-scoped subroutine with explicit package declaration
        /// - `Some("state")` — persistent lexical subroutine (persistent across invocations)
        declarator: Option<String>,

        /// Optional prototype node (e.g. `($;@)`).
        prototype: Option<Box<Node>>,
        /// Optional signature node (Perl 5.20+ feature).
        signature: Option<Box<Node>>,
        /// Attributes attached to the subroutine (`:lvalue`, etc.).
        attributes: Vec<String>,
        /// The body block of the subroutine.
        body: Box<Node>,
    },

    /// Subroutine prototype specification: `sub foo ($;@) { ... }`
    Prototype {
        /// Prototype string defining argument behavior
        content: String,
    },

    /// Subroutine signature (Perl 5.20+): `sub foo ($x, $y = 0) { ... }`
    Signature {
        /// List of signature parameters
        parameters: Vec<Node>,
    },

    /// Mandatory signature parameter: `$x` in `sub foo ($x) { }`
    MandatoryParameter {
        /// Variable being bound
        variable: Box<Node>,
    },

    /// Optional signature parameter with default: `$y = 0` in `sub foo ($y = 0) { }`
    OptionalParameter {
        /// Variable being bound
        variable: Box<Node>,
        /// Default value expression
        default_value: Box<Node>,
    },

    /// Slurpy parameter collecting remaining args: `@rest` or `%opts` in signature
    SlurpyParameter {
        /// Array or hash variable to receive remaining arguments
        variable: Box<Node>,
    },

    /// Named parameter in a signature: `:$alpha` or `:$beta = 1`
    /// (Perl 5.44 named arguments, PPC0024). The caller supplies these by
    /// name (`f(alpha => 1)`); the external key is derived from the lexical
    /// variable name without its sigil.
    NamedParameter {
        /// Variable for named parameter binding (e.g. `$alpha`)
        variable: Box<Node>,
        /// External argument name, derived from the variable name without its
        /// sigil (e.g. `alpha` for `:$alpha`). This is the key callers use.
        external_name: String,
        /// Default-assignment operator when a default is present: `=`, `//=`,
        /// or `||=`. `None` when the parameter has no default.
        default_operator: Option<String>,
        /// Default value expression, when the parameter is defaulted.
        default_value: Option<Box<Node>>,
        /// True when the parameter has no default (the caller must supply it).
        required: bool,
    },

    /// Method declaration (Perl 5.38+ with `use feature 'class'`)
    Method {
        /// Method name
        name: String,
        /// Source location span of the method name
        name_span: Option<SourceLocation>,
        /// Optional signature
        signature: Option<Box<Node>>,
        /// Method attributes (e.g., `:lvalue`)
        attributes: Vec<String>,
        /// Method body
        body: Box<Node>,
    },

    /// Return statement: `return;` or `return $value;`
    Return {
        /// Optional return value
        value: Option<Box<Node>>,
    },

    /// Loop control statement: `next`, `last`, or `redo`
    LoopControl {
        /// Control keyword: "next", "last", or "redo"
        op: String,
        /// Optional label: `next LABEL`
        label: Option<String>,
    },

    /// Goto statement: `goto LABEL`, `goto &sub`, or `goto $expr`
    Goto {
        /// The target of the goto (label identifier, sub reference, or expression)
        target: Box<Node>,
        /// Which of the three goto forms this is.
        ///
        /// Always populated at parse time. Consumers should use this rather than
        /// inspecting the target's node kind, to avoid coupling to target representation.
        form: GotoTargetForm,
    },

    /// Method call: `$obj->method(@args)` or `$obj->method`
    MethodCall {
        /// Object or class expression
        object: Box<Node>,
        /// Method name being called
        method: String,
        /// Method arguments
        args: Vec<Node>,
    },

    /// Function call: `foo(@args)` or `foo()`
    FunctionCall {
        /// Function name (may be qualified: `Package::func`)
        name: String,
        /// Function arguments
        args: Vec<Node>,
    },

    /// Ampersand-sigil subroutine call: `&foo(@args)`, `&Package::sub()`
    ///
    /// Distinct from `FunctionCall`: the explicit `&` sigil bypasses prototypes
    /// and changes argument-passing semantics in Perl (`&foo` with no parens
    /// forwards the caller's `@_` verbatim).
    AmperCall {
        /// Target name (bareword or qualified: `Package::func`)
        name: String,
        /// Call arguments (empty when called without parens)
        args: Vec<Node>,
    },

    /// Indirect object call (legacy syntax): `new Class @args`
    IndirectCall {
        /// Method name
        method: String,
        /// Object or class
        object: Box<Node>,
        /// Arguments
        args: Vec<Node>,
    },

    /// Regex literal: `/pattern/modifiers` or `qr/pattern/modifiers`
    Regex {
        /// Regular expression pattern
        pattern: String,
        /// Replacement string (for s/// when parsed as regex)
        replacement: Option<String>,
        /// Regex modifiers (i, m, s, x, g, etc.)
        modifiers: String,
        /// Whether the regex contains embedded code `(?{...})`
        has_embedded_code: bool,
    },

    /// Match operation: `$str =~ /pattern/modifiers` or `$str !~ /pattern/modifiers`
    Match {
        /// Expression to match against
        expr: Box<Node>,
        /// Pattern to match
        pattern: String,
        /// Match modifiers
        modifiers: String,
        /// Whether the regex contains embedded code `(?{...})`
        has_embedded_code: bool,
        /// Whether the binding operator was `!~` (negated match)
        negated: bool,
    },

    /// Substitution operation: `$str =~ s/pattern/replacement/modifiers`
    Substitution {
        /// Expression to substitute in
        expr: Box<Node>,
        /// Pattern to find
        pattern: String,
        /// Replacement string
        replacement: String,
        /// Substitution modifiers (g, e, r, etc.)
        modifiers: String,
        /// Whether the substitution contains embedded code — either a `(?{...})` inline
        /// code block in the pattern, or the `e`/`ee` modifier which evaluates the
        /// replacement string as Perl code (equivalent to `eval`).
        has_embedded_code: bool,
        /// Whether the binding operator was `!~` (negated match)
        negated: bool,
    },

    /// Transliteration operation: `$str =~ tr/search/replace/` or `y///`
    Transliteration {
        /// Expression to transliterate
        expr: Box<Node>,
        /// Characters to search for
        search: String,
        /// Replacement characters
        replace: String,
        /// Transliteration modifiers (c, d, s, r)
        modifiers: String,
        /// Whether the binding operator was `!~` (negated match)
        negated: bool,
    },

    // Package system
    /// Package declaration (e.g. `package Foo;`) and optional inline block form.
    Package {
        /// Name of the package
        ///
        /// # Precise Navigation Support
        /// - Added name_span for exact LSP navigation
        /// - Enables precise go-to-definition and hover behavior
        /// - O(1) span lookup in workspace symbols
        ///
        /// ## Integration Points
        /// - Workspace indexing
        /// - Cross-module symbol resolution
        /// - Code action providers
        name: String,

        /// Source location span of the package name
        ///
        /// ## Usage Notes
        /// - Always corresponds to the name field
        /// - Provides constant-time position information
        /// - Essential for precise editor interactions
        name_span: SourceLocation,

        /// Optional inline block for `package Foo { ... }` declarations.
        block: Option<Box<Node>>,
    },

    /// Use statement for module loading: `use Module qw(imports);`
    Use {
        /// Module name to load
        module: String,
        /// Import arguments (symbols to import)
        args: Vec<String>,
        /// Whether this module is a known source filter (security risk)
        has_filter_risk: bool,
    },

    /// No statement for disabling features: `no strict;`
    No {
        /// Module/pragma name to disable
        module: String,
        /// Arguments for the no statement
        args: Vec<String>,
        /// Whether this module is a known source filter (security risk)
        has_filter_risk: bool,
    },

    /// Phase block for compile/runtime hooks: `BEGIN`, `END`, `CHECK`, `INIT`, `UNITCHECK`
    PhaseBlock {
        /// Phase name: BEGIN, END, CHECK, INIT, UNITCHECK
        phase: String,
        /// Source location span of the phase block name for precise navigation
        phase_span: Option<SourceLocation>,
        /// Block to execute during the specified phase
        block: Box<Node>,
    },

    /// Data section marker: `__DATA__` or `__END__`
    DataSection {
        /// Section marker (__DATA__ or __END__)
        marker: String,
        /// Source location span of the marker token itself, for precise navigation
        marker_span: Option<SourceLocation>,
        /// Content following the marker (if any)
        body: Option<String>,
        /// Source location span of the payload text following the marker, if any
        body_span: Option<SourceLocation>,
    },

    /// Class declaration (Perl 5.38+ with `use feature 'class'`)
    Class {
        /// Class name
        name: String,
        /// Source location span of the class name
        name_span: Option<SourceLocation>,
        /// Parent class names from `:isa(Parent)` attributes
        parents: Vec<String>,
        /// Class body containing methods and attributes
        body: Box<Node>,
    },

    /// Format declaration for legacy report generation
    Format {
        /// Format name (defaults to filehandle name)
        name: String,
        /// Source location span of the format name
        name_span: Option<SourceLocation>,
        /// Format specification body
        body: String,
    },

    /// Bare identifier (bareword or package-qualified name)
    Identifier {
        /// Identifier string
        name: String,
    },

    /// Parse error placeholder with error message and recovery context
    Error {
        /// Error description
        message: String,
        /// Expected token types (if any)
        expected: Vec<TokenKind>,
        /// The token actually found (if any)
        found: Option<Token>,
        /// Partial AST node parsed before error (if any)
        partial: Option<Box<Node>>,
    },

    /// Missing expression where one was expected.
    ///
    /// Emitted by `recover_missing_infix_rhs` when a binary operator has no
    /// right-hand-side (e.g. `1 +` at end of input). This is the **only**
    /// `Missing*` variant currently emitted by the production parser.
    MissingExpression,

    /// RESERVED — not currently emitted by the parser.
    ///
    /// Retained for API symmetry and future error-recovery work. If recovery
    /// starts emitting this variant, add real parser fixture tests before
    /// shipping. Do not pattern-match on this variant expecting it to appear
    /// in normal parse output.
    MissingStatement,

    /// RESERVED — not currently emitted by the parser.
    ///
    /// Retained for API symmetry and future error-recovery work. If recovery
    /// starts emitting this variant, add real parser fixture tests before
    /// shipping. Do not pattern-match on this variant expecting it to appear
    /// in normal parse output.
    MissingIdentifier,

    /// RESERVED — not currently emitted by the parser.
    ///
    /// Retained for API symmetry and future error-recovery work. If recovery
    /// starts emitting this variant, add real parser fixture tests before
    /// shipping. Do not pattern-match on this variant expecting it to appear
    /// in normal parse output.
    MissingBlock,

    /// Lexer budget exceeded marker preserving partial parse results
    ///
    /// Used when recursion or token limits are hit to preserve already-parsed content.
    UnknownRest,
}

impl NodeKind {
    /// Get the name of this `NodeKind` as a static string.
    ///
    /// Useful for diagnostics, logging, and human-readable AST dumps.
    ///
    /// # Examples
    ///
    /// ```
    /// use perl_ast::NodeKind;
    ///
    /// let kind = NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() };
    /// assert_eq!(kind.kind_name(), "Variable");
    ///
    /// let kind = NodeKind::Program { statements: vec![] };
    /// assert_eq!(kind.kind_name(), "Program");
    /// ```
    pub fn kind_name(&self) -> &'static str {
        match self {
            NodeKind::Program { .. } => "Program",
            NodeKind::ExpressionStatement { .. } => "ExpressionStatement",
            NodeKind::VariableDeclaration { .. } => "VariableDeclaration",
            NodeKind::VariableListDeclaration { .. } => "VariableListDeclaration",
            NodeKind::NestedVariableList { .. } => "NestedVariableList",
            NodeKind::Variable { .. } => "Variable",
            NodeKind::VariableWithAttributes { .. } => "VariableWithAttributes",
            NodeKind::Assignment { .. } => "Assignment",
            NodeKind::Binary { .. } => "Binary",
            NodeKind::ArraySlice { .. } => "ArraySlice",
            NodeKind::HashSlice { .. } => "HashSlice",
            NodeKind::KeyValueSlice { .. } => "KeyValueSlice",
            NodeKind::ChainedComparison { .. } => "ChainedComparison",
            NodeKind::Ternary { .. } => "Ternary",
            NodeKind::Unary { .. } => "Unary",
            NodeKind::Diamond => "Diamond",
            NodeKind::Ellipsis => "Ellipsis",
            NodeKind::Undef => "Undef",
            NodeKind::Readline { .. } => "Readline",
            NodeKind::Glob { .. } => "Glob",
            NodeKind::Typeglob { .. } => "Typeglob",
            NodeKind::Number { .. } => "Number",
            NodeKind::String { .. } => "String",
            NodeKind::VString { .. } => "VString",
            NodeKind::Heredoc { .. } => "Heredoc",
            NodeKind::ArrayLiteral { .. } => "ArrayLiteral",
            NodeKind::HashLiteral { .. } => "HashLiteral",
            NodeKind::Block { .. } => "Block",
            NodeKind::Eval { .. } => "Eval",
            NodeKind::Do { .. } => "Do",
            NodeKind::Defer { .. } => "Defer",
            NodeKind::Try { .. } => "Try",
            NodeKind::If { .. } => "If",
            NodeKind::LabeledStatement { .. } => "LabeledStatement",
            NodeKind::While { .. } => "While",
            NodeKind::Tie { .. } => "Tie",
            NodeKind::Untie { .. } => "Untie",
            NodeKind::For { .. } => "For",
            NodeKind::Foreach { .. } => "Foreach",
            NodeKind::Given { .. } => "Given",
            NodeKind::When { .. } => "When",
            NodeKind::Default { .. } => "Default",
            NodeKind::StatementModifier { .. } => "StatementModifier",
            NodeKind::Subroutine { .. } => "Subroutine",
            NodeKind::Prototype { .. } => "Prototype",
            NodeKind::Signature { .. } => "Signature",
            NodeKind::MandatoryParameter { .. } => "MandatoryParameter",
            NodeKind::OptionalParameter { .. } => "OptionalParameter",
            NodeKind::SlurpyParameter { .. } => "SlurpyParameter",
            NodeKind::NamedParameter { .. } => "NamedParameter",
            NodeKind::Method { .. } => "Method",
            NodeKind::Return { .. } => "Return",
            NodeKind::LoopControl { .. } => "LoopControl",
            NodeKind::Goto { .. } => "Goto",
            NodeKind::MethodCall { .. } => "MethodCall",
            NodeKind::FunctionCall { .. } => "FunctionCall",
            NodeKind::AmperCall { .. } => "AmperCall",
            NodeKind::IndirectCall { .. } => "IndirectCall",
            NodeKind::Regex { .. } => "Regex",
            NodeKind::Match { .. } => "Match",
            NodeKind::Substitution { .. } => "Substitution",
            NodeKind::Transliteration { .. } => "Transliteration",
            NodeKind::Package { .. } => "Package",
            NodeKind::Use { .. } => "Use",
            NodeKind::No { .. } => "No",
            NodeKind::PhaseBlock { .. } => "PhaseBlock",
            NodeKind::DataSection { .. } => "DataSection",
            NodeKind::Class { .. } => "Class",
            NodeKind::Format { .. } => "Format",
            NodeKind::Identifier { .. } => "Identifier",
            NodeKind::Error { .. } => "Error",
            NodeKind::MissingExpression => "MissingExpression",
            NodeKind::MissingStatement => "MissingStatement",
            NodeKind::MissingIdentifier => "MissingIdentifier",
            NodeKind::MissingBlock => "MissingBlock",
            NodeKind::UnknownRest => "UnknownRest",
        }
    }

    /// Return the grammar kind when it is determined solely by the node variant.
    ///
    /// This is the allocation-free metadata path used by tree-sitter-style
    /// facades.  `None` is reserved for variants whose grammar kind depends on
    /// a runtime field such as an operator, keyword, or interpolation mode;
    /// callers should use [`grammar_kind_name`](Self::grammar_kind_name) when
    /// they need the complete name.
    pub fn grammar_kind_name_static(&self) -> Option<&'static str> {
        match self {
            NodeKind::Program { .. } => Some("source_file"),
            NodeKind::ExpressionStatement { .. } => Some("expression_statement"),
            NodeKind::VariableDeclaration { .. }
            | NodeKind::VariableListDeclaration { .. }
            | NodeKind::Assignment { .. }
            | NodeKind::Binary { .. }
            | NodeKind::Unary { .. }
            | NodeKind::String { .. }
            | NodeKind::Heredoc { .. }
            | NodeKind::If { .. }
            | NodeKind::While { .. }
            | NodeKind::StatementModifier { .. }
            | NodeKind::Subroutine { .. }
            | NodeKind::LoopControl { .. }
            | NodeKind::FunctionCall { .. }
            | NodeKind::AmperCall { .. }
            | NodeKind::Match { .. }
            | NodeKind::PhaseBlock { .. } => None,
            NodeKind::ArraySlice { .. } => Some("array_slice"),
            NodeKind::HashSlice { .. } => Some("hash_slice"),
            NodeKind::KeyValueSlice { .. } => Some("key_value_slice"),
            NodeKind::ChainedComparison { .. } => Some("chained_comparison"),
            NodeKind::NestedVariableList { .. } => Some("nested_variable_list"),
            NodeKind::Variable { .. } => Some("variable"),
            NodeKind::VariableWithAttributes { .. } => Some("variable_with_attributes"),
            NodeKind::Ternary { .. } => Some("ternary"),
            NodeKind::Diamond => Some("diamond"),
            NodeKind::Ellipsis => Some("ellipsis"),
            NodeKind::Undef => Some("undef"),
            NodeKind::Readline { .. } => Some("readline"),
            NodeKind::Glob { .. } => Some("glob"),
            NodeKind::Typeglob { .. } => Some("typeglob"),
            NodeKind::Number { .. } => Some("number"),
            NodeKind::VString { .. } => Some("vstring"),
            NodeKind::ArrayLiteral { .. } => Some("array"),
            NodeKind::HashLiteral { .. } => Some("hash"),
            NodeKind::Block { .. } => Some("block"),
            NodeKind::Eval { .. } => Some("eval"),
            NodeKind::Do { .. } => Some("do"),
            NodeKind::Defer { .. } => Some("defer"),
            NodeKind::Try { .. } => Some("try"),
            NodeKind::LabeledStatement { .. } => Some("labeled_statement"),
            NodeKind::Tie { .. } => Some("tie"),
            NodeKind::Untie { .. } => Some("untie"),
            NodeKind::For { .. } => Some("for"),
            NodeKind::Foreach { .. } => Some("foreach"),
            NodeKind::Given { .. } => Some("given"),
            NodeKind::When { .. } => Some("when"),
            NodeKind::Default { .. } => Some("default"),
            NodeKind::Prototype { .. } => Some("prototype"),
            NodeKind::Signature { .. } => Some("signature"),
            NodeKind::MandatoryParameter { .. } => Some("mandatory_parameter"),
            NodeKind::OptionalParameter { .. } => Some("optional_parameter"),
            NodeKind::SlurpyParameter { .. } => Some("slurpy_parameter"),
            NodeKind::NamedParameter { .. } => Some("named_parameter"),
            NodeKind::Method { .. } => Some("method_declaration_statement"),
            NodeKind::Return { .. } => Some("return"),
            NodeKind::Goto { .. } => Some("goto"),
            NodeKind::MethodCall { .. } => Some("method_call"),
            NodeKind::IndirectCall { .. } => Some("indirect_call"),
            NodeKind::Regex { .. } => Some("regex"),
            NodeKind::Substitution { .. } => Some("substitution"),
            NodeKind::Transliteration { .. } => Some("transliteration"),
            NodeKind::Package { .. } => Some("package"),
            NodeKind::Use { .. } => Some("use"),
            NodeKind::No { .. } => Some("no"),
            NodeKind::DataSection { .. } => Some("data_section"),
            NodeKind::Class { .. } => Some("class"),
            NodeKind::Format { .. } => Some("format"),
            NodeKind::Identifier { .. } => Some("identifier"),
            NodeKind::Error { .. } => Some("ERROR"),
            NodeKind::MissingExpression => Some("missing_expression"),
            NodeKind::MissingStatement => Some("missing_statement"),
            NodeKind::MissingIdentifier => Some("missing_identifier"),
            NodeKind::MissingBlock => Some("missing_block"),
            NodeKind::UnknownRest => Some("UNKNOWN_REST"),
        }
    }

    /// Return the tree-sitter-style grammar kind without serializing the subtree.
    ///
    /// Most variants use [`grammar_kind_name_static`](Self::grammar_kind_name_static),
    /// so lookup is O(1) and independent of subtree size. The returned `String`
    /// may still allocate; the performance win is avoiding a full S-expression
    /// traversal and allocation. Only runtime-derived names take the dynamic path.
    pub fn grammar_kind_name(&self) -> String {
        if let Some(name) = self.grammar_kind_name_static() {
            return name.to_string();
        }

        match self {
            NodeKind::VariableDeclaration { declarator, .. }
            | NodeKind::VariableListDeclaration { declarator, .. } => {
                format!("{declarator}_declaration")
            }
            NodeKind::Assignment { op, .. } => {
                format!("assignment_{}", op.replace('=', "assign"))
            }
            NodeKind::Binary { op, .. } => format_binary_operator(op),
            NodeKind::Unary { op, .. } => format_unary_operator(op),
            NodeKind::String { interpolated, .. } => {
                if *interpolated { "string_interpolated" } else { "string" }.to_string()
            }
            NodeKind::Heredoc { interpolated, indented, command, .. } => {
                let name = if *command {
                    "heredoc_command"
                } else if *indented {
                    if *interpolated { "heredoc_indented_interpolated" } else { "heredoc_indented" }
                } else if *interpolated {
                    "heredoc_interpolated"
                } else {
                    "heredoc"
                };
                name.to_string()
            }
            NodeKind::If { keyword, .. } => keyword.as_deref().unwrap_or("if").to_string(),
            NodeKind::While { keyword, .. } => keyword.as_deref().unwrap_or("while").to_string(),
            NodeKind::StatementModifier { modifier, .. } => {
                format!("statement_modifier_{modifier}")
            }
            NodeKind::Subroutine { name, .. } => {
                if name.is_some() { "sub" } else { "anonymous_subroutine_expression" }.to_string()
            }
            NodeKind::LoopControl { op, .. } => op.clone(),
            NodeKind::FunctionCall { name, args } => if is_call_form_function(name) {
                "call"
            } else if args.is_empty() {
                "function_call_expression"
            } else {
                "ambiguous_function_call_expression"
            }
            .to_string(),
            NodeKind::AmperCall { args, .. } => {
                if args.is_empty() { "amper_sub" } else { "amper_call_expression" }.to_string()
            }
            NodeKind::Match { negated, .. } => {
                if *negated { "not_match" } else { "match" }.to_string()
            }
            NodeKind::PhaseBlock { phase, .. } => phase.clone(),
            // Every variant with a runtime-derived grammar name is covered
            // above; the exhaustive match is the drift guard for this table.
            NodeKind::ArraySlice { .. }
            | NodeKind::HashSlice { .. }
            | NodeKind::KeyValueSlice { .. }
            | NodeKind::ChainedComparison { .. }
            | NodeKind::NestedVariableList { .. }
            | NodeKind::Variable { .. }
            | NodeKind::VariableWithAttributes { .. }
            | NodeKind::Ternary { .. }
            | NodeKind::Diamond
            | NodeKind::Ellipsis
            | NodeKind::Undef
            | NodeKind::Readline { .. }
            | NodeKind::Glob { .. }
            | NodeKind::Typeglob { .. }
            | NodeKind::Number { .. }
            | NodeKind::VString { .. }
            | NodeKind::ArrayLiteral { .. }
            | NodeKind::HashLiteral { .. }
            | NodeKind::Block { .. }
            | NodeKind::Eval { .. }
            | NodeKind::Do { .. }
            | NodeKind::Defer { .. }
            | NodeKind::Try { .. }
            | NodeKind::LabeledStatement { .. }
            | NodeKind::Tie { .. }
            | NodeKind::Untie { .. }
            | NodeKind::For { .. }
            | NodeKind::Foreach { .. }
            | NodeKind::Given { .. }
            | NodeKind::When { .. }
            | NodeKind::Default { .. }
            | NodeKind::Prototype { .. }
            | NodeKind::Signature { .. }
            | NodeKind::MandatoryParameter { .. }
            | NodeKind::OptionalParameter { .. }
            | NodeKind::SlurpyParameter { .. }
            | NodeKind::NamedParameter { .. }
            | NodeKind::Method { .. }
            | NodeKind::Return { .. }
            | NodeKind::Goto { .. }
            | NodeKind::MethodCall { .. }
            | NodeKind::IndirectCall { .. }
            | NodeKind::Regex { .. }
            | NodeKind::Substitution { .. }
            | NodeKind::Transliteration { .. }
            | NodeKind::Package { .. }
            | NodeKind::Use { .. }
            | NodeKind::No { .. }
            | NodeKind::DataSection { .. }
            | NodeKind::Class { .. }
            | NodeKind::Format { .. }
            | NodeKind::Identifier { .. }
            | NodeKind::Error { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock
            | NodeKind::UnknownRest
            | NodeKind::Program { .. }
            | NodeKind::ExpressionStatement { .. } => {
                // The preceding static lookup handled these variants.
                self.grammar_kind_name_static()
                    .map_or_else(|| self.kind_name().to_string(), str::to_owned)
            }
        }
    }

    /// Canonical list of **all** `kind_name()` strings, in declaration order.
    ///
    /// Auto-derived from the `NodeKind` enum via `strum::VariantNames` — adding a new
    /// variant automatically updates this list. No manual maintenance required.
    ///
    /// Every consumer that needs the full set of NodeKind names should reference
    /// this constant instead of maintaining a hand-written copy.
    pub const ALL_KIND_NAMES: &[&'static str] = NodeKind::VARIANTS;

    /// Subset of `ALL_KIND_NAMES` that represent synthetic/recovery nodes.
    ///
    /// These kinds are only produced by `parse_with_recovery()` on malformed
    /// input and should not be expected in clean parses.
    pub const RECOVERY_KIND_NAMES: &[&'static str] = &[
        "Error",
        "MissingBlock",
        "MissingExpression",
        "MissingIdentifier",
        "MissingStatement",
        "UnknownRest",
    ];
}

impl fmt::Display for NodeKind {
    /// Formats as the canonical `kind_name()` string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind_name())
    }
}

/// Format unary operator for S-expression output
fn format_unary_operator(op: &str) -> String {
    match op {
        // Arithmetic unary operators
        "+" => "unary_+".to_string(),
        "-" => "unary_-".to_string(),

        // Logical unary operators
        "!" => "unary_not".to_string(),
        "not" => "unary_not".to_string(),

        // Bitwise complement
        "~" => "unary_complement".to_string(),

        // Reference operator
        "\\" => "unary_ref".to_string(),

        // Postfix operators
        "++" => "unary_++".to_string(),
        "--" => "unary_--".to_string(),

        // File test operators
        "-f" => "unary_-f".to_string(),
        "-d" => "unary_-d".to_string(),
        "-e" => "unary_-e".to_string(),
        "-r" => "unary_-r".to_string(),
        "-w" => "unary_-w".to_string(),
        "-x" => "unary_-x".to_string(),
        "-o" => "unary_-o".to_string(),
        "-R" => "unary_-R".to_string(),
        "-W" => "unary_-W".to_string(),
        "-X" => "unary_-X".to_string(),
        "-O" => "unary_-O".to_string(),
        "-s" => "unary_-s".to_string(),
        "-p" => "unary_-p".to_string(),
        "-S" => "unary_-S".to_string(),
        "-b" => "unary_-b".to_string(),
        "-c" => "unary_-c".to_string(),
        "-t" => "unary_-t".to_string(),
        "-u" => "unary_-u".to_string(),
        "-g" => "unary_-g".to_string(),
        "-k" => "unary_-k".to_string(),
        "-T" => "unary_-T".to_string(),
        "-B" => "unary_-B".to_string(),
        "-M" => "unary_-M".to_string(),
        "-A" => "unary_-A".to_string(),
        "-C" => "unary_-C".to_string(),
        "-l" => "unary_-l".to_string(),
        "-z" => "unary_-z".to_string(),

        // Postfix dereferencing
        "->@*" => "unary_->@*".to_string(),
        "->%*" => "unary_->%*".to_string(),
        "->$*" => "unary_->$*".to_string(),
        "->&*" => "unary_->&*".to_string(),
        "->**" => "unary_->**".to_string(),

        // Defined operator
        "defined" => "unary_defined".to_string(),

        // Default case for unknown operators
        _ => format!("unary_{}", op.replace(' ', "_")),
    }
}

/// Whether a function call uses the explicit `(call name (args...))` form.
///
/// This predicate is shared by S-expression rendering and grammar-kind
/// metadata so those two public representations cannot drift apart.
fn is_call_form_function(name: &str) -> bool {
    matches!(
        name,
        "bless"
            | "shift"
            | "unshift"
            | "open"
            | "die"
            | "warn"
            | "print"
            | "printf"
            | "say"
            | "push"
            | "pop"
            | "map"
            | "sort"
            | "grep"
            | "keys"
            | "values"
            | "each"
            | "defined"
            | "scalar"
            | "ref"
    )
}

/// Format binary operator for S-expression output
fn format_binary_operator(op: &str) -> String {
    match op {
        // Arithmetic operators
        "+" => "binary_+".to_string(),
        "-" => "binary_-".to_string(),
        "*" => "binary_*".to_string(),
        "/" => "binary_/".to_string(),
        "%" => "binary_%".to_string(),
        "**" => "binary_**".to_string(),

        // Comparison operators
        "==" => "binary_==".to_string(),
        "!=" => "binary_!=".to_string(),
        "<" => "binary_<".to_string(),
        ">" => "binary_>".to_string(),
        "<=" => "binary_<=".to_string(),
        ">=" => "binary_>=".to_string(),
        "<=>" => "binary_<=>".to_string(),

        // String comparison
        "eq" => "binary_eq".to_string(),
        "ne" => "binary_ne".to_string(),
        "lt" => "binary_lt".to_string(),
        "le" => "binary_le".to_string(),
        "gt" => "binary_gt".to_string(),
        "ge" => "binary_ge".to_string(),
        "cmp" => "binary_cmp".to_string(),

        // Logical operators
        "&&" => "binary_&&".to_string(),
        "||" => "binary_||".to_string(),
        "and" => "binary_and".to_string(),
        "or" => "binary_or".to_string(),
        "xor" => "binary_xor".to_string(),

        // Bitwise operators
        "&" => "binary_&".to_string(),
        "|" => "binary_|".to_string(),
        "^" => "binary_^".to_string(),
        "<<" => "binary_<<".to_string(),
        ">>" => "binary_>>".to_string(),

        // Pattern matching
        "=~" => "binary_=~".to_string(),
        "!~" => "binary_!~".to_string(),

        // Smart match
        "~~" => "binary_~~".to_string(),

        // String repetition
        "x" => "binary_x".to_string(),

        // Concatenation
        "." => "binary_.".to_string(),

        // Range operators
        ".." => "binary_..".to_string(),
        "..." => "binary_...".to_string(),

        // Type checking
        "isa" => "binary_isa".to_string(),

        // Assignment operators
        "=" => "binary_=".to_string(),
        "+=" => "binary_+=".to_string(),
        "-=" => "binary_-=".to_string(),
        "*=" => "binary_*=".to_string(),
        "/=" => "binary_/=".to_string(),
        "%=" => "binary_%=".to_string(),
        "**=" => "binary_**=".to_string(),
        ".=" => "binary_.=".to_string(),
        "&=" => "binary_&=".to_string(),
        "|=" => "binary_|=".to_string(),
        "^=" => "binary_^=".to_string(),
        "<<=" => "binary_<<=".to_string(),
        ">>=" => "binary_>>=".to_string(),
        "&&=" => "binary_&&=".to_string(),
        "||=" => "binary_||=".to_string(),
        "//=" => "binary_//=".to_string(),

        // Defined-or operator
        "//" => "binary_//".to_string(),

        // Method calls and dereferencing
        "->" => "binary_->".to_string(),

        // Hash/array access
        "{}" => "binary_{}".to_string(),
        "[]" => "binary_[]".to_string(),

        // Arrow hash/array dereference
        "->{}" => "arrow_hash_deref".to_string(),
        "->[]" => "arrow_array_deref".to_string(),

        // Default case for unknown operators
        _ => format!("binary_{}", op.replace(' ', "_")),
    }
}

// SourceLocation is now provided by perl-position-tracking crate
// See the re-export at the top of this file

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Build a dummy instance for every `NodeKind` variant.
    ///
    /// Keeping this constructor exhaustive makes metadata tests fail at compile
    /// time when a new variant is added without being classified deliberately.
    fn all_node_kinds() -> Vec<NodeKind> {
        let loc = SourceLocation::new(0, 0);
        let dummy_node = || Node::new(NodeKind::Undef, loc);

        let variants: Vec<NodeKind> = vec![
            NodeKind::Program { statements: vec![] },
            NodeKind::ExpressionStatement { expression: Box::new(dummy_node()) },
            NodeKind::VariableDeclaration {
                declarator: String::new(),
                variable: Box::new(dummy_node()),
                attributes: vec![],
                initializer: None,
            },
            NodeKind::VariableListDeclaration {
                declarator: String::new(),
                variables: vec![],
                attributes: vec![],
                initializer: None,
            },
            NodeKind::NestedVariableList { items: vec![] },
            NodeKind::Variable { sigil: String::new(), name: String::new() },
            NodeKind::VariableWithAttributes {
                variable: Box::new(dummy_node()),
                attributes: vec![],
            },
            NodeKind::Assignment {
                lhs: Box::new(dummy_node()),
                rhs: Box::new(dummy_node()),
                op: String::new(),
            },
            NodeKind::Binary {
                op: String::new(),
                left: Box::new(dummy_node()),
                right: Box::new(dummy_node()),
            },
            NodeKind::ArraySlice {
                target: Box::new(dummy_node()),
                indices: Box::new(dummy_node()),
            },
            NodeKind::HashSlice { target: Box::new(dummy_node()), keys: Box::new(dummy_node()) },
            NodeKind::KeyValueSlice {
                target: Box::new(dummy_node()),
                keys: Box::new(dummy_node()),
            },
            NodeKind::ChainedComparison { operands: vec![], ops: vec![] },
            NodeKind::Ternary {
                condition: Box::new(dummy_node()),
                then_expr: Box::new(dummy_node()),
                else_expr: Box::new(dummy_node()),
            },
            NodeKind::Unary { op: String::new(), operand: Box::new(dummy_node()) },
            NodeKind::Diamond,
            NodeKind::Ellipsis,
            NodeKind::Undef,
            NodeKind::Readline { filehandle: None },
            NodeKind::Glob { pattern: String::new() },
            NodeKind::Typeglob { name: String::new() },
            NodeKind::Number { value: String::new() },
            NodeKind::String { value: String::new(), interpolated: false },
            NodeKind::VString { value: String::new() },
            NodeKind::Heredoc {
                delimiter: String::new(),
                content: String::new(),
                interpolated: false,
                indented: false,
                command: false,
                body_span: None,
            },
            NodeKind::ArrayLiteral { elements: vec![] },
            NodeKind::HashLiteral { pairs: vec![] },
            NodeKind::Block { statements: vec![] },
            NodeKind::Eval { block: Box::new(dummy_node()) },
            NodeKind::Do { block: Box::new(dummy_node()) },
            NodeKind::Defer { block: Box::new(dummy_node()) },
            NodeKind::Try {
                body: Box::new(dummy_node()),
                catch_blocks: vec![],
                finally_block: None,
            },
            NodeKind::If {
                condition: Box::new(dummy_node()),
                then_branch: Box::new(dummy_node()),
                elsif_branches: vec![],
                else_branch: None,
                keyword: None,
            },
            NodeKind::LabeledStatement { label: String::new(), statement: Box::new(dummy_node()) },
            NodeKind::While {
                condition: Box::new(dummy_node()),
                body: Box::new(dummy_node()),
                continue_block: None,
                keyword: None,
            },
            NodeKind::Tie {
                variable: Box::new(dummy_node()),
                package: Box::new(dummy_node()),
                args: vec![],
            },
            NodeKind::Untie { variable: Box::new(dummy_node()) },
            NodeKind::For {
                init: None,
                condition: None,
                update: None,
                body: Box::new(dummy_node()),
                continue_block: None,
            },
            NodeKind::Foreach {
                variable: Box::new(dummy_node()),
                list: Box::new(dummy_node()),
                body: Box::new(dummy_node()),
                continue_block: None,
            },
            NodeKind::Given { expr: Box::new(dummy_node()), body: Box::new(dummy_node()) },
            NodeKind::When { condition: Box::new(dummy_node()), body: Box::new(dummy_node()) },
            NodeKind::Default { body: Box::new(dummy_node()) },
            NodeKind::StatementModifier {
                statement: Box::new(dummy_node()),
                modifier: String::new(),
                condition: Box::new(dummy_node()),
            },
            NodeKind::Subroutine {
                name: None,
                name_span: None,
                declarator: None,
                prototype: None,
                signature: None,
                attributes: vec![],
                body: Box::new(dummy_node()),
            },
            NodeKind::Prototype { content: String::new() },
            NodeKind::Signature { parameters: vec![] },
            NodeKind::MandatoryParameter { variable: Box::new(dummy_node()) },
            NodeKind::OptionalParameter {
                variable: Box::new(dummy_node()),
                default_value: Box::new(dummy_node()),
            },
            NodeKind::SlurpyParameter { variable: Box::new(dummy_node()) },
            NodeKind::NamedParameter {
                variable: Box::new(dummy_node()),
                external_name: String::new(),
                default_operator: None,
                default_value: None,
                required: true,
            },
            NodeKind::Method {
                name: String::new(),
                name_span: None,
                signature: None,
                attributes: vec![],
                body: Box::new(dummy_node()),
            },
            NodeKind::Return { value: None },
            NodeKind::LoopControl { op: String::new(), label: None },
            NodeKind::Goto { target: Box::new(dummy_node()), form: GotoTargetForm::Label },
            NodeKind::MethodCall {
                object: Box::new(dummy_node()),
                method: String::new(),
                args: vec![],
            },
            NodeKind::FunctionCall { name: String::new(), args: vec![] },
            NodeKind::AmperCall { name: String::new(), args: vec![] },
            NodeKind::IndirectCall {
                method: String::new(),
                object: Box::new(dummy_node()),
                args: vec![],
            },
            NodeKind::Regex {
                pattern: String::new(),
                replacement: None,
                modifiers: String::new(),
                has_embedded_code: false,
            },
            NodeKind::Match {
                expr: Box::new(dummy_node()),
                pattern: String::new(),
                modifiers: String::new(),
                has_embedded_code: false,
                negated: false,
            },
            NodeKind::Substitution {
                expr: Box::new(dummy_node()),
                pattern: String::new(),
                replacement: String::new(),
                modifiers: String::new(),
                has_embedded_code: false,
                negated: false,
            },
            NodeKind::Transliteration {
                expr: Box::new(dummy_node()),
                search: String::new(),
                replace: String::new(),
                modifiers: String::new(),
                negated: false,
            },
            NodeKind::Package { name: String::new(), name_span: loc, block: None },
            NodeKind::Use { module: String::new(), args: vec![], has_filter_risk: false },
            NodeKind::No { module: String::new(), args: vec![], has_filter_risk: false },
            NodeKind::PhaseBlock {
                phase: String::new(),
                phase_span: None,
                block: Box::new(dummy_node()),
            },
            NodeKind::DataSection {
                marker: String::new(),
                marker_span: None,
                body: None,
                body_span: None,
            },
            NodeKind::Class {
                name: String::new(),
                name_span: None,
                parents: vec![],
                body: Box::new(dummy_node()),
            },
            NodeKind::Format { name: String::new(), name_span: None, body: String::new() },
            NodeKind::Identifier { name: String::new() },
            NodeKind::Error {
                message: String::new(),
                expected: vec![],
                found: None,
                partial: None,
            },
            NodeKind::MissingExpression,
            NodeKind::MissingStatement,
            NodeKind::MissingIdentifier,
            NodeKind::MissingBlock,
            NodeKind::UnknownRest,
        ];

        variants
    }

    /// Return the set of `kind_name()` values represented by every variant.
    fn all_kind_names_from_variants() -> BTreeSet<&'static str> {
        all_node_kinds().iter().map(|v| v.kind_name()).collect()
    }

    #[test]
    fn for_each_child_mut_nested_variable_list() {
        // Covers lines 876-879: NestedVariableList arm in for_each_child_mut.
        let loc = SourceLocation::new(0, 10);
        let item_a =
            Node::new(NodeKind::Variable { sigil: "$".to_string(), name: "a".to_string() }, loc);
        let item_b =
            Node::new(NodeKind::Variable { sigil: "$".to_string(), name: "b".to_string() }, loc);
        let mut node = Node::new(NodeKind::NestedVariableList { items: vec![item_a, item_b] }, loc);
        let mut count = 0;
        node.for_each_child_mut(|_child| count += 1);
        assert_eq!(count, 2, "for_each_child_mut should visit both items in NestedVariableList");
    }

    #[test]
    fn for_each_child_nested_variable_list() {
        // Covers lines 1131-1134: NestedVariableList arm in for_each_child.
        let loc = SourceLocation::new(0, 10);
        let item_a =
            Node::new(NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() }, loc);
        let item_b =
            Node::new(NodeKind::Variable { sigil: "$".to_string(), name: "y".to_string() }, loc);
        let node = Node::new(NodeKind::NestedVariableList { items: vec![item_a, item_b] }, loc);
        let mut names = Vec::new();
        node.for_each_child(|child| {
            if let NodeKind::Variable { name, .. } = &child.kind {
                names.push(name.clone());
            }
        });
        assert_eq!(
            names,
            vec!["x", "y"],
            "for_each_child should visit all items in NestedVariableList"
        );
    }

    #[test]
    fn field_ids_round_trip_through_canonical_names() {
        for field in FieldId::ALL {
            assert_eq!(FieldId::from_name(field.name()), Some(*field));
        }
    }

    #[test]
    fn field_aware_traversal_preserves_structural_order() {
        let loc = SourceLocation::new(0, 1);
        let leaf = || Node::new(NodeKind::Number { value: "1".into() }, loc);
        let node = Node::new(
            NodeKind::If {
                condition: Box::new(leaf()),
                then_branch: Box::new(leaf()),
                elsif_branches: vec![(Box::new(leaf()), Box::new(leaf()))],
                else_branch: Some(Box::new(leaf())),
                keyword: None,
            },
            loc,
        );

        let fields: Vec<_> = {
            let mut fields = Vec::new();
            node.for_each_child_with_field(|field, child| {
                fields.push((field.map(FieldId::name), child.kind.kind_name()));
            });
            fields
        };

        assert_eq!(
            fields,
            vec![
                (Some("condition"), "Number"),
                (Some("then_branch"), "Number"),
                (Some("condition"), "Number"),
                (Some("body"), "Number"),
                (Some("else_branch"), "Number"),
            ]
        );
        assert_eq!(node.children().len(), fields.len());
    }

    #[test]
    fn field_aware_traversal_labels_repeated_container_children() {
        let loc = SourceLocation::new(0, 1);
        let leaf = || Node::new(NodeKind::Number { value: "1".into() }, loc);
        let node = Node::new(
            NodeKind::HashLiteral { pairs: vec![(leaf(), leaf()), (leaf(), leaf())] },
            loc,
        );
        let mut names = Vec::new();
        node.for_each_child_with_field(|field, _| names.push(field.map(FieldId::name)));
        assert_eq!(names, vec![Some("key"), Some("value"), Some("key"), Some("value")]);
    }

    #[test]
    fn field_aware_metadata_covers_declarations_calls_signatures_and_recovery() {
        let loc = SourceLocation::new(0, 1);
        let leaf = || Node::new(NodeKind::Number { value: "1".into() }, loc);
        let names = |node: &Node| {
            let mut names = Vec::new();
            node.for_each_child_with_field(|field, _| names.push(field.map(FieldId::name)));
            names
        };

        let declaration = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".into(),
                variable: Box::new(leaf()),
                attributes: vec!["lvalue".into()],
                initializer: Some(Box::new(leaf())),
            },
            loc,
        );
        assert_eq!(names(&declaration), vec![Some("variable"), Some("initializer")]);

        let binary = Node::new(
            NodeKind::Binary { op: "+".into(), left: Box::new(leaf()), right: Box::new(leaf()) },
            loc,
        );
        assert_eq!(names(&binary), vec![Some("left"), Some("right")]);

        let call = Node::new(
            NodeKind::MethodCall {
                object: Box::new(leaf()),
                method: "run".into(),
                args: vec![leaf(), leaf()],
            },
            loc,
        );
        assert_eq!(names(&call), vec![Some("object"), Some("args"), Some("args")]);

        let subroutine = Node::new(
            NodeKind::Subroutine {
                name: Some("run".into()),
                name_span: None,
                declarator: None,
                prototype: Some(Box::new(leaf())),
                signature: Some(Box::new(leaf())),
                attributes: vec![],
                body: Box::new(leaf()),
            },
            loc,
        );
        assert_eq!(names(&subroutine), vec![Some("prototype"), Some("signature"), Some("body")]);

        let recovery = Node::new(
            NodeKind::Error {
                message: "bad".into(),
                expected: vec![],
                found: None,
                partial: Some(Box::new(leaf())),
            },
            loc,
        );
        assert_eq!(names(&recovery), vec![Some("partial")]);

        let heredoc = Node::new(
            NodeKind::Heredoc {
                delimiter: "END".into(),
                content: "body".into(),
                interpolated: true,
                indented: false,
                command: false,
                body_span: None,
            },
            loc,
        );
        assert!(names(&heredoc).is_empty());
    }

    #[test]
    fn all_kind_names_is_consistent_with_kind_name() {
        let from_enum = all_kind_names_from_variants();
        let from_const: BTreeSet<&str> = NodeKind::ALL_KIND_NAMES.iter().copied().collect();

        // Check for duplicates in the const array
        assert_eq!(
            NodeKind::ALL_KIND_NAMES.len(),
            from_const.len(),
            "ALL_KIND_NAMES contains duplicates"
        );

        let only_in_enum: Vec<_> = from_enum.difference(&from_const).collect();
        let only_in_const: Vec<_> = from_const.difference(&from_enum).collect();

        assert!(
            only_in_enum.is_empty() && only_in_const.is_empty(),
            "ALL_KIND_NAMES is out of sync with NodeKind variants:\n  \
             in enum but not in ALL_KIND_NAMES: {only_in_enum:?}\n  \
             in ALL_KIND_NAMES but not in enum: {only_in_const:?}"
        );
    }

    #[test]
    fn static_grammar_kind_metadata_matches_sexp_roots() {
        let loc = SourceLocation::new(0, 0);

        for kind in all_node_kinds() {
            if kind.grammar_kind_name_static().is_none() {
                continue;
            }

            let grammar_kind = kind.grammar_kind_name();
            let sexp = Node::new(kind, loc).to_sexp();
            if sexp.starts_with("((") {
                // VariableWithAttributes preserves its child as the outer
                // S-expression form, but still has a stable facade kind.
                assert_eq!(grammar_kind, "variable_with_attributes");
                continue;
            }

            let root = sexp.trim_start_matches('(');
            let end = root.find([' ', ')']).unwrap_or(root.len());
            assert_eq!(grammar_kind, root[..end]);
        }
    }

    #[test]
    fn dynamic_grammar_kind_metadata_matches_sexp_roots() {
        let loc = SourceLocation::new(0, 0);
        let leaf = || Node::new(NodeKind::Number { value: "1".into() }, loc);
        let cases = [
            NodeKind::VariableDeclaration {
                declarator: "my".into(),
                variable: Box::new(leaf()),
                attributes: vec![],
                initializer: None,
            },
            NodeKind::Assignment { lhs: Box::new(leaf()), rhs: Box::new(leaf()), op: "+=".into() },
            NodeKind::Binary { op: "->{}".into(), left: Box::new(leaf()), right: Box::new(leaf()) },
            NodeKind::Unary { op: "!".into(), operand: Box::new(leaf()) },
            NodeKind::String { value: "x".into(), interpolated: true },
            NodeKind::Heredoc {
                delimiter: "END".into(),
                content: String::new(),
                interpolated: true,
                indented: false,
                command: false,
                body_span: None,
            },
            NodeKind::If {
                condition: Box::new(leaf()),
                then_branch: Box::new(leaf()),
                elsif_branches: vec![],
                else_branch: None,
                keyword: Some("unless".into()),
            },
            NodeKind::StatementModifier {
                statement: Box::new(leaf()),
                modifier: "unless".into(),
                condition: Box::new(leaf()),
            },
            NodeKind::Subroutine {
                name: None,
                name_span: None,
                declarator: None,
                prototype: None,
                signature: None,
                attributes: vec![],
                body: Box::new(leaf()),
            },
            NodeKind::LoopControl { op: "next".into(), label: None },
            NodeKind::FunctionCall { name: "print".into(), args: vec![] },
            NodeKind::FunctionCall { name: "custom".into(), args: vec![leaf()] },
            NodeKind::Match {
                expr: Box::new(leaf()),
                pattern: "x".into(),
                modifiers: String::new(),
                has_embedded_code: false,
                negated: true,
            },
            NodeKind::PhaseBlock {
                phase: "BEGIN".into(),
                phase_span: None,
                block: Box::new(leaf()),
            },
        ];

        for kind in cases {
            let grammar_kind = kind.grammar_kind_name();
            let sexp = Node::new(kind, loc).to_sexp();
            let root = sexp.trim_start_matches('(');
            let end = root.find([' ', ')']).unwrap_or(root.len());
            assert_eq!(grammar_kind, root[..end]);
        }
    }

    /// Construct recovery variants and return their `kind_name()` strings.
    ///
    /// Adding a recovery variant to `NodeKind` without updating `RECOVERY_KIND_NAMES`
    /// will cause `recovery_kind_names_is_consistent_with_kind_name` to fail.
    fn recovery_kind_names_from_variants() -> BTreeSet<&'static str> {
        [
            NodeKind::Error {
                message: String::new(),
                expected: vec![],
                found: None,
                partial: None,
            },
            NodeKind::MissingExpression,
            NodeKind::MissingStatement,
            NodeKind::MissingIdentifier,
            NodeKind::MissingBlock,
            NodeKind::UnknownRest,
        ]
        .iter()
        .map(|v| v.kind_name())
        .collect()
    }

    #[test]
    fn recovery_kind_names_is_consistent_with_kind_name() {
        let from_enum = recovery_kind_names_from_variants();
        let from_const: BTreeSet<&str> = NodeKind::RECOVERY_KIND_NAMES.iter().copied().collect();
        let only_in_enum: Vec<_> = from_enum.difference(&from_const).collect();
        let only_in_const: Vec<_> = from_const.difference(&from_enum).collect();
        assert!(
            only_in_enum.is_empty() && only_in_const.is_empty(),
            "RECOVERY_KIND_NAMES is out of sync with recovery variants:\n  \
             in enum but not in RECOVERY_KIND_NAMES: {only_in_enum:?}\n  \
             in RECOVERY_KIND_NAMES but not in enum: {only_in_const:?}"
        );
    }

    #[test]
    fn recovery_kind_names_is_subset_of_all() {
        let all: BTreeSet<&str> = NodeKind::ALL_KIND_NAMES.iter().copied().collect();
        let recovery: BTreeSet<&str> = NodeKind::RECOVERY_KIND_NAMES.iter().copied().collect();

        // No duplicates
        assert_eq!(
            NodeKind::RECOVERY_KIND_NAMES.len(),
            recovery.len(),
            "RECOVERY_KIND_NAMES contains duplicates"
        );

        let not_in_all: Vec<_> = recovery.difference(&all).collect();
        assert!(
            not_in_all.is_empty(),
            "RECOVERY_KIND_NAMES contains entries not in ALL_KIND_NAMES: {not_in_all:?}"
        );
    }

    #[test]
    fn all_kind_names_not_empty() {
        // Regression guard: ALL_KIND_NAMES should always be populated
        assert!(
            !NodeKind::ALL_KIND_NAMES.is_empty(),
            "ALL_KIND_NAMES should not be empty; strum derivation failed"
        );
    }

    #[test]
    fn all_kind_names_no_empty_strings() {
        // Boundary condition: no entry should be an empty string
        for (i, name) in NodeKind::ALL_KIND_NAMES.iter().enumerate() {
            assert!(!name.is_empty(), "ALL_KIND_NAMES[{}] is empty string", i);
        }
    }

    #[test]
    fn all_kind_names_starts_with_program() {
        // Regression guard: first variant is Program (declaration order invariant)
        assert_eq!(
            NodeKind::ALL_KIND_NAMES.first(),
            Some(&"Program"),
            "First variant in ALL_KIND_NAMES should be 'Program' (declaration order)"
        );
    }

    #[test]
    fn all_kind_names_ends_with_unknown_rest() {
        // Regression guard: last variant is UnknownRest (declaration order invariant)
        assert_eq!(
            NodeKind::ALL_KIND_NAMES.last(),
            Some(&"UnknownRest"),
            "Last variant in ALL_KIND_NAMES should be 'UnknownRest' (declaration order)"
        );
    }

    #[test]
    fn all_kind_names_valid_kind_names() {
        // Regression guard: every string in ALL_KIND_NAMES is a valid kind_name() output
        for (i, name) in NodeKind::ALL_KIND_NAMES.iter().enumerate() {
            let found = all_kind_names_from_variants().contains(name);
            assert!(
                found,
                "ALL_KIND_NAMES[{}] = '{}' is not a valid kind_name() return value",
                i, name
            );
        }
    }

    #[test]
    fn all_kind_names_exact_match_with_variants_set() {
        // Regression guard: ALL_KIND_NAMES contains exactly the same names as all variants
        let from_enum = all_kind_names_from_variants();
        let from_const: BTreeSet<&str> = NodeKind::ALL_KIND_NAMES.iter().copied().collect();

        assert_eq!(from_enum, from_const, "ALL_KIND_NAMES set does not match variant kind_names");
    }

    #[test]
    fn all_kind_names_no_whitespace_padding() {
        // Boundary condition: no leading/trailing whitespace in variant names
        for (i, name) in NodeKind::ALL_KIND_NAMES.iter().enumerate() {
            assert_eq!(
                *name,
                name.trim(),
                "ALL_KIND_NAMES[{}] = '{}' has leading/trailing whitespace",
                i,
                name
            );
        }
    }

    #[test]
    fn all_kind_names_count_regression_guard() {
        // Regression guard: ALL_KIND_NAMES must have at least 70 entries.
        // The previous hand-maintained list had 70 variants (including NestedVariableList
        // added in #1457). Failing below that count means a variant was deleted or the
        // strum derivation silently stopped working.
        //
        // Note: the previous test `all_kind_names_strum_derived_stability` asserted
        // `NodeKind::VARIANTS == NodeKind::ALL_KIND_NAMES`, which is trivially true by
        // definition (ALL_KIND_NAMES = NodeKind::VARIANTS). That assertion was vacuous
        // and has been replaced with this count guard.
        assert!(
            NodeKind::ALL_KIND_NAMES.len() >= 70,
            "ALL_KIND_NAMES has only {} entries; expected >= 70. \
             A variant may have been accidentally removed, or strum::VariantNames \
             is not being applied correctly.",
            NodeKind::ALL_KIND_NAMES.len()
        );
    }

    /// Destruction audit over one fully populated representative of every
    /// `NodeKind` variant.
    ///
    /// The mutable traversal that drives detachment and the canonical
    /// read-only traversal must agree on every direct child, including
    /// optional, repeated, pair, and clause fields. The populated fixture
    /// corpus supplies the child-bearing cases, while the immutable traversal
    /// establishes the expected cardinality and exact destruction accounting
    /// catches any detached placeholder that was not created.
    #[test]
    fn every_variant_drains_through_canonical_traversal_parity() {
        for fixture in crate::invariant_policy::node_kind_fixtures() {
            let kind_name = fixture.sample.kind.kind_name().to_string();
            let mut node = fixture.sample;
            let expected = node.count_nodes();

            let mut immutable_kinds = Vec::new();
            node.for_each_child(|child| immutable_kinds.push(child.kind.kind_name()));
            let expected_children = immutable_kinds.len();
            let mut mutable_kinds = Vec::new();
            node.for_each_child_mut(|child| mutable_kinds.push(child.kind.kind_name()));
            assert_eq!(
                mutable_kinds, immutable_kinds,
                "{kind_name}: mutable and immutable traversals disagree on direct children"
            );
            assert_eq!(
                mutable_kinds.len(),
                expected_children,
                "{kind_name}: populated fixture exposed the wrong direct-child cardinality"
            );

            let _ = drop_audit::take_counts();
            drop(node);
            let (_, destroyed) = drop_audit::take_counts();
            assert!(
                destroyed == (expected + expected_children) as u64,
                "{kind_name}: destroyed {destroyed} nodes; populated fixture held {expected} and \
                 detachment created {expected_children} placeholders"
            );
        }
    }

    #[test]
    fn every_variant_clone_preserves_equality_and_kind_clone() {
        for fixture in crate::invariant_policy::node_kind_fixtures() {
            let kind_name = fixture.sample.kind.kind_name().to_string();
            let cloned = fixture.sample.clone();
            assert_eq!(
                fixture.sample, cloned,
                "{kind_name}: Node::clone must preserve public equality"
            );
            let cloned_kind = fixture.sample.kind.clone();
            assert_eq!(
                fixture.sample.kind, cloned_kind,
                "{kind_name}: NodeKind::clone must preserve public equality"
            );
            let mut mutated = cloned;
            mutated.location = SourceLocation::new(
                mutated.location.start(),
                mutated.location.end().saturating_add(17),
            );
            assert_ne!(
                fixture.sample.location, mutated.location,
                "{kind_name}: cloned location must be independent"
            );
            assert_eq!(
                fixture.sample.location.end(),
                0,
                "{kind_name}: mutating the clone must not change the original location"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Depth-guard regression tests (--lib coverage for Codecov/Patch 95)
// ---------------------------------------------------------------------------
//
// These tests verify that `to_sexp` stays iterative (#8832) and that
// exact whole-tree reads (`count_nodes`, `find_deepest_containing_offset`)
// complete iteratively on a pathologically deep input (50 000 levels).
//
// The tree is built iteratively (no recursion in the fixture builder itself),
// so the fixture construction cannot itself overflow the stack.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod depth_guard_tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn loc() -> SourceLocation {
        SourceLocation::new(0, 1)
    }

    /// Build a linearly-nested AST of depth `n` using `ExpressionStatement`
    /// wrappers around a leaf `Number` node.  The resulting chain has `n + 1`
    /// nodes in total.
    ///
    /// Construction is iterative, so this function itself does not recurse.
    fn deep_chain(n: usize) -> Node {
        let mut node = Node::new(NodeKind::Number { value: "1".to_string() }, loc());
        for _ in 0..n {
            node = Node::new(NodeKind::ExpressionStatement { expression: Box::new(node) }, loc());
        }
        node
    }

    // ------------------------------------------------------------------
    // count_nodes
    // ------------------------------------------------------------------

    #[test]
    fn count_nodes_does_not_overflow_on_deep_input() -> TestResult {
        // 50 000 levels deep: the exact iterative walk must return the
        // independently constructed size, not a MAX_AST_DEPTH-truncated count.
        let deep = deep_chain(50_000);
        let count = deep.count_nodes();
        drop(deep);
        assert_eq!(count, 50_001, "exact count must include every wrapper and the leaf");
        Ok(())
    }

    #[test]
    fn count_nodes_exact_on_shallow_input() -> TestResult {
        // Depth-2 chain: ExpressionStatement(Number) — both visible.
        let inner = Node::new(NodeKind::Number { value: "42".to_string() }, loc());
        let outer = Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, loc());
        // Depth guard must not fire: count must be exact.
        assert_eq!(outer.count_nodes(), 2, "shallow chain: ExpressionStatement + Number = 2");
        Ok(())
    }

    // ------------------------------------------------------------------
    // to_sexp
    // ------------------------------------------------------------------

    #[test]
    fn to_sexp_does_not_overflow_on_deep_input() -> TestResult {
        // 50 000 levels deep: the iterative engine must complete without a fake node.
        struct CountingWriter(usize);
        impl std::fmt::Write for CountingWriter {
            fn write_str(&mut self, s: &str) -> std::fmt::Result {
                self.0 = self.0.saturating_add(s.len());
                Ok(())
            }
        }

        let deep = deep_chain(50_000);
        let mut writer = CountingWriter(0);
        match deep.render_debug_sexp(&mut writer, NativeDebugSexpLimits::unbounded()) {
            NativeDebugSexpResult::Complete { work } => {
                assert_eq!(work.nodes_visited, 50_001);
                assert_eq!(work.bytes_written, writer.0);
                assert!(work.bytes_written > 0);
            }
            other => {
                return Err(format!("deep unbounded render must Complete, got {other:?}").into());
            }
        }
        drop(deep);
        Ok(())
    }

    #[test]
    fn to_sexp_nested_calls_do_not_share_renderer_state() -> TestResult {
        struct CountingWriter(usize);
        impl std::fmt::Write for CountingWriter {
            fn write_str(&mut self, s: &str) -> std::fmt::Result {
                self.0 = self.0.saturating_add(s.len());
                Ok(())
            }
        }

        let deep = deep_chain(50_000);
        let mut writer = CountingWriter(0);
        let first = deep.render_debug_sexp(&mut writer, NativeDebugSexpLimits::unbounded());
        assert!(
            matches!(first, NativeDebugSexpResult::Complete { .. }),
            "deep render must Complete before the nested shallow call"
        );
        drop(deep);

        let shallow = Node::new(NodeKind::Number { value: "7".to_string() }, loc());
        let sexp2 = shallow.to_sexp();
        assert_eq!(sexp2, "(number (value 7))");
        assert!(
            !sexp2.contains("depth_limit_exceeded"),
            "nested/sequential renders must be isolated; got: {sexp2}"
        );
        Ok(())
    }

    #[test]
    fn to_sexp_normal_output_on_shallow_input() -> TestResult {
        let inner = Node::new(NodeKind::Number { value: "42".to_string() }, loc());
        let stmt = Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, loc());
        let program = Node::new(NodeKind::Program { statements: vec![stmt] }, loc());
        let sexp = program.to_sexp();
        // Normal output: no truncation marker.
        assert!(!sexp.contains("depth_limit_exceeded"), "shallow tree must not be truncated");
        assert!(sexp.starts_with("(source_file"), "expected source_file wrapper");
        assert!(sexp.contains("number"), "expected number node in output");
        Ok(())
    }

    // ------------------------------------------------------------------
    // find_deepest_containing_offset
    // ------------------------------------------------------------------

    #[test]
    fn find_deepest_containing_offset_does_not_overflow_on_deep_input() -> TestResult {
        // 50 000 levels deep: exact lookup must reach the leaf, not a node
        // frozen at MAX_AST_DEPTH.
        let deep = deep_chain(50_000);
        assert_eq!(
            deep.find_deepest_containing_offset(0).map(|node| node.kind.kind_name()),
            Some("Number"),
            "must return the leaf, not a truncated wrapper"
        );
        drop(deep);
        Ok(())
    }

    #[test]
    fn find_deepest_containing_offset_returns_none_for_out_of_range() -> TestResult {
        // Offset 100 is outside the span (start: 0, end: 1) of every node in the chain.
        let deep = deep_chain(50_000);
        // Assert while `deep` is still borrowed.
        assert!(
            deep.find_deepest_containing_offset(100).is_none(),
            "offset outside root span must return None"
        );
        drop(deep);
        Ok(())
    }

    #[test]
    fn find_deepest_containing_offset_finds_deepest_on_shallow_input() -> TestResult {
        // Build: Program(loc 0..10) → ExpressionStatement(0..10)
        //          → Number "42"(3..5)
        let number_loc = SourceLocation::new(3, 5);
        let stmt_loc = SourceLocation::new(0, 10);

        let number = Node::new(NodeKind::Number { value: "42".to_string() }, number_loc);
        let stmt =
            Node::new(NodeKind::ExpressionStatement { expression: Box::new(number) }, stmt_loc);
        let program = Node::new(NodeKind::Program { statements: vec![stmt] }, stmt_loc);

        // Offset 4 is inside the Number node — deepest match.
        let found = program.find_deepest_containing_offset(4);
        assert!(found.is_some(), "offset 4 is inside Number(3..5)");
        assert_eq!(
            found.map(|n| n.kind.kind_name()),
            Some("Number"),
            "deepest node at offset 4 must be Number"
        );
        Ok(())
    }

    #[test]
    fn observed_child_traversal_stops_source_pulls_on_break() -> TestResult {
        let statements = (0..512)
            .map(|index| {
                Node::new(NodeKind::Number { value: index.to_string() }, SourceLocation::new(0, 0))
            })
            .collect();
        let program = Node::new(NodeKind::Program { statements }, SourceLocation::new(0, 0));
        let mut pulls = 0usize;
        let mut visits = 0usize;

        let result = program.try_for_each_child_with_field_observed(
            |_, _| pulls = pulls.saturating_add(1),
            |_, _| {
                visits = visits.saturating_add(1);
                ControlFlow::Break("stop")
            },
        );

        assert_eq!(result, ControlFlow::Break("stop"));
        assert_eq!(pulls, 1, "the source must not pull later wide-node children after break");
        assert_eq!(visits, 1, "the consumer must receive only the first child");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Iterative deep-tree destruction (#8836), clone (#8837), equality (#8839),
// and debug (#8840)
// ---------------------------------------------------------------------------
//
// `Node` owns its descendants through boxed, optional, repeated, pair-record,
// clause-pair, and recovery fields. Destruction is iterative: a custom `Drop`
// detaches every child into an explicit work stack before each node's own
// fields are dropped, so destructor stack depth no longer grows with tree
// depth.
//
// Clone is likewise iterative: it walks those same canonical child fields,
// clones payloads through a one-level `NodeKind` shell, and rebuilds each
// parent only after cloned children are available. Derived `Clone` glue would
// recurse through `Node`/`NodeKind` and overflow the small-stack harness.
//
// Equality is the third operation on that seam: a custom `PartialEq` compares
// location and derived `NodeKind` payload/shape behind an operation-scoped
// child skip, then walks `for_each_child` on a heap stack of pairs. Starting
// `Node::eq` with unguarded `self.kind == other.kind` re-enters derived
// `NodeKind::eq` and overflows the same 50,000-node 256 KiB harness.
//
// Debug is the fourth: a custom `Debug` sketches kind, range, a bounded
// payload summary, and a bounded child projection on a heap stack. Derived
// recursive `Debug` glue would format the 50,000-node chain by descending
// `Node`/`NodeKind` on the thread stack and abort the process. The same
// harness also proves the rendering stays under [`NODE_DEBUG_MAX_BYTES`] and
// that truncation is visible. Debug bytes are not structural identity:
// two chains that differ only at the hidden leaf compare unequal by `==`
// while their Debug strings match.
//
// The small-stack harness (256 KiB worker threads) discriminates naive
// recursive drop/clone/eq/debug glue from the iterative paths: a 50 000-node
// chain recursively needs multiple megabytes of frames and aborts the process.
// `into_parts` returns the original `NodeKind` payload, so dropping that
// extracted kind must stay stack-safe as well. Direct `NodeKind` equality
// stays derived: child slots route through iterative `Node::eq`. `NodeKind`
// `Debug` does not dump children. A mutation that restores recursive drop
// glue, omits one registered child field from `for_each_child_mut`, drops a
// detached child recursively, clones through `self.kind.clone()` before
// detaching children, compares `Node` by unguarded `self.kind == other.kind`,
// or restores derived recursive `Debug` overflows or fails these same tests.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod deep_tree_destruction_tests {
    use super::node_clone::{CloneObserver, clone_node};
    use super::node_debug::{DebugObserver, render_node};
    use super::node_eq::{EqObserver, nodes_eq};
    use super::*;

    /// Operation-local clone work recorded by [`clone_node`].
    ///
    /// Counts are the clone operations actually performed for one call, not the
    /// depth-bounded [`Node::count_nodes`] population of the result. Lives next
    /// to the tests that read it so field construction is not a production seam.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    struct CloneWork {
        nodes_entered: u64,
        nodes_rebuilt: u64,
        child_edges: u64,
        max_explicit_stack_depth: usize,
    }

    impl CloneObserver for CloneWork {
        fn on_enter(&mut self, child_count: usize) {
            self.nodes_entered = self.nodes_entered.saturating_add(1);
            self.child_edges = self.child_edges.saturating_add(child_count as u64);
        }

        fn on_rebuild(&mut self) {
            self.nodes_rebuilt = self.nodes_rebuilt.saturating_add(1);
        }

        fn on_stack_depth(&mut self, depth: usize) {
            if depth > self.max_explicit_stack_depth {
                self.max_explicit_stack_depth = depth;
            }
        }
    }

    const SMALL_STACK_BYTES: usize = 256 * 1024;
    const DEEP_DEPTH: usize = 50_000;
    const FAMILY_DEPTH: usize = 20_000;
    const MIXED_DEPTH: usize = 24_000;
    const DEEP_CYCLE_DEPTH: usize = 10_000;

    fn loc() -> SourceLocation {
        SourceLocation::new(0, 1)
    }

    fn number_leaf(value: &str) -> Node {
        Node::new(NodeKind::Number { value: value.to_string() }, loc())
    }

    /// Run `body` on a thread whose stack is far smaller than the recursive
    /// drop glue for [`DEEP_DEPTH`] nodes would require.
    fn run_on_small_stack<F>(body: F) -> Result<(), String>
    where
        F: FnOnce() + Send + 'static,
    {
        let handle = std::thread::Builder::new()
            .stack_size(SMALL_STACK_BYTES)
            .spawn(body)
            .map_err(|error| format!("failed to spawn small-stack worker: {error}"))?;
        handle.join().map_err(|_| "small-stack worker aborted (likely stack overflow)".to_string())
    }

    fn chain_of(depth: usize, wrap: fn(Node) -> Node) -> Node {
        let mut node = number_leaf("1");
        for _ in 0..depth {
            node = wrap(node);
        }
        node
    }

    fn wrap_boxed(inner: Node) -> Node {
        Node::new(NodeKind::ExpressionStatement { expression: Box::new(inner) }, loc())
    }

    fn wrap_optional_boxed(inner: Node) -> Node {
        Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(number_leaf("v")),
                attributes: Vec::new(),
                initializer: Some(Box::new(inner)),
            },
            loc(),
        )
    }

    fn wrap_repeated(inner: Node) -> Node {
        Node::new(NodeKind::Program { statements: vec![inner] }, loc())
    }

    fn wrap_pair_record(inner: Node) -> Node {
        Node::new(NodeKind::HashLiteral { pairs: vec![(number_leaf("k"), inner)] }, loc())
    }

    fn wrap_clause_pair(inner: Node) -> Node {
        Node::new(
            NodeKind::If {
                condition: Box::new(number_leaf("c")),
                then_branch: Box::new(number_leaf("t")),
                elsif_branches: vec![(Box::new(number_leaf("e")), Box::new(inner))],
                else_branch: None,
                keyword: None,
            },
            loc(),
        )
    }

    fn wrap_try(inner: Node) -> Node {
        Node::new(
            NodeKind::Try {
                body: Box::new(number_leaf("body")),
                catch_blocks: vec![(Some(("error".to_string(), loc())), Box::new(inner))],
                finally_block: Some(Box::new(number_leaf("finally"))),
            },
            loc(),
        )
    }

    fn wrap_recovery(inner: Node) -> Node {
        Node::new(
            NodeKind::Error {
                message: "recovery".to_string(),
                expected: Vec::new(),
                found: None,
                partial: Some(Box::new(inner)),
            },
            loc(),
        )
    }

    fn wrap_if_else(inner: Node) -> Node {
        Node::new(
            NodeKind::If {
                condition: Box::new(number_leaf("c")),
                then_branch: Box::new(number_leaf("t")),
                elsif_branches: Vec::new(),
                else_branch: Some(Box::new(inner)),
                keyword: None,
            },
            loc(),
        )
    }

    fn all_family_wrappers() -> Vec<(&'static str, fn(Node) -> Node)> {
        vec![
            ("boxed", wrap_boxed),
            ("optional_boxed", wrap_optional_boxed),
            ("repeated", wrap_repeated),
            ("pair_record", wrap_pair_record),
            ("clause_pair", wrap_clause_pair),
            ("try_catch_pair_and_finally", wrap_try),
            ("recovery", wrap_recovery),
        ]
    }

    #[test]
    fn deep_boxed_chain_destroys_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            drop(chain_of(DEEP_DEPTH, wrap_boxed));
        })
    }

    #[test]
    fn into_parts_payload_destroys_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let deep = chain_of(DEEP_DEPTH, wrap_boxed);
            let (kind, _) = deep.into_parts();
            drop(kind);
        })
    }

    #[test]
    fn into_parts_payload_releases_every_node() {
        let _ = drop_audit::take_counts();
        let deep = chain_of(DEEP_CYCLE_DEPTH, wrap_boxed);
        let (kind, _) = deep.into_parts();
        drop(kind);
        let (constructed, destroyed) = drop_audit::take_counts();
        assert_eq!(
            constructed, destroyed,
            "into_parts payload drop constructed {constructed} nodes but destroyed {destroyed}"
        );
        assert!(
            destroyed >= (DEEP_CYCLE_DEPTH + 1) as u64,
            "into_parts payload drop destroyed only {destroyed} of at least {} nodes",
            DEEP_CYCLE_DEPTH + 1
        );
    }

    #[test]
    fn deep_chains_through_every_child_family_destroy_on_small_stack() -> Result<(), String> {
        for (name, wrap) in all_family_wrappers() {
            run_on_small_stack(move || {
                drop(chain_of(FAMILY_DEPTH, wrap));
            })
            .map_err(|error| format!("family {name}: {error}"))?;
        }
        Ok(())
    }

    #[test]
    fn deep_mixed_family_chain_destroys_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let wraps = all_family_wrappers();
            let mut node = number_leaf("1");
            for depth in 0..MIXED_DEPTH {
                let (_, wrap) = wraps[depth % wraps.len()];
                node = wrap(node);
            }
            drop(node);
        })
    }

    #[test]
    fn repeated_deep_cycles_release_every_node_exactly_once() -> Result<(), String> {
        let _ = drop_audit::take_counts();
        for cycle in 0..16u64 {
            let deep = chain_of(DEEP_CYCLE_DEPTH, wrap_boxed);
            drop(deep);
            let (constructed, destroyed) = drop_audit::take_counts();
            assert_eq!(
                constructed, destroyed,
                "cycle {cycle}: constructed {constructed} nodes but destroyed {destroyed}"
            );
            assert!(
                destroyed >= (DEEP_CYCLE_DEPTH + 1) as u64,
                "cycle {cycle}: destroyed only {destroyed} of at least {} nodes",
                DEEP_CYCLE_DEPTH + 1
            );
        }
        Ok(())
    }

    #[test]
    fn repeated_broad_cycles_release_every_node_exactly_once() -> Result<(), String> {
        let _ = drop_audit::take_counts();
        for cycle in 0..32u64 {
            let tree = broad_multi_family_tree(cycle);
            let expected = tree.count_nodes();
            assert!(expected > 10, "fixture must be non-trivial");
            drop(tree);
            let (constructed, destroyed) = drop_audit::take_counts();
            assert_eq!(
                constructed, destroyed,
                "cycle {cycle}: constructed {constructed} nodes but destroyed {destroyed}"
            );
            assert!(
                destroyed >= expected as u64,
                "cycle {cycle}: destroyed {destroyed}, fixture held {expected}"
            );
        }
        Ok(())
    }

    #[test]
    fn unwind_before_drop_leaves_deep_tree_safely_droppable() -> Result<(), String> {
        run_on_small_stack(|| {
            let deep = chain_of(DEEP_DEPTH, wrap_boxed);
            let touched = std::panic::catch_unwind(|| deep.count_nodes() >= 1);
            assert!(touched.is_ok(), "tree must remain readable before the panic");
            let injected =
                std::panic::catch_unwind(|| std::panic::resume_unwind(Box::new("injected")));
            assert!(injected.is_err(), "the injected unwind must be caught before drop");
            // This covers ordinary unwind-before-owner-drop behavior. It does
            // not claim recovery from a panic injected inside `Node::drop`.
            drop(deep);
        })
    }

    /// Broad shallow tree touching every child-field family used above.
    fn broad_multi_family_tree(seed: u64) -> Node {
        let leaf = |n: u64| number_leaf(&(seed * 1000 + n).to_string());
        let try_node = Node::new(
            NodeKind::Try {
                body: Box::new(Node::new(NodeKind::Block { statements: vec![leaf(1)] }, loc())),
                catch_blocks: vec![(
                    Some(("err".to_string(), loc())),
                    Box::new(Node::new(NodeKind::Block { statements: vec![leaf(2)] }, loc())),
                )],
                finally_block: Some(Box::new(leaf(3))),
            },
            loc(),
        );
        let declaration = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(Node::new(
                    NodeKind::Variable { sigil: "$".to_string(), name: seed.to_string() },
                    loc(),
                )),
                attributes: vec![":shared".to_string()],
                initializer: Some(Box::new(leaf(4))),
            },
            loc(),
        );
        Node::new(
            NodeKind::Program {
                statements: vec![
                    try_node,
                    declaration,
                    wrap_clause_pair(leaf(5)),
                    wrap_pair_record(leaf(6)),
                    wrap_recovery(leaf(7)),
                    Node::new(
                        NodeKind::ArrayLiteral { elements: vec![leaf(8), leaf(9), leaf(10)] },
                        loc(),
                    ),
                ],
            },
            loc(),
        )
    }

    #[test]
    fn mutable_traversal_covers_broad_fixture_children() -> Result<(), String> {
        fn count_mutable_tree(node: &mut Node) -> usize {
            let mut count = 1;
            node.for_each_child_mut(|child| count += count_mutable_tree(child));
            count
        }

        let mut tree = broad_multi_family_tree(7);
        let expected = tree.count_nodes();
        let observed = count_mutable_tree(&mut tree);
        assert_eq!(
            observed, expected,
            "mutable child traversal must reach every child in the broad control fixture"
        );
        Ok(())
    }

    fn clone_and_count(node: &Node) -> (Node, CloneWork) {
        let mut work = CloneWork {
            nodes_entered: 0,
            nodes_rebuilt: 0,
            child_edges: 0,
            max_explicit_stack_depth: 0,
        };
        let cloned = clone_node(node, &mut work);
        (cloned, work)
    }

    /// Iterative structure check for clone reconstruction.
    ///
    /// Public [`PartialEq`] is now iterative, so deep clone tests also use
    /// `assert_eq!`. This helper still names kind/location/cardinality/number
    /// fields explicitly so a clone-only reconstruction bug is not hidden
    /// behind a single boolean.
    fn assert_iterative_shape_eq(left: &Node, right: &Node) {
        let mut stack = vec![(left, right)];
        while let Some((left, right)) = stack.pop() {
            assert_eq!(left.kind.kind_name(), right.kind.kind_name(), "cloned kind diverged");
            assert_eq!(left.location, right.location, "cloned location diverged");
            if let (
                NodeKind::Number { value: left_value },
                NodeKind::Number { value: right_value },
            ) = (&left.kind, &right.kind)
            {
                assert_eq!(left_value, right_value, "cloned number payload diverged");
            }
            let mut left_children = Vec::new();
            left.for_each_child(|child| left_children.push(child));
            let mut right_children = Vec::new();
            right.for_each_child(|child| right_children.push(child));
            assert_eq!(
                left_children.len(),
                right_children.len(),
                "cloned child cardinality diverged"
            );
            for (left_child, right_child) in left_children.into_iter().zip(right_children).rev() {
                stack.push((left_child, right_child));
            }
        }
    }

    fn assert_boxed_chain_eq(original: &Node, cloned: &Node, depth: usize) {
        let mut left = original;
        let mut right = cloned;
        for layer in 0..depth {
            match (&left.kind, &right.kind) {
                (
                    NodeKind::ExpressionStatement { expression: left_inner },
                    NodeKind::ExpressionStatement { expression: right_inner },
                ) => {
                    assert_eq!(
                        left.location, right.location,
                        "boxed wrapper location diverged at layer {layer}"
                    );
                    left = left_inner;
                    right = right_inner;
                }
                (left_kind, right_kind) => {
                    assert_eq!(
                        left_kind.kind_name(),
                        "ExpressionStatement",
                        "boxed chain left kind at layer {layer}"
                    );
                    assert_eq!(
                        right_kind.kind_name(),
                        "ExpressionStatement",
                        "boxed chain right kind at layer {layer}"
                    );
                }
            }
        }
        match (&left.kind, &right.kind) {
            (NodeKind::Number { value: left_value }, NodeKind::Number { value: right_value }) => {
                assert_eq!(left.location, right.location, "boxed leaf locations");
                assert_eq!(left_value, right_value, "boxed leaf values");
            }
            (left_kind, right_kind) => {
                assert_eq!(left_kind.kind_name(), "Number", "boxed chain left leaf");
                assert_eq!(right_kind.kind_name(), "Number", "boxed chain right leaf");
            }
        }
    }

    fn spine_child_index(family: &str) -> usize {
        assert!(
            matches!(
                family,
                "boxed"
                    | "repeated"
                    | "recovery"
                    | "optional_boxed"
                    | "pair_record"
                    | "try_catch_pair_and_finally"
                    | "clause_pair"
            ),
            "unknown family {family}"
        );
        match family {
            "optional_boxed" | "pair_record" | "try_catch_pair_and_finally" => 1,
            "clause_pair" => 3,
            _ => 0,
        }
    }

    fn assert_family_chain_eq(family: &str, original: &Node, cloned: &Node, depth: usize) {
        let mut left = original;
        let mut right = cloned;
        let spine = spine_child_index(family);
        for layer in 0..depth {
            assert_eq!(
                left.kind.kind_name(),
                right.kind.kind_name(),
                "{family} kind diverged at layer {layer}"
            );
            assert_eq!(
                left.location, right.location,
                "{family} location diverged at layer {layer}"
            );
            match family {
                "optional_boxed" => match (&left.kind, &right.kind) {
                    (
                        NodeKind::VariableDeclaration {
                            declarator: left_decl,
                            attributes: left_attrs,
                            ..
                        },
                        NodeKind::VariableDeclaration {
                            declarator: right_decl,
                            attributes: right_attrs,
                            ..
                        },
                    ) => {
                        assert_eq!(left_decl, right_decl, "{family} declarator at layer {layer}");
                        assert_eq!(left_attrs, right_attrs, "{family} attributes at layer {layer}");
                    }
                    (left_kind, _) => {
                        assert_eq!(
                            left_kind.kind_name(),
                            "VariableDeclaration",
                            "{family} at layer {layer}"
                        );
                    }
                },
                "clause_pair" => match (&left.kind, &right.kind) {
                    (
                        NodeKind::If { keyword: left_kw, .. },
                        NodeKind::If { keyword: right_kw, .. },
                    ) => {
                        assert_eq!(left_kw, right_kw, "{family} keyword at layer {layer}");
                    }
                    (left_kind, _) => {
                        assert_eq!(left_kind.kind_name(), "If", "{family} at layer {layer}");
                    }
                },
                "try_catch_pair_and_finally" => match (&left.kind, &right.kind) {
                    (
                        NodeKind::Try { catch_blocks: left_catches, .. },
                        NodeKind::Try { catch_blocks: right_catches, .. },
                    ) => {
                        assert_eq!(
                            left_catches.len(),
                            right_catches.len(),
                            "{family} catch count at layer {layer}"
                        );
                        let left_binding =
                            left_catches.first().and_then(|(binding, _)| binding.as_ref());
                        let right_binding =
                            right_catches.first().and_then(|(binding, _)| binding.as_ref());
                        assert_eq!(
                            left_binding, right_binding,
                            "{family} catch binding at layer {layer}"
                        );
                    }
                    (left_kind, _) => {
                        assert_eq!(left_kind.kind_name(), "Try", "{family} at layer {layer}");
                    }
                },
                "recovery" => match (&left.kind, &right.kind) {
                    (
                        NodeKind::Error {
                            message: left_message,
                            expected: left_expected,
                            found: left_found,
                            ..
                        },
                        NodeKind::Error {
                            message: right_message,
                            expected: right_expected,
                            found: right_found,
                            ..
                        },
                    ) => {
                        assert_eq!(
                            left_message, right_message,
                            "{family} message at layer {layer}"
                        );
                        assert_eq!(
                            left_expected, right_expected,
                            "{family} expected tokens at layer {layer}"
                        );
                        assert_eq!(
                            left_found, right_found,
                            "{family} found token at layer {layer}"
                        );
                    }
                    (left_kind, _) => {
                        assert_eq!(left_kind.kind_name(), "Error", "{family} at layer {layer}");
                    }
                },
                _ => {}
            }

            let mut left_children = Vec::new();
            left.for_each_child(|child| left_children.push(child));
            let mut right_children = Vec::new();
            right.for_each_child(|child| right_children.push(child));
            assert_eq!(
                left_children.len(),
                right_children.len(),
                "{family} child count diverged at layer {layer}"
            );
            assert!(
                spine < left_children.len(),
                "{family} spine index {spine} out of range at layer {layer}"
            );
            for (index, (left_child, right_child)) in
                left_children.iter().zip(right_children.iter()).enumerate()
            {
                if index == spine {
                    continue;
                }
                assert_eq!(
                    *left_child, *right_child,
                    "{family} non-spine child {index} diverged at layer {layer}"
                );
            }
            left = left_children[spine];
            right = right_children[spine];
        }
        assert_eq!(left, right, "{family} leaf diverged");
    }

    #[test]
    fn deep_boxed_chain_clones_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let original = chain_of(DEEP_DEPTH, wrap_boxed);
            let cloned = original.clone();
            let (counted, work) = clone_and_count(&original);
            assert_eq!(work.nodes_entered, (DEEP_DEPTH + 1) as u64);
            assert_eq!(work.nodes_rebuilt, (DEEP_DEPTH + 1) as u64);
            assert_eq!(work.child_edges, DEEP_DEPTH as u64);
            assert!(
                work.max_explicit_stack_depth >= DEEP_DEPTH,
                "explicit clone stack must grow with chain depth, got {}",
                work.max_explicit_stack_depth
            );
            let truncated_population = match cloned
                .count_nodes_bounded(AstReadLimits::max_depth(MAX_AST_DEPTH))
            {
                AstReadResult::Truncated { partial, .. } => partial as u64,
                other => {
                    assert!(
                        matches!(other, AstReadResult::Truncated { .. }),
                        "50k clone fixture must still truncate a depth-512 bounded read, got {other:?}"
                    );
                    0
                }
            };
            assert!(
                truncated_population < work.nodes_rebuilt,
                "clone work must record performed rebuilds ({}) rather than a depth-512 truncated read ({truncated_population})",
                work.nodes_rebuilt
            );
            assert_boxed_chain_eq(&original, &cloned, DEEP_DEPTH);
            assert_boxed_chain_eq(&original, &counted, DEEP_DEPTH);
            drop(counted);
            drop(cloned);
            drop(original);
        })
    }

    #[test]
    fn deep_boxed_chain_kind_clones_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let original = chain_of(DEEP_DEPTH, wrap_boxed);
            let cloned_kind = original.kind.clone();
            let cloned = Node::new(cloned_kind, original.location);
            assert_boxed_chain_eq(&original, &cloned, DEEP_DEPTH);
            drop(cloned);
            drop(original);
        })
    }

    #[test]
    fn cloned_deep_tree_is_independent() -> Result<(), String> {
        run_on_small_stack(|| {
            let original = chain_of(DEEP_DEPTH, wrap_boxed);
            let mut cloned = original.clone();
            let mut cursor = &mut cloned;
            for _ in 0..DEEP_DEPTH {
                let kind_name = cursor.kind.kind_name();
                let NodeKind::ExpressionStatement { expression } = &mut cursor.kind else {
                    assert_eq!(kind_name, "ExpressionStatement");
                    return;
                };
                cursor = expression;
            }
            let leaf_name = cursor.kind.kind_name();
            let NodeKind::Number { value } = &mut cursor.kind else {
                assert_eq!(leaf_name, "Number");
                return;
            };
            value.push_str("-cloned");

            let mut original_cursor = &original;
            for _ in 0..DEEP_DEPTH {
                let kind_name = original_cursor.kind.kind_name();
                let NodeKind::ExpressionStatement { expression } = &original_cursor.kind else {
                    assert_eq!(kind_name, "ExpressionStatement");
                    return;
                };
                original_cursor = expression;
            }
            match &original_cursor.kind {
                NodeKind::Number { value } => {
                    assert_eq!(value, "1", "mutating the clone must not change the original leaf");
                }
                other => {
                    assert_eq!(other.kind_name(), "Number");
                }
            }
            drop(cloned);
            drop(original);
        })
    }

    #[test]
    fn deep_chains_through_every_child_family_clone_on_small_stack() -> Result<(), String> {
        for (name, wrap) in all_family_wrappers() {
            run_on_small_stack(move || {
                let original = chain_of(FAMILY_DEPTH, wrap);
                let cloned = original.clone();
                let (counted, work) = clone_and_count(&original);
                let children_per_layer = original.child_count() as u64;
                let expected_nodes =
                    (FAMILY_DEPTH as u64).saturating_mul(children_per_layer).saturating_add(1);
                let expected_edges = (FAMILY_DEPTH as u64).saturating_mul(children_per_layer);
                assert_eq!(work.nodes_entered, expected_nodes, "family {name}: entered nodes");
                assert_eq!(work.nodes_rebuilt, expected_nodes, "family {name}: rebuilt nodes");
                assert_eq!(work.child_edges, expected_edges, "family {name}: child edges");
                assert_family_chain_eq(name, &original, &cloned, FAMILY_DEPTH);
                assert_family_chain_eq(name, &original, &counted, FAMILY_DEPTH);
                drop(counted);
                drop(cloned);
                drop(original);
            })
            .map_err(|error| format!("family {name}: {error}"))?;
        }
        Ok(())
    }

    #[test]
    fn child_shapes_clone_with_exact_structure() {
        for (name, wrap) in all_family_wrappers() {
            let original = chain_of(3, wrap);
            let cloned = original.clone();
            assert_eq!(
                original, cloned,
                "{name} shallow family clone must preserve public equality"
            );
        }

        let loc = loc();
        let first =
            Node::new(NodeKind::Number { value: "same".to_string() }, SourceLocation::new(0, 1));
        let second =
            Node::new(NodeKind::Number { value: "same".to_string() }, SourceLocation::new(4, 5));
        let array = Node::new(NodeKind::ArrayLiteral { elements: vec![first, second] }, loc);
        let cloned_array = array.clone();
        match (&array.kind, &cloned_array.kind) {
            (
                NodeKind::ArrayLiteral { elements: original_elements },
                NodeKind::ArrayLiteral { elements: cloned_elements },
            ) => {
                assert_eq!(original_elements.len(), 2);
                assert_eq!(cloned_elements.len(), 2);
                assert_eq!(cloned_elements[0].location.start(), 0);
                assert_eq!(cloned_elements[1].location.start(), 4);
                assert_ne!(
                    cloned_elements[0].location, cloned_elements[1].location,
                    "equal-looking repeated children must keep source order"
                );
            }
            (left_kind, _) => {
                assert_eq!(left_kind.kind_name(), "ArrayLiteral");
            }
        }

        let present = wrap_optional_boxed(number_leaf("init"));
        let absent = Node::new(
            NodeKind::VariableDeclaration {
                declarator: "my".to_string(),
                variable: Box::new(number_leaf("v")),
                attributes: Vec::new(),
                initializer: None,
            },
            loc,
        );
        assert_eq!(present.clone(), present);
        assert_eq!(absent.clone(), absent);
        assert_ne!(present, absent, "optional child presence is part of clone equality");

        let empty = Node::new(NodeKind::Program { statements: vec![] }, loc);
        let one = wrap_repeated(number_leaf("1"));
        let many = Node::new(
            NodeKind::Program {
                statements: vec![number_leaf("1"), number_leaf("2"), number_leaf("3")],
            },
            loc,
        );
        assert_eq!(empty.clone(), empty);
        assert_eq!(one.clone(), one);
        assert_eq!(many.clone(), many);
        let cloned_many = many.clone();
        match &cloned_many.kind {
            NodeKind::Program { statements } => {
                assert_eq!(statements[0].kind.kind_name(), "Number");
                match (&statements[0].kind, &statements[2].kind) {
                    (NodeKind::Number { value: first }, NodeKind::Number { value: last }) => {
                        assert_eq!(first, "1");
                        assert_eq!(last, "3");
                    }
                    (left_kind, _) => {
                        assert_eq!(left_kind.kind_name(), "Number");
                    }
                }
            }
            other => {
                assert_eq!(other.kind_name(), "Program");
            }
        }
    }

    #[test]
    fn clone_work_is_operation_local_and_concurrent() -> Result<(), String> {
        let tree = broad_multi_family_tree(3);
        let (_, expected) = clone_and_count(&tree);

        std::thread::scope(|scope| {
            let first = scope.spawn(|| clone_and_count(&tree));
            let second = scope.spawn(|| clone_and_count(&tree));
            let (first_clone, first_work) =
                first.join().map_err(|_| "first clone thread aborted".to_string())?;
            let (second_clone, second_work) =
                second.join().map_err(|_| "second clone thread aborted".to_string())?;
            assert_eq!(first_work, expected);
            assert_eq!(second_work, expected);
            assert_eq!(first_clone, tree);
            assert_eq!(second_clone, tree);
            Ok(())
        })
    }

    #[test]
    fn observer_panic_during_clone_is_stack_safe_and_leaves_original() -> Result<(), String> {
        struct PanicAfter {
            inner: CloneWork,
            remaining_rebuilds: u64,
        }

        impl CloneObserver for PanicAfter {
            fn on_enter(&mut self, child_count: usize) {
                self.inner.on_enter(child_count);
            }

            fn on_rebuild(&mut self) {
                self.inner.on_rebuild();
                self.remaining_rebuilds = self.remaining_rebuilds.saturating_sub(1);
                if self.remaining_rebuilds == 0 {
                    std::panic::resume_unwind(Box::new("clone observer panic"));
                }
            }

            fn on_stack_depth(&mut self, depth: usize) {
                self.inner.on_stack_depth(depth);
            }
        }

        run_on_small_stack(|| {
            let original = chain_of(DEEP_DEPTH, wrap_boxed);
            let mut observer = PanicAfter { inner: CloneWork::default(), remaining_rebuilds: 8 };
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _cloned = clone_node(&original, &mut observer);
            }));
            assert!(panicked.is_err(), "observer panic must unwind");
            let (cloned, work) = clone_and_count(&original);
            assert_eq!(work.nodes_rebuilt, (DEEP_DEPTH + 1) as u64);
            assert_boxed_chain_eq(&original, &cloned, DEEP_DEPTH);
            drop(cloned);
            drop(original);
        })
    }

    #[test]
    fn nested_payload_clone_is_independent() {
        let original = wrap_try(number_leaf("caught"));
        let mut cloned = original.clone();
        match &mut cloned.kind {
            NodeKind::Try { catch_blocks, finally_block, .. } => {
                if let Some((binding, _)) = catch_blocks.first_mut()
                    && let Some((name, _)) = binding.as_mut()
                {
                    name.push_str("-mutated");
                }
                if let Some(finally) = finally_block.as_mut()
                    && let NodeKind::Number { value } = &mut finally.kind
                {
                    value.push_str("-mutated");
                }
            }
            other => {
                assert_eq!(other.kind_name(), "Try");
            }
        }
        match &original.kind {
            NodeKind::Try { catch_blocks, finally_block, .. } => {
                let name = catch_blocks
                    .first()
                    .and_then(|(binding, _)| binding.as_ref())
                    .map(|(name, _)| name.as_str());
                assert_eq!(name, Some("error"));
                match finally_block.as_ref().map(|node| &node.kind) {
                    Some(NodeKind::Number { value }) => assert_eq!(value, "finally"),
                    other => {
                        assert!(
                            matches!(other, Some(NodeKind::Number { .. })),
                            "expected Number finally, got {other:?}"
                        );
                    }
                }
            }
            other => {
                assert_eq!(other.kind_name(), "Try");
            }
        }
    }

    #[test]
    fn deep_mixed_family_chain_clones_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let wraps = all_family_wrappers();
            let mut node = number_leaf("1");
            for depth in 0..MIXED_DEPTH {
                let (_, wrap) = wraps[depth % wraps.len()];
                node = wrap(node);
            }
            let original = node;
            let cloned = original.clone();
            let (counted, work) = clone_and_count(&original);
            assert!(
                work.nodes_rebuilt > MAX_AST_DEPTH as u64,
                "mixed clone work must exceed the depth-bounded population counter"
            );
            assert_iterative_shape_eq(&original, &cloned);
            assert_iterative_shape_eq(&original, &counted);
            assert_eq!(original, cloned);
            assert_eq!(original, counted);
            drop(counted);
            drop(cloned);
            drop(original);
        })
    }

    #[test]
    fn wide_repeated_children_clone_preserves_order_and_work() {
        const WIDTH: usize = 128;
        let elements: Vec<Node> = (0..WIDTH)
            .map(|index| {
                Node::new(
                    NodeKind::Number { value: index.to_string() },
                    SourceLocation::new(index, index + 1),
                )
            })
            .collect();
        let original = Node::new(NodeKind::Program { statements: elements }, loc());
        let cloned = original.clone();
        let (counted, work) = clone_and_count(&original);
        assert_eq!(work.nodes_entered, (WIDTH + 1) as u64);
        assert_eq!(work.nodes_rebuilt, (WIDTH + 1) as u64);
        assert_eq!(work.child_edges, WIDTH as u64);
        assert!(
            work.max_explicit_stack_depth >= WIDTH,
            "wide clone stack must hold every child frame, got {}",
            work.max_explicit_stack_depth
        );
        assert_eq!(original, cloned);
        assert_eq!(original, counted);
        match &cloned.kind {
            NodeKind::Program { statements } => {
                assert_eq!(statements.len(), WIDTH);
                match &statements[0].kind {
                    NodeKind::Number { value } => assert_eq!(value, "0"),
                    other => assert_eq!(other.kind_name(), "Number"),
                }
                match &statements[WIDTH - 1].kind {
                    NodeKind::Number { value } => assert_eq!(value, &(WIDTH - 1).to_string()),
                    other => assert_eq!(other.kind_name(), "Number"),
                }
                assert_eq!(statements[1].location.start(), 1);
                assert_ne!(
                    statements[0].kind.kind_name(),
                    "Ellipsis",
                    "install must replace shell placeholders"
                );
            }
            other => assert_eq!(other.kind_name(), "Program"),
        }
    }

    #[test]
    fn leaf_and_ellipsis_clone_is_not_a_placeholder() {
        let leaf = number_leaf("7");
        let (cloned_leaf, leaf_work) = clone_and_count(&leaf);
        assert_eq!(leaf_work.nodes_entered, 1);
        assert_eq!(leaf_work.nodes_rebuilt, 1);
        assert_eq!(leaf_work.child_edges, 0);
        assert_eq!(leaf, cloned_leaf);

        let ellipsis = Node::new(NodeKind::Ellipsis, SourceLocation::new(10, 13));
        let cloned_ellipsis = ellipsis.clone();
        assert_eq!(ellipsis, cloned_ellipsis);
        assert_eq!(cloned_ellipsis.location.start(), 10);
        assert_eq!(cloned_ellipsis.kind.kind_name(), "Ellipsis");

        let _ = ellipsis.clone();
        assert_eq!(
            number_leaf("1").clone(),
            number_leaf("1"),
            "payload-shell flag must not leak into a later public clone"
        );
    }

    #[test]
    fn sibling_and_else_branch_clone_edges() {
        let original = Node::new(
            NodeKind::Program {
                statements: vec![number_leaf("a"), number_leaf("b"), number_leaf("c")],
            },
            loc(),
        );
        let mut cloned = original.clone();
        match &mut cloned.kind {
            NodeKind::Program { statements } => match &mut statements[1].kind {
                NodeKind::Number { value } => value.push_str("-mut"),
                other => assert_eq!(other.kind_name(), "Number"),
            },
            other => assert_eq!(other.kind_name(), "Program"),
        }
        match &original.kind {
            NodeKind::Program { statements } => match &statements[1].kind {
                NodeKind::Number { value } => assert_eq!(value, "b"),
                other => assert_eq!(other.kind_name(), "Number"),
            },
            other => assert_eq!(other.kind_name(), "Program"),
        }

        let with_else = wrap_if_else(number_leaf("e"));
        let without_else = Node::new(
            NodeKind::If {
                condition: Box::new(number_leaf("c")),
                then_branch: Box::new(number_leaf("t")),
                elsif_branches: Vec::new(),
                else_branch: None,
                keyword: None,
            },
            loc(),
        );
        assert_eq!(with_else.clone(), with_else);
        assert_eq!(without_else.clone(), without_else);
        assert_ne!(with_else, without_else);
        assert_eq!(with_else.child_count(), 3);
        assert_eq!(without_else.child_count(), 2);
        let (cloned_else, else_work) = clone_and_count(&with_else);
        assert_eq!(else_work.child_edges, 3);
        assert_eq!(cloned_else, with_else);
    }

    #[test]
    fn cloned_tree_releases_every_node() {
        let _ = drop_audit::take_counts();
        let original = chain_of(DEEP_CYCLE_DEPTH, wrap_boxed);
        let cloned = original.clone();
        drop(cloned);
        drop(original);
        let (constructed, destroyed) = drop_audit::take_counts();
        assert_eq!(
            constructed, destroyed,
            "clone+drop constructed {constructed} nodes but destroyed {destroyed}"
        );
        assert!(
            destroyed >= (DEEP_CYCLE_DEPTH + 1) as u64,
            "clone+drop destroyed only {destroyed} of at least {} original nodes",
            DEEP_CYCLE_DEPTH + 1
        );
    }

    #[test]
    fn pair_and_clause_lists_clone_preserve_order() {
        let numbered = |value: &str, start: usize| {
            Node::new(
                NodeKind::Number { value: value.to_string() },
                SourceLocation::new(start, start + 1),
            )
        };
        let hash = Node::new(
            NodeKind::HashLiteral {
                pairs: vec![
                    (numbered("k0", 0), numbered("v0", 1)),
                    (numbered("k1", 2), numbered("v1", 3)),
                ],
            },
            loc(),
        );
        let cloned_hash = hash.clone();
        match &cloned_hash.kind {
            NodeKind::HashLiteral { pairs } => {
                assert_eq!(pairs.len(), 2);
                match (&pairs[0].0.kind, &pairs[0].1.kind) {
                    (NodeKind::Number { value: key }, NodeKind::Number { value }) => {
                        assert_eq!(key, "k0");
                        assert_eq!(value, "v0");
                    }
                    (left, _) => assert_eq!(left.kind_name(), "Number"),
                }
                match (&pairs[1].0.kind, &pairs[1].1.kind) {
                    (NodeKind::Number { value: key }, NodeKind::Number { value }) => {
                        assert_eq!(key, "k1");
                        assert_eq!(value, "v1");
                    }
                    (left, _) => assert_eq!(left.kind_name(), "Number"),
                }
            }
            other => assert_eq!(other.kind_name(), "HashLiteral"),
        }
        assert_eq!(hash, cloned_hash);
        let (_, hash_work) = clone_and_count(&hash);
        assert_eq!(hash_work.nodes_entered, 5);
        assert_eq!(hash_work.nodes_rebuilt, 5);
        assert_eq!(hash_work.child_edges, 4);

        let two_elsif = Node::new(
            NodeKind::If {
                condition: Box::new(number_leaf("c")),
                then_branch: Box::new(number_leaf("t")),
                elsif_branches: vec![
                    (Box::new(number_leaf("e0")), Box::new(number_leaf("b0"))),
                    (Box::new(number_leaf("e1")), Box::new(number_leaf("b1"))),
                ],
                else_branch: None,
                keyword: None,
            },
            loc(),
        );
        let cloned_if = two_elsif.clone();
        match &cloned_if.kind {
            NodeKind::If { elsif_branches, .. } => {
                assert_eq!(elsif_branches.len(), 2);
                match &elsif_branches[0].0.kind {
                    NodeKind::Number { value } => assert_eq!(value, "e0"),
                    other => assert_eq!(other.kind_name(), "Number"),
                }
                match &elsif_branches[1].0.kind {
                    NodeKind::Number { value } => assert_eq!(value, "e1"),
                    other => assert_eq!(other.kind_name(), "Number"),
                }
            }
            other => assert_eq!(other.kind_name(), "If"),
        }
        assert_eq!(two_elsif, cloned_if);
        let (_, if_work) = clone_and_count(&two_elsif);
        assert_eq!(if_work.child_edges, 6);
        assert_eq!(if_work.nodes_rebuilt, 7);
    }

    struct EqWork {
        nodes_entered: u64,
        max_explicit_stack_depth: usize,
    }

    impl EqObserver for EqWork {
        fn on_enter(&mut self) {
            self.nodes_entered = self.nodes_entered.saturating_add(1);
        }

        fn on_stack_depth(&mut self, depth: usize) {
            if depth > self.max_explicit_stack_depth {
                self.max_explicit_stack_depth = depth;
            }
        }
    }

    fn compare_and_count(left: &Node, right: &Node) -> (bool, EqWork) {
        let mut work = EqWork { nodes_entered: 0, max_explicit_stack_depth: 0 };
        let equal = nodes_eq(left, right, &mut work);
        (equal, work)
    }

    fn chain_of_value(depth: usize, wrap: fn(Node) -> Node, value: &str) -> Node {
        let mut node = number_leaf(value);
        for _ in 0..depth {
            node = wrap(node);
        }
        node
    }

    #[test]
    fn deep_boxed_chain_equals_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let left = chain_of(DEEP_DEPTH, wrap_boxed);
            let right = chain_of(DEEP_DEPTH, wrap_boxed);
            let (equal, work) = compare_and_count(&left, &right);
            assert!(equal, "independent equal 50k chains must compare equal");
            assert_eq!(left, right);
            assert_eq!(work.nodes_entered, (DEEP_DEPTH + 1) as u64);
            assert!(
                work.max_explicit_stack_depth >= 1,
                "equal compare must use the explicit stack"
            );
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn deep_boxed_chain_deepest_leaf_differs_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let left = chain_of(DEEP_DEPTH, wrap_boxed);
            let right = chain_of_value(DEEP_DEPTH, wrap_boxed, "1-neq");
            let (equal, work) = compare_and_count(&left, &right);
            assert!(!equal, "deepest leaf payload must be material");
            assert_ne!(left, right);
            assert_eq!(
                work.nodes_entered,
                (DEEP_DEPTH + 1) as u64,
                "leaf mismatch must still visit every ancestor exactly once"
            );
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn deep_boxed_chain_kind_equals_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let left = chain_of(DEEP_DEPTH, wrap_boxed);
            let right = chain_of(DEEP_DEPTH, wrap_boxed);
            assert_eq!(left.kind, right.kind, "derived NodeKind eq must route through Node::eq");
            let cloned_kind = left.kind.clone();
            assert_eq!(cloned_kind, right.kind);
            drop(cloned_kind);
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn deep_boxed_chain_kind_differs_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let left = chain_of(DEEP_DEPTH, wrap_boxed);
            let right = chain_of_value(DEEP_DEPTH, wrap_boxed, "1-neq");
            assert_ne!(
                left.kind, right.kind,
                "derived NodeKind inequality must route through iterative Node::eq"
            );
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn deep_root_location_mismatch_does_not_walk_the_chain() -> Result<(), String> {
        run_on_small_stack(|| {
            let left = chain_of(DEEP_DEPTH, wrap_boxed);
            let mut right = chain_of(DEEP_DEPTH, wrap_boxed);
            right.location = SourceLocation::new(9, 10);
            let (equal, work) = compare_and_count(&left, &right);
            assert!(!equal);
            assert_eq!(work.nodes_entered, 1, "root mismatch must not visit descendants");
            assert_ne!(left, right);
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn deep_chains_through_every_child_family_compare_on_small_stack() -> Result<(), String> {
        for (name, wrap) in all_family_wrappers() {
            run_on_small_stack(move || {
                let left = chain_of(FAMILY_DEPTH, wrap);
                let right = chain_of(FAMILY_DEPTH, wrap);
                assert_eq!(left, right, "family {name}: equal chains");
                assert_eq!(left.kind, right.kind, "family {name}: NodeKind eq");
                let cloned = left.clone();
                assert_eq!(left, cloned, "family {name}: clone then eq");
                drop(cloned);
                drop(right);
                drop(left);
            })
            .map_err(|error| format!("family {name}: equal: {error}"))?;
            run_on_small_stack(move || {
                let left = chain_of(FAMILY_DEPTH, wrap);
                let right = chain_of_value(FAMILY_DEPTH, wrap, "1-neq");
                assert_ne!(left, right, "family {name}: deepest leaf must be material");
                assert_ne!(left.kind, right.kind, "family {name}: NodeKind leaf must be material");
                drop(right);
                drop(left);
            })
            .map_err(|error| format!("family {name}: unequal: {error}"))?;
        }
        Ok(())
    }

    #[test]
    fn deep_mixed_family_chain_compares_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let wraps = all_family_wrappers();
            let mut left = number_leaf("1");
            let mut right = number_leaf("1");
            for depth in 0..MIXED_DEPTH {
                let (_, wrap) = wraps[depth % wraps.len()];
                left = wrap(left);
                right = wrap(right);
            }
            assert_eq!(left, right);
            let cloned = left.clone();
            assert_eq!(left, cloned);
            drop(cloned);
            drop(right);
            drop(left);
        })?;
        run_on_small_stack(|| {
            let wraps = all_family_wrappers();
            let mut left = number_leaf("1");
            let mut right = number_leaf("1-neq");
            for depth in 0..MIXED_DEPTH {
                let (_, wrap) = wraps[depth % wraps.len()];
                left = wrap(left);
                right = wrap(right);
            }
            assert_ne!(left, right, "mixed-family deepest leaf must be material");
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn into_parts_kind_equals_original_kind_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let original = chain_of(DEEP_DEPTH, wrap_boxed);
            let expected_kind = original.kind.clone();
            let (kind, _) = original.into_parts();
            assert_eq!(kind, expected_kind);
            drop(kind);
            drop(expected_kind);
        })
    }

    struct DebugWork {
        nodes_entered: u64,
        max_explicit_stack_depth: usize,
    }

    impl DebugObserver for DebugWork {
        fn on_enter(&mut self) {
            self.nodes_entered = self.nodes_entered.saturating_add(1);
        }

        fn on_stack_depth(&mut self, depth: usize) {
            if depth > self.max_explicit_stack_depth {
                self.max_explicit_stack_depth = depth;
            }
        }
    }

    fn debug_and_count(node: &Node) -> (String, DebugWork) {
        let mut work = DebugWork { nodes_entered: 0, max_explicit_stack_depth: 0 };
        let rendered = render_node(node, &mut work);
        (rendered, work)
    }

    #[test]
    fn deep_boxed_chain_debug_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let node = chain_of(DEEP_DEPTH, wrap_boxed);
            let (rendered, work) = debug_and_count(&node);
            assert!(rendered.contains("ExpressionStatement"), "rendered = {rendered:?}");
            assert!(
                rendered.contains(NODE_DEBUG_TRUNCATION_MARKER),
                "truncation must be visible: {rendered:?}"
            );
            assert!(
                rendered.len() <= NODE_DEBUG_MAX_BYTES,
                "debug len {} exceeds bound {}",
                rendered.len(),
                NODE_DEBUG_MAX_BYTES
            );
            assert!(!rendered.contains("location: SourceLocation"), "rendered = {rendered:?}");
            assert!(work.nodes_entered > 0);
            assert!(work.nodes_entered <= NODE_DEBUG_MAX_NODES as u64);
            assert!(work.max_explicit_stack_depth >= 1, "debug must use the explicit stack");
            drop(node);
        })
    }

    #[test]
    fn deep_boxed_chain_debug_is_not_identity_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let left = chain_of(DEEP_DEPTH, wrap_boxed);
            let right = chain_of_value(DEEP_DEPTH, wrap_boxed, "1-neq");
            assert_ne!(left, right, "PartialEq must still see the hidden leaf");
            let left_dbg = format!("{left:?}");
            let right_dbg = format!("{right:?}");
            assert_eq!(left_dbg, right_dbg, "truncated Debug must not be an equality oracle");
            assert!(left_dbg.contains(NODE_DEBUG_TRUNCATION_MARKER), "left = {left_dbg:?}");
            assert!(left_dbg.len() <= NODE_DEBUG_MAX_BYTES);
            let kind_dbg = format!("{:?}", left.kind);
            assert!(
                !kind_dbg.contains("ExpressionStatement @"),
                "NodeKind Debug must not dump the child tree: {kind_dbg:?}"
            );
            assert!(kind_dbg.len() <= NODE_DEBUG_MAX_BYTES, "kind debug len={}", kind_dbg.len());
            drop(right);
            drop(left);
        })
    }

    #[test]
    fn deep_chains_through_every_child_family_debug_on_small_stack() -> Result<(), String> {
        for (name, wrap) in all_family_wrappers() {
            run_on_small_stack(move || {
                let node = chain_of(FAMILY_DEPTH, wrap);
                let rendered = format!("{node:?}");
                assert!(
                    rendered.contains(NODE_DEBUG_TRUNCATION_MARKER),
                    "family {name}: truncation missing: {rendered:?}"
                );
                assert!(
                    rendered.len() <= NODE_DEBUG_MAX_BYTES,
                    "family {name}: debug len {}",
                    rendered.len()
                );
                drop(node);
            })
            .map_err(|error| format!("family {name}: {error}"))?;
        }
        Ok(())
    }

    #[test]
    fn deep_mixed_family_chain_debug_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let wraps = all_family_wrappers();
            let mut node = number_leaf("1");
            for depth in 0..MIXED_DEPTH {
                let (_, wrap) = wraps[depth % wraps.len()];
                node = wrap(node);
            }
            let rendered = format!("{node:?}");
            assert!(rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
            assert!(rendered.len() <= NODE_DEBUG_MAX_BYTES, "len={}", rendered.len());
            drop(node);
        })
    }

    #[test]
    fn wide_program_debug_stays_bounded_on_small_stack() -> Result<(), String> {
        run_on_small_stack(|| {
            let statements: Vec<Node> = (0..10_000).map(|i| number_leaf(&i.to_string())).collect();
            let node = Node::new(NodeKind::Program { statements }, loc());
            let rendered = format!("{node:?}");
            assert!(rendered.contains("Program"), "rendered = {rendered:?}");
            assert!(rendered.contains("... +"), "rendered = {rendered:?}");
            assert!(rendered.contains(NODE_DEBUG_TRUNCATION_MARKER), "rendered = {rendered:?}");
            assert!(rendered.len() <= NODE_DEBUG_MAX_BYTES, "len={}", rendered.len());
            drop(node);
        })
    }
}
