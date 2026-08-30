//! Gate for the field-level source-geometry registry (#7015).
//!
//! The registry exists so a coordinate-mapping consumer (#13234) can enumerate
//! every payload field carrying byte offsets of its own. Two different things
//! have to be true for that to be trustworthy, and they are proven separately:
//!
//! 1. **Nothing is missing today.** The fully populated fixture bank is
//!    observed through `observe_geometry_fields` and reconciled against the
//!    registry, so a stale row or an unregistered field is red here.
//! 2. **Nothing can go missing later.** The reconciliation function is fed
//!    deliberately mutated inputs, proving it actually discriminates rather
//!    than returning `Ok` for everything. The complementary half — that a new
//!    geometry field cannot be added without being classified — is structural:
//!    `observe_geometry_fields` destructures every field of every variant with
//!    no `..` arm, so the addition fails to compile until an author handles it.

use perl_ast::ast::{Token, TokenKind};
use perl_ast::{
    AST_GEOMETRY_SCHEMA_VERSION, AST_NODE_GEOMETRY_FIELDS, AstGeometryDisposition,
    AstGeometryDrift, AstGeometryField, AstGeometryMapping, AstGeometryShape,
    AstNodeClassification, NodeKind, ObservedGeometryField, SourceLocation, ast_node_policy,
    geometry_disposition_for_classification, geometry_fields_for, geometry_shapes_in_use,
    node_kind_fixtures, observe_geometry_fields, reconcile_geometry_rows, reconcile_node_geometry,
};
use std::collections::BTreeSet;

/// Registry rows may only name kinds that exist, in canonical order.
#[test]
fn every_geometry_row_names_a_live_nodekind() {
    let canonical = NodeKind::ALL_KIND_NAMES.iter().copied().collect::<BTreeSet<_>>();
    for row in AST_NODE_GEOMETRY_FIELDS {
        assert!(
            canonical.contains(row.kind_name),
            "geometry row {}.{} names a NodeKind that does not exist",
            row.kind_name,
            row.field
        );
    }

    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for row in AST_NODE_GEOMETRY_FIELDS {
        assert!(
            seen.insert((row.kind_name, row.field)),
            "geometry row {}.{} is registered more than once",
            row.kind_name,
            row.field
        );
    }

    let declaration_order: Vec<usize> = AST_NODE_GEOMETRY_FIELDS
        .iter()
        .filter_map(|row| NodeKind::ALL_KIND_NAMES.iter().position(|name| *name == row.kind_name))
        .collect();
    let mut sorted = declaration_order.clone();
    sorted.sort_unstable();
    assert_eq!(
        declaration_order, sorted,
        "geometry rows must follow NodeKind::ALL_KIND_NAMES declaration order so the table can be \
         read against the enum"
    );

    assert_eq!(AST_GEOMETRY_SCHEMA_VERSION, 1, "geometry schema version drifted");
}

/// The positive gate: every variant's real geometry matches its rows.
#[test]
fn every_fixture_reconciles_with_its_registered_geometry() -> Result<(), Box<dyn std::error::Error>>
{
    for fixture in node_kind_fixtures() {
        let kind_name = fixture.sample.kind.kind_name();
        reconcile_node_geometry(&fixture.sample)
            .map_err(|drift| format!("{kind_name}: {drift}"))?;
    }
    Ok(())
}

/// A fully populated sample must actually exercise every registered field.
///
/// Without this, a registry row could describe a field that the fixture leaves
/// absent, and the reconciliation above would still pass while proving nothing
/// about that field.
#[test]
fn the_populated_fixture_observes_every_registered_field() -> Result<(), Box<dyn std::error::Error>>
{
    for fixture in node_kind_fixtures() {
        let kind_name = fixture.sample.kind.kind_name();
        let observed = observe_geometry_fields(&fixture.sample.kind);

        for row in geometry_fields_for(kind_name) {
            let entry = observed
                .iter()
                .find(|entry| entry.field == row.field)
                .ok_or_else(|| format!("{kind_name}.{}: registered but not observed", row.field))?;

            match row.shape {
                AstGeometryShape::Direct | AstGeometryShape::Optional | AstGeometryShape::Token => {
                    assert_eq!(
                        entry.occurrences,
                        1,
                        "{kind_name}.{}: the fully populated fixture must carry exactly one span \
                         for a {} field, observed {}",
                        row.field,
                        row.shape.token(),
                        entry.occurrences
                    );
                }
                AstGeometryShape::Nested | AstGeometryShape::Repeated => {
                    assert!(
                        entry.occurrences >= 2,
                        "{kind_name}.{}: a {} field must observe more than one span on the fully \
                         populated fixture so repetition is provable, observed {}",
                        row.field,
                        row.shape.token(),
                        entry.occurrences
                    );
                }
            }
        }
    }
    Ok(())
}

