use perl_parser_core::Parser;
use perl_parser_core::hir::{CallForm, HirExpr, HirKind, HirLiteralKind, lower_ast};

#[test]
fn root_calls_keep_their_own_argument_occurrence() {
    let source = "is(Foo::tier(101), 1); is(Foo::tier(100), 1);";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    assert!(parsed.diagnostics.is_empty(), "fixture must parse without diagnostics");
    let hir = lower_ast(&parsed.ast);
    let body = hir.root_body().expect("root body");
    let calls: Vec<_> = hir.items.iter().filter(|item| matches!(&item.kind,
        HirKind::CallExpr(call) if call.name == "Foo::tier" && call.form == CallForm::NamedFunction
    )).collect();
    assert_eq!(calls.len(), 2);
    let ids: Vec<_> = calls
        .iter()
        .map(|call| hir.root_call_expression(call.id).expect("unique body call"))
        .collect();
    assert_ne!(ids[0], ids[1]);
    let values: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let call_range = body.source_map.expr_range(*id).expect("call range");
            assert_eq!(call_range, calls[index].range);
            assert_eq!(
                &source[call_range.start..call_range.end],
                ["Foo::tier(101)", "Foo::tier(100)"][index]
            );
            let HirExpr::Call { args, .. } = body.expr(*id).expect("call expression") else {
                panic!("expected call")
            };
            let range = body.source_map.expr_range(args[0]).expect("argument range");
            assert_eq!(&source[range.start..range.end], ["101", "100"][index]);
            let HirExpr::Literal(literal) = body.expr(args[0]).expect("argument") else {
                panic!("expected literal")
            };
            assert_eq!(literal.kind, HirLiteralKind::Number);
            literal.payload.as_str()
        })
        .collect();
    assert_eq!(values, ["101", "100"]);
}

#[test]
fn dynamic_call_has_no_named_call_occurrence() {
    let mut parser = Parser::new("$callback->(100);");
    let parsed = parser.parse_with_recovery();
    let hir = lower_ast(&parsed.ast);
    let call = hir
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind,
                HirKind::CallExpr(call) if call.form == CallForm::Coderef
            )
        })
        .expect("dynamic call shell");
    assert!(hir.root_call_expression(call.id).is_none());
}

#[test]
fn ambiguous_shell_or_expression_and_recovery_refuse_the_join() {
    use perl_parser_core::hir::{HirExprId, HirId, RecoveryConfidence};
    let mut parser = Parser::new("Foo::tier(100);");
    let mut hir = lower_ast(&parser.parse_with_recovery().ast);
    let call = hir
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind,
                HirKind::CallExpr(call) if call.name == "Foo::tier"
            )
        })
        .expect("call")
        .clone();
    let id = hir.root_call_expression(call.id).expect("initial exact syntax join");
    let body = hir.bodies.first_mut().expect("root body");
    let expr = body.expr(id).expect("expression").clone();
    let range = body.source_map.expr_range(id).expect("range");
    let duplicate_id = HirExprId(body.exprs.alloc(expr));
    body.source_map.expr_ranges.push(range);
    assert_ne!(id, duplicate_id);
    assert!(hir.root_call_expression(call.id).is_none(), "ambiguous body expressions");

    let mut parser = Parser::new("Foo::tier(100);");
    let mut hir = lower_ast(&parser.parse_with_recovery().ast);
    hir.items.push(call.clone());
    assert!(hir.root_call_expression(call.id).is_none(), "ambiguous flat shells");
    hir.items.pop();
    assert!(hir.root_call_expression(HirId::from_index(u32::MAX)).is_none());
    let item = hir.items.iter_mut().find(|item| item.id == call.id).expect("call item");
    item.range.end = item.range.start;
    assert!(hir.root_call_expression(call.id).is_none(), "empty range");
    let item = hir.items.iter_mut().find(|item| item.id == call.id).expect("call item");
    item.range = call.range;
    item.recovery_confidence = RecoveryConfidence::Recovered;
    assert!(hir.root_call_expression(call.id).is_none(), "recovered shell");
}

#[test]
fn call_in_subroutine_is_not_joined_to_root_body() {
    let mut parser = Parser::new("sub test { Foo::tier(100) }");
    let hir = lower_ast(&parser.parse_with_recovery().ast);
    let call = hir
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind,
                HirKind::CallExpr(call) if call.name == "Foo::tier"
            )
        })
        .expect("call in subroutine");
    assert!(hir.root_call_expression(call.id).is_none());
}
