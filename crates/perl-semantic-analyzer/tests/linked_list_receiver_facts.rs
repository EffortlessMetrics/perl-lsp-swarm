//! Regression coverage for recursive linked-list receiver facts.
//!
//! These tests establish the source-backed linked-node patterns that already
//! work before #13121 changes post-construction object-field assignment.

use perl_semantic_analyzer::analysis::type_facts::{ShapeFact, TypeEvidence};
use perl_semantic_analyzer::analysis::type_inference::{PerlType, TypeInferenceEngine};
use perl_semantic_analyzer::{Node, NodeKind, Parser};
use perl_semantic_facts::Confidence;

fn parse_ast(code: &str) -> Result<Node, String> {
    let mut parser = Parser::new(code);
    parser.parse().map_err(|err| format!("parse failed: {err:?}"))
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

fn method_receiver<'a>(node: &'a Node, name: &str) -> Result<&'a Node, String> {
    let call = method_call_named(node, name).ok_or_else(|| format!("missing {name} call"))?;
    match &call.kind {
        NodeKind::MethodCall { object, .. } => Ok(object),
        _ => Err(format!("{name} is not a method call")),
    }
}

fn object_shape_package(
    fact: &perl_semantic_analyzer::analysis::type_facts::TypeFact,
) -> Result<&str, String> {
    let shape = fact.shape.as_ref().ok_or_else(|| "missing object shape".to_string())?;
    match shape {
        ShapeFact::Object(object) => Ok(object.package.as_str()),
        _ => Err("fact should carry object shape".to_string()),
    }
}

#[test]
fn moo_recursive_node_accessor_keeps_link_receiver_package() -> Result<(), String> {
    let code = "package LinkedList::Node; use Moo; has next => (is => 'ro', isa => 'LinkedList::Node'); package main; my $head = LinkedList::Node->new; $head->next->value;";
    let ast = parse_ast(code)?;
    let mut engine = TypeInferenceEngine::new();

    engine.infer(&ast).map_err(|err| format!("inference failed: {err:?}"))?;

    let receiver = method_receiver(&ast, "value")?;
    let fact = engine.infer_expr_fact(receiver);

    assert_eq!(fact.ty, PerlType::Any);
    assert_eq!(fact.confidence, Confidence::Medium);
    assert_eq!(object_shape_package(&fact)?, "LinkedList::Node");
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::MooseIsa { attr, isa }
                if attr == "next" && isa == "LinkedList::Node"
        )
    }));
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::AccessorReturn { method, field }
                if method == "next" && field == "next"
        )
    }));
    Ok(())
}

#[test]
fn blessed_recursive_link_initializer_keeps_node_receiver_fact() -> Result<(), String> {
    let code = "my $head = bless { next => LinkedList::Node->new }, 'LinkedList::Node'; $head->{next}->value;";
    let ast = parse_ast(code)?;
    let mut engine = TypeInferenceEngine::new();

    engine.infer(&ast).map_err(|err| format!("inference failed: {err:?}"))?;

    let head = engine.get_fact_at("head").ok_or_else(|| "missing head fact".to_string())?;
    assert_eq!(head.ty, PerlType::Object("LinkedList::Node".to_string()));
    assert_eq!(head.confidence, Confidence::Medium);
    let ShapeFact::Object(head_shape) =
        head.shape.as_ref().ok_or_else(|| "missing head object shape".to_string())?
    else {
        return Err("head fact should carry object shape".to_string());
    };
    assert!(head_shape.fields.contains_key("next"));

    let receiver = method_receiver(&ast, "value")?;
    let fact = engine.infer_expr_fact(receiver);

    assert_eq!(fact.ty, PerlType::Object("LinkedList::Node".to_string()));
    assert_eq!(fact.confidence, Confidence::Medium);
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::ConstructorCall { package } if package == "LinkedList::Node"
        )
    }));
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::BlessLiteral { package } if package == "LinkedList::Node"
        )
    }));
    assert!(fact.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { base, key }
                if base == "$head" && key == "next"
        )
    }));
    Ok(())
}

