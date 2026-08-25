//! PL406 semantic traversal disposition registry (issue #10844).
//!
//! For every primary [`perl_ast::NodeKind`] variant this registry records how
//! PL406's local flow summarizer treats that variant: which children execute,
//! in what order, which child summaries may close parent fallthrough, which
//! scope applies, which conditional rules hold, and what the construct's
//! proof ceiling is. It prevents another hand-maintained partial traversal
//! from silently omitting executable children.
//!
//! Authority boundaries:
//!
//! - Structural children remain #7298's authority
//!   ([`perl_ast::Node::try_for_each_child_with_field`]). This registry adds
//!   semantic dispositions on top; it is never a second structural walker.
//! - Structural classification remains #7015's authority
//!   ([`perl_ast::AST_NODE_POLICIES`]). A variant whose structural policy
//!   permits children may only be disposed as [`Pl406SemanticClass::Leaf`]
//!   with an explicit `leaf_reason`.
//! - The registry drives or validates the local summarizer in
//!   [`super::unreachable_code`]; the summarizer remains the single
//!   recursive traversal core for PL406.
//!
//! Reconciliation is machine-checked in
//! `tests/unreachable_code_semantic_traversal_tests.rs`: registry completeness
//! against [`perl_ast::NodeKind::ALL_KIND_NAMES`], declared child fields
//! against the canonical traversal over fully populated fixture samples, and
//! leaf dispositions against the #7015 classification.
//!
//! No variant is currently disposed as unsupported. If one ever is, its row
//! must name an owning successor issue and must preserve conservative
//! parent fallthrough rather than fabricate exact non-fallthrough.

use perl_ast::NodeKind;

/// Version of the PL406 disposition contract.
pub const PL406_DISPOSITION_SCHEMA_VERSION: u32 = 1;

/// Semantic role of one `NodeKind` variant under PL406 local flow analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pl406SemanticClass {
    /// No executable children; the variant cannot transfer or host nested flow.
    Leaf,
    /// Children execute in the declared order within one execution unit.
    Sequential,
    /// Exactly-one-of child groups (branch chains, ternary alternatives).
    Alternatives,
    /// A conditional rule gates whether child code runs at all.
    Conditional,
    /// A loop body and optional continue block; their transfers stay local.
    Loop,
    /// Declares a fresh execution unit; the declaration falls through.
    CallableBoundary,
    /// Analyzed locally, but child transfers never promote to parent
    /// fallthrough authority.
    EvaluationBoundary,
    /// An expression with executable child fields visited for nested
    /// diagnostics; the expression itself conservatively falls through.
    ExpressionContainer,
    /// Recovered or synthetic syntax; useful for nested diagnostics but
    /// never exact non-fallthrough authority.
    Recovery,
}

/// Scope relationship introduced or inherited by the construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pl406Scope {
    /// The construct opens a new lexical/execution scope.
    Introduced,
    /// The construct continues the enclosing execution unit's scope.
    Inherited,
}

/// Short-circuit or conditional evaluation rule registered for the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pl406ConditionalRule {
    /// No conditional gating applies.
    None,
    /// A statement modifier may skip the controlled statement entirely;
    /// without a constant-value fact the parent keeps a skip path.
    ModifierGated,
    /// Short-circuit binary operators (`&&`, `||`, `//`, `and`, `or`, `xor`)
    /// may skip the right operand; child transfers require operator-specific
    /// facts before promotion.
    ShortCircuitBinary,
    /// Ternary alternatives: exactly one branch evaluates; the summary is
    /// exhaustive across both branches.
    TernaryAlternatives,
}

/// Strongest parent-facing claim the construct's summary may make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pl406ProofCeiling {
    /// Local transfers close parent fallthrough exactly.
    ExactLocalTransfer,
    /// The construct conservatively retains parent fallthrough regardless of
    /// child summaries; exactness requires successor facts (#10849/#10856).
    ConservativeFallthrough,
    /// Recovered syntax must preserve fallthrough and may never fabricate
    /// exact non-fallthrough.
    RecoveredFallthrough,
}

/// One PL406 semantic traversal disposition for a stable `NodeKind` identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pl406Disposition {
    /// Stable [`perl_ast::NodeKind::kind_name`] token.
    pub kind_name: &'static str,
    /// Semantic class under PL406 local flow analysis.
    pub class: Pl406SemanticClass,
    /// Executable child fields in evaluation order, named by their canonical
    /// [`perl_ast::FieldId`] tokens. Empty for leaves. For `Assignment` this
    /// order encodes Perl's rhs-before-lhs evaluation.
    pub executable_children: &'static [&'static str],
    /// Child fields analyzed for nested diagnostics whose summaries never
    /// propagate into the parent fallthrough decision.
    pub analyzed_not_propagated: &'static [&'static str],
    /// Child fields whose transfer summary may determine parent fallthrough.
    pub fallthrough_determining: &'static [&'static str],
    /// Scope introduced or inherited.
    pub scope: Pl406Scope,
    /// Short-circuit/conditional rule.
    pub conditional_rule: Pl406ConditionalRule,
    /// Parent-facing proof ceiling.
    pub proof_ceiling: Pl406ProofCeiling,
    /// Required justification when `class` is [`Pl406SemanticClass::Leaf`]
    /// while the #7015 structural classification permits children.
    pub leaf_reason: Option<&'static str>,
}

