//! Source-order visibility of bindings in both HIR projections (#13868).

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirExpr, HirExprId, HirFile, StorageClass, VariableKind, lower_ast,
};
use perl_parser_core::pir::{PirOperation, lower_hir_bodies};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    lower_ast(&parser.parse_with_recovery().ast)
}

fn check_reference(file: &HirFile, source: &str, start: usize, expected: Option<StorageClass>) {
    let graph = file.scope_graph.references.iter().find(|r| r.range.start == start);
    let graph = graph.expect("first-pass reference at marker");
    let graph_storage = graph.resolved_binding.map(|id| {
        file.scope_graph.bindings.iter().find(|b| b.id == id).expect("graph binding").storage
    });
    assert_eq!(graph_storage, expected, "first pass at {start}: {source}");

    let body_var = file
        .bodies
        .iter()
        .find_map(|body| {
            body.source_map.expr_ranges.iter().enumerate().find_map(|(idx, range)| {
                if range.start != start {
                    return None;
                }
                match body.expr(HirExprId(idx as u32)) {
                    Some(HirExpr::Variable(var)) => Some(var),
                    _ => None,
                }
            })
        })
        .expect("body variable at marker");
    assert_eq!(body_var.binding, graph.resolved_binding, "passes disagree at {start}: {source}");
    let expected_kind = match expected {
        Some(StorageClass::LexicalMy | StorageClass::LexicalState | StorageClass::Parameter) => {
            VariableKind::Lexical
        }
        Some(StorageClass::ClassField) => VariableKind::Field,
        _ => VariableKind::Package,
    };
    assert_eq!(body_var.kind, expected_kind);
}

#[test]
fn forward_references_and_prior_shadowing_follow_source_order() {
    for decl in ["my", "state"] {
        let source = format!("sub f {{ print $x; {decl} $x; print $x; }}\n");
        let file = lower(&source);
        let first = source.find("print $x;").expect("first read") + "print ".len();
        check_reference(&file, &source, first, None);
        let after = source.rfind("print $x;").expect("second read");
        check_reference(
            &file,
            &source,
            after + "print ".len(),
            Some(if decl == "my" { StorageClass::LexicalMy } else { StorageClass::LexicalState }),
        );
    }
}

#[test]
fn later_inner_binding_does_not_hide_earlier_outer_binding() {
    let source = "sub f { my $x; if ($ok) { print $x; my $x; print $x; } }";
    let file = lower(source);
    let bindings: Vec<_> = file.scope_graph.bindings.iter().filter(|b| b.name == "x").collect();
    assert_eq!(bindings.len(), 2);
    let reads: Vec<_> = file.scope_graph.references.iter().filter(|r| r.name == "x").collect();
    assert_eq!(reads.len(), 2);
    assert_eq!(reads[0].resolved_binding, Some(bindings[0].id));
    assert_eq!(reads[1].resolved_binding, Some(bindings[1].id));
    for read in reads {
        let body_binding = file.bodies.iter().find_map(|body| {
            body.source_map.expr_ranges.iter().enumerate().find_map(|(idx, range)| {
                if range.start != read.range.start {
                    return None;
                }
                match body.expr(HirExprId(idx as u32)) {
                    Some(HirExpr::Variable(var)) => Some(var.binding),
                    _ => None,
                }
            })
        });
        assert_eq!(body_binding, Some(read.resolved_binding));
    }
}

#[test]
fn initializer_reads_the_outer_binding_before_its_new_binding_is_available() {
    let source = "sub f { my $x = 1; if ($ok) { my $x = $x; print $x; } }";
    let file = lower(source);
    let initializer = source.find("= $x").expect("initializer") + 2;
    let after = source.rfind("print $x").expect("later read") + 6;
    let x_bindings: Vec<_> = file.scope_graph.bindings.iter().filter(|b| b.name == "x").collect();
    assert_eq!(x_bindings.len(), 2);
    check_reference(&file, source, initializer, Some(StorageClass::LexicalMy));
    check_reference(&file, source, after, Some(StorageClass::LexicalMy));
    let initializer_ref = file
        .scope_graph
        .references
        .iter()
        .find(|r| r.range.start == initializer)
        .expect("initializer reference");
    let later_ref = file
        .scope_graph
        .references
        .iter()
        .find(|r| r.range.start == after)
        .expect("later reference");
    assert_eq!(initializer_ref.resolved_binding, Some(x_bindings[0].id));
    assert_eq!(later_ref.resolved_binding, Some(x_bindings[1].id));
}

