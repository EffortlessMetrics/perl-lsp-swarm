//! Shadow NodeKind structural registry: production parity and negative controls.
//!
//! These tests prove the checker fails for realistic wrong registries. Production
//! FieldId membership and field-aware traversal are owned by the registry after
//! #8424. Schema identity and freshness-gated status live in `kind_schema_identity`.

use perl_ast::kind_schema::{
    ChildFieldSpec, FieldCardinality, GrammarInputWitness, GrammarNameSpec, KindBody,
    KindSchemaEvidence, KindSchemaMismatch, KindStructuralRow, NODE_KIND_STRUCTURAL_REGISTRY,
    SchemaCompatibility, cardinality_forms, check_current_kind_schema, check_kind_schema,
    current_kind_schema_evidence, grammar_input_witnesses, representative_nodes,
    serialize_kind_schema,
};
use perl_ast::{FieldId, Node, NodeKind, SourceLocation};

fn report_for(registry: &[KindStructuralRow<'_>]) -> perl_ast::kind_schema::KindSchemaReport {
    let representatives = representative_nodes();
    let cardinality = cardinality_forms();
    let grammar_witnesses = grammar_input_witnesses();
    check_kind_schema(&current_kind_schema_evidence(
        registry,
        &representatives,
        &cardinality,
        &grammar_witnesses,
    ))
}

fn row_index(kind_name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    NODE_KIND_STRUCTURAL_REGISTRY
        .iter()
        .position(|row| row.kind_name == kind_name)
        .ok_or_else(|| format!("{kind_name} missing from the structural registry").into())
}

#[test]
fn production_shadow_registry_is_in_parity() {
    let report = check_current_kind_schema();
    assert!(report.is_clean(), "{report}");
}

#[test]
fn registry_serialization_is_deterministic() {
    let first = serialize_kind_schema(NODE_KIND_STRUCTURAL_REGISTRY);
    let second = serialize_kind_schema(NODE_KIND_STRUCTURAL_REGISTRY);
    assert_eq!(first, second, "schema serialization must be byte-stable");
    assert!(first.starts_with("# perl-ast NodeKind structural schema v1\n"));
    assert!(first.contains("mode=production-traversal"));
    let names: Vec<_> = NODE_KIND_STRUCTURAL_REGISTRY.iter().map(|row| row.kind_name).collect();
    assert_eq!(names, NodeKind::ALL_KIND_NAMES);
}

#[test]
fn adding_a_variant_without_a_row_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    rows.remove(row_index("Program")?);
    let report = report_for(&rows);
    assert!(
        report
            .mismatches
            .iter()
            .any(|mismatch| matches!(mismatch, KindSchemaMismatch::MissingRow { kind_name } if kind_name == "Program")),
        "{report}"
    );
    Ok(())
}

#[test]
fn reordering_registry_rows_fails() {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    rows.swap(0, 1);
    let report = report_for(&rows);
    assert!(
        report
            .mismatches
            .iter()
            .any(|mismatch| matches!(mismatch, KindSchemaMismatch::OrderDrift { .. })),
        "{report}"
    );
}

#[test]
fn omitting_a_child_field_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Assignment")?;
    let children: Vec<_> = rows[idx].children.iter().copied().take(1).collect();
    rows[idx].children = &children;
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::ChildFieldSet { kind_name, .. } if kind_name == "Assignment"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn renaming_a_child_field_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Assignment")?;
    let children = [
        ChildFieldSpec { field: FieldId::TARGET, cardinality: FieldCardinality::Required },
        ChildFieldSpec { field: FieldId::RHS, cardinality: FieldCardinality::Required },
    ];
    rows[idx].children = &children;
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::ChildFieldSet { kind_name, .. } if kind_name == "Assignment"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn reordering_child_fields_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Assignment")?;
    let mut children = rows[idx].children.to_vec();
    children.swap(0, 1);
    rows[idx].children = &children;
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::ChildFieldOrder { kind_name, .. } if kind_name == "Assignment"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn marking_a_child_bearing_variant_as_a_leaf_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Program")?;
    rows[idx].body = KindBody::Leaf;
    rows[idx].children = &[];
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::ChildBearingMarkedLeaf { kind_name } if kind_name == "Program"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn optional_field_marked_required_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("VariableDeclaration")?;
    let children = [
        ChildFieldSpec { field: FieldId::VARIABLE, cardinality: FieldCardinality::Required },
        ChildFieldSpec { field: FieldId::INITIALIZER, cardinality: FieldCardinality::Required },
    ];
    rows[idx].children = &children;
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::Cardinality { kind_name, field, .. }
                if kind_name == "VariableDeclaration" && field == "initializer"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn repeated_field_marked_required_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Program")?;
    let children =
        [ChildFieldSpec { field: FieldId::STATEMENTS, cardinality: FieldCardinality::Required }];
    rows[idx].children = &children;
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::Cardinality { kind_name, field, .. }
                if kind_name == "Program" && field == "statements"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn duplicate_child_field_names_fail() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Assignment")?;
    let mut children = rows[idx].children.to_vec();
    children.push(children[0]);
    rows[idx].children = &children;
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::DuplicateChildField { kind_name, field }
                if kind_name == "Assignment" && field == "lhs"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn duplicate_field_id_inventory_fails() {
    let representatives = representative_nodes();
    let cardinality = cardinality_forms();
    let grammar_witnesses = grammar_input_witnesses();
    let field_ids = [FieldId::ARGS, FieldId::ARGS];
    let evidence = KindSchemaEvidence {
        registry: NODE_KIND_STRUCTURAL_REGISTRY,
        kind_names: NodeKind::ALL_KIND_NAMES,
        recovery_names: NodeKind::RECOVERY_KIND_NAMES,
        field_ids: &field_ids,
        representatives: &representatives,
        cardinality_forms: &cardinality,
        grammar_witnesses: &grammar_witnesses,
    };
    let report = check_kind_schema(&evidence);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::DuplicateFieldIdInventory { field } if field == "args"
        )),
        "{report}"
    );
}

