//! HIR body graph: arena-based expression/statement/block representation.
//!
//! This module provides the first vertical slice of HIR body infrastructure,
//! implementing the arena-based graph described in ADR #2564. It introduces:
//!
//! - Typed arena indices: [`HirBodyId`], [`HirExprId`], [`HirStmtId`], [`HirBlockId`]
//! - Per-body arenas and a source map: [`HirBody`], [`BodySourceMap`]
//! - Body owners (program root + subroutine): [`BodyOwnerKind`]
//! - Expression/statement/block node taxonomy: [`HirExpr`], [`HirStmt`], [`HirBlock`]
//! - A lowering entry point for one Perl source string: [`lower_body`]
//!
//! # Specimen
//!
//! The first vertical slice lowers exactly one specimen end-to-end:
//!
//! ```text
//! my $x = $a + $b;
//! ```
//!
//! This produces a [`HirBody`] with:
//! - One [`HirStmt::Let`] binding `$x`
//! - One [`HirExpr::Binary`] `+` node with children reading `$a` and `$b`
//! - One [`HirExpr::Assign`] connecting the decl place to the binary value
//! - All nodes carry exact byte-offset source ranges in [`BodySourceMap`]

use crate::SourceLocation;

use super::model::{
    BranchKeyword, ControlTransferKind, LoopKind, ReadlineSource, StatementModifierKind,
    glob_pattern_interpolates,
};

// ──────────────────────────────────────────────────────────────────────────────
// Typed arena indices
// ──────────────────────────────────────────────────────────────────────────────

/// Stable index into the workspace body registry (one per body owner).
///
/// Currently unused at runtime — reserved so that cross-body references can be
/// introduced later without changing existing index types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirBodyId(pub u32);

/// Typed index into a [`HirBody`]'s expression arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirExprId(pub u32);

/// Typed index into a [`HirBody`]'s statement arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirStmtId(pub u32);

/// Typed index into a [`HirBody`]'s block arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirBlockId(pub u32);

/// Stable body-local identity for a control region that can receive a Perl
/// loop-control transfer (`next`, `last`, `redo`).
///
/// Region IDs are allocated in body source order as loops are lowered, so the
/// identity is stable across identical inputs. Consumers such as PIR-A and
/// downstream verifiers must use this ID rather than reconstructing the target
/// from raw source ranges, flat-HIR shells, or label strings (see #13249).
///
/// The identity is scoped to one [`HirBody`]: two different bodies may allocate
/// the same numeric value for unrelated regions, so a region ID has meaning
/// only inside the [`HirBody`] that produced it.
///
/// Both ordinary structured loops ([`HirExpr::Loop`]) and loop-form postfix
/// modifiers ([`HirStmt::PostfixCondition`] with `postfix_loop_region: Some(_)`)
/// allocate region IDs. Branch-form modifiers (`if`/`unless`) never do —
/// they are not loop targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirLoopRegionId(u32);

impl HirLoopRegionId {
    /// Construct a region ID from its raw index. Not part of the public
    /// contract — reserved for the body lowerer.
    pub(super) fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// The raw index as `u32`.
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// The raw index as `usize`, for indexing external per-region tables.
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Arena
// ──────────────────────────────────────────────────────────────────────────────

/// Typed append-only arena indexed by a strongly-typed ID.
///
/// The ID type must implement a constructor from `u32`; see the `impl` blocks
/// below for each concrete instantiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> Arena<T> {
    /// Push a value and return its index.
    pub fn alloc(&mut self, value: T) -> u32 {
        let id = self.items.len() as u32;
        self.items.push(value);
        id
    }

    /// Return a reference to the value at `index`.
    ///
    /// Returns `None` if the index is out of bounds.
    pub fn get(&self, index: u32) -> Option<&T> {
        self.items.get(index as usize)
    }

    /// Return the number of allocated items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Return true when no items have been allocated.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterate over all allocated items in allocation order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Source map
// ──────────────────────────────────────────────────────────────────────────────

/// Source map for one [`HirBody`].
///
/// Each arena entry in [`HirBody`] has a corresponding `SourceLocation` here,
/// indexed by the same raw `u32` that the typed ID wraps.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodySourceMap {
    /// Source range for each expression, indexed by [`HirExprId`] value.
    pub expr_ranges: Vec<SourceLocation>,
    /// Source range for each statement, indexed by [`HirStmtId`] value.
    pub stmt_ranges: Vec<SourceLocation>,
    /// Source range for each block, indexed by [`HirBlockId`] value.
    pub block_ranges: Vec<SourceLocation>,
}

