//! Parse-backed CallSemantics adapter matrix (#8851).
//!
//! One adapter test per current source family. Anchors come from AST/HIR
//! fields, never from scanning the source string for callee/argument text.

mod cpan_test_helpers;

use cpan_test_helpers::*;
use perl_ast::{
    ArgumentSource, CallBoundary, CallLimitation, CallSyntax, DispatchKind, OperandList,
    OperandPresence, OperandRole, adapt_ast_call,
};
use perl_parser_core::hir::{
    HirExpr, HirKind, adapt_body_hir_call, adapt_flat_hir_call, lower_ast,
};
use perl_parser_core::{Node, NodeKind, Parser};
use perl_tdd_support::must_some_with;

fn first_adapted_ast(source: &str) -> (Node, perl_ast::CallSemantics) {
    let ast = parse(source);
    let node = must_some_with(find_call_node(&ast), format!("no call node in {source}"));
    let semantics = must_some_with(
        adapt_ast_call(node),
        format!("adapter refused {} for {source}", node.kind.kind_name()),
    );
    assert_eq!(semantics.call_range, node.location, "adapter must keep the node range");
    (ast, semantics)
}

fn find_call_node(node: &Node) -> Option<&Node> {
    if adapt_ast_call(node).is_some() {
        return Some(node);
    }
    node.children().into_iter().find_map(find_call_node)
}

fn lower_source(source: &str) -> perl_parser_core::hir::HirFile {
    let ast = parse(source);
    lower_ast(&ast)
}

fn loc(start: usize, end: usize) -> perl_parser_core::SourceLocation {
    perl_parser_core::SourceLocation { start, end }
}

#[test]
fn named_call_foo_parens_exact_anchor_and_arg_order() {
    let source = "foo(1, 2);";
    let (_, semantics) = first_adapted_ast(source);
    assert_eq!(semantics.syntax, CallSyntax::Named);
    assert_eq!(semantics.dispatch, DispatchKind::Function);
    let args = must_some_with(semantics.arguments.explicit(), "AST args");
    assert_eq!(args.len(), 2);
    assert_eq!(
        args.iter().map(|operand| operand.role).collect::<Vec<_>>(),
        vec![OperandRole::Argument, OperandRole::Argument]
    );
    assert!(args.windows(2).all(|pair| {
        let left = must_some_with(pair.first(), "left window arg").source_range;
        let right = must_some_with(pair.get(1), "right window arg").source_range;
        must_some_with(left, "left arg range").start
            < must_some_with(right, "right arg range").start
    }));
}

#[test]
fn bare_form_is_not_guessed_from_empty_function_call() {
    let source = "foo();";
    let (_, semantics) = first_adapted_ast(source);
    assert_eq!(semantics.syntax, CallSyntax::Named);
    assert_ne!(semantics.syntax, CallSyntax::Bare);
    assert_eq!(semantics.argument_source, ArgumentSource::Unknown);
}

#[test]
fn ampersand_call_syntax_and_unknown_empty_args() {
    let (_, with_args) = first_adapted_ast("&foo(1);");
    assert_eq!(with_args.syntax, CallSyntax::Ampersand);
    assert_eq!(with_args.argument_source, ArgumentSource::ExplicitList);
    let args = must_some_with(with_args.arguments.explicit(), "amper args");
    assert!(!args.is_empty());
    assert!(args.windows(2).all(|pair| {
        must_some_with(pair.first(), "left amper arg").source_order
            < must_some_with(pair.get(1), "right amper arg").source_order
    }));

    let (_, empty) = first_adapted_ast("&foo;");
    assert_eq!(empty.syntax, CallSyntax::Ampersand);
    assert_eq!(empty.argument_source, ArgumentSource::Unknown);
    let (_, empty_parens) = first_adapted_ast("&foo();");
    assert_eq!(empty_parens.syntax, CallSyntax::Ampersand);
    assert_eq!(empty_parens.argument_source, ArgumentSource::Unknown);
    assert_eq!(empty.argument_source, empty_parens.argument_source);
}

#[test]
fn coderef_arrow_call_keeps_callee_operand() {
    let (_, semantics) = first_adapted_ast("$cb->(1, 2);");
    assert_eq!(semantics.syntax, CallSyntax::Coderef);
    assert_eq!(semantics.dispatch, DispatchKind::Coderef);
    assert!(matches!(semantics.callee_operand, OperandPresence::Present(_)));
    assert_eq!(semantics.arguments.explicit().map(|args| args.len()), Some(2));
}

#[test]
fn method_arrow_call_keeps_receiver_and_args() {
    let (_, semantics) = first_adapted_ast("$obj->m(1, 2);");
    assert_eq!(semantics.syntax, CallSyntax::MethodArrow);
    assert_eq!(semantics.dispatch, DispatchKind::Method);
    let receiver =
        must_some_with(semantics.receiver.present(), "method adapter dropped the receiver");
    assert_eq!(receiver.role, OperandRole::Receiver);
    assert_eq!(semantics.arguments.explicit().map(|args| args.len()), Some(2));
}

