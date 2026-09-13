use perl_semantic_analyzer::analysis::receiver_facts::{
    ReceiverFact, ReceiverFactContext, ReceiverFallbackState, ReceiverKind,
    receiver_fact_for_method_call,
};
use perl_semantic_analyzer::analysis::type_facts::{DynamicBoundary, TypeEvidence};
use perl_semantic_analyzer::analysis::type_inference::TypeInferenceEngine;
use perl_semantic_analyzer::{Node, NodeKind, Parser};
use perl_semantic_facts::Confidence;

fn parse_ast(code: &str) -> Result<Node, String> {
    let mut parser = Parser::new(code);
    parser.parse().map_err(|error| format!("parse failed: {error:?}"))
}

fn method_call_named<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if let NodeKind::MethodCall { method, .. } = &node.kind
        && method == name
    {
        return Some(node);
    }

    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            statements.iter().find_map(|child| method_call_named(child, name))
        }
        NodeKind::ExpressionStatement { expression } => method_call_named(expression, name),
        NodeKind::VariableDeclaration { initializer, .. } => {
            initializer.as_deref().and_then(|child| method_call_named(child, name))
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            method_call_named(lhs, name).or_else(|| method_call_named(rhs, name))
        }
        NodeKind::MethodCall { object, args, .. } => method_call_named(object, name)
            .or_else(|| args.iter().find_map(|child| method_call_named(child, name))),
        NodeKind::Binary { left, right, .. } => {
            method_call_named(left, name).or_else(|| method_call_named(right, name))
        }
        NodeKind::ArrayLiteral { elements } => {
            elements.iter().find_map(|child| method_call_named(child, name))
        }
        NodeKind::HashLiteral { pairs } => pairs.iter().find_map(|(key, value)| {
            method_call_named(key, name).or_else(|| method_call_named(value, name))
        }),
        _ => None,
    }
}

fn receiver_fact_for_source(code: &str, method: &str) -> Result<ReceiverFact, String> {
    let ast = parse_ast(code)?;
    let call = method_call_named(&ast, method).ok_or_else(|| format!("missing {method} call"))?;
    let mut engine = TypeInferenceEngine::new();
    engine.infer(&ast).map_err(|errors| format!("inference failed: {errors:?}"))?;

    Ok(receiver_fact_for_method_call(
        call,
        ReceiverFactContext::new(Some(engine.environment())).with_source(code),
    ))
}

#[test]
fn plain_hash_of_arrays_resolves_through_public_receiver_api() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my %groups = (staff => [My::User->new]); $groups{staff}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.candidate_packages, vec!["My::User"]);
    assert_eq!(fact.confidence, Confidence::High);
    assert_eq!(fact.dynamic_boundary, None);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashSlot { hash, key } if hash == "$groups" && key == "staff"
        )
    }));
    Ok(())
}

#[test]
fn mixed_hashref_then_plain_hash_keeps_each_local_operator() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my $root = { outer => { staff => [My::User->new] } }; $root->{outer}{staff}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { base, key } if base == "$root" && key == "outer"
        )
    }));
    // Access evidence only: the initializer path labels literal slots with
    // hash "literal" (type_inference hash_literal_fact), so requiring a
    // non-literal base pins the walk-produced {staff} hop itself.
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashSlot { hash, key } if hash != "literal" && key == "staff"
        )
    }));
    assert!(!fact.evidence.iter().any(|evidence| {
        matches!(evidence, TypeEvidence::HashRefSlot { key, .. } if key == "staff")
    }));
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::Heuristic { reason } if reason == "array index receiver"
        )
    }));
    Ok(())
}

#[test]
fn dynamic_key_boundary_survives_later_static_steps() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my $root = { staff => [My::User->new] }; my $key = 'staff'; $root->{$key}[0]{leaf}->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package, None);
    assert!(fact.candidate_packages.is_empty());
    assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::DynamicHashKey));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::Heuristic { reason } if reason == "hash receiver key is dynamic"
        )
    }));
    Ok(())
}

#[test]
fn missing_intermediate_slot_cannot_recover_to_exact() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my $root = { staff => [My::User->new] }; $root->{missing}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package, None);
    assert!(fact.candidate_packages.is_empty());
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
fn wrong_intermediate_shape_cannot_recover_to_exact() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my $root = { staff => My::User->new }; $root->{staff}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package, None);
    assert!(fact.candidate_packages.is_empty());
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}