impl BodySourceMap {
    /// Look up the source range for an expression.
    pub fn expr_range(&self, id: HirExprId) -> Option<SourceLocation> {
        self.expr_ranges.get(id.0 as usize).copied()
    }

    /// Look up the source range for a statement.
    pub fn stmt_range(&self, id: HirStmtId) -> Option<SourceLocation> {
        self.stmt_ranges.get(id.0 as usize).copied()
    }

    /// Look up the source range for a block.
    pub fn block_range(&self, id: HirBlockId) -> Option<SourceLocation> {
        self.block_ranges.get(id.0 as usize).copied()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Body owner
// ──────────────────────────────────────────────────────────────────────────────

/// What syntactic construct owns this body.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyOwnerKind {
    /// Top-level program root (file-level statement sequence).
    ProgramRoot,
    /// Named subroutine body (`sub foo { ... }`).
    Subroutine {
        /// Subroutine name, or `None` for anonymous subs.
        name: Option<String>,
    },
    /// Method body (`method foo { ... }`).
    Method {
        /// Method name.
        name: String,
    },
}

/// Stable key for a body in the per-file body registry.
///
/// The ordinal disambiguates multiple anonymous subroutines or method bodies
/// with the same name in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyOwner {
    /// What owns this body.
    pub kind: BodyOwnerKind,
    /// Zero-based ordinal for disambiguation within one file.
    pub ordinal: u32,
}

impl BodyOwner {
    /// Create a `BodyOwner` key.
    pub fn new(kind: BodyOwnerKind, ordinal: u32) -> Self {
        Self { kind, ordinal }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Expression nodes
// ──────────────────────────────────────────────────────────────────────────────

/// Sigil for a variable reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sigil {
    /// `$` — scalar.
    Scalar,
    /// `@` — array.
    Array,
    /// `%` — hash.
    Hash,
    /// `&` — code ref / sub.
    Code,
    /// `*` — typeglob.
    Glob,
}

impl Sigil {
    /// Parse from a sigil character string as produced by the AST.
    fn from_str(s: &str) -> Self {
        match s {
            "$" => Sigil::Scalar,
            "@" => Sigil::Array,
            "%" => Sigil::Hash,
            "&" => Sigil::Code,
            "*" => Sigil::Glob,
            _ => Sigil::Scalar, // graceful fallback; should not occur
        }
    }
}

/// How a variable is accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessMode {
    /// Value is being read.
    Read,
    /// Variable is the target of an assignment (lexical place / lvalue).
    Write,
    /// Variable is both read and written — compound assignment or `++`/`--`.
    ReadModifyWrite,
}

/// Variable origin classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariableKind {
    /// `my`-declared lexical in scope.
    Lexical,
    /// Package / stash variable.
    Package,
}

/// A variable reference node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirVariable {
    /// Variable sigil.
    pub sigil: Sigil,
    /// Variable name without sigil.
    pub name: String,
    /// Lexical or package origin.
    pub kind: VariableKind,
    /// How this node uses the variable.
    pub access: AccessMode,
}

/// Aggregate flavour of a subscript element access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriptKind {
    /// Array element access: `$arr[index]`.
    Array,
    /// Hash element access: `$hash{key}`.
    Hash,
}

/// An array/hash element access with evaluate-once place semantics.
///
/// The `container` and `subscript` are kept as separate explicit expression IDs
/// so a computed index/key (e.g. `$h{f()}`) is modeled as evaluated exactly
/// once, and so PIR lowering can treat the element as an lvalue place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirSubscript {
    /// Array vs hash element access.
    pub kind: SubscriptKind,
    /// The aggregate being indexed (array or hash), as an explicit expr ID.
    pub container: HirExprId,
    /// The index or key expression, as an explicit expr ID.
    pub subscript: HirExprId,
    /// How the element is accessed (read, write-place, or read-modify-write).
    pub access: AccessMode,
}

