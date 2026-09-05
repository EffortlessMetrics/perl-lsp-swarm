//! Production FieldId + field-aware traversal cutover (#8424).
//!
//! These tests fail realistic wrong implementations:
//! - regrouping children by registry field instead of preserving interleaved order
//! - mutating immutable vs mutable field sequences independently
//! - continuing after visitor failure
//! - treating If.condition as Optional / inventing a fourth cardinality
//! - treating `source_boundary` as production authority
//! - leaving FieldId::ALL membership out of the registry
//!
//! They do not cover S-expression redesign, schema identity, generated status,
//! or parser semantics.

use perl_ast::kind_schema::{
    FieldCardinality, KIND_SCHEMA_MODE, NODE_KIND_STRUCTURAL_REGISTRY, check_current_kind_schema,
    observe_kind_traversal, registered_child_fields, registry_field_id_set, structural_row,
};
use perl_ast::{FieldId, Node, NodeKind, SourceLocation, node_kind_fixtures};
use std::collections::BTreeSet;
use std::ops::ControlFlow;

fn loc() -> SourceLocation {
    SourceLocation { start: 0, end: 0 }
}

fn dummy() -> Node {
    Node::new(NodeKind::Undef, loc())
}

fn boxed() -> Box<Node> {
    Box::new(dummy())
}

fn immutable_fields(node: &Node) -> Vec<&'static str> {
    let mut names = Vec::new();
    let _ = node.try_for_each_child_with_field(|field, _| {
        names.push(field.map(FieldId::name).unwrap_or("unnamed"));
        ControlFlow::<()>::Continue(())
    });
    names
}

fn mutable_fields(node: &Node) -> Vec<&'static str> {
    let mut cloned = node.clone();
    let mut names = Vec::new();
    let _ = cloned.try_for_each_child_mut_with_field(|field, _| {
        names.push(field.map(FieldId::name).unwrap_or("unnamed"));
        ControlFlow::<()>::Continue(())
    });
    names
}

/// Group emissions by first-occurrence field — the wrong generated walker.
fn regroup_by_field(sequence: &[&'static str]) -> Vec<&'static str> {
    let mut order = Vec::new();
    let mut buckets: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    for name in sequence {
        if !order.contains(name) {
            order.push(*name);
        }
        buckets.entry(*name).or_default().push(*name);
    }
    order.into_iter().flat_map(|name| buckets.remove(name).unwrap_or_default()).collect()
}

#[test]
fn production_mode_is_traversal_cutover() {
    assert_eq!(KIND_SCHEMA_MODE, "production-traversal");
    let report = check_current_kind_schema();
    assert!(report.is_clean(), "{report}");
}

#[test]
fn field_id_all_membership_is_the_registry_set() {
    let registry: BTreeSet<&str> = registry_field_id_set().into_iter().map(FieldId::name).collect();
    let all: BTreeSet<&str> = FieldId::ALL.iter().copied().map(FieldId::name).collect();
    assert_eq!(
        all, registry,
        "FieldId::ALL membership must equal unique registry fields; ALL order is the \
         compatibility inventory"
    );
    for field in FieldId::ALL {
        assert_eq!(FieldId::from_name(field.name()), Some(*field));
    }
}

#[test]
fn hash_literal_keeps_interleaved_key_value_order() {
    let node = Node::new(
        NodeKind::HashLiteral { pairs: vec![(dummy(), dummy()), (dummy(), dummy())] },
        loc(),
    );
    let fields = immutable_fields(&node);
    assert_eq!(fields, vec!["key", "value", "key", "value"]);
    assert_eq!(mutable_fields(&node), fields);
    let regrouped = regroup_by_field(&fields);
    assert_eq!(regrouped, vec!["key", "key", "value", "value"]);
    assert_ne!(
        fields, regrouped,
        "a registry-grouped walk would emit all keys then all values and change observable order"
    );
}

#[test]
fn if_elsif_reuses_required_condition_and_repeated_body_without_regrouping()
-> Result<(), Box<dyn std::error::Error>> {
    let node = Node::new(
        NodeKind::If {
            condition: boxed(),
            then_branch: boxed(),
            elsif_branches: vec![(boxed(), boxed()), (boxed(), boxed())],
            else_branch: Some(boxed()),
            keyword: None,
        },
        loc(),
    );
    let fields = immutable_fields(&node);
    assert_eq!(
        fields,
        vec!["condition", "then_branch", "condition", "body", "condition", "body", "else_branch",]
    );
    assert_eq!(mutable_fields(&node), fields);
    let regrouped = regroup_by_field(&fields);
    assert_eq!(
        regrouped,
        vec!["condition", "condition", "condition", "then_branch", "body", "body", "else_branch",]
    );
    assert_ne!(fields, regrouped, "If elsif pairs must stay interleaved");

    let row = structural_row("If").ok_or("If row")?;
    let condition = row.children.first().ok_or("If.condition")?;
    let body = row.children.get(2).ok_or("If.body")?;
    assert_eq!(condition.field, FieldId::CONDITION);
    assert_eq!(condition.cardinality, FieldCardinality::Required);
    assert_eq!(body.field, FieldId::BODY);
    assert_eq!(body.cardinality, FieldCardinality::Repeated);
    assert!(
        !row.children.iter().any(|spec| spec.cardinality != FieldCardinality::Required
            && spec.cardinality != FieldCardinality::Optional
            && spec.cardinality != FieldCardinality::Repeated),
        "If must not invent a fourth cardinality"
    );
    Ok(())
}

#[test]
fn optional_absent_fields_emit_nothing_and_empty_repeated_fields_emit_nothing() {
    let declaration = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".into(),
            variable: boxed(),
            attributes: vec![],
            initializer: None,
        },
        loc(),
    );
    assert_eq!(immutable_fields(&declaration), vec!["variable"]);
    assert_eq!(mutable_fields(&declaration), vec!["variable"]);

    let program = Node::new(NodeKind::Program { statements: vec![] }, loc());
    assert!(immutable_fields(&program).is_empty());
    assert!(mutable_fields(&program).is_empty());
}

