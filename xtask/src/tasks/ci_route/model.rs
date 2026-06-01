use serde::Serialize;
use std::collections::BTreeMap;

use super::coverage::CoverageProofPackReceipt;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct CiRouteReceipt {
    pub(super) schema_version: &'static str,
    pub(super) provider_action: &'static str,
    pub(super) claim_boundary: &'static str,
    pub(super) base: String,
    pub(super) head: String,
    pub(super) changed_files: Vec<String>,
    pub(super) changed_surfaces: Vec<String>,
    pub(super) required_proof_packs: Vec<ProofPackReceipt>,
    pub(super) skipped_by_policy: BTreeMap<String, String>,
    pub(super) coverage_pack_selector: Vec<String>,
    pub(super) coverage_proof_packs: Vec<CoverageProofPackReceipt>,
    pub(super) estimated_lem: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct ProofPackReceipt {
    pub(super) id: String,
    pub(super) commands: Vec<String>,
}