/// How an assignment is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignMode {
    /// Simple `=` assignment.
    Simple,
    /// Compound assignment: `+=`, `-=`, `*=`, etc.
    ///
    /// The LHS is both read (to compute the new value) and written (to store
    /// the result).  The LHS variable node carries [`AccessMode::ReadModifyWrite`].
    ReadModifyWrite,
}

/// How a unary operator accesses its operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryMode {
    /// Pure read (e.g. unary minus, `!`).
    Read,
    /// Read-modify-write (e.g. `++`, `--`).
    ReadModifyWrite,
}

/// Binary operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    /// Numeric addition `+`.
    Add,
    /// Numeric subtraction `-`.
    Sub,
    /// Numeric multiplication `*`.
    Mul,
    /// Numeric division `/`.
    Div,
    /// String concatenation `.`.
    Concat,
    /// Other/unknown operator — preserves the original text.
    Other(String),
}

impl BinaryOp {
    fn from_str(s: &str) -> Self {
        match s {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "." => BinaryOp::Concat,
            other => BinaryOp::Other(other.to_string()),
        }
    }
}

/// Optional controlling label attached to a loop region.
///
/// Perl `LABEL:` syntax attaches an identifier to the immediately-following
/// loop (or loop-form postfix modifier) so that `next LABEL` / `last LABEL` /
/// `redo LABEL` can target that specific enclosing loop.
///
/// The `range` is the parser's `LabeledStatement` span: it starts at the label
/// token and extends through the subordinate statement. The trailing colon is
/// therefore included as part of the enclosing statement span. Consumers that
/// need the token-only extent should use the label spelling and source text
/// rather than treating this range as a token range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirLoopLabel {
    /// Label spelling as written in source (e.g. `"OUTER"`).
    pub name: String,
    /// Source range of the enclosing labeled statement.
    pub range: SourceLocation,
}

/// Explanation of how a [`HirStmt::LoopControl`] was bound to a target region.
///
/// A statically-valid transfer resolves to [`LoopControlResolution::Resolved`]
/// with `resolved_target: Some(_)`. Every other outcome carries a typed
/// disposition — the body lowerer must never silently fall back to the nearest
/// loop or drop a label. Downstream verifiers, diagnostics, and PIR consumers
/// read the disposition rather than reconstructing target identity from
/// source ranges (see #13249).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopControlResolution {
    /// The transfer is bound to a specific loop region — see
    /// [`HirStmt::LoopControl::resolved_target`] for the region ID.
    Resolved,
    /// Unlabelled transfer with no enclosing loop region visible from this
    /// statement. `next`/`last`/`redo` outside a loop.
    NoEnclosingLoop,
    /// Labelled transfer whose label matches an enclosing labelled construct
    /// that this HIR does not model as a loop region (e.g. a labelled bare
    /// block, `LABEL: { ... }`). A typed boundary rather than a silent
    /// misresolve to the nearest loop.
    NonLoopTarget {
        /// Label spelling as written on the enclosing non-loop construct.
        label: String,
    },
    /// Labelled transfer whose label does not match any enclosing labelled
    /// construct visible from this statement.
    UnresolvedLabel {
        /// Label spelling as written on the `next`/`last`/`redo`.
        label: String,
    },
}

