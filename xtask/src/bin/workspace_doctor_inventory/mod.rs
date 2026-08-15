//! Source-backed inventory of current workspace doctor and local readiness behavior.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod inventory;
mod rows;
mod sources;

pub use inventory::{build_inventory, render_human, validate_inventory, validate_rows};
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
