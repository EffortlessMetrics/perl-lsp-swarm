//! Black-box compatibility matrix for nested aggregate receiver chains (#13103).
//!
//! Drives only public parser, type-inference, type-fact, and receiver-fact APIs.
//! This is compatibility proof for the recursive substrate, not canonical #9474
//! completion or support-tier evidence.

use perl_semantic_analyzer::analysis::receiver_facts::{
    ARRAY_INDEX_RECEIVER_REASON, DYNAMIC_HASH_KEY_RECEIVER_REASON, ReceiverFact,
    ReceiverFactContext, ReceiverFallbackState, ReceiverKind, receiver_fact_for_method_call,
};
use perl_semantic_analyzer::analysis::type_facts::{
    ArrayShape, DynamicBoundary, HashShape, ObjectShape, ShapeFact, TypeEvidence, TypeFact,
};
use perl_semantic_analyzer::analysis::type_inference::{
    PerlType, TypeEnvironment, TypeInferenceEngine,
};
use perl_semantic_analyzer::{Node, NodeKind, Parser};
use perl_semantic_facts::Confidence;
use std::collections::BTreeMap;

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

fn receiver_fact_for_env(
    code: &str,
    method: &str,
    env: &TypeEnvironment,
) -> Result<ReceiverFact, String> {
    let ast = parse_ast(code)?;
    let call = method_call_named(&ast, method).ok_or_else(|| format!("missing {method} call"))?;
    Ok(receiver_fact_for_method_call(call, ReceiverFactContext::new(Some(env)).with_source(code)))
}

fn hash_type() -> PerlType {
    PerlType::Hash { key: Box::new(PerlType::Any), value: Box::new(PerlType::Any) }
}

fn literal_shape_fact(ty: PerlType, shape: ShapeFact) -> TypeFact {
    let mut fact = TypeFact::new(ty, Confidence::High);
    fact.evidence = vec![TypeEvidence::Literal];
    fact.shape = Some(shape);
    fact
}

fn object_fact(package: &str, confidence: Confidence) -> TypeFact {
    let mut fact = TypeFact::new(PerlType::Object(package.to_string()), confidence);
    fact.evidence = vec![TypeEvidence::WorkspaceSymbol { package: package.to_string() }];
    fact.shape = Some(ShapeFact::Object(ObjectShape::new(package.to_string(), BTreeMap::new())));
    fact
}

fn hash_shape_fact(slot: &str, package: &str) -> TypeFact {
    let mut slots = BTreeMap::new();
    slots.insert(slot.to_string(), object_fact(package, Confidence::High));
    literal_shape_fact(hash_type(), ShapeFact::Hash(HashShape::new(slots, None)))
}

fn array_shape_fact(index: usize, package: &str) -> TypeFact {
    let mut indexed = BTreeMap::new();
    indexed.insert(index, object_fact(package, Confidence::High));
    literal_shape_fact(
        PerlType::Array(Box::new(PerlType::Any)),
        ShapeFact::Array(ArrayShape::new(indexed, None)),
    )
}

fn hash_of_array_shape_fact(slot: &str, index: usize, package: &str) -> TypeFact {
    let mut slots = BTreeMap::new();
    slots.insert(slot.to_string(), array_shape_fact(index, package));
    literal_shape_fact(hash_type(), ShapeFact::Hash(HashShape::new(slots, None)))
}

fn hash_of_hash_shape_fact(outer_slot: &str, inner_slot: &str, package: &str) -> TypeFact {
    let mut inner_slots = BTreeMap::new();
    inner_slots.insert(inner_slot.to_string(), object_fact(package, Confidence::High));
    let inner = literal_shape_fact(hash_type(), ShapeFact::Hash(HashShape::new(inner_slots, None)));
    let mut slots = BTreeMap::new();
    slots.insert(outer_slot.to_string(), inner);
    literal_shape_fact(hash_type(), ShapeFact::Hash(HashShape::new(slots, None)))
}

fn array_of_hash_shape_fact(index: usize, slot: &str, package: &str) -> TypeFact {
    let mut indexed = BTreeMap::new();
    indexed.insert(index, hash_shape_fact(slot, package));
    literal_shape_fact(
        PerlType::Array(Box::new(PerlType::Any)),
        ShapeFact::Array(ArrayShape::new(indexed, None)),
    )
}