/// One expression node in the HIR body graph.
///
/// Every variant that has child expressions carries explicit [`HirExprId`]
/// references — there are no flat shells.
///
/// `#[non_exhaustive]` for the same reason as [`HirKind`]: this taxonomy grows
/// as construct families are modeled, and a downstream exhaustive match must
/// not break each time a family lands.
///
/// [`HirKind`]: super::model::HirKind
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HirExpr {
    /// Variable read or write-place reference.
    Variable(HirVariable),

    /// Binary expression: `lhs OP rhs`, both children are explicit IDs.
    Binary {
        /// Left-hand operand.
        lhs: HirExprId,
        /// Operator.
        op: BinaryOp,
        /// Right-hand operand.
        rhs: HirExprId,
    },

    /// Assignment expression: `lhs = rhs`, both sides are explicit IDs.
    ///
    /// For `my $x = …`, the `lhs` is a `Variable` node with `access: Write`
    /// representing the declared place, and `rhs` is the initializer.
    Assign {
        /// Assignment target (place / lvalue).
        lhs: HirExprId,
        /// Value being assigned.
        rhs: HirExprId,
        /// Assignment mode.
        mode: AssignMode,
    },

    /// Unary expression: `OP operand`.
    Unary {
        /// Operand.
        operand: HirExprId,
        /// How the operator accesses the operand.
        mode: UnaryMode,
        /// Operator text, for diagnostics.
        op: String,
    },

    /// Structured `if`/`unless` control flow.
    Branch {
        /// Condition expression.
        condition: HirExprId,
        /// Then-arm block.
        then_block: HirBlockId,
        /// Zero or more `elsif` condition/block pairs in source order.
        elsif_arms: Vec<(HirExprId, HirBlockId)>,
        /// Optional `else` block.
        else_block: Option<HirBlockId>,
        /// Surface branch keyword (`if`, `unless`).
        keyword: BranchKeyword,
    },

    /// Structured loop control flow.
    Loop {
        /// Loop family.
        kind: LoopKind,
        /// Stable body-local target identity for `next`/`last`/`redo` (#13249).
        ///
        /// Consumers such as PIR-A and downstream verifiers use this ID to
        /// pair a [`HirStmt::LoopControl`] with its target loop region — they
        /// must not reconstruct target identity from raw source ranges, flat
        /// HIR shells, or label strings.
        region_id: HirLoopRegionId,
        /// Optional controlling label inherited from an enclosing
        /// `LABEL:` statement, with its source range (#13249).
        ///
        /// Present when this loop was written as `LABEL: while/until/for
        /// /foreach (...)`. `None` for unlabelled loops.
        label: Option<HirLoopLabel>,
        /// Optional C-style loop initializer block.
        ///
        /// The block preserves every initializer statement, including
        /// comma-separated declarations in the C-style header.
        init: Option<HirBlockId>,
        /// Loop condition or iterable expression, when present.
        condition: Option<HirExprId>,
        /// Optional C-style loop update expression.
        update: Option<HirExprId>,
        /// Loop body block.
        body: HirBlockId,
        /// Optional `continue` block.
        continue_block: Option<HirBlockId>,
        /// Optional foreach iterator binding.
        iterator_binding: Option<HirExprId>,
    },

    /// Ternary conditional expression.
    Ternary {
        /// Condition expression.
        condition: HirExprId,
        /// Expression selected for a true condition.
        then_expr: HirExprId,
        /// Expression selected for a false condition.
        else_expr: HirExprId,
    },

    /// Return from the enclosing subroutine.
    Return {
        /// Optional returned value.
        value: Option<HirExprId>,
    },

    /// Function/method call expression (first-pass model).
    ///
    /// Arguments that are individually lowerable carry explicit IDs; everything
    /// else is `Opaque`.
    Call {
        /// Argument expressions in source order.
        args: Vec<HirExprId>,
        /// The AST node kind name, for diagnostics.
        ast_kind: String,
        /// Source span of the callee (function/method name) sub-expression.
        /// `None` when the callee has no distinct span (e.g. bare-word in
        /// `foo()` is not a separate AST node). Enables linking a call site
        /// back to its declaration for effect analysis (#5682).
        callee_span: Option<SourceLocation>,
    },

    /// Array/hash element access (`$arr[i]`, `$hash{k}`) modeled as an
    /// evaluate-once place. See [`HirSubscript`].
    Subscript(HirSubscript),

    /// Heredoc value shell with source-backed body text.
    Heredoc {
        /// Terminator token as written.
        delimiter: String,
        /// Whether the body interpolates variables.
        interpolated: bool,
        /// Whether the indented heredoc form was used.
        indented: bool,
        /// Whether the body executes through the shell.
        command: bool,
        /// Source range of the body text, when available.
        body_range: Option<SourceLocation>,
    },

    /// Filehandle-read value shell.
    Readline {
        /// Read source classification.
        source: ReadlineSource,
        /// Filehandle text, absent for the diamond form.
        filehandle: Option<String>,
    },

    /// Angle-bracket glob value shell.
    Glob {
        /// Pattern without surrounding angle brackets.
        pattern: String,
        /// Whether the pattern interpolates variables.
        interpolated: bool,
    },

    /// Opaque expression — used when the AST shape is not yet modeled.
    Opaque {
        /// The AST node kind name for diagnostics.
        ast_kind: String,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// Statement nodes
// ──────────────────────────────────────────────────────────────────────────────

/// Storage class for a variable declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclStorageClass {
    /// `my` — lexical.
    My,
    /// `our` — package alias.
    Our,
    /// `local` — dynamic.
    Local,
    /// `state` — persistent lexical.
    State,
}

impl DeclStorageClass {
    fn from_str(s: &str) -> Self {
        match s {
            "my" => DeclStorageClass::My,
            "our" => DeclStorageClass::Our,
            "local" => DeclStorageClass::Local,
            "state" => DeclStorageClass::State,
            _ => DeclStorageClass::My,
        }
    }
}

/// One statement node in the HIR body graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirStmt {
    /// Expression statement: evaluate the expression for its side effects.
    Expr(HirExprId),

    /// Variable declaration: `my $x = …`
    ///
    /// The `init` expression, when present, is the full initializer (which may
    /// itself be an [`HirExpr::Assign`] that links the declared place to its
    /// value).
    Let {
        /// Variable name without sigil.
        name: String,
        /// Sigil of the declared variable.
        sigil: Sigil,
        /// Storage class: `my`, `our`, `local`, `state`.
        storage: DeclStorageClass,
        /// Optional initializer expression ID.
        init: Option<HirExprId>,
        /// Source span of the declared variable token (`$x`), for reference / LSP
        /// anchoring. Distinct from the enclosing statement span (`stmt_ranges`)
        /// and present for EVERY declaration form, including those without an
        /// initializer — so PIR lowering anchors declarations at the variable,
        /// matching the legacy find-references provider (#2643 range parity).
        binding_range: SourceLocation,
    },

    /// Loop-control transfer (`next`, `last`, or `redo`).
    LoopControl {
        /// Transfer verb.
        verb: LoopControlVerb,
        /// Label as written on the transfer, if any (e.g. `next OUTER`).
        ///
        /// Preserved verbatim from the AST for diagnostics and re-serialisation.
        /// Downstream consumers must NOT rely on this field for target
        /// identity; use `resolved_target` and `resolution` instead (#13249).
        written_label: Option<String>,
        /// Resolved target loop region when the transfer is statically
        /// valid — otherwise `None`, in which case `resolution` explains why.
        ///
        /// Unlabelled transfers resolve to the innermost enclosing loop
        /// region. Labelled transfers resolve to the innermost enclosing
        /// loop region whose controlling label matches by exact string
        /// equality; two nested loops sharing a spelling both remain
        /// addressable by their distinct region IDs (#13249).
        resolved_target: Option<HirLoopRegionId>,
        /// Explanation of the resolution outcome. Downstream verifiers use
        /// this to distinguish an unbound-label transfer from an unlabelled
        /// transfer outside a loop, and to detect labelled transfers into
        /// non-loop labelled regions (#13249).
        resolution: LoopControlResolution,
    },

    /// Statement followed by a postfix condition (`expr if condition`).
    PostfixCondition {
        /// Structured statement being conditionally executed.
        statement: HirStmtId,
        /// Postfix condition expression.
        condition: HirExprId,
        /// Postfix modifier verb.
        verb: StatementModifierKind,
        /// Body-local loop-region identity when this postfix modifier acts as
        /// a loop (`STMT while COND`, `STMT until COND`, `STMT for LIST`,
        /// `STMT foreach LIST`), and `None` for the branch-form modifiers
        /// `if`/`unless` — which are never loop targets (#13249).
        postfix_loop_region: Option<HirLoopRegionId>,
        /// Optional controlling label inherited from an enclosing `LABEL:`
        /// statement, applicable only to loop-form postfix modifiers.
        /// Branch-form modifiers ignore any pending label so that a
        /// `LABEL: STMT if COND;` does not silently misclassify the label as
        /// a loop target (#13249).
        postfix_label: Option<HirLoopLabel>,
    },
}

