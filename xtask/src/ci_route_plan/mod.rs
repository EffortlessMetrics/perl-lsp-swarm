//! Deterministic domain payload for routed CI planning.
//!
//! This module owns the versioned, side-effect-free representation of a route
//! plan. It does not discover changed files, select gates, inspect workflow
//! YAML, execute commands, or decide live GitHub enforcement.

mod compile;
mod validate;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CI_ROUTE_PLAN_SCHEMA: &str = "ci_route_plan.v1";
pub const CI_ROUTE_PLAN_PRODUCER: &str = "xtask::ci_route_plan";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiRoutePlanV1 {
    pub schema: String,
    pub producer: String,
    pub subject: RouteSubjectRef,
    pub profile: String,
    pub policy_digest: String,
    pub workflow_digest: String,
    pub selection: RouteSelectionEvidence,
    pub rows: Vec<RoutePlanRow>,
    pub summary: RoutePlanSummary,
    pub semantic_fingerprint: String,
}

impl CiRoutePlanV1 {
    pub fn compile(input: CompileRoutePlanInput) -> Result<Self, String> {
        compile::compile(input)
    }

    pub fn normalize(&mut self) -> Result<(), String> {
        validate::normalize(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate::validate(self)
    }

    pub fn canonical_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self).map_err(|error| error.to_string())
    }

    pub fn explain(&self, gate_id: Option<&str>) -> Result<String, String> {
        self.validate()?;
        if let Some(gate_id) = gate_id {
            let row = self
                .rows
                .iter()
                .find(|row| row.gate_id == gate_id)
                .ok_or_else(|| format!("unknown gate {gate_id:?}"))?;
            return serde_json::to_string_pretty(row).map_err(|error| error.to_string());
        }
        serde_json::to_string_pretty(&self.summary).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSubjectRef {
    /// Opaque subject kind supplied by the exact-subject authority (#8042).
    pub kind: String,
    pub head_sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    /// Digest of the complete subject payload supplied by its owner.
    pub subject_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSelectionEvidence {
    pub base: String,
    pub scope_ok: bool,
    pub fallback_used: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub package_args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<RouteScopeEvidence>,
    pub selector_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteScopeEvidence {
    pub head_sha: String,
    pub diff_class: String,
    pub direct_crates: Vec<ScopedIdentity>,
    pub reverse_dependencies: Vec<ScopedIdentity>,
    pub architecture_wideners: Vec<ScopedIdentity>,
    pub risk_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedIdentity {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePlanRow {
    pub gate_id: String,
    pub policy_role: PolicyRole,
    pub selector_role: SelectorRole,
    pub applicability: Applicability,
    pub outcome: PlannedOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorRole {
    AlwaysOn,
    RustScoped,
    RustFallback,
    RustPackageScoped,
    Static,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRole {
    Required,
    Advisory,
    Informational,
    LocalOnly,
    ReleaseOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    Applicable,
    NotApplicable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlannedOutcome {
    Run {
        command: String,
        timeout_seconds: u64,
        reason: String,
    },
    ScopedNoop {
        reason: String,
        selector_digest: String,
    },
    Quarantined {
        reason: String,
        owner_issue: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        review_after: Option<String>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RoutePlanSummary {
    pub governed: u64,
    pub run: u64,
    pub scoped_noop: u64,
    pub quarantined: u64,
    pub error: u64,
    pub by_policy_role: BTreeMap<PolicyRole, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileRoutePlanInput {
    pub subject: RouteSubjectRef,
    pub plan: LegacyGatePlanInput,
    pub policy: LegacyGatePolicyInput,
    pub policy_digest: String,
    pub workflow_digest: String,
    pub selector_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyGatePlanInput {
    pub tier: String,
    pub base: String,
    pub scope_ok: bool,
    pub fallback_used: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub package_args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<LegacyScopeInput>,
    pub selected: Vec<LegacyPlannedGate>,
    pub skipped: Vec<LegacySkippedGate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyScopeInput {
    pub head_sha: String,
    pub diff_class: String,
    pub direct_crates: Vec<ScopedIdentity>,
    pub reverse_dependencies: Vec<ScopedIdentity>,
    pub architecture_wideners: Vec<ScopedIdentity>,
    pub risk_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyPlannedGate {
    pub name: String,
    pub role: LegacyPlanningRole,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacySkippedGate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<LegacyPlanningRole>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyPlanningRole {
    AlwaysOn,
    RustScoped,
    RustFallback,
    RustPackageScoped,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyGatePolicyInput {
    pub gates: Vec<LegacyGatePolicyRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyGatePolicyRow {
    pub name: String,
    pub tier: String,
    pub required: bool,
    pub command: String,
    pub timeout_seconds: u64,
    pub quarantine: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarantine_owner_issue: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarantine_review_after: Option<String>,
}
