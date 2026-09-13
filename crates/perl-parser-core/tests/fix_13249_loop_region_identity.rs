//! Focused proof for issue #13249 — canonical body loop regions carry stable
//! label-target identity, and `next`/`last`/`redo` statements bind to a typed
//! resolution disposition rather than dropping the label or guessing by
//! source proximity.
//!
//! The suite exercises the twelve falsifiers listed on the issue plus the
//! required fixture matrix (unlabelled/labelled transfers in nested loops,
//! same-spelled nested labels, labelled loop-form and branch-form postfix
//! modifiers, labelled bare blocks, and unresolved labels). Each test is
//! written so it fails when the model would silently drop a label, resolve
//! to the wrong region, treat a branch-form postfix as a loop target, or
//! misclassify a labelled non-loop target.
//!
//! The tests read the second-pass body HIR produced by `lower_ast` (which
//! populates `HirFile::bodies`), because that is the surface downstream PIR
//! and verifier consumers actually see.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    ControlTransferKind, HirBlock, HirBlockId, HirBody, HirExpr, HirFile, HirLoopRegionId, HirStmt,
    HirVariable, LoopControlResolution, LoopKind, StatementModifierKind, VariableKind, lower_ast,
};

type TestResult = Result<(), Box<dyn Error>>;

fn parse(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    assert!(
        output.diagnostics.is_empty(),
        "fixture must parse cleanly: {source:?}: {:?}",
        output.diagnostics
    );
    lower_ast(&output.ast)
}

fn root_body(file: &HirFile) -> Result<&HirBody, Box<dyn Error>> {
    file.root_body().ok_or_else(|| "root body is missing".to_string().into())
}

fn root_block<'a>(body: &'a HirBody) -> Result<&'a HirBlock, Box<dyn Error>> {
    body.block(body.root_block).ok_or_else(|| "root block is missing".to_string().into())
}

fn first_expr<'a>(body: &'a HirBody) -> Result<&'a HirExpr, Box<dyn Error>> {
    let block = root_block(body)?;
    let stmt_id = *block.stmts.first().ok_or_else(|| "root has no statements".to_string())?;
    let stmt = body.stmt(stmt_id).ok_or_else(|| "first statement is missing".to_string())?;
    let expr_id = match stmt {
        HirStmt::Expr(id) => *id,
        other => return Err(format!("expected expression statement, got {other:?}").into()),
    };
    body.expr(expr_id).ok_or_else(|| "first expression is missing".to_string().into())
}

/// Collect every `HirStmt::LoopControl` in `body` in body-source order.
fn collect_loop_controls(body: &HirBody) -> Vec<&HirStmt> {
    body.stmts.iter().filter(|&s| matches!(s, HirStmt::LoopControl { .. })).collect()
}

/// Collect every `HirExpr::Loop` in `body`.
///
/// The expression arena is populated in construction order (innermost child
/// first), so this returns loops in that arena order. When a test needs the
/// outer-to-inner source order, use [`loops_by_region_id`] instead — region
/// IDs are allocated in source order as loops are lowered.
fn collect_loops(body: &HirBody) -> Vec<&HirExpr> {
    body.exprs.iter().filter(|&e| matches!(e, HirExpr::Loop { .. })).collect()
}

/// Loops sorted by their stable region ID. For the sibling/nesting shapes
/// used here that coincides with source order, but region IDs follow the
/// lowerer's traversal in general — see
/// [`region_ids_are_dense_deterministic_and_outer_before_nested`].
fn loops_by_region_id(body: &HirBody) -> Vec<&HirExpr> {
    let mut loops = collect_loops(body);
    loops.sort_by_key(|e| match e {
        HirExpr::Loop { region_id, .. } => region_id.as_u32(),
        _ => u32::MAX,
    });
    loops
}

fn loop_region(expr: &HirExpr) -> HirLoopRegionId {
    match expr {
        HirExpr::Loop { region_id, .. } => *region_id,
        other => panic!("expected HirExpr::Loop, got {other:?}"),
    }
}

fn loop_control(
    stmt: &HirStmt,
) -> (&Option<String>, Option<HirLoopRegionId>, &LoopControlResolution) {
    match stmt {
        HirStmt::LoopControl { written_label, resolved_target, resolution, .. } => {
            (written_label, *resolved_target, resolution)
        }
        other => panic!("expected HirStmt::LoopControl, got {other:?}"),
    }
}

