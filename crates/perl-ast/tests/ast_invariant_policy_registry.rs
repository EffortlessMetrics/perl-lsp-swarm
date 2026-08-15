use perl_ast::{
    AST_NODE_POLICIES, AST_NODE_POLICY_SCHEMA_VERSION, AstChildContainmentPolicy,
    AstChildOrderPolicy, AstChildOverlapPolicy, AstEmptyRangePolicy, AstNodeClassification,
    AstPayloadPolicy, AstSourceBacking, NodeKind, ast_node_policy,
    policy_accepts_observed_children,
};
use std::collections::BTreeSet;

#[test]
fn registry_exactly_matches_the_canonical_nodekind_order() {
    let registered = AST_NODE_POLICIES.iter().map(|policy| policy.kind_name).collect::<Vec<_>>();
    assert_eq!(registered, NodeKind::ALL_KIND_NAMES);

    let unique = registered.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), registered.len(), "policy registry contains duplicate NodeKinds");
    assert_eq!(AST_NODE_POLICY_SCHEMA_VERSION, 1);
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
                assert_eq!(policy.child_order, AstChildOrderPolicy::NotApplicable);
                assert_eq!(policy.child_overlap, AstChildOverlapPolicy::NotApplicable);
            }
            AstNodeClassification::ChildBearing | AstNodeClassification::Wrapper => {
                assert_eq!(
                    policy.child_containment,
                    AstChildContainmentPolicy::Required,
                    "{} may expose children but lacks containment policy",
                    policy.kind_name
                );
                assert_eq!(policy.child_order, AstChildOrderPolicy::Nondecreasing);
                assert_ne!(policy.child_overlap, AstChildOverlapPolicy::NotApplicable);
            }
            AstNodeClassification::Recovery => {
                assert_eq!(policy.empty_range, AstEmptyRangePolicy::SyntheticAllowed);
                assert!(policy.payload_policies.contains(&AstPayloadPolicy::RecoverySynthetic));
                assert!(matches!(
                    policy.source_backing,
                    AstSourceBacking::Synthetic | AstSourceBacking::Mixed
                ));
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

    assert_eq!(registered, canonical);
}

#[test]
fn source_derived_payload_classes_are_registered_deliberately()
-> Result<(), Box<dyn std::error::Error>> {
    let number = ast_node_policy("Number").ok_or("Number policy must exist")?;
    assert_eq!(number.classification, AstNodeClassification::Leaf);
    assert!(number.payload_policies.contains(&AstPayloadPolicy::LiteralRawAndCooked));

    let variable = ast_node_policy("Variable").ok_or("Variable policy must exist")?;
    assert!(variable.payload_policies.contains(&AstPayloadPolicy::VariableSigilAndName));

    let heredoc = ast_node_policy("Heredoc").ok_or("Heredoc policy must exist")?;
    assert_eq!(heredoc.classification, AstNodeClassification::SourceBoundary);
    assert!(heredoc.payload_policies.contains(&AstPayloadPolicy::HeredocLabelAndIndent));

    let package = ast_node_policy("Package").ok_or("Package policy must exist")?;
    assert!(package.payload_policies.contains(&AstPayloadPolicy::DeclarationNameAnchor));
    Ok(())
}

#[test]
fn misclassifying_an_observed_child_is_a_load_bearing_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let leaf = ast_node_policy("Number").ok_or("Number policy must exist")?;
    assert!(!policy_accepts_observed_children(leaf, true));
    assert!(policy_accepts_observed_children(leaf, false));

    let container = ast_node_policy("Program").ok_or("Program policy must exist")?;
    assert!(policy_accepts_observed_children(container, true));
    Ok(())
}

#[test]
fn unknown_nodekind_names_do_not_inherit_a_permissive_default() {
    assert!(ast_node_policy("FutureUnregisteredNode").is_none());
}
