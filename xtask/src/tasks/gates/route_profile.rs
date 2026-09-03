//! `ci_route_profile.v1` — typed profile expansion and the governed gate
//! denominator (#10178).
//!
//! One typed, versioned expansion contract that encodes the execution
//! planner's *actual* tier-inclusion semantics — the semantics `plan_gates`
//! implements through `gates_for_tier`/`MERGE_GATE_EXTRA_TIERS`/
//! `NIGHTLY_EXTRA_TIERS`/`extend_plan_with_non_pr_fast_static_gates` — as a
//! machine authority shared by planning, validation, explain output, and
//! tests:
//!
//! ```text
//! commit      -> commit
//! pr_fast     -> pr_fast
//! merge_gate  -> pr_fast + merge_gate
//! nightly     -> pr_fast + merge_gate + nightly
//!               (excludes commit and release by definition)
//! all         -> every tier present in the accepted policy
//! release     -> typed unsupported until a reviewed composition exists
//! ```
//!
//! Comparing the requested profile's string with each policy row's concrete
//! `tier` — the defect this authority removes — has three false outcomes:
//! `all` matches nothing, `merge_gate` loses its inherited `pr_fast` rows,
//! and `nightly` loses inherited `pr_fast`/`merge_gate` rows or accidentally
//! adopts display-only semantics.
//!
//! ## Not authority
//!
//! The display/list path (`filter_gates`, used by `--list`) has historically
//! different tier semantics — its `Nightly` arm keeps every gate for display
//! while execution excludes `commit` and `release`. Display filtering is not
//! authority for the routed denominator and is deliberately not consumed
//! here; the divergence is pinned by a test instead.
//!
//! ## Denominator invariants
//!
//! The denominator is constructed before selectors or planned outcomes are
//! applied, and #10176's lifecycle disposition may alter a row's outcome but
//! can never erase the row: every policy row in an included tier stays in
//! the denominator regardless of selected/skipped/quarantined/retired/
//! blocked state. Each canonical gate ID appears exactly once even when
//! multiple inclusion paths reach it. Outside-profile gates are excluded
//! with an accounted reason, never silently omitted. A single-gate request
//! is a separately identified narrowing over the expanded profile, not a
//! redefinition of profile membership.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use color_eyre::eyre::Result;
use sha2::{Digest, Sha256};

use super::{GatePolicy, GateTier};

/// Authority identity stamped into every explanation.
pub const AUTHORITY_NAME: &str = "ci_route_profile.v1";
/// Schema version of this expansion contract.
pub const SCHEMA_VERSION: u32 = 1;
/// Version of the machine-encoded expansion rule table below.
pub const EXPANSION_RULE_VERSION: u32 = 1;

/// Canonical ordering position for native tiers. Unknown (future) tiers sort
/// after every known tier, alphabetically.
fn tier_order(tier: &str) -> (u8, String) {
    match tier {
        "commit" => (0, tier.to_string()),
        "pr_fast" => (1, tier.to_string()),
        "merge_gate" => (2, tier.to_string()),
        "nightly" => (3, tier.to_string()),
        "release" => (4, tier.to_string()),
        other => (5, other.to_string()),
    }
}

/// The machine-encoded expansion rule table: requested profile -> included
/// native tiers, in canonical order. This array *is* the rule authority; its
/// digest is carried by every expansion so rule movement is detectable.
///
/// `nightly` deliberately excludes `commit` and `release`: it is a scheduled
/// deep-testing tier, not "every gate regardless of tier". `all` admits
/// every tier present in the accepted policy (computed at expansion time, so
/// a tier added later is automatically included); every other profile admits
/// exactly the tiers named here.
const EXPANSION_RULES: &[(RequestedProfile, &[&str])] = &[
    (RequestedProfile::Commit, &["commit"]),
    (RequestedProfile::PrFast, &["pr_fast"]),
    (RequestedProfile::MergeGate, &["pr_fast", "merge_gate"]),
    (RequestedProfile::Nightly, &["pr_fast", "merge_gate", "nightly"]),
];

// ---------------------------------------------------------------------------
// Typed vocabulary
// ---------------------------------------------------------------------------

/// A requested execution profile — an aggregate execution request, kept
/// distinct from any native policy tier string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestedProfile {
    Commit,
    PrFast,
    MergeGate,
    Nightly,
    All,
    /// Explicit release request. No reviewed composition exists yet, so it
    /// resolves typed `Unsupported` — never silently `all`, `merge_gate`, or
    /// tier-string equality.
    Release,
}

