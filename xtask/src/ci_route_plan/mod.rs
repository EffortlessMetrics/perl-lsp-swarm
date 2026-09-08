//! Typed route-plan domain (`ci_route_plan.v1`) for candidate-bound CI
//! planning.
//!
//! This module owns the closed typed domain payload and one cross-field
//! semantic validator. It does not discover changed files, select gates,
//! inspect workflow YAML, execute gates, observe GitHub enforcement, or
//! publish artifacts.
//!
//! Authorities consumed (never re-derived here):
//!
//! - `ci_route_profile.v1` (#10178): requested-profile expansion and the
//!   exact governed denominator, received through [`RouteProfileExpansionInput`].
//! - `gate_disposition.v1` (#10176): typed lifecycle/quarantine resolution,
//!   received through [`RouteDispositionInput`].
//! - PR #6858 policy role: the gate-row projection `required|advisory`,
//!   carried per gate by the disposition authority.
//!
//! The binary-side adapters in `tasks/gates/planning_types.rs` project the
//! resolved authority results into these input types. Canonical byte
//! encoding and the semantic fingerprint are owned by [`canonical`] (leaf
//! #10179); the checked-in JSON-Schema projection lives at
//! `.ci/schemas/ci-route-plan.v1.schema.json`; CLI/filesystem publication
//! lives in the `ci-route-plan` binary. This module still performs no
//! filesystem, clock, or network access.

mod canonical;
mod compile;
mod validate;

pub use canonical::{CanonicalPayload, FINGERPRINT_DOMAIN, SemanticProjection};

use canonical::deserialize_option_reject_null;
pub use validate::KNOWN_PROFILES;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Domain contract identity. The checked-in JSON-Schema projection of this
/// payload is owned by #10179, not this module.
pub const CI_ROUTE_PLAN_SCHEMA: &str = "ci_route_plan.v1";
pub const CI_ROUTE_PLAN_PRODUCER: &str = "xtask::ci_route_plan";

/// Closed `PlannedOutcome::Error` reason-code vocabulary. Unknown codes fail
/// validation instead of passing as free-form text.
pub const CLOSED_ERROR_CODES: &[&str] = &[
    "disposition_expired",
    "disposition_invalid",
    "lifecycle_non_runnable",
    "selector_evidence_missing",
    "selector_contradiction",
    "execution_identity_missing",
    "run_timeout_invalid",
];

/// One compiled route plan: exactly one row per governed denominator gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiRoutePlanV1 {
    pub schema: String,
    pub producer: String,
    pub subject: RouteSubjectRef,
    /// Requested execution profile name, projected from #10178.
    pub requested_profile: String,
    /// Native tiers included by the profile expansion, projected from #10178.
    pub included_native_tiers: Vec<String>,
    /// #10178 profile/denominator identity (authority-computed).
    pub expansion_fingerprint: String,
    /// Policy semantic digest, projected from #10178 (authority-computed).
    pub policy_digest: String,
    /// #10176 disposition semantic digest (authority-computed).
    pub disposition_digest: String,
    pub workflow_digest: String,
    /// Governed gate ids from #10178: sorted, unique, exactly one row each.
    pub denominator: Vec<String>,
    pub selection: RouteSelectionEvidence,
    pub rows: Vec<RoutePlanRow>,
    pub summary: RoutePlanSummary,
    /// Domain-separated SHA-256 fingerprint of the canonical semantic
    /// projection (#10179): `SHA-256("ci_route_plan.v1\0" || bytes)`.
    /// Recomputed and compared at validation; never part of its own
    /// preimage.
    pub semantic_fingerprint: String,
}

impl CiRoutePlanV1 {
    /// Compile the typed domain payload from projected authority results and
    /// runner adapter inputs. Pure: no filesystem, clock, or network access.
    /// The compiled plan carries its semantic fingerprint, computed over
    /// the canonical semantic projection.
    pub fn compile(input: CompileRoutePlanInput) -> Result<Self, String> {
        compile::compile(input)
    }

    /// Validate every cross-field invariant of the compiled payload,
    /// including that `semantic_fingerprint` equals the recomputed digest
    /// of the canonical semantic projection.
    pub fn validate(&self) -> Result<(), String> {
        validate::validate(self)
    }
}

/// Opaque subject reference supplied by the exact-subject authority (#8042).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSubjectRef {
    pub kind: String,
    pub head_sha: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub base_sha: Option<String>,
    /// Digest of the complete subject payload supplied by its owner.
    pub subject_digest: String,
}

/// Selection evidence from the runner's scope seam, adapted from
/// `ci_scope::ScopeOutput`. Fallback is recorded honestly: a fallback
/// selection carries no positive selector proof anywhere in this payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSelectionEvidence {
    pub base: String,
    pub scope_ok: bool,
    pub fallback_used: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub fallback_reason: Option<String>,
    pub package_args: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub scope: Option<RouteScopeEvidence>,
    /// #9149 selector-payload identity. Required whenever any row claims
    /// selector-proved non-applicability.
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

/// One governed gate's planned row. Lifecycle, policy role, selector
/// placement/applicability, and planned outcome stay independent facts;
/// the outcome is composed from them, never substituted for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePlanRow {
    pub gate_id: String,
    /// The gate row's native policy tier (#10176 `intended_profile`).
    pub native_tier: String,
    /// Repository merge-policy role projected from the #6858/#10176
    /// authority. Never derived from the requested profile.
    pub policy_role: PolicyRole,
    /// Lifecycle disposition projected from #10176.
    pub lifecycle: LifecycleDisposition,
    /// Runner planning role for this gate on this subject.
    pub selector_role: SelectorRole,
    /// The runner population the adapter observed for this gate.
    pub selector_placement: SelectorPlacement,
    /// Positive selector proof state. `Unknown` means no positive proof.
    pub applicability: Applicability,
    pub outcome: PlannedOutcome,
}