/// Loop-control verb stored in a structured body statement.
pub type LoopControlVerb = ControlTransferKind;

// ──────────────────────────────────────────────────────────────────────────────
// Block node
// ──────────────────────────────────────────────────────────────────────────────

/// A sequenced list of statements — the building block of every body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HirBlock {
    /// Ordered statement IDs in source order.
    pub stmts: Vec<HirStmtId>,
}

// ──────────────────────────────────────────────────────────────────────────────
// HirBody
// ──────────────────────────────────────────────────────────────────────────────

/// Arena-based expression/statement/block graph for one body owner.
///
/// This is the unit of lowering produced by [`lower_body`]. It is separate
/// from [`crate::hir::HirFile`]'s flat item list — flat items remain for
/// compile-time fact extraction; bodies are the new representation for data-flow
/// analysis, context propagation, and PIR-A lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirBody {
    /// All expression nodes, indexed by [`HirExprId`].
    pub exprs: Arena<HirExpr>,
    /// All statement nodes, indexed by [`HirStmtId`].
    pub stmts: Arena<HirStmt>,
    /// All block nodes (ordered statement sequences), indexed by [`HirBlockId`].
    pub blocks: Arena<HirBlock>,
    /// Source map: maps each expr/stmt/block index to its [`SourceLocation`].
    pub source_map: BodySourceMap,
    /// Root block — the entry point for the body.
    pub root_block: HirBlockId,
    /// What syntactic construct owns this body.
    pub owner: BodyOwnerKind,
}