// ── §A: stable region identity on ordinary loops ────────────────────────────

/// Every loop kind lowered by the body lowerer carries a `region_id`, and
/// distinct loops in the same body allocate distinct region IDs (falsifier
/// 10 — a body-owner/region-range change must invalidate the relationship).
#[test]
fn each_loop_kind_gets_a_stable_region_id() -> TestResult {
    let file = parse(
        "\
        while ($a) { }\n\
        until ($b) { }\n\
        for (my $i = 0; $i < 10; $i++) { }\n\
        foreach my $x (@items) { }\n",
    );
    let body = root_body(&file)?;
    let loops = loops_by_region_id(body);
    assert_eq!(loops.len(), 4, "four loop kinds must each lower to a Loop node");
    let mut seen = std::collections::HashSet::new();
    for l in &loops {
        let id = loop_region(l);
        assert!(seen.insert(id), "region IDs must be distinct across loops in the same body");
    }
    // Region IDs allocated in body source order — the first loop takes 0.
    match &loops[0] {
        HirExpr::Loop { region_id, kind, .. } => {
            assert_eq!(region_id.as_u32(), 0, "first loop must allocate region 0");
            assert!(matches!(kind, LoopKind::While));
        }
        other => panic!("expected first loop, got {other:?}"),
    }
    // Fourth loop is the foreach — allocated last in source order.
    match &loops[3] {
        HirExpr::Loop { region_id, kind, .. } => {
            assert_eq!(region_id.as_u32(), 3);
            assert!(matches!(kind, LoopKind::Foreach));
        }
        other => panic!("expected fourth loop, got {other:?}"),
    }
    Ok(())
}

/// Falsifier 1: removing a loop label while preserving the loop body must
/// invalidate any assertion that binds the label to the loop.
#[test]
fn loop_without_label_has_none_label_field() -> TestResult {
    let file = parse("while ($ready) { }");
    let body = root_body(&file)?;
    let HirExpr::Loop { label, .. } = first_expr(body)? else {
        return Err("expected structured loop".into());
    };
    assert!(label.is_none(), "unlabelled loop must not synthesise a label");
    Ok(())
}

/// A `LABEL: while (...)` loop must attach the label to the loop expression
/// itself. Falsifier 2 (attaches a label to the next sibling loop) is
/// exercised negatively by [`labels_do_not_leak_to_sibling_loops`].
#[test]
fn labeled_loop_binds_label_to_loop_expr() -> TestResult {
    let source = "OUTER: while ($ready) { }";
    let file = parse(source);
    let body = root_body(&file)?;
    let HirExpr::Loop { label, .. } = first_expr(body)? else {
        return Err("expected structured loop".into());
    };
    let label = label.as_ref().ok_or_else(|| "labelled loop must carry its label".to_string())?;
    assert_eq!(label.name, "OUTER");
    assert_eq!(label.range.start, 0, "label span starts at the label token");
    assert_eq!(
        label.range.end,
        source.len(),
        "label span follows the parser's full labeled-statement range"
    );
    Ok(())
}

/// Falsifier 2: attaching a label to the next sibling loop.
#[test]
fn labels_do_not_leak_to_sibling_loops() -> TestResult {
    let file = parse("OUTER: while ($a) { } while ($b) { }");
    let body = root_body(&file)?;
    let loops = loops_by_region_id(body);
    assert_eq!(loops.len(), 2, "expected two sibling loops");
    let first = match &loops[0] {
        HirExpr::Loop { label, .. } => label.as_ref().map(|l| l.name.clone()),
        _ => unreachable!(),
    };
    let second = match &loops[1] {
        HirExpr::Loop { label, .. } => label.as_ref().map(|l| l.name.clone()),
        _ => unreachable!(),
    };
    assert_eq!(first.as_deref(), Some("OUTER"), "the labelled loop keeps its label");
    assert_eq!(second, None, "the sibling loop must NOT inherit the label");
    Ok(())
}

// ── §B: LoopControl resolution ──────────────────────────────────────────────