#[test]
fn omitting_a_runtime_grammar_input_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("String")?;
    let inputs: &[&str] = &[];
    rows[idx].grammar = GrammarNameSpec::RuntimeDerived { inputs };
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::MissingGrammarInput { kind_name, input }
                if kind_name == "String" && input == "interpolated"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn omitting_a_representative_fails() {
    let representatives: Vec<Node> = representative_nodes()
        .into_iter()
        .filter(|node| node.kind.kind_name() != "Diamond")
        .collect();
    let cardinality: Vec<Node> =
        cardinality_forms().into_iter().filter(|node| node.kind.kind_name() != "Diamond").collect();
    let grammar_witnesses: Vec<GrammarInputWitness> = grammar_input_witnesses()
        .into_iter()
        .filter(|witness| witness.kind_name != "Diamond")
        .collect();
    let evidence = current_kind_schema_evidence(
        NODE_KIND_STRUCTURAL_REGISTRY,
        &representatives,
        &cardinality,
        &grammar_witnesses,
    );
    let report = check_kind_schema(&evidence);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::MissingRepresentative { kind_name } if kind_name == "Diamond"
        )),
        "{report}"
    );
}

#[test]
fn if_elsif_reuses_condition_without_becoming_optional() -> Result<(), Box<dyn std::error::Error>> {
    let report = check_current_kind_schema();
    assert!(report.is_clean(), "{report}");
    let row = NODE_KIND_STRUCTURAL_REGISTRY[row_index("If")?];
    assert_eq!(row.children[0].field, FieldId::CONDITION);
    assert_eq!(row.children[0].cardinality, FieldCardinality::Required);
    assert_eq!(row.children[2].field, FieldId::BODY);
    assert_eq!(row.children[2].cardinality, FieldCardinality::Repeated);
    Ok(())
}

#[test]
fn function_call_args_are_both_repeated_children_and_grammar_inputs()
-> Result<(), Box<dyn std::error::Error>> {
    let row = NODE_KIND_STRUCTURAL_REGISTRY[row_index("FunctionCall")?];
    assert_eq!(row.children[0].field, FieldId::ARGS);
    assert_eq!(row.children[0].cardinality, FieldCardinality::Repeated);
    assert!(
        matches!(
            row.grammar,
            GrammarNameSpec::RuntimeDerived { inputs }
                if inputs.contains(&"name") && inputs.contains(&"args")
        ),
        "FunctionCall must declare runtime grammar inputs name and args, got {:?}",
        row.grammar
    );
    Ok(())
}

#[test]
fn vacuous_grammar_witness_fails() {
    let loc = SourceLocation::new(0, 0);
    let witnesses = [GrammarInputWitness {
        kind_name: "String",
        input: "interpolated",
        left: Node::new(NodeKind::String { value: "a".to_string(), interpolated: false }, loc),
        right: Node::new(NodeKind::String { value: "b".to_string(), interpolated: false }, loc),
    }];
    let representatives = representative_nodes();
    let cardinality = cardinality_forms();
    let evidence = KindSchemaEvidence {
        registry: NODE_KIND_STRUCTURAL_REGISTRY,
        kind_names: NodeKind::ALL_KIND_NAMES,
        recovery_names: NodeKind::RECOVERY_KIND_NAMES,
        field_ids: FieldId::ALL,
        representatives: &representatives,
        cardinality_forms: &cardinality,
        grammar_witnesses: &witnesses,
    };
    let report = check_kind_schema(&evidence);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::VacuousGrammarWitness { kind_name, input }
                if kind_name == "String" && input == "interpolated"
        )),
        "{report}"
    );
}

#[test]
fn unused_field_id_inventory_fails() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let owned: Vec<Vec<_>> = rows
        .iter()
        .map(|row| {
            row.children.iter().copied().filter(|child| child.field != FieldId::ARGS).collect()
        })
        .collect();
    for (row, children) in rows.iter_mut().zip(owned.iter()) {
        row.children = children;
    }
    let report = report_for(&rows);
    assert!(
        report.mismatches.iter().any(|mismatch| matches!(
            mismatch,
            KindSchemaMismatch::UnusedFieldIdInventory { field } if field == "args"
        )),
        "{report}"
    );
    Ok(())
}

#[test]
fn schema_compatibility_is_current_for_every_row() {
    for row in NODE_KIND_STRUCTURAL_REGISTRY {
        assert_eq!(row.compatibility, SchemaCompatibility::Current);
    }
}
