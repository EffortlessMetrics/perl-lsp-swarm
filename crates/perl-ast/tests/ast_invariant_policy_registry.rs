use perl_ast::{
    AST_NODE_POLICIES, AST_NODE_POLICY_SCHEMA_VERSION, AstChildContainmentPolicy,
    AstChildOrderPolicy, AstChildOverlapPolicy, AstEmptyRangePolicy, AstNodeClassification,
    AstPayloadPolicy, AstSourceBacking, NodeKind, ast_node_policy, node_kind_fixtures,
    policy_accepts_observed_children,
};
use std::collections::BTreeSet;
use std::ops::ControlFlow;

#[test]
fn registry_exactly_matches_the_canonical_nodekind_order() {
    let registered = AST_NODE_POLICIES.iter().map(|policy| policy.kind_name).collect::<Vec<_>>();
    assert_eq!(
        registered,
        NodeKind::ALL_KIND_NAMES,
        "policy registry rows must cover every NodeKind in canonical declaration order"
    );

    let unique = registered.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), registered.len(), "policy registry contains duplicate NodeKinds");
    assert_eq!(AST_NODE_POLICY_SCHEMA_VERSION, 1, "policy schema version drifted");
}

#[test]
fn every_policy_has_a_coherent_structural_classification() {
    for policy in AST_NODE_POLICIES {
        match policy.classification {
            AstNodeClassification::Leaf | AstNodeClassification::SourceBoundary => {
                assert_eq!(
                    policy.child_containment,
                    AstChildContainmentPolicy::NotApplicable,
                    "{} is non-child-bearing but requires containment",
                    policy.kind_name
                );
                assert_eq!(
                    policy.child_order,
                    AstChildOrderPolicy::NotApplicable,
                    "{} is non-child-bearing but registers a child-order policy",
                    policy.kind_name
                );
                assert_eq!(
                    policy.child_overlap,
                    AstChildOverlapPolicy::NotApplicable,
                    "{} is non-child-bearing but registers a child-overlap policy",
                    policy.kind_name
                );
            }
            AstNodeClassification::ChildBearing | AstNodeClassification::Wrapper => {
                assert_eq!(
                    policy.child_containment,
                    AstChildContainmentPolicy::Required,
                    "{} may expose children but lacks containment policy",
                    policy.kind_name
                );
                assert_eq!(
                    policy.child_order,
                    AstChildOrderPolicy::Nondecreasing,
                    "{} may expose children but lacks a child-order policy",
                    policy.kind_name
                );
                assert_ne!(
                    policy.child_overlap,
                    AstChildOverlapPolicy::NotApplicable,
                    "{} may expose children but lacks a child-overlap policy",
                    policy.kind_name
                );
            }
            AstNodeClassification::Recovery => {
                assert_eq!(
                    policy.empty_range,
                    AstEmptyRangePolicy::SyntheticAllowed,
                    "{} is a recovery node without the synthetic empty-range policy",
                    policy.kind_name
                );
                assert!(
                    policy.payload_policies.contains(&AstPayloadPolicy::RecoverySynthetic),
                    "{} is a recovery node without the RecoverySynthetic payload policy",
                    policy.kind_name
                );
                assert!(
                    matches!(
                        policy.source_backing,
                        AstSourceBacking::Synthetic | AstSourceBacking::Mixed
                    ),
                    "{} is a recovery node with non-synthetic source backing {:?}",
                    policy.kind_name,
                    policy.source_backing
                );
            }
        }
    }
}

#[test]
fn recovery_registry_is_generated_from_the_canonical_recovery_set() {
    let registered = AST_NODE_POLICIES
        .iter()
        .filter(|policy| policy.classification == AstNodeClassification::Recovery)
        .map(|policy| policy.kind_name)
        .collect::<BTreeSet<_>>();
    let canonical = NodeKind::RECOVERY_KIND_NAMES.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(
        registered, canonical,
        "recovery-classified policy rows must equal NodeKind::RECOVERY_KIND_NAMES"
    );
}

