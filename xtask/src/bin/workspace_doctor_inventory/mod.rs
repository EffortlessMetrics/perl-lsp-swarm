//! Source-backed inventory of current workspace doctor and local readiness behavior.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

mod inventory;
mod rows;
mod sources;

/// Lowercase hex encoding of the SHA-256 digest of `bytes`.
///
/// The workspace's `sha2` digest output does not implement `LowerHex`, so the
/// bytes are encoded explicitly. This mirrors the existing helper in
/// `xtask::publication_drift::authority`.
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

// This module is shared by the `workspace-doctor-inventory` bin (which uses
// `render_human`) and by the inventory tests, which include it by path and do
// not. The re-export is live; only the test include sees it as unused.
#[allow(unused_imports)]
pub use inventory::{build_inventory, render_human, validate_inventory};
// Re-exported for the contract and falsifier test crates, which include this
// module through `#[path]`. The binary itself does not call them, so the
// binary's own compilation would otherwise report them unused.
#[allow(unused_imports)]
pub use inventory::validate_rows;
#[allow(unused_imports)]
pub use rows::canonical_rows;

pub(super) const SCHEMA: u32 = 1;
pub(super) const SOURCE_PATHS: [&str; 6] = [
    "justfile",
    "xtask/src/tasks/writer_admission.rs",
    "xtask/src/tasks/devex_doctor.rs",
    "xtask/src/tasks/worktrees.rs",
    "crates/perl-ci-hygiene/src/cli.rs",
    "scripts/storage-doctor",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityState {
    Current,
    Planned,
    CurrentTransitional,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationPosture {
    ReadOnly,
    AutomaticMutation,
    NotObserved,
    InheritsDoctorMutation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultClass {
    Advisory,
    Blocked,
    NotProven,
    RepairAvailable,
    BlockedOrNotProven,
    TypedReadiness,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Disposition {
    ReuseAuthority,
    MoveToExplicitRepair,
    RetainAdvisory,
    RetainBlocking,
    RetireDuplicate,
    ReviseSemantics,
    NotProven,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRow {
    pub check_id: String,
    pub fact_key: String,
    pub current_implementation: String,
    pub source_path: String,
    pub canonical_authority: String,
    pub authority_state: AuthorityState,
    pub subject: String,
    pub current_mutation: MutationPosture,
    pub current_exit_behavior: String,
    pub target_result: ResultClass,
    pub selected_transitions: Vec<String>,
    pub disposition: Disposition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorHeading {
    pub number: u32,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveMutation {
    pub kind: String,
    pub line: String,
    pub owned_by: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub finding_id: String,
    pub check_id: String,
    pub current: String,
    pub required_disposition: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDigest {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema: u32,
    pub status: String,
    pub doctor_check_headings: Vec<DoctorHeading>,
    pub rows: Vec<CheckRow>,
    pub findings: Vec<Finding>,
    pub active_mutations: Vec<ActiveMutation>,
    pub sources: BTreeMap<String, SourceDigest>,
    pub inventory_digest: String,
}
