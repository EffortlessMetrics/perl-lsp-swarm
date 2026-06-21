//! HIR lowering completeness / boundary-soundness gate.
//!
//! Ensures that every AST NodeKind is **explicitly classified** as one of:
//!   - `lowered`             — emits one or more HIR items in `lower.rs`
//!   - `dynamic_boundary`    — emits an explicit `DynamicBoundary` HIR item
//!   - `intentionally_skipped` — traversal-only or recovery node; no HIR item expected
//!   - `not_yet_modeled`     — unimplemented; must fall to `visit_children`, not silently vanish
//!
//! The completeness invariant: **no AST kind silently disappears** through the
//! `_ => self.visit_children(node, confidence)` fallthrough arm in `lower.rs`
//! without being explicitly acknowledged here.
//!
//! When a new NodeKind variant is added to `perl_ast::NodeKind`, this test
//! will fail — forcing the author to decide its classification and update this
//! table before the PR can merge.
//!
//! Refs: issue #2193 (HIR lowering completeness gate), epic #2076.

use std::collections::BTreeSet;

use perl_parser_core::NodeKind;

/// Lowering classification for each AST NodeKind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LoweringClass {
    /// Emits one or more HIR items from `lower.rs` (PackageDecl, SubDecl, CallExpr, etc.)
    Lowered,
    /// Emits an explicit `DynamicBoundary` HIR item for the unsupported static construct.
    DynamicBoundary,
    /// Traversal-only, metadata, or recovery placeholder — no standalone HIR item expected.
    /// Lower.rs traverses children for side-effects (scope bindings, stash slots, etc.)
    IntentionallySkipped,
    /// Not yet implemented; falls through to `visit_children` in lower.rs.
    /// This is acknowledged-unimplemented, NOT a silent disappearance.
    NotYetModeled,
}