#[test]
fn indirect_object_syntax() {
    let (_, semantics) = first_adapted_ast("new Class 1, 2;");
    assert_eq!(semantics.syntax, CallSyntax::Indirect);
    assert_eq!(semantics.dispatch, DispatchKind::IndirectObject);
    assert!(matches!(semantics.receiver, OperandPresence::Present(_)));
}

#[test]
fn builtin_list_op_print() {
    let (_, semantics) = first_adapted_ast("print $x, $y;");
    assert_eq!(semantics.syntax, CallSyntax::BuiltinListOp);
    assert_eq!(semantics.dispatch, DispatchKind::Builtin);
    assert_eq!(semantics.arguments.explicit().map(|args| args.len()), Some(2));
}

#[test]
fn core_qualified_call() {
    let (_, semantics) = first_adapted_ast("CORE::open($fh, '<', $path);");
    assert_eq!(semantics.syntax, CallSyntax::CoreQualified);
    assert!(
        matches!(&semantics.callee, perl_ast::CalleeIdentity::Named { name } if name == "CORE::open"),
        "expected CORE::open name field, got {:?}",
        semantics.callee
    );
}

#[test]
fn block_bearing_map() {
    let (_, semantics) = first_adapted_ast("map { $_ } @items;");
    assert_eq!(semantics.syntax, CallSyntax::BlockBearing);
    let blocks = must_some_with(semantics.blocks.explicit(), "block operands");
    assert_eq!(blocks.len(), 1);
    let block = must_some_with(blocks.first(), "first block operand");
    assert_eq!(block.role, OperandRole::Block);
    assert_eq!(semantics.arguments.explicit().map(|args| args.len()), Some(1));
}

