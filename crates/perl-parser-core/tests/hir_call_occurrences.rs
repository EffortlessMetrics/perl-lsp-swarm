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

#[test]
fn signature_occurrences_retain_owner_position_mode_and_binding() {
    use perl_parser_core::hir::SignatureParameterMode;
    let source = "use feature 'signatures'; sub f ($x, $y = 1, @rest) { $x } sub g ($x) { $x }";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    assert!(parsed.diagnostics.is_empty(), "fixture must parse cleanly: {:?}", parsed.diagnostics);
    let hir = lower_ast(&parsed.ast);
    let rows = &hir.scope_graph.signature_parameters;
    assert_eq!(rows.len(), 4);
    assert_eq!(rows.iter().map(|row| row.position).collect::<Vec<_>>(), [0, 1, 2, 0]);
    assert_eq!(
        rows.iter().map(|row| row.mode).collect::<Vec<_>>(),
        [
            SignatureParameterMode::Required,
            SignatureParameterMode::Optional,
            SignatureParameterMode::Slurpy,
            SignatureParameterMode::Required,
        ]
    );
    assert_eq!(rows[0].callable, rows[1].callable);
    assert_ne!(rows[0].callable, rows[3].callable);
    assert_ne!(rows[0].binding, rows[3].binding);
    for row in rows {
        let binding = hir
            .scope_graph
            .bindings
            .iter()
            .find(|binding| Some(binding.id) == row.binding)
            .expect("source binding");
        assert_eq!(binding.scope_id, row.signature_scope);
        assert!(row.range.start <= binding.range.start && binding.range.end <= row.range.end);
    }
}

#[test]
fn named_and_unrepresented_parameters_keep_their_positions() {
    use perl_parser_core::NodeKind;
    use perl_parser_core::hir::SignatureParameterMode;
    let source = "use feature 'signatures'; sub f ($x, :$named) { $x }";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    assert!(parsed.diagnostics.is_empty(), "fixture must parse cleanly: {:?}", parsed.diagnostics);
    let hir = lower_ast(&parsed.ast);
    assert_eq!(hir.scope_graph.signature_parameters[1].mode, SignatureParameterMode::Named);

    let mut parser = Parser::new("sub f ($x, $y) { $y }");
    let mut parsed = parser.parse_with_recovery();
    let NodeKind::Program { statements } = &mut parsed.ast.kind else { panic!("program") };
    let NodeKind::Subroutine { signature: Some(signature), .. } = &mut statements[0].kind else {
        panic!("subroutine")
    };
    let NodeKind::Signature { parameters } = &mut signature.kind else { panic!("signature") };
    parameters[0].kind = NodeKind::MissingExpression;
    let hir = lower_ast(&parsed.ast);
    let rows = &hir.scope_graph.signature_parameters;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].position, rows[0].mode, rows[0].binding),
        (0, SignatureParameterMode::Unknown, None)
    );
    assert_eq!(rows[1].position, 1);
    assert_eq!(rows[1].mode, SignatureParameterMode::Required);
    assert!(rows[1].binding.is_some());
}

#[test]
fn parameter_binding_is_not_taken_from_default_expression() {
    use perl_parser_core::NodeKind;
    let source = "use feature 'signatures'; sub f ($x = do { my $local = 1; 1 }) { $x }";
    let mut parser = Parser::new(source);
    let parsed = parser.parse_with_recovery();
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let hir = lower_ast(&parsed.ast);
    let row = &hir.scope_graph.signature_parameters[0];
    let binding = hir
        .scope_graph
        .bindings
        .iter()
        .find(|b| Some(b.id) == row.binding)
        .expect("parameter binding");
    assert_eq!(binding.name, "x");
    assert_eq!(binding.scope_id, row.signature_scope);

    let mut parser = Parser::new("sub f ($x = do { my $local = 1; 1 }) { 1 }");
    let mut parsed = parser.parse_with_recovery();
    let NodeKind::Program { statements } = &mut parsed.ast.kind else { panic!("program") };
    let NodeKind::Subroutine { signature: Some(signature), .. } = &mut statements[0].kind else {
        panic!("sub")
    };
    let NodeKind::Signature { parameters } = &mut signature.kind else { panic!("signature") };
    let NodeKind::OptionalParameter { variable, .. } = &mut parameters[0].kind else {
        panic!("optional")
    };
    variable.kind = NodeKind::MissingExpression;
    let hir = lower_ast(&parsed.ast);
    assert_eq!(
        hir.scope_graph.signature_parameters[0].binding, None,
        "default's local is not the parameter"
    );
}

#[test]
fn recovered_callable_does_not_promote_nested_default_signature() {
    use perl_parser_core::hir::RecoveryConfidence;
    use perl_parser_core::{Node, NodeKind};
    let mut parser = Parser::new("sub outer ($x = sub ($z) { $z }) { $x }");
    let mut parsed = parser.parse_with_recovery();
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let NodeKind::Program { statements } = &mut parsed.ast.kind else { panic!("program") };
    let sub = statements[0].clone();
    statements[0] = Node::new(
        NodeKind::Error {
            message: "recovered".into(),
            expected: vec![],
            found: None,
            partial: Some(Box::new(sub.clone())),
        },
        sub.location,
    );
    let hir = lower_ast(&parsed.ast);
    let rows = &hir.scope_graph.signature_parameters;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].callable, rows[1].callable);
    assert!(rows.iter().all(
        |row| row.recovery_confidence == RecoveryConfidence::Recovered && row.binding.is_none()
    ));
}