/// A field cannot claim a friendlier source relationship than its owning node.
#[test]
fn dispositions_are_derived_from_the_owning_variant_classification()
-> Result<(), Box<dyn std::error::Error>> {
    for row in AST_NODE_GEOMETRY_FIELDS {
        let policy = ast_node_policy(row.kind_name)
            .ok_or_else(|| format!("{} has geometry rows but no policy row", row.kind_name))?;
        let expected = geometry_disposition_for_classification(policy.classification);
        assert_eq!(
            row.disposition,
            expected,
            "{}.{}: classification {:?} requires disposition {} but the row registers {}",
            row.kind_name,
            row.field,
            policy.classification,
            expected.token(),
            row.disposition.token()
        );
    }
    Ok(())
}

/// Recovery geometry must be visible as recovery geometry.
#[test]
fn recovery_token_geometry_is_registered_as_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let rows = geometry_fields_for("Error");
    let found = rows
        .iter()
        .find(|row| row.field == "found")
        .ok_or("Error.found must be registered as recovery-token geometry")?;

    assert_eq!(found.shape, AstGeometryShape::Token);
    assert_eq!(
        found.mapping,
        AstGeometryMapping::MapStartPreserveWidth,
        "a recovery token's byte width is fixed by its immutable text"
    );
    assert_eq!(found.disposition, AstGeometryDisposition::Recovery);

    let policy = ast_node_policy("Error").ok_or("Error policy must exist")?;
    assert_eq!(
        policy.classification,
        AstNodeClassification::Recovery,
        "Error must remain a recovery policy row"
    );
    Ok(())
}

/// The vocabulary is wider than current coverage; say so out loud.
///
/// `Repeated` is reserved. Registering the first row that uses it should be a
/// deliberate act that updates this denominator, not a silent widening.
#[test]
fn shape_coverage_is_an_explicit_denominator() {
    let in_use =
        geometry_shapes_in_use().iter().map(|shape| shape.token()).collect::<BTreeSet<_>>();
    let expected = ["direct", "nested", "optional", "token"].into_iter().collect::<BTreeSet<_>>();

    assert_eq!(
        in_use, expected,
        "the set of geometry shapes actually in use changed; update the denominator deliberately \
         rather than letting coverage drift"
    );
    assert!(
        !in_use.contains(AstGeometryShape::Repeated.token()),
        "AstGeometryShape::Repeated is documented as reserved vocabulary with no current row"
    );
}

/// The compile-time guard must not be silenceable with a rest pattern.
///
/// Adding a geometry field to an existing variant fails to compile inside
/// `observe_geometry_fields` — that is the point of listing every field. But an
/// author under time pressure can make that error disappear by writing `..`
/// instead of classifying the field, which would silently restore exactly the
/// drift this registry exists to prevent (it is the defect #13234 names in the
/// hand-maintained payload mapper). Convention alone does not survive; this
/// keeps it executable.
#[test]
fn the_observer_never_uses_a_rest_pattern() {
    const SOURCE: &str = include_str!("../src/geometry_policy.rs");

    let body =
        SOURCE.split_once("pub fn observe_geometry_fields").map(|(_, rest)| rest).unwrap_or(SOURCE);

    for (offset, line) in body.lines().enumerate() {
        let code = line.split("//").next().unwrap_or(line);
        assert!(
            !code.contains(".."),
            "observe_geometry_fields must destructure every field explicitly, but line {} uses a \
             rest pattern: {}\nA `..` here would let a new geometry-bearing field compile without \
             being classified.",
            offset + 1,
            line.trim()
        );
    }
}

