//! Package/foreach scope composition regressions for #14173 and #18208.
//! Check exact HIR binding identity and production PIR read reachability.

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, HirBindingId, HirExpr, HirExprId, HirFile, VariableKind, lower_ast,
};
use perl_tdd_support::must_some;

/// One flattened `HirExpr::Variable` occurrence with its source anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Occurrence {
    name: String,
    access: AccessMode,
    kind: VariableKind,
    binding: Option<HirBindingId>,
    start: usize,
    end: usize,
}

/// Flatten every `HirExpr::Variable` across all bodies, ordered by source span.
fn occurrences(file: &HirFile) -> Vec<Occurrence> {
    let mut found = Vec::new();
    for body in &file.bodies {
        for idx in 0..body.source_map.expr_ranges.len() {
            let id = HirExprId(idx as u32);
            if let Some(HirExpr::Variable(var)) = body.expr(id) {
                let range = must_some(body.source_map.expr_range(id));
                found.push(Occurrence {
                    name: var.name.clone(),
                    access: var.access,
                    kind: var.kind,
                    binding: var.binding,
                    start: range.start,
                    end: range.end,
                });
            }
        }
    }
    found.sort_by_key(|o| (o.start, o.end));
    found
}

/// Compare each read with the exact declaration it must see, in source order.
/// In particular, `Some(binding)` alone cannot detect iterator leakage.
fn assert_package_foreach_reads(source: &str, expected: &[usize]) {
    let output = Parser::new(source).parse_with_recovery();
    assert!(output.diagnostics.is_empty(), "fixture must parse cleanly: {:?}", output.diagnostics);
    let file = lower_ast(&output.ast);
    let mut bindings: Vec<_> = file.scope_graph.bindings.iter().filter(|b| b.name == "i").collect();
    bindings.sort_by_key(|b| b.range.start);
    assert_eq!(bindings.len(), expected.iter().max().map_or(0, |index| index + 1));
    let reads: Vec<_> = occurrences(&file)
        .into_iter()
        .filter(|o| o.name == "i" && o.access == AccessMode::Read)
        .collect();
    assert_eq!(reads.len(), expected.len(), "every expected read must reach body HIR: {source}");
    for (read, &index) in reads.iter().zip(expected) {
        assert_eq!(
            read.binding,
            Some(bindings[index].id),
            "wrong declaration at {}: {source}",
            read.start
        );
        assert_eq!(read.kind, VariableKind::Lexical, "lexical read at {}: {source}", read.start);
    }
    // Traverse the production body-to-PIR consumer as well: orphan arena
    // occurrences cannot supply the reads that this extractor reports.
    let receipt = perl_parser_core::pir::extract_lexical_facts(&file);
    assert_eq!(receipt.total_read_count, expected.len(), "PIR must retain every read: {source}");
    let mut pir_read_starts: Vec<_> = receipt
        .bodies
        .iter()
        .flat_map(|body| &body.facts)
        .filter(|fact| fact.role == perl_parser_core::pir::LexicalRole::Read)
        .map(|fact| must_some(fact.source_anchor.range).start)
        .collect();
    pir_read_starts.sort_unstable();
    assert_eq!(pir_read_starts, reads.iter().map(|read| read.start).collect::<Vec<_>>());
}

#[test]
fn package_foreach_list_body_continue_and_after_use_exact_bindings() {
    for package in ["package P;", "LBL: package P;"] {
        let source = format!(
            "{package} my $i=9; print $i; foreach my $i ($i) {{ print $i; }} continue {{ print $i; }} print $i;"
        );
        assert_package_foreach_reads(&source, &[0, 0, 1, 1, 0]);
    }
}

#[test]
fn multiply_labelled_package_foreach_uses_exact_bindings() {
    assert_package_foreach_reads(
        "A: B: package P; my $i=9; print $i; foreach my $i ($i) { print $i; } continue { print $i; } print $i;",
        &[0, 0, 1, 1, 0],
    );
}

#[test]
fn package_switch_in_foreach_body_restores_continue_and_outer_bindings() {
    assert_package_foreach_reads(
        "package P; my $i=9; foreach my $i (1) { LBL: package Q; my $i=2; print $i; } continue { print $i; } print $i;",
        &[2, 1, 0],
    );
}

#[test]
fn package_foreach_inside_bare_block_restores_outer_binding() {
    for label in ["", "BLK:"] {
        let source = format!(
            "my $i=9; {label} {{ LBL: package P; my $i=8; foreach my $i ($i) {{ print $i; }} print $i; }} print $i;"
        );
        assert_package_foreach_reads(&source, &[1, 2, 1, 0]);
    }
}

#[test]
fn block_form_package_foreach_restores_outer_binding() {
    assert_package_foreach_reads(
        "my $i=9; package P { my $i=8; foreach my $i ($i) { print $i; } print $i; } print $i;",
        &[1, 2, 1, 0],
    );
}
