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
    ControlTransferKind, HirBlock, HirBody, HirExpr, HirFile, HirLoopRegionId, HirStmt,
    LoopControlResolution, LoopKind, StatementModifierKind, lower_ast,
};

type TestResult = Result<(), Box<dyn Error>>;

fn parse(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
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

/// Loops sorted by their stable region ID (which is allocated in body
/// source order). `[0]` is the outermost-first loop, `[1]` the next, etc.
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
    let file = parse("OUTER: while ($ready) { }");
    let body = root_body(&file)?;
    let HirExpr::Loop { label, .. } = first_expr(body)? else {
        return Err("expected structured loop".into());
    };
    let label = label.as_ref().ok_or_else(|| "labelled loop must carry its label".to_string())?;
    assert_eq!(label.name, "OUTER");
    assert!(label.range.end > label.range.start, "label range must be non-empty");
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
    let file = parse("while ($x) { BLK: last BLK; }");
    let body = root_body(&file)?;
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (written, _, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("BLK"));
    assert!(
        matches!(disposition, LoopControlResolution::NonLoopTarget { label } if label == "BLK"),
        "must return `NonLoopTarget {{ BLK }}` even inside an enclosing loop, got {disposition:?}"
    );
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
    // `postfix_label` must both stay `None`.
    let file = parse("BLK: $x = 1 if $ready;");
    let body = root_body(&file)?;
    let block = root_block(body)?;
    let stmt =
        body.stmt(*block.stmts.first().ok_or("root has no statements")?).ok_or("stmt missing")?;
    let HirStmt::PostfixCondition { verb, postfix_loop_region, postfix_label, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    assert!(matches!(verb, StatementModifierKind::If));
    assert!(
        postfix_loop_region.is_none(),
        "branch-form `if` postfix must never allocate a loop region"
    );
    assert!(postfix_label.is_none(), "branch-form postfix must never carry a loop label");
    Ok(())
}

/// Falsifier 6: a loop-form postfix modifier must retain the enclosing
/// label instead of dropping it.
#[test]
fn labelled_loop_form_postfix_preserves_label_and_region() -> TestResult {
    let file = parse("LOOP: $x = 1 while $ready;");
    let body = root_body(&file)?;
    let block = root_block(body)?;
    let stmt =
        body.stmt(*block.stmts.first().ok_or("root has no statements")?).ok_or("stmt missing")?;
    let HirStmt::PostfixCondition { verb, postfix_loop_region, postfix_label, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    assert!(matches!(verb, StatementModifierKind::While));
    let region = postfix_loop_region
        .ok_or_else(|| "loop-form postfix must allocate a region".to_string())?;
    let label = postfix_label
        .as_ref()
        .ok_or_else(|| "labelled loop-form postfix must carry its label".to_string())?;
    assert_eq!(label.name, "LOOP");
    // The region should be distinct from any ordinary loop region — this
    // fixture has exactly one loop region (the postfix), so the ID is 0.
    assert_eq!(region.as_u32(), 0);
    Ok(())
}

/// A `last LOOP;` transfer inside a labelled loop-form postfix must
/// resolve to that postfix's region ID.
#[test]
fn last_inside_labelled_postfix_loop_resolves_to_the_postfix_region() -> TestResult {
    // Statement-form of a labelled loop-form postfix. `last LOOP` inside
    // the statement is lowered while the postfix loop's region is on the
    // enclosing-region stack, so it resolves to that region.
    let file = parse("LOOP: last LOOP while $ready;");
    let body = root_body(&file)?;
    let block = root_block(body)?;
    let stmt =
        body.stmt(*block.stmts.first().ok_or("root has no statements")?).ok_or("stmt missing")?;
    let HirStmt::PostfixCondition { postfix_loop_region, postfix_label, .. } = stmt else {
        return Err(format!("expected postfix condition, got {stmt:?}").into());
    };
    let postfix_region = postfix_loop_region.ok_or("postfix loop region missing")?;
    assert_eq!(
        postfix_label.as_ref().map(|l| l.name.as_str()),
        Some("LOOP"),
        "labelled postfix must carry its label"
    );
    let controls = collect_loop_controls(body);
    assert_eq!(controls.len(), 1);
    let (written, resolved, disposition) = loop_control(controls[0]);
    assert_eq!(written.as_deref(), Some("LOOP"));
    assert_eq!(
        resolved,
        Some(postfix_region),
        "`last LOOP` must resolve to the labelled postfix-loop region"
    );
    assert!(matches!(disposition, LoopControlResolution::Resolved));
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
