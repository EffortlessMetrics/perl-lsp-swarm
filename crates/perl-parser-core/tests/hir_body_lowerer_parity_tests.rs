//! Differential HIR body lowerer parity tests — regression gate for issue #5813.
//!
//! This file documents and guards the behavioural divergence between the two
//! independent HIR body lowerers that coexist in `perl-parser-core`:
//!
//! | Lowerer        | Entry point    | Module         | Used by                        |
//! |----------------|----------------|----------------|--------------------------------|
//! | `BodyBuilder`  | `lower_body()` | `hir/body.rs`  | focused slice tests (test-only)|
//! | `BodyBuilder2` | `lower_ast()`  | `hir/lower.rs` | production path: PIR-A, LSP    |
//!
//! ## Problem (issue #5813)
//!
//! The two lowerers are independent `match` traversals over the same `NodeKind`
//! producing the same output type, with no shared contract enforced between them.
//! `BodyBuilder` handles only three `NodeKind` variants in `lower_expr`; everything
//! else silently falls through to `HirExpr::Opaque`.  `BodyBuilder2` handles 13+
//! variants, including a transparent unwrapping of `ExpressionStatement` wrappers.
//! A past regression (Heredoc/Readline/Glob added to one lowerer but not the other)
//! caused PIR-A to silently receive wrong nodes.
//!
//! This file is the machine-checkable regression gate: it records exactly which
//! constructs each lowerer handles and catches any future drift.
//!
//! ## Gap inventory (state at time of issue #5813)
//!
//! **§ A — ExpressionStatement wrapper gap**
//!
//! Bare expression statements at file level are wrapped in `NodeKind::ExpressionStatement`.
//! `lower_body`'s `lower_expr` does not handle this wrapper and emits
//! `HirExpr::Opaque { "ExpressionStatement" }`.  `BodyBuilder2` transparently
//! unwraps it via `NodeKind::ExpressionStatement { expression } => self.lower_expr(expression)`.
//!
//! **§ B — Inner expression gaps (tested via `my $x = EXPR;` to bypass the wrapper)**
//!
//! Even without an `ExpressionStatement` wrapper, `lower_body`'s `lower_expr` falls
//! through to `Opaque` for these NodeKinds:
//!
//! | Perl construct (as initializer) | `lower_body` init RHS         | `BodyBuilder2` init RHS |
//! |---------------------------------|-------------------------------|-------------------------|
//! | `!$flag`                        | `Opaque { "Unary" }`          | `HirExpr::Unary`        |
//! | `$c ? $a : $b`                  | `Opaque { "Ternary" }`        | `HirExpr::Ternary`      |
//! | `foo($a)`                       | `Opaque { "FunctionCall" }`   | `HirExpr::Call`         |
//!
//! **§ C — Statement-level gaps (no ExpressionStatement wrapper)**
//!
//! `if`, `while`, and `return` appear directly as statement nodes (no wrapper),
//! so these tests precisely target the NodeKind-level gap:
//!
//! | Perl construct            | `lower_body` emits    | `BodyBuilder2` emits |
//! |---------------------------|-----------------------|----------------------|
//! | `if ($x) { … }`          | `Opaque { "If" }`     | `HirExpr::Branch`    |
//! | `while ($x) { … }`       | `Opaque { "While" }`  | `HirExpr::Loop`      |
//! | `return $x`               | `Opaque { "Return" }` | `HirExpr::Return`    |
//!
//! **§ D — Shared expression shapes and semantic divergence**
//!
//! Within a `my $x = EXPR;` initializer (no `ExpressionStatement` wrapper), both
//! lowerers agree on the expression shapes that `lower_body`'s `lower_expr` handles.
//! The unbound `$a` specimen deliberately records a semantic divergence too:
//! `lower_body` defaults it to `Lexical`, while `lower_ast` resolves it as an
//! unbound `Package` variable. The test must assert that distinction rather than
//! hiding it behind a name-only shape check.
//!
//! | Perl initializer | Both produce               |
//! |------------------|----------------------------|
//! | `$a`             | `HirExpr::Variable(Lexical)` / `HirExpr::Variable(Package)` |
//! | `$a + $b`        | `HirExpr::Binary { Add }`  |
//! | `my $x = $y`     | `HirStmt::Let` + `HirExpr::Assign { Simple }` |

use std::error::Error;

