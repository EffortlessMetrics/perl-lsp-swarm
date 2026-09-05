//! Discriminating proof for AST structural schema identity and freshness (#8429).
//!
//! These tests exist to fail realistic wrong implementations:
//! - a stale checked report with the previous variant count still going green
//! - a field-only structural edit that does not change identity
//! - semantic prose, host paths, timestamps, or consumer policy leaking into identity
//!
//! They do not cover #5238 consumer migration, #7045 equality, #8044 native output,
//! serde of internal AST, or parser behavior.

use perl_ast::kind_schema::{
    ChildFieldSpec, FieldCardinality, GrammarNameSpec, KindBody, KindStructuralRow,
    NODE_KIND_STRUCTURAL_REGISTRY, SchemaChange, SchemaCompatibility, SchemaIdentityError,
    StatusFreshnessError, canonical_structural_subject, check_status_freshness,
    current_ast_structural_schema_identity, current_nodekind_inventory, diff_structural_registries,
    fingerprint_registry, inventory_from_registry, parse_schema_identity,
    render_checked_status_report,
};
use perl_ast::{FieldId, NodeKind};

fn row_index(kind_name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    NODE_KIND_STRUCTURAL_REGISTRY
        .iter()
        .position(|row| row.kind_name == kind_name)
        .ok_or_else(|| format!("{kind_name} missing from the structural registry").into())
}

const ASSIGNMENT_OPTIONAL_RHS: &[ChildFieldSpec] = &[
    ChildFieldSpec { field: FieldId::LHS, cardinality: FieldCardinality::Required },
    ChildFieldSpec { field: FieldId::RHS, cardinality: FieldCardinality::Optional },
];
const ASSIGNMENT_LHS_ONLY: &[ChildFieldSpec] =
    &[ChildFieldSpec { field: FieldId::LHS, cardinality: FieldCardinality::Required }];
const ASSIGNMENT_REORDERED: &[ChildFieldSpec] = &[
    ChildFieldSpec { field: FieldId::RHS, cardinality: FieldCardinality::Required },
    ChildFieldSpec { field: FieldId::LHS, cardinality: FieldCardinality::Required },
];
const ASSIGNMENT_RUNTIME_EXTRA: &[&str] = &["op", "extra"];

fn assignment_optional_rhs(
    rows: &mut [KindStructuralRow<'static>],
) -> Result<(), Box<dyn std::error::Error>> {
    let idx = row_index("Assignment")?;
    rows[idx].children = ASSIGNMENT_OPTIONAL_RHS;
    Ok(())
}

#[test]
fn identical_structural_inputs_produce_identical_fingerprints() {
    let first = fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY);
    let second = fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY);
    let cloned = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let from_clone = fingerprint_registry(&cloned);
    assert_eq!(first, second);
    assert_eq!(first, from_clone);
    assert_eq!(first, current_ast_structural_schema_identity());
    assert_eq!(first.wire(), second.wire());
}

#[test]
fn identity_is_independent_of_host_path_timestamp_and_map_insertion() {
    let subject = canonical_structural_subject(NODE_KIND_STRUCTURAL_REGISTRY);
    assert!(
        !subject.contains("/tmp/cursor/host-root"),
        "canonical subject must not include host paths: {subject}"
    );
    assert!(
        !subject.contains("2026-08-26T21:25:00Z"),
        "canonical subject must not include wall-clock timestamps"
    );
    assert!(
        !subject.contains("KindStructuralRow"),
        "canonical subject must not be Debug formatting of internal types"
    );
    assert!(
        !subject.contains("production-traversal"),
        "canonical subject must not include KIND_SCHEMA_MODE consumer policy"
    );
    assert!(
        !subject.contains("compatibility"),
        "canonical subject must not include consumer compatibility policy"
    );
}

#[test]
fn field_only_change_changes_identity_and_is_observable() -> Result<(), Box<dyn std::error::Error>>
{
    let before = fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY);
    let mut rows = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    assignment_optional_rhs(&mut rows)?;
    let after = fingerprint_registry(&rows);
    assert_ne!(before, after, "field cardinality is behavior-bearing structure");

    let diff = diff_structural_registries(NODE_KIND_STRUCTURAL_REGISTRY, &rows);
    assert_ne!(diff.from, diff.to);
    assert!(
        diff.changes.iter().any(|change| matches!(
            change,
            SchemaChange::CardinalityChanged { kind_name, field, from, to }
                if kind_name == "Assignment" && field == "rhs" && from == "required" && to == "optional"
        )),
        "field-only change must be visible as a cardinality delta, got {diff:?}"
    );
    assert!(
        !diff.changes.iter().any(|change| matches!(
            change,
            SchemaChange::AddedVariant { .. } | SchemaChange::RemovedVariant { .. }
        )),
        "field-only change must not look like a variant add/remove: {diff:?}"
    );
    Ok(())
}

