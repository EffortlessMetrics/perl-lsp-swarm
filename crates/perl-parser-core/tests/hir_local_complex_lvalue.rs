//! Complex-lvalue `local` must not invent a placeholder `<unknown>` binding.
//!
//! #14809: `local $ENV{PATH} = '/bin'` and `foo(local($ENV{PATH}) = '/bin')`
//! are not named-variable declarations. Canonical body HIR must lower the real
//! target place (element, slice, or fail-closed opaque) and PIR-A must never
//! emit `StashWrite { name: "<unknown>" }`. Named `local $x` stays a `Let`.
//! Typeglob `local *FH` and arrow-postfix `my $cache->{key}` recover the real
//! declared name; arrow-element `local $obj->{key}` does not bind `$obj`.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::hir::{
    AccessMode, AssignMode, DeclStorageClass, HirBody, HirExpr, HirKind, HirStmt, Sigil,
    SubscriptKind, lower_ast, lower_body,
};
use perl_parser_core::pir::{PirGraph, PirOperation, lower_hir_bodies};
use perl_parser_core::{Parser, hir::HirFile};

const PLACEHOLDER: &str = "<unknown>";

fn canonical_body(source: &str) -> Result<HirFile, String> {
    assert_clean_parse(source);
    Ok(lower_ast(&parse(source)))
}

fn body_of(file: &HirFile) -> Result<&HirBody, String> {
    file.root_body().ok_or_else(|| "expected canonical production root body".to_string())
}

fn pir_of(file: &HirFile) -> PirGraph {
    lower_hir_bodies(file)
}

