//! Semantic packet E contract discovery (#6599).

#[test]
fn analyzer_consumers_can_import_parser_to_semantic_contract_note() {
    use perl_semantic_facts::parser_to_semantic_contract::{
        FOLLOW_UP_CAPABILITY_BACKLOG, FOLLOW_UP_COMPATIBILITY_LEDGER, FOLLOW_UP_FACT_TAXONOMY,
        ROUTING_POLICY_PATH, SPEC_PATH,
    };
    assert_eq!(FOLLOW_UP_COMPATIBILITY_LEDGER, 4736);
    assert_eq!(FOLLOW_UP_FACT_TAXONOMY, 2543);
    assert_eq!(FOLLOW_UP_CAPABILITY_BACKLOG, 2559);
    assert!(SPEC_PATH.contains("PLSP-SPEC-0039"));
    assert!(ROUTING_POLICY_PATH.contains("packet-e-issue-routing"));
}
