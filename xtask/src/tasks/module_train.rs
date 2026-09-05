//! Offline current-tree status and safe frontier over the stable `module_train.v1` graph.
//!
//! This is slice one of issue #11626 (train node C02): it consumes
//! `.spec/11625-module-train-graph/train.manifest.json` strictly as DATA and
//! derives, deterministically and offline:
//!
//! * a fail-closed loader: strict schema, structural laws, and a pinned
//!   canonical digest of the manifest (any tampering or un-re-derived
//!   semantic revision fails loudly);
//! * a typed current-tree state projection per node (`status`), keeping
//!   implementation presence, dependency typing, and evidence limitations
//!   independent;
//! * the safe offline parallel frontier (`next`): all and only hard-ready,
//!   role-valid, conflict-recorded leaves.
//!
//! Boundaries honored (C02 claim ceiling): no network, no GitHub state, no
//! scheduling, no agent launch, no product mutation, no support inference.
//! Per-node semantic implementation probes beyond the C01 manifest probe and
//! the `explain`/`graph` static packet projections are recorded residuals of
//! this slice, never guessed (`not_proven` by law).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sha2::Digest as ShaDigest;
use sha2::Sha256;

#[cfg(test)]
#[path = "module_train_tests.rs"]
mod tests;

/// Repository-relative location of the stable module train manifest (C01, #11625).
pub const MANIFEST_RELATIVE_PATH: &str = ".spec/11625-module-train-graph/train.manifest.json";

/// Schema identity consumed by this tool.
pub const SCHEMA_NAME: &str = "module_train.v1";
pub const SCHEMA_VERSION: u64 = 1;

/// C01's published semantic SHA-256 for the current manifest revision
/// (closeout of PR #12043 / merge `7cc48f77`). Recorded here as provenance:
/// it is computed by C01's bundle checker, whose canonicalization sorts with
/// PowerShell's culture-sensitive `Sort-Object`. This tool's canonical
/// digest (below) uses the same recursive walk with byte-ordinal sorting so
/// it is platform-independent; byte-equality between the two sorts is not a
/// contract, the pinned binding is.
pub const C01_SEMANTIC_SHA256_PROVENANCE: &str =
    "9B46B0F8791BECAD62D503DC00AA1F9E993E6D5307DDB94A1CBB25EB80988090";

/// Pinned canonical digest of the current `module_train.v1` manifest revision
/// under this tool's documented ordinal canonicalization. A classified
/// semantic revision of the manifest (route: #11625) must update this pin and
/// re-derive projections — `revision_governance.invalidates` — it can never
/// be patched around silently.
pub const PINNED_CANONICAL_DIGEST: &str =
    "10BA261976BCF7A9BADE7AE94991E4C40ACE4FF88C00BF4EF10E90CD62C104FB";

/// Closed dependency-class vocabulary (`#10858` typed edges).
const DEP_CLASSES: [&str; 4] = ["hard", "evidence", "optional", "external"];

/// Closed writer capacity-class vocabulary (ceilings and semantic groupings,
/// never quotas).
const WRITER_GROUPS: [&str; 5] = ["A", "B", "C", "D", "none"];

/// Roles that are grouping, composition, or authorization surfaces and can
/// never appear as implementation starts in the offline frontier.
const ROLE_NEVER_FRONTIER: [&str; 4] = ["controller", "fan_in", "external_gate", "claim"];

