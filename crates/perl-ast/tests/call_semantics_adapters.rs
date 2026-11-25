//! Adapter matrix for versioned [`perl_ast::CallSemantics`] (#8851).
//!
//! Each current AST call form is adapted from fields only. These tests fail when
//! a mutation conflates syntax and dispatch, guesses `&foo` vs `&foo()`, treats
//! missing operands as an empty list, or promotes recovery to exact.

use perl_ast::{
    ArgumentSource, CALL_SEMANTICS_SCHEMA_WIRE, CODEREF_CALL_SENTINEL, CallBoundary,
    CallContradiction, CallLimitation, CallOperand, CallSemantics, CallSyntax, CalleeIdentity,
    CurrentRepresentation, DispatchKind, Node, NodeKind, OperandPresence, OperandRole,
    SourceLocation, adapt_ast_call,
};
use perl_test_must::must_some_with;

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn number(start: usize, end: usize, value: &str) -> Node {
    Node::new(NodeKind::Number { value: value.to_string() }, loc(start, end))
}

fn variable(start: usize, end: usize, sigil: &str, name: &str) -> Node {
    Node::new(
        NodeKind::Variable { sigil: sigil.to_string(), name: name.to_string() },
        loc(start, end),
    )
}

fn block(start: usize, end: usize) -> Node {
    Node::new(NodeKind::Block { statements: vec![] }, loc(start, end))
}

fn adapt(node: &Node) -> CallSemantics {
    must_some_with(
        adapt_ast_call(node),
        format!("expected call adapter for {}", node.kind.kind_name()),
    )
}

fn argument_ranges(semantics: &CallSemantics) -> Vec<SourceLocation> {
    must_some_with(semantics.arguments.explicit(), "arguments must be explicit for AST adapters")
        .iter()
        .filter_map(|operand| operand.source_range)
        .collect()
}

fn present_operand<'a>(presence: &'a OperandPresence, context: &str) -> &'a CallOperand {
    must_some_with(presence.present(), context)
}

fn first_operand<'a>(operands: &'a [CallOperand], context: &str) -> &'a CallOperand {
    must_some_with(operands.first(), context)
}

#[test]
fn named_call_preserves_exact_range_and_argument_order() {
    let call = Node::new(
        NodeKind::FunctionCall {
            name: "foo".to_string(),
            args: vec![number(4, 5, "1"), number(7, 8, "2")],
        },
        loc(0, 9),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::Named);
    assert_eq!(semantics.dispatch, DispatchKind::Function);
    assert_eq!(semantics.call_range, loc(0, 9));
    assert_eq!(semantics.callee, CalleeIdentity::Named { name: "foo".to_string() });
    assert_eq!(semantics.callee_anchor, None);
    assert_eq!(semantics.argument_source, ArgumentSource::ExplicitList);
    assert_eq!(argument_ranges(&semantics), vec![loc(4, 5), loc(7, 8)]);
    assert_eq!(semantics.schema_wire(), CALL_SEMANTICS_SCHEMA_WIRE);
    assert!(semantics.contradictions().is_empty());
}

#[test]
fn empty_named_call_is_not_bare_and_does_not_guess_parens() {
    let call =
        Node::new(NodeKind::FunctionCall { name: "foo".to_string(), args: vec![] }, loc(0, 5));
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::Named);
    assert_ne!(semantics.syntax, CallSyntax::Bare);
    assert_eq!(semantics.argument_source, ArgumentSource::Unknown);
    assert_eq!(semantics.arguments.explicit().map(Vec::from), Some(vec![]));
    assert!(semantics.limitations.contains(&CallLimitation::NamedParensFormUnavailable));
}

#[test]
fn ampersand_empty_args_stay_unknown_not_implicit_or_explicit_empty() {
    let without_parens =
        Node::new(NodeKind::AmperCall { name: "foo".to_string(), args: vec![] }, loc(0, 4));
    let with_parens =
        Node::new(NodeKind::AmperCall { name: "foo".to_string(), args: vec![] }, loc(0, 6));
    for node in [&without_parens, &with_parens] {
        let semantics = adapt(node);
        assert_eq!(semantics.syntax, CallSyntax::Ampersand);
        assert_eq!(semantics.dispatch, DispatchKind::Function);
        assert_eq!(semantics.call_range, node.location);
        assert_eq!(semantics.argument_source, ArgumentSource::Unknown);
        assert_ne!(semantics.argument_source, ArgumentSource::ImplicitCallerArgs);
        assert_ne!(semantics.argument_source, ArgumentSource::ExplicitEmpty);
        assert!(semantics.limitations.contains(&CallLimitation::AmpersandParensFormUnavailable));
    }
}

