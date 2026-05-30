//! Static classification metadata for [`NodeKind`] variants.
//!
//! This module answers the question **"what kind of node is this?"** at the
//! variant level, without looking at the node's position in the tree or at
//! the source text it covers. Consumers that need positional facts (is this
//! inside a heredoc body? inside POD? after `__DATA__`?) must layer those
//! checks on top using their own positional knowledge.
//!
//! # Design contract
//!
//! - **Variant-level only.** Every flag is determined by the `NodeKind`
//!   discriminant alone. The same `NodeKind::Heredoc { .. }` always returns
//!   the same flags regardless of where it appears in the AST.
//!
//! - **No wildcard arms.** Both `category()` and `flags()` use exhaustive
//!   `match self { ... }` expressions with no `_ =>` catch-all arm. Adding a
//!   new `NodeKind` variant is a compile error until the match is extended.
//!   This is the **drift guard**: consumers that pattern-match `NodeKind` can
//!   rely on classification staying current.
//!
//! - **`safe_for_breakpoint` semantics.** This flag answers the question
//!   *"can a breakpoint ever be set on this kind of node?"* at the variant
//!   level. A `true` value must be AND-ed by the DAP/LSP consumer with
//!   positional checks (e.g., is the cursor inside a heredoc body? Is the
//!   line inside a POD block?). The flag never means "always stop here" —
//!   it means "this kind of node is a valid candidate for breakpoint
//!   placement, pending positional validation."
//!
//! - **Invariant.** `recovery_artifact == true` implies
//!   `safe_for_breakpoint == false`. This is enforced by the table and
//!   verified by [`NodeKindFlags::validate`].
//!
//! # Usage
//!
//! ```rust
//! use perl_ast::NodeKind;
//! use perl_ast::classification::NodeKindCategory;
//!
//! let kind = NodeKind::FunctionCall { name: "print".to_string(), args: vec![] };
//! assert_eq!(kind.category(), NodeKindCategory::Expression);
//! assert!(kind.is_executable());
//! assert!(kind.safe_for_breakpoint());
//! ```

use crate::ast::NodeKind;

/// High-level semantic category for a [`NodeKind`] variant.
///
/// Each variant belongs to exactly one category. The category reflects the
/// dominant role of the construct in the Perl language, not its syntactic
/// surface form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKindCategory {
    /// The root program node.
    Program,
    /// A statement (flow control, returns, loop control).
    Statement,
    /// An expression (has a value; may have side effects).
    Expression,
    /// A declaration (introduces a name into some scope or symbol table).
    Declaration,
    /// A scoping construct that directly wraps a block (Block only).
    Scope,
    /// A pure literal value with no side effects.
    Literal,
    /// An operator or punctuation node (Ellipsis).
    Operator,
    /// A comment or documentation node (reserved for future use).
    CommentDoc,
    /// A synthetic recovery node produced by error recovery.
    Recovery,
    /// An uncategorized or unknown node (reserved for future use).
    Unknown,
}

/// Boolean flags describing the semantic role of a [`NodeKind`] variant.
///
/// All flags are determined statically from the variant discriminant alone.
/// See the module-level documentation for the precise semantics of each flag
/// and the invariants between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeKindFlags {
    /// Code that can/will execute (methods, ops, side effects).
    ///
    /// Opposite of pure literals and declaration-only nodes.
    /// Conservative default: `false`. If wrong, breakpoints fail silently.
    pub executable: bool,

    /// Creates a new lexical scope (my vars, blocks, subs, class, package).
    ///
    /// Triggers scope analyzer stack push. Conservative default: `false`.
    /// If wrong, scope-bleeding bugs may result.
    pub introduces_scope: bool,

    /// Binds a name (`my $x`, `sub foo`, `use Module`, `package Foo`).
    ///
    /// Triggers symbol table insertion. Conservative default: `false`.
    /// Missing declarations lose symbols but do not crash.
    pub declares_symbol: bool,

    /// References a name (`$x`, `foo()`, `@arr`).
    ///
    /// Triggers use-def chain lookups. Conservative default: `false`.
    /// Missing refs lose cross-references but are safe.
    pub references_symbol: bool,

    /// Has `Vec` or `Box` children worth walking during AST traversal.
    ///
    /// Speeds up traversal filters that can skip leaf nodes. Conservative
    /// default: `false`. If wrong, traversal misses nodes but does not crash.
    pub contains_children: bool,

    /// Synthetic node produced by error recovery (`Error`, `Missing*`, `UnknownRest`).
    ///
    /// **Critical**: recovery nodes must NEVER be `safe_for_breakpoint`.
    /// Conservative default: `false`.
    pub recovery_artifact: bool,

    /// This kind of node can host a debugger breakpoint.
    ///
    /// **Static, variant-level.** Must be AND-ed with positional checks by
    /// the consumer. `true` means "this kind is a valid breakpoint candidate";
    /// it does not mean "always stop here".
    ///
    /// **Invariant**: if `recovery_artifact` is `true`, this must be `false`.
    /// Conservative default: `false`. Only `true` if proven safe.
    pub safe_for_breakpoint: bool,
}