/// A node that carries no geometry reconciles to an empty set, not a default.
#[test]
fn geometry_free_variants_are_explicitly_empty() {
    let loc = SourceLocation { start: 0, end: 1 };
    let number = NodeKind::Number { value: "1".to_string() };
    assert!(
        observe_geometry_fields(&number).is_empty(),
        "Number carries no independent payload geometry"
    );
    assert!(geometry_fields_for("Number").is_empty(), "Number must have no geometry rows");
    assert!(reconcile_node_geometry(&perl_ast::Node::new(number, loc)).is_ok());

    assert!(
        geometry_fields_for("FutureUnregisteredNode").is_empty(),
        "an unknown kind must fail closed with no rows rather than a permissive default"
    );
}

// ---------------------------------------------------------------------------
// Negative controls.
//
// These feed the checker deliberately wrong inputs. If any of them returned
// `Ok`, the positive gate above would be decorative.
// ---------------------------------------------------------------------------

/// The discriminating mutation named by #13234: geometry added to an existing
/// variant without a registry row.
#[test]
fn unregistered_geometry_on_an_existing_variant_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    // Stands in for `Subroutine { .., return_type_span: Option<SourceLocation> }`
    // being added to the enum: the field is observed, nothing registers it.
    let observed = vec![
        ObservedGeometryField {
            field: "name_span",
            shape: AstGeometryShape::Optional,
            occurrences: 1,
        },
        ObservedGeometryField {
            field: "return_type_span",
            shape: AstGeometryShape::Optional,
            occurrences: 1,
        },
    ];

    let Err(drift) = reconcile_geometry_rows("Subroutine", AST_NODE_GEOMETRY_FIELDS, &observed)
    else {
        return Err("an unregistered geometry field must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::UnregisteredField {
            kind_name: "Subroutine".to_string(),
            field: "return_type_span".to_string(),
        }
    );
    assert!(
        drift.to_string().contains("return_type_span"),
        "the failure must name the responsible field: {drift}"
    );
    Ok(())
}

/// Adding a *variant* is not the discriminating mutation, but adding geometry
/// to a previously geometry-free variant is.
#[test]
fn geometry_added_to_a_previously_geometry_free_variant_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let observed = vec![ObservedGeometryField {
        field: "value_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Number", AST_NODE_GEOMETRY_FIELDS, &observed) else {
        return Err("a geometry-free variant that gains a span must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::UnregisteredField {
            kind_name: "Number".to_string(),
            field: "value_span".to_string(),
        }
    );
    Ok(())
}

/// A row that outlives the field it names must fail rather than silently
/// describing geometry nobody carries.
#[test]
fn a_stale_registry_row_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let Err(drift) = reconcile_geometry_rows("Package", AST_NODE_GEOMETRY_FIELDS, &[]) else {
        return Err("a registered field that is never observed must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::StaleRow {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
        }
    );
    Ok(())
}

/// A registry row that misdescribes the shape must fail: a consumer would
/// otherwise map an optional field as if it were always present.
#[test]
fn a_shape_mismatch_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let observed = vec![ObservedGeometryField {
        field: "name_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Subroutine", AST_NODE_GEOMETRY_FIELDS, &observed)
    else {
        return Err("a registered shape that disagrees with observation must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::ShapeMismatch {
            kind_name: "Subroutine".to_string(),
            field: "name_span".to_string(),
            registered: AstGeometryShape::Optional,
            observed: AstGeometryShape::Direct,
        }
    );
    Ok(())
}