fn placeholder_lets(body: &HirBody) -> Vec<String> {
    body.stmts
        .iter()
        .filter_map(|stmt| match stmt {
            HirStmt::Let { name, .. } if name == PLACEHOLDER => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn placeholder_variables(body: &HirBody) -> usize {
    body.exprs
        .iter()
        .filter(|expr| matches!(expr, HirExpr::Variable(variable) if variable.name == PLACEHOLDER))
        .count()
}

fn placeholder_pir_symbols(graph: &PirGraph) -> Vec<&'static str> {
    graph
        .nodes
        .iter()
        .filter_map(|node| match &node.operation {
            PirOperation::StashWrite { symbol }
            | PirOperation::StashRead { symbol }
            | PirOperation::StashModify { symbol, .. }
                if symbol.name == PLACEHOLDER =>
            {
                Some(node.operation.name())
            }
            PirOperation::LexicalWrite { name } | PirOperation::LexicalRead { name }
                if name.name == PLACEHOLDER =>
            {
                Some(node.operation.name())
            }
            _ => None,
        })
        .collect()
}

fn require_no_placeholder(body: &HirBody, graph: &PirGraph, source: &str) -> Result<(), String> {
    let lets = placeholder_lets(body);
    if !lets.is_empty() {
        return Err(format!("{source}: placeholder Let bindings: {lets:?}"));
    }
    let variables = placeholder_variables(body);
    if variables != 0 {
        return Err(format!("{source}: placeholder Variable places: {variables}"));
    }
    let pir = placeholder_pir_symbols(graph);
    if !pir.is_empty() {
        return Err(format!("{source}: placeholder PIR symbols: {pir:?}"));
    }
    Ok(())
}

/// An assignment whose LHS is a hash/array element with the expected access.
fn element_assign(
    body: &HirBody,
    container: &str,
    kind: SubscriptKind,
    access: AccessMode,
    mode: AssignMode,
) -> Result<(), String> {
    for expr in body.exprs.iter() {
        let HirExpr::Assign { lhs, mode: got_mode, .. } = expr else {
            continue;
        };
        if got_mode != &mode {
            continue;
        }
        let Some(HirExpr::Subscript(subscript)) = body.expr(*lhs) else {
            continue;
        };
        if subscript.kind != kind || subscript.access != access {
            continue;
        }
        let Some(HirExpr::Variable(variable)) = body.expr(subscript.container) else {
            continue;
        };
        if variable.name == container && variable.access == AccessMode::Read {
            return Ok(());
        }
    }
    Err(format!(
        "expected {mode:?} assignment to {kind:?} element of {container:?} with {access:?} access"
    ))
}

fn named_let(
    body: &HirBody,
    name: &str,
    sigil: Sigil,
    storage: DeclStorageClass,
) -> Result<(), String> {
    for stmt in body.stmts.iter() {
        if let HirStmt::Let { name: got, sigil: got_sigil, storage: got_storage, .. } = stmt
            && got == name
            && *got_sigil == sigil
            && *got_storage == storage
        {
            return Ok(());
        }
    }
    Err(format!("expected Let {name:?} sigil={sigil:?} storage={storage:?}"))
}

fn named_local_let(body: &HirBody, name: &str) -> Result<(), String> {
    named_let(body, name, Sigil::Scalar, DeclStorageClass::Local)
}

fn let_names(body: &HirBody) -> Vec<&str> {
    body.stmts
        .iter()
        .filter_map(|stmt| match stmt {
            HirStmt::Let { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

fn pir_writes_name(graph: &PirGraph, name: &str) -> bool {
    graph.nodes.iter().any(|node| match &node.operation {
        PirOperation::StashWrite { symbol } | PirOperation::StashModify { symbol, .. } => {
            symbol.name == name
        }
        PirOperation::LexicalWrite { name: lexical } => lexical.name == name,
        _ => false,
    })
}

fn pir_has_assign(graph: &PirGraph) -> bool {
    graph.nodes.iter().any(|node| matches!(node.operation, PirOperation::Assign))
}

fn pir_unsupported(graph: &PirGraph, key: &str) -> usize {
    graph.receipt.unsupported_construct_counts.get(key).copied().unwrap_or(0)
}

#[test]
fn hash_element_local_lowers_the_real_place() -> Result<(), String> {
    let source = "local $ENV{PATH} = '/bin';";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    element_assign(body, "ENV", SubscriptKind::Hash, AccessMode::Write, AssignMode::Simple)?;
    if pir_unsupported(&graph, "Subscript") != 1 {
        return Err(format!("element target must remain fail-closed Subscript: {graph:?}"));
    }
    if !pir_has_assign(&graph) {
        return Err("element write must keep its Assign node".to_string());
    }
    Ok(())
}

#[test]
fn parenthesized_argument_local_lowers_the_real_place() -> Result<(), String> {
    // Distinct parser branch: `parse_declaration_arg` stores the subscript in
    // `variable` and the RHS in `initializer` (not an embedded Assignment).
    let source = "foo(local($ENV{PATH}) = '/bin');";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    element_assign(body, "ENV", SubscriptKind::Hash, AccessMode::Write, AssignMode::Simple)?;
    if pir_unsupported(&graph, "Subscript") == 0 {
        return Err(format!("parenthesized element must reach PIR as Subscript: {graph:?}"));
    }
    if !pir_has_assign(&graph) {
        return Err("parenthesized element write must keep its Assign node".to_string());
    }
    Ok(())
}

#[test]
fn hash_element_compound_local_keeps_rmw_place() -> Result<(), String> {
    let source = "local $h{k} .= 'x';";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    element_assign(
        body,
        "h",
        SubscriptKind::Hash,
        AccessMode::ReadModifyWrite,
        AssignMode::ReadModifyWrite,
    )?;
    if pir_unsupported(&graph, "Subscript") == 0 {
        return Err(format!("RMW element target must still surface as Subscript: {graph:?}"));
    }
    if pir_unsupported(&graph, "CompoundAssignNonVarLhs") == 0 {
        return Err("RMW element local must stay fail-closed, not a fake named modify".to_string());
    }
    Ok(())
}

#[test]
fn array_slice_local_rejects_placeholder_and_keeps_the_write() -> Result<(), String> {
    let source = "local @a[0,1] = (1,2);";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    let assign = body.exprs.iter().find_map(|expr| match expr {
        HirExpr::Assign { lhs, mode: AssignMode::Simple, .. } => Some(*lhs),
        _ => None,
    });
    let Some(lhs_id) = assign else {
        return Err("slice local must keep a simple assignment".to_string());
    };
    match body.expr(lhs_id) {
        Some(HirExpr::Call { ast_kind, .. }) if ast_kind == "ArraySlice" => {}
        other => {
            return Err(format!("slice local must lower the ArraySlice target, got {other:?}"));
        }
    }
    if matches!(
        body.stmts.iter().next(),
        Some(HirStmt::Let { name, .. }) if name == PLACEHOLDER
    ) {
        return Err("slice local must not be a placeholder Let".to_string());
    }
    if !pir_has_assign(&graph) {
        return Err("slice write must keep its Assign node".to_string());
    }
    Ok(())
}

#[test]
fn array_element_and_computed_key_are_real_places() -> Result<(), String> {
    for (source, container, kind) in [
        ("local $a[0] = 1;", "a", SubscriptKind::Array),
        ("local $h{$k} = 1;", "h", SubscriptKind::Hash),
    ] {
        let file = canonical_body(source)?;
        let body = body_of(&file)?;
        let graph = pir_of(&file);
        require_no_placeholder(body, &graph, source)?;
        element_assign(body, container, kind, AccessMode::Write, AssignMode::Simple)?;
    }
    Ok(())
}

#[test]
fn bare_element_local_is_a_write_place_not_a_named_let() -> Result<(), String> {
    let source = "local $ENV{PATH};";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    let found = body.exprs.iter().any(|expr| {
        matches!(
            expr,
            HirExpr::Subscript(subscript)
                if subscript.kind == SubscriptKind::Hash
                    && subscript.access == AccessMode::Write
                    && matches!(
                        body.expr(subscript.container),
                        Some(HirExpr::Variable(variable)) if variable.name == "ENV"
                    )
        )
    });
    if !found {
        return Err("bare element local must lower a Write subscript place".to_string());
    }
    if body.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Let { .. })) {
        return Err("bare element local must not invent a named Let".to_string());
    }
    if pir_unsupported(&graph, "Subscript") == 0 {
        return Err(format!("bare element local must remain fail-closed Subscript: {graph:?}"));
    }
    Ok(())
}

#[test]
fn named_local_still_binds_a_real_let() -> Result<(), String> {
    let source = "local $x = 1;";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    named_local_let(body, "x")?;
    let stash_writes = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(&node.operation, PirOperation::StashWrite { symbol } if symbol.name == "x")
        })
        .count();
    if stash_writes != 1 {
        return Err(format!("named local must keep one StashWrite of x, got {stash_writes}"));
    }
    Ok(())
}

#[test]
fn non_local_element_assignment_is_unchanged() -> Result<(), String> {
    // Opposite-direction control: a plain element write never was a `local` Let.
    let source = "$ENV{PATH} = '/bin';";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    element_assign(body, "ENV", SubscriptKind::Hash, AccessMode::Write, AssignMode::Simple)?;
    if body.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Let { .. })) {
        return Err("plain element assignment must not become a Let".to_string());
    }
    Ok(())
}

