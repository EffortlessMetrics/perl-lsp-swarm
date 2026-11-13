//! Versioned currentness of declared formal inputs (#19314).
//!
//! Equality is conditional on the reviewed input boundary. This evaluator neither
//! discovers arbitrary Rust dependencies nor authenticates native execution.

pub mod snapshot;
mod unique_json;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use super::formal_receipt::{self, ClaimIdentity, ClosureIdentity, DispositionStatus,
    FormalProofReceipt, ImplementationBinding, ProofBounds, SourceIdentity, SourceState,
    ToolchainIdentity};
use snapshot::Snapshot;

pub const ALGORITHM: &str = "formal_declared_closure.v1";
pub const SPEC_VERSION: &str = "formal_closure_spec.v1";
pub const MAX_DOCUMENT: usize = 2 * 1024 * 1024;
const MAX_MEMBERS: usize = 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// The entire set must be accounted for by members or explicit exclusions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Kernel, Wrapper, Consumer, EnforcementBoundary, Manifest, Lockfile,
    BuildInput, GeneratedSource, GeneratorInput, Model, Harness, Property,
    Assumption, Control, SymbolicDomain, Correspondence,
}

const ROLES: &[Role] = &[
    Role::Kernel, Role::Wrapper, Role::Consumer, Role::EnforcementBoundary,
    Role::Manifest, Role::Lockfile, Role::BuildInput, Role::GeneratedSource,
    Role::GeneratorInput, Role::Model, Role::Harness, Role::Property,
    Role::Assumption, Role::Control, Role::SymbolicDomain, Role::Correspondence,
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub path: String,
    pub roles: BTreeSet<Role>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePolicy { CleanOnly }

/// Caller selection is independent of the historical receipt being compared.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema_version: String,
    pub source_policy: SourcePolicy,
    pub claim: ClaimIdentity,
    pub members: Vec<Member>,
    pub exclusions: BTreeMap<Role, String>,
    pub toolchain: ToolchainIdentity,
    pub bounds: ProofBounds,
    pub command: Vec<String>,
    pub dependency_model: BTreeMap<String, String>,
    pub environment: BTreeMap<String, String>,
    /// Unresolved inputs invalidate reuse; there is no trusted `complete: true` switch.
    pub unknown_inputs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Computed {
    pub evidence_class: &'static str,
    pub proof_accepted: bool,
    pub closure_completeness: &'static str,
    pub declared_roles: BTreeMap<String, BTreeSet<Role>>,
    pub source: SourceIdentity,
    pub claim: ClaimIdentity,
    pub closure: ClosureIdentity,
    pub input_blobs: BTreeMap<String, String>,
    pub unknown_inputs: Vec<String>,
    pub toolchain: ToolchainIdentity,
    pub bounds: ProofBounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness { Current, Stale, Unknown, Retired, Superseded }

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub evidence_class: &'static str,
    pub proof_accepted: bool,
    pub closure_completeness: &'static str,
    pub freshness: Freshness,
    pub reason: &'static str,
    pub first_difference: Option<String>,
    pub evaluated_commit: Option<String>,
    pub evaluated_closure: Option<String>,
    /// Canonical decoded-receipt identity, not authentication of its producer.
    pub compared_receipt_identity: Option<String>,
    /// Input equality alone never establishes live consumer correspondence.
    pub correspondence_established: bool,
    pub correspondence_inputs_unchanged: bool,
}

pub fn parse_spec(bytes: &[u8]) -> Result<Spec> {
    let value = unique_json::parse(bytes)?;
    serde_json::from_value(value).context("invalid formal closure specification")
}

pub fn parse_receipt(bytes: &[u8]) -> Result<FormalProofReceipt> {
    let value = unique_json::parse(bytes)?;
    let errors = formal_receipt::validate_value(&value);
    ensure!(errors.is_empty(), "receipt consistency refused: {}", errors.join("; "));
    Ok(serde_json::from_value(value)?)
}

fn unique_sorted(values: &mut Vec<String>, what: &str) -> Result<()> {
    ensure!(values.iter().all(|v| !v.trim().is_empty()), "blank {what}");
    values.sort();
    ensure!(values.windows(2).all(|pair| pair[0] != pair[1]), "duplicate {what}");
    Ok(())
}

fn digest_valid(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn claim_id_valid(value: &str) -> bool {
    let Some((prefix, suffix)) = value.strip_prefix("FV-").and_then(|s| s.rsplit_once('-')) else {
        return false;
    };
    suffix.len() == 3 && suffix.bytes().all(|b| b.is_ascii_digit())
        && prefix.split('-').all(|part| !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
}

fn normalize(spec: &Spec) -> Result<Spec> {
    let mut spec = spec.clone();
    ensure!(spec.schema_version == SPEC_VERSION, "unsupported specification version");
    ensure!(spec.claim.rollout != formal_receipt::RolloutState::Retired,
        "retired claim requires its historical receipt, not an active input selection");
    ensure!(claim_id_valid(&spec.claim.id) && spec.claim.generation > 0
        && spec.claim.owner_issue > 0 && !spec.claim.profile.trim().is_empty(), "invalid claim identity");
    ensure!(!spec.members.is_empty() && spec.members.len() <= MAX_MEMBERS, "invalid member count");
    spec.members.sort_by(|a, b| a.path.cmp(&b.path));
    let mut paths = BTreeSet::new();
    let mut represented = BTreeSet::new();
    for member in &spec.members {
        ensure!(snapshot::valid_path(&member.path), "noncanonical member path");
        ensure!(paths.insert(&member.path), "duplicate member path");
        ensure!(!member.roles.is_empty(), "member has no semantic role");
        represented.extend(member.roles.iter().copied());
    }
    for role in ROLES {
        match (represented.contains(role), spec.exclusions.get(role)) {
            (true, None) => {}
            (false, Some(reason)) if !reason.trim().is_empty() => {}
            _ => anyhow::bail!("role must be included or explicitly excluded, not both: {role:?}"),
        }
    }
    for role in [Role::Harness, Role::Property, Role::Control] {
        ensure!(represented.contains(&role), "mandatory role absent: {role:?}");
    }
    if spec.claim.binding != ImplementationBinding::ModelOnly {
        ensure!(represented.contains(&Role::Kernel), "production binding has no kernel");
    }
    if spec.claim.binding == ImplementationBinding::LiveConsumerBound {
        for role in [Role::Wrapper, Role::Consumer, Role::EnforcementBoundary, Role::Correspondence] {
            ensure!(represented.contains(&role), "live binding has no {role:?}");
        }
    }
    if represented.contains(&Role::GeneratedSource) {
        ensure!(represented.contains(&Role::GeneratorInput), "generated source lacks generator input");
    }
    ensure!(!spec.toolchain.provider.trim().is_empty() && !spec.toolchain.target.trim().is_empty()
        && !spec.toolchain.components.is_empty(), "missing toolchain boundary");
    for (name, tool) in &spec.toolchain.components {
        ensure!(!name.trim().is_empty() && !tool.version.trim().is_empty()
            && digest_valid(&tool.digest), "invalid tool identity");
    }
    unique_sorted(&mut spec.toolchain.features, "feature")?;
    unique_sorted(&mut spec.toolchain.cfg, "cfg")?;
    unique_sorted(&mut spec.bounds.assumptions, "assumption")?;
    unique_sorted(&mut spec.bounds.fairness, "fairness assumption")?;
    unique_sorted(&mut spec.unknown_inputs, "unknown input")?;
    ensure!(spec.bounds.wall_seconds > 0 && spec.bounds.output_bytes > 0
        && !spec.bounds.values.is_empty(), "missing resource or semantic bounds");
    for (name, value) in &spec.bounds.values {
        ensure!(!name.trim().is_empty() && (value.is_boolean() || value.is_string()
            || value.as_u64().is_some() || value.as_i64().is_some()), "invalid bound value");
    }
    ensure!(!spec.command.is_empty() && spec.command.iter().all(|arg| !arg.is_empty()
        && !arg.contains('\0')), "missing or invalid invocation");
    ensure!(!spec.dependency_model.is_empty(), "dependency model must be explicit");
    for (name, digest) in &spec.dependency_model {
        ensure!(!name.trim().is_empty() && digest_valid(digest), "invalid dependency identity");
    }
    ensure!(spec.environment.keys().all(|key| !key.trim().is_empty()), "blank environment key");
    Ok(spec)
}

/// Canonical JSON independent of serde_json's optional preserve_order feature.
fn canonical(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::Object(map) => {
            let mut bytes = vec![b'{'];
            let sorted: BTreeMap<_, _> = map.iter().collect();
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index != 0 { bytes.push(b','); }
                bytes.extend(serde_json::to_vec(key)?);
                bytes.push(b':');
                bytes.extend(canonical(value)?);
            }
            bytes.push(b'}');
            Ok(bytes)
        }
        Value::Array(values) => {
            let mut bytes = vec![b'['];
            for (index, value) in values.iter().enumerate() {
                if index != 0 { bytes.push(b','); }
                bytes.extend(canonical(value)?);
            }
            bytes.push(b']');
            Ok(bytes)
        }
        _ => Ok(serde_json::to_vec(value)?),
    }
}

fn hash(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().iter().fold(String::from("sha256:"), |mut text, byte| {
        text.push_str(&format!("{byte:02x}"));
        text
    })
}

pub fn closure_digest(members: &BTreeMap<String, String>) -> Result<String> {
    ensure!(!members.is_empty() && members.values().all(|v| digest_valid(v)), "invalid closure map");
    let data = canonical(&serde_json::to_value(members)?)?;
    Ok(hash(b"perl-lsp/formal-declared-closure/v1\0", &[&data]))
}

/// Closure-engine source changes invalidate results even when the spec omits them.
pub const ENGINE: &[(&str, &str)] = &[
    ("xtask/src/formal_closure/mod.rs", include_str!("mod.rs")),
    ("xtask/src/formal_closure/snapshot.rs", include_str!("snapshot.rs")),
    ("xtask/src/formal_closure/unique_json.rs", include_str!("unique_json.rs")),
    ("xtask/src/bin/formal-closure.rs", include_str!("../bin/formal-closure.rs")),
    ("xtask/src/tasks/formal_receipt.rs", include_str!("../tasks/formal_receipt.rs")),
    ("Cargo.toml", include_str!("../../../Cargo.toml")),
    ("Cargo.lock", include_str!("../../../Cargo.lock")),
    ("xtask/Cargo.toml", include_str!("../../Cargo.toml")),
    ("xtask/src/git_environment.rs", include_str!("../git_environment.rs")),
];

pub fn compute(snapshot: &Snapshot, spec: &Spec) -> Result<Computed> {
    let spec = normalize(spec)?;
    let mut entries = BTreeMap::new();
    let mut blobs = BTreeMap::new();
    let mut total = 0usize;
    for member in &spec.members {
        let blob = snapshot.read(&member.path)?;
        total = total.checked_add(blob.bytes.len()).context("closure size overflow")?;
        ensure!(total <= MAX_TOTAL_BYTES, "closure exceeds byte ceiling");
        let roles = canonical(&serde_json::to_value(&member.roles)?)?;
        entries.insert(format!("source/{}", member.path),
            hash(b"perl-lsp/formal-member/v1\0", &[blob.mode.as_bytes(), &roles, &blob.bytes]));
        blobs.insert(member.path.clone(), blob.id);
    }
    for (path, compiled) in ENGINE {
        let blob = snapshot.read(path)?;
        ensure!(blob.bytes == compiled.as_bytes(), "closure binary differs from evaluated engine: {path}");
        total = total.checked_add(blob.bytes.len()).context("closure size overflow")?;
        ensure!(total <= MAX_TOTAL_BYTES, "closure exceeds byte ceiling");
        entries.insert(format!("engine/{path}"),
            hash(b"perl-lsp/formal-engine/v1\0", &[blob.mode.as_bytes(), &blob.bytes]));
        blobs.insert((*path).to_owned(), blob.id);
    }
    let mut profile = serde_json::to_value(&spec)?;
    // Deployment policy is orthogonal to semantics; evidence never gains native
    // authority merely because rollout moves from advisory to required.
    let _ = profile["claim"].as_object_mut().context("claim is not an object")?.remove("rollout");
    entries.insert("profile/semantic-inputs".to_owned(),
        hash(b"perl-lsp/formal-profile/v1\0", &[&canonical(&profile)?]));
    let digest = closure_digest(&entries)?;
    snapshot.finish()?;
    Ok(Computed {
        evidence_class: "declared_input_identity", proof_accepted: false,
        closure_completeness: "not_established",
        declared_roles: spec.members.iter().map(|member|
            (member.path.clone(), member.roles.clone())).collect(),
        source: snapshot.identity(), claim: spec.claim,
        closure: ClosureIdentity { algorithm: ALGORITHM.to_owned(), digest, members: entries },
        input_blobs: blobs, unknown_inputs: spec.unknown_inputs,
        toolchain: spec.toolchain, bounds: spec.bounds,
    })
}

fn result(state: Freshness, reason: &'static str, current: Option<&Computed>) -> Comparison {
    Comparison {
        evidence_class: "declared_input_comparison", proof_accepted: false,
        closure_completeness: "not_established",
        freshness: state, reason, first_difference: None,
        evaluated_commit: current.map(|value| value.source.commit.clone()),
        evaluated_closure: current.map(|value| value.closure.digest.clone()),
        compared_receipt_identity: None,
        correspondence_established: false, correspondence_inputs_unchanged: false,
    }
}

/// Does not mutate or reissue the receipt. Native evidence is never upgraded.
pub fn compare(receipt: &FormalProofReceipt, current: Option<&Computed>) -> Comparison {
    let mut comparison = compare_inputs(receipt, current);
    comparison.compared_receipt_identity = serde_json::to_value(receipt).ok()
        .and_then(|value| canonical(&value).ok())
        .map(|bytes| hash(b"perl-lsp/formal-receipt-input/v1\0", &[&bytes]));
    comparison
}

fn compare_inputs(receipt: &FormalProofReceipt, current: Option<&Computed>) -> Comparison {
    let Ok(value) = serde_json::to_value(receipt) else {
        return result(Freshness::Unknown, "unreadable_receipt", current);
    };
    if !formal_receipt::validate_value(&value).is_empty() {
        return result(Freshness::Unknown, "inconsistent_receipt", current);
    }
    match receipt.disposition.status {
        DispositionStatus::Retired => return result(Freshness::Retired, "explicit_retirement", current),
        DispositionStatus::Superseded => return result(Freshness::Superseded, "explicit_successor", current),
        DispositionStatus::Active => {}
    }
    if receipt.claim.rollout == formal_receipt::RolloutState::Retired {
        return result(Freshness::Unknown, "contradictory_retirement", current);
    }
    if receipt.closure.algorithm != ALGORITHM {
        return result(Freshness::Stale, "algorithm_changed", current);
    }
    if closure_digest(&receipt.closure.members).ok().as_ref() != Some(&receipt.closure.digest) {
        return result(Freshness::Unknown, "recorded_digest_does_not_match_members", current);
    }
    let Some(current) = current else {
        return result(Freshness::Unknown, "current_inputs_unavailable", None);
    };
    if current.closure.algorithm != ALGORITHM || current.source.state != SourceState::Clean
        || current.claim.rollout == formal_receipt::RolloutState::Retired
        || closure_digest(&current.closure.members).ok().as_ref() != Some(&current.closure.digest)
    {
        return result(Freshness::Unknown, "inconsistent_current_identity", Some(current));
    }
    if !current.unknown_inputs.is_empty() || receipt.source.state != SourceState::Clean {
        return result(Freshness::Unknown, "incomplete_or_nonclean_inputs", Some(current));
    }
    let a = &receipt.claim;
    let b = &current.claim;
    if a.id != b.id || a.generation != b.generation || a.owner_issue != b.owner_issue
        || a.profile != b.profile || a.method != b.method || a.binding != b.binding
    {
        return result(Freshness::Stale, "claim_or_binding_changed", Some(current));
    }
    let mut old_tools = receipt.toolchain.clone();
    let mut old_bounds = receipt.bounds.clone();
    for values in [&mut old_tools.features, &mut old_tools.cfg,
        &mut old_bounds.assumptions, &mut old_bounds.fairness] { values.sort(); }
    if old_tools != current.toolchain || old_bounds != current.bounds {
        return result(Freshness::Stale, "receipt_configuration_changed", Some(current));
    }
    if receipt.closure.members != current.closure.members {
        let mut output = result(Freshness::Stale, "semantic_input_changed", Some(current));
        output.first_difference = receipt.closure.members.keys().chain(current.closure.members.keys())
            .collect::<BTreeSet<_>>().into_iter()
            .find(|key| receipt.closure.members.get(*key) != current.closure.members.get(*key))
            .cloned();
        return output;
    }
    let mut output = result(Freshness::Current, "declared_semantic_inputs_equal", Some(current));
    output.correspondence_inputs_unchanged = a.binding != ImplementationBinding::ModelOnly
        && current.declared_roles.values().any(|roles| roles.contains(&Role::Correspondence));
    output
}

#[cfg(test)]
mod tests;