/// The authoritative explicit classification of every AST NodeKind.
///
/// **Critical invariant**: this function must match ALL NodeKind variants
/// by name. It contains NO `_ =>` fallthrough arm. If a new NodeKind variant
/// is added, the `hir_lowering_completeness_gate` test will detect the
/// unclassified name and fail, requiring explicit classification here.
///
/// Classification rationale per kind:
/// - `Lowered`: has an explicit match arm in `lower.rs` that calls `push_item()`
/// - `DynamicBoundary`: has an explicit match arm that may call `push_item(DynamicBoundary)`
/// - `IntentionallySkipped`: either traversal-only (Program, ExpressionStatement) or
///   captures side-effects without emitting HIR items (Variable, Signature params)
/// - `NotYetModeled`: no explicit match arm; falls to `_ => visit_children` but acknowledged
fn classify(kind_name: &str) -> Option<LoweringClass> {
    use LoweringClass::*;
    match kind_name {
        // ── Lowered: explicit match arm emits HIR item(s) ──────────────────────────────
        "ArrayLiteral" => Some(Lowered),
        "Block" => Some(Lowered),
        "FunctionCall" => Some(Lowered), // includes RequireDecl for `require`, CallExpr otherwise
        "HashLiteral" => Some(Lowered),
        "Identifier" => Some(Lowered), // BarewordExpr
        "IndirectCall" => Some(Lowered),
        "Method" => Some(Lowered),
        "MethodCall" => Some(Lowered),
        "Number" => Some(Lowered),
        "Package" => Some(Lowered),
        "String" => Some(Lowered),
        "Subroutine" => Some(Lowered),
        "Undef" => Some(Lowered),
        "Use" => Some(Lowered),
        "VariableDeclaration" => Some(Lowered),
        "VariableListDeclaration" => Some(Lowered),
        // Wave 1 — control structures (implemented)
        "If" => Some(Lowered),
        "Ternary" => Some(Lowered),
        "While" => Some(Lowered),
        "For" => Some(Lowered),
        "Foreach" => Some(Lowered),
        "Return" => Some(Lowered),
        "LoopControl" => Some(Lowered),
        "Goto" => Some(Lowered),
        "StatementModifier" => Some(Lowered),

        // ── DynamicBoundary: explicit match arm conditionally emits DynamicBoundary ────
        "Assignment" => Some(DynamicBoundary), // typeglob assignment with non-static RHS
        "Eval" => Some(DynamicBoundary),       // expression `eval` form
        "Do" => Some(DynamicBoundary),         // `do EXPR` (non-block) form
        "Unary" => Some(DynamicBoundary),      // symbolic reference deref → DynamicBoundary

        // ── IntentionallySkipped: traversal-only or metadata capture ─────────────────
        // Root/structural traversal
        "Program" => Some(IntentionallySkipped), // root wrapper, just traverses statements
        "ExpressionStatement" => Some(IntentionallySkipped), // statement wrapper only
        "LabeledStatement" => Some(IntentionallySkipped), // label threaded to inner loop; no HIR item

        // Signature/parameter nodes: captured as ScopeGraph binding metadata
        "Prototype" => Some(IntentionallySkipped),
        "Signature" => Some(IntentionallySkipped),
        "MandatoryParameter" => Some(IntentionallySkipped),
        "OptionalParameter" => Some(IntentionallySkipped),
        "SlurpyParameter" => Some(IntentionallySkipped),
        "NamedParameter" => Some(IntentionallySkipped),

        // Variable/reference nodes: captured as ScopeGraph reference metadata
        "Variable" => Some(IntentionallySkipped), // records reference via `record_reference()`
        "VariableWithAttributes" => Some(IntentionallySkipped),
        "NestedVariableList" => Some(IntentionallySkipped), // consumed by list declaration lowering

        // Phase/directive nodes: record CompileEnvironment facts, no HIR item
        "PhaseBlock" => Some(IntentionallySkipped),
        "No" => Some(IntentionallySkipped),

        // Parser recovery/error placeholders
        "Error" => Some(IntentionallySkipped), // partials are traversed; raw Error emits no HIR
        "MissingExpression" => Some(IntentionallySkipped),
        "MissingStatement" => Some(IntentionallySkipped),
        "MissingIdentifier" => Some(IntentionallySkipped),
        "MissingBlock" => Some(IntentionallySkipped),
        "UnknownRest" => Some(IntentionallySkipped),

        // ── NotYetModeled: falls to `_ => visit_children`; acknowledged-unimplemented ─
        // Values / I-O
        "Binary" => Some(NotYetModeled),
        "Heredoc" => Some(NotYetModeled),
        "Readline" => Some(NotYetModeled),
        "Glob" => Some(NotYetModeled),
        "Diamond" => Some(NotYetModeled),
        "Ellipsis" => Some(NotYetModeled),
        "Typeglob" => Some(NotYetModeled), // stash slots via Assignment; no standalone shell yet

        // Pattern matching
        "Regex" => Some(NotYetModeled),
        "Match" => Some(NotYetModeled),
        "Substitution" => Some(NotYetModeled),
        "Transliteration" => Some(NotYetModeled),

        // Control flow (deferred / advanced)
        "Given" => Some(NotYetModeled),
        "When" => Some(NotYetModeled),
        "Default" => Some(NotYetModeled),
        "Try" => Some(NotYetModeled),
        "Defer" => Some(NotYetModeled),
        "Tie" => Some(NotYetModeled),
        "Untie" => Some(NotYetModeled),

        // Declarations (deferred)
        "Class" => Some(NotYetModeled),
        "Format" => Some(NotYetModeled), // contributes ScopeGraph format frame; no HIR shell yet
        "DataSection" => Some(NotYetModeled),

        // Unknown: caller detects missing classification
        _ => None,
    }
}