/// Gate-row projection of the accepted policy role vocabulary. `informational`
/// and `local` exist in the #6858 status-context surface
/// (`.ci/policies/required-checks.toml`); no gate row carries them today, so
/// the honest gate-row projection is `required|advisory` only (per the
/// #10176 authority) and this enum does not fabricate the unreachable
/// variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRole {
    Required,
    Advisory,
}

impl PolicyRole {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyRole::Required => "required",
            PolicyRole::Advisory => "advisory",
        }
    }
}

/// Lifecycle disposition, projected 1:1 from `gate_disposition.v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleDisposition {
    pub state: LifecycleState,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Active,
    Dormant,
    Quarantined,
    Retired,
    Blocked,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleState::Active => "active",
            LifecycleState::Dormant => "dormant",
            LifecycleState::Quarantined => "quarantined",
            LifecycleState::Retired => "retired",
            LifecycleState::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Current,
    Expired,
    Invalid,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Resolution::Current => "current",
            Resolution::Expired => "expired",
            Resolution::Invalid => "invalid",
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorPlacement {
    Selected,
    Skipped,
}

impl SelectorPlacement {
    pub fn as_str(self) -> &'static str {
        match self {
            SelectorPlacement::Selected => "selected",
            SelectorPlacement::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    Applicable,
    NotApplicable,
    Unknown,
}

/// Planned outcome for one governed gate. `run` and `scoped_noop` both
/// require positive selector proof; lifecycle authority gates
/// `quarantined`; every other non-runnable or action-required state is a
/// typed `error` row that remains visible in the denominator.
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
        owner: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_option_reject_null"
        )]
        owner_issue: Option<String>,
        /// Review horizon (`YYYY-MM-DD`) from #10176 quarantine evidence.
        review_after: String,
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

// ---------------------------------------------------------------------------
// Compile inputs (projected authority results + runner adapter inputs)
// ---------------------------------------------------------------------------

/// Compile input for the `ci-route-plan` CLI handoff: the binary-side
/// adapters build this from resolved authority results and the runner's
/// planning facts; the CLI deserializes exactly this shape (unknown fields
/// fail closed, matching the rest of the domain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileRoutePlanInput {
    pub subject: RouteSubjectRef,
    /// #10178 expansion result, projected by the binary-side adapter.
    pub expansion: RouteProfileExpansionInput,
    /// #10176 resolved dispositions, projected 1:1 by the binary-side
    /// adapter. Must cover every denominator gate.
    pub dispositions: Vec<RouteDispositionInput>,
    /// #10176 authority semantic digest.
    pub disposition_digest: String,
    pub workflow_digest: String,
    /// Runner gate populations projected to typed selector inputs.
    pub selectors: Vec<GateSelectorInput>,
    pub selection: RouteSelectionEvidence,
    /// Executable identity per gate, projected from the gate policy.
    pub execution: Vec<RouteExecutionIdentity>,
}

/// 1:1 projection of a `ci_route_profile.v1` expansion result. Only a
/// `Complete` expansion may carry a denominator; the adapter refuses to
/// project unsupported/invalid expansions and compile fails closed on any
/// non-complete status that reaches it anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteProfileExpansionInput {
    pub requested_profile: String,
    pub included_native_tiers: Vec<String>,
    pub semantic_fingerprint: String,
    pub policy_digest: String,
    pub denominator: Vec<String>,
    pub resolution: ExpansionStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpansionStatus {
    Complete,
    Unsupported,
    Invalid,
}

impl ExpansionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ExpansionStatus::Complete => "complete",
            ExpansionStatus::Unsupported => "unsupported",
            ExpansionStatus::Invalid => "invalid",
        }
    }
}

/// 1:1 projection of one `gate_disposition.v1` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDispositionInput {
    pub gate_id: String,
    pub policy_role: PolicyRole,
    pub lifecycle: LifecycleDisposition,
    /// The gate row's native policy tier.
    pub native_tier: String,
    /// Present exactly when the lifecycle claim is `Quarantined` with
    /// complete current evidence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub quarantine: Option<RouteQuarantineEvidence>,
    /// Closed error detail when the resolution is not `Current`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteQuarantineEvidence {
    pub owner: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub owner_issue: Option<String>,
    pub reason_token: String,
    /// Review horizon (`YYYY-MM-DD`).
    pub review_after: String,
}

/// One gate's runner-observed selector facts. `proof` is `Some` only when a
/// positive exact-subject selector proof exists (#9149 adapter seam; today
/// the scope selector supplies it and fallback decisions supply nothing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateSelectorInput {
    pub gate_id: String,
    pub placement: SelectorPlacement,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub role: Option<SelectorRole>,
    pub reason: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_option_reject_null"
    )]
    pub proof: Option<SelectorProof>,
}

/// Positive exact-subject selector proof, projected 1:1 from the #10176
/// planner-seam `SelectorEvidence` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorProof {
    Applicable,
    NotApplicableToSubject,
}

/// Executable identity for one gate, projected from the gate policy row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteExecutionIdentity {
    pub gate_id: String,
    pub command: String,
    pub timeout_seconds: u64,
}