// ---------------------------------------------------------------------------
// Typed manifest model (strict: unknown keys are rejected, mirroring C01's
// exact key-set laws; a schema change therefore fails closed here too).
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    schema_version: u64,
    programme: Programme,
    authority_planes: Vec<AuthorityPlane>,
    train_role_vocabulary: Vec<RoleEntry>,
    evidence_semantics: EvidenceSemantics,
    external_authorities: Vec<ExternalAuthority>,
    open_decisions_routed_elsewhere: Vec<OpenDecision>,
    case_work_packet_bindings: CaseBindings,
    claim_profiles: Vec<ClaimProfile>,
    cross_programme_imports: Vec<CrossImport>,
    nodes: Vec<TrainNode>,
    limitations: Vec<String>,
    revision_governance: RevisionGovernance,
    /// Unexercised in the current revision (empty). Kept as raw values: this
    /// slice defines no supersession projection, so a populated list fails
    /// closed instead of guessing a state transition.
    supersessions: Vec<Value>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Programme {
    parent_programme_issue: u64,
    controller_issue: u64,
    evidence_controller_issue: u64,
    home_programme: String,
    method_authority: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct AuthorityPlane {
    plane: String,
    owns: String,
    never_substitutes: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct RoleEntry {
    role: String,
    owns: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EvidenceSemantics {
    not_proven_law: String,
    optional_visibility: String,
    metadata_only_rule: String,
    issue_identity_rule: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ExternalAuthority {
    id: String,
    subject: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct OpenDecision {
    id: String,
    owner: String,
    subject: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct CaseBindings {
    status: String,
    law: String,
    binding_nodes: Vec<String>,
    consumers: Vec<String>,
    evidence_authority: String,
    promotion_route: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ClaimProfile {
    id: String,
    definition: String,
    members: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct CrossImport {
    authority: String,
    home_train: String,
    relation: String,
    note: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct RevisionGovernance {
    owner_node: String,
    owner_issue: u64,
    invalidates: String,
    never: String,
    metadata_only: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct TrainNode {
    node_id: String,
    issue: u64,
    title: String,
    title_fingerprint: String,
    aliases: Vec<String>,
    train_role: String,
    lane: String,
    chain: Chain,
    one_pr_outcome: String,
    authority_before: String,
    authority_after: String,
    buildable: bool,
    dependencies: Vec<Dep>,
    claim_ceiling: String,
    writer: Writer,
    consumed_authorities: Vec<String>,
    allowed_components: Vec<String>,
    forbidden_adjacent_owners: Vec<String>,
    spec: NodeSpec,
    first_falsifier: String,
    controls: Controls,
    proof: NodeProof,
    review_forward: ReviewForward,
    obligations: Obligations,
    exits: Exits,
    rollback: Rollback,
    successors: Vec<String>,
    identity_fields: Vec<String>,
    limitations: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Chain {
    home: String,
    controller: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Dep {
    target: String,
    class: String,
    provenance: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Writer {
    conflict_key: String,
    parallel_group: String,
    stack_relation: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct NodeSpec {
    disposition: String,
    owner: String,
    stale_policy: String,
    spec_authority: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Controls {
    positive: String,
    opposite: String,
    stale: String,
    wrong_subject: String,
    fault: String,
    mutation: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct NodeProof {
    focused: String,
    routed: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ReviewForward {
    questions: Vec<String>,
    lenses: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Obligations {
    schema: String,
    generated: String,
    docs: String,
    changelog: String,
    receipt: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Exits {
    old_path: String,
    compatibility: String,
    supersession: String,
    transfer: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Rollback {
    rollback: String,
    return_to_issue: String,
    not_proven: String,
    stop: String,
}

// ---------------------------------------------------------------------------
// Canonical digest: recursive content walk, order-invariant (sorted object
// keys and sorted array parts), byte-ordinal sorting, SHA-256 uppercase hex.
// Mirrors C01's walk shape (null `n;`, bool `b:True;`, integer `i:<n>;`,
// escaped `s:...;`, arrays `[...]`, objects `{k=...}`) so the two stay
// comparable in shape while this one is platform-independent.
// ---------------------------------------------------------------------------

pub fn canonical_digest(value: &Value) -> Result<String> {
    let mut canonical = String::new();
    canonical_walk(value, &mut canonical)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02X}");
    }
    Ok(hex)
}

fn canonical_walk(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("n;"),
        Value::Bool(flag) => {
            let _ = write!(out, "b:{};", if *flag { "True" } else { "False" });
        }
        Value::Number(number) => {
            // The manifest defines integers only; anything else is a schema
            // drift this canonicalization refuses to guess a rendering for.
            if let Some(signed) = number.as_i64() {
                let _ = write!(out, "i:{signed};");
            } else if let Some(unsigned) = number.as_u64() {
                let _ = write!(out, "i:{unsigned};");
            } else {
                bail!("manifest canonicalization defines integers only; found {number}");
            }
        }
        Value::String(text) => {
            out.push_str("s:");
            for ch in text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    ';' => out.push_str("\\;"),
                    _ => out.push(ch),
                }
            }
            out.push(';');
        }
        Value::Array(items) => {
            let mut parts: Vec<String> = Vec::with_capacity(items.len());
            for item in items {
                let mut inner = String::new();
                canonical_walk(item, &mut inner)?;
                parts.push(inner);
            }
            parts.sort();
            out.push('[');
            for part in &parts {
                out.push_str(part);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for key in keys {
                out.push_str(key);
                out.push('=');
                canonical_walk(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// C01 title fingerprint law: first 16 uppercase hex characters of the
/// SHA-256 of the exact title bytes.
fn title_fingerprint(title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for byte in digest.iter().take(8) {
        let _ = write!(hex, "{byte:02X}");
    }
    hex
}

// ---------------------------------------------------------------------------
// Loading: read bytes, parse to a full value tree (digest input), pin-check,
// then strictly deserialize and enforce structural laws.
// ---------------------------------------------------------------------------

/// A loaded, digest-pinned, structurally validated manifest. Fields are
/// module-private: consumers go through the render/CLI functions so the
/// digest binding cannot be bypassed.
pub struct LoadedManifest {
    manifest: Manifest,
    canonical_digest: String,
}

/// Bounded static facts of one train node, exposed for the C03 live join
/// (#11627). This is an additive read-only accessor over already-validated
/// manifest data: it changes no C02 semantics and introduces no second
/// topology authority.
#[derive(Debug, Clone)]
pub struct NodeStaticFact {
    pub node_id: String,
    pub issue: u64,
    pub role: String,
    pub lane: String,
    pub chain_home: String,
    pub chain_controller: String,
    pub one_pr_outcome: String,
    pub claim_ceiling: String,
    pub first_falsifier: String,
    pub rollback_stop: String,
    pub buildable: bool,
    pub conflict_key: String,
    pub parallel_group: String,
    pub stack_relation: String,
    pub aliases: Vec<String>,
    /// Typed dependencies as `(target, class)` pairs (node ids or `#authority`
    /// references), in manifest order.
    pub dependencies: Vec<(String, String)>,
}

impl LoadedManifest {
    /// Project every node into its typed current-tree state (the same
    /// deterministic projection the `status`/`next` commands render).
    /// Additive seam for the #11627 live join; no semantic change to C02.
    pub fn node_statuses(&self) -> Result<Vec<NodeStatus>> {
        project_states(&self.manifest)
    }

    /// Bounded static facts for the #11627 live explain addendum.
    pub fn node_static_facts(&self) -> Vec<NodeStaticFact> {
        self.manifest
            .nodes
            .iter()
            .map(|node| NodeStaticFact {
                node_id: node.node_id.clone(),
                issue: node.issue,
                role: node.train_role.clone(),
                lane: node.lane.clone(),
                chain_home: node.chain.home.clone(),
                chain_controller: node.chain.controller.clone(),
                one_pr_outcome: node.one_pr_outcome.clone(),
                claim_ceiling: node.claim_ceiling.clone(),
                first_falsifier: node.first_falsifier.clone(),
                rollback_stop: node.rollback.stop.clone(),
                buildable: node.buildable,
                conflict_key: node.writer.conflict_key.clone(),
                parallel_group: node.writer.parallel_group.clone(),
                stack_relation: node.writer.stack_relation.clone(),
                aliases: node.aliases.clone(),
                dependencies: node
                    .dependencies
                    .iter()
                    .map(|dep| (dep.target.clone(), dep.class.clone()))
                    .collect(),
            })
            .collect()
    }

    /// The manifest's controller nodes, used to validate the identity block's
    /// `Parent/controller` trailer against the declared chain.
    pub fn controller_issue(&self, controller_node_id: &str) -> Option<u64> {
        self.manifest
            .nodes
            .iter()
            .find(|node| node.node_id == controller_node_id)
            .map(|node| node.issue)
    }
}

/// Load the workspace manifest, verifying the pinned digest binding.
pub fn load_manifest() -> Result<LoadedManifest> {
    let root = crate::utils::project_root()?;
    let path: PathBuf = root.join(MANIFEST_RELATIVE_PATH);
    load_manifest_from(&path)
}

/// Load a manifest from an explicit path (used by tests and fixtures).
pub fn load_manifest_from(path: &std::path::Path) -> Result<LoadedManifest> {
    let bytes = std::fs::read(path).with_context(|| {
        format!("failed to read module_train.v1 manifest at {}", path.display())
    })?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("manifest at {} is not valid JSON", path.display()))?;
    let digest = canonical_digest(&value)?;
    if digest != PINNED_CANONICAL_DIGEST {
        bail!(
            "module_train.v1 manifest digest drift at {}: computed {digest} but pinned {PINNED_CANONICAL_DIGEST}; \
             a semantic revision must be classified through #11625 and this projection re-derived",
            path.display()
        );
    }
    let manifest: Manifest = serde_json::from_value(value).with_context(|| {
        format!("manifest at {} violates the strict module_train.v1 schema", path.display())
    })?;
    validate_manifest(&manifest).with_context(|| {
        format!("manifest at {} fails module_train.v1 structural laws", path.display())
    })?;
    Ok(LoadedManifest { manifest, canonical_digest: digest })
}

// ---------------------------------------------------------------------------
// Structural laws (generic, data-derived; the node set itself is never
// frozen here — the digest pin carries revision identity).
// ---------------------------------------------------------------------------

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema != SCHEMA_NAME {
        bail!("schema name mismatch: expected {SCHEMA_NAME}, found {}", manifest.schema);
    }
    if manifest.schema_version != SCHEMA_VERSION {
        bail!(
            "schema_version mismatch: expected {SCHEMA_VERSION}, found {}",
            manifest.schema_version
        );
    }
    if manifest.nodes.is_empty() {
        bail!("manifest carries no nodes");
    }

    // Programme block: non-empty identities only (their exact values are
    // reviewed topology data, not something this loader freezes).
    if manifest.programme.parent_programme_issue == 0
        || manifest.programme.controller_issue == 0
        || manifest.programme.evidence_controller_issue == 0
    {
        bail!("programme issue identities must be positive");
    }
    if manifest.programme.home_programme.trim().is_empty()
        || manifest.programme.method_authority.trim().is_empty()
    {
        bail!("programme home/method authority must be non-empty");
    }

    // Authority planes: unique, fully worded planes.
    let mut plane_names: BTreeSet<&str> = BTreeSet::new();
    for plane in &manifest.authority_planes {
        if !plane_names.insert(plane.plane.as_str()) {
            bail!("duplicate authority plane: {}", plane.plane);
        }
        if plane.owns.trim().is_empty() || plane.never_substitutes.trim().is_empty() {
            bail!("authority plane {} carries an empty law", plane.plane);
        }
    }
    if plane_names.is_empty() {
        bail!("manifest carries no authority planes");
    }

    // Evidence semantics: the four laws must be worded.
    for (name, law) in [
        ("not_proven_law", &manifest.evidence_semantics.not_proven_law),
        ("optional_visibility", &manifest.evidence_semantics.optional_visibility),
        ("metadata_only_rule", &manifest.evidence_semantics.metadata_only_rule),
        ("issue_identity_rule", &manifest.evidence_semantics.issue_identity_rule),
    ] {
        if law.trim().is_empty() {
            bail!("evidence_semantics.{name} is empty");
        }
    }

    // Open decisions: unique, fully routed.
    let mut decision_ids: BTreeSet<&str> = BTreeSet::new();
    for decision in &manifest.open_decisions_routed_elsewhere {
        if !decision_ids.insert(decision.id.as_str()) {
            bail!("duplicate open decision: {}", decision.id);
        }
        if decision.owner.trim().is_empty() || decision.subject.trim().is_empty() {
            bail!("open decision {} carries an empty owner or subject", decision.id);
        }
    }

    // Top-level limitations must be worded.
    for limitation in &manifest.limitations {
        if limitation.trim().is_empty() {
            bail!("top-level limitations carry an empty entry");
        }
    }

    let vocabulary: BTreeSet<&str> =
        manifest.train_role_vocabulary.iter().map(|entry| entry.role.as_str()).collect();
    if vocabulary.is_empty() {
        bail!("train role vocabulary is empty");
    }
    for entry in &manifest.train_role_vocabulary {
        if entry.owns.trim().is_empty() {
            bail!("train role {} carries an empty ownership law", entry.role);
        }
    }

    let authority_ids: BTreeSet<&str> =
        manifest.external_authorities.iter().map(|entry| entry.id.as_str()).collect();
    if authority_ids.len() != manifest.external_authorities.len() {
        bail!("duplicate external authority id");
    }
    for authority in &manifest.external_authorities {
        if !authority.id.starts_with('#') {
            bail!("external authority id must start with '#': {}", authority.id);
        }
        if authority.subject.trim().is_empty() {
            bail!("external authority {} carries an empty subject", authority.id);
        }
    }

    // Import relation fixes the class of any edge that targets the authority.
    let mut import_relation: BTreeMap<&str, &str> = BTreeMap::new();
    for import in &manifest.cross_programme_imports {
        if import_relation.insert(import.authority.as_str(), import.relation.as_str()).is_some() {
            bail!("duplicate cross-programme import authority: {}", import.authority);
        }
        if import.home_train.trim().is_empty() || import.note.trim().is_empty() {
            bail!("cross-programme import {} is incompletely worded", import.authority);
        }
    }

    let by_id: BTreeMap<&str, &TrainNode> =
        manifest.nodes.iter().map(|node| (node.node_id.as_str(), node)).collect();
    if by_id.len() != manifest.nodes.len() {
        bail!("duplicate node_id in manifest");
    }

    // Revision governance: the named owner must be a real node; the
    // invalidation law must be worded (this loader's digest pin is the C02
    // side of exactly that law).
    if !by_id.contains_key(manifest.revision_governance.owner_node.as_str()) {
        bail!(
            "revision governance owner node {} does not exist",
            manifest.revision_governance.owner_node
        );
    }
    if manifest.revision_governance.owner_issue == 0
        || manifest.revision_governance.invalidates.trim().is_empty()
        || manifest.revision_governance.never.trim().is_empty()
        || manifest.revision_governance.metadata_only.trim().is_empty()
    {
        bail!("revision governance block is incompletely worded");
    }

    let mut seen_issues: BTreeSet<u64> = BTreeSet::new();
    let mut seen_conflict_keys: BTreeSet<&str> = BTreeSet::new();
    let mut seen_authority_after: BTreeSet<&str> = BTreeSet::new();
    for node in &manifest.nodes {
        validate_node_wording(node)?;
        if !seen_issues.insert(node.issue) {
            bail!("duplicate issue identity at {}: #{}", node.node_id, node.issue);
        }
        if !seen_conflict_keys.insert(node.writer.conflict_key.as_str()) {
            bail!(
                "duplicate writer conflict key at {}: {}",
                node.node_id,
                node.writer.conflict_key
            );
        }
        if !seen_authority_after.insert(node.authority_after.as_str()) {
            bail!(
                "duplicate authority_after proposition at {}: {}",
                node.node_id,
                node.authority_after
            );
        }
        if !vocabulary.contains(node.train_role.as_str()) {
            bail!("unknown train role at {}: {}", node.node_id, node.train_role);
        }
        if !WRITER_GROUPS.contains(&node.writer.parallel_group.as_str()) {
            bail!(
                "unknown writer capacity class at {}: {}",
                node.node_id,
                node.writer.parallel_group
            );
        }
        if node.title_fingerprint != title_fingerprint(&node.title) {
            bail!("title fingerprint mismatch at {}", node.node_id);
        }

        // Role/buildable law: grouping, fan-in and gate surfaces are never
        // one-PR builder propositions.
        let grouping_role =
            matches!(node.train_role.as_str(), "controller" | "fan_in" | "external_gate");
        if grouping_role == node.buildable {
            bail!(
                "role/buildable law broken at {}: role {} with buildable={}",
                node.node_id,
                node.train_role,
                node.buildable
            );
        }

        let mut dep_targets: BTreeSet<&str> = BTreeSet::new();
        for dep in &node.dependencies {
            if dep.provenance.trim().is_empty() {
                bail!("edge {} -> {} carries an empty provenance", node.node_id, dep.target);
            }
            if !DEP_CLASSES.contains(&dep.class.as_str()) {
                bail!(
                    "unknown dependency class at {} -> {}: {}",
                    node.node_id,
                    dep.target,
                    dep.class
                );
            }
            if !dep_targets.insert(dep.target.as_str()) {
                bail!("more than one dependency edge to target {} at {}", dep.target, node.node_id);
            }
            if dep.target.starts_with('#') {
                if !authority_ids.contains(dep.target.as_str()) {
                    bail!(
                        "{} depends on unknown authority {} at {}",
                        node.node_id,
                        dep.target,
                        node.node_id
                    );
                }
                if dep.target == "#EXPLICIT-AUTHORIZATION" {
                    if dep.class != "external" {
                        bail!(
                            "#EXPLICIT-AUTHORIZATION must be external-class at {}, found {}",
                            node.node_id,
                            dep.class
                        );
                    }
                    if node.train_role != "external_gate" {
                        bail!("#EXPLICIT-AUTHORIZATION carried by non-gate node {}", node.node_id);
                    }
                }
                if let Some(relation) = import_relation.get(dep.target.as_str()) {
                    let expected = match *relation {
                        "hard import" => Some("hard"),
                        "evidence import" => Some("evidence"),
                        _ => None,
                    };
                    match expected {
                        Some(class) if dep.class != class => {
                            bail!(
                                "import relation {} requires class {} but found {} at {} -> {}",
                                relation,
                                class,
                                dep.class,
                                node.node_id,
                                dep.target
                            );
                        }
                        None => {
                            bail!(
                                "non-edge import {} (relation {}) carries a dependency edge at {}",
                                dep.target,
                                relation,
                                node.node_id
                            );
                        }
                        _ => {}
                    }
                }
            } else {
                if dep.target == node.node_id {
                    bail!("self-dependency at {}", node.node_id);
                }
                if !by_id.contains_key(dep.target.as_str()) {
                    bail!("{} depends on unknown node {}", node.node_id, dep.target);
                }
            }
        }
    }

    // Successors must be exactly the derived reverse node-edge set.
    let mut derived: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for id in by_id.keys() {
        derived.insert(id, BTreeSet::new());
    }
    for node in &manifest.nodes {
        for dep in &node.dependencies {
            if !dep.target.starts_with('#') {
                derived
                    .get_mut(dep.target.as_str())
                    .ok_or_else(|| {
                        color_eyre::eyre::eyre!("unknown dependency target {}", dep.target)
                    })?
                    .insert(node.node_id.as_str());
            }
        }
    }
    for node in &manifest.nodes {
        let actual: BTreeSet<&str> = node.successors.iter().map(String::as_str).collect();
        let want = derived.get(node.node_id.as_str()).ok_or_else(|| {
            color_eyre::eyre::eyre!("node {} missing from derived map", node.node_id)
        })?;
        if actual != *want {
            bail!(
                "successor set mismatch at {}: manifest=[{}] derived=[{}]",
                node.node_id,
                node.successors.join(","),
                want.iter().copied().collect::<Vec<_>>().join(",")
            );
        }
    }

    // Acyclicity over hard/evidence node edges.
    let mut colour: BTreeMap<&str, u8> = BTreeMap::new();
    for id in by_id.keys() {
        colour.insert(id, 0);
    }
    for id in by_id.keys().copied().collect::<Vec<_>>() {
        visit_acyclic(id, &by_id, &mut colour)?;
    }

    // Case/work-packet binding law: consumers and binding nodes must be
    // known nodes; the status is a non-empty disposition.
    if manifest.case_work_packet_bindings.status.trim().is_empty() {
        bail!("case_work_packet_bindings.status is empty");
    }
    for (field, value) in [
        ("law", &manifest.case_work_packet_bindings.law),
        ("evidence_authority", &manifest.case_work_packet_bindings.evidence_authority),
        ("promotion_route", &manifest.case_work_packet_bindings.promotion_route),
    ] {
        if value.trim().is_empty() {
            bail!("case_work_packet_bindings.{field} is empty");
        }
    }
    for consumer in manifest
        .case_work_packet_bindings
        .consumers
        .iter()
        .chain(manifest.case_work_packet_bindings.binding_nodes.iter())
    {
        if !by_id.contains_key(consumer.as_str()) {
            bail!("case/work-packet binding references unknown node {consumer}");
        }
    }

    // Claim profiles: unique ids, known members.
    let mut profile_ids: BTreeSet<&str> = BTreeSet::new();
    for profile in &manifest.claim_profiles {
        if !profile_ids.insert(profile.id.as_str()) {
            bail!("duplicate claim profile id: {}", profile.id);
        }
        if profile.definition.trim().is_empty() {
            bail!("claim profile {} carries an empty definition", profile.id);
        }
        for member in &profile.members {
            if !by_id.contains_key(member.as_str()) {
                bail!("claim profile {} references unknown node {member}", profile.id);
            }
        }
    }

    Ok(())
}

/// Worded-contract laws, mirroring C01's Assert-NonEmpty checker pass: every
/// declared string law must actually be worded, every declared list must
/// carry real entries where the contract requires them, spec ownership is
/// the node itself, and consumed authorities stay issue references.
fn validate_node_wording(node: &TrainNode) -> Result<()> {
    let id = node.node_id.as_str();
    let non_empty = |field: &str, value: &str| -> Result<()> {
        if value.trim().is_empty() {
            bail!("node {id} carries an empty {field}");
        }
        Ok(())
    };
    non_empty("lane", &node.lane)?;
    non_empty("one_pr_outcome", &node.one_pr_outcome)?;
    non_empty("authority_before", &node.authority_before)?;
    non_empty("claim_ceiling", &node.claim_ceiling)?;
    non_empty("first_falsifier", &node.first_falsifier)?;
    non_empty("chain.home", &node.chain.home)?;
    non_empty("chain.controller", &node.chain.controller)?;
    non_empty("writer.stack_relation", &node.writer.stack_relation)?;
    non_empty("spec.disposition", &node.spec.disposition)?;
    non_empty("spec.stale_policy", &node.spec.stale_policy)?;
    non_empty("spec.spec_authority", &node.spec.spec_authority)?;
    if node.spec.owner != id {
        bail!("node {id} spec owner must be itself, found {}", node.spec.owner);
    }
    if !node.spec.spec_authority.starts_with('#') {
        bail!("node {id} spec authority must be an issue reference");
    }
    for (field, value) in [
        ("controls.positive", &node.controls.positive),
        ("controls.opposite", &node.controls.opposite),
        ("controls.stale", &node.controls.stale),
        ("controls.wrong_subject", &node.controls.wrong_subject),
        ("controls.fault", &node.controls.fault),
        ("controls.mutation", &node.controls.mutation),
        ("proof.focused", &node.proof.focused),
        ("proof.routed", &node.proof.routed),
        ("obligations.schema", &node.obligations.schema),
        ("obligations.generated", &node.obligations.generated),
        ("obligations.docs", &node.obligations.docs),
        ("obligations.changelog", &node.obligations.changelog),
        ("obligations.receipt", &node.obligations.receipt),
        ("exits.old_path", &node.exits.old_path),
        ("exits.compatibility", &node.exits.compatibility),
        ("exits.supersession", &node.exits.supersession),
        ("exits.transfer", &node.exits.transfer),
        ("rollback.rollback", &node.rollback.rollback),
        ("rollback.return_to_issue", &node.rollback.return_to_issue),
        ("rollback.not_proven", &node.rollback.not_proven),
        ("rollback.stop", &node.rollback.stop),
    ] {
        non_empty(field, value)?;
    }
    for alias in &node.aliases {
        non_empty("alias", alias)?;
    }
    for authority in &node.consumed_authorities {
        if !authority.starts_with('#') {
            bail!("node {id} consumes a non-issue authority: {authority}");
        }
    }
    for (field, entries) in [
        ("review_forward.questions", &node.review_forward.questions),
        ("review_forward.lenses", &node.review_forward.lenses),
        ("identity_fields", &node.identity_fields),
        ("allowed_components", &node.allowed_components),
        ("forbidden_adjacent_owners", &node.forbidden_adjacent_owners),
    ] {
        if entries.is_empty() {
            bail!("node {id} carries an empty {field} list");
        }
        for entry in entries.iter() {
            non_empty(field, entry)?;
        }
    }
    for limitation in &node.limitations {
        non_empty("limitation", limitation)?;
    }
    Ok(())
}

fn visit_acyclic<'a>(
    id: &'a str,
    by_id: &'a BTreeMap<&'a str, &'a TrainNode>,
    colour: &mut BTreeMap<&'a str, u8>,
) -> Result<()> {
    match colour.get(id) {
        Some(1) => bail!("dependency cycle detected at {id}"),
        Some(2) => return Ok(()),
        _ => {}
    }
    colour.insert(id, 1);
    if let Some(node) = by_id.get(id) {
        for dep in &node.dependencies {
            if !dep.target.starts_with('#') && matches!(dep.class.as_str(), "hard" | "evidence") {
                visit_acyclic(dep.target.as_str(), by_id, colour)?;
            }
        }
    }
    colour.insert(id, 2);
    Ok(())
}

// ---------------------------------------------------------------------------
// Current-tree state projection (offline; typed vocabulary from #11626).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTreeState {
    LandedCurrentTree,
    Ready,
    BlockedHard,
    BlockedEvidence,
    BlockedExternalOrAuthorization,
    /// Part of the #11626 state vocabulary: a partial implementation whose
    /// retirement/negative selectors are unmet. Not derivable until per-node
    /// semantic probes exist (recorded residual of this slice).
    #[allow(dead_code)] // vocabulary completeness; no probe can produce it yet
    IncompleteCurrentTree,
    /// Part of the #11626 state vocabulary: supersession is manifest data;
    /// this slice defines no projection and fails closed on a populated list.
    #[allow(dead_code)] // vocabulary completeness; no projection is defined yet
    Superseded,
    NotProven,
}

impl CurrentTreeState {
    /// Public label (stable vocabulary; consumed by the #11627 live join).
    pub fn as_str(self) -> &'static str {
        match self {
            CurrentTreeState::LandedCurrentTree => "landed_current_tree",
            CurrentTreeState::Ready => "ready",
            CurrentTreeState::BlockedHard => "blocked_hard",
            CurrentTreeState::BlockedEvidence => "blocked_evidence",
            CurrentTreeState::BlockedExternalOrAuthorization => "blocked_external_or_authorization",
            CurrentTreeState::IncompleteCurrentTree => "incomplete_current_tree",
            CurrentTreeState::Superseded => "superseded",
            CurrentTreeState::NotProven => "not_proven",
        }
    }
}

/// Implementation-presence probe outcome, kept independent from the frontier
/// state: a node may be ready while its implementation presence is not
/// proven, and landed work may still carry unproven evidence obligations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The node's declared positive surface is verified on this tree.
    Pass,
    /// No semantic probe is defined for this node in this slice; presence is
    /// `not_proven` by law, never guessed from names or file existence.
    Absent,
}

/// Slice-one probe registry: exactly one real probe exists. C01's declared
/// implementation IS the validated `module_train.v1` data bundle, so the
/// loaded manifest at the pinned digest is its positive surface. Every other
/// node's semantic probe is a recorded residual of this slice.
fn slice_probe(node: &TrainNode) -> ProbeOutcome {
    if node.node_id == "C01" && node.issue == 11625 {
        ProbeOutcome::Pass
    } else {
        ProbeOutcome::Absent
    }
}

#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub node_id: String,
    pub issue: u64,
    pub role: String,
    pub lane: String,
    pub state: CurrentTreeState,
    pub implementation_presence: ProbeOutcome,
    pub conflict_key: String,
    pub parallel_group: String,
    /// Typed, sorted, deduplicated reason codes. Blocking reasons and
    /// visibility-only limitations (evidence/optional deps that stay
    /// non-blocking by class) are recorded side by side.
    pub reasons: Vec<String>,
}

/// Project every stable node into its typed current-tree state.
fn project_states(manifest: &Manifest) -> Result<Vec<NodeStatus>> {
    // Supersession projection is intentionally undefined in this slice; an
    // unexercised vocabulary must fail closed rather than guess.
    if !manifest.supersessions.is_empty() {
        bail!(
            "supersessions list is populated but this C02 slice defines no supersession projection; re-derive via #11626"
        );
    }

    let by_id: BTreeMap<&str, &TrainNode> =
        manifest.nodes.iter().map(|node| (node.node_id.as_str(), node)).collect();

    let landed: BTreeSet<&str> = manifest
        .nodes
        .iter()
        .filter(|node| slice_probe(node) == ProbeOutcome::Pass)
        .map(|node| node.node_id.as_str())
        .collect();

    let mut statuses: Vec<NodeStatus> = Vec::with_capacity(manifest.nodes.len());
    for node in &manifest.nodes {
        let id = node.node_id.as_str();
        let mut reasons: BTreeSet<String> = BTreeSet::new();
        let probe = slice_probe(node);

        let state = if probe == ProbeOutcome::Pass {
            CurrentTreeState::LandedCurrentTree
        } else {
            let frontier_eligible =
                node.buildable && !ROLE_NEVER_FRONTIER.contains(&node.train_role.as_str());
            if !frontier_eligible {
                reasons.insert(format!("role_never_implementation_start:{}", node.train_role));
                if !node.buildable {
                    reasons.insert("not_a_one_pr_builder_proposition".to_string());
                }
                CurrentTreeState::NotProven
            } else {
                let mut hard_blocks: Vec<String> = Vec::new();
                let mut evidence_blocks: Vec<String> = Vec::new();
                let mut external_blocks: Vec<String> = Vec::new();

                for dep in &node.dependencies {
                    let target = dep.target.as_str();
                    match dep.class.as_str() {
                        "hard" => {
                            if target.starts_with('#') {
                                // Cross-programme hard import: this train's
                                // offline data cannot establish the home
                                // train's current-tree state; a per-authority
                                // probe is a recorded residual.
                                reasons.insert(format!(
                                    "hard_dep_cross_programme_state_not_establishable:{target}"
                                ));
                                hard_blocks.push(format!("hard_dep_not_landed:{target}"));
                            } else if let Some(target_node) = by_id.get(target) {
                                if target_node.train_role == "controller" {
                                    // Manifest law (limitations[1]): controller
                                    // family dependencies are satisfied
                                    // transitively; controllers never gate
                                    // builders directly.
                                } else if !landed.contains(target) {
                                    hard_blocks.push(format!("hard_dep_not_landed:{target}"));
                                }
                            } else {
                                bail!(
                                    "dependency target {target} vanished between validation and projection"
                                );
                            }
                        }
                        "evidence" => {
                            let satisfied = if target.starts_with('#') {
                                false
                            } else if let Some(target_node) = by_id.get(target) {
                                target_node.train_role == "controller" || landed.contains(target)
                            } else {
                                false
                            };
                            if !satisfied {
                                // Visible limitation, never a hard blocker
                                // unless this node's own contract consumes it
                                // as a required case/spec input (below).
                                reasons.insert(format!("evidence_dep_not_current:{target}"));
                            }
                        }
                        "optional" => {
                            // Same satisfaction test as the evidence arm:
                            // optional and evidence edges are both
                            // visibility-only currentness reasons, so a
                            // landed-or-controller target must satisfy both
                            // identically.
                            let satisfied = if target.starts_with('#') {
                                false
                            } else if let Some(target_node) = by_id.get(target) {
                                target_node.train_role == "controller" || landed.contains(target)
                            } else {
                                false
                            };
                            if !satisfied {
                                reasons.insert(format!("optional_dep_not_current:{target}"));
                            }
                        }
                        "external" => {
                            external_blocks
                                .push(format!("external_authorization_not_granted:{target}"));
                        }
                        _ => {
                            bail!("unhandled dependency class {} at {id}", dep.class);
                        }
                    }
                }

                // Required case/spec input currentness: consumers of the
                // case/work-packet binding treat it as structurally pending,
                // never satisfied, until a classified revision binds it.
                if manifest.case_work_packet_bindings.consumers.iter().any(|c| c == id)
                    && manifest.case_work_packet_bindings.status != "bound"
                {
                    evidence_blocks.push(format!(
                        "case_work_packet_binding:{}",
                        manifest.case_work_packet_bindings.status
                    ));
                }

                // Typed effects stay visible independently of which state
                // wins: a hard-blocked node still shows its pending evidence
                // obligations and authorization gates.
                for reason in
                    hard_blocks.iter().chain(evidence_blocks.iter()).chain(external_blocks.iter())
                {
                    reasons.insert(reason.clone());
                }
                if !hard_blocks.is_empty() {
                    CurrentTreeState::BlockedHard
                } else if !evidence_blocks.is_empty() {
                    CurrentTreeState::BlockedEvidence
                } else if !external_blocks.is_empty() {
                    CurrentTreeState::BlockedExternalOrAuthorization
                } else {
                    CurrentTreeState::Ready
                }
            }
        };

        statuses.push(NodeStatus {
            node_id: node.node_id.to_string(),
            issue: node.issue,
            role: node.train_role.clone(),
            lane: node.lane.clone(),
            state,
            implementation_presence: probe,
            conflict_key: node.writer.conflict_key.clone(),
            parallel_group: node.writer.parallel_group.clone(),
            reasons: reasons.into_iter().collect(),
        });
    }

    // Deterministic canonical presentation order (presentation only; it is
    // not a hidden total-order dependency).
    statuses.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    Ok(statuses)
}

// ---------------------------------------------------------------------------
// Exact-tree binding (offline git facts only).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeBinding {
    pub tree_head: String,
    pub dirty_paths: usize,
    pub manifest_dirty: bool,
}

fn git_output(root: &std::path::Path, args: &[&str]) -> Result<String> {
    let output =
        std::process::Command::new("git").args(args).current_dir(root).output().with_context(
            || format!("failed to spawn git {} in {}", args.join(" "), root.display()),
        )?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8(output.stdout)
        .with_context(|| format!("git {} produced non-UTF-8 output", args.join(" ")))?
        .trim()
        .to_string())
}

/// Resolve the exact-tree binding. This slice binds the current checkout at
/// `HEAD` only; checking out arbitrary trees is a recorded residual.
///
/// Git facts are always resolved inside the repository root the manifest was
/// loaded from, never the ambient process working directory: a caller
/// invoking the binary from elsewhere must never get a foreign `HEAD` bound
/// to this manifest.
pub fn tree_binding(tree: &str) -> Result<TreeBinding> {
    if tree != "HEAD" {
        bail!(
            "this C02 slice binds --tree HEAD only (found {tree}); \
             arbitrary-tree checkout support is a recorded residual of #11626"
        );
    }
    let root = crate::utils::project_root()?;
    let tree_head = git_output(&root, &["rev-parse", "HEAD"])?;
    let status = git_output(&root, &["status", "--porcelain"])?;
    let dirty_paths = status.lines().filter(|line| !line.trim().is_empty()).count();
    let manifest_status =
        git_output(&root, &["status", "--porcelain", "--", MANIFEST_RELATIVE_PATH])?;
    Ok(TreeBinding { tree_head, dirty_paths, manifest_dirty: !manifest_status.trim().is_empty() })
}

// ---------------------------------------------------------------------------
// Rendering (deterministic: sorted rows, no timestamps, no ambient paths).
// ---------------------------------------------------------------------------

fn render_binding(binding: &TreeBinding, loaded: &LoadedManifest) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "tree_head: {}", binding.tree_head);
    let _ = writeln!(
        out,
        "worktree: {}",
        if binding.dirty_paths == 0 {
            "clean".to_string()
        } else {
            format!("dirty:{}paths", binding.dirty_paths)
        }
    );
    // The odd-looking `dirty:<n>paths` form above is kept stable for
    // byte-deterministic output; do not reword without a re-derivation note.
    let _ = writeln!(
        out,
        "manifest_state: {}",
        if binding.manifest_dirty { "dirty" } else { "committed" }
    );
    let _ = writeln!(
        out,
        "schema: {} (version {})",
        loaded.manifest.schema, loaded.manifest.schema_version
    );
    let _ = writeln!(out, "canonical_digest: {}", loaded.canonical_digest);
    let _ = writeln!(out, "digest_binding: pinned={PINNED_CANONICAL_DIGEST} match=yes");
    let _ = writeln!(out, "c01_semantic_sha_provenance: {C01_SEMANTIC_SHA256_PROVENANCE}");
    out
}

/// Render the full current-tree status projection.
pub fn render_status(loaded: &LoadedManifest, binding: &TreeBinding) -> Result<String> {
    let statuses = project_states(&loaded.manifest)?;
    let mut out = String::new();
    out.push_str("module-train status (offline projection; no network, no GitHub state)\n");
    out.push_str(&render_binding(binding, loaded));
    let ready_count = statuses.iter().filter(|s| s.state == CurrentTreeState::Ready).count();
    let _ = writeln!(
        out,
        "nodes: {} (ready:{}, landed:{}, blocked:{}, not_proven:{})",
        statuses.len(),
        ready_count,
        statuses.iter().filter(|s| s.state == CurrentTreeState::LandedCurrentTree).count(),
        statuses
            .iter()
            .filter(|s| matches!(
                s.state,
                CurrentTreeState::BlockedHard
                    | CurrentTreeState::BlockedEvidence
                    | CurrentTreeState::BlockedExternalOrAuthorization
            ))
            .count(),
        statuses.iter().filter(|s| s.state == CurrentTreeState::NotProven).count(),
    );
    out.push_str("NODE      ISSUE   ROLE           LANE             STATE                                  IMPLEMENTATION REASONS\n");
    for status in &statuses {
        let _ = writeln!(
            out,
            "{:<9} {:<7} {:<15} {:<17} {:<34} {:<14} {}",
            status.node_id,
            format!("#{}", status.issue),
            status.role,
            status.lane,
            status.state.as_str(),
            match status.implementation_presence {
                ProbeOutcome::Pass => "probe:pass",
                ProbeOutcome::Absent => "not_proven",
            },
            status.reasons.join(","),
        );
    }
    Ok(out)
}

/// Render the safe offline parallel frontier: all and only hard-ready,
/// role-valid leaves, with writer ceilings recorded (never filled as quotas).
pub fn render_next(loaded: &LoadedManifest, binding: &TreeBinding) -> Result<String> {
    let statuses = project_states(&loaded.manifest)?;
    let mut out = String::new();
    out.push_str("module-train next (safe offline parallel frontier)\n");
    out.push_str(&render_binding(binding, loaded));
    let ready: Vec<&NodeStatus> =
        statuses.iter().filter(|status| status.state == CurrentTreeState::Ready).collect();
    let _ = writeln!(out, "ready_leaves: {}", ready.len());
    let mut by_group: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for status in &ready {
        let _ = writeln!(
            out,
            "{} #{:<6} role={} lane={} writer_class={} conflict_key={}",
            status.node_id,
            status.issue,
            status.role,
            status.lane,
            status.parallel_group,
            status.conflict_key
        );
        if !status.reasons.is_empty() {
            let _ = writeln!(out, "  visible_limitations: {}", status.reasons.join(","));
        }
        by_group.entry(status.parallel_group.as_str()).or_default().push(status.node_id.as_str());
    }
    for (group, members) in &by_group {
        let _ = writeln!(out, "writer_class {group}: {}", members.join(","));
    }
    out.push_str(
        "law: writer classes are ceilings and semantic groupings, never quotas; \
         conflict keys are identities, not reservations; ordering is presentation-only\n",
    );
    Ok(out)
}

// ---------------------------------------------------------------------------
// CLI entry points.
// ---------------------------------------------------------------------------

pub fn run_status(tree: &str) -> Result<()> {
    let loaded = load_manifest()?;
    let binding = tree_binding(tree)?;
    print!("{}", render_status(&loaded, &binding)?);
    Ok(())
}

pub fn run_next(tree: &str) -> Result<()> {
    let loaded = load_manifest()?;
    let binding = tree_binding(tree)?;
    print!("{}", render_next(&loaded, &binding)?);
    Ok(())
}