#[test]
fn post_construction_link_assignment_preserves_blessed_container_shape() -> Result<(), String> {
    let code = "my $head = bless { parent => LinkedList::Node->new }, 'LinkedList::Node'; my $tail = LinkedList::Node->new; $head->{child} = $tail; $head->{child}->value;";
    let ast = parse_ast(code)?;
    let mut engine = TypeInferenceEngine::new();

    engine.infer(&ast).map_err(|err| format!("inference failed: {err:?}"))?;

    let tail = engine.get_fact_at("tail").ok_or_else(|| "missing tail fact".to_string())?;
    let head = engine.get_fact_at("head").ok_or_else(|| "missing head fact".to_string())?;
    assert_eq!(head.ty, PerlType::Object("LinkedList::Node".to_string()));
    assert_eq!(head.confidence, Confidence::Medium);
    assert!(head.dynamic_boundary.is_none());
    assert!(head.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::BlessLiteral { package } if package == "LinkedList::Node"
        )
    }));

    let ShapeFact::Object(head_shape) =
        head.shape.as_ref().ok_or_else(|| "missing head object shape".to_string())?
    else {
        return Err("head assignment replaced its object shape".to_string());
    };
    assert_eq!(head_shape.package, "LinkedList::Node");
    assert!(head_shape.fields.contains_key("parent"));
    let child =
        head_shape.fields.get("child").ok_or_else(|| "missing assigned child field".to_string())?;
    assert_eq!(child.ty, tail.ty);
    assert_eq!(child.confidence, tail.confidence);
    assert!(child.evidence.iter().any(|evidence| {
        matches!(evidence, TypeEvidence::Assignment { name } if name == "head")
    }));
    assert!(child.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashRefSlot { base, key }
                if base == "$head" && key == "child"
        )
    }));

    let receiver = method_receiver(&ast, "value")?;
    let receiver_fact = engine.infer_expr_fact(receiver);
    assert_eq!(receiver_fact.ty, PerlType::Object("LinkedList::Node".to_string()));
    assert_eq!(receiver_fact.confidence, tail.confidence);
    Ok(())
}

#[test]
fn dynamic_field_assignment_does_not_manufacture_a_static_object_field() -> Result<(), String> {
    let code = "my $field = 'child'; my $head = bless {}, 'LinkedList::Node'; my $tail = LinkedList::Node->new; $head->{$field} = $tail; $head->{child}->value;";
    let ast = parse_ast(code)?;
    let mut engine = TypeInferenceEngine::new();

    engine.infer(&ast).map_err(|err| format!("inference failed: {err:?}"))?;

    let head = engine.get_fact_at("head").ok_or_else(|| "missing head fact".to_string())?;
    let ShapeFact::Object(head_shape) =
        head.shape.as_ref().ok_or_else(|| "missing head object shape".to_string())?
    else {
        return Err("dynamic field assignment replaced the object shape".to_string());
    };
    assert!(!head_shape.fields.contains_key("child"));

    let receiver = method_receiver(&ast, "value")?;
    let receiver_fact = engine.infer_expr_fact(receiver);
    assert_eq!(receiver_fact.ty, PerlType::Any);
    Ok(())
}

#[test]
fn ordinary_hash_slot_assignment_remains_a_hash_shape() -> Result<(), String> {
    let code = "my %links; my $tail = LinkedList::Node->new; $links{child} = $tail;";
    let ast = parse_ast(code)?;
    let mut engine = TypeInferenceEngine::new();

    engine.infer(&ast).map_err(|err| format!("inference failed: {err:?}"))?;

    let links = engine.get_fact_at("links").ok_or_else(|| "missing links fact".to_string())?;
    let ShapeFact::Hash(shape) =
        links.shape.as_ref().ok_or_else(|| "missing links hash shape".to_string())?
    else {
        return Err("ordinary hash assignment stopped producing a hash shape".to_string());
    };
    let child =
        shape.slots.get("child").ok_or_else(|| "missing assigned hash child slot".to_string())?;
    assert_eq!(child.ty, PerlType::Object("LinkedList::Node".to_string()));
    assert!(child.evidence.iter().any(|evidence| {
        matches!(
            evidence,
            TypeEvidence::HashSlot { hash, key }
                if hash == "$links" && key == "child"
        )
    }));
    Ok(())
}
