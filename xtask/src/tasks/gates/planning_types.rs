use std::collections::HashSet;

use crate::tasks::ci_scope::ScopeOutput;
use xtask::ci_route_plan::{
    LegacyGatePlanInput, LegacyGatePolicyInput, LegacyGatePolicyRow, LegacyPlannedGate,
    LegacyPlanningRole, LegacyScopeInput, LegacySkippedGate, ScopedIdentity,
};

use super::{GateDefinition, GatePlanningRole, GatePolicy, GateTier};

#[derive(Debug, Clone)]
pub(super) struct GatePlan {
    pub(super) tier: GateTier,
    pub(super) base: String,
    pub(super) scope: Option<ScopeOutput>,
    pub(super) scope_ok: bool,
    pub(super) fallback_used: bool,
    pub(super) fallback_reason: Option<String>,
    pub(super) package_args: Vec<String>,
    pub(super) selected: Vec<PlannedGate>,
    pub(super) skipped: Vec<SkippedGate>,
    /// `git write-tree` OID of the staged tree this plan was built against
    /// (issue #3786). `Some` only when the run was invoked with `--staged`;
    /// carried through to `AgentReceipt.staged_tree_oid` in the action
    /// packet.
    pub(super) staged_tree_oid: Option<String>,
}

impl From<&GatePlan> for LegacyGatePlanInput {
    fn from(plan: &GatePlan) -> Self {
        let scope = plan.scope.as_ref().map(|scope| LegacyScopeInput {
            head_sha: scope.head_sha.clone(),
            diff_class: scope.diff_class.clone(),
            direct_crates: scope
                .direct_crates
                .iter()
                .map(|row| ScopedIdentity {
                    name: row.name.clone(),
                    reason: row.reason.clone(),
                })
                .collect(),
            reverse_dependencies: scope
                .reverse_dep_closure
                .iter()
                .map(|row| ScopedIdentity {
                    name: row.name.clone(),
                    reason: row.reason.clone(),
                })
                .collect(),
            architecture_wideners: scope
                .architecture_wideners
                .iter()
                .map(|row| ScopedIdentity {
                    name: row.name.clone(),
                    reason: row.rule.clone(),
                })
                .collect(),
            risk_tags: scope.risk_tags.clone(),
        });
        Self {
            tier: plan.tier.to_string(),
            base: plan.base.clone(),
            scope_ok: plan.scope_ok,
            fallback_used: plan.fallback_used,
            fallback_reason: plan.fallback_reason.clone(),
            package_args: plan.package_args.clone(),
            scope,
            selected: plan
                .selected
                .iter()
                .map(|row| LegacyPlannedGate {
                    name: row.gate.name.clone(),
                    role: row.role.into(),
                    reason: row.reason.clone(),
                })
                .collect(),
            skipped: plan
                .skipped
                .iter()
                .map(|row| LegacySkippedGate {
                    name: row.name.clone(),
                    role: row.role.map(Into::into),
                    reason: row.reason.clone(),
                })
                .collect(),
        }
    }
}

impl From<&GatePolicy> for LegacyGatePolicyInput {
    fn from(policy: &GatePolicy) -> Self {
        Self {
            gates: policy
                .gates
                .iter()
                .map(|gate| LegacyGatePolicyRow {
                    name: gate.name.clone(),
                    tier: gate.tier.clone(),
                    required: gate.required,
                    command: gate.command.clone(),
                    timeout_seconds: gate.timeout_seconds,
                    quarantine: gate.quarantine,
                    // The current policy does not yet carry structured
                    // quarantine ownership. Compilation preserves this as an
                    // explicit error row instead of guessing.
                    quarantine_owner_issue: None,
                    quarantine_review_after: None,
                })
                .collect(),
        }
    }
}

impl From<GatePlanningRole> for LegacyPlanningRole {
    fn from(role: GatePlanningRole) -> Self {
        match role {
            GatePlanningRole::AlwaysOn => Self::AlwaysOn,
            GatePlanningRole::RustScoped => Self::RustScoped,
            GatePlanningRole::RustFallback => Self::RustFallback,
            GatePlanningRole::RustPackageScoped => Self::RustPackageScoped,
            GatePlanningRole::Static => Self::Static,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PlannedGate {
    pub(super) gate: GateDefinition,
    pub(super) role: GatePlanningRole,
    pub(super) reason: String,
}

#[derive(Debug, Clone)]
pub(super) struct SkippedGate {
    pub(super) name: String,
    pub(super) role: Option<GatePlanningRole>,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PackageTargetIndex {
    pub(super) lib_packages: HashSet<String>,
}

impl PackageTargetIndex {
    pub(super) fn has_lib(&self, package: &str) -> bool {
        self.lib_packages.contains(package)
    }
}
