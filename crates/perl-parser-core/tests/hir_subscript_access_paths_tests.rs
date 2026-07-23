//! HIR subscript access paths (#2580): array/hash element access (`$arr[i]`,
//! `$hash{k}`) lowers to a first-class `HirExpr::Subscript` place with the
//! container and subscript kept as separate expression IDs (evaluate-once), and
//! with the element's `AccessMode` propagated from context (read vs. write vs.
//! read-modify-write). These fixtures exercise the canonical `lower_ast` body
//! lowering path.

use perl_parser_core::Parser;
use perl_parser_core::hir::{AccessMode, HirBody, HirExpr, HirFile, SubscriptKind, lower_ast};

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    lower_ast(&output.ast)
}

/// A flattened description of one `HirExpr::Subscript` node for assertions.
#[derive(Debug, PartialEq, Eq)]
struct SubInfo {
    kind: SubscriptKind,
    access: AccessMode,
    /// Short description of the container expression.
    container: String,
    /// Short description of the subscript (index/key) expression.
    subscript: String,
}

/// Short, stable description of an expression for test assertions.
fn describe(body: &HirBody, id: perl_parser_core::hir::HirExprId) -> String {
    match body.expr(id) {
        Some(HirExpr::Variable(v)) => format!("var:{}", v.name),
        Some(HirExpr::Opaque { ast_kind }) => format!("opaque:{ast_kind}"),
        Some(HirExpr::Call { .. }) => "call".to_string(),
        Some(HirExpr::Subscript(sub)) => format!("subscript:{:?}", sub.kind),
        Some(HirExpr::Binary { .. }) => "binary".to_string(),
        Some(other) => format!("{other:?}"),
        None => "<none>".to_string(),
    }
}

/// Collect every `HirExpr::Subscript` across all bodies, in arena order.
fn subscripts(file: &HirFile) -> Vec<SubInfo> {
    let mut found = Vec::new();
    for body in &file.bodies {
        let expr_count = body.source_map.expr_ranges.len();
        for idx in 0..expr_count {
            let id = perl_parser_core::hir::HirExprId(idx as u32);
            if let Some(HirExpr::Subscript(sub)) = body.expr(id) {
                found.push(SubInfo {
                    kind: sub.kind,
                    access: sub.access,
                    container: describe(body, sub.container),
                    subscript: describe(body, sub.subscript),
                });
            }
        }
    }
    found
}

#[test]
fn array_literal_index_reads_element() {
    let file = lower("my @arr = (1, 2, 3); my $x = $arr[0];");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 1, "expected exactly one subscript, got {subs:?}");
    assert_eq!(subs[0].kind, SubscriptKind::Array);
    assert_eq!(subs[0].access, AccessMode::Read);
    assert_eq!(subs[0].container, "var:arr");
    assert_eq!(subs[0].subscript, "opaque:Number");
}

#[test]
fn array_computed_index_write_place() {
    let file = lower("my @arr; my $i = 1; $arr[$i] = 99;");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 1, "expected exactly one subscript, got {subs:?}");
    assert_eq!(subs[0].kind, SubscriptKind::Array);
    assert_eq!(subs[0].access, AccessMode::Write, "assignment LHS subscript is a write place");
    assert_eq!(subs[0].container, "var:arr");
    assert_eq!(subs[0].subscript, "var:i");
}

#[test]
fn array_function_call_index_is_evaluate_once() {
    // `$arr[f()]++` — the index call must appear as a single subscript child expr,
    // and `++` makes the element a read-modify-write place.
    let file = lower("my @arr; $arr[f()]++;");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 1, "expected exactly one subscript, got {subs:?}");
    assert_eq!(subs[0].kind, SubscriptKind::Array);
    assert_eq!(subs[0].access, AccessMode::ReadModifyWrite);
    assert_eq!(subs[0].container, "var:arr");
    assert_eq!(subs[0].subscript, "call", "computed index evaluated once as a Call child");
}

#[test]
fn hash_literal_key_reads_element() {
    let file = lower("my %hash = (key => 1); my $x = $hash{key};");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 1, "expected exactly one subscript, got {subs:?}");
    assert_eq!(subs[0].kind, SubscriptKind::Hash);
    assert_eq!(subs[0].access, AccessMode::Read);
    assert_eq!(subs[0].container, "var:hash");
    assert_eq!(subs[0].subscript, "opaque:Identifier");
}

#[test]
fn hash_computed_key_write_place() {
    let file = lower("my %h; my $k = 'x'; $h{$k} = 42;");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 1, "expected exactly one subscript, got {subs:?}");
    assert_eq!(subs[0].kind, SubscriptKind::Hash);
    assert_eq!(subs[0].access, AccessMode::Write);
    assert_eq!(subs[0].container, "var:h");
    assert_eq!(subs[0].subscript, "var:k");
}

#[test]
fn hash_function_call_key_is_evaluate_once() {
    let file = lower("my %h; $h{f()}++;");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 1, "expected exactly one subscript, got {subs:?}");
    assert_eq!(subs[0].kind, SubscriptKind::Hash);
    assert_eq!(subs[0].access, AccessMode::ReadModifyWrite);
    assert_eq!(subs[0].container, "var:h");
    assert_eq!(subs[0].subscript, "call");
}

#[test]
fn nested_subscripts_form_a_subscript_tree() {
    // `$data{a}{b}` — the outer hash access's container is itself a subscript.
    let file = lower("my %data; my $x = $data{a}{b};");
    let subs = subscripts(&file);
    assert_eq!(subs.len(), 2, "expected two nested subscripts, got {subs:?}");
    // Exactly one of them has a subscript-typed container (the outer access).
    let outer: Vec<&SubInfo> =
        subs.iter().filter(|s| s.container.starts_with("subscript:")).collect();
    assert_eq!(outer.len(), 1, "exactly one outer subscript whose container is a subscript");
    assert_eq!(outer[0].kind, SubscriptKind::Hash);
    assert_eq!(outer[0].access, AccessMode::Read);
    // The inner access reads `$data`.
    let inner: Vec<&SubInfo> = subs.iter().filter(|s| s.container == "var:data").collect();
    assert_eq!(inner.len(), 1, "exactly one inner subscript whose container is $data");
    assert_eq!(inner[0].kind, SubscriptKind::Hash);
}

#[test]
fn subscript_is_not_a_generic_binary() {
    // Regression guard: a subscript must not fall through to `HirExpr::Binary`.
    let file = lower("my @arr; my $x = $arr[0];");
    let has_binary = file.bodies.iter().any(|body| {
        (0..body.source_map.expr_ranges.len()).any(|i| {
            matches!(
                body.expr(perl_parser_core::hir::HirExprId(i as u32)),
                Some(HirExpr::Binary { .. })
            )
        })
    });
    assert!(!has_binary, "subscript access must lower to Subscript, not a generic Binary");
}
