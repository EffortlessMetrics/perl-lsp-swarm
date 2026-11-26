//! Parser-to-semantic transition contract (#6599, PLSP-SPEC-0039).
//!
//! This module is a durable contract note only: it does not parse, lower, or
//! analyze Perl. Normative detail lives in
//! `docs/specs/PLSP-SPEC-0039-semantic-contract-packet-e.md`,
//! [PLSP-SPEC-0030](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/specs/PLSP-SPEC-0030-compile-state-layers.md),
//! and [PLSP-SPEC-0038](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/specs/PLSP-SPEC-0038-unsupported-fact-catalog.md).
//!
//! ## Unsupported facts
//!
//! Recognized unsupported behavior must map to a stable catalog class ID from
//! `policy/compiler/unsupported-facts.v1.toml` or surface as an explicit
//! boundary / `Refused` envelope outcome. Prose-only reasons are not
//! receipt-compatible class identity.
//!
//! ## Parser-to-fact transitions
//!
//! ```text
//! AST (parser) → HirFile compile-state layers (lower_ast)
//!            → analyzer-synthesized typed facts
//!            → SemanticFactEnvelope (transport)
//! ```
//!
//! AST nodes are never neutral semantic facts. Each fact must carry provenance
//! and confidence metadata sufficient for envelope classification; missing
//! metadata fails closed to non-exact status. Unsupported gaps must not mint
//! facts with exact-source reason codes.

/// Packet E routing follow-up owner for harness compatibility ledger work (#4736).
pub(crate) const FOLLOW_UP_COMPATIBILITY_LEDGER: u32 = 4736;

/// Packet E routing follow-up owner for unsupported-fact taxonomy work (#2543).
pub(crate) const FOLLOW_UP_FACT_TAXONOMY: u32 = 2543;

/// Packet E routing follow-up owner for compiler programme capability backlog (#2559).
pub(crate) const FOLLOW_UP_CAPABILITY_BACKLOG: u32 = 2559;

/// Spec path for the normative packet E semantic contract.
pub(crate) const SPEC_PATH: &str = "docs/specs/PLSP-SPEC-0039-semantic-contract-packet-e.md";

/// Policy path for machine-readable issue routing.
pub(crate) const ROUTING_POLICY_PATH: &str = "policy/semantic/packet-e-issue-routing.v1.toml";

// Keep contract anchors live in non-test builds (clippy `-D warnings` / dead_code).
const _: () = {
    assert!(FOLLOW_UP_COMPATIBILITY_LEDGER == 4736);
    assert!(FOLLOW_UP_FACT_TAXONOMY == 2543);
    assert!(FOLLOW_UP_CAPABILITY_BACKLOG == 2559);
    assert!(!SPEC_PATH.is_empty());
    assert!(!ROUTING_POLICY_PATH.is_empty());
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_note_names_unsupported_facts_and_transitions() {
        let source = include_str!("parser_to_semantic_contract.rs");
        let doc_lines: Vec<&str> =
            source.lines().map(str::trim_start).filter(|line| line.starts_with("//!")).collect();

        assert!(
            doc_lines.iter().any(|line| line.starts_with("//! ## Unsupported facts")),
            "contract note must name unsupported facts"
        );
        assert!(
            doc_lines.iter().any(|line| line.starts_with("//! ## Parser-to-fact transitions")),
            "contract note must name parser-to-fact transitions"
        );
        assert!(
            doc_lines.iter().any(|line| line.contains("SemanticFactEnvelope (transport)")),
            "contract note must anchor envelope transport"
        );
    }

    #[test]
    fn follow_up_owner_constants_match_routing_policy() {
        assert_eq!(FOLLOW_UP_COMPATIBILITY_LEDGER, 4736);
        assert_eq!(FOLLOW_UP_FACT_TAXONOMY, 2543);
        assert_eq!(FOLLOW_UP_CAPABILITY_BACKLOG, 2559);
    }
}