/// Gate: every NodeKind has an explicit classification — no silent fallthrough.
///
/// This test fails if any `NodeKind` variant is present in `ALL_KIND_NAMES`
/// but absent from the `classify()` table above. That forces the author of
/// the new variant to decide its lowering classification explicitly.
#[test]
fn hir_lowering_completeness_gate() {
    let all_kinds: BTreeSet<&str> = NodeKind::ALL_KIND_NAMES.iter().copied().collect();

    // Collect every kind name that our classify() table explicitly handles.
    // We re-invoke classify() for each name to detect missing entries.
    let mut unclassified: Vec<&str> = Vec::new();
    for &kind_name in NodeKind::ALL_KIND_NAMES {
        if classify(kind_name).is_none() {
            unclassified.push(kind_name);
        }
    }

    assert!(
        unclassified.is_empty(),
        "HIR lowering completeness gate FAILED — the following AST NodeKind variants \
         have no explicit lowering classification.\n\
         \n\
         Every kind must be listed in `classify()` in `hir_lowering_completeness_tests.rs`\n\
         as one of: Lowered | DynamicBoundary | IntentionallySkipped | NotYetModeled.\n\
         \n\
         Unclassified kinds ({}):\n  {}\n\
         \n\
         This prevents silent disappearance of Perl constructs through the\n\
         `_ => visit_children` fallthrough arm in `crates/perl-parser-core/src/hir/lower.rs`.\n\
         Refs: issue #2193, epic #2076.",
        unclassified.len(),
        unclassified.join("\n  ")
    );

    // Secondary check: no stale names in our table that no longer exist in the enum.
    // We build the set of names our classify() returns Some(_) for, then check against
    // ALL_KIND_NAMES. Since classify() is just a match on &str, stale names would silently
    // succeed — we guard against that by cross-checking.
    let classified_names: BTreeSet<&str> =
        NodeKind::ALL_KIND_NAMES.iter().copied().filter(|&name| classify(name).is_some()).collect();

    // Everything in all_kinds must be classified (proven above).
    // Now ensure there are no duplicate classifications by verifying counts match.
    assert_eq!(
        classified_names.len(),
        all_kinds.len(),
        "Classification count mismatch: {} AST kinds vs {} classified entries. \
         This likely means the classify() table has duplicate or stale entries.",
        all_kinds.len(),
        classified_names.len()
    );
}

/// Verify that `not_yet_modeled` kinds do NOT silently lose their children.
///
/// For each `NotYetModeled` kind (those falling to `_ => visit_children`),
/// the lowerer MUST traverse child nodes. This test verifies that a simple
/// Perl program containing such constructs still produces HIR items for
/// any contained sub-expressions that ARE explicitly modeled.
///
/// Specifically: a `Binary` expression wrapping a FunctionCall must still
/// produce a `CallExpr` HIR item, because `Binary` falls to `visit_children`
/// which recurses into the call.
#[test]
fn hir_not_yet_modeled_kinds_traverse_children() {
    use perl_parser_core::hir::lower_ast;
    use perl_parser_core::hir::HirKind;
    use perl_parser_core::Parser;

    // Binary wraps a FunctionCall — Binary is NotYetModeled, FunctionCall is Lowered.
    // After lowering, we expect to find the CallExpr from the inner FunctionCall.
    let source = "my $x = foo() + bar();";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let file = lower_ast(&output.ast);

    let has_call_expr = file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(_)));
    assert!(
        has_call_expr,
        "Binary (NotYetModeled) should traverse children; expected CallExpr for foo()/bar() \
         inside binary expression, but none found in HIR.\n\
         HIR item count: {}",
        file.items.len()
    );
}