macro_rules! disposition {
    (
        $name:literal,
        $class:ident,
        $exec:expr,
        $not_propagated:expr,
        $determining:expr,
        $scope:ident,
        $rule:ident,
        $ceiling:ident,
        $leaf_reason:expr
    ) => {
        Pl406Disposition {
            kind_name: $name,
            class: Pl406SemanticClass::$class,
            executable_children: $exec,
            analyzed_not_propagated: $not_propagated,
            fallthrough_determining: $determining,
            scope: Pl406Scope::$scope,
            conditional_rule: Pl406ConditionalRule::$rule,
            proof_ceiling: Pl406ProofCeiling::$ceiling,
            leaf_reason: $leaf_reason,
        }
    };
}

/// One disposition row for every member of [`perl_ast::NodeKind::ALL_KIND_NAMES`].
///
/// Rows follow enum declaration order. The table is intentionally one row per
/// line so a reviewer can diff a single variant's disposition against its
/// neighbours. A new `NodeKind` variant fails the reconciliation tests until
/// its row and fixtures are added here.
#[rustfmt::skip]
pub const PL406_DISPOSITIONS: &[Pl406Disposition] = &[
    // Root and statement lists ------------------------------------------------
    disposition!("Program", Sequential, &["statements"], &[], &["statements"], Inherited, None, ExactLocalTransfer, None),
    disposition!("ExpressionStatement", Sequential, &["expression"], &[], &["expression"], Inherited, None, ExactLocalTransfer, None),
    disposition!("VariableDeclaration", Sequential, &["initializer"], &["variable"], &["initializer"], Inherited, None, ExactLocalTransfer, None),
    disposition!("VariableListDeclaration", Sequential, &["initializer"], &["variable"], &["initializer"], Inherited, None, ExactLocalTransfer, None),
    disposition!("NestedVariableList", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("destructuring binding list; carries no executable expression")),
    // Bindings and payload leaves ---------------------------------------------
    disposition!("Variable", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("VariableWithAttributes", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("attributes are compile-time payloads; the variable is a binding")),
    // Expressions ---------------------------------------------------------------
    // Assignment encodes Perl's rhs-before-lhs evaluation order; the first
    // transferring side in execution order selects the summary (issue #10844).
    disposition!("Assignment", Sequential, &["rhs", "lhs"], &[], &["rhs", "lhs"], Inherited, None, ExactLocalTransfer, None),
    disposition!("Binary", ExpressionContainer, &["left", "right"], &[], &[], Inherited, ShortCircuitBinary, ConservativeFallthrough, None),
    disposition!("ArraySlice", ExpressionContainer, &["target", "elements"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("HashSlice", ExpressionContainer, &["target", "key"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("KeyValueSlice", ExpressionContainer, &["target", "key"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("ChainedComparison", ExpressionContainer, &["elements"], &[], &[], Inherited, ShortCircuitBinary, ConservativeFallthrough, None),
    disposition!("Ternary", Alternatives, &["condition", "then_expr", "else_expr"], &[], &["then_expr", "else_expr"], Inherited, TernaryAlternatives, ExactLocalTransfer, None),
    disposition!("Unary", ExpressionContainer, &["operand"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Diamond", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Ellipsis", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Undef", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Readline", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Glob", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Typeglob", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Number", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("String", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("VString", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Heredoc", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("ArrayLiteral", ExpressionContainer, &["elements"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("HashLiteral", ExpressionContainer, &["key", "value"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    // Blocks --------------------------------------------------------------------
    // A bare BLOCK statement is a one-shot loop: unlabeled/self-labeled
    // last/next demote at the block boundary while die/return/goto promote.
    // In expression position (map/grep/sort block arguments) the same kind is
    // an evaluation boundary: analyzed locally, transfers never promoted.
    disposition!("Block", Sequential, &["statements"], &[], &["statements"], Introduced, None, ExactLocalTransfer, None),
    // Evaluation boundaries -----------------------------------------------------
    disposition!("Eval", EvaluationBoundary, &["block"], &["block"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("Do", EvaluationBoundary, &["block"], &["block"], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Defer", EvaluationBoundary, &["block"], &["block"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("Try", EvaluationBoundary, &["body", "catch", "finally"], &["body", "catch", "finally"], &[], Introduced, None, ConservativeFallthrough, None),
    // Branching -----------------------------------------------------------------
    disposition!("If", Alternatives, &["condition", "then_branch", "body", "else_branch"], &[], &["then_branch", "body", "else_branch"], Inherited, None, ExactLocalTransfer, None),
    disposition!("LabeledStatement", Sequential, &["statement"], &[], &["statement"], Inherited, None, ExactLocalTransfer, None),
    disposition!("While", Loop, &["condition", "body", "continue_block"], &["body", "continue_block"], &[], Introduced, None, ConservativeFallthrough, None),
    // Calls and tie family ------------------------------------------------------
    disposition!("Tie", ExpressionContainer, &["variable", "package", "args"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Untie", ExpressionContainer, &["variable"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    // Loops ---------------------------------------------------------------------
    disposition!("For", Loop, &["init", "condition", "body", "continue_block", "update"], &["body", "continue_block"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("Foreach", Loop, &["list", "body", "continue_block"], &["variable", "body", "continue_block"], &[], Introduced, None, ConservativeFallthrough, None),
    // Topical constructs --------------------------------------------------------
    disposition!("Given", EvaluationBoundary, &["expr", "body"], &["body"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("When", EvaluationBoundary, &["condition", "body"], &["body"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("Default", EvaluationBoundary, &["body"], &["body"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("StatementModifier", Conditional, &["condition", "statement"], &["statement"], &[], Inherited, ModifierGated, ConservativeFallthrough, None),
    // Callables -----------------------------------------------------------------
    disposition!("Subroutine", CallableBoundary, &["body"], &["body"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("Prototype", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("source-region prototype payload; no runtime flow")),
    disposition!("Signature", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("parameter defaults bind at call time, outside enclosing-list flow")),
    disposition!("MandatoryParameter", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("binding form evaluated at call time")),
    disposition!("OptionalParameter", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("default_value binds at call time, outside enclosing-list flow")),
    disposition!("SlurpyParameter", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("binding form evaluated at call time")),
    disposition!("NamedParameter", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("default_value binds at call time, outside enclosing-list flow")),
    disposition!("Method", CallableBoundary, &["body"], &["body"], &[], Introduced, None, ConservativeFallthrough, None),
    // Transfers -----------------------------------------------------------------
    disposition!("Return", Sequential, &["value"], &[], &[], Inherited, None, ExactLocalTransfer, None),
    disposition!("LoopControl", Leaf, &[], &[], &[], Inherited, None, ExactLocalTransfer, Some("no executable children; the statement itself is the exact transfer")),
    disposition!("Goto", ExpressionContainer, &["target"], &[], &[], Inherited, None, ExactLocalTransfer, None),
    // Call expressions ----------------------------------------------------------
    disposition!("MethodCall", ExpressionContainer, &["object", "args"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    // Exact terminators (die/exit/exec/croak/confess) are selected by the
    // summarizer's accepted name table (#5062); the kind-level disposition
    // stays conservative for every other spelling.
    disposition!("FunctionCall", ExpressionContainer, &["args"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("AmperCall", ExpressionContainer, &["args"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("IndirectCall", ExpressionContainer, &["object", "args"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    // Regex family ---------------------------------------------------------------
    disposition!("Regex", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Match", ExpressionContainer, &["expr"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Substitution", ExpressionContainer, &["expr"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Transliteration", ExpressionContainer, &["expr"], &[], &[], Inherited, None, ConservativeFallthrough, None),
    // Package-level units ---------------------------------------------------------
    disposition!("Package", EvaluationBoundary, &["block"], &["block"], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Use", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("compile-time import; no enclosing-list runtime flow")),
    disposition!("No", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("compile-time import; no enclosing-list runtime flow")),
    disposition!("PhaseBlock", EvaluationBoundary, &["block"], &["block"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("DataSection", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    disposition!("Class", EvaluationBoundary, &["body"], &["body"], &[], Introduced, None, ConservativeFallthrough, None),
    disposition!("Format", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, Some("source-boundary format body")),
    disposition!("Identifier", Leaf, &[], &[], &[], Inherited, None, ConservativeFallthrough, None),
    // Recovery --------------------------------------------------------------------
    disposition!("Error", Recovery, &["partial"], &["partial"], &[], Inherited, None, RecoveredFallthrough, None),
    disposition!("MissingExpression", Recovery, &[], &[], &[], Inherited, None, RecoveredFallthrough, None),
    disposition!("MissingStatement", Recovery, &[], &[], &[], Inherited, None, RecoveredFallthrough, None),
    disposition!("MissingIdentifier", Recovery, &[], &[], &[], Inherited, None, RecoveredFallthrough, None),
    disposition!("MissingBlock", Recovery, &[], &[], &[], Inherited, None, RecoveredFallthrough, None),
    disposition!("UnknownRest", Recovery, &[], &[], &[], Inherited, None, RecoveredFallthrough, None),
];

/// Return the registered PL406 disposition for a stable `NodeKind` name.
#[must_use]
pub fn pl406_disposition(kind_name: &str) -> Option<&'static Pl406Disposition> {
    PL406_DISPOSITIONS.iter().find(|row| row.kind_name == kind_name)
}

/// Return the registered PL406 disposition for a `NodeKind`, deriving the
/// token from the enum so a stale string cannot silently disable governance.
#[must_use]
pub fn pl406_disposition_of(kind: &NodeKind) -> Option<&'static Pl406Disposition> {
    pl406_disposition(kind.kind_name())
}

/// Return all registered dispositions in canonical declaration order.
#[must_use]
pub const fn all_pl406_dispositions() -> &'static [Pl406Disposition] {
    PL406_DISPOSITIONS
}