impl RequestedProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestedProfile::Commit => "commit",
            RequestedProfile::PrFast => "pr_fast",
            RequestedProfile::MergeGate => "merge_gate",
            RequestedProfile::Nightly => "nightly",
            RequestedProfile::All => "all",
            RequestedProfile::Release => "release",
        }
    }

    /// Parse a profile identity. Unknown identities fail closed (return
    /// `None`); the caller records them as invalid rather than guessing.
    #[allow(dead_code)] // consumer seam: #9148 reading plan inputs by name
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "commit" => Some(RequestedProfile::Commit),
            "pr_fast" => Some(RequestedProfile::PrFast),
            "merge_gate" => Some(RequestedProfile::MergeGate),
            "nightly" => Some(RequestedProfile::Nightly),
            "all" => Some(RequestedProfile::All),
            "release" => Some(RequestedProfile::Release),
            _ => None,
        }
    }

    /// Project the runner's CLI tier onto the typed profile vocabulary.
    /// `GateTier` has no release variant: release remains programmatically
    /// requestable only, resolving typed unsupported until its composition
    /// is reviewed.
    pub fn from_gate_tier(tier: &GateTier) -> Self {
        match tier {
            GateTier::Commit => RequestedProfile::Commit,
            GateTier::PrFast => RequestedProfile::PrFast,
            GateTier::MergeGate => RequestedProfile::MergeGate,
            GateTier::Nightly => RequestedProfile::Nightly,
            GateTier::All => RequestedProfile::All,
        }
    }

    fn included_native_tiers(self, policy_tiers: &[String]) -> Vec<String> {
        let mut tiers: Vec<String> = match self {
            RequestedProfile::All => policy_tiers.to_vec(),
            _ => EXPANSION_RULES
                .iter()
                .find(|(profile, _)| *profile == self)
                .map(|(_, tiers)| tiers.iter().map(|tier| (*tier).to_string()).collect())
                .unwrap_or_default(),
        };
        tiers.sort_by_key(|tier| tier_order(tier));
        tiers.dedup();
        tiers
    }

    /// Native tiers the profile excludes by definition, with the reason —
    /// recorded so exclusions are accounted rather than silent. Always in
    /// canonical tier order: `policy_tiers` comes from a `HashMap`, so the
    /// derived branch must not preserve its randomized iteration order into
    /// a digest/explanation that claims per-process determinism.
    fn excluded_native_tiers(self, policy_tiers: &[String]) -> Vec<ExcludedTier> {
        let mut excluded = match self {
            RequestedProfile::Nightly => vec![
                ExcludedTier {
                    tier: "commit".to_string(),
                    reason: "nightly is scheduled deep-testing; commit-tier hygiene is a \
                             pre-commit boundary"
                        .to_string(),
                },
                ExcludedTier {
                    tier: "release".to_string(),
                    reason: "release evidence is governed by the release authorities, not \
                             the nightly train"
                        .to_string(),
                },
            ],
            RequestedProfile::All => Vec::new(),
            _ => {
                let included = self.included_native_tiers(policy_tiers);
                policy_tiers
                    .iter()
                    .filter(|tier| !included.iter().any(|candidate| candidate == *tier))
                    .map(|tier| ExcludedTier {
                        tier: tier.clone(),
                        reason: format!("outside the {} profile by expansion rule", self.as_str()),
                    })
                    .collect()
            }
        };
        excluded.sort_by_key(|excluded| tier_order(&excluded.tier));
        excluded
    }
}

impl std::fmt::Display for RequestedProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A native policy tier excluded by the requested profile, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedTier {
    pub tier: String,
    pub reason: String,
}

/// An explicitly requested single-gate narrowing — separately identified so
/// it can never redefine aggregate profile membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateFilter {
    pub gate_id: String,
    pub reason: String,
}

/// Why a canonical gate is outside the denominator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusionReason {
    /// The gate's native tier is not included by the requested profile's
    /// expansion rule.
    OutsideProfileTier { tier: String },
    /// The gate is in the expanded profile but removed by the explicit
    /// single-gate narrowing.
    NarrowedByGateFilter,
}

impl ExclusionReason {
    fn as_str(&self) -> &'static str {
        match self {
            ExclusionReason::OutsideProfileTier { .. } => "outside-profile-tier",
            ExclusionReason::NarrowedByGateFilter => "narrowed-by-gate-filter",
        }
    }
}

/// A canonical gate excluded from the denominator, with its accounted reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedGate {
    pub gate_id: String,
    pub reason: ExclusionReason,
}

/// Whether the expansion is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionResolution {
    /// The governed denominator is complete for the requested profile.
    Complete,
    /// The profile is real but has no reviewed composition yet (release).
    Unsupported,
    /// An input identity is unknown or inconsistent; the denominator must
    /// not be consumed.
    Invalid,
}

impl ExpansionResolution {
    pub fn as_str(self) -> &'static str {
        match self {
            ExpansionResolution::Complete => "complete",
            ExpansionResolution::Unsupported => "unsupported",
            ExpansionResolution::Invalid => "invalid",
        }
    }
}

impl std::fmt::Display for ExpansionResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The typed profile-expansion result consumed before route rows are
/// compiled (#9148), by selectors (#9149), the producer audit (#9151), and
/// result/fan-in binding (#9156/#9159).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileExpansion {
    pub schema_version: u32,
    pub requested_profile: RequestedProfile,
    /// Included native tiers in canonical order.
    pub included_native_tiers: Vec<String>,
    /// Explicitly excluded native tiers with reasons where material.
    pub excluded_native_tiers: Vec<ExcludedTier>,
    /// Policy source identity: path, schema version, and semantic digest.
    pub policy_source_path: String,
    pub policy_schema_version: u32,
    pub policy_digest: String,
    /// Expansion-rule source identity.
    pub expansion_rule_version: u32,
    pub expansion_rule_digest: String,
    /// Optional exact single-gate narrowing, separately identified.
    pub gate_filter: Option<GateFilter>,
    /// Canonical governed gate IDs: sorted, each exactly once, constructed
    /// before selectors or planned outcomes.
    pub denominator: Vec<String>,
    /// Gates outside the denominator, with accounted reasons.
    pub excluded_gate_ids: Vec<ExcludedGate>,
    /// Unknown gate tiers, profiles, or filters that failed closed.
    pub unknown_identities: Vec<String>,
    pub resolution: ExpansionResolution,
    /// Deterministic identity over the semantic content: same inputs in any
    /// source order produce identical bytes; any movement changes them.
    pub semantic_fingerprint: String,
    /// Closed detail when the resolution is not `Complete`.
    pub detail: Option<String>,
}

impl ProfileExpansion {
    /// Bind a result or plan to this expansion's profile/denominator
    /// identity: a fingerprint from a narrower or differently expanded
    /// profile cannot satisfy the current plan (#9156/#9159).
    #[allow(dead_code)] // consumer seam: #9156 / #9159 fingerprint binding
    pub fn same_profile_identity(&self, other: &ProfileExpansion) -> bool {
        self.semantic_fingerprint == other.semantic_fingerprint
    }