/// Classifying a token as a freely resizable range would let a remap invent
/// bytes that the token's immutable text does not have.
#[test]
fn a_token_registered_as_a_resizable_range_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [AstGeometryField {
        kind_name: "Error",
        field: "found",
        shape: AstGeometryShape::Token,
        mapping: AstGeometryMapping::MapRange,
        disposition: AstGeometryDisposition::Recovery,
    }];

    let observed = vec![ObservedGeometryField {
        field: "found",
        shape: AstGeometryShape::Token,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Error", &mutated, &observed) else {
        return Err("a token must not be registered as a freely resizable range".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::TokenIsNotResizable {
            kind_name: "Error".to_string(),
            field: "found".to_string(),
            mapping: AstGeometryMapping::MapRange,
        }
    );
    Ok(())
}

/// The inverse: an ordinary span may not claim the token width rule.
#[test]
fn a_non_token_claiming_width_preservation_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [AstGeometryField {
        kind_name: "Package",
        field: "name_span",
        shape: AstGeometryShape::Direct,
        mapping: AstGeometryMapping::MapStartPreserveWidth,
        disposition: AstGeometryDisposition::SourceExact,
    }];

    let observed = vec![ObservedGeometryField {
        field: "name_span",
        shape: AstGeometryShape::Direct,
        occurrences: 1,
    }];

    let Err(drift) = reconcile_geometry_rows("Package", &mutated, &observed) else {
        return Err("only a token may claim the width-preserving mapping rule".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::WidthPreservationRequiresToken {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
            shape: AstGeometryShape::Direct,
        }
    );
    Ok(())
}

/// Two rows for the same field would give a consumer two mapping rules.
#[test]
fn a_duplicate_registry_row_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mutated = [
        AstGeometryField {
            kind_name: "Package",
            field: "name_span",
            shape: AstGeometryShape::Direct,
            mapping: AstGeometryMapping::MapRange,
            disposition: AstGeometryDisposition::SourceExact,
        },
        AstGeometryField {
            kind_name: "Package",
            field: "name_span",
            shape: AstGeometryShape::Optional,
            mapping: AstGeometryMapping::MapRange,
            disposition: AstGeometryDisposition::SourceExact,
        },
    ];

    let Err(drift) = reconcile_geometry_rows("Package", &mutated, &[]) else {
        return Err("a duplicated field row must fail the gate".into());
    };

    assert_eq!(
        drift,
        AstGeometryDrift::DuplicateRow {
            kind_name: "Package".to_string(),
            field: "name_span".to_string(),
        }
    );
    Ok(())
}

/// An absent optional span is legitimate and must not be reported as drift.
///
/// This is the opposite-direction control for the unregistered-field test: the
/// gate must distinguish "this field is missing from the registry" from "this
/// instance simply has no value here".
#[test]
fn an_absent_optional_span_is_not_drift() -> Result<(), Box<dyn std::error::Error>> {
    let loc = SourceLocation { start: 0, end: 4 };
    let without_span = perl_ast::Node::new(
        NodeKind::Subroutine {
            name: Some("f".to_string()),
            name_span: None,
            declarator: Some("sub".to_string()),
            prototype: None,
            signature: None,
            attributes: vec![],
            body: Box::new(perl_ast::Node::new(NodeKind::Block { statements: vec![] }, loc)),
        },
        loc,
    );

    let observed = observe_geometry_fields(&without_span.kind);
    assert_eq!(observed.len(), 1, "the field is still registered even when absent");
    assert_eq!(observed[0].occurrences, 0, "an absent optional span observes zero spans");

    reconcile_node_geometry(&without_span)?;
    Ok(())
}

/// The recovery token observes its real width, which is what the width-
/// preserving mapping rule protects.
#[test]
fn a_recovery_token_carries_its_own_validated_width() -> Result<(), Box<dyn std::error::Error>> {
    let loc = SourceLocation { start: 10, end: 13 };
    let token = Token::new_checked(TokenKind::Unknown, "abc", 10, 13)?;
    assert_eq!(token.text.len(), 3, "token text length is its byte width");

    let node = perl_ast::Node::new(
        NodeKind::Error {
            message: "unexpected".to_string(),
            expected: vec![TokenKind::Eof],
            found: Some(token),
            partial: None,
        },
        loc,
    );

    let observed = observe_geometry_fields(&node.kind);
    assert_eq!(
        observed,
        vec![ObservedGeometryField {
            field: "found",
            shape: AstGeometryShape::Token,
            occurrences: 1,
        }]
    );
    reconcile_node_geometry(&node)?;
    Ok(())
}

/// Nested geometry counts real elements, not declared cardinality.
#[test]
fn nested_catch_variable_geometry_counts_actual_elements() -> Result<(), Box<dyn std::error::Error>>
{
    let loc = SourceLocation { start: 0, end: 1 };
    let block = || Box::new(perl_ast::Node::new(NodeKind::Block { statements: vec![] }, loc));

    let node = perl_ast::Node::new(
        NodeKind::Try {
            body: block(),
            catch_blocks: vec![
                (Some(("$e".to_string(), loc)), block()),
                (None, block()),
                (Some(("$f".to_string(), loc)), block()),
            ],
            finally_block: None,
        },
        loc,
    );

    let observed = observe_geometry_fields(&node.kind);
    assert_eq!(
        observed,
        vec![ObservedGeometryField {
            field: "catch_blocks.variable",
            shape: AstGeometryShape::Nested,
            occurrences: 2,
        }],
        "only catch blocks that actually bind a variable carry a span"
    );
    reconcile_node_geometry(&node)?;
    Ok(())
}
