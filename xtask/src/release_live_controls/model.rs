//! Typed model for `release_live_controls.v1` (#9403).
//!
//! Every type here is plain data. Nothing in this module reads GitHub, runs a
//! command, or mutates anything — [`super::live`] is the only place that
//! touches the network, and even there only through `gh api` GET reads.
//!
//! The central discipline is [`ObservationState`]: a control is either
//! [`ObservationState::Observed`] (the API affirmatively returned it),
//! [`ObservationState::Absent`] (the API affirmatively said it does not
//! exist), or [`ObservationState::NotProven`] (we could not establish which).
//! These three are never interchangeable, and nothing downstream may collapse
//! `Absent` and `NotProven` into each other or infer one from silence.

use serde::{Deserialize, Serialize};

pub const RELEASE_LIVE_CONTROLS_SCHEMA_VERSION: &str = "release_live_controls.v1";

/// Whether one control was affirmatively observed, affirmatively found
/// absent, or could not be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationState {
    /// The instrument read this control and it exists.
    Observed,
    /// The instrument read this control and the API affirmatively said it
    /// does not exist. Distinct from "we could not tell".
    Absent,
    /// The instrument could not establish this control either way —
    /// inaccessible, ambiguous, or simply not asked. Never a synonym for
    /// "no", and never inferred from an unrelated field.
    NotProven,
}

/// One observed value, or the reason it was not observed.
///
/// The pairing between `state` and `value` is an invariant `serde` alone
/// cannot enforce, because `value` must stay optional for the inconclusive
/// states: `OBSERVED` must carry `Some(value)`; `ABSENT` and `NOT_PROVEN`
/// must carry `None`. Construct through [`Self::observed`], [`Self::absent`],
/// or [`Self::not_proven`] to hold the pairing by construction, and run
/// [`Self::structural_problem`] on anything that arrived as external JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed<T> {
    pub state: ObservationState,
    // `default = "none"` rather than bare `default`: serde-derive's automatic
    // bound inference would otherwise require `T: Default` on the whole
    // generic impl even though `Option<T>` itself needs no such bound — a
    // known serde-derive limitation (serde-rs/serde#1728). An explicit
    // default function sidesteps it.
    #[serde(default = "none", skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default = "none", skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
}

fn none<X>() -> Option<X> {
    None
}

impl<T> Observed<T> {
    /// A value the instrument affirmatively read.
    // `observed` deliberately names the `OBSERVED` state, matching `absent`
    // and `not_proven` below; it is not an accidental self-named
    // constructor.
    #[allow(clippy::self_named_constructors)]
    pub fn observed(value: T) -> Self {
        Self { state: ObservationState::Observed, detail: None, value: Some(value) }
    }

    /// The API affirmatively said this control does not exist.
    pub fn absent(detail: impl Into<String>) -> Self {
        Self { state: ObservationState::Absent, detail: Some(detail.into()), value: None }
    }

    /// The control could not be established. Never a synonym for `absent`.
    pub fn not_proven(detail: impl Into<String>) -> Self {
        Self { state: ObservationState::NotProven, detail: Some(detail.into()), value: None }
    }

    pub fn is_observed(&self) -> bool {
        self.state == ObservationState::Observed
    }

    /// Whether this observation is settled either way — genuinely present or
    /// genuinely absent. `NOT_PROVEN` is the only inconclusive state.
    pub fn is_conclusive(&self) -> bool {
        matches!(self.state, ObservationState::Observed | ObservationState::Absent)
    }

    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Reject a row that claims `OBSERVED` with no value, or claims
    /// `ABSENT`/`NOT_PROVEN` while still carrying one.
    ///
    /// This is the guard against a hand-written or replayed JSON document
    /// smuggling `"state": "OBSERVED", "value": null` (or the reverse) past
    /// deserialization.
    pub fn structural_problem(&self, label: &str) -> Option<String> {
        match (self.state, self.value.is_some()) {
            (ObservationState::Observed, false) => {
                Some(format!("{label}: OBSERVED carries no value"))
            }
            (ObservationState::Absent, true) => {
                Some(format!("{label}: ABSENT must not carry a value"))
            }
            (ObservationState::NotProven, true) => {
                Some(format!("{label}: NOT_PROVEN must not carry a value"))
            }
            _ => None,
        }
    }
}