impl NodeKindFlags {
    /// Verify the invariant: `recovery_artifact && safe_for_breakpoint` is forbidden.
    ///
    /// Returns `Ok(())` when the flags are internally consistent, or an error
    /// string describing the violation.
    pub fn validate(&self) -> Result<(), String> {
        if self.recovery_artifact && self.safe_for_breakpoint {
            Err("recovery_artifact and safe_for_breakpoint must not both be true".into())
        } else {
            Ok(())
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helper macro for concise flag construction
// ────────────────────────────────────────────────────────────────────────────

macro_rules! flags {
    (
        exec=$exec:expr,
        scope=$scope:expr,
        decl=$decl:expr,
        refs=$refs:expr,
        children=$children:expr,
        recovery=$recovery:expr,
        bp=$bp:expr
    ) => {
        NodeKindFlags {
            executable: $exec,
            introduces_scope: $scope,
            declares_symbol: $decl,
            references_symbol: $refs,
            contains_children: $children,
            recovery_artifact: $recovery,
            safe_for_breakpoint: $bp,
        }
    };
}

impl NodeKind {
    /// Return the high-level [`NodeKindCategory`] for this variant.
    ///
    /// The match is exhaustive with no wildcard arm — adding a new
    /// `NodeKind` variant is a compile error until this function is updated.
    pub fn category(&self) -> NodeKindCategory {
        match self {
            NodeKind::Program { .. } => NodeKindCategory::Program,

            NodeKind::ExpressionStatement { .. }
            | NodeKind::Defer { .. }
            | NodeKind::Try { .. }
            | NodeKind::If { .. }
            | NodeKind::LabeledStatement { .. }
            | NodeKind::While { .. }
            | NodeKind::For { .. }
            | NodeKind::Foreach { .. }
            | NodeKind::Given { .. }
            | NodeKind::When { .. }
            | NodeKind::Default { .. }
            | NodeKind::StatementModifier { .. }
            | NodeKind::Return { .. }
            | NodeKind::LoopControl { .. }
            | NodeKind::Goto { .. } => NodeKindCategory::Statement,

            NodeKind::Variable { .. }
            | NodeKind::VariableWithAttributes { .. }
            | NodeKind::Assignment { .. }
            | NodeKind::Binary { .. }
            | NodeKind::Ternary { .. }
            | NodeKind::Unary { .. }
            | NodeKind::Diamond
            | NodeKind::Readline { .. }
            | NodeKind::Glob { .. }
            | NodeKind::Typeglob { .. }
            | NodeKind::ArrayLiteral { .. }
            | NodeKind::HashLiteral { .. }
            | NodeKind::Eval { .. }
            | NodeKind::Do { .. }
            | NodeKind::Tie { .. }
            | NodeKind::Untie { .. }
            | NodeKind::MethodCall { .. }
            | NodeKind::FunctionCall { .. }
            | NodeKind::IndirectCall { .. }
            | NodeKind::Match { .. }
            | NodeKind::Substitution { .. }
            | NodeKind::Transliteration { .. }
            | NodeKind::Identifier { .. } => NodeKindCategory::Expression,

            NodeKind::VariableDeclaration { .. }
            | NodeKind::VariableListDeclaration { .. }
            | NodeKind::Subroutine { .. }
            | NodeKind::Prototype { .. }
            | NodeKind::Signature { .. }
            | NodeKind::MandatoryParameter { .. }
            | NodeKind::OptionalParameter { .. }
            | NodeKind::SlurpyParameter { .. }
            | NodeKind::NamedParameter { .. }
            | NodeKind::Method { .. }
            | NodeKind::Package { .. }
            | NodeKind::Use { .. }
            | NodeKind::No { .. }
            | NodeKind::PhaseBlock { .. }
            | NodeKind::DataSection { .. }
            | NodeKind::Class { .. }
            | NodeKind::Format { .. } => NodeKindCategory::Declaration,

            NodeKind::Block { .. } => NodeKindCategory::Scope,

            NodeKind::Number { .. }
            | NodeKind::String { .. }
            | NodeKind::Heredoc { .. }
            | NodeKind::Regex { .. }
            | NodeKind::Undef => NodeKindCategory::Literal,

            NodeKind::Ellipsis => NodeKindCategory::Operator,

            NodeKind::Error { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock
            | NodeKind::UnknownRest => NodeKindCategory::Recovery,
        }
    }

    /// Return the full [`NodeKindFlags`] for this variant.
    ///
    /// The match is exhaustive with no wildcard arm — adding a new
    /// `NodeKind` variant is a compile error until this function is updated.
    ///
    /// See [`NodeKindFlags`] for the precise semantics of each flag.
    ///
    /// # Invariant
    ///
    /// The returned flags always satisfy `flags.validate().is_ok()`.
    pub fn flags(&self) -> NodeKindFlags {
        //  Columns:       exec   scope  decl   refs   children  recovery  bp
        match self {
            NodeKind::Program { .. } => flags!(
                exec = false,
                scope = true,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::ExpressionStatement { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::VariableDeclaration { .. } => flags!(
                exec = true,
                scope = true,
                decl = true,
                refs = false,
                children = false,
                recovery = false,
                bp = true
            ),
            NodeKind::VariableListDeclaration { .. } => flags!(
                exec = true,
                scope = true,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Variable { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = true,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::VariableWithAttributes { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::Assignment { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Binary { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Ternary { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Unary { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Diamond => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = true
            ),
            NodeKind::Ellipsis => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Undef => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Readline { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Glob { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Typeglob { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = true,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Number { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::String { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::Heredoc { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::ArrayLiteral { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::HashLiteral { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::Block { .. } => flags!(
                exec = true,
                scope = true,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Eval { .. } => flags!(
                exec = true,
                scope = true,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Do { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Defer { .. } => flags!(
                exec = true,
                scope = true,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Try { .. } => flags!(
                exec = true,
                scope = true,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::If { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::LabeledStatement { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::While { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Tie { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Untie { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = false,
                recovery = false,
                bp = true
            ),
            NodeKind::For { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Foreach { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Given { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::When { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Default { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::StatementModifier { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Subroutine { .. } => flags!(
                exec = false,
                scope = true,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Prototype { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Signature { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::MandatoryParameter { .. } => flags!(
                exec = false,
                scope = false,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::OptionalParameter { .. } => flags!(
                exec = false,
                scope = false,
                decl = true,
                refs = true,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::SlurpyParameter { .. } => flags!(
                exec = false,
                scope = false,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::NamedParameter { .. } => flags!(
                exec = false,
                scope = false,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = false
            ),
            NodeKind::Method { .. } => flags!(
                exec = false,
                scope = true,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Return { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::LoopControl { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = true
            ),
            NodeKind::Goto { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::MethodCall { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::FunctionCall { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::IndirectCall { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Regex { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Match { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Substitution { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Transliteration { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = true,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Package { .. } => flags!(
                exec = true,
                scope = true,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Use { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::No { .. } => flags!(
                exec = true,
                scope = false,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::PhaseBlock { .. } => flags!(
                exec = true,
                scope = true,
                decl = false,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::DataSection { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Class { .. } => flags!(
                exec = false,
                scope = true,
                decl = true,
                refs = false,
                children = true,
                recovery = false,
                bp = true
            ),
            NodeKind::Format { .. } => flags!(
                exec = false,
                scope = false,
                decl = true,
                refs = false,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Identifier { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = true,
                children = false,
                recovery = false,
                bp = false
            ),
            NodeKind::Error { .. } => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = true,
                bp = false
            ),
            NodeKind::MissingExpression => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = true,
                bp = false
            ),
            NodeKind::MissingStatement => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = true,
                bp = false
            ),
            NodeKind::MissingIdentifier => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = true,
                bp = false
            ),
            NodeKind::MissingBlock => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = true,
                bp = false
            ),
            NodeKind::UnknownRest => flags!(
                exec = false,
                scope = false,
                decl = false,
                refs = false,
                children = false,
                recovery = true,
                bp = false
            ),
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // Convenience flag accessors
    // ────────────────────────────────────────────────────────────────────────

    /// Returns `true` if this node kind represents executable code.
    ///
    /// See [`NodeKindFlags::executable`].
    #[inline]
    pub fn is_executable(&self) -> bool {
        self.flags().executable
    }

    /// Returns `true` if this node kind introduces a new lexical scope.
    ///
    /// See [`NodeKindFlags::introduces_scope`].
    #[inline]
    pub fn introduces_scope(&self) -> bool {
        self.flags().introduces_scope
    }

    /// Returns `true` if this node kind declares a symbol into a scope or
    /// symbol table.
    ///
    /// See [`NodeKindFlags::declares_symbol`].
    #[inline]
    pub fn declares_symbol(&self) -> bool {
        self.flags().declares_symbol
    }

    /// Returns `true` if this node kind references a symbol.
    ///
    /// See [`NodeKindFlags::references_symbol`].
    #[inline]
    pub fn references_symbol(&self) -> bool {
        self.flags().references_symbol
    }

    /// Returns `true` if this node kind can host a debugger breakpoint.
    ///
    /// This is a **variant-level** flag. Consumers must AND it with positional
    /// checks (heredoc body interior, POD block, `__DATA__` section, etc.)
    /// before accepting a breakpoint request.
    ///
    /// See [`NodeKindFlags::safe_for_breakpoint`].
    #[inline]
    pub fn safe_for_breakpoint(&self) -> bool {
        self.flags().safe_for_breakpoint
    }

    /// Returns `true` if this node kind is a synthetic recovery artifact.
    ///
    /// Recovery nodes should never be offered to editor features such as
    /// hover, go-to-definition, or breakpoint placement.
    ///
    /// See [`NodeKindFlags::recovery_artifact`].
    #[inline]
    pub fn is_recovery(&self) -> bool {
        self.flags().recovery_artifact
    }
}