/// Falsifier 3: unlabelled transfer must resolve to the innermost enclosing
/// loop, not an outer one.
#[test]
fn unlabelled_next_resolves_to_innermost_loop() -> TestResult {
    let file = parse("OUTER: while ($a) { INNER: while ($b) { next; } }");
    let body = root_body(&file)?;
    let loops = loops_by_region_id(body);
    assert_eq!(loops.len(), 2, "expected outer + inner loops");
    let outer_region = loop_region(loops[0]);
    let inner_region = loop_region(loops[1]);
    assert_ne!(outer_region, inner_region);
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert!(written.is_none());
    assert_eq!(
        resolved,
        Some(inner_region),
        "unlabelled `next` inside a nested loop must resolve to the inner loop"
    );
    assert!(matches!(disposition, LoopControlResolution::Resolved));
    Ok(())
}

/// Falsifier 4: labelled transfer must resolve to the labelled outer loop
/// even when a differently-labelled inner loop is enclosing.
#[test]
fn labelled_last_resolves_across_nested_loop() -> TestResult {
    let file = parse("OUTER: while ($a) { INNER: while ($b) { last OUTER; } }");
    let body = root_body(&file)?;
    let loops = loops_by_region_id(body);
    let outer_region = loop_region(loops[0]);
    let controls = collect_loop_controls(body);
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("OUTER"), "written_label must preserve the source label");
    assert_eq!(resolved, Some(outer_region), "`last OUTER` must resolve to the outer loop");
    assert!(matches!(disposition, LoopControlResolution::Resolved));
    Ok(())
}