/// The repository and branch a caller asked to observe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySubject {
    pub owner: String,
    pub name: String,
    pub branch: String,
}

impl RepositorySubject {
    pub fn render(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// The repository identity the API actually returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryIdentity {
    pub full_name: String,
    pub node_id: String,
    pub database_id: u64,
    pub default_branch: String,
}

/// Whether the observed identity is the repository that was requested.
///
/// A payload naming a different repository (a redirect, an alias, a stale
/// cache) is [`Self::Mismatched`], never silently accepted as a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IdentityMatch {
    Matched,
    Mismatched { detail: String },
    NotProven { detail: String },
}

/// One required status-check context, as GitHub reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredContextRow {
    pub context: String,
    pub app_id: Option<u64>,
}

/// The classic `required_status_checks` block of branch protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredStatusChecks {
    pub strict: bool,
    pub contexts: Vec<RequiredContextRow>,
}

/// The classic `required_pull_request_reviews` block of branch protection.
///
/// Every field is `Option` on purpose: the API omitting a field means it did
/// not say, and an omitted field must never be read as `false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestReviewRule {
    pub required_approving_review_count: Option<u32>,
    pub dismiss_stale_reviews: Option<bool>,
    pub require_code_owner_reviews: Option<bool>,
    pub require_last_push_approval: Option<bool>,
}

/// Classic (non-ruleset) branch protection, as observed live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassicProtection {
    pub required_status_checks: Observed<RequiredStatusChecks>,
    pub enforce_admins: Observed<bool>,
    pub required_pull_request_reviews: Observed<PullRequestReviewRule>,
    pub required_conversation_resolution: Observed<bool>,
    pub restrictions_present: Observed<bool>,
}

impl ClassicProtection {
    fn structural_problem(&self, label: &str) -> Option<String> {
        self.required_status_checks
            .structural_problem(&format!("{label}.required_status_checks"))
            .or_else(|| self.enforce_admins.structural_problem(&format!("{label}.enforce_admins")))
            .or_else(|| {
                self.required_pull_request_reviews
                    .structural_problem(&format!("{label}.required_pull_request_reviews"))
            })
            .or_else(|| {
                self.required_conversation_resolution
                    .structural_problem(&format!("{label}.required_conversation_resolution"))
            })
            .or_else(|| {
                self.restrictions_present
                    .structural_problem(&format!("{label}.restrictions_present"))
            })
    }
}

/// One bypass actor named on a ruleset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BypassActor {
    pub actor_id: Option<u64>,
    pub actor_type: String,
    pub bypass_mode: String,
}

/// One rule carried by a ruleset (e.g. `required_status_checks`,
/// `pull_request`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesetRule {
    pub rule_type: String,
    pub required_contexts: Vec<String>,
    pub required_approving_review_count: Option<u32>,
    pub required_review_thread_resolution: Option<bool>,
    pub dismiss_stale_reviews_on_push: Option<bool>,
}

/// One ruleset, targeting either `branch` or `tag` refs.
///
/// `bypass_actors` is `NOT_PROVEN` when the ruleset payload omits the field
/// entirely, and `Observed(vec![])` only when the payload carries an
/// explicit empty array. An omitted bypass list must never read as "no
/// bypass" — that is exactly the gap a bypassable ruleset would hide in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ruleset {
    pub id: u64,
    pub name: String,
    pub target: String,
    /// GitHub's `enforcement` for the ruleset: `active`, `evaluate`, or
    /// `disabled`. Only `active` rulesets contribute to enforcement.
    pub enforcement: String,
    /// Whether the ruleset's `conditions.ref_name` include/exclude patterns
    /// select the requested branch. `Absent` for tag rulesets (no branch to
    /// apply to); `NOT_PROVEN` when the conditions could not be read or
    /// depend on a default branch that was not observed.
    pub applies_to_branch: Observed<bool>,
    pub bypass_actors: Observed<Vec<BypassActor>>,
    pub rules: Observed<Vec<RulesetRule>>,
}