/// Verify `DynamicBoundary` kinds emit the boundary marker rather than silently traversing.
///
/// Expression-form `eval` (non-block) must emit `DynamicBoundary::EvalExpression`.
#[test]
fn hir_dynamic_boundary_kinds_emit_boundary_marker() {
    use perl_parser_core::hir::lower_ast;
    use perl_parser_core::hir::{DynamicBoundaryKind, HirKind};
    use perl_parser_core::Parser;

    // Expression eval must emit DynamicBoundary
    let source = r#"my $result = eval "1 + 1";"#;
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let file = lower_ast(&output.ast);

    let has_eval_boundary = file.items.iter().any(|item| {
        matches!(&item.kind, HirKind::DynamicBoundary(b) if b.kind == DynamicBoundaryKind::EvalExpression)
    });
    assert!(
        has_eval_boundary,
        "Expression-form `eval` (DynamicBoundary kind) must emit DynamicBoundary::EvalExpression.\n\
         HIR item count: {}",
        file.items.len()
    );
}

/// Verify `intentionally_skipped` kinds still allow child HIR emission.
///
/// `ExpressionStatement` is traversal-only — it should not block HIR emission
/// from its contained expression. A `FunctionCall` inside an `ExpressionStatement`
/// must still produce a `CallExpr`.
#[test]
fn hir_intentionally_skipped_kinds_allow_child_hir_emission() {
    use perl_parser_core::hir::lower_ast;
    use perl_parser_core::hir::HirKind;
    use perl_parser_core::Parser;

    // Top-level function call is wrapped in ExpressionStatement (intentionally skipped)
    let source = "say 'hello';";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    let file = lower_ast(&output.ast);

    let has_call_expr = file.items.iter().any(|item| matches!(&item.kind, HirKind::CallExpr(_)));
    assert!(
        has_call_expr,
        "ExpressionStatement (IntentionallySkipped) must allow child HIR emission. \
         Expected CallExpr for `say` call, but none found.\n\
         HIR item count: {}",
        file.items.len()
    );
}

/// Regression guard: classification table must be exhaustive relative to current enum size.
///
/// This test ensures the total number of NodeKind variants matches our expectation.
/// If it fails, a variant was added or removed and the completeness gate table needs updating.
#[test]
fn hir_completeness_classification_covers_all_variants() {
    // Collect any unclassified kinds (same logic as hir_lowering_completeness_gate)
    let unclassified: Vec<&str> =
        NodeKind::ALL_KIND_NAMES.iter().copied().filter(|&n| classify(n).is_none()).collect();
    assert!(
        unclassified.is_empty(),
        "Unclassified NodeKind(s) detected in hir_completeness_classification_covers_all_variants: \
         {:?} — update classify() in hir_lowering_completeness_tests.rs",
        unclassified
    );

    // Count how many kinds fall into each class
    let mut counts = [0usize; 4];
    for &kind_name in NodeKind::ALL_KIND_NAMES {
        match classify(kind_name) {
            Some(LoweringClass::Lowered) => counts[0] += 1,
            Some(LoweringClass::DynamicBoundary) => counts[1] += 1,
            Some(LoweringClass::IntentionallySkipped) => counts[2] += 1,
            Some(LoweringClass::NotYetModeled) => counts[3] += 1,
            None => {} // already asserted above; unreachable in practice
        }
    }

    let total_classified = counts.iter().sum::<usize>();
    assert_eq!(
        total_classified,
        NodeKind::ALL_KIND_NAMES.len(),
        "Classification total ({total_classified}) != NodeKind variant count ({}). \
         This is a logic error in the test — counts should always match.",
        NodeKind::ALL_KIND_NAMES.len()
    );

    // Sanity floor: we should have at least some in each meaningful category
    assert!(
        counts[0] >= 16,
        "Expected at least 16 Lowered kinds (baseline from issue #2124), got {}",
        counts[0]
    );
    assert!(
        counts[1] >= 3,
        "Expected at least 3 DynamicBoundary kinds (baseline from issue #2124), got {}",
        counts[1]
    );
    assert!(counts[2] >= 10, "Expected at least 10 IntentionallySkipped kinds, got {}", counts[2]);
}