#[test]
fn same_scope_redeclaration_selects_the_nearest_prior_binding() {
    let source = "sub f { my $x; print $x; my $x; print $x; }";
    let file = lower(source);
    let bindings: Vec<_> = file.scope_graph.bindings.iter().filter(|b| b.name == "x").collect();
    let reads: Vec<_> = file.scope_graph.references.iter().filter(|r| r.name == "x").collect();
    assert_eq!(bindings.len(), 2);
    assert_eq!(reads.len(), 2);
    assert_eq!(reads[0].resolved_binding, Some(bindings[0].id));
    assert_eq!(reads[1].resolved_binding, Some(bindings[1].id));
    for read in reads {
        check_reference(&file, source, read.range.start, Some(StorageClass::LexicalMy));
    }
}

#[test]
fn declaration_list_initializer_reads_outer_bindings() {
    let source = "sub f { my ($a, $b) = (1, 2); if ($ok) { my ($a, $b) = ($b, $a); print $a; } }";
    let file = lower(source);
    let bindings: Vec<_> =
        file.scope_graph.bindings.iter().filter(|b| b.name == "a" || b.name == "b").collect();
    assert_eq!(bindings.len(), 4);
    let initializer = source.find("= ($b, $a)").expect("inner initializer");
    for (needle, outer_name) in [("$b", "b"), ("$a", "a")] {
        let start = initializer + source[initializer..].find(needle).expect("RHS variable");
        // Body HIR does not project VariableListDeclaration expressions yet;
        // the first-pass graph still has the source-backed RHS references.
        let reference = file
            .scope_graph
            .references
            .iter()
            .find(|r| r.range.start == start)
            .expect("RHS reference");
        let outer = bindings.iter().find(|b| b.name == outer_name).expect("outer declaration");
        assert_eq!(reference.resolved_binding, Some(outer.id));
    }
    let after = source.rfind("print $a").expect("post-declaration read") + 6;
    let inner_a = bindings.iter().rfind(|b| b.name == "a").expect("inner declaration");
    let reference = file
        .scope_graph
        .references
        .iter()
        .find(|r| r.range.start == after)
        .expect("post-declaration reference");
    assert_eq!(reference.resolved_binding, Some(inner_a.id));
}

#[test]
fn c_style_for_header_sees_its_completed_initializer_binding() {
    let source = "for (my $i = 0; $i < 2; $i++) { print $i; }";
    let file = lower(source);
    let binding =
        file.scope_graph.bindings.iter().find(|b| b.name == "i").expect("for initializer binding");
    for (idx, start) in source.match_indices("$i").map(|(idx, _)| idx).enumerate() {
        if idx == 0 {
            continue;
        }
        check_reference(&file, source, start, Some(StorageClass::LexicalMy));
        let reference = file
            .scope_graph
            .references
            .iter()
            .find(|r| r.range.start == start)
            .expect("for header/body reference");
        assert_eq!(reference.resolved_binding, Some(binding.id));
    }
}

#[test]
fn foreach_iterator_is_visible_in_its_body() {
    let source = "foreach my $i (1, 2) { print $i; }";
    let file = lower(source);
    let read = source.rfind("$i").expect("body read");
    check_reference(&file, source, read, Some(StorageClass::LexicalMy));
}