/// Derive the observed child structure of every `NodeKind` through the
/// canonical field-aware traversal and reconcile it against the policy row.
///
/// The fixture constructor is compile-exhaustive: a new variant or field fails
/// compilation instead of silently escaping classification. A wrongly
/// classified existing row therefore turns red here instead of becoming
/// trusted input to downstream consumers.
#[test]
fn every_kind_reconciles_observed_structure_with_its_policy_row()
-> Result<(), Box<dyn std::error::Error>> {
    let fixtures = node_kind_fixtures();

    let fixture_names =
        fixtures.iter().map(|fixture| fixture.sample.kind.kind_name()).collect::<BTreeSet<_>>();
    let canonical = NodeKind::ALL_KIND_NAMES.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        fixture_names, canonical,
        "the structural fixture must cover every NodeKind exactly once"
    );

    for fixture in &fixtures {
        let kind_name = fixture.sample.kind.kind_name();
        let policy = ast_node_policy(kind_name)
            .ok_or_else(|| format!("{kind_name} has a fixture but no policy row"))?;

        // Observe the fully populated sample through the canonical traversal,
        // keeping per-field occurrence counts: collection-backed fixture fields
        // carry two children, so repetition is observable rather than declared.
        let mut observed_counts: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        let mut lost_field_identity = false;
        let _ = fixture.sample.try_for_each_child_with_field(|field, _child| {
            match field {
                Some(field) => *observed_counts.entry(field.name()).or_insert(0) += 1,
                None => lost_field_identity = true,
            }
            ControlFlow::<()>::Continue(())
        });
        assert!(!lost_field_identity, "{kind_name}: a child emission lost its field identity");

        let mut expected_counts: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for (name, _repeating) in fixture.child_fields {
            assert!(
                expected_counts.insert(*name, 0).is_none(),
                "{kind_name}: fixture declares child field {name} twice"
            );
        }
        let observed_names: Vec<&str> = observed_counts.keys().copied().collect();
        let expected_names: Vec<&str> = expected_counts.keys().copied().collect();
        assert_eq!(
            observed_names, expected_names,
            "{kind_name}: canonical traversal observed child fields {observed_names:?} but the \
             fixture declares {expected_names:?}; the field-aware traversal remains the authority"
        );
        for (name, repeating) in fixture.child_fields {
            let count = observed_counts.get(name).copied().unwrap_or(0);
            if *repeating {
                assert!(
                    count >= 2,
                    "{kind_name}: field {name} is declared repeating but the fully populated \
                     sample observed it {count} time(s)"
                );
            } else {
                assert_eq!(
                    count, 1,
                    "{kind_name}: field {name} is declared single-occurrence but the sample \
                     observed it {count} times"
                );
            }
        }

        // Every declared field lives in exactly one governance bucket.
        let mut bucket_owner: std::collections::BTreeMap<&str, &str> =
            std::collections::BTreeMap::new();
        for (bucket, fields) in [
            ("child_fields", fixture.child_fields.iter().map(|(name, _)| *name).collect()),
            ("payload_fields", fixture.payload_fields.to_vec()),
            ("untracked_fields", fixture.untracked_fields.to_vec()),
        ] {
            for name in fields {
                assert!(
                    bucket_owner.insert(name, bucket).is_none(),
                    "{kind_name}: field {name} appears in more than one governance bucket"
                );
            }
        }

        let observed: Vec<&str> = observed_counts.keys().copied().collect();

        assert!(
            policy_accepts_observed_children(policy, !observed.is_empty()),
            "{kind_name}: policy classification {:?} rejects the children its variant actually \
             bears ({observed:?})",
            policy.classification
        );

        match policy.classification {
            AstNodeClassification::Leaf | AstNodeClassification::SourceBoundary => {
                assert!(
                    fixture.child_fields.is_empty(),
                    "{kind_name}: classified {:?} but the variant bears child fields {:?}",
                    policy.classification,
                    fixture.child_fields
                );
            }
            AstNodeClassification::Wrapper => {
                assert_eq!(
                    fixture.child_fields.len(),
                    1,
                    "{kind_name}: a Wrapper owns exactly one optional or required child field, \
                     found {:?}",
                    fixture.child_fields
                );
                assert!(
                    !fixture.child_fields[0].1,
                    "{kind_name}: a Wrapper child must be single-occurrence, not repeating"
                );
            }
            AstNodeClassification::ChildBearing => {
                assert!(
                    !fixture.child_fields.is_empty(),
                    "{kind_name}: classified ChildBearing but the variant bears no child fields"
                );
                if fixture.child_fields.len() == 1 && !fixture.child_fields[0].1 {
                    assert!(
                        !fixture.payload_fields.is_empty(),
                        "{kind_name}: a single-child single-occurrence ChildBearing node must \
                         carry governed payload fields, or it is a Wrapper"
                    );
                }
            }
            AstNodeClassification::Recovery => {}
        }

        if policy.classification != AstNodeClassification::Recovery {
            // Recovery rows carry RecoverySynthetic as a semantic prohibition on
            // source-exactness claims, not as governance over present fields.
            assert_eq!(
                fixture.payload_fields.is_empty(),
                policy.payload_policies.is_empty(),
                "{kind_name}: governed payload fields {:?} and registered payload policies {:?} \
                 must both be present or both be absent",
                fixture.payload_fields,
                policy.payload_policies
            );
        }
    }
    Ok(())
}

