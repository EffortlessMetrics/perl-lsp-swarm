#!/usr/bin/env python3
"""Build and publish the clean #10176 candidate from current main."""

from __future__ import annotations

import argparse
import base64
import json
import os
import urllib.error
import urllib.request
from pathlib import Path

BASE_SHA = "10be0eceeee4ba27c18545a1cb75771a1057fcb9"
TARGET_BRANCH = "codex/ci-gate-disposition-authority"
CHANGED_PATHS = [
    ".ci/gate-policy.yaml",
    "xtask/src/tasks/mod.rs",
    "xtask/src/tasks/gate_disposition.rs",
    "docs/ci/gate-disposition-authority.md",
]

DISPOSITION_SOURCE = r'''//! Typed lifecycle and quarantine authority for CI gates.
//!
//! The canonical declarations live inside `.ci/gate-policy.yaml`. Every gate
//! receives one lifecycle row: ordinary rows derive `active` from that same
//! policy authority, while every non-active state must be explicit and
//! evidence-bearing. Lifecycle never establishes selector non-applicability,
//! a planned outcome, an execution verdict, or live GitHub enforcement.

use crate::tasks::gates::{GatePolicy, load_policy_for_inspection};
use chrono::NaiveDate;
use color_eyre::eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub const GATE_DISPOSITION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateDispositionPolicy {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub declarations: Vec<GateDispositionDeclaration>,
}

impl Default for GateDispositionPolicy {
    fn default() -> Self {
        Self {
            schema_version: GATE_DISPOSITION_SCHEMA_VERSION,
            declarations: Vec::new(),
        }
    }
}

fn default_schema_version() -> u32 {
    GATE_DISPOSITION_SCHEMA_VERSION
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateDispositionDeclaration {
    pub gate_id: String,
    pub lifecycle: GateLifecycle,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub review_after: Option<NaiveDate>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub authoritative_producer: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateLifecycle {
    Active,
    Dormant,
    Quarantined,
    Retired,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatePolicyRole {
    Required,
    Advisory,
    Informational,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionSourceKind {
    DefaultActive,
    Explicit,
    LegacyBoolean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedDispositionState {
    Current,
    Expired,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDispositionAuthorityStatus {
    Current,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispositionFinding {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateDispositionRow {
    pub gate_id: String,
    pub policy_role: GatePolicyRole,
    pub lifecycle: GateLifecycle,
    pub intended_profiles: Vec<String>,
    pub execution_allowed: bool,
    pub owner: Option<String>,
    pub reason: Option<String>,
    pub review_after: Option<NaiveDate>,
    pub prerequisites: Vec<String>,
    pub authoritative_producer: Option<String>,
    pub source_kind: DispositionSourceKind,
    pub resolved_state: ResolvedDispositionState,
    pub findings: Vec<DispositionFinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateDispositionAuthority {
    pub schema_version: u32,
    pub source_path: String,
    pub source_sha256: String,
    pub as_of: NaiveDate,
    pub status: GateDispositionAuthorityStatus,
    pub rows: Vec<GateDispositionRow>,
    pub semantic_sha256: String,
}

#[derive(Deserialize)]
struct PolicyEnvelope {
    #[serde(default)]
    gate_dispositions: GateDispositionPolicy,
}

#[derive(Serialize)]
struct SourceGate<'a> {
    gate_id: &'a str,
    native_tier: &'a str,
    required: bool,
    legacy_quarantine: bool,
}

#[derive(Serialize)]
struct SourceIdentity<'a> {
    policy_schema_version: u32,
    disposition_schema_version: u32,
    gates: &'a [SourceGate<'a>],
    declarations: &'a [GateDispositionDeclaration],
}

#[derive(Serialize)]
struct SemanticIdentity<'a> {
    schema_version: u32,
    source_path: &'a str,
    source_sha256: &'a str,
    as_of: NaiveDate,
    status: GateDispositionAuthorityStatus,
    rows: &'a [GateDispositionRow],
}

pub fn load_from_path(path: &Path, as_of: NaiveDate) -> Result<GateDispositionAuthority> {
    let raw = fs::read_to_string(path)
        .map_err(|error| eyre!("failed to read {}: {error}", path.display()))?;
    let envelope: PolicyEnvelope = serde_yaml_ng::from_str(&raw)
        .map_err(|error| eyre!("failed to parse gate dispositions from {}: {error}", path.display()))?;
    let policy = load_policy_for_inspection(path)?;
    resolve(
        &policy,
        &envelope.gate_dispositions,
        as_of,
        &path.to_string_lossy(),
    )
}

pub fn resolve(
    policy: &GatePolicy,
    disposition_policy: &GateDispositionPolicy,
    as_of: NaiveDate,
    source_path: &str,
) -> Result<GateDispositionAuthority> {
    if disposition_policy.schema_version != GATE_DISPOSITION_SCHEMA_VERSION {
        bail!(
            "unsupported gate-disposition schema version {}; expected {}",
            disposition_policy.schema_version,
            GATE_DISPOSITION_SCHEMA_VERSION
        );
    }

    let mut gate_ids = HashSet::new();
    for gate in &policy.gates {
        if !gate_ids.insert(gate.name.as_str()) {
            bail!("duplicate governed gate identity '{}'", gate.name);
        }
    }

    let mut declarations = disposition_policy.declarations.clone();
    declarations.sort_by(|left, right| left.gate_id.cmp(&right.gate_id));
    let mut by_gate = HashMap::new();
    for declaration in &declarations {
        if !gate_ids.contains(declaration.gate_id.as_str()) {
            bail!(
                "gate disposition names unknown gate '{}'",
                declaration.gate_id
            );
        }
        if by_gate.insert(declaration.gate_id.as_str(), declaration).is_some() {
            bail!(
                "conflicting duplicate gate disposition for '{}'",
                declaration.gate_id
            );
        }
    }

    let source_sha256 = source_sha256(policy, disposition_policy.schema_version, &declarations)?;
    let mut rows = policy
        .gates
        .iter()
        .map(|gate| {
            resolve_gate(
                gate.name.as_str(),
                gate.tier.as_str(),
                gate.required,
                gate.quarantine,
                by_gate.get(gate.name.as_str()).copied(),
                as_of,
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.gate_id.cmp(&right.gate_id));

    let status = if rows
        .iter()
        .all(|row| row.resolved_state == ResolvedDispositionState::Current)
    {
        GateDispositionAuthorityStatus::Current
    } else {
        GateDispositionAuthorityStatus::Invalid
    };

    let mut authority = GateDispositionAuthority {
        schema_version: GATE_DISPOSITION_SCHEMA_VERSION,
        source_path: source_path.to_string(),
        source_sha256,
        as_of,
        status,
        rows,
        semantic_sha256: String::new(),
    };
    authority.semantic_sha256 = semantic_sha256(&authority)?;
    Ok(authority)
}

pub fn explain_gate(authority: &GateDispositionAuthority, gate_id: &str) -> Result<String> {
    let row = authority
        .rows
        .iter()
        .find(|row| row.gate_id == gate_id)
        .ok_or_else(|| eyre!("gate disposition has no row for '{gate_id}'"))?;
    let finding_codes = row
        .findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "gate={} role={:?} lifecycle={:?} resolved={:?} source={:?} execution_allowed={} owner={} reason={} review_after={} findings={}",
        row.gate_id,
        row.policy_role,
        row.lifecycle,
        row.resolved_state,
        row.source_kind,
        row.execution_allowed,
        row.owner.as_deref().unwrap_or("none"),
        row.reason.as_deref().unwrap_or("none"),
        row.review_after.map_or_else(|| "none".to_string(), |date| date.to_string()),
        finding_codes,
    ))
}

fn resolve_gate(
    gate_id: &str,
    native_tier: &str,
    required: bool,
    legacy_quarantine: bool,
    declaration: Option<&GateDispositionDeclaration>,
    as_of: NaiveDate,
) -> GateDispositionRow {
    let policy_role = if required {
        GatePolicyRole::Required
    } else {
        GatePolicyRole::Advisory
    };

    let Some(declaration) = declaration else {
        if legacy_quarantine {
            return GateDispositionRow {
                gate_id: gate_id.to_string(),
                policy_role,
                lifecycle: GateLifecycle::Quarantined,
                intended_profiles: vec![native_tier.to_string()],
                execution_allowed: false,
                owner: None,
                reason: None,
                review_after: None,
                prerequisites: Vec::new(),
                authoritative_producer: None,
                source_kind: DispositionSourceKind::LegacyBoolean,
                resolved_state: ResolvedDispositionState::Invalid,
                findings: vec![finding(
                    "legacy_quarantine_without_authority",
                    "legacy gate.quarantine=true lacks owner, reason, review horizon, and typed source identity",
                )],
            };
        }
        return GateDispositionRow {
            gate_id: gate_id.to_string(),
            policy_role,
            lifecycle: GateLifecycle::Active,
            intended_profiles: vec![native_tier.to_string()],
            execution_allowed: true,
            owner: None,
            reason: None,
            review_after: None,
            prerequisites: Vec::new(),
            authoritative_producer: None,
            source_kind: DispositionSourceKind::DefaultActive,
            resolved_state: ResolvedDispositionState::Current,
            findings: Vec::new(),
        };
    };

    let mut findings = Vec::new();
    if declaration.lifecycle != GateLifecycle::Active {
        require_non_empty(
            &mut findings,
            "owner_missing",
            declaration.owner.as_deref(),
            "non-active lifecycle requires an accountable owner",
        );
        require_non_empty(
            &mut findings,
            "reason_missing",
            declaration.reason.as_deref(),
            "non-active lifecycle requires a reason token or bounded explanation",
        );
    }
    if matches!(
        declaration.lifecycle,
        GateLifecycle::Dormant | GateLifecycle::Quarantined | GateLifecycle::Blocked
    ) && declaration.review_after.is_none()
    {
        findings.push(finding(
            "review_horizon_missing",
            "dormant, quarantined, or blocked lifecycle requires review_after",
        ));
    }
    if declaration.lifecycle == GateLifecycle::Active && legacy_quarantine {
        findings.push(finding(
            "legacy_quarantine_conflicts_with_active",
            "gate.quarantine=true conflicts with explicit active lifecycle",
        ));
    }
    if declaration.lifecycle != GateLifecycle::Quarantined && legacy_quarantine {
        findings.push(finding(
            "legacy_quarantine_conflicts_with_lifecycle",
            "gate.quarantine=true conflicts with the explicit non-quarantined lifecycle",
        ));
    }

    let expired = declaration.review_after.is_some_and(|review_after| review_after < as_of);
    if expired {
        findings.push(finding(
            "review_horizon_expired",
            "review_after is earlier than the resolver as_of date",
        ));
    }

    let resolved_state = if expired {
        ResolvedDispositionState::Expired
    } else if findings.is_empty() {
        ResolvedDispositionState::Current
    } else {
        ResolvedDispositionState::Invalid
    };

    GateDispositionRow {
        gate_id: gate_id.to_string(),
        policy_role,
        lifecycle: declaration.lifecycle,
        intended_profiles: vec![native_tier.to_string()],
        execution_allowed: declaration.lifecycle == GateLifecycle::Active
            && resolved_state == ResolvedDispositionState::Current,
        owner: declaration.owner.clone(),
        reason: declaration.reason.clone(),
        review_after: declaration.review_after,
        prerequisites: sorted_strings(&declaration.prerequisites),
        authoritative_producer: declaration.authoritative_producer.clone(),
        source_kind: DispositionSourceKind::Explicit,
        resolved_state,
        findings,
    }
}

fn require_non_empty(
    findings: &mut Vec<DispositionFinding>,
    code: &str,
    value: Option<&str>,
    detail: &str,
) {
    if value.is_none_or(|value| value.trim().is_empty()) {
        findings.push(finding(code, detail));
    }
}

fn finding(code: &str, detail: &str) -> DispositionFinding {
    DispositionFinding {
        code: code.to_string(),
        detail: detail.to_string(),
    }
}

fn source_sha256(
    policy: &GatePolicy,
    disposition_schema_version: u32,
    declarations: &[GateDispositionDeclaration],
) -> Result<String> {
    let mut gates = policy
        .gates
        .iter()
        .map(|gate| SourceGate {
            gate_id: gate.name.as_str(),
            native_tier: gate.tier.as_str(),
            required: gate.required,
            legacy_quarantine: gate.quarantine,
        })
        .collect::<Vec<_>>();
    gates.sort_by(|left, right| {
        left.gate_id
            .cmp(right.gate_id)
            .then_with(|| left.native_tier.cmp(right.native_tier))
    });
    let identity = SourceIdentity {
        policy_schema_version: policy.schema_version,
        disposition_schema_version,
        gates: &gates,
        declarations,
    };
    let bytes = serde_json::to_vec(&identity)
        .map_err(|error| eyre!("failed to encode gate-disposition source identity: {error}"))?;
    Ok(sha256_hex(&bytes))
}

fn semantic_sha256(authority: &GateDispositionAuthority) -> Result<String> {
    let identity = SemanticIdentity {
        schema_version: authority.schema_version,
        source_path: &authority.source_path,
        source_sha256: &authority.source_sha256,
        as_of: authority.as_of,
        status: authority.status,
        rows: &authority.rows,
    };
    let bytes = serde_json::to_vec(&identity)
        .map_err(|error| eyre!("failed to encode gate-disposition semantic identity: {error}"))?;
    Ok(sha256_hex(&bytes))
}

fn sorted_strings(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
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

    fn date(value: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|error| eyre!("invalid test date {value}: {error}"))
    }

    fn tier() -> TierDefinition {
        TierDefinition {
            description: "fixture".to_string(),
            target_duration_seconds: 30,
            enforcement: "advisory".to_string(),
            trigger: Vec::new(),
        }
    }

    fn gate(name: &str, required: bool) -> GateDefinition {
        GateDefinition {
            name: name.to_string(),
            tier: "merge_gate".to_string(),
            description: name.to_string(),
            required,
            command: "true".to_string(),
            timeout_seconds: 30,
            retry_count: 0,
            budgets: None,
            quarantine: false,
            tags: Vec::new(),
            artifacts: Vec::new(),
            matrix: None,
            planning: Some(GatePlanningConfig {
                role: GatePlanningRole::Static,
                packages: Vec::new(),
            }),
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
            tiers: HashMap::from([("merge_gate".to_string(), tier())]),
            gates: vec![gate("required_gate", true), gate("advisory_gate", false)],
            flake_policy: None,
            audit: None,
        }
    }

    fn declaration(gate_id: &str, lifecycle: GateLifecycle) -> GateDispositionDeclaration {
        GateDispositionDeclaration {
            gate_id: gate_id.to_string(),
            lifecycle,
            owner: Some("#6261".to_string()),
            reason: Some("bounded_policy_state".to_string()),
            review_after: Some(
                NaiveDate::from_ymd_opt(2027, 1, 1)
                    .unwrap_or(NaiveDate::MAX),
            ),
            prerequisites: vec!["proof_receipt".to_string()],
            authoritative_producer: None,
        }
    }

    #[test]
    fn ordinary_rows_receive_current_active_defaults() -> Result<()> {
        let authority = resolve(
            &policy(),
            &GateDispositionPolicy::default(),
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;

        assert_eq!(authority.status, GateDispositionAuthorityStatus::Current);
        assert_eq!(authority.rows.len(), 2);
        assert!(authority.rows.iter().all(|row| row.lifecycle == GateLifecycle::Active));
        assert!(authority.rows.iter().all(|row| row.execution_allowed));
        let required = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "required_gate")
            .ok_or_else(|| eyre!("missing required gate row"))?;
        assert_eq!(required.policy_role, GatePolicyRole::Required);
        Ok(())
    }

    #[test]
    fn current_quarantine_retains_owner_reason_and_review_horizon() -> Result<()> {
        let policy = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![declaration("required_gate", GateLifecycle::Quarantined)],
        };
        let authority = resolve(
            &self::policy(),
            &policy,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let row = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "required_gate")
            .ok_or_else(|| eyre!("missing quarantine row"))?;

        assert_eq!(row.resolved_state, ResolvedDispositionState::Current);
        assert_eq!(row.lifecycle, GateLifecycle::Quarantined);
        assert!(!row.execution_allowed);
        assert_eq!(row.owner.as_deref(), Some("#6261"));
        assert!(row.findings.is_empty());
        Ok(())
    }

    #[test]
    fn quarantine_without_owner_reason_or_review_horizon_is_invalid() -> Result<()> {
        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![GateDispositionDeclaration {
                gate_id: "required_gate".to_string(),
                lifecycle: GateLifecycle::Quarantined,
                owner: None,
                reason: None,
                review_after: None,
                prerequisites: Vec::new(),
                authoritative_producer: None,
            }],
        };
        let authority = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let row = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "required_gate")
            .ok_or_else(|| eyre!("missing invalid row"))?;
        let codes = row
            .findings
            .iter()
            .map(|finding| finding.code.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(authority.status, GateDispositionAuthorityStatus::Invalid);
        assert_eq!(row.resolved_state, ResolvedDispositionState::Invalid);
        assert!(codes.contains("owner_missing"));
        assert!(codes.contains("reason_missing"));
        assert!(codes.contains("review_horizon_missing"));
        Ok(())
    }

    #[test]
    fn expired_review_horizon_remains_explicit_non_success() -> Result<()> {
        let mut item = declaration("required_gate", GateLifecycle::Blocked);
        item.review_after = Some(date("2026-01-01")?);
        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![item],
        };
        let authority = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let row = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "required_gate")
            .ok_or_else(|| eyre!("missing expired row"))?;

        assert_eq!(row.resolved_state, ResolvedDispositionState::Expired);
        assert!(!row.execution_allowed);
        assert!(row.findings.iter().any(|finding| finding.code == "review_horizon_expired"));
        Ok(())
    }

    #[test]
    fn required_policy_role_does_not_activate_dormant_lifecycle() -> Result<()> {
        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![declaration("required_gate", GateLifecycle::Dormant)],
        };
        let authority = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let row = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "required_gate")
            .ok_or_else(|| eyre!("missing dormant row"))?;

        assert_eq!(row.policy_role, GatePolicyRole::Required);
        assert_eq!(row.lifecycle, GateLifecycle::Dormant);
        assert!(!row.execution_allowed);
        Ok(())
    }

    #[test]
    fn retired_gate_is_not_runnable() -> Result<()> {
        let mut item = declaration("advisory_gate", GateLifecycle::Retired);
        item.review_after = None;
        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![item],
        };
        let authority = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let row = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "advisory_gate")
            .ok_or_else(|| eyre!("missing retired row"))?;

        assert_eq!(row.resolved_state, ResolvedDispositionState::Current);
        assert!(!row.execution_allowed);
        Ok(())
    }

    #[test]
    fn duplicate_or_unknown_declarations_fail_closed() -> Result<()> {
        let duplicate = declaration("required_gate", GateLifecycle::Dormant);
        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![duplicate.clone(), duplicate],
        };
        let Err(duplicate_error) = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        ) else {
            bail!("duplicate declaration should fail");
        };
        assert!(duplicate_error.to_string().contains("conflicting duplicate"));

        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![declaration("unknown_gate", GateLifecycle::Dormant)],
        };
        let Err(unknown_error) = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        ) else {
            bail!("unknown declaration should fail");
        };
        assert!(unknown_error.to_string().contains("unknown gate"));
        Ok(())
    }

    #[test]
    fn legacy_quarantine_without_typed_authority_is_invalid() -> Result<()> {
        let mut policy = policy();
        let gate = policy
            .gates
            .iter_mut()
            .find(|gate| gate.name == "required_gate")
            .ok_or_else(|| eyre!("missing gate fixture"))?;
        gate.quarantine = true;

        let authority = resolve(
            &policy,
            &GateDispositionPolicy::default(),
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let row = authority
            .rows
            .iter()
            .find(|row| row.gate_id == "required_gate")
            .ok_or_else(|| eyre!("missing legacy quarantine row"))?;

        assert_eq!(row.source_kind, DispositionSourceKind::LegacyBoolean);
        assert_eq!(row.resolved_state, ResolvedDispositionState::Invalid);
        assert!(!row.execution_allowed);
        Ok(())
    }

    #[test]
    fn source_order_movement_preserves_semantic_identity() -> Result<()> {
        let mut first_policy = policy();
        let mut second_policy = policy();
        second_policy.gates.reverse();
        let first_dispositions = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![
                declaration("required_gate", GateLifecycle::Dormant),
                declaration("advisory_gate", GateLifecycle::Blocked),
            ],
        };
        let mut second_dispositions = first_dispositions.clone();
        second_dispositions.declarations.reverse();

        let first = resolve(
            &first_policy,
            &first_dispositions,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        first_policy.gates.reverse();
        let second = resolve(
            &second_policy,
            &second_dispositions,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;

        assert_eq!(first.source_sha256, second.source_sha256);
        assert_eq!(first.semantic_sha256, second.semantic_sha256);
        Ok(())
    }

    #[test]
    fn lifecycle_movement_changes_semantic_identity() -> Result<()> {
        let first = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![declaration("required_gate", GateLifecycle::Dormant)],
        };
        let second = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![declaration("required_gate", GateLifecycle::Blocked)],
        };

        let first = resolve(
            &policy(),
            &first,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let second = resolve(
            &policy(),
            &second,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;

        assert_ne!(first.source_sha256, second.source_sha256);
        assert_ne!(first.semantic_sha256, second.semantic_sha256);
        Ok(())
    }

    #[test]
    fn explain_output_identifies_exact_authority_and_state() -> Result<()> {
        let disposition = GateDispositionPolicy {
            schema_version: 1,
            declarations: vec![declaration("required_gate", GateLifecycle::Dormant)],
        };
        let authority = resolve(
            &policy(),
            &disposition,
            date("2026-08-16")?,
            ".ci/gate-policy.yaml",
        )?;
        let explanation = explain_gate(&authority, "required_gate")?;

        assert!(explanation.contains("gate=required_gate"));
        assert!(explanation.contains("lifecycle=Dormant"));
        assert!(explanation.contains("source=Explicit"));
        assert!(explanation.contains("execution_allowed=false"));
        Ok(())
    }
}
'''