#[test]
fn ampersand_explicit_list_preserves_order() {
    let call = Node::new(
        NodeKind::AmperCall {
            name: "foo".to_string(),
            args: vec![number(5, 6, "1"), number(8, 9, "2")],
        },
        loc(0, 10),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::Ampersand);
    assert_eq!(semantics.argument_source, ArgumentSource::ExplicitList);
    assert_eq!(argument_ranges(&semantics), vec![loc(5, 6), loc(8, 9)]);
}

#[test]
fn coderef_sentinel_is_not_canonical_callee_and_first_arg_is_callee_operand() {
    let cb = variable(0, 3, "$", "cb");
    let arg = number(7, 8, "1");
    let call = Node::new(
        NodeKind::FunctionCall { name: CODEREF_CALL_SENTINEL.to_string(), args: vec![cb, arg] },
        loc(0, 9),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::Coderef);
    assert_eq!(semantics.dispatch, DispatchKind::Coderef);
    assert_eq!(semantics.callee, CalleeIdentity::SentinelCoderef);
    assert!(matches!(semantics.receiver, OperandPresence::Absent));
    let callee = present_operand(
        &semantics.callee_operand,
        "coderef must retain the callee expression operand",
    );
    assert_eq!(callee.role, OperandRole::Callee);
    assert_eq!(callee.source_range, Some(loc(0, 3)));
    let args = must_some_with(semantics.arguments.explicit(), "coderef args");
    assert_eq!(args.len(), 1);
    let first = first_operand(args, "coderef argument operand");
    assert_eq!(first.role, OperandRole::Argument);
    assert_eq!(first.source_range, Some(loc(7, 8)));
    assert!(semantics.limitations.contains(&CallLimitation::CoderefSentinelNotCanonical));
}

#[test]
fn method_arrow_keeps_receiver_operand_not_object_kind_string() {
    let object = variable(0, 4, "$", "obj");
    let call = Node::new(
        NodeKind::MethodCall {
            object: Box::new(object),
            method: "m".to_string(),
            args: vec![number(8, 9, "1")],
        },
        loc(0, 10),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::MethodArrow);
    assert_eq!(semantics.dispatch, DispatchKind::Method);
    assert_eq!(semantics.call_range, loc(0, 10));
    let receiver =
        present_operand(&semantics.receiver, "method call must retain the receiver operand");
    assert_eq!(receiver.role, OperandRole::Receiver);
    assert_eq!(receiver.source_range, Some(loc(0, 4)));
    assert_eq!(argument_ranges(&semantics), vec![loc(8, 9)]);
}

#[test]
fn dynamic_method_text_is_boundary_dynamic_not_resolved_method() {
    let call = Node::new(
        NodeKind::MethodCall {
            object: Box::new(variable(0, 4, "$", "obj")),
            method: "$name".to_string(),
            args: vec![],
        },
        loc(0, 12),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::MethodArrow);
    assert_eq!(semantics.dispatch, DispatchKind::Dynamic);
    assert_eq!(semantics.boundary, CallBoundary::Dynamic);
    assert!(semantics.limitations.contains(&CallLimitation::DynamicMethodNameTextOnly));
}

#[test]
fn indirect_object_keeps_receiver_and_arguments_separate() {
    let class = Node::new(NodeKind::Identifier { name: "Class".to_string() }, loc(4, 9));
    let call = Node::new(
        NodeKind::IndirectCall {
            method: "new".to_string(),
            object: Box::new(class),
            args: vec![number(10, 11, "1")],
        },
        loc(0, 12),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::Indirect);
    assert_eq!(semantics.dispatch, DispatchKind::IndirectObject);
    let receiver =
        present_operand(&semantics.receiver, "indirect call must retain the invocant operand");
    assert_eq!(receiver.role, OperandRole::Receiver);
    assert_eq!(receiver.source_range, Some(loc(4, 9)));
    assert_eq!(argument_ranges(&semantics), vec![loc(10, 11)]);
    assert!(matches!(semantics.filehandle, OperandPresence::Absent));
}

#[test]
fn builtin_list_op_uses_name_field_not_source_scan() {
    let call = Node::new(
        NodeKind::FunctionCall { name: "print".to_string(), args: vec![variable(6, 8, "$", "x")] },
        loc(0, 8),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::BuiltinListOp);
    assert_eq!(semantics.dispatch, DispatchKind::Builtin);
    assert_eq!(semantics.call_range, loc(0, 8));
    assert_eq!(argument_ranges(&semantics), vec![loc(6, 8)]);
}