#[test]
fn source_derived_payload_classes_are_registered_deliberately()
-> Result<(), Box<dyn std::error::Error>> {
    let number = ast_node_policy("Number").ok_or("Number policy must exist")?;
    assert_eq!(
        number.classification,
        AstNodeClassification::Leaf,
        "Number must remain a leaf policy row"
    );
    assert!(
        number.payload_policies.contains(&AstPayloadPolicy::LiteralRawAndCooked),
        "Number must register the LiteralRawAndCooked payload policy"
    );

    let variable = ast_node_policy("Variable").ok_or("Variable policy must exist")?;
    assert!(
        variable.payload_policies.contains(&AstPayloadPolicy::VariableSigilAndName),
        "Variable must register the VariableSigilAndName payload policy"
    );

    let heredoc = ast_node_policy("Heredoc").ok_or("Heredoc policy must exist")?;
    assert_eq!(
        heredoc.classification,
        AstNodeClassification::SourceBoundary,
        "Heredoc must remain a source-boundary policy row"
    );
    assert!(
        heredoc.payload_policies.contains(&AstPayloadPolicy::HeredocLabelAndIndent),
        "Heredoc must register the HeredocLabelAndIndent payload policy"
    );

    let package = ast_node_policy("Package").ok_or("Package policy must exist")?;
    assert!(
        package.payload_policies.contains(&AstPayloadPolicy::DeclarationNameAnchor),
        "Package must register the DeclarationNameAnchor payload policy"
    );
    Ok(())
}

#[test]
fn misclassifying_an_observed_child_is_a_load_bearing_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let leaf = ast_node_policy("Number").ok_or("Number policy must exist")?;
    assert!(
        !policy_accepts_observed_children(leaf, true),
        "a Leaf policy must reject an observed child"
    );
    assert!(
        policy_accepts_observed_children(leaf, false),
        "a Leaf policy must accept a childless observation"
    );

    let container = ast_node_policy("Program").ok_or("Program policy must exist")?;
    assert!(
        policy_accepts_observed_children(container, true),
        "a ChildBearing policy must accept an observed child"
    );
    Ok(())
}

#[test]
fn typed_lookup_derives_the_canonical_token_from_the_enum() {
    use perl_ast::{Node, SourceLocation, ast_node_policy_of};

    let loc = SourceLocation { start: 0, end: 1 };
    let number = Node::new(NodeKind::Number { value: "1".to_string() }, loc);
    let typed = ast_node_policy_of(&number.kind);
    let named = ast_node_policy("Number");
    assert_eq!(
        typed, named,
        "the NodeKind-typed lookup must resolve the same row as the canonical token"
    );
    assert!(
        typed.is_some(),
        "every current NodeKind must resolve a policy row through the typed lookup"
    );
}

#[test]
fn unknown_nodekind_names_do_not_inherit_a_permissive_default() {
    assert!(
        ast_node_policy("FutureUnregisteredNode").is_none(),
        "an unregistered NodeKind name must fail closed with no policy row"
    );
}