DOC_SOURCE = r'''# Gate disposition authority

`gate_disposition.v1` resolves one current lifecycle state for every gate in `.ci/gate-policy.yaml`:

```text
active | dormant | quarantined | retired | blocked
```

The lifecycle axis is independent from policy role, requested execution profile, exact-subject applicability, planned outcome, execution result, and live GitHub enforcement.

## Canonical source

The source remains `.ci/gate-policy.yaml`. Its `gate_dispositions` section contains only explicit non-default declarations. A gate with no declaration derives `active` mechanically from the same gate-policy authority.

```yaml
gate_dispositions:
  schema_version: 1
  declarations: []
```

Every non-active declaration names the stable gate identity. Dormant, quarantined, and blocked states require an owner, reason, and review horizon. Retired states require an owner and reason. Expired or incomplete authority remains explicit non-success; it never reverts to active or becomes a selector no-op.

The legacy per-gate `quarantine: true` Boolean is not sufficient authority. Without a matching typed declaration it resolves as invalid because it lacks owner, reason, review horizon, and source identity.

## Receipt

The resolver retains:

- policy role without deriving it from lifecycle;
- lifecycle and whether execution is permitted;
- intended native profile/tier;
- owner, reason, review horizon, prerequisites, and authoritative producer;
- default, explicit, or legacy source class;
- current, expired, or invalid state;
- deterministic source and semantic identities;
- bounded findings and explain output.

## Boundary

The resolver does not activate, retire, or execute a gate. It does not decide whether a gate applies to one change, whether a skipped process was legitimate, whether a product command passed, or whether GitHub requires a status context. Those decisions remain with their owning route, execution, and enforcement contracts.
'''