#[test]
fn adding_reordering_or_removing_a_variant_or_field_changes_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let baseline = fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY);

    let mut removed = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    removed.remove(row_index("Program")?);
    assert_ne!(baseline, fingerprint_registry(&removed));
    let removed_diff = diff_structural_registries(NODE_KIND_STRUCTURAL_REGISTRY, &removed);
    assert!(
        removed_diff.changes.iter().any(|change| matches!(
            change,
            SchemaChange::RemovedVariant { kind_name } if kind_name == "Program"
        )),
        "{removed_diff:?}"
    );
    assert!(
        !removed_diff
            .changes
            .iter()
            .any(|change| matches!(change, SchemaChange::ReorderedVariants { .. })),
        "add/remove must not report ReorderedVariants: {removed_diff:?}"
    );

    let mut reordered = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    reordered.swap(0, 1);
    assert_ne!(
        baseline,
        fingerprint_registry(&reordered),
        "declaration order is a behavior-bearing structural fact"
    );
    let reorder_diff = diff_structural_registries(NODE_KIND_STRUCTURAL_REGISTRY, &reordered);
    assert!(
        reorder_diff
            .changes
            .iter()
            .any(|change| matches!(change, SchemaChange::ReorderedVariants { .. })),
        "same-set declaration-order swap must be a reorder: {reorder_diff:?}"
    );
    assert!(
        !reorder_diff.changes.iter().any(|change| matches!(
            change,
            SchemaChange::AddedVariant { .. } | SchemaChange::RemovedVariant { .. }
        )),
        "{reorder_diff:?}"
    );

    let mut added = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    added.push(KindStructuralRow {
        kind_name: "SyntheticExtra",
        children: &[],
        body: KindBody::Leaf,
        recovery: false,
        source_boundary: false,
        grammar: GrammarNameSpec::Static("synthetic_extra"),
        compatibility: SchemaCompatibility::Current,
    });
    assert_ne!(baseline, fingerprint_registry(&added));
    let added_diff = diff_structural_registries(NODE_KIND_STRUCTURAL_REGISTRY, &added);
    assert!(
        added_diff.changes.iter().any(|change| matches!(
            change,
            SchemaChange::AddedVariant { kind_name } if kind_name == "SyntheticExtra"
        )),
        "{added_diff:?}"
    );
    assert!(
        !added_diff
            .changes
            .iter()
            .any(|change| matches!(change, SchemaChange::ReorderedVariants { .. })),
        "add/remove must not report ReorderedVariants: {added_diff:?}"
    );

    let mut dropped_field = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let idx = row_index("Assignment")?;
    dropped_field[idx].children = ASSIGNMENT_LHS_ONLY;
    assert_ne!(baseline, fingerprint_registry(&dropped_field));

    let mut reordered_fields = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    reordered_fields[idx].children = ASSIGNMENT_REORDERED;
    assert_ne!(baseline, fingerprint_registry(&reordered_fields));
    Ok(())
}

#[test]
fn semantic_prose_and_consumer_policy_do_not_change_structural_identity() {
    let inventory = current_nodekind_inventory();
    let first =
        render_checked_status_report(&inventory, "corpus coverage is a consumer policy note");
    let second = render_checked_status_report(
        &inventory,
        "different prose, /tmp/host, 2026-08-26T00:00:00Z, ignore-this-policy",
    );
    assert_eq!(inventory.identity, current_ast_structural_schema_identity());
    assert_ne!(first, second, "prose must still be visible in the generated document");
    assert!(check_status_freshness(&first, &inventory).is_ok(), "{first}");
    assert!(
        check_status_freshness(&second, &inventory).is_ok(),
        "semantic prose must not fail freshness: {second}"
    );

    let mut policy = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    // Compatibility is retained on the row as consumer policy, not a fingerprint input.
    for row in &mut policy {
        row.compatibility = SchemaCompatibility::Current;
    }
    assert_eq!(fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY), fingerprint_registry(&policy));
    let diff = diff_structural_registries(NODE_KIND_STRUCTURAL_REGISTRY, &policy);
    assert!(diff.changes.is_empty(), "compatibility-only rows must not produce a structural delta");
}

#[test]
fn stale_checked_report_with_previous_count_fails() {
    let inventory = current_nodekind_inventory();
    let current = render_checked_status_report(&inventory, "current");
    assert!(check_status_freshness(&current, &inventory).is_ok());

    let stale_count = current.replace(
        &format!("variant_count={}", inventory.variant_count),
        &format!("variant_count={}", inventory.variant_count.saturating_sub(1)),
    );
    assert_ne!(stale_count, current);
    let stale = check_status_freshness(&stale_count, &inventory);
    assert!(
        matches!(stale, Err(StatusFreshnessError::StaleCheckedOutput { .. })),
        "stale count must fail closed, got {stale:?}"
    );
}