fn array_union_shape_fact(index: usize, first: &str, second: &str) -> TypeFact {
    let mut union = TypeFact::new(
        PerlType::Union(vec![
            PerlType::Object(first.to_string()),
            PerlType::Object(second.to_string()),
        ]),
        Confidence::High,
    );
    union.evidence = vec![TypeEvidence::WorkspaceSymbol { package: first.to_string() }];
    let mut indexed = BTreeMap::new();
    indexed.insert(index, union);
    literal_shape_fact(
        PerlType::Array(Box::new(PerlType::Any)),
        ShapeFact::Array(ArrayShape::new(indexed, None)),
    )
}

// --- static positives (source-derived) ---

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
fn hashref_of_arrays_resolves_through_public_receiver_api() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my $groups = { staff => [My::User->new] }; $groups->{staff}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { base, key } if base == "$groups" && key == "staff"
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
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashSlot { hash, key } if hash != "literal" && key == "staff"
        )
    }));
    assert!(!fact.evidence.iter().any(|evidence| {
        matches!(evidence, TypeEvidence::HashRefSlot { key, .. } if key == "staff")
    }));
    assert!(
        fact.evidence
            .iter()
            .any(|evidence| evidence.is_heuristic_reason(ARRAY_INDEX_RECEIVER_REASON))
    );
    assert!(
        !fact
            .evidence
            .iter()
            .any(|evidence| evidence.is_heuristic_reason(DYNAMIC_HASH_KEY_RECEIVER_REASON))
    );
    Ok(())
}

#[test]
#[ignore = "needs #9474: plain hash slot then hashref slot then array index exact object package"]
fn plain_hash_then_hashref_slot_resolves_through_public_receiver_api() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my %root = (outer => \\{ staff => [My::User->new] }); $root{outer}->{staff}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashSlot { hash, key } if hash == "$root" && key == "outer"
        )
    }));
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { key, .. } if key == "staff"
        )
    }));
    Ok(())
}

#[test]
fn plain_hash_then_hashref_slot_stays_fallback_on_current_main() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my %root = (outer => \\{ staff => [My::User->new] }); $root{outer}->{staff}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package, None);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
#[ignore = "needs #9474: source-derived array index then hash slot exact object package"]
fn array_then_hash_slot_resolves_through_public_receiver_api() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my @items = ({ leaf => My::User->new }); $items[0]{leaf}->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    assert!(
        fact.evidence
            .iter()
            .any(|evidence| evidence.is_heuristic_reason(ARRAY_INDEX_RECEIVER_REASON))
    );
    Ok(())
}

#[test]
fn array_then_hash_slot_stays_fallback_on_current_main() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my @items = ({ leaf => My::User->new }); $items[0]{leaf}->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package, None);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

// --- static positives (supplied environment) ---

#[test]
fn supplied_environment_hashref_of_arrays_matches_public_route() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    env.set_variable_fact("config".to_string(), hash_of_array_shape_fact("db", 1, "My::Handle"));

    let fact = receiver_fact_for_env("$config->{db}[1]->call();", "call", &env)?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package.as_deref(), Some("My::Handle"));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { base, key } if base.contains("config") && key == "db"
        )
    }));
    Ok(())
}

#[test]
fn supplied_environment_nested_hash_slots_match_public_route() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    env.set_variable_fact(
        "data".to_string(),
        hash_of_hash_shape_fact("outer", "inner", "My::Leaf"),
    );

    let fact = receiver_fact_for_env("$data{outer}{inner}->render();", "render", &env)?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package.as_deref(), Some("My::Leaf"));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    Ok(())
}

// --- dynamic boundaries and negative controls ---

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
    assert!(
        fact.evidence
            .iter()
            .any(|evidence| evidence.is_heuristic_reason(DYNAMIC_HASH_KEY_RECEIVER_REASON))
    );
    Ok(())
}

#[test]
fn dynamic_hash_key_before_static_index_fails_closed() -> Result<(), String> {
    let fact = receiver_fact_for_source(
        "my %groups = (staff => [My::User->new]); my $bucket = 'staff'; $groups{$bucket}[0]->render();",
        "render",
    )?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package, None);
    assert!(fact.candidate_packages.is_empty());
    assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::DynamicHashKey));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
fn dynamic_index_before_static_hash_slot_fails_closed() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    env.set_variable_fact("items".to_string(), array_of_hash_shape_fact(0, "leaf", "My::User"));

    let fact = receiver_fact_for_env("$items[$i]{leaf}->render();", "render", &env)?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package, None);
    assert_eq!(fact.dynamic_boundary, Some(DynamicBoundary::UnknownReceiver));
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
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
fn missing_outer_hash_slot_cannot_recover_to_exact() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    env.set_variable_fact(
        "data".to_string(),
        hash_of_hash_shape_fact("outer", "inner", "My::Leaf"),
    );

    let fact = receiver_fact_for_env("$data{missing}{inner}->render();", "render", &env)?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package, None);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