#[test]
fn core_qualified_name_field_is_core_syntax() {
    let call = Node::new(
        NodeKind::FunctionCall {
            name: "CORE::open".to_string(),
            args: vec![variable(11, 15, "$", "fh")],
        },
        loc(0, 16),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::CoreQualified);
    assert_eq!(semantics.dispatch, DispatchKind::Builtin);
    assert_eq!(semantics.callee, CalleeIdentity::Named { name: "CORE::open".to_string() });
}

#[test]
fn block_bearing_map_keeps_block_out_of_ordinary_arguments() {
    let call = Node::new(
        NodeKind::FunctionCall {
            name: "map".to_string(),
            args: vec![block(4, 9), variable(10, 16, "@", "items")],
        },
        loc(0, 16),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::BlockBearing);
    assert_eq!(semantics.dispatch, DispatchKind::Builtin);
    let blocks = must_some_with(semantics.blocks.explicit(), "block operands");
    assert_eq!(blocks.len(), 1);
    let block = first_operand(blocks, "block-bearing first block");
    assert_eq!(block.role, OperandRole::Block);
    assert_eq!(block.source_range, Some(loc(4, 9)));
    assert_eq!(argument_ranges(&semantics), vec![loc(10, 16)]);
}

#[test]
fn print_filehandle_indirect_uses_filehandle_role_not_ordinary_argument() {
    let call = Node::new(
        NodeKind::IndirectCall {
            method: "print".to_string(),
            object: Box::new(variable(6, 9, "$", "fh")),
            args: vec![number(10, 13, "1")],
        },
        loc(0, 13),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.syntax, CallSyntax::PrintFilehandleLike);
    assert_eq!(semantics.dispatch, DispatchKind::Builtin);
    assert!(matches!(semantics.receiver, OperandPresence::Absent));
    let filehandle =
        present_operand(&semantics.filehandle, "print-like form must retain a filehandle operand");
    assert_eq!(filehandle.role, OperandRole::Filehandle);
    assert_eq!(filehandle.source_range, Some(loc(6, 9)));
    assert_eq!(argument_ranges(&semantics), vec![loc(10, 13)]);
}

#[test]
fn recovered_partial_call_is_boundary_not_exact() {
    let inner =
        Node::new(NodeKind::FunctionCall { name: "foo".to_string(), args: vec![] }, loc(0, 4));
    let recovered = Node::new(
        NodeKind::Error {
            message: "recovered call".to_string(),
            expected: vec![],
            found: None,
            partial: Some(Box::new(inner)),
        },
        loc(0, 4),
    );
    let semantics = adapt(&recovered);
    assert_eq!(semantics.syntax, CallSyntax::RecoveredUnknown);
    assert_eq!(semantics.dispatch, DispatchKind::Boundary);
    assert_eq!(semantics.boundary, CallBoundary::Recovered);
    assert_eq!(semantics.representation, CurrentRepresentation::AstRecovered);
    assert_ne!(semantics.boundary, CallBoundary::Exact);
    assert!(semantics.limitations.contains(&CallLimitation::RecoveredSource));
}

#[test]
fn non_call_nodes_are_refused() {
    let node = number(0, 1, "1");
    assert!(adapt_ast_call(&node).is_none());
}

#[test]
fn empty_args_are_not_promoted_to_explicit_empty() {
    let call =
        Node::new(NodeKind::FunctionCall { name: "foo".to_string(), args: vec![] }, loc(0, 5));
    let mut semantics = adapt(&call);
    assert_eq!(semantics.argument_source, ArgumentSource::Unknown);
    semantics.argument_source = ArgumentSource::ExplicitEmpty;
    assert!(
        !semantics.contradictions().contains(&CallContradiction::EmptyArgsGuessed),
        "proven ExplicitEmpty with Explicit([]) is valid; adapters must not emit it"
    );
    semantics.argument_source = ArgumentSource::ImplicitCallerArgs;
    assert!(semantics.contradictions().contains(&CallContradiction::EmptyArgsGuessed));
}

#[test]
fn recovery_hole_is_not_a_source_backed_argument_identity() {
    let call = Node::new(
        NodeKind::FunctionCall {
            name: "foo".to_string(),
            args: vec![Node::new(NodeKind::MissingExpression, loc(4, 4))],
        },
        loc(0, 4),
    );
    let semantics = adapt(&call);
    assert_eq!(semantics.dispatch, DispatchKind::Boundary);
    assert_eq!(semantics.boundary, CallBoundary::Recovered);
    let args = must_some_with(semantics.arguments.explicit(), "recovered args retained for order");
    let hole = first_operand(args, "recovery hole operand slot");
    assert_eq!(hole.source_range, None);
    assert_eq!(hole.evaluation, perl_ast::EvaluationContract::NotApplicable);
    assert!(semantics.limitations.contains(&CallLimitation::RecoveredHoleNotAnOperandIdentity));
}