/// Falsifier 4 (variant): two nested same-spelled labels — the inner one
/// must win for that label, so both remain independently addressable.
#[test]
fn same_spelled_nested_labels_pick_innermost() -> TestResult {
    let file = parse("SAME: while ($a) { SAME: while ($b) { next SAME; } }");
    let body = root_body(&file)?;
    let loops = loops_by_region_id(body);
    assert_eq!(loops.len(), 2);
    let outer_region = loop_region(loops[0]);
    let inner_region = loop_region(loops[1]);
    assert_ne!(outer_region, inner_region, "two loops must not share a region ID");
    let controls = collect_loop_controls(body);
    let (_, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(
        resolved,
        Some(inner_region),
        "innermost matching label wins for `next SAME`; the outer SAME must remain unreachable through this transfer"
    );
    assert!(matches!(disposition, LoopControlResolution::Resolved));
    Ok(())
}

/// Falsifier 3 (negative): unlabelled `next` with no enclosing loop must
/// return `NoEnclosingLoop`, not silently resolve to nothing.
#[test]
fn bare_next_outside_any_loop_reports_no_enclosing_loop() -> TestResult {
    let file = parse("sub bad { next; }");
    let body = file
        .bodies
        .iter()
        .find(|b| {
            matches!(
                &b.owner,
                perl_parser_core::hir::BodyOwnerKind::Subroutine { name: Some(n) } if n == "bad"
            )
        })
        .ok_or_else(|| "sub body is missing".to_string())?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (_, resolved, disposition) = loop_control(controls[0]);
    assert!(resolved.is_none(), "no enclosing loop → no resolved target");
    assert!(
        matches!(disposition, LoopControlResolution::NoEnclosingLoop),
        "must return a typed `NoEnclosingLoop` disposition, got {disposition:?}"
    );
    Ok(())
}

/// Falsifier 7: resolves by raw string globally rather than by lexical
/// enclosure. A labelled loop that is NOT an ancestor of the transfer must
/// not be resolvable from that transfer.
#[test]
fn labelled_loop_outside_enclosure_is_unresolved() -> TestResult {
    // OUTER labels the FIRST while; the second while is a sibling that
    // contains the `next OUTER`. From that inner loop's perspective, OUTER
    // is not an enclosing region, so the transfer must NOT resolve to it.
    let file = parse("OUTER: while ($a) { } while ($b) { next OUTER; }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("OUTER"));
    assert!(resolved.is_none(), "OUTER is not an enclosing loop from this transfer");
    assert!(
        matches!(disposition, LoopControlResolution::UnresolvedLabel { label } if label == "OUTER"),
        "must return `UnresolvedLabel {{ OUTER }}`, got {disposition:?}"
    );
    Ok(())
}

/// Falsifier 9: labelled non-loop target must not be silently misclassified
/// as a loop — it must return a typed `NonLoopTarget` boundary.
#[test]
fn labelled_non_loop_statement_reports_nonloop_target() -> TestResult {
    // A labelled control-transfer whose label matches an enclosing labelled
    // non-loop statement (here, `LABEL:` wrapping the `last LABEL;` transfer
    // itself). The resolver must return `NonLoopTarget` rather than silently
    // reaching for the nearest enclosing loop.
    //
    // The construct is contrived — Perl programmers would not write it —
    // but it is the smallest AST-lowerable input that exercises the
    // `nonloop_label_stack` branch of `resolve_loop_control`. The
    // corresponding labelled-bare-block form (`BLK: { last BLK; }`) is
    // covered by [`nonloop_target_from_bare_block_when_enclosed`] once the
    // enclosing loop causes bare-block statements to be lowered.
    let file = parse("while ($x) { LABEL: last LABEL; }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("LABEL"));
    assert!(resolved.is_none(), "non-loop target must not carry a resolved loop region");
    assert!(
        matches!(disposition, LoopControlResolution::NonLoopTarget { label } if label == "LABEL"),
        "must return `NonLoopTarget {{ LABEL }}`, got {disposition:?}"
    );
    Ok(())
}

/// Companion to [`labelled_non_loop_statement_reports_nonloop_target`]: when
/// the labelled non-loop is a bare block that itself is nested inside a
/// loop, the resolver still returns `NonLoopTarget` for `last BLK` inside
/// the block. This exercise depends on the enclosing loop's body being
/// walked into its statements — the parser produces `while > body > block
/// > statements > labeled_statement > statement > block > statements >
/// last`, and the body lowerer descends the outer block.
#[test]
fn nonloop_target_from_bare_block_when_enclosed() -> TestResult {
    let file = parse("while ($x) { BLK: { last BLK; next BLK; } }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 2, "all statements in a bare block must be lowered");
    let (written, _, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("BLK"));
    assert!(
        matches!(disposition, LoopControlResolution::NonLoopTarget { label } if label == "BLK"),
        "must return `NonLoopTarget {{ BLK }}` even inside an enclosing loop, got {disposition:?}"
    );
    Ok(())
}

#[test]
fn labelled_bare_block_keeps_all_child_statements() -> TestResult {
    let file = parse("while ($x) { BLK: { next BLK; last BLK; } }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 2, "both transfers in the bare block must be lowered");
    for control in controls {
        let (_, resolved, disposition) = loop_control(control);
        assert!(resolved.is_none());
        assert!(
            matches!(disposition, LoopControlResolution::NonLoopTarget { label } if label == "BLK")
        );
    }
    Ok(())
}

/// `redo` is a loop-control verb like `next`/`last`; must resolve the same
/// way.
#[test]
fn redo_resolves_to_enclosing_loop() -> TestResult {
    let file = parse("while ($a) { redo; }");
    let body = root_body(&file)?;
    let loops = collect_loops(body);
    let region = loop_region(loops[0]);
    let controls = collect_loop_controls(body);
    let (_, resolved, disposition) = loop_control(controls[0]);
    match controls[0] {
        HirStmt::LoopControl { verb, .. } => {
            assert!(matches!(verb, ControlTransferKind::Redo));
        }
        _ => unreachable!(),
    }
    assert_eq!(resolved, Some(region));
    assert!(matches!(disposition, LoopControlResolution::Resolved));
    Ok(())
}

// ── §C: labelled postfix modifiers ──────────────────────────────────────────

/// Falsifier 5: an `if`/`unless` postfix modifier must NOT become a loop
/// target — even when the surrounding syntax carries a `LABEL:`.
#[test]
fn branch_form_postfix_never_becomes_a_loop_target() -> TestResult {
    // A labelled branch-form postfix. The label must be absorbed by the
    // labelled statement wrapper as a non-loop labelled region — the `if`
    // postfix itself must remain a non-loop, and `postfix_loop_region` /
    // must stay `None`.
    let file = parse("BLK: $x = 1 if $ready;");
    let body = root_body(&file)?;
    let block = root_block(body)?;
    let stmt =
        body.stmt(*block.stmts.first().ok_or("root has no statements")?).ok_or("stmt missing")?;
    let HirStmt::PostfixCondition { verb, postfix_loop_region, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    assert!(matches!(verb, StatementModifierKind::If));
    assert!(
        postfix_loop_region.is_none(),
        "branch-form `if` postfix must never allocate a loop region"
    );
    Ok(())
}

/// Loop-form postfix modifiers are not loop-control targets.
#[test]
fn labelled_loop_form_postfix_is_not_a_target() -> TestResult {
    let file = parse("LOOP: $x = 1 while $ready;");
    let body = root_body(&file)?;
    let block = root_block(body)?;
    let stmt =
        body.stmt(*block.stmts.first().ok_or("root has no statements")?).ok_or("stmt missing")?;
    let HirStmt::PostfixCondition { verb, postfix_loop_region, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    assert!(matches!(verb, StatementModifierKind::While));
    assert!(postfix_loop_region.is_none());
    Ok(())
}

/// A transfer inside a postfix modifier does not resolve to the modifier.
#[test]
fn last_inside_labelled_postfix_loop_does_not_create_a_target() -> TestResult {
    // A loop-form postfix is not an enclosing loop for its statement; the
    // labelled transfer therefore remains a non-loop-target disposition.
    let file = parse("LOOP: last LOOP while $ready;");
    let body = root_body(&file)?;
    let block = root_block(body)?;
    let stmt =
        body.stmt(*block.stmts.first().ok_or("root has no statements")?).ok_or("stmt missing")?;
    let HirStmt::PostfixCondition { postfix_loop_region, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    assert!(postfix_loop_region.is_none());
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("LOOP"));
    assert!(resolved.is_none());
    assert!(matches!(
        disposition,
        LoopControlResolution::NonLoopTarget { label } if label == "LOOP"
    ));
    assert!(resolved.is_none());
    Ok(())
}

#[test]
fn postfix_control_does_not_target_the_modifier() -> TestResult {
    let file = parse("last while $ready;");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (_, resolved, disposition) = loop_control(controls[0]);
    assert!(resolved.is_none());
    assert!(matches!(disposition, LoopControlResolution::NoEnclosingLoop));
    Ok(())
}

#[test]
fn c_style_initializer_control_is_outside_loop() -> TestResult {
    let file = parse("for (last; 1; ) { }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (_, resolved, disposition) = loop_control(controls[0]);
    assert!(resolved.is_none());
    assert!(matches!(disposition, LoopControlResolution::NoEnclosingLoop));
    Ok(())
}

#[test]
fn inner_same_named_bare_block_shadows_outer_loop_label() -> TestResult {
    let file = parse("OUTER: while ($x) { OUTER: { last OUTER; } }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (_, resolved, disposition) = loop_control(controls[0]);
    assert!(resolved.is_none());
    assert!(
        matches!(disposition, LoopControlResolution::NonLoopTarget { label } if label == "OUTER")
    );
    Ok(())
}

// ── §D: continue block sees its loop as the enclosing region ────────────────

/// A `next` inside a `continue { ... }` block must still resolve to the
/// loop the `continue` is attached to.
#[test]
fn next_inside_continue_block_targets_the_loop() -> TestResult {
    let file = parse("while ($a) { } continue { next; }");
    let body = root_body(&file)?;
    let loops = collect_loops(body);
    let region = loop_region(loops[0]);
    let controls = collect_loop_controls(body);
    let (_, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(resolved, Some(region), "continue-block `next` must target the loop");
    assert!(matches!(disposition, LoopControlResolution::Resolved));
    Ok(())
}

// ── §E: region-ID distinctness / cross-body isolation ───────────────────────

/// Falsifier 12: preserves a target count while duplicating one target ID
/// and omitting another. Two nested loops must never share a region ID.
#[test]
fn nested_loops_never_share_a_region_id() -> TestResult {
    let file = parse("while ($a) { while ($b) { while ($c) { } } }");
    let body = root_body(&file)?;
    let loops = collect_loops(body);
    assert_eq!(loops.len(), 3);
    let ids: std::collections::HashSet<_> = loops.iter().map(|e| loop_region(e)).collect();
    assert_eq!(ids.len(), 3, "three nested loops must produce three distinct region IDs");
    Ok(())
}

/// Region IDs are body-local: two subroutines must both start allocating
/// from 0. A cross-body region ID has no meaning.
#[test]
fn region_ids_are_body_local() -> TestResult {
    let file = parse("sub a { while ($x) { } } sub b { while ($y) { } }");
    let mut per_body_ids = Vec::new();
    for body in &file.bodies {
        for expr in body.exprs.iter() {
            if let HirExpr::Loop { region_id, .. } = expr {
                per_body_ids.push(region_id.as_u32());
            }
        }
    }
    // Each sub body has exactly one loop; both allocate region 0.
    assert_eq!(per_body_ids, vec![0, 0]);
    Ok(())
}

// ── §F: bare-block reachability, scope, and direct-label discrimination ──────

/// Every statement ID reachable by walking blocks from `body.root_block`,
/// in execution order.
///
/// Consumers (PIR lowering, graph walkers) start at `root_block` and follow
/// block statement lists; they never scan the statement arena. A test that
/// scanned `body.stmts` would therefore pass on orphaned statements, so this
/// helper deliberately walks the reachable graph instead.
fn reachable_stmts<'a>(body: &'a HirBody, block: &'a HirBlock, out: &mut Vec<&'a HirStmt>) {
    for stmt_id in &block.stmts {
        let Some(stmt) = body.stmt(*stmt_id) else { continue };
        out.push(stmt);
        match stmt {
            HirStmt::Block(nested_id) => descend(body, Some(*nested_id), out),
            HirStmt::PostfixCondition { statement, .. } => {
                if let Some(inner) = body.stmt(*statement) {
                    out.push(inner);
                    if let HirStmt::Block(nested_id) = inner {
                        descend(body, Some(*nested_id), out);
                    }
                }
            }
            // Block-carrying expressions (loop bodies, branch arms) are part
            // of the reachable graph too.
            HirStmt::Expr(expr_id) => match body.expr(*expr_id) {
                Some(HirExpr::Loop { init, body: loop_body, continue_block, .. }) => {
                    descend(body, *init, out);
                    descend(body, Some(*loop_body), out);
                    descend(body, *continue_block, out);
                }
                Some(HirExpr::Branch { then_block, elsif_arms, else_block, .. }) => {
                    descend(body, Some(*then_block), out);
                    for (_, arm) in elsif_arms {
                        descend(body, Some(*arm), out);
                    }
                    descend(body, *else_block, out);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn descend<'a>(body: &'a HirBody, block_id: Option<HirBlockId>, out: &mut Vec<&'a HirStmt>) {
    if let Some(id) = block_id
        && let Some(block) = body.block(id)
    {
        reachable_stmts(body, block, out);
    }
}

fn reachable_from_root<'a>(body: &'a HirBody) -> Result<Vec<&'a HirStmt>, Box<dyn Error>> {
    let mut out = Vec::new();
    reachable_stmts(body, root_block(body)?, &mut out);
    Ok(out)
}

/// A bare block holds a statement *sequence*, and a statement ID cannot
/// represent one. Lowering must therefore keep the block's children in the
/// block arena and link them from the statement, or statements after the
/// first become orphan arena entries that no consumer reaches.
#[test]
fn bare_block_keeps_every_statement_reachable_from_the_root_block() -> TestResult {
    let file = parse("{ my $a = 1; my $b = 2; my $c = 3; } my $d = 4;");
    let body = root_body(&file)?;
    let names: Vec<&str> = reachable_from_root(body)?
        .into_iter()
        .filter_map(|stmt| match stmt {
            HirStmt::Let { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec!["a", "b", "c", "d"],
        "every bare-block child must stay reachable from the root block, in source order"
    );
    Ok(())
}

/// The same guarantee for a labelled bare block holding a loop transfer: the
/// `last BLK` after a leading statement must still be reachable.
#[test]
fn labelled_bare_block_keeps_trailing_loop_control_reachable() -> TestResult {
    let file = parse("while ($x) { BLK: { my $seen = 1; last BLK; } }");
    let body = root_body(&file)?;
    let reachable = reachable_from_root(body)?;
    assert!(
        reachable.iter().any(|stmt| matches!(stmt, HirStmt::Let { name, .. } if name == "seen")),
        "the block's leading declaration must be reachable"
    );
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1, "the labelled block's `last BLK` must be lowered");
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("BLK"));
    assert!(resolved.is_none(), "a labelled bare block is not a loop region");
    assert!(matches!(
        disposition,
        LoopControlResolution::NonLoopTarget { label } if label == "BLK"
    ));
    Ok(())
}

/// A `my` declared inside a labelled bare block and read later in that block
/// must resolve as a lexical. Lowering the children under the parent scope
/// instead would misclassify the read as a package variable.
#[test]
fn declaration_in_a_labelled_bare_block_resolves_as_lexical() -> TestResult {
    let file = parse("BLK: { my $inner = 1; print $inner; }");
    let body = root_body(&file)?;
    let reads: Vec<&HirVariable> = body
        .exprs
        .iter()
        .filter_map(|expr| match expr {
            HirExpr::Variable(var) if var.name == "inner" => Some(var),
            _ => None,
        })
        .collect();
    assert!(!reads.is_empty(), "the `$inner` read must be lowered");
    assert!(
        reads.iter().all(|var| matches!(var.kind, VariableKind::Lexical)),
        "a block-local `my` read inside its own block must be lexical, got {reads:?}"
    );
    Ok(())
}

/// A `NonLoop` frame from a labelled *ancestor* must not be mistaken for a
/// label written on the modifier itself: both unlabelled postfix loops below
/// own distinct regions, even though `BLK:` is on the enclosing-label stack
/// the whole time they are lowered.
#[test]
fn unlabelled_postfix_loops_inside_a_labelled_block_keep_distinct_regions() -> TestResult {
    let file = parse("BLK: { $x++ while $ready; $y++ until $done; }");
    let body = root_body(&file)?;
    let regions: Vec<HirLoopRegionId> = body
        .stmts
        .iter()
        .filter_map(|stmt| match stmt {
            HirStmt::PostfixCondition { postfix_loop_region, .. } => *postfix_loop_region,
            _ => None,
        })
        .collect();
    assert_eq!(
        regions.len(),
        2,
        "both unlabelled loop-form modifiers must mint a region despite the labelled ancestor"
    );
    assert_ne!(regions[0], regions[1], "sibling postfix loops must not share a region ID");
    Ok(())
}

/// The complement: a label written *directly* on a loop-form modifier is
/// absorbed by the labelled-statement wrapper, so the modifier mints no
/// region of its own. This keeps the ancestor-vs-direct distinction honest
/// in both directions.
#[test]
fn directly_labelled_postfix_loop_still_mints_no_region() -> TestResult {
    let file = parse("LOOP: $x++ while $ready;");
    let body = root_body(&file)?;
    let regions: Vec<Option<HirLoopRegionId>> = body
        .stmts
        .iter()
        .filter_map(|stmt| match stmt {
            HirStmt::PostfixCondition { postfix_loop_region, .. } => Some(*postfix_loop_region),
            _ => None,
        })
        .collect();
    assert_eq!(regions, vec![None], "a directly-labelled loop-form modifier mints no region");
    Ok(())
}

/// The documented allocation contract for region IDs: dense from 0, unique,
/// deterministic across identical input, and allocated before a region's own
/// children so an enclosing loop always holds a lower ID than one nested in
/// it. Consumers index per-region tables by these IDs, so a gap, a duplicate,
/// or run-to-run drift would corrupt the join.
#[test]
fn region_ids_are_dense_deterministic_and_outer_before_nested() -> TestResult {
    let source = "OUTER: while ($a) { INNER: while ($b) { last OUTER; } } while ($c) { }";

    let collect = || -> Result<Vec<u32>, Box<dyn Error>> {
        let file = parse(source);
        let body = root_body(&file)?;
        let mut ids: Vec<u32> =
            collect_loops(body).iter().map(|l| loop_region(l).as_u32()).collect();
        ids.sort_unstable();
        Ok(ids)
    };

    let ids = collect()?;
    assert_eq!(ids, vec![0, 1, 2], "region IDs must be dense from 0 with no gaps or duplicates");
    assert_eq!(ids, collect()?, "identical input must yield identical region IDs");

    // A region is allocated before its own children are lowered, so the outer
    // loop holds a strictly lower ID than the loop nested inside it.
    let file = parse(source);
    let body = root_body(&file)?;
    let labelled = |wanted: &str| -> Option<u32> {
        collect_loops(body).into_iter().find_map(|l| match l {
            HirExpr::Loop { region_id, label: Some(label), .. } if label.name == wanted => {
                Some(region_id.as_u32())
            }
            _ => None,
        })
    };
    let outer = labelled("OUTER").ok_or("OUTER loop is missing")?;
    let inner = labelled("INNER").ok_or("INNER loop is missing")?;
    assert!(outer < inner, "an enclosing loop must hold a lower region ID than a nested one");
    Ok(())
}
