//! Versioned accounting authority for the bounded FR-C05 fixture claim.
//!
//! This is intentionally maintained separately from the packet-node registry:
//! changing fixture construction cannot silently change the claimed membership.
//! It is not the full feature-readiness DAG; that authority remains #11279.

use super::model::{DenominatorDisposition, DenominatorEntry};

/// Version of the co-maintained denominator authority consumed by the gate.
pub const VERSION: &str = "feature_readiness_denominator.v1";

const A: DenominatorDisposition = DenominatorDisposition::Actionable;
const D: DenominatorDisposition = DenominatorDisposition::Deferred;
const X: DenominatorDisposition = DenominatorDisposition::Excluded;

const fn entry(
    issue: u32,
    packet_node: Option<&'static str>,
    disposition: DenominatorDisposition,
    reason: &'static str,
) -> DenominatorEntry {
    DenominatorEntry { issue, packet_node, disposition, reason }
}

/// The bounded fixture denominator for #11286.
pub const ENTRIES: &[DenominatorEntry] = &[
    entry(1850, Some("fr_1850_semantic_token_geometry"), A, "mandated product leaf"),
    entry(5108, Some("fr_5108_navigation_truth_repair"), A, "mandated product leaf"),
    entry(6992, Some("fr_6992_installed_critic_journey_proof"), A, "mandated installed proof row"),
    entry(6997, Some("fr_6997_critic_product_child"), A, "mandated product child"),
    entry(7122, Some("fr_7122_support_registry_governance_row"), A, "mandated governance mapping"),
    entry(
        7278,
        Some("fr_7278_dap_release_ruling"),
        X,
        "external/manual ruling boundary cannot emit actionable coding work",
    ),
    entry(
        8277,
        Some("fr_8277_import_governed_operations_leaf"),
        A,
        "mandated controller-child implementation row",
    ),
    entry(
        8301,
        Some("fr_8301_deferred_npm_distribution"),
        D,
        "release-topology decision remains deferred",
    ),
    entry(8305, Some("fr_8305_import_containment_leaf"), A, "mandated containment leaf"),
    entry(8336, Some("fr_8336_import_claim_proof_row"), A, "mandated proof-only row"),
    entry(8944, Some("fr_8944_signature_semantic_cutover"), A, "mandated product leaf"),
    entry(9349, Some("fr_9349_formatter_product_child"), A, "mandated product child"),
    entry(9415, Some("fr_9415_dap_reliability_leaf"), A, "mandated product leaf"),
    entry(
        10724,
        Some("fr_10724_formatting_currentness_proof"),
        A,
        "mandated exact-process proof row",
    ),
    entry(11250, Some("fr_11250_semantic_token_shadow"), A, "mandated shadow row"),
    entry(11259, Some("fr_11259_semantic_token_live_cutover"), A, "mandated live cutover row"),
    entry(
        11261,
        Some("fr_11261_object_facts_source_anchors"),
        A,
        "mandated optional framework row",
    ),
    entry(
        11263,
        Some("fr_11263_application_framework_projection"),
        A,
        "mandated optional framework row",
    ),
    entry(11267, Some("fr_11267_installed_vscode_proof"), A, "mandated installed proof row"),
    entry(11271, Some("fr_11271_zydeco_research"), A, "mandated research/decision row"),
    entry(10122, None, X, "routing controller, not a packet work item"),
    entry(11280, None, X, "current-tree observational plane owned by its successor"),
    entry(11281, None, X, "offline-readiness observational plane owned by its successor"),
];