#[test]
fn print_filehandle_like_form() {
    let (_, semantics) = first_adapted_ast(r#"print $fh "hello";"#);
    assert_eq!(semantics.syntax, CallSyntax::PrintFilehandleLike);
    assert_eq!(semantics.dispatch, DispatchKind::Builtin);
    let filehandle = must_some_with(
        semantics.filehandle.present(),
        "print $fh LIST must retain a filehandle operand",
    );
    assert_eq!(filehandle.role, OperandRole::Filehandle);
}

#[test]
fn recovered_or_unknown_call_is_boundary() {
    let inner = Node::new(
        NodeKind::FunctionCall {
            name: "foo".to_string(),
            args: vec![Node::new(NodeKind::MissingExpression, loc(4, 4))],
        },
        loc(0, 4),
    );
    let recovered = Node::new(
        NodeKind::Error {
            message: "recovered call".to_string(),
            expected: vec![],
            found: None,
            partial: Some(Box::new(inner)),
        },
        loc(0, 4),
    );
    let semantics = must_some_with(adapt_ast_call(&recovered), "recovered call adapter");
    assert_eq!(semantics.syntax, CallSyntax::RecoveredUnknown);
    assert_eq!(semantics.dispatch, DispatchKind::Boundary);
    assert_eq!(semantics.boundary, CallBoundary::Recovered);
    assert_ne!(semantics.boundary, CallBoundary::Exact);

    let missing_arg = Node::new(
        NodeKind::FunctionCall {
            name: "foo".to_string(),
            args: vec![Node::new(NodeKind::MissingExpression, loc(4, 4))],
        },
        loc(0, 4),
    );
    let missing = must_some_with(adapt_ast_call(&missing_arg), "missing-arg call");
    assert_eq!(missing.dispatch, DispatchKind::Boundary);
    assert_eq!(missing.boundary, CallBoundary::Recovered);
}

#[test]
fn flat_hir_call_does_not_invent_empty_operands() {
    let hir = lower_source("foo(1, 2);");
    let item = must_some_with(
        hir.items.iter().find(|item| matches!(item.kind, HirKind::CallExpr(_))),
        "flat CallExpr",
    );
    let semantics = must_some_with(adapt_flat_hir_call(item), "flat adapter");
    assert_eq!(semantics.syntax, CallSyntax::Named);
    assert!(semantics.arguments.is_unavailable());
    assert_ne!(semantics.argument_source, ArgumentSource::ExplicitEmpty);
    assert!(
        matches!(semantics.arguments, OperandList::Unavailable { known_count: Some(2) }),
        "flat HIR must keep count-only loss, got {:?}",
        semantics.arguments
    );
    assert_eq!(semantics.call_range, item.range);
}

#[test]
fn flat_hir_method_does_not_promote_object_kind_to_receiver() {
    let hir = lower_source("$obj->m(1);");
    let item = must_some_with(
        hir.items.iter().find(|item| matches!(item.kind, HirKind::MethodCallExpr(_))),
        "flat MethodCallExpr",
    );
    let semantics = must_some_with(adapt_flat_hir_call(item), "flat method adapter");
    assert_eq!(semantics.syntax, CallSyntax::MethodArrow);
    assert!(matches!(semantics.receiver, OperandPresence::Unknown));
}

#[test]
fn recovered_flat_hir_is_boundary() {
    let mut parser = Parser::new("foo(");
    let output = parser.parse_with_recovery();
    let hir = lower_ast(&output.ast);
    let Some(item) = hir.items.iter().find(|item| adapt_flat_hir_call(item).is_some()) else {
        return;
    };
    let semantics = must_some_with(adapt_flat_hir_call(item), "flat recovered adapter");
    if item.recovery_confidence != perl_parser_core::hir::RecoveryConfidence::Parsed {
        assert_eq!(semantics.dispatch, DispatchKind::Boundary);
        assert_eq!(semantics.boundary, CallBoundary::Recovered);
    }
}

#[test]
fn body_hir_preserves_argument_order_and_receiver() {
    let hir = lower_source("$obj->m(1, 2);");
    let body = must_some_with(hir.root_body(), "root body");
    let (id, semantics) = must_some_with(
        body.exprs.iter().enumerate().find_map(|(i, expr)| {
            matches!(expr, HirExpr::Call { ast_kind, .. } if ast_kind == "MethodCall")
                .then(|| perl_parser_core::hir::HirExprId(i as u32))
                .and_then(|id| adapt_body_hir_call(body, id).map(|sem| (id, sem)))
        }),
        "body MethodCall",
    );
    assert_eq!(semantics.syntax, CallSyntax::MethodArrow);
    assert!(matches!(semantics.receiver, OperandPresence::Present(_)));
    let args = must_some_with(semantics.arguments.explicit(), "body args");
    assert_eq!(args.len(), 2);
    assert!(args.windows(2).all(|pair| {
        must_some_with(pair.first(), "left body arg").source_order
            < must_some_with(pair.get(1), "right body arg").source_order
    }));
    assert_eq!(semantics.call_range, must_some_with(body.source_map.expr_range(id), "body range"));
}

fn first_body_call_with_kind(source: &str, kind: &str) -> perl_ast::CallSemantics {
    let hir = lower_source(source);
    let body = must_some_with(hir.root_body(), "root body");
    must_some_with(
        body.exprs.iter().enumerate().find_map(|(i, expr)| {
            matches!(expr, HirExpr::Call { ast_kind, .. } if ast_kind == kind)
                .then(|| perl_parser_core::hir::HirExprId(i as u32))
                .and_then(|id| adapt_body_hir_call(body, id))
        }),
        format!("body {kind} in {source}"),
    )
}

#[test]
fn body_hir_refuses_eval_and_other_non_call_call_shells() {
    let hir = lower_source("eval { 1 };");
    let body = must_some_with(hir.root_body(), "root body");
    let eval_id = must_some_with(
        body.exprs.iter().enumerate().find_map(|(i, expr)| {
            matches!(expr, HirExpr::Call { ast_kind, .. } if ast_kind == "Eval")
                .then(|| perl_parser_core::hir::HirExprId(i as u32))
        }),
        "body Eval shell",
    );
    assert!(
        adapt_body_hir_call(body, eval_id).is_none(),
        "eval must not adapt as a named function call"
    );
}

#[test]
fn body_hir_function_call_does_not_treat_coderef_callee_as_an_argument_role_split() {
    let semantics = first_body_call_with_kind("$cb->(1);", "FunctionCall");
    assert_eq!(semantics.syntax, CallSyntax::RecoveredUnknown);
    assert_eq!(semantics.dispatch, DispatchKind::Boundary);
    assert!(matches!(semantics.callee_operand, OperandPresence::Unknown));
    assert!(semantics.limitations.contains(&CallLimitation::BodyHirFunctionCallFormUnavailable));
    let args = must_some_with(semantics.arguments.explicit(), "body function-call args");
    assert_eq!(args.len(), 2);
    assert!(args.iter().all(|operand| operand.role == OperandRole::Argument));
}

#[test]
fn body_hir_indirect_does_not_claim_print_filehandle_as_method_receiver() {
    let semantics = first_body_call_with_kind(r#"print $fh "hello";"#, "IndirectCall");
    assert_eq!(semantics.syntax, CallSyntax::Indirect);
    assert_eq!(semantics.dispatch, DispatchKind::Boundary);
    assert!(matches!(semantics.receiver, OperandPresence::Absent | OperandPresence::Unknown));
    assert!(matches!(semantics.filehandle, OperandPresence::Absent | OperandPresence::Unknown));
    assert!(
        semantics.limitations.contains(&CallLimitation::BodyHirIndirectInvocantRoleUnavailable)
    );
}

#[test]
fn flat_hir_dynamic_method_is_not_statically_resolved() {
    let hir = lower_source("$obj->$name();");
    let item = must_some_with(
        hir.items.iter().find(|item| matches!(item.kind, HirKind::MethodCallExpr(_))),
        "flat dynamic MethodCallExpr",
    );
    let semantics = must_some_with(adapt_flat_hir_call(item), "flat dynamic method adapter");
    assert_eq!(semantics.syntax, CallSyntax::MethodArrow);
    assert_eq!(semantics.dispatch, DispatchKind::Dynamic);
    assert_eq!(semantics.boundary, CallBoundary::Dynamic);
    assert!(semantics.limitations.contains(&CallLimitation::DynamicMethodNameTextOnly));
}