#[test]
fn recovered_element_local_does_not_become_a_placeholder() -> Result<(), String> {
    let source = "local $ENV{PATH} =;";
    let output = Parser::new(source).parse_with_recovery();
    if output.diagnostics.is_empty() {
        return Err("expected recovery diagnostics for a missing element RHS".to_string());
    }
    let file = lower_ast(&output.ast);
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    if !placeholder_lets(body).is_empty() || placeholder_variables(body) != 0 {
        return Err("recovered element local must not invent <unknown>".to_string());
    }
    if !placeholder_pir_symbols(&graph).is_empty() {
        return Err("recovered element local must not stash-write <unknown>".to_string());
    }
    let kept = body.exprs.iter().any(|expr| {
        matches!(
            expr,
            HirExpr::Assign { lhs, .. }
                if matches!(body.expr(*lhs), Some(HirExpr::Subscript(_)))
        )
    });
    if !kept {
        return Err("recovered element local must keep the subscript assignment".to_string());
    }
    Ok(())
}

#[test]
fn local_typeglob_binds_the_real_glob_name() -> Result<(), String> {
    let source = "local *FH;";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    named_let(body, "FH", Sigil::Glob, DeclStorageClass::Local)?;
    if !pir_writes_name(&graph, "FH") {
        return Err(format!("local *FH must stash-write FH, got {graph:?}"));
    }
    Ok(())
}

#[test]
fn arrow_postfix_my_recovers_the_declared_base() -> Result<(), String> {
    let source = "my $cache->{key} = [1,2,3];";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    named_let(body, "cache", Sigil::Scalar, DeclStorageClass::My)?;
    element_assign(body, "cache", SubscriptKind::Hash, AccessMode::Write, AssignMode::Simple)?;
    if !pir_writes_name(&graph, "cache") {
        return Err(format!("postfix my must lexical-write cache, got {graph:?}"));
    }
    if pir_unsupported(&graph, "Subscript") == 0 {
        return Err(format!("postfix assignment must keep the hash subscript: {graph:?}"));
    }
    let first_pass = file.items.iter().find_map(|item| match &item.kind {
        HirKind::VariableDecl(declaration) => Some(declaration),
        _ => None,
    });
    let Some(declaration) = first_pass else {
        return Err("postfix my must remain a first-pass VariableDecl".to_string());
    };
    let names: Vec<&str> =
        declaration.variables.iter().map(|binding| binding.name.as_str()).collect();
    if names != ["cache"] {
        return Err(format!("first-pass must bind cache, not the hash slot: {names:?}"));
    }
    Ok(())
}

#[test]
fn computed_typeglob_local_is_not_a_static_symbol() -> Result<(), String> {
    let source = "local *{$name} = \\&foo;";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    if let_names(body).iter().any(|name| *name == "$name" || *name == "name") {
        return Err(format!("computed glob must not become a named Let: {:?}", let_names(body)));
    }
    if pir_writes_name(&graph, "$name") || pir_writes_name(&graph, "name") {
        return Err("computed glob must not stash-write the capture text".to_string());
    }
    let first_pass = file.items.iter().find_map(|item| match &item.kind {
        HirKind::VariableDecl(declaration) => Some(declaration),
        _ => None,
    });
    if let Some(declaration) = first_pass {
        let names: Vec<&str> =
            declaration.variables.iter().map(|binding| binding.name.as_str()).collect();
        if names.iter().any(|name| *name == "$name" || *name == "name") {
            return Err(format!("first-pass must not bind the capture text: {names:?}"));
        }
    }
    assert_clean_parse(source);
    let mirror = lower_body(&parse(source));
    if let_names(&mirror).iter().any(|name| *name == "$name" || *name == "name") {
        return Err("mirror computed glob must not become a named Let".to_string());
    }
    Ok(())
}