#[test]
fn compound_declaration_modifies_its_own_binding_while_rhs_reads_outer() {
    for op in ["+=", "||="] {
        let source = format!("sub f {{ my $x = 7; if ($ok) {{ my $x {op} $x; }} }}");
        let file = lower(&source);
        let x: Vec<_> = file.scope_graph.bindings.iter().filter(|b| b.name == "x").collect();
        assert_eq!(x.len(), 2);
        let inner = source.rfind("my $x").expect("inner declaration") + 3;
        let rhs = source.rfind("$x").expect("explicit RHS");
        let mut declaration_places = Vec::new();
        let mut rhs_reads = Vec::new();
        for body in &file.bodies {
            for (idx, range) in body.source_map.expr_ranges.iter().enumerate() {
                if let Some(HirExpr::Variable(var)) = body.expr(HirExprId(idx as u32)) {
                    if range.start == inner {
                        declaration_places.push(var);
                    } else if range.start == rhs {
                        rhs_reads.push(var);
                    }
                }
            }
        }
        assert_eq!(declaration_places.len(), 1, "one declaration place: {source}");
        assert_eq!(declaration_places[0].access, AccessMode::ReadModifyWrite);
        assert_eq!(declaration_places[0].binding, Some(x[1].id));
        assert_eq!(declaration_places[0].kind, VariableKind::Lexical);
        assert_eq!(rhs_reads.len(), 1, "one explicit RHS read: {source}");
        assert_eq!(rhs_reads[0].binding, Some(x[0].id));
        let graph = lower_hir_bodies(&file);
        assert_eq!(
            graph.nodes.iter().filter(|n| matches!(n.operation, PirOperation::Modify { .. })).count(),
            1,
            "the declaration must produce one lexical modification: {source}"
        );
        assert!(
            graph.nodes.iter().all(|n| !matches!(n.operation, PirOperation::StashModify { .. })),
            "the declaration's own LHS must not turn into a package modification: {source}"
        );
        assert_eq!(
            graph
                .nodes
                .iter()
                .filter(|n| {
                    matches!(&n.operation, PirOperation::LexicalRead { name } if name.name == "x")
                        && n.source_anchor.range.map(|range| (range.start, range.end))
                            == Some((rhs, rhs + "$x".len()))
                })
                .count(),
            1,
            "the explicit RHS must reach PIR as one anchored outer lexical read: {source}"
        );
    }
}

#[test]
fn compound_declaration_without_outer_binding_does_not_modify_package() {
    let source = "sub f { my $x += $x; }";
    let file = lower(source);
    let declared = file.scope_graph.bindings.iter().find(|b| b.name == "x").expect("declaration");
    let target = source.find("$x").expect("declaration target");
    let rhs = source.rfind("$x").expect("explicit RHS");
    let vars: Vec<_> = file
        .bodies
        .iter()
        .flat_map(|body| {
            body.source_map.expr_ranges.iter().enumerate().filter_map(|(idx, range)| {
                match body.expr(HirExprId(idx as u32)) {
                    Some(HirExpr::Variable(var)) => Some((range.start, var)),
                    _ => None,
                }
            })
        })
        .collect();
    assert!(vars.iter().any(|(start, var)| {
        *start == target && var.binding == Some(declared.id) && var.kind == VariableKind::Lexical
    }));
    assert!(vars.iter().any(|(start, var)| {
        *start == rhs && var.binding.is_none() && var.kind == VariableKind::Package
    }));
    let graph = lower_hir_bodies(&file);
    assert!(graph.nodes.iter().any(|n| matches!(n.operation, PirOperation::Modify { .. })));
    assert!(graph.nodes.iter().all(|n| !matches!(n.operation, PirOperation::StashModify { .. })));
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|n| {
                matches!(&n.operation, PirOperation::StashRead { symbol } if symbol.name == "x")
                    && n.source_anchor.range.map(|range| (range.start, range.end))
                        == Some((rhs, rhs + "$x".len()))
            })
            .count(),
        1,
        "the explicit RHS without an outer binding must reach PIR as a package read"
    );
}

#[test]
fn class_field_is_invisible_before_its_declaration_and_visible_afterward() {
    let source = concat!(
        "use feature 'class'; class C { ",
        "method before { $x; } field $x; method after { $x; } }",
    );
    let file = lower(source);
    let first = source.find("$x").expect("forward field read");
    let last = source.rfind("$x").expect("later field read");
    check_reference(&file, source, first, None);
    check_reference(&file, source, last, Some(StorageClass::ClassField));
}

#[test]
fn fresh_lowering_after_reorder_recovers_the_original_answer() {
    let before = "sub f { print $x; } my $x;";
    let after = "my $x; sub f { print $x; }";
    let forward = lower(before);
    check_reference(&forward, before, before.find("print $x").expect("read") + 6, None);
    let reordered = lower(after);
    check_reference(
        &reordered,
        after,
        after.find("print $x").expect("read") + 6,
        Some(StorageClass::LexicalMy),
    );
    // Rebuilding the original text must recover its original answer. This
    // tests the HIR rebuild, not the LSP open/close notification lifecycle.
    let reopened = lower(before);
    check_reference(&reopened, before, before.find("print $x").expect("read") + 6, None);
}