use perl_parser_core::{
    Parser,
    hir::{
        AssignMode, BinaryOp, HirBody, HirExpr, HirFile, HirStmt, VariableKind, lower_ast,
        lower_body,
    },
};

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Parse `source` and lower it through the simple `lower_body` path (BodyBuilder).
fn via_lower_body(source: &str) -> HirBody {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_body(&output.ast)
}

/// Parse `source` and lower it through the rich `lower_ast` path (BodyBuilder2).
fn via_lower_ast(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// Extract the first expression from a body's root block, expecting the first
/// statement to be `HirStmt::Expr`.
///
/// Returns `Err` when the body has no statements, the first statement is not
/// an expression statement, or the expression ID is out of range.
fn first_expr_in_body(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or_else(|| "root block is missing".to_string())?;
    let stmt_id = root.stmts.first().ok_or_else(|| "root block has no statements".to_string())?;
    let stmt =
        body.stmt(*stmt_id).ok_or_else(|| "first statement is missing from arena".to_string())?;
    let expr_id = match stmt {
        HirStmt::Expr(id) => *id,
        other => return Err(format!("expected HirStmt::Expr, got {other:?}").into()),
    };
    body.expr(expr_id).ok_or_else(|| "first expression is missing from arena".to_string().into())
}

/// Extract the RHS of the synthesized `Assign` inside a `HirStmt::Let` initializer.
///
/// For `my $x = EXPR;`, both lowerers build:
/// - `HirStmt::Let { init: Some(assign_id), … }`
/// - `HirExpr::Assign { lhs: Variable($x, Write), rhs: rhs_id, … }` at `assign_id`
/// - The lowered form of `EXPR` at `rhs_id`
///
/// This helper follows that chain and returns the expression at `rhs_id`, which is
/// the lowered representation of `EXPR` — the form that differs between the two
/// lowerers for Unary / Ternary / Call.
fn let_init_rhs(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
    let root = body.block(body.root_block).ok_or_else(|| "root block is missing".to_string())?;
    let stmt_id = root.stmts.first().ok_or_else(|| "root block has no statements".to_string())?;
    let stmt =
        body.stmt(*stmt_id).ok_or_else(|| "first statement is missing from arena".to_string())?;
    let init_id = match stmt {
        HirStmt::Let { init, .. } => {
            (*init).ok_or_else(|| "Let statement has no initializer".to_string())?
        }
        other => return Err(format!("expected HirStmt::Let, got {other:?}").into()),
    };
    let assign = body
        .expr(init_id)
        .ok_or_else(|| "Let initializer expression is missing from arena".to_string())?;
    let rhs_id = match assign {
        HirExpr::Assign { rhs, .. } => *rhs,
        other => return Err(format!("expected HirExpr::Assign as Let init, got {other:?}").into()),
    };
    body.expr(rhs_id)
        .ok_or_else(|| "Assign RHS expression is missing from arena".to_string().into())
}

// ══════════════════════════════════════════════════════════════════════════════
// § A  ExpressionStatement wrapper gap
//
// Bare expression statements at file level arrive as
// `NodeKind::ExpressionStatement { expression }`.  lower_body's lower_expr does
// not match this wrapper, so the whole statement collapses to
// `Opaque { "ExpressionStatement" }`.  BodyBuilder2 has an explicit arm that
// unwraps the wrapper and delegates to the inner expression.
// ══════════════════════════════════════════════════════════════════════════════

/// `$a;` at file level — the AST wraps the variable reference in an
/// `ExpressionStatement` node.  `lower_body` does not handle that wrapper and
/// emits `HirExpr::Opaque`.  BodyBuilder2 unwraps the statement and emits
/// `HirExpr::Variable`.
///
/// This is the root cause behind why `$a;`, `$a + $b;`, and similar bare
/// expression statements all produce `Opaque` in `lower_body`.
#[test]
fn gap_expression_stmt_wrapper_is_opaque_in_lower_body_but_unwrapped_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "$a;";

    // lower_body: ExpressionStatement falls through lower_expr → Opaque
    let body = via_lower_body(source);
    let lb_expr = first_expr_in_body(&body)?;
    assert!(
        matches!(lb_expr, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for bare `$a;` (ExpressionStatement wrapper not \
         handled by lower_expr), got {lb_expr:?}\n\
         (If this fails, lower_body now handles ExpressionStatement — update §A above)"
    );

    // BodyBuilder2: ExpressionStatement unwrapped → Variable
    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_expr = first_expr_in_body(bb2_body)?;
    assert!(
        matches!(bb2_expr, HirExpr::Variable(v) if v.name == "a"),
        "BodyBuilder2 must emit HirExpr::Variable('a') for `$a;`, got {bb2_expr:?}\n\
         (If this fails, the production path regressed — ExpressionStatement is no longer unwrapped)"
    );
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
// § B  Inner expression gaps — tested via `my $x = EXPR;` to bypass the wrapper
//
// Inside a VariableDeclaration, the AST delivers the initializer expression
// directly to lower_expr (no ExpressionStatement wrapper).  These tests therefore
// precisely target the NodeKind-level gap in lower_body's lower_expr match arms.
// ══════════════════════════════════════════════════════════════════════════════

/// `my $x = !$flag;` — the initializer `!$flag` is `NodeKind::Unary { … }`.
/// `lower_body`'s `lower_expr` has no arm for Unary and falls through to `Opaque`.
/// BodyBuilder2 emits `HirExpr::Unary`.
#[test]
fn gap_unary_in_initializer_is_opaque_in_lower_body_but_structured_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "my $x = !$flag;";

    // lower_body: Unary node falls through lower_expr → Opaque
    let body = via_lower_body(source);
    let lb_rhs = let_init_rhs(&body)?;
    assert!(
        matches!(lb_rhs, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for Unary initializer `!$flag`, got {lb_rhs:?}\n\
         (If this fails, lower_body now handles Unary — update §B gap table above)"
    );

    // BodyBuilder2: Unary explicitly handled
    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_rhs = let_init_rhs(bb2_body)?;
    assert!(
        matches!(bb2_rhs, HirExpr::Unary { .. }),
        "BodyBuilder2 must emit HirExpr::Unary for `!$flag` initializer, got {bb2_rhs:?}\n\
         (If this fails, the production path regressed — Unary initializers no longer structured)"
    );
    Ok(())
}

/// `my $x = $c ? $a : $b;` — the initializer is `NodeKind::Ternary { … }`.
/// `lower_body` has no arm for Ternary and falls through to `Opaque`.
/// BodyBuilder2 emits `HirExpr::Ternary`.
#[test]
fn gap_ternary_in_initializer_is_opaque_in_lower_body_but_structured_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "my $x = $c ? $a : $b;";

    let body = via_lower_body(source);
    let lb_rhs = let_init_rhs(&body)?;
    assert!(
        matches!(lb_rhs, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for Ternary initializer `$c ? $a : $b`, \
         got {lb_rhs:?}\n\
         (If this fails, lower_body now handles Ternary — update §B gap table above)"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_rhs = let_init_rhs(bb2_body)?;
    assert!(
        matches!(bb2_rhs, HirExpr::Ternary { .. }),
        "BodyBuilder2 must emit HirExpr::Ternary for `$c ? $a : $b` initializer, \
         got {bb2_rhs:?}"
    );
    Ok(())
}

/// `my $x = foo($a);` — the initializer is `NodeKind::FunctionCall { … }`.
/// `lower_body` has no arm for FunctionCall and falls through to `Opaque`.
/// BodyBuilder2 emits `HirExpr::Call`.
#[test]
fn gap_function_call_in_initializer_is_opaque_in_lower_body_but_structured_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "my $x = foo($a);";

    let body = via_lower_body(source);
    let lb_rhs = let_init_rhs(&body)?;
    assert!(
        matches!(lb_rhs, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for FunctionCall initializer `foo($a)`, \
         got {lb_rhs:?}\n\
         (If this fails, lower_body now handles FunctionCall — update §B gap table above)"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_rhs = let_init_rhs(bb2_body)?;
    assert!(
        matches!(bb2_rhs, HirExpr::Call { .. }),
        "BodyBuilder2 must emit HirExpr::Call for `foo($a)` initializer, got {bb2_rhs:?}"
    );
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
// § C  Statement-level gaps — no ExpressionStatement wrapper
//
// `if`, `while`, and `return` appear as bare statement nodes in the AST (the
// parser does not wrap them in ExpressionStatement).  These tests therefore
// precisely test the NodeKind-level gap in lower_body's lower_expr match.
// ══════════════════════════════════════════════════════════════════════════════

/// `if ($flag) { $x = 1; }` — `NodeKind::If` is parsed as a bare statement node
/// (no ExpressionStatement wrapper).  `lower_body`'s `lower_expr` has no arm for
/// it and falls through to `Opaque { "If" }`.  BodyBuilder2 emits `HirExpr::Branch`.
#[test]
fn gap_if_branch_at_stmt_level_is_opaque_in_lower_body_but_structured_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "if ($flag) { $x = 1; }";

    let body = via_lower_body(source);
    let lb_expr = first_expr_in_body(&body)?;
    assert!(
        matches!(lb_expr, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for `if ($flag) {{ … }}`, got {lb_expr:?}\n\
         (If this fails, lower_body now handles If — update §C gap table above)"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_expr = first_expr_in_body(bb2_body)?;
    assert!(
        matches!(bb2_expr, HirExpr::Branch { .. }),
        "BodyBuilder2 must emit HirExpr::Branch for `if ($flag) {{ … }}`, got {bb2_expr:?}"
    );
    Ok(())
}

/// `while ($ready) { $x = 1; }` — `NodeKind::While` is a bare statement node.
/// `lower_body` has no arm for it and falls through to `Opaque { "While" }`.
/// BodyBuilder2 emits `HirExpr::Loop`.
#[test]
fn gap_while_loop_at_stmt_level_is_opaque_in_lower_body_but_structured_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "while ($ready) { $x = 1; }";

    let body = via_lower_body(source);
    let lb_expr = first_expr_in_body(&body)?;
    assert!(
        matches!(lb_expr, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for `while ($ready) {{ … }}`, got {lb_expr:?}\n\
         (If this fails, lower_body now handles While — update §C gap table above)"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_expr = first_expr_in_body(bb2_body)?;
    assert!(
        matches!(bb2_expr, HirExpr::Loop { .. }),
        "BodyBuilder2 must emit HirExpr::Loop for `while ($ready) {{ … }}`, got {bb2_expr:?}"
    );
    Ok(())
}

/// `return $x;` — `NodeKind::Return` is a bare statement node (not wrapped in
/// `ExpressionStatement`).  `lower_body` has no arm for it in `lower_expr` and
/// falls through to `Opaque { "Return" }`.  BodyBuilder2 emits `HirExpr::Return`.
///
/// `return` at file level is syntactically valid Perl; both lowerers encounter
/// the node at program root.
#[test]
fn gap_return_at_stmt_level_is_opaque_in_lower_body_but_structured_in_bb2()
-> Result<(), Box<dyn Error>> {
    let source = "return $x;";

    let body = via_lower_body(source);
    let lb_expr = first_expr_in_body(&body)?;
    assert!(
        matches!(lb_expr, HirExpr::Opaque { .. }),
        "lower_body must emit HirExpr::Opaque for `return $x`, got {lb_expr:?}\n\
         (If this fails, lower_body now handles Return — update §C gap table above)"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_expr = first_expr_in_body(bb2_body)?;
    assert!(
        matches!(bb2_expr, HirExpr::Return { value: Some(_) }),
        "BodyBuilder2 must emit HirExpr::Return {{ value: Some(_) }} for `return $x`, \
         got {bb2_expr:?}"
    );
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
// § D  Parity zone — constructs both lowerers handle identically
//
// Within a `my $x = EXPR;` initializer (no ExpressionStatement wrapper), both
// lowerers agree on the three NodeKinds that lower_body's lower_expr explicitly
// handles: Variable, Binary, and the synthesized Assign for declarations.
//
// The tests access the init RHS via `let_init_rhs()` to bypass the outer Assign
// synthesized by both lowerers for the declared place.
// ══════════════════════════════════════════════════════════════════════════════

/// `my $x = $a;` — both lowerers produce a variable-shaped initializer RHS, but
/// they disagree on the semantic kind of the unbound `$a`: `lower_body`
/// defaults to `Lexical`, while `BodyBuilder2` resolves it as `Package`.
#[test]
fn variable_kind_divergence_in_let_initializer_is_explicit() -> Result<(), Box<dyn Error>> {
    let source = "my $x = $a;";

    let body = via_lower_body(source);
    let lb_rhs = let_init_rhs(&body)?;
    assert!(
        matches!(lb_rhs, HirExpr::Variable(v) if v.name == "a" && v.kind == VariableKind::Lexical),
        "lower_body must record unbound `$a` as Lexical in this legacy path, got {lb_rhs:?}"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_rhs = let_init_rhs(bb2_body)?;
    assert!(
        matches!(bb2_rhs, HirExpr::Variable(v) if v.name == "a" && v.kind == VariableKind::Package),
        "BodyBuilder2 must resolve unbound `$a` as Package, got {bb2_rhs:?}"
    );
    Ok(())
}

/// `my $x = $a + $b;` — both lowerers produce `HirExpr::Binary { op: Add, … }`
/// for the `$a + $b` initializer RHS.
#[test]
fn parity_binary_add_in_let_initializer_both_produce_binary_node() -> Result<(), Box<dyn Error>> {
    let source = "my $x = $a + $b;";

    let body = via_lower_body(source);
    let lb_rhs = let_init_rhs(&body)?;
    assert!(
        matches!(lb_rhs, HirExpr::Binary { op: BinaryOp::Add, .. }),
        "lower_body must produce HirExpr::Binary {{ op: Add }} as Let init RHS, got {lb_rhs:?}"
    );

    let file = via_lower_ast(source);
    let bb2_body =
        file.root_body().ok_or_else(|| "lower_ast must produce a root body".to_string())?;
    let bb2_rhs = let_init_rhs(bb2_body)?;
    assert!(
        matches!(bb2_rhs, HirExpr::Binary { op: BinaryOp::Add, .. }),
        "BodyBuilder2 must produce HirExpr::Binary {{ op: Add }} as Let init RHS, got {bb2_rhs:?}"
    );
    Ok(())
}

/// `my $x = $y;` — both lowerers produce an `HirStmt::Let` whose `init` is a
/// synthesized `HirExpr::Assign { Simple }`.
///
/// The `=` assignment node is not present in the AST for `VariableDeclaration`;
/// both builders synthesize it explicitly to link the declared place to the RHS.
#[test]
fn parity_let_declaration_both_produce_let_stmt_with_assign_init() -> Result<(), Box<dyn Error>> {
    let source = "my $x = $y;";

    // ── lower_body ──────────────────────────────────────────────────────────
    {
        let body = via_lower_body(source);
        let root = body
            .block(body.root_block)
            .ok_or_else(|| "lower_body: root block must exist".to_string())?;
        let stmt_id = root
            .stmts
            .first()
            .ok_or_else(|| "lower_body: root block must have a statement".to_string())?;
        let stmt = body
            .stmt(*stmt_id)
            .ok_or_else(|| "lower_body: first stmt must be in arena".to_string())?;

        let init_id = match stmt {
            HirStmt::Let { name, init, .. } => {
                assert_eq!(name.as_str(), "x", "lower_body: declared name must be 'x'");
                (*init).ok_or_else(|| {
                    "lower_body: `my $x = $y` must have a Let initializer".to_string()
                })?
            }
            other => {
                return Err(format!(
                    "lower_body must produce HirStmt::Let for `my $x = $y`, got {other:?}"
                )
                .into());
            }
        };

        let init_expr = body
            .expr(init_id)
            .ok_or_else(|| "lower_body: Let init expr must be in arena".to_string())?;
        assert!(
            matches!(init_expr, HirExpr::Assign { mode: AssignMode::Simple, .. }),
            "lower_body: Let init must be HirExpr::Assign {{ Simple }}, got {init_expr:?}"
        );
    }

    // ── BodyBuilder2 ────────────────────────────────────────────────────────
    {
        let file = via_lower_ast(source);
        let bb2_body =
            file.root_body().ok_or_else(|| "lower_ast: must produce a root body".to_string())?;
        let root = bb2_body
            .block(bb2_body.root_block)
            .ok_or_else(|| "BodyBuilder2: root block must exist".to_string())?;
        let stmt_id = root
            .stmts
            .first()
            .ok_or_else(|| "BodyBuilder2: root block must have a statement".to_string())?;
        let stmt = bb2_body
            .stmt(*stmt_id)
            .ok_or_else(|| "BodyBuilder2: first stmt must be in arena".to_string())?;

        let init_id = match stmt {
            HirStmt::Let { name, init, .. } => {
                assert_eq!(name.as_str(), "x", "BodyBuilder2: declared name must be 'x'");
                (*init).ok_or_else(|| {
                    "BodyBuilder2: `my $x = $y` must have a Let initializer".to_string()
                })?
            }
            other => {
                return Err(format!(
                    "BodyBuilder2 must produce HirStmt::Let for `my $x = $y`, got {other:?}"
                )
                .into());
            }
        };

        let init_expr = bb2_body
            .expr(init_id)
            .ok_or_else(|| "BodyBuilder2: Let init expr must be in arena".to_string())?;
        assert!(
            matches!(init_expr, HirExpr::Assign { mode: AssignMode::Simple, .. }),
            "BodyBuilder2: Let init must be HirExpr::Assign {{ Simple }}, got {init_expr:?}"
        );
    }
    Ok(())
}