def patch() -> None:
    mod_path = Path("xtask/src/tasks/mod.rs")
    mod_text = mod_path.read_text(encoding="utf-8")
    marker = "pub mod gate_policy;\npub mod gate_receipts;\n"
    replacement = "pub mod gate_disposition;\npub mod gate_policy;\npub mod gate_receipts;\n"
    if mod_text.count(marker) != 1:
        raise SystemExit("tasks module insertion point did not match exactly once")
    mod_path.write_text(mod_text.replace(marker, replacement, 1), encoding="utf-8")

    policy_path = Path(".ci/gate-policy.yaml")
    policy_text = policy_path.read_text(encoding="utf-8")
    marker = "# =============================================================================\n# Flake Management Policy\n"
    block = '''# =============================================================================
# Gate Lifecycle Disposition Authority
# =============================================================================
# Ordinary gates derive active/current from the canonical gate row. Every
# non-active lifecycle must be explicit and evidence-bearing here; lifecycle
# never establishes selector non-applicability or an execution verdict.

gate_dispositions:
  schema_version: 1
  declarations: []

'''
    if policy_text.count(marker) != 1:
        raise SystemExit("gate disposition policy insertion point did not match exactly once")
    policy_path.write_text(policy_text.replace(marker, block + marker, 1), encoding="utf-8")

    source_path = Path("xtask/src/tasks/gate_disposition.rs")
    source_path.write_text(DISPOSITION_SOURCE, encoding="utf-8")
    doc_path = Path("docs/ci/gate-disposition-authority.md")
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
            "message": "feat(ci): expose typed gate disposition authority (#10176)",
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
    output = Path("target/10176-result.json")
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