#[test]
fn stale_checked_report_with_same_counts_but_structural_drift_fails() {
    let inventory = current_nodekind_inventory();
    let current = render_checked_status_report(&inventory, "current");
    assert!(check_status_freshness(&current, &inventory).is_ok());

    let stale_row = current.replacen("statements:repeated", "statements:optional", 1);
    assert_ne!(stale_row, current);
    assert!(
        stale_row.contains(&format!("variant_count={}", inventory.variant_count)),
        "counts must remain current so this case is not the previous-count control"
    );
    assert!(
        stale_row.contains(&format!("identity={}", inventory.identity.wire())),
        "identity wire must remain current so summaries are the discriminated surface"
    );
    let stale = check_status_freshness(&stale_row, &inventory);
    assert!(
        matches!(stale, Err(StatusFreshnessError::StaleCheckedOutput { .. })),
        "same-count field-cardinality drift must fail freshness, got {stale:?}"
    );
}

#[test]
fn compiled_count_and_status_count_cannot_disagree() {
    let inventory = current_nodekind_inventory();
    assert_eq!(inventory.variant_count, NODE_KIND_STRUCTURAL_REGISTRY.len());
    assert_eq!(inventory.variant_count, NodeKind::ALL_KIND_NAMES.len());
    assert_eq!(
        inventory.recovery_count,
        NODE_KIND_STRUCTURAL_REGISTRY.iter().filter(|row| row.recovery).count()
    );
    assert_eq!(inventory.field_count, perl_ast::kind_schema::registry_field_id_set().len());

    let report = render_checked_status_report(&inventory, "");
    assert!(report.contains(&format!("variant_count={}", inventory.variant_count)));
    assert!(report.contains(&format!("field_count={}", inventory.field_count)));
    assert!(check_status_freshness(&report, &inventory).is_ok());

    let from_registry = inventory_from_registry(NODE_KIND_STRUCTURAL_REGISTRY);
    assert_eq!(from_registry.variant_count, inventory.variant_count);
    assert_eq!(from_registry.identity, inventory.identity);
}

#[test]
fn historical_report_cannot_satisfy_current_freshness() {
    let inventory = current_nodekind_inventory();
    let historical =
        "| **Node-kind coverage** | 65/69 (94.2%) | historical hand-entered denominator |\n";
    let result = check_status_freshness(historical, &inventory);
    assert!(
        matches!(result, Err(StatusFreshnessError::HistoricalUnversioned)),
        "historical unversioned status must not satisfy current freshness, got {result:?}"
    );
}

#[test]
fn unknown_schema_versions_and_digest_algorithms_fail_closed() {
    let unknown_version = parse_schema_identity(
        "ast-schema.v2-sha256-v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    assert!(
        matches!(unknown_version, Err(SchemaIdentityError::UnknownVersion { version: 2 })),
        "expected unknown version, got {unknown_version:?}"
    );
    let unknown_algorithm = parse_schema_identity(
        "ast-schema.v1-md5:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    assert!(
        matches!(
            unknown_algorithm,
            Err(SchemaIdentityError::UnknownAlgorithm { ref algorithm }) if algorithm == "md5"
        ),
        "expected unknown algorithm, got {unknown_algorithm:?}"
    );
    let uppercase =
        parse_schema_identity(&current_ast_structural_schema_identity().wire().to_uppercase());
    assert!(
        matches!(
            uppercase,
            Err(SchemaIdentityError::MalformedDigest { .. }
                | SchemaIdentityError::MalformedWire { .. })
        ),
        "uppercase wire must fail closed, got {uppercase:?}"
    );
}

#[test]
fn recovery_and_grammar_inputs_are_fingerprint_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let baseline = fingerprint_registry(NODE_KIND_STRUCTURAL_REGISTRY);

    let mut recovery = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let program = row_index("Program")?;
    recovery[program].recovery = true;
    assert_ne!(baseline, fingerprint_registry(&recovery));

    let mut boundary = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    boundary[program].source_boundary = true;
    assert_ne!(baseline, fingerprint_registry(&boundary));

    let mut grammar = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    grammar[program].grammar = GrammarNameSpec::Static("not_source_file");
    assert_ne!(baseline, fingerprint_registry(&grammar));

    let mut inputs = NODE_KIND_STRUCTURAL_REGISTRY.to_vec();
    let assignment = row_index("Assignment")?;
    inputs[assignment].grammar =
        GrammarNameSpec::RuntimeDerived { inputs: ASSIGNMENT_RUNTIME_EXTRA };
    assert_ne!(baseline, fingerprint_registry(&inputs));
    Ok(())
}

#[test]
fn current_inventory_rows_follow_declaration_order() {
    let inventory = current_nodekind_inventory();
    let names: Vec<_> = inventory.variants.iter().map(|row| row.kind_name).collect();
    assert_eq!(names, NodeKind::ALL_KIND_NAMES);
    assert_eq!(names.first().copied(), Some("Program"));
    assert_eq!(names.last().copied(), Some("UnknownRest"));
}