impl Ruleset {
    /// Whether this ruleset is actually in force for the requested branch:
    /// `enforcement == "active"` and `applies_to_branch` observed `true`.
    ///
    /// A ruleset in `evaluate` or `disabled` mode, or one whose ref
    /// conditions exclude the branch, is recorded but must not inflate the
    /// required-contexts union.
    pub fn enforced_on_branch(&self) -> bool {
        self.enforcement == "active" && self.applies_to_branch.value() == Some(&true)
    }

    fn structural_problem(&self, label: &str) -> Option<String> {
        self.applies_to_branch
            .structural_problem(&format!("{label}.applies_to_branch"))
            .or_else(|| self.bypass_actors.structural_problem(&format!("{label}.bypass_actors")))
            .or_else(|| self.rules.structural_problem(&format!("{label}.rules")))
    }
}

/// One deployment-environment protection rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentProtectionRule {
    pub rule_type: String,
    pub wait_timer: Option<u64>,
    pub reviewer_count: Option<usize>,
    pub prevent_self_review: Option<bool>,
}

/// The environment's deployment-branch policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentBranchPolicy {
    pub protected_branches: bool,
    pub custom_branch_policies: bool,
}

/// One deployment environment.
///
/// Redaction law: this records counts, types, and names of environments and
/// rules — never secret values, and never environment secret *names*. Only
/// [`Self::secret_count`] is ever carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub protection_rules: Observed<Vec<EnvironmentProtectionRule>>,
    pub deployment_branch_policy: Observed<Option<DeploymentBranchPolicy>>,
    pub secret_count: Observed<usize>,
}

impl Environment {
    fn structural_problem(&self, label: &str) -> Option<String> {
        self.protection_rules
            .structural_problem(&format!("{label}.protection_rules"))
            .or_else(|| {
                self.deployment_branch_policy
                    .structural_problem(&format!("{label}.deployment_branch_policy"))
            })
            .or_else(|| self.secret_count.structural_problem(&format!("{label}.secret_count")))
    }
}

/// Repository-level release posture: immutability and whether tag rulesets
/// cover the release surface.
///
/// `immutable_releases` is `NOT_PROVEN` when the repository payload does not
/// carry the field — never `false`. A field the API never mentions is not
/// evidence that the setting is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasePosture {
    pub immutable_releases: Observed<bool>,
    pub tag_rulesets_present: Observed<bool>,
}

impl ReleasePosture {
    fn structural_problem(&self, label: &str) -> Option<String> {
        self.immutable_releases.structural_problem(&format!("{label}.immutable_releases")).or_else(
            || {
                self.tag_rulesets_present
                    .structural_problem(&format!("{label}.tag_rulesets_present"))
            },
        )
    }
}

/// One required-context name and the enforcement source(s) that carry it.
///
/// `sources` entries are `"branch_protection"` or `"ruleset:<id>"`, sorted
/// and deduplicated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnionContext {
    pub name: String,
    pub sources: Vec<String>,
}

/// The merged view of required contexts across classic branch protection and
/// branch rulesets.
///
/// Classic protection and rulesets are additive enforcement: GitHub applies
/// both simultaneously. See [`super::evaluate::required_contexts_union`] for
/// the law that computes this — it is `OBSERVED` only when both
/// contributing halves are conclusive, and never infers a missing half.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredContextsUnion {
    pub state: ObservationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub contexts: Vec<UnionContext>,
}

