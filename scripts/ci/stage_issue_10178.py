#!/usr/bin/env python3
"""Build and publish the clean #10178 candidate from current main.

This helper and its workflow live only on the construction branch. The clean
candidate branch contains only the production Rust contract and documentation.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import urllib.error
import urllib.request
from pathlib import Path

BASE_SHA = "10be0eceeee4ba27c18545a1cb75771a1057fcb9"
TARGET_BRANCH = "codex/ci-route-profile-denominator"
CHANGED_PATHS = [
    "xtask/src/tasks/gates.rs",
    "xtask/src/tasks/gates/route_profile.rs",
    "docs/ci/route-profile-denominator.md",
]

ROUTE_PROFILE_SOURCE = r'''//! Requested CI profile expansion and governed gate denominator.
//!
//! This module owns only the mapping from a requested execution profile to
//! native policy tiers and the exact gate population governed by that profile.
//! Lifecycle disposition, exact-subject applicability, planned outcomes,
//! execution results, and live GitHub enforcement remain separate authorities.

use super::{GatePolicy, GateTier};
use color_eyre::eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};

pub(crate) const ROUTE_PROFILE_SCHEMA_VERSION: u32 = 1;
const EXPANSION_RULES_V1: &str = concat!(
    "commit=commit;",
    "pr_fast=pr_fast;",
    "merge_gate=pr_fast,merge_gate;",
    "nightly=pr_fast,merge_gate,nightly;",
    "all=all_policy_tiers;",
    "release=not_proven_until_reviewed"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestedRouteProfile {
    Commit,
    PrFast,
    MergeGate,
    Nightly,
    All,
    Release,
}

impl From<&GateTier> for RequestedRouteProfile {
    fn from(value: &GateTier) -> Self {
        match value {
            GateTier::Commit => Self::Commit,
            GateTier::PrFast => Self::PrFast,
            GateTier::MergeGate => Self::MergeGate,
            GateTier::Nightly => Self::Nightly,
            GateTier::All => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RouteProfileStatus {
    Supported,
    NotProven,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TierExclusion {
    pub native_tier: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteProfileDenominator {
    pub schema_version: u32,
    pub requested_profile: RequestedRouteProfile,
    pub status: RouteProfileStatus,
    pub included_native_tiers: Vec<String>,
    pub excluded_native_tiers: Vec<TierExclusion>,
    pub exact_gate_filter: Option<String>,
    /// Complete profile membership before an optional one-gate narrowing.
    pub profile_gate_ids: Vec<String>,
    /// Exact denominator after the separately identified narrowing operation.
    pub governed_gate_ids: Vec<String>,
    pub filtered_out_gate_ids: Vec<String>,
    pub outside_profile_gate_ids: Vec<String>,
    pub policy_denominator_sha256: String,
    pub expansion_rules_sha256: String,
    pub semantic_sha256: String,
    pub limitations: Vec<String>,
}

#[derive(Serialize)]
struct PolicyGateIdentity<'a> {
    id: &'a str,
    native_tier: &'a str,
}

#[derive(Serialize)]
struct PolicyDenominatorIdentity<'a> {
    schema_version: u32,
    native_tiers: &'a [String],
    gates: &'a [PolicyGateIdentity<'a>],
}

#[derive(Serialize)]
struct SemanticIdentity<'a> {
    schema_version: u32,
    requested_profile: RequestedRouteProfile,
    status: RouteProfileStatus,
    included_native_tiers: &'a [String],
    excluded_native_tiers: &'a [TierExclusion],
    exact_gate_filter: &'a Option<String>,
    profile_gate_ids: &'a [String],
    governed_gate_ids: &'a [String],
    filtered_out_gate_ids: &'a [String],
    outside_profile_gate_ids: &'a [String],
    policy_denominator_sha256: &'a str,
    expansion_rules_sha256: &'a str,
    limitations: &'a [String],
}

pub(crate) fn expand_gate_tier(
    policy: &GatePolicy,
    tier: &GateTier,
    exact_gate_filter: Option<&str>,
) -> Result<RouteProfileDenominator> {
    expand_route_profile(policy, RequestedRouteProfile::from(tier), exact_gate_filter)
}

pub(crate) fn profile_includes_native_tier(
    policy: &GatePolicy,
    tier: &GateTier,
    native_tier: &str,
) -> Result<bool> {
    let expansion = expand_gate_tier(policy, tier, None)?;
    if expansion.status != RouteProfileStatus::Supported {
        bail!(
            "requested profile {:?} is not supported: {:?}",
            expansion.requested_profile,
            expansion.limitations
        );
    }
    Ok(expansion.included_native_tiers.iter().any(|candidate| candidate == native_tier))
}

pub(crate) fn expand_route_profile(
    policy: &GatePolicy,
    requested_profile: RequestedRouteProfile,
    exact_gate_filter: Option<&str>,
) -> Result<RouteProfileDenominator> {
    validate_policy_identity(policy)?;

    let native_tiers = canonical_native_tiers(policy);
    let policy_denominator_sha256 = policy_denominator_sha256(policy, &native_tiers)?;
    let expansion_rules_sha256 = sha256_hex(EXPANSION_RULES_V1.as_bytes());

    if requested_profile == RequestedRouteProfile::Release {
        let limitations = vec!["release_profile_composition_not_reviewed".to_string()];
        let excluded_native_tiers = native_tiers
            .iter()
            .map(|tier| TierExclusion {
                native_tier: tier.clone(),
                reason: "release_profile_not_proven".to_string(),
            })
            .collect::<Vec<_>>();
        let outside_profile_gate_ids = sorted_gate_ids(policy.gates.iter().map(|gate| gate.name.as_str()));
        return finish(RouteProfileDenominator {
            schema_version: ROUTE_PROFILE_SCHEMA_VERSION,
            requested_profile,
            status: RouteProfileStatus::NotProven,
            included_native_tiers: Vec::new(),
            excluded_native_tiers,
            exact_gate_filter: exact_gate_filter.map(str::to_string),
            profile_gate_ids: Vec::new(),
            governed_gate_ids: Vec::new(),
            filtered_out_gate_ids: Vec::new(),
            outside_profile_gate_ids,
            policy_denominator_sha256,
            expansion_rules_sha256,
            semantic_sha256: String::new(),
            limitations,
        });
    }

    let included_native_tiers = included_tiers(&native_tiers, requested_profile)?;
    let included_set: HashSet<&str> = included_native_tiers.iter().map(String::as_str).collect();
    let excluded_native_tiers = native_tiers
        .iter()
        .filter(|tier| !included_set.contains(tier.as_str()))
        .map(|tier| TierExclusion {
            native_tier: tier.clone(),
            reason: "outside_requested_profile".to_string(),
        })
        .collect::<Vec<_>>();

    let profile_gate_ids = sorted_gate_ids(
        policy
            .gates
            .iter()
            .filter(|gate| included_set.contains(gate.tier.as_str()))
            .map(|gate| gate.name.as_str()),
    );
    let outside_profile_gate_ids = sorted_gate_ids(
        policy
            .gates
            .iter()
            .filter(|gate| !included_set.contains(gate.tier.as_str()))
            .map(|gate| gate.name.as_str()),
    );

    let (governed_gate_ids, filtered_out_gate_ids, exact_gate_filter) =
        apply_exact_gate_filter(policy, &profile_gate_ids, exact_gate_filter)?;

    finish(RouteProfileDenominator {
        schema_version: ROUTE_PROFILE_SCHEMA_VERSION,
        requested_profile,
        status: RouteProfileStatus::Supported,
        included_native_tiers,
        excluded_native_tiers,
        exact_gate_filter,
        profile_gate_ids,
        governed_gate_ids,
        filtered_out_gate_ids,
        outside_profile_gate_ids,
        policy_denominator_sha256,
        expansion_rules_sha256,
        semantic_sha256: String::new(),
        limitations: Vec::new(),
    })
}

fn validate_policy_identity(policy: &GatePolicy) -> Result<()> {
    let declared_tiers: HashSet<&str> = policy.tiers.keys().map(String::as_str).collect();
    let mut gate_ids = HashSet::new();
    for gate in &policy.gates {
        if !declared_tiers.contains(gate.tier.as_str()) {
            bail!(
                "gate '{}' references native tier '{}' absent from policy.tiers",
                gate.name,
                gate.tier
            );
        }
        if !gate_ids.insert(gate.name.as_str()) {
            bail!("duplicate governed gate identity '{}'", gate.name);
        }
    }
    Ok(())
}

fn canonical_native_tiers(policy: &GatePolicy) -> Vec<String> {
    let tiers: BTreeSet<String> = policy
        .tiers
        .keys()
        .cloned()
        .chain(policy.gates.iter().map(|gate| gate.tier.clone()))
        .collect();
    let mut tiers = tiers.into_iter().collect::<Vec<_>>();
    tiers.sort_by(|left, right| {
        tier_priority(left)
            .cmp(&tier_priority(right))
            .then_with(|| left.cmp(right))
    });
    tiers
}

fn tier_priority(tier: &str) -> u8 {
    match tier {
        "commit" => 0,
        "pr_fast" => 1,
        "merge_gate" => 2,
        "nightly" => 3,
        "release" => 4,
        _ => 5,
    }
}

fn included_tiers(
    native_tiers: &[String],
    requested_profile: RequestedRouteProfile,
) -> Result<Vec<String>> {
    let requested = match requested_profile {
        RequestedRouteProfile::Commit => vec!["commit"],
        RequestedRouteProfile::PrFast => vec!["pr_fast"],
        RequestedRouteProfile::MergeGate => vec!["pr_fast", "merge_gate"],
        RequestedRouteProfile::Nightly => vec!["pr_fast", "merge_gate", "nightly"],
        RequestedRouteProfile::All => return Ok(native_tiers.to_vec()),
        RequestedRouteProfile::Release => {
            bail!("release profile must be handled as typed not-proven")
        }
    };

    let available: HashSet<&str> = native_tiers.iter().map(String::as_str).collect();
    let missing = requested
        .iter()
        .copied()
        .filter(|tier| !available.contains(*tier))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "requested profile {:?} requires missing native tiers: {:?}",
            requested_profile,
            missing
        );
    }
    Ok(requested.into_iter().map(str::to_string).collect())
}

fn apply_exact_gate_filter(
    policy: &GatePolicy,
    profile_gate_ids: &[String],
    exact_gate_filter: Option<&str>,
) -> Result<(Vec<String>, Vec<String>, Option<String>)> {
    let Some(filter) = exact_gate_filter else {
        return Ok((profile_gate_ids.to_vec(), Vec::new(), None));
    };
    if filter.trim().is_empty() {
        bail!("exact gate filter must be a non-empty stable gate identity");
    }
    if !policy.gates.iter().any(|gate| gate.name == filter) {
        bail!("exact gate filter '{}' names no governed policy gate", filter);
    }
    if !profile_gate_ids.iter().any(|gate| gate == filter) {
        bail!(
            "exact gate filter '{}' is outside the requested profile denominator",
            filter
        );
    }
    let filtered_out_gate_ids = profile_gate_ids
        .iter()
        .filter(|gate| gate.as_str() != filter)
        .cloned()
        .collect();
    Ok((vec![filter.to_string()], filtered_out_gate_ids, Some(filter.to_string())))
}

fn policy_denominator_sha256(policy: &GatePolicy, native_tiers: &[String]) -> Result<String> {
    let mut gates = policy
        .gates
        .iter()
        .map(|gate| PolicyGateIdentity {
            id: gate.name.as_str(),
            native_tier: gate.tier.as_str(),
        })
        .collect::<Vec<_>>();
    gates.sort_by(|left, right| {
        left.id
            .cmp(right.id)
            .then_with(|| left.native_tier.cmp(right.native_tier))
    });
    let source = PolicyDenominatorIdentity {
        schema_version: policy.schema_version,
        native_tiers,
        gates: &gates,
    };
    let bytes = serde_json::to_vec(&source)
        .map_err(|error| eyre!("failed to encode policy denominator identity: {error}"))?;
    Ok(sha256_hex(&bytes))
}

fn finish(mut result: RouteProfileDenominator) -> Result<RouteProfileDenominator> {
    let semantic = SemanticIdentity {
        schema_version: result.schema_version,
        requested_profile: result.requested_profile,
        status: result.status,
        included_native_tiers: &result.included_native_tiers,
        excluded_native_tiers: &result.excluded_native_tiers,
        exact_gate_filter: &result.exact_gate_filter,
        profile_gate_ids: &result.profile_gate_ids,
        governed_gate_ids: &result.governed_gate_ids,
        filtered_out_gate_ids: &result.filtered_out_gate_ids,
        outside_profile_gate_ids: &result.outside_profile_gate_ids,
        policy_denominator_sha256: &result.policy_denominator_sha256,
        expansion_rules_sha256: &result.expansion_rules_sha256,
        limitations: &result.limitations,
    };
    let bytes = serde_json::to_vec(&semantic)
        .map_err(|error| eyre!("failed to encode route-profile identity: {error}"))?;
    result.semantic_sha256 = sha256_hex(&bytes);
    Ok(result)
}

fn sorted_gate_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut ids = ids.map(str::to_string).collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::gates::{
        GateDefinition, GatePlanningConfig, GatePlanningRole, GlobalSettings, TierDefinition,
    };

    fn gate(name: &str, tier: &str) -> GateDefinition {
        GateDefinition {
            name: name.to_string(),
            tier: tier.to_string(),
            description: name.to_string(),
            required: false,
            command: "true".to_string(),
            timeout_seconds: 30,
            retry_count: 0,
            budgets: None,
            quarantine: false,
            tags: Vec::new(),
            artifacts: Vec::new(),
            matrix: None,
            planning: if tier == "pr_fast" {
                Some(GatePlanningConfig {
                    role: GatePlanningRole::AlwaysOn,
                    packages: Vec::new(),
                })
            } else {
                None
            },
        }
    }

    fn tier() -> TierDefinition {
        TierDefinition {
            description: "fixture".to_string(),
            target_duration_seconds: 30,
            enforcement: "advisory".to_string(),
            trigger: Vec::new(),
        }
    }

    fn policy() -> GatePolicy {
        GatePolicy {
            schema_version: 1,
            global: GlobalSettings {
                default_timeout_seconds: 30,
                artifact_retention_days: 0,
                default_retry_count: 0,
                environment: HashMap::new(),
                toolchain: None,
            },
            tiers: HashMap::from([
                ("release".to_string(), tier()),
                ("nightly".to_string(), tier()),
                ("merge_gate".to_string(), tier()),
                ("pr_fast".to_string(), tier()),
                ("commit".to_string(), tier()),
            ]),
            gates: vec![
                gate("release_gate", "release"),
                gate("nightly_gate", "nightly"),
                gate("merge_gate", "merge_gate"),
                gate("pr_gate", "pr_fast"),
                gate("commit_gate", "commit"),
            ],
            flake_policy: None,
            audit: None,
        }
    }

    #[test]
    fn aggregate_profiles_preserve_execution_membership() -> Result<()> {
        let policy = policy();
        let commit = expand_route_profile(&policy, RequestedRouteProfile::Commit, None)?;
        let pr_fast = expand_route_profile(&policy, RequestedRouteProfile::PrFast, None)?;
        let merge = expand_route_profile(&policy, RequestedRouteProfile::MergeGate, None)?;
        let nightly = expand_route_profile(&policy, RequestedRouteProfile::Nightly, None)?;
        let all = expand_route_profile(&policy, RequestedRouteProfile::All, None)?;

        assert_eq!(commit.governed_gate_ids, vec!["commit_gate"]);
        assert_eq!(pr_fast.governed_gate_ids, vec!["pr_gate"]);
        assert_eq!(merge.governed_gate_ids, vec!["merge_gate", "pr_gate"]);
        assert_eq!(
            nightly.governed_gate_ids,
            vec!["merge_gate", "nightly_gate", "pr_gate"]
        );
        assert_eq!(
            all.governed_gate_ids,
            vec!["commit_gate", "merge_gate", "nightly_gate", "pr_gate", "release_gate"]
        );
        assert!(!nightly.included_native_tiers.contains(&"commit".to_string()));
        assert!(!nightly.included_native_tiers.contains(&"release".to_string()));
        Ok(())
    }

    #[test]
    fn release_profile_is_explicitly_not_proven() -> Result<()> {
        let result = expand_route_profile(&policy(), RequestedRouteProfile::Release, None)?;

        assert_eq!(result.status, RouteProfileStatus::NotProven);
        assert!(result.governed_gate_ids.is_empty());
        assert_eq!(
            result.limitations,
            vec!["release_profile_composition_not_reviewed".to_string()]
        );
        Ok(())
    }

    #[test]
    fn exact_gate_filter_is_a_separate_narrowing_operation() -> Result<()> {
        let result = expand_route_profile(
            &policy(),
            RequestedRouteProfile::MergeGate,
            Some("merge_gate"),
        )?;

        assert_eq!(result.profile_gate_ids, vec!["merge_gate", "pr_gate"]);
        assert_eq!(result.governed_gate_ids, vec!["merge_gate"]);
        assert_eq!(result.filtered_out_gate_ids, vec!["pr_gate"]);
        assert_eq!(result.exact_gate_filter.as_deref(), Some("merge_gate"));
        Ok(())
    }

    #[test]
    fn filter_outside_profile_fails_closed() -> Result<()> {
        let error = expand_route_profile(
            &policy(),
            RequestedRouteProfile::MergeGate,
            Some("release_gate"),
        )
        .expect_err("outside-profile filter must fail");

        assert!(error.to_string().contains("outside the requested profile"));
        Ok(())
    }

    #[test]
    fn duplicate_gate_identity_fails_closed() -> Result<()> {
        let mut policy = policy();
        policy.gates.push(gate("pr_gate", "merge_gate"));

        let error = expand_route_profile(&policy, RequestedRouteProfile::All, None)
            .expect_err("duplicate gate identity must fail");

        assert!(error.to_string().contains("duplicate governed gate identity"));
        Ok(())
    }

    #[test]
    fn unknown_native_tier_fails_closed() -> Result<()> {
        let mut policy = policy();
        policy.gates.push(gate("unknown_gate", "unregistered"));

        let error = expand_route_profile(&policy, RequestedRouteProfile::All, None)
            .expect_err("unknown native tier must fail");

        assert!(error.to_string().contains("absent from policy.tiers"));
        Ok(())
    }

    #[test]
    fn all_automatically_includes_a_new_policy_tier() -> Result<()> {
        let mut policy = policy();
        policy.tiers.insert("experimental".to_string(), tier());
        policy.gates.push(gate("experimental_gate", "experimental"));

        let result = expand_route_profile(&policy, RequestedRouteProfile::All, None)?;

        assert!(result.included_native_tiers.contains(&"experimental".to_string()));
        assert!(result.governed_gate_ids.contains(&"experimental_gate".to_string()));
        Ok(())
    }

    #[test]
    fn source_order_does_not_change_semantic_identity() -> Result<()> {
        let first = policy();
        let mut second = policy();
        second.gates.reverse();

        let first = expand_route_profile(&first, RequestedRouteProfile::All, None)?;
        let second = expand_route_profile(&second, RequestedRouteProfile::All, None)?;

        assert_eq!(first.semantic_sha256, second.semantic_sha256);
        assert_eq!(first.policy_denominator_sha256, second.policy_denominator_sha256);
        Ok(())
    }

    #[test]
    fn semantic_membership_change_changes_identity() -> Result<()> {
        let first = policy();
        let mut second = policy();
        second.gates.push(gate("new_gate", "merge_gate"));

        let first = expand_route_profile(&first, RequestedRouteProfile::MergeGate, None)?;
        let second = expand_route_profile(&second, RequestedRouteProfile::MergeGate, None)?;

        assert_ne!(first.semantic_sha256, second.semantic_sha256);
        assert_ne!(first.policy_denominator_sha256, second.policy_denominator_sha256);
        Ok(())
    }
}
'''

DOC_SOURCE = r'''# Route-profile gate denominator

`ci_route_profile.v1` defines which native gate-policy rows are governed by one requested execution profile. It is the denominator consumed before lifecycle disposition, exact-subject selectors, or planned outcomes are applied.

## Current execution profiles

```text
commit     -> commit
pr_fast    -> pr_fast
merge_gate -> pr_fast + merge_gate
nightly    -> pr_fast + merge_gate + nightly
all        -> every tier present in the accepted policy
release    -> NOT_PROVEN until its composition is separately reviewed
```

The `nightly` profile deliberately excludes `commit` and `release`. The `all` profile automatically includes a newly accepted native tier rather than requiring another hard-coded list.

## Contract

The receipt retains:

- the requested profile and status;
- included and explicitly excluded native tiers;
- the complete profile gate population;
- an optional exact-gate narrowing and every filtered-out row;
- gates outside the profile;
- deterministic policy-denominator, expansion-rule, and semantic identities;
- typed limitations where a profile is not proven.

An exact gate filter narrows an already-expanded profile. It does not redefine profile membership. A filter naming an unknown or outside-profile gate fails closed.

## Authority boundary

This contract answers only:

> Which canonical policy rows must receive one route-plan disposition for this requested profile?

It does not decide whether a gate is active, dormant, quarantined, retired, or blocked; whether it applies to one changed subject; whether it should run; whether it passed; or whether GitHub enforces its context. Those remain the lifecycle, selector, route-plan, result, and live-enforcement authorities.

The current `gates` runner consumes this expansion for actual profile composition. The independently listed `--list` display path remains non-authoritative where it historically differs, and regression tests preserve that distinction.
'''


def patch() -> None:
    gates_path = Path("xtask/src/tasks/gates.rs")
    text = gates_path.read_text(encoding="utf-8")

    old_modules = "mod first_failure;\nmod planning_types;\n"
    new_modules = "mod first_failure;\nmod planning_types;\nmod route_profile;\n"
    if text.count(old_modules) != 1:
        raise SystemExit("gates module insertion point did not match exactly once")
    text = text.replace(old_modules, new_modules, 1)

    start = text.index("/// Tiers `plan_gates`'s `MergeGate` arm")
    end_marker = 'const NIGHTLY_EXTRA_TIERS: &[&str] = &["merge_gate", "nightly"];\n'
    end = text.index(end_marker, start) + len(end_marker)
    text = text[:start] + (
        "/// Requested-profile composition is owned by `route_profile`; both the\n"
        "/// staged-tree guard and executable planner consume that one authority.\n"
    ) + text[end:]

    old_match = '''    Ok(match config.tier {
        GateTier::Commit => true,
        // A commit-tier gate's `tier` field is literally `"commit"`, so it
        // can never be selected by `gates_for_tier(policy, "pr_fast")` —
        // pr_fast's base selection structurally excludes it regardless of
        // policy content.
        GateTier::PrFast => false,
        GateTier::MergeGate => MERGE_GATE_EXTRA_TIERS.contains(&"commit"),
        GateTier::Nightly => NIGHTLY_EXTRA_TIERS.contains(&"commit"),
        // `extend_plan_with_non_pr_fast_static_gates` adds every gate whose
        // tier isn't "pr_fast" — i.e. truly everything the policy defines,
        // so the real policy content (not a hard-coded set) is the correct
        // oracle here.
        GateTier::All => policy.gates.iter().any(|gate| gate.tier == "commit"),
    })
'''
    new_match = '''    route_profile::profile_includes_native_tier(policy, &config.tier, "commit")
'''
    if text.count(old_match) != 1:
        raise SystemExit("selects_commit_tier_gate match did not match exactly once")
    text = text.replace(old_match, new_match, 1)

    plan_start = text.index("fn plan_gates(")
    plan_end = text.index("\nfn static_gate_plan(", plan_start)
    new_plan = r'''fn plan_gates(root: &Path, policy: &GatePolicy, config: &GateRunnerConfig) -> Result<GatePlan> {
    let base = config.base_ref.clone().unwrap_or_else(|| select_scope_base(root));
    let staged_tree_oid = resolve_staged_tree_oid(root, config)?;

    if config.gate_filter.is_some() {
        return Ok(static_gate_plan(
            config.tier.clone(),
            base,
            filter_gates(policy, config)?,
            staged_tree_oid,
        ));
    }

    let profile = route_profile::expand_gate_tier(policy, &config.tier, None)?;
    if profile.status != route_profile::RouteProfileStatus::Supported {
        bail!(
            "requested gate profile {:?} is not proven: {:?}",
            profile.requested_profile,
            profile.limitations
        );
    }

    match config.tier {
        GateTier::Commit => Ok(static_gate_plan(
            GateTier::Commit,
            base,
            gates_for_tier(policy, "commit"),
            staged_tree_oid,
        )),
        GateTier::PrFast => {
            let mut plan = plan_pr_fast_gates(root, gates_for_tier(policy, "pr_fast"), base)?;
            plan.staged_tree_oid = staged_tree_oid;
            Ok(plan)
        }
        GateTier::MergeGate | GateTier::Nightly | GateTier::All => {
            let mut plan = plan_pr_fast_gates(root, gates_for_tier(policy, "pr_fast"), base)?;
            plan.tier = config.tier.clone();
            let extra_tiers = profile
                .included_native_tiers
                .iter()
                .map(String::as_str)
                .filter(|tier| *tier != "pr_fast")
                .collect::<Vec<_>>();
            extend_plan_with_static_tiers(&mut plan, policy, &extra_tiers);
            plan.staged_tree_oid = staged_tree_oid;
            Ok(plan)
        }
    }
}
'''
    text = text[:plan_start] + new_plan + text[plan_end:]

    gates_path.write_text(text, encoding="utf-8")
    route_path = Path("xtask/src/tasks/gates/route_profile.rs")
    route_path.parent.mkdir(parents=True, exist_ok=True)
    route_path.write_text(ROUTE_PROFILE_SOURCE, encoding="utf-8")
    doc_path = Path("docs/ci/route-profile-denominator.md")
    doc_path.parent.mkdir(parents=True, exist_ok=True)
    doc_path.write_text(DOC_SOURCE, encoding="utf-8")


def request(method: str, path: str, payload: dict | None = None) -> dict:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}{path}",
        data=data,
        method=method,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {os.environ['GITHUB_TOKEN']}",
            "Content-Type": "application/json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(
            f"GitHub API {method} {path} failed: {error.code} "
            f"{error.read().decode(errors='replace')}"
        ) from error


def publish() -> None:
    base = request("GET", f"/git/commits/{BASE_SHA}")
    entries = []
    manifest = {}
    for raw_path in CHANGED_PATHS:
        path = Path(raw_path)
        data = path.read_bytes()
        blob = request(
            "POST",
            "/git/blobs",
            {
                "content": base64.b64encode(data).decode("ascii"),
                "encoding": "base64",
            },
        )
        entries.append(
            {
                "path": raw_path,
                "mode": "100644",
                "type": "blob",
                "sha": blob["sha"],
            }
        )
        manifest[raw_path] = {"blob_sha": blob["sha"], "size": len(data)}
    tree = request(
        "POST",
        "/git/trees",
        {"base_tree": base["tree"]["sha"], "tree": entries},
    )
    commit = request(
        "POST",
        "/git/commits",
        {
            "message": "feat(ci): define route-profile gate denominator (#10178)",
            "tree": tree["sha"],
            "parents": [BASE_SHA],
        },
    )
    request(
        "POST",
        "/git/refs",
        {"ref": f"refs/heads/{TARGET_BRANCH}", "sha": commit["sha"]},
    )
    result = {
        "schema_version": 1,
        "base": BASE_SHA,
        "branch": TARGET_BRANCH,
        "head": commit["sha"],
        "files": manifest,
    }
    output = Path("target/10178-result.json")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, sort_keys=True))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("patch", "publish"))
    args = parser.parse_args()
    if args.mode == "patch":
        patch()
    else:
        publish()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