fn out_of_range_array_index_cannot_recover_to_exact() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    env.set_variable_fact("groups".to_string(), hash_of_array_shape_fact("staff", 0, "My::User"));

    let fact = receiver_fact_for_env("$groups{staff}[9]->render();", "render", &env)?;

    assert_eq!(fact.kind, ReceiverKind::ArrayIndex);
    assert_eq!(fact.package, None);
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

#[test]
fn wrong_shape_array_element_cannot_be_admitted_as_hash_slot() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    env.set_variable_fact("items".to_string(), array_shape_fact(0, "My::Item"));

    let fact = receiver_fact_for_env("$items[0]{field}->render();", "render", &env)?;

    assert_eq!(fact.kind, ReceiverKind::HashSlot);
    assert_eq!(fact.package, None);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

// --- confidence, unions, shadowing ---

#[test]
fn traversed_medium_confidence_cannot_upgrade_to_exact() -> Result<(), String> {
    let mut container = hash_of_array_shape_fact("staff", 0, "My::User");
    container.confidence = Confidence::Medium;
    let mut env = TypeEnvironment::new();
    env.set_variable_fact("groups".to_string(), container);

    let fact = receiver_fact_for_env("$groups{staff}[0]->render();", "render", &env)?;

    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.confidence, Confidence::Medium);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
fn finite_array_union_retains_all_candidates_without_exactness() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    let mut slots = BTreeMap::new();
    slots.insert("staff".to_string(), array_union_shape_fact(0, "My::User", "My::Admin"));
    env.set_variable_fact(
        "groups".to_string(),
        literal_shape_fact(hash_type(), ShapeFact::Hash(HashShape::new(slots, None))),
    );

    let fact = receiver_fact_for_env("$groups{staff}[0]->render();", "render", &env)?;

    assert_eq!(fact.package.as_deref(), Some("My::User"));
    assert_eq!(fact.candidate_packages, vec!["My::User", "My::Admin"]);
    assert!(fact.is_union_receiver());
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
fn mixed_object_scalar_union_yields_no_candidates() -> Result<(), String> {
    let mut env = TypeEnvironment::new();
    let mixed_union = TypeFact::new(
        PerlType::Union(vec![
            PerlType::Object("My::Widget".to_string()),
            PerlType::Scalar(perl_semantic_analyzer::analysis::type_inference::ScalarType::String),
        ]),
        Confidence::High,
    );
    env.set_variable_fact("obj".to_string(), mixed_union);

    let fact = receiver_fact_for_env("$obj->run();", "run", &env)?;

    assert!(fact.candidate_packages.is_empty());
    assert!(!fact.is_union_receiver());
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Fallback);
    Ok(())
}

#[test]
fn shadowed_environment_fact_wins_on_public_route() -> Result<(), String> {
    let mut parent = TypeEnvironment::new();
    parent.set_variable_fact(
        "groups".to_string(),
        hash_of_array_shape_fact("staff", 0, "Outer::User"),
    );
    let mut env = TypeEnvironment::with_parent(parent);
    env.set_variable_fact(
        "groups".to_string(),
        hash_of_array_shape_fact("staff", 0, "Inner::User"),
    );

    let fact = receiver_fact_for_env("$groups{staff}[0]->render();", "render", &env)?;

    assert_eq!(fact.package.as_deref(), Some("Inner::User"));
    assert_eq!(fact.candidate_packages, vec!["Inner::User"]);
    assert_eq!(fact.fallback_state, ReceiverFallbackState::Exact);
    Ok(())
}

#[test]
fn malformed_receiver_does_not_fabricate_exact_fact() -> Result<(), String> {
    let code = "$groups{staff}[0]->render(";
    let mut parser = Parser::new(code);
    let parsed = parser.parse();
    if let Ok(ast) = parsed
        && let Some(call) = method_call_named(&ast, "render")
    {
        let env = TypeEnvironment::new();
        let fact = receiver_fact_for_method_call(
            call,
            ReceiverFactContext::new(Some(&env)).with_source(code),
        );
        if fact.package.is_some() || fact.fallback_state == ReceiverFallbackState::Exact {
            return Err(format!("malformed receiver fabricated exact fact: {fact:?}"));
        }
    }
    Ok(())
}