/// Everything observed for one requested repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryControls {
    pub requested: RepositorySubject,
    pub identity: Observed<RepositoryIdentity>,
    pub identity_match: IdentityMatch,
    pub classic_branch_protection: Observed<ClassicProtection>,
    pub branch_rulesets: Observed<Vec<Ruleset>>,
    pub tag_rulesets: Observed<Vec<Ruleset>>,
    pub environments: Observed<Vec<Environment>>,
    pub release_posture: ReleasePosture,
    pub required_contexts_union: RequiredContextsUnion,
}

impl RepositoryControls {
    /// The first structural violation found in this repository's rows, if
    /// any — see [`Observed::structural_problem`] for what counts as one.
    pub fn structural_problem(&self) -> Option<String> {
        let label = self.requested.render();
        self.identity
            .structural_problem(&format!("{label}.identity"))
            .or_else(|| {
                self.classic_branch_protection
                    .structural_problem(&format!("{label}.classic_branch_protection"))
            })
            .or_else(|| {
                self.classic_branch_protection.value().and_then(|v| {
                    v.structural_problem(&format!("{label}.classic_branch_protection"))
                })
            })
            .or_else(|| {
                ruleset_list_structural_problem(&self.branch_rulesets, &label, "branch_rulesets")
            })
            .or_else(|| ruleset_list_structural_problem(&self.tag_rulesets, &label, "tag_rulesets"))
            .or_else(|| self.environments.structural_problem(&format!("{label}.environments")))
            .or_else(|| {
                self.environments.value().and_then(|environments| {
                    environments.iter().find_map(|environment| {
                        environment.structural_problem(&format!(
                            "{label}.environments[{}]",
                            environment.name
                        ))
                    })
                })
            })
            .or_else(|| {
                self.release_posture.structural_problem(&format!("{label}.release_posture"))
            })
    }
}

fn ruleset_list_structural_problem(
    observed: &Observed<Vec<Ruleset>>,
    repository_label: &str,
    field: &str,
) -> Option<String> {
    observed.structural_problem(&format!("{repository_label}.{field}")).or_else(|| {
        observed.value().and_then(|rulesets| {
            rulesets.iter().find_map(|ruleset| {
                ruleset.structural_problem(&format!("{repository_label}.{field}[{}]", ruleset.id))
            })
        })
    })
}

/// Whether this receipt describes a live read or a replayed snapshot.
///
/// Set by the process that produced the receipt, never by the file itself:
/// [`super::load_snapshot`] forces this to [`Self::Snapshot`] regardless of
/// what a loaded document claims, so a replayed observation can never
/// represent itself as current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Currency {
    Live,
    Snapshot,
}

/// Whether the observing instrument (`gh`) itself was usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instrument {
    pub state: ObservationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gh_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Instrument {
    fn structural_problem(&self) -> Option<String> {
        match (self.state, self.gh_version.is_some()) {
            (ObservationState::Observed, false) => {
                Some("instrument: OBSERVED carries no gh_version".to_string())
            }
            (ObservationState::Absent, true) | (ObservationState::NotProven, true) => {
                Some("instrument: non-OBSERVED state must not carry a gh_version".to_string())
            }
            _ => None,
        }
    }
}

/// The overall verdict: whether every repository's every plane was
/// conclusively observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Observed,
    NotProven,
}

/// The complete `release_live_controls.v1` receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveControlsReceipt {
    pub schema_version: String,
    pub observed_at: String,
    pub currency: Currency,
    pub instrument: Instrument,
    pub repositories: Vec<RepositoryControls>,
    pub verdict: Verdict,
    pub limitations: Vec<String>,
}

impl LiveControlsReceipt {
    /// The first structural violation found anywhere in this receipt, if
    /// any. A hand-written or replayed document that pairs `OBSERVED` with a
    /// missing value (or `ABSENT`/`NOT_PROVEN` with a present one) is
    /// rejected here rather than trusted.
    pub fn structural_problem(&self) -> Option<String> {
        self.instrument
            .structural_problem()
            .or_else(|| self.repositories.iter().find_map(RepositoryControls::structural_problem))
    }
}