#[test]
fn compound_postfix_my_keeps_one_rmw_assign() -> Result<(), String> {
    let source = "my $cache->{key} += 1;";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    named_let(body, "cache", Sigil::Scalar, DeclStorageClass::My)?;
    element_assign(
        body,
        "cache",
        SubscriptKind::Hash,
        AccessMode::ReadModifyWrite,
        AssignMode::ReadModifyWrite,
    )?;
    let simple_wraps = body
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr,
                HirExpr::Assign { lhs, mode: AssignMode::Simple, .. }
                    if matches!(body.expr(*lhs), Some(HirExpr::Assign { mode: AssignMode::ReadModifyWrite, .. }))
            )
        })
        .count();
    if simple_wraps != 0 {
        return Err("compound postfix must not wrap RMW in a second simple Assign".to_string());
    }
    if !pir_writes_name(&graph, "cache") {
        return Err(format!("compound postfix my must still lexical-write cache, got {graph:?}"));
    }
    Ok(())
}

#[test]
fn arrow_postfix_local_is_element_localization_not_a_container_binding() -> Result<(), String> {
    // Class falsifier: recovering every Binary.left would turn this into Let `obj`.
    let source = "local $obj->{key} = 1;";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    element_assign(body, "obj", SubscriptKind::Hash, AccessMode::Write, AssignMode::Simple)?;
    if let_names(body).contains(&"obj") {
        return Err("element local via arrow must not bind the container".to_string());
    }
    if body.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Let { .. })) {
        return Err("arrow-element local must stay a place write, not a Let".to_string());
    }
    if pir_writes_name(&graph, "obj") {
        return Err("arrow-element local must not stash-write the container".to_string());
    }
    Ok(())
}

#[test]
fn mirror_statement_form_rejects_placeholder_let() -> Result<(), String> {
    // The test-only mirror does not model Subscript places (#5813). It still
    // must not emit a named `<unknown>` Let for a statement-level element local.
    let source = "local $ENV{PATH} = '/bin';";
    assert_clean_parse(source);
    let body = lower_body(&parse(source));
    if !placeholder_lets(&body).is_empty() || placeholder_variables(&body) != 0 {
        return Err("mirror must not invent <unknown> for element local".to_string());
    }
    if body.stmts.iter().any(|stmt| matches!(stmt, HirStmt::Let { .. })) {
        return Err("mirror element local must not be a named Let".to_string());
    }
    let assign = body.exprs.iter().any(|expr| matches!(expr, HirExpr::Assign { .. }));
    if !assign {
        return Err("mirror must keep the embedded assignment".to_string());
    }
    Ok(())
}

#[test]
fn mirror_recovers_typeglob_and_postfix_my_without_placeholder() -> Result<(), String> {
    assert_clean_parse("local *FH;");
    let glob_body = lower_body(&parse("local *FH;"));
    if !placeholder_lets(&glob_body).is_empty() {
        return Err("mirror local *FH must not invent <unknown>".to_string());
    }
    named_let(&glob_body, "FH", Sigil::Glob, DeclStorageClass::Local)?;

    assert_clean_parse("my $cache->{key} = [1,2,3];");
    let postfix_body = lower_body(&parse("my $cache->{key} = [1,2,3];"));
    if !placeholder_lets(&postfix_body).is_empty() || placeholder_variables(&postfix_body) != 0 {
        return Err("mirror postfix my must not invent <unknown>".to_string());
    }
    named_let(&postfix_body, "cache", Sigil::Scalar, DeclStorageClass::My)?;
    Ok(())
}

#[test]
fn foreach_named_my_iterator_still_binds() -> Result<(), String> {
    // Real Perl accepts `foreach my $x`, not `foreach local $ENV{PATH}` as the
    // iterator (`Missing $ on loop variable`). Keep the named-my write after
    // the declaration classifier started owning this seam.
    let source = "foreach my $x (@a) { 1 }";
    let file = canonical_body(source)?;
    let body = body_of(&file)?;
    let graph = pir_of(&file);
    require_no_placeholder(body, &graph, source)?;
    let found = body.exprs.iter().any(|expr| {
        matches!(
            expr,
            HirExpr::Variable(variable)
                if variable.name == "x" && variable.access == AccessMode::Write
        )
    });
    if !found {
        return Err("named foreach my iterator must remain a write of x".to_string());
    }
    Ok(())
}
