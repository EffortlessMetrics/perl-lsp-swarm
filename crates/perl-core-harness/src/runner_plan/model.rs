//! Typed runner-selection and membership-parity receipts.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RUNNER_PLAN_SCHEMA_VERSION: &str = "perl_core_harness.runner_plan.v1";
pub const RUNNER_PARITY_SCHEMA_VERSION: &str = "perl_core_harness.runner_parity.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Test,
    Harness,
    DirectFallback,
}

impl RunnerKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "test" => Ok(Self::Test),
            "harness" => Ok(Self::Harness),
            "direct_fallback" | "direct-fallback" => Ok(Self::DirectFallback),
            other => Err(format!(
                "unsupported runner {other}; expected test, harness, or direct_fallback"
            )),
        }
    }

    pub fn entrypoint(self) -> &'static str {
        match self {
            Self::Test => "t/TEST",
            Self::Harness => "t/harness",
            Self::DirectFallback => "perl-core-harness direct fallback",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceForm {
    DotT,
    TestPl,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePathClass {
    LocalT,
    RootLib,
    Dist,
    Ext,
    Cpan,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationContextClass {
    BaseCompRun,
    LocalTestInit,
    RootLibU1,
    DistributionU2T,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerSourceItem {
    pub raw_path: String,
    pub canonical_path: String,
    pub source_form: SourceForm,
    pub path_class: SourcePathClass,
    pub invocation_context: InvocationContextClass,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerScheduling {
    pub jobs: Option<u32>,
    pub asap: bool,
    pub state_ordering: bool,
    pub properties: BTreeMap<String, String>,
}

impl Default for RunnerScheduling {
    fn default() -> Self {
        Self { jobs: None, asap: false, state_ordering: false, properties: BTreeMap::new() }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationCaptureStatus {
    NotProven,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerPlan {
    pub schema_version: String,
    pub matrix_fingerprint: String,
    pub target_id: String,
    pub target_contract_digest: String,
    pub runner: RunnerKind,
    pub runner_entrypoint: String,
    pub canonical_selection_entrypoint: String,
    pub raw_discovery_digest: String,
    pub source_items: Vec<RunnerSourceItem>,
    pub normalized_order: Vec<String>,
    pub normalized_membership: Vec<String>,
    pub scheduling: RunnerScheduling,
    pub invocation_capture: InvocationCaptureStatus,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipParityStatus {
    Parity,
    Mismatch,
    NotProven,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerParityReport {
    pub schema_version: String,
    pub matrix_fingerprint: String,
    pub target_id: String,
    pub target_contract_digest: String,
    pub left_runner: RunnerKind,
    pub right_runner: RunnerKind,
    pub membership_status: MembershipParityStatus,
    pub missing_from_right: Vec<String>,
    pub extra_in_right: Vec<String>,
    pub order_equal: bool,
    pub scheduling_equal: bool,
    pub invocation_capture: InvocationCaptureStatus,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}