    /// Human-readable explain output identifying the authority, the
    /// expansion rules, and the complete governed denominator.
    pub fn format_explanation(&self) -> String {
        let mut lines = vec![
            format!("{AUTHORITY_NAME} schema={}", self.schema_version),
            format!("profile={} resolution={}", self.requested_profile, self.resolution),
            format!("included_native_tiers={}", self.included_native_tiers.join(",")),
        ];
        if !self.policy_source_path.is_empty() {
            lines.push(format!("policy_source={}", self.policy_source_path));
        }
        if !self.excluded_native_tiers.is_empty() {
            lines.push(format!(
                "excluded_native_tiers={}",
                self.excluded_native_tiers
                    .iter()
                    .map(|excluded| excluded.tier.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if let Some(filter) = &self.gate_filter {
            lines.push(format!("gate_filter={} ({})", filter.gate_id, filter.reason));
        }
        lines.push(format!(
            "denominator({})={}",
            self.denominator.len(),
            self.denominator.join(",")
        ));
        if !self.excluded_gate_ids.is_empty() {
            lines.push(format!(
                "excluded_gates={}",
                self.excluded_gate_ids
                    .iter()
                    .map(|excluded| format!("{}[{}]", excluded.gate_id, excluded.reason.as_str()))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.unknown_identities.is_empty() {
            lines.push(format!("unknown_identities={}", self.unknown_identities.join(",")));
        }
        if let Some(detail) = &self.detail {
            lines.push(format!("detail={detail}"));
        }
        lines.push(format!("policy_digest={}", self.policy_digest));
        lines.push(format!(
            "expansion_rule_version={} digest={}",
            self.expansion_rule_version, self.expansion_rule_digest
        ));
        lines.push(format!("semantic_fingerprint={}", self.semantic_fingerprint));
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

/// Expand the requested profile over the accepted policy into the governed
/// denominator. Pure: same semantic inputs produce the same fingerprint in
/// any source order.
pub fn expand(
    policy: &GatePolicy,
    profile: RequestedProfile,
    gate_filter: Option<&str>,
) -> ProfileExpansion {
    let policy_tiers: Vec<String> = policy.tiers.keys().cloned().collect();
    let included_tiers = profile.included_native_tiers(&policy_tiers);
    let excluded_native_tiers = profile.excluded_native_tiers(&policy_tiers);

    let mut unknown_identities: Vec<String> = Vec::new();
    let mut resolution = ExpansionResolution::Complete;
    let mut detail: Option<String> = None;

    // Fail closed on duplicate gate rows: two rows claiming one identity
    // would double-count or split the governed denominator.
    let mut seen = BTreeSet::new();
    let mut duplicate_gate_ids = BTreeSet::new();
    for gate in &policy.gates {
        if !seen.insert(gate.name.clone()) {
            duplicate_gate_ids.insert(gate.name.clone());
        }
    }
    if !duplicate_gate_ids.is_empty() {
        resolution = ExpansionResolution::Invalid;
        detail = Some(format!(
            "duplicate gate rows claim one identity: {}",
            duplicate_gate_ids.iter().cloned().collect::<Vec<_>>().join(",")
        ));
    }

    // Every gate row must carry a declared native tier; an unknown tier is
    // never silently treated as an implicit member or non-member.
    for gate in &policy.gates {
        if !policy.tiers.contains_key(&gate.tier) {
            unknown_identities
                .push(format!("gate {} has undeclared tier {:?}", gate.name, gate.tier));
        }
    }
    if !unknown_identities.is_empty() {
        resolution = ExpansionResolution::Invalid;
        detail = Some(format!("unknown tier identities: {}", unknown_identities.join("; ")));
    }

    // Single pass over the policy: a gate is in the denominator exactly when
    // its native tier is included, regardless of lifecycle state. Set-keyed
    // so multiple inclusion paths can never duplicate an entry.
    let mut denominator: BTreeSet<String> = BTreeSet::new();
    let mut excluded_gate_ids: Vec<ExcludedGate> = Vec::new();
    let mut gate_tiers: BTreeMap<&str, &str> = BTreeMap::new();
    for gate in &policy.gates {
        gate_tiers.insert(gate.name.as_str(), gate.tier.as_str());
        if included_tiers.iter().any(|tier| tier == &gate.tier) {
            denominator.insert(gate.name.clone());
        } else {
            excluded_gate_ids.push(ExcludedGate {
                gate_id: gate.name.clone(),
                reason: ExclusionReason::OutsideProfileTier { tier: gate.tier.clone() },
            });
        }
    }
    excluded_gate_ids.sort_by(|left, right| left.gate_id.cmp(&right.gate_id));

    // The explicit single-gate narrowing applies AFTER expansion: it narrows
    // the already expanded profile and can never redefine membership. A
    // filter naming an outside-profile or unknown gate fails closed instead
    // of widening the profile.
    let gate_filter = gate_filter.map(|gate_id| GateFilter {
        gate_id: gate_id.to_string(),
        reason: "explicit single-gate narrowing over the expanded profile".to_string(),
    });
    if let Some(filter) = &gate_filter {
        match gate_tiers.get(filter.gate_id.as_str()) {
            None => {
                resolution = ExpansionResolution::Invalid;
                detail =
                    Some(format!("explicit gate filter names unknown gate {:?}", filter.gate_id));
            }
            Some(tier) => {
                if !denominator.contains(&filter.gate_id) {
                    resolution = ExpansionResolution::Invalid;
                    detail = Some(format!(
                        "explicit gate filter {:?} names a gate outside the {} profile (tier \
                         {tier:?}); a narrowing cannot redefine profile membership",
                        filter.gate_id, profile
                    ));
                } else {
                    let narrowed: BTreeSet<String> =
                        std::iter::once(filter.gate_id.clone()).collect();
                    for gate_id in denominator.difference(&narrowed) {
                        excluded_gate_ids.push(ExcludedGate {
                            gate_id: gate_id.clone(),
                            reason: ExclusionReason::NarrowedByGateFilter,
                        });
                    }
                    excluded_gate_ids.sort_by(|left, right| left.gate_id.cmp(&right.gate_id));
                    denominator = narrowed;
                }
            }
        }
    }

    // Release has no reviewed composition: typed unsupported, never an
    // alias of all/merge_gate and never tier-string equality.
    if profile == RequestedProfile::Release {
        resolution = ExpansionResolution::Unsupported;
        detail = Some(
            "release request profile has no reviewed composition; define it through the \
             release authorities before consuming a release denominator"
                .to_string(),
        );
    }

    let policy_digest = policy_digest(policy);
    let expansion_rule_digest = expansion_rule_digest(&included_tiers, &excluded_native_tiers);
    let semantic_fingerprint = semantic_fingerprint(
        profile,
        &included_tiers,
        &denominator,
        &gate_tiers,
        &excluded_gate_ids,
        gate_filter.as_ref(),
    );

    ProfileExpansion {
        schema_version: SCHEMA_VERSION,
        requested_profile: profile,
        included_native_tiers: included_tiers,
        excluded_native_tiers,
        policy_source_path: String::new(),
        policy_schema_version: policy.schema_version,
        policy_digest,
        expansion_rule_version: EXPANSION_RULE_VERSION,
        expansion_rule_digest,
        gate_filter,
        denominator: denominator.into_iter().collect(),
        excluded_gate_ids,
        unknown_identities,
        resolution,
        semantic_fingerprint,
        detail,
    }
}

/// Expand from the checked-in policy under `root`, recording the source path.
#[allow(dead_code)] // test/consumer seam: production explains expand the
// already-selected policy (honoring `--gate-policy`) and record its path
pub fn expand_from_root(
    root: &Path,
    profile: RequestedProfile,
    gate_filter: Option<&str>,
) -> Result<ProfileExpansion> {
    let policy_path = root.join(".ci/gate-policy.yaml");
    let policy = super::load_policy_for_inspection(&policy_path)?;
    let mut expansion = expand(&policy, profile, gate_filter);
    expansion.policy_source_path = policy_path.display().to_string();
    Ok(expansion)
}

/// Semantic digest over the canonical policy rows: sorted by gate id, so
/// source reordering cannot move it, while tier/role/quarantine movement does.
/// `short_circuit` is hashed too (#14409 review): it changes execution (a
/// failed required gate retires the remaining pr_fast plan), so two policies
/// that differ only in it must not publish the same identity.
fn policy_digest(policy: &GatePolicy) -> String {
    let mut rows: Vec<&super::GateDefinition> = policy.gates.iter().collect();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    let mut hasher = Sha256::new();
    for gate in rows {
        hasher.update(gate.name.as_bytes());
        hasher.update([0x1f]);
        hasher.update(gate.tier.as_bytes());
        hasher.update([0x1f]);
        hasher.update([if gate.required { b'r' } else { b'a' }]);
        hasher.update([0x1f]);
        hasher.update([if gate.quarantine { b'q' } else { b'.' }]);
        hasher.update([0x1f]);
        hasher.update([if gate.short_circuit { b's' } else { b'.' }]);
        hasher.update([0x1e]);
    }
    hex(&hasher.finalize())
}

/// Digest over the effective expansion rules: the profile's included tiers
/// and accounted exclusions. Rule movement changes this digest even when the
/// policy is untouched.
fn expansion_rule_digest(included: &[String], excluded: &[ExcludedTier]) -> String {
    let mut hasher = Sha256::new();
    for tier in included {
        hasher.update(tier.as_bytes());
        hasher.update([0x1f]);
    }
    hasher.update([0x1e]);
    for excluded_tier in excluded {
        hasher.update(excluded_tier.tier.as_bytes());
        hasher.update([0x1f]);
        hasher.update(excluded_tier.reason.as_bytes());
        hasher.update([0x1e]);
    }
    hex(&hasher.finalize())
}

/// Deterministic profile/denominator identity: requested profile, included
/// tiers, sorted denominator *with each gate's native tier assignment*,
/// accounted exclusions, and filter identity. Tier assignments are hashed so
/// a gate moving between two included tiers (e.g. `pr_fast` -> `merge_gate`,
/// which changes execution from scope-planned to static) cannot keep a stale
/// plan/result identity even though the denominator set is unchanged.
fn semantic_fingerprint(
    profile: RequestedProfile,
    included: &[String],
    denominator: &BTreeSet<String>,
    gate_tiers: &BTreeMap<&str, &str>,
    excluded: &[ExcludedGate],
    gate_filter: Option<&GateFilter>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(profile.as_str().as_bytes());
    hasher.update([0x1e]);
    for tier in included {
        hasher.update(tier.as_bytes());
        hasher.update([0x1f]);
    }
    hasher.update([0x1e]);
    for gate_id in denominator {
        hasher.update(gate_id.as_bytes());
        hasher.update([0x1f]);
        if let Some(tier) = gate_tiers.get(gate_id.as_str()) {
            hasher.update(tier.as_bytes());
        }
        hasher.update([0x1f]);
    }
    hasher.update([0x1e]);
    for excluded_gate in excluded {
        hasher.update(excluded_gate.gate_id.as_bytes());
        hasher.update([0x1f]);
        hasher.update(excluded_gate.reason.as_str().as_bytes());
        hasher.update([0x1f]);
        if let ExclusionReason::OutsideProfileTier { tier } = &excluded_gate.reason {
            hasher.update(tier.as_bytes());
        }
        hasher.update([0x1e]);
    }
    if let Some(filter) = gate_filter {
        hasher.update(filter.gate_id.as_bytes());
    }
    hex(&hasher.finalize())
}

/// Established repository hex encoding (per-byte `format!("{byte:02x}")`);
/// `Sha256::digest` does not implement `LowerHex` under the current
/// sha2/generic-array pair.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod route_profile_spec {
    use super::*;
    use crate::tasks::gates::{
        GateDefinition, GatePlanningConfig, GatePlanningRole, GatePolicy, GateRunnerConfig,
        GlobalSettings, OutputFormat, TierDefinition,
    };
    use std::collections::HashMap;

    fn gate(name: &str, tier: &str, quarantine: bool) -> GateDefinition {
        GateDefinition {
            name: name.to_string(),
            tier: tier.to_string(),
            description: name.to_string(),
            required: true,
            command: "true".to_string(),
            timeout_seconds: 30,
            retry_count: 0,
            budgets: None,
            quarantine,
            tags: Vec::new(),
            artifacts: Vec::new(),
            matrix: None,
            planning: Some(GatePlanningConfig {
                role: GatePlanningRole::Static,
                packages: Vec::new(),
            }),
            short_circuit: false,
        }
    }

    fn full_policy() -> GatePolicy {
        let mut tiers: HashMap<String, TierDefinition> = HashMap::new();
        for tier in ["commit", "pr_fast", "merge_gate", "nightly", "release"] {
            tiers.insert(
                tier.to_string(),
                TierDefinition {
                    description: tier.to_string(),
                    target_duration_seconds: 60,
                    enforcement: "none".to_string(),
                    trigger: Vec::new(),
                },
            );
        }
        GatePolicy {
            schema_version: 1,
            global: GlobalSettings {
                default_timeout_seconds: 30,
                artifact_retention_days: 0,
                default_retry_count: 0,
                environment: HashMap::new(),
                toolchain: None,
            },
            tiers,
            gates: vec![
                gate("staged_tree_identity", "commit", false),
                gate("fmt_check", "pr_fast", false),
                gate("clippy_scoped", "pr_fast", false),
                gate("policy_checks", "merge_gate", false),
                // Non-active lifecycle (quarantined) row: must remain in the
                // denominator (#10176 disposition may change its outcome,
                // never its membership).
                gate("security_audit", "merge_gate", true),
                gate("mutation", "nightly", true),
                gate("release_history", "release", false),
            ],
            flake_policy: None,
            audit: None,
        }
    }

    fn denominator_of(profile: RequestedProfile) -> Vec<String> {
        expand(&full_policy(), profile, None).denominator
    }

    // -------------------------------------------------------------------
    // Pinned expansion rows (controls 1-5)
    // -------------------------------------------------------------------

    #[test]
    fn commit_expands_to_exactly_the_commit_tier() {
        assert_eq!(
            denominator_of(RequestedProfile::Commit),
            vec!["staged_tree_identity".to_string()]
        );
    }

    #[test]
    fn pr_fast_expands_to_exactly_pr_fast() {
        assert_eq!(
            denominator_of(RequestedProfile::PrFast),
            vec!["clippy_scoped".to_string(), "fmt_check".to_string()]
        );
    }

    #[test]
    fn merge_gate_inherits_pr_fast_and_keeps_a_pr_fast_only_gate() {
        assert_eq!(
            denominator_of(RequestedProfile::MergeGate),
            vec![
                "clippy_scoped".to_string(),
                "fmt_check".to_string(),
                "policy_checks".to_string(),
                "security_audit".to_string(),
            ]
        );
        // The pr_fast-only gate is retained (control 2 falsifier).
        assert!(denominator_of(RequestedProfile::MergeGate).contains(&"fmt_check".to_string()));
    }

    #[test]
    fn nightly_inherits_pr_fast_and_merge_gate_and_excludes_commit_and_release() {
        assert_eq!(
            denominator_of(RequestedProfile::Nightly),
            vec![
                "clippy_scoped".to_string(),
                "fmt_check".to_string(),
                "mutation".to_string(),
                "policy_checks".to_string(),
                "security_audit".to_string(),
            ]
        );
        let expansion = expand(&full_policy(), RequestedProfile::Nightly, None);
        assert!(!expansion.denominator.contains(&"staged_tree_identity".to_string()));
        assert!(!expansion.denominator.contains(&"release_history".to_string()));
        let excluded_tiers: Vec<&str> =
            expansion.excluded_native_tiers.iter().map(|excluded| excluded.tier.as_str()).collect();
        assert!(excluded_tiers.contains(&"commit"));
        assert!(excluded_tiers.contains(&"release"));
    }

    #[test]
    fn all_includes_commit_and_release_and_every_policy_tier() {
        assert_eq!(
            denominator_of(RequestedProfile::All),
            vec![
                "clippy_scoped".to_string(),
                "fmt_check".to_string(),
                "mutation".to_string(),
                "policy_checks".to_string(),
                "release_history".to_string(),
                "security_audit".to_string(),
                "staged_tree_identity".to_string(),
            ]
        );
    }

    #[test]
    fn all_admits_a_newly_added_policy_tier_and_other_profiles_do_not() {
        // Control 5: a later policy tier is automatically included by `all`
        // and otherwise admitted only through an explicit profile rule.
        let mut policy = full_policy();
        policy.tiers.insert(
            "canary".to_string(),
            TierDefinition {
                description: "canary".to_string(),
                target_duration_seconds: 60,
                enforcement: "none".to_string(),
                trigger: Vec::new(),
            },
        );
        policy.gates.push(gate("canary_probe", "canary", false));

        let all = expand(&policy, RequestedProfile::All, None);
        assert!(all.denominator.contains(&"canary_probe".to_string()));
        assert_eq!(all.resolution, ExpansionResolution::Complete);

        let nightly = expand(&policy, RequestedProfile::Nightly, None);
        assert!(!nightly.denominator.contains(&"canary_probe".to_string()));
        let excluded: Vec<&ExcludedGate> = nightly
            .excluded_gate_ids
            .iter()
            .filter(|excluded| excluded.gate_id == "canary_probe")
            .collect();
        assert_eq!(excluded.len(), 1, "canary exclusion must be accounted, not silent");
        assert_eq!(
            excluded[0].reason,
            ExclusionReason::OutsideProfileTier { tier: "canary".to_string() }
        );
    }

    // -------------------------------------------------------------------
    // Mutation control: direct tier-string equality fails the contract
    // -------------------------------------------------------------------

    #[test]
    fn direct_tier_string_equality_yields_empty_or_partial_denominators() {
        // The #9148 draft-adapter defect class: comparing the requested
        // profile's string with each row's concrete tier. This test pins
        // what that implementation would actually produce so the typed
        // expansion's rows stay the authority.
        let policy = full_policy();
        for profile in
            [RequestedProfile::All, RequestedProfile::MergeGate, RequestedProfile::Nightly]
        {
            let equality_denominator: Vec<&str> = policy
                .gates
                .iter()
                .filter(|gate| gate.tier == profile.as_str())
                .map(|gate| gate.name.as_str())
                .collect();
            let typed = expand(&policy, profile, None);
            // `all` is not a policy tier: the equality denominator is empty
            // and governed rows disappear (control 1).
            // `merge_gate`/`nightly` lose their inherited pr_fast rows
            // (controls 2-3).
            assert!(
                !equality_denominator.contains(&"fmt_check"),
                "equality must lose the inherited pr_fast gate for {profile}"
            );
            assert!(typed.denominator.contains(&"fmt_check".to_string()));
            assert_ne!(equality_denominator.len(), typed.denominator.len());
        }
    }

    // -------------------------------------------------------------------
    // Denominator invariants (controls 6-8)
    // -------------------------------------------------------------------

    #[test]
    fn every_gate_appears_exactly_once_even_through_overlapping_paths() {
        let expansion = expand(&full_policy(), RequestedProfile::All, None);
        let unique: BTreeSet<&str> = expansion.denominator.iter().map(String::as_str).collect();
        assert_eq!(expansion.denominator.len(), unique.len());
        assert_eq!(expansion.denominator.len(), full_policy().gates.len());
    }

    #[test]
    fn non_active_lifecycle_rows_remain_in_the_denominator() {
        // Control 7: a skipped/quarantined/retired/blocked policy row cannot
        // disappear from the denominator (#10176 changes outcome, not
        // membership) — for every profile whose expansion includes its tier.
        for profile in
            [RequestedProfile::MergeGate, RequestedProfile::Nightly, RequestedProfile::All]
        {
            let expansion = expand(&full_policy(), profile, None);
            assert!(
                expansion.denominator.contains(&"security_audit".to_string()),
                "quarantined merge_gate row must remain in the {profile} denominator"
            );
        }
        for profile in [RequestedProfile::Nightly, RequestedProfile::All] {
            let expansion = expand(&full_policy(), profile, None);
            assert!(
                expansion.denominator.contains(&"mutation".to_string()),
                "quarantined nightly row must remain in the {profile} denominator"
            );
        }
        // The same quarantined nightly row stays OUTSIDE merge_gate's
        // denominator: lifecycle cannot widen the profile either, and the
        // exclusion is accounted (control 8).
        let merge_gate = expand(&full_policy(), RequestedProfile::MergeGate, None);
        assert!(!merge_gate.denominator.contains(&"mutation".to_string()));
        assert!(merge_gate.excluded_gate_ids.iter().any(|excluded| excluded.gate_id == "mutation"
            && excluded.reason
                == ExclusionReason::OutsideProfileTier { tier: "nightly".to_string() }));
    }

    #[test]
    fn outside_profile_gates_are_accounted_not_silently_omitted() {
        let expansion = expand(&full_policy(), RequestedProfile::PrFast, None);
        let excluded: BTreeMap<&str, &ExclusionReason> = expansion
            .excluded_gate_ids
            .iter()
            .map(|excluded| (excluded.gate_id.as_str(), &excluded.reason))
            .collect();
        assert_eq!(
            excluded.get("staged_tree_identity"),
            Some(&&ExclusionReason::OutsideProfileTier { tier: "commit".to_string() }),
        );
        assert_eq!(
            excluded.get("release_history"),
            Some(&&ExclusionReason::OutsideProfileTier { tier: "release".to_string() }),
        );
        // Every policy gate is either in the denominator or accounted.
        let accounted: BTreeSet<&str> = expansion
            .denominator
            .iter()
            .map(String::as_str)
            .chain(expansion.excluded_gate_ids.iter().map(|excluded| excluded.gate_id.as_str()))
            .collect();
        assert_eq!(accounted.len(), full_policy().gates.len());
    }

    // -------------------------------------------------------------------
    // Single-gate narrowing
    // -------------------------------------------------------------------

    #[test]
    fn gate_filter_narrows_the_expanded_profile_without_redefining_membership() {
        let expansion = expand(&full_policy(), RequestedProfile::MergeGate, Some("policy_checks"));
        assert_eq!(expansion.denominator, vec!["policy_checks".to_string()]);
        let filter = expansion.gate_filter.as_ref().unwrap();
        assert_eq!(filter.gate_id, "policy_checks");
        // The narrowing is separately identified, and the narrowed-out gates
        // are accounted with their own reason.
        let narrowed: Vec<&str> = expansion
            .excluded_gate_ids
            .iter()
            .filter(|excluded| excluded.reason == ExclusionReason::NarrowedByGateFilter)
            .map(|excluded| excluded.gate_id.as_str())
            .collect();
        assert!(narrowed.contains(&"fmt_check"));
        assert_eq!(expansion.resolution, ExpansionResolution::Complete);
    }

    #[test]
    fn gate_filter_naming_an_outside_profile_gate_fails_closed() {
        let expansion = expand(&full_policy(), RequestedProfile::PrFast, Some("release_history"));
        assert_eq!(expansion.resolution, ExpansionResolution::Invalid);
        assert!(expansion.detail.as_deref().unwrap().contains("outside the pr_fast profile"));
    }

    #[test]
    fn gate_filter_naming_an_unknown_gate_fails_closed() {
        let expansion = expand(&full_policy(), RequestedProfile::PrFast, Some("nonexistent"));
        assert_eq!(expansion.resolution, ExpansionResolution::Invalid);
        assert!(expansion.detail.as_deref().unwrap().contains("unknown gate"));
    }

    // -------------------------------------------------------------------
    // Unknown identities and release (controls 9-10)
    // -------------------------------------------------------------------

    #[test]
    fn undeclared_gate_tier_fails_closed() {
        let mut policy = full_policy();
        policy.gates.push(gate("orphan_tier", "ghost_tier", false));
        let expansion = expand(&policy, RequestedProfile::All, None);
        assert_eq!(expansion.resolution, ExpansionResolution::Invalid);
        assert!(expansion.unknown_identities.iter().any(|unknown| unknown.contains("orphan_tier")));
    }

    #[test]
    fn unknown_request_profile_fails_closed_at_parse() {
        assert!(RequestedProfile::parse("bogus").is_none());
        assert!(RequestedProfile::parse("all").is_some());
    }

    #[test]
    fn release_is_typed_unsupported_and_never_aliases_another_profile() {
        let expansion = expand(&full_policy(), RequestedProfile::Release, None);
        assert_eq!(expansion.resolution, ExpansionResolution::Unsupported);
        assert!(expansion.denominator.is_empty());
        assert!(expansion.detail.as_deref().unwrap().contains("no reviewed composition"));
        let all = expand(&full_policy(), RequestedProfile::All, None);
        let merge_gate = expand(&full_policy(), RequestedProfile::MergeGate, None);
        assert_ne!(expansion.semantic_fingerprint, all.semantic_fingerprint);
        assert_ne!(expansion.semantic_fingerprint, merge_gate.semantic_fingerprint);
    }

    #[test]
    fn duplicate_gate_rows_fail_closed() {
        let mut policy = full_policy();
        policy.gates.push(gate("fmt_check", "pr_fast", false));
        let expansion = expand(&policy, RequestedProfile::PrFast, None);
        assert_eq!(expansion.resolution, ExpansionResolution::Invalid);
        assert!(expansion.detail.as_deref().unwrap().contains("duplicate"));
    }

    // -------------------------------------------------------------------
    // Display-versus-execution divergence (control 11)
    // -------------------------------------------------------------------

    #[test]
    fn display_list_semantics_diverge_and_execution_denominator_stays_correct() {
        // `filter_gates` (the `--list` display path) keeps every gate for
        // `Nightly`; the execution denominator excludes commit and release.
        // Pin both facts so the divergence stays visible and the execution
        // contract stays the authority.
        let policy = full_policy();
        let display = super::super::filter_gates(
            &policy,
            &GateRunnerConfig {
                tier: GateTier::Nightly,
                output_format: OutputFormat::Human,
                ..GateRunnerConfig::default()
            },
        )
        .unwrap();
        let display_names: BTreeSet<&str> = display.iter().map(|gate| gate.name.as_str()).collect();
        assert!(display_names.contains("staged_tree_identity"));
        assert!(display_names.contains("release_history"));

        let execution = expand(&policy, RequestedProfile::Nightly, None);
        assert!(!execution.denominator.contains(&"staged_tree_identity".to_string()));
        assert!(!execution.denominator.contains(&"release_history".to_string()));
        assert!(execution.denominator.contains(&"fmt_check".to_string()));
    }

    // -------------------------------------------------------------------
    // Determinism and identity (controls 12-14)
    // -------------------------------------------------------------------

    #[test]
    fn reordered_policy_produces_identical_fingerprint_and_movement_changes_it() {
        let mut reordered = full_policy();
        reordered.gates.reverse();
        assert_eq!(
            expand(&full_policy(), RequestedProfile::Nightly, None).semantic_fingerprint,
            expand(&reordered, RequestedProfile::Nightly, None).semantic_fingerprint,
        );

        let mut moved = full_policy();
        moved.gates[0].tier = "pr_fast".to_string();
        assert_ne!(
            expand(&full_policy(), RequestedProfile::Commit, None).semantic_fingerprint,
            expand(&moved, RequestedProfile::Commit, None).semantic_fingerprint,
        );
        // Policy digest has the same order-independence / movement property.
        assert_eq!(policy_digest(&full_policy()), policy_digest(&reordered),);
        assert_ne!(policy_digest(&full_policy()), policy_digest(&moved));
        // `short_circuit` movement changes the published identity too (#14409
        // review): two policies that differ only in the field execute
        // differently — a failed required gate retires the remaining pr_fast
        // plan — so the digest must see the drift.
        let mut toggled = full_policy();
        toggled.gates[0].short_circuit = !toggled.gates[0].short_circuit;
        assert_ne!(policy_digest(&full_policy()), policy_digest(&toggled));
    }

    #[test]
    fn excluded_native_tiers_are_in_canonical_order_not_hashmap_order() {
        // Review finding: the derived exclusion branch preserved the policy
        // HashMap's randomized iteration order into the rule digest and
        // explanation, so separate processes could disagree about identity.
        // The exclusion list must come back in canonical tier order.
        let expansion = expand(&full_policy(), RequestedProfile::PrFast, None);
        let excluded: Vec<&str> =
            expansion.excluded_native_tiers.iter().map(|tier| tier.tier.as_str()).collect();
        assert_eq!(excluded, vec!["commit", "merge_gate", "nightly", "release"]);
        // The nightly exclusions stay in canonical order too.
        let nightly = expand(&full_policy(), RequestedProfile::Nightly, None);
        let nightly_excluded: Vec<&str> =
            nightly.excluded_native_tiers.iter().map(|tier| tier.tier.as_str()).collect();
        assert_eq!(nightly_excluded, vec!["commit", "release"]);
    }

    #[test]
    fn gate_tier_movement_between_included_tiers_changes_the_fingerprint() {
        // Review finding: the fingerprint previously hashed only the
        // denominator set, so a gate moving pr_fast -> merge_gate under a
        // merge_gate request kept the same identity even though execution
        // changes from scope-planned to static. Tier assignments are bound.
        let base = expand(&full_policy(), RequestedProfile::MergeGate, None);
        let mut moved = full_policy();
        let fmt = moved.gates.iter_mut().find(|gate| gate.name == "fmt_check").unwrap();
        fmt.tier = "merge_gate".to_string();
        let moved = expand(&moved, RequestedProfile::MergeGate, None);
        // Same denominator set, different gate->tier assignment.
        assert_eq!(base.denominator, moved.denominator);
        assert_ne!(base.semantic_fingerprint, moved.semantic_fingerprint);
        assert!(!base.same_profile_identity(&moved));
    }

    #[test]
    fn narrower_profile_identity_cannot_satisfy_the_expanded_one() {
        // Control 14: a result/plan produced against a narrower denominator
        // cannot validate against the expanded one.
        let merge_gate = expand(&full_policy(), RequestedProfile::MergeGate, None);
        let nightly = expand(&full_policy(), RequestedProfile::Nightly, None);
        assert!(!nightly.same_profile_identity(&merge_gate));
        assert!(!merge_gate.same_profile_identity(&nightly));
        // And the narrowed single-gate identity differs from its own profile.
        let narrowed = expand(&full_policy(), RequestedProfile::MergeGate, Some("policy_checks"));
        assert!(!narrowed.same_profile_identity(&merge_gate));
    }

    #[test]
    fn removing_one_inherited_gate_changes_the_denominator_identity() {
        // Handoff falsifier 5: a plan/result produced without one inherited
        // gate cannot validate against the complete expansion.
        let complete = expand(&full_policy(), RequestedProfile::MergeGate, None);
        let mut pruned = full_policy();
        pruned.gates.retain(|gate| gate.name != "fmt_check");
        let pruned = expand(&pruned, RequestedProfile::MergeGate, None);
        assert_ne!(
            complete.semantic_fingerprint, pruned.semantic_fingerprint,
            "removing an inherited pr_fast gate must move the denominator identity"
        );
        assert!(!complete.same_profile_identity(&pruned));
        assert!(complete.denominator.contains(&"fmt_check".to_string()));
        assert!(!pruned.denominator.contains(&"fmt_check".to_string()));
    }

    #[test]
    fn expansion_rule_digest_tracks_rule_movement() {
        let nightly = expand(&full_policy(), RequestedProfile::Nightly, None);
        let merge_gate = expand(&full_policy(), RequestedProfile::MergeGate, None);
        assert_ne!(nightly.expansion_rule_digest, merge_gate.expansion_rule_digest);
        assert_eq!(nightly.expansion_rule_version, EXPANSION_RULE_VERSION);
    }

    // -------------------------------------------------------------------
    // Explain output and checked-in policy
    // -------------------------------------------------------------------

    #[test]
    fn explain_output_names_authority_profile_and_denominator() {
        let expansion = expand(&full_policy(), RequestedProfile::MergeGate, None);
        let text = expansion.format_explanation();
        assert!(text.contains("ci_route_profile.v1"));
        assert!(text.contains("profile=merge_gate resolution=complete"));
        assert!(text.contains("included_native_tiers=pr_fast,merge_gate"));
        assert!(text.contains("denominator(4)="));
        assert!(text.contains("semantic_fingerprint="));
    }

    #[test]
    fn checked_in_policy_denominator_matches_the_execution_planner_semantics() {
        let root = crate::utils::project_root().unwrap();
        let expansion = expand_from_root(&root, RequestedProfile::Nightly, None).unwrap();
        assert_eq!(expansion.resolution, ExpansionResolution::Complete);
        // security_audit (merge_gate, quarantined) and the nightly
        // expensive gates stay in the nightly denominator; commit/release
        // rows stay out.
        assert!(expansion.denominator.contains(&"security_audit".to_string()));
        assert!(expansion.denominator.contains(&"mutation".to_string()));
        assert!(
            !expansion
                .denominator
                .iter()
                .any(|gate| policy_tier_of(&root, gate).as_deref() == Some("commit"))
        );
        assert!(
            !expansion
                .denominator
                .iter()
                .any(|gate| policy_tier_of(&root, gate).as_deref() == Some("release"))
        );
        assert_eq!(expansion.policy_schema_version, 1);
        assert!(expansion.policy_source_path.contains("gate-policy.yaml"));
    }

    fn policy_tier_of(root: &std::path::Path, gate_id: &str) -> Option<String> {
        let policy =
            crate::tasks::gates::load_policy_for_inspection(&root.join(".ci/gate-policy.yaml"))
                .unwrap();
        policy.gates.iter().find(|gate| gate.name == gate_id).map(|gate| gate.tier.clone())
    }
}