impl HirBody {
    /// Look up an expression node by ID.
    pub fn expr(&self, id: HirExprId) -> Option<&HirExpr> {
        self.exprs.get(id.0)
    }

    /// Look up a statement node by ID.
    pub fn stmt(&self, id: HirStmtId) -> Option<&HirStmt> {
        self.stmts.get(id.0)
    }

    /// Look up a block node by ID.
    pub fn block(&self, id: HirBlockId) -> Option<&HirBlock> {
        self.blocks.get(id.0)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Body builder (internal)
// ──────────────────────────────────────────────────────────────────────────────

struct BodyBuilder {
    exprs: Arena<HirExpr>,
    stmts: Arena<HirStmt>,
    blocks: Arena<HirBlock>,
    source_map: BodySourceMap,
}

impl BodyBuilder {
    fn new() -> Self {
        Self {
            exprs: Arena::default(),
            stmts: Arena::default(),
            blocks: Arena::default(),
            source_map: BodySourceMap::default(),
        }
    }

    fn alloc_expr(&mut self, expr: HirExpr, range: SourceLocation) -> HirExprId {
        let idx = self.exprs.alloc(expr);
        self.source_map.expr_ranges.push(range);
        HirExprId(idx)
    }

    fn alloc_stmt(&mut self, stmt: HirStmt, range: SourceLocation) -> HirStmtId {
        let idx = self.stmts.alloc(stmt);
        self.source_map.stmt_ranges.push(range);
        HirStmtId(idx)
    }

    fn alloc_block(&mut self, block: HirBlock, range: SourceLocation) -> HirBlockId {
        let idx = self.blocks.alloc(block);
        self.source_map.block_ranges.push(range);
        HirBlockId(idx)
    }

    fn finish(self, root_block: HirBlockId, owner: BodyOwnerKind) -> HirBody {
        HirBody {
            exprs: self.exprs,
            stmts: self.stmts,
            blocks: self.blocks,
            source_map: self.source_map,
            root_block,
            owner,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Lowering
// ──────────────────────────────────────────────────────────────────────────────

use crate::{Node, NodeKind};

/// Lower the top-level program-root body from a parsed AST node.
///
/// This is the entry point for the first vertical slice. It lowers the
/// statements inside a `Program` node into an arena-based [`HirBody`].
///
/// Currently handled constructs:
/// - `my $x = EXPR;` — [`HirStmt::Let`] with an [`HirExpr::Assign`] initializer
/// - `$a + $b` — [`HirExpr::Binary`] with explicit child IDs
/// - Variable references — [`HirExpr::Variable`]
/// - Everything else — [`HirExpr::Opaque`] / [`HirStmt::Expr`] fallback
pub fn lower_body(ast: &Node) -> HirBody {
    let mut builder = BodyBuilder::new();

    let stmts = match &ast.kind {
        NodeKind::Program { statements } => statements.as_slice(),
        _ => std::slice::from_ref(ast),
    };

    let root_range = ast.location;
    let mut root_block = HirBlock::default();

    for stmt_node in stmts {
        let stmt_id = lower_statement(&mut builder, stmt_node);
        root_block.stmts.push(stmt_id);
    }

    let root_id = builder.alloc_block(root_block, root_range);
    builder.finish(root_id, BodyOwnerKind::ProgramRoot)
}

fn lower_statement(builder: &mut BodyBuilder, node: &Node) -> HirStmtId {
    let range = node.location;

    match &node.kind {
        NodeKind::VariableDeclaration { declarator, variable, initializer, .. } => {
            // `local $x = EXPR` parses its target as an `Assignment` (`$x = EXPR`)
            // rather than a bare `Variable`, because `local` accepts arbitrary
            // lvalues. Unwrap to the localized lvalue so the declared name and the
            // `binding_range` anchor at the variable token, not the whole
            // `$x = EXPR` span (mirrors `variable_binding()` in the first pass).
            // For `my`/`our`/`state` the initializer is a separate field, so
            // `variable` is already the bare token and this unwrap is a no-op.
            let binding_node: &Node = match &variable.kind {
                NodeKind::Assignment { lhs, .. } => lhs.as_ref(),
                _ => variable.as_ref(),
            };
            // Extract variable name and sigil from the inner Variable node.
            let (sigil_str, var_name) = match &binding_node.kind {
                NodeKind::Variable { sigil, name } => (sigil.as_str(), name.clone()),
                _ => ("$", String::from("<unknown>")),
            };
            let sigil = Sigil::from_str(sigil_str);
            let storage = DeclStorageClass::from_str(declarator);

            let init_expr_id = initializer.as_ref().map(|init_node| {
                // The initializer is the full RHS expression.
                // For `my $x = $a + $b`, the AST may represent this as:
                //   VariableDeclaration { variable: $x, initializer: Binary(+, $a, $b) }
                // We model the assignment as:
                //   Assign { lhs: Variable($x, Write), rhs: lower_expr(initializer) }

                // Allocate the write-place for $x
                let place_expr = HirExpr::Variable(HirVariable {
                    sigil: Sigil::from_str(sigil_str),
                    name: var_name.clone(),
                    kind: VariableKind::Lexical,
                    access: AccessMode::Write,
                });
                let place_id = builder.alloc_expr(place_expr, variable.location);

                // Lower the RHS value expression
                let rhs_id = lower_expr(builder, init_node);

                // Wrap in an Assign node spanning the full declaration range
                let assign_range =
                    SourceLocation { start: variable.location.start, end: init_node.location.end };
                let assign_expr =
                    HirExpr::Assign { lhs: place_id, rhs: rhs_id, mode: AssignMode::Simple };
                builder.alloc_expr(assign_expr, assign_range)
            });
            let init_expr_id = init_expr_id.or_else(|| match &variable.kind {
                // `local $x OP EXPR`: the parser stores the whole assignment in
                // `variable`. Lower it directly so the operator picks the mode;
                // the place is the dynamically scoped package slot, never a
                // lexical, so a PIR consumer sees a stash modification.
                NodeKind::Assignment { lhs, rhs, op } => {
                    Some(lower_assignment(builder, variable, lhs, rhs, op, VariableKind::Package))
                }
                _ => None,
            });

            builder.alloc_stmt(
                HirStmt::Let {
                    name: var_name,
                    sigil,
                    storage,
                    init: init_expr_id,
                    binding_range: binding_node.location,
                },
                range,
            )
        }

        // Expression statement fallback
        _ => {
            let expr_id = lower_expr(builder, node);
            builder.alloc_stmt(HirStmt::Expr(expr_id), range)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Shared string/IO payload builders
// ──────────────────────────────────────────────────────────────────────────────
//
// Two body-expression lowerers exist: [`lower_expr`] below (reached by
// [`lower_body`]) and `Lowerer::lower_expr` in `hir::lower` (reached by
// `lower_ast`, which populates `HirFile::bodies`). They must agree exactly.
//
// These builders own the AST-field-to-payload mapping so the two call sites
// cannot drift. That is not hypothetical: the first version of this slice added
// the string/IO arms to only one lowerer, and everything reaching
// `HirFile::bodies` — which is what PIR-A actually consumes — silently kept
// emitting `HirExpr::Opaque`. Each lowerer still owns its own `alloc_expr` call.

/// Build the canonical [`HirExpr::Heredoc`] payload from a `NodeKind::Heredoc`.
pub(super) fn heredoc_expr(
    delimiter: &str,
    interpolated: bool,
    indented: bool,
    command: bool,
    body_span: Option<SourceLocation>,
) -> HirExpr {
    HirExpr::Heredoc {
        delimiter: delimiter.to_string(),
        interpolated,
        indented,
        command,
        body_range: body_span,
    }
}

/// Build the canonical [`HirExpr::Readline`] payload for `<FH>` / `<$fh>`.
pub(super) fn readline_expr(filehandle: Option<&str>) -> HirExpr {
    HirExpr::Readline {
        source: ReadlineSource::from_filehandle(filehandle),
        filehandle: filehandle.map(str::to_string),
    }
}

/// Build the canonical [`HirExpr::Readline`] payload for the `<>` / `<<>>`
/// diamond forms, which read `@ARGV` and carry no filehandle.
pub(super) fn diamond_expr() -> HirExpr {
    HirExpr::Readline { source: ReadlineSource::ArgvDiamond, filehandle: None }
}

/// Build the canonical [`HirExpr::Glob`] payload from a `NodeKind::Glob`.
pub(super) fn glob_expr(pattern: &str) -> HirExpr {
    HirExpr::Glob { pattern: pattern.to_string(), interpolated: glob_pattern_interpolates(pattern) }
}

/// Lower an assignment target. A plain variable becomes a place with the
/// requested storage kind and access mode; anything else falls back to
/// ordinary expression lowering, as this mirror does not model subscript
/// places.
fn lower_place(
    builder: &mut BodyBuilder,
    node: &Node,
    kind: VariableKind,
    access: AccessMode,
) -> HirExprId {
    match &node.kind {
        NodeKind::Variable { sigil, name } => {
            let var =
                HirVariable { sigil: Sigil::from_str(sigil), name: name.clone(), kind, access };
            builder.alloc_expr(HirExpr::Variable(var), node.location)
        }
        _ => lower_expr(builder, node),
    }
}

/// Lower an `Assignment` node. Mirrors the canonical builder: `=` writes its
/// target, every compound operator reads then writes it.
fn lower_assignment(
    builder: &mut BodyBuilder,
    node: &Node,
    lhs: &Node,
    rhs: &Node,
    op: &str,
    place_kind: VariableKind,
) -> HirExprId {
    let (mode, access) = if op == "=" {
        (AssignMode::Simple, AccessMode::Write)
    } else {
        (AssignMode::ReadModifyWrite, AccessMode::ReadModifyWrite)
    };
    let lhs_id = lower_place(builder, lhs, place_kind, access);
    let rhs_id = lower_expr(builder, rhs);
    builder.alloc_expr(HirExpr::Assign { lhs: lhs_id, rhs: rhs_id, mode }, node.location)
}

fn lower_expr(builder: &mut BodyBuilder, node: &Node) -> HirExprId {
    let range = node.location;

    match &node.kind {
        NodeKind::Variable { sigil, name } => {
            let var = HirVariable {
                sigil: Sigil::from_str(sigil),
                name: name.clone(),
                kind: VariableKind::Lexical,
                access: AccessMode::Read,
            };
            builder.alloc_expr(HirExpr::Variable(var), range)
        }

        NodeKind::Binary { op, left, right } => {
            let lhs_id = lower_expr(builder, left);
            let rhs_id = lower_expr(builder, right);
            let binary_op = BinaryOp::from_str(op);
            builder.alloc_expr(HirExpr::Binary { lhs: lhs_id, op: binary_op, rhs: rhs_id }, range)
        }

        NodeKind::Assignment { lhs, rhs, op } => {
            lower_assignment(builder, node, lhs, rhs, op, VariableKind::Lexical)
        }

        NodeKind::Heredoc { delimiter, interpolated, indented, command, body_span, .. } => builder
            .alloc_expr(
                heredoc_expr(delimiter, *interpolated, *indented, *command, *body_span),
                range,
            ),

        NodeKind::Readline { filehandle } => {
            builder.alloc_expr(readline_expr(filehandle.as_deref()), range)
        }

        NodeKind::Diamond => builder.alloc_expr(diamond_expr(), range),

        NodeKind::Glob { pattern } => builder.alloc_expr(glob_expr(pattern), range),

        _ => {
            let kind_name = node.kind.kind_name().to_string();
            builder.alloc_expr(HirExpr::Opaque { ast_kind: kind_name }, range)
        }
    }
}