#[test]
fn immutable_and_mutable_field_sequences_match_every_fixture() {
    for fixture in node_kind_fixtures() {
        let observation = observe_kind_traversal(&fixture.sample);
        assert_eq!(
            observation.immutable_field_sequence, observation.mutable_field_sequence,
            "{}: immutable and mutable field sequences diverged",
            observation.kind_name
        );
        assert_eq!(
            observation.immutable_visit_ids, observation.mutable_visit_ids,
            "{}: immutable and mutable visit order diverged",
            observation.kind_name
        );
    }
}

#[test]
fn immutable_short_circuit_does_not_pull_later_children() {
    let program =
        Node::new(NodeKind::Program { statements: vec![dummy(), dummy(), dummy()] }, loc());
    let mut pulls = 0usize;
    let mut visits = 0usize;
    let result = program.try_for_each_child_with_field_observed(
        |_, _| pulls = pulls.saturating_add(1),
        |_, _| {
            visits = visits.saturating_add(1);
            ControlFlow::Break("stop")
        },
    );
    assert_eq!(result, ControlFlow::Break("stop"));
    assert_eq!(pulls, 1);
    assert_eq!(visits, 1);
}

#[test]
fn mutable_short_circuit_does_not_pull_later_children() {
    let mut program =
        Node::new(NodeKind::Program { statements: vec![dummy(), dummy(), dummy()] }, loc());
    let mut pulls = 0usize;
    let mut visits = 0usize;
    let result = program.try_for_each_child_mut_with_field_observed(
        |_, _| pulls = pulls.saturating_add(1),
        |_, _| {
            visits = visits.saturating_add(1);
            ControlFlow::Break("stop")
        },
    );
    assert_eq!(result, ControlFlow::Break("stop"));
    assert_eq!(pulls, 1);
    assert_eq!(visits, 1);
}

#[test]
fn dropping_or_duplicating_a_child_is_visible_against_the_registry() {
    let assignment =
        Node::new(NodeKind::Assignment { lhs: boxed(), rhs: boxed(), op: "+".into() }, loc());
    let fields = immutable_fields(&assignment);
    let registered: Vec<&str> =
        registered_child_fields("Assignment").iter().map(|spec| spec.field.name()).collect();
    assert_eq!(fields, registered);
    let mut dropped = fields.clone();
    dropped.pop();
    assert_ne!(dropped, registered, "dropping rhs must disagree with the registry");
    let mut duplicated = fields.clone();
    duplicated.push("rhs");
    assert_ne!(duplicated, registered, "duplicating rhs must disagree with the registry");
    let mut swapped = fields.clone();
    swapped.swap(0, 1);
    assert_ne!(swapped, registered, "swapping lhs/rhs must disagree with the registry");
}

#[test]
fn for_each_child_mut_forwards_to_the_field_aware_walker() {
    let mut node = Node::new(
        NodeKind::If {
            condition: boxed(),
            then_branch: boxed(),
            elsif_branches: vec![(boxed(), boxed())],
            else_branch: Some(boxed()),
            keyword: None,
        },
        loc(),
    );
    let mut wrapper_count = 0usize;
    node.for_each_child_mut(|_| wrapper_count += 1);
    let mut field_count = 0usize;
    let _ = node.try_for_each_child_mut_with_field(|_, _| {
        field_count += 1;
        ControlFlow::<()>::Continue(())
    });
    assert_eq!(wrapper_count, field_count);
    assert_eq!(wrapper_count, 5);
}

#[test]
fn source_boundary_tags_are_recorded_but_not_production_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let heredoc = structural_row("Heredoc").ok_or("Heredoc row")?;
    let data = structural_row("DataSection").ok_or("DataSection row")?;
    let format = structural_row("Format").ok_or("Format row")?;
    let program = structural_row("Program").ok_or("Program row")?;
    assert!(heredoc.source_boundary);
    assert!(data.source_boundary);
    assert!(format.source_boundary);
    assert!(!program.source_boundary);
    assert!(heredoc.is_leaf());
    assert!(registered_child_fields("Heredoc").is_empty());
    // Traversal consults children/body, not the source_boundary flag.
    let node = Node::new(
        NodeKind::Heredoc {
            delimiter: "END".into(),
            content: "body".into(),
            interpolated: false,
            indented: false,
            command: false,
            body_span: None,
        },
        loc(),
    );
    assert!(immutable_fields(&node).is_empty());
    assert!(mutable_fields(&node).is_empty());
    Ok(())
}

#[test]
fn recovery_nodes_follow_registered_structure() {
    let error = Node::new(
        NodeKind::Error {
            message: "bad".into(),
            expected: vec![],
            found: None,
            partial: Some(boxed()),
        },
        loc(),
    );
    assert_eq!(immutable_fields(&error), vec!["partial"]);
    assert_eq!(mutable_fields(&error), vec!["partial"]);
    let missing = Node::new(NodeKind::MissingExpression, loc());
    assert!(immutable_fields(&missing).is_empty());
}

#[test]
fn registry_covers_every_kind_exactly_once() {
    let names: Vec<_> = NODE_KIND_STRUCTURAL_REGISTRY.iter().map(|row| row.kind_name).collect();
    assert_eq!(names, NodeKind::ALL_KIND_NAMES);
}
