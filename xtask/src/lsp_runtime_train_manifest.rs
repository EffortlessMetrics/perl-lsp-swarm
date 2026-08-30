//! The stable LSP-runtime train node and authority contract (`lsp_runtime_train.v1`).
//!
//! G01 of issue #11036 under the #10360 control plane. This module consumes
//! `.spec/11036-lsp-runtime-train-schema/lsp_runtime_train.v1.json` strictly as
//! DATA and provides, offline:
//!
//! * a fail-closed loader: strict schema (`deny_unknown_fields`), closed
//!   vocabularies, contiguous ladders, role claim caps, referential integrity,
//!   acyclic hard edges, and a pinned canonical digest;
//! * the twelve structural laws #11036 names as falsifiers, each rejecting with
//!   a distinct reason so the focused proof can discriminate them;
//! * mutable-state containment in two halves: a forbidden-key scan, and the
//!   load-bearing value scan that rejects commit-shaped ids, live check and
//!   review verdicts, readiness claims, and writer assignments smuggled through
//!   accepted prose fields;
//! * bounded read accessors for the later control-plane slices (#11037
//!   population, #11072 artifact map, #11033 proof profiles, #11038 probes,
//!   #11306 frontier, #11040 observation, #11042 packets, #11044 closeout).
//!
//! Claim ceiling (#11036): data contract and fixtures only. No populated
//! programme graph, current-tree probe, readiness computation, GitHub access,
//! agent packet, runtime behavior, or external mutation belongs here — adding
//! one violates the issue's non-goals and the guard test named
//! `no_state_or_command_surface_is_added`.
//!
//! Shared-mechanics ruling: the landed `import_cleanup_train.v1` loading shape
//! is mirrored deliberately rather than extracted. The manifest records the
//! concrete duplication evidence for #10554; two manifests sharing a loading
//! shape does not satisfy that issue's own landed-duplication start gate.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sha2::Digest as ShaDigest;
use sha2::Sha256;

/// Repository-relative location of the stable runtime train contract (#11036).
pub const MANIFEST_RELATIVE_PATH: &str =
    ".spec/11036-lsp-runtime-train-schema/lsp_runtime_train.v1.json";

/// Schema identity consumed by this model.
pub const SCHEMA_NAME: &str = "lsp_runtime_train.v1";
pub const SCHEMA_VERSION: u64 = 1;

/// Pinned canonical digest of the current `lsp_runtime_train.v1` revision.
///
/// Canonicalization: recursive content walk with byte-ordinal ordering (see
/// `canonical_digest`). A semantic revision must move this pin deliberately
/// together with the manifest bytes; patching around it silently is exactly
/// what the pin exists to prevent.
pub const PINNED_CANONICAL_DIGEST: &str =
    "98BFDE8AEB5B9B610C45DDADAC21B15E3166152377B1A793703EB744491EF63C";

/// The only population status `v1` may claim. Completing the graph is #11037's
/// authority, so a manifest that calls itself complete fails closed here.
const REQUIRED_POPULATION_STATUS: &str = "schema_fixture_subset";

/// A non-selectable node states no proposition. Matched exactly, or as the
/// `none:` prefix that introduces the reason — a bare `starts_with("none")`
/// would read "nonsense implementation" as absent and reject a legitimate
/// proposition beginning "nonetheless".
fn states_no_proposition(proposition: &str) -> bool {
    let trimmed = proposition.trim();
    trimmed == "none" || trimmed.starts_with("none:")
}

// ---------------------------------------------------------------------------
// Code-owned v1 vocabularies. A cardinality check lets a repinned manifest
// rename a value and keep the count, so the reviewed sets live here and are
// compared for exact membership.
// ---------------------------------------------------------------------------

const V1_ROLES: [&str; 7] = [
    "controller",
    "implementation",
    "proof",
    "decision",
    "cutover",
    "external_action",
    "historical",
];

const V1_RELEASE_HORIZONS: [&str; 6] = [
    "shipped_correctness",
    "runtime_spine",
    "package_api",
    "product_cutover",
    "externalization",
    "optional_breadth",
];

const V1_CLAIM_CEILINGS: [&str; 5] =
    ["none", "schema_contract", "implementation_slice", "programme_stage", "programme"];

const V1_EDGE_KINDS: [&str; 4] = ["hard", "evidence", "authorization", "consumer"];

const V1_OLD_PATH_DISPOSITIONS: [&str; 7] = [
    "none",
    "delete",
    "unreachable",
    "forwarding_with_exit",
    "oracle_with_exit",
    "intentionally_independent",
    "external_transition",
];

const V1_STACK_RELATIONS: [&str; 2] = ["independent", "stacked_on"];

const V1_PARALLEL_DISPOSITIONS: [&str; 3] =
    ["parallel_safe", "serialized_by_conflict", "serialized_by_hard_dependency"];

const V1_AUTHORITY_PLANES: [&str; 6] = [
    "stable_graph",
    "artifact_and_proof_map",
    "current_tree_observation",
    "offline_frontier",
    "live_collaboration",
    "external_authorization",
];

/// Conflict-key spellings that would imply one global actor or lock. The
/// manifest must declare at least these; it may add more.
const V1_REQUIRED_CONFLICT_SENTINELS: [&str; 5] = ["*", "global", "all", "repository", "workspace"];

/// Probes the declared `forbidden_value_patterns` must actually reject. The
/// guard is otherwise defined by the data it guards: a repinned manifest could
/// swap every pattern for a harmless one and keep the list non-empty. Asserting
/// behavior rather than pattern text keeps the semantics code-owned while
/// leaving the exact expressions reviewable in the manifest.
const V1_MUTABLE_STATE_PROBES: [(&str, &str); 6] = [
    ("commit-shaped identifier", "landed at 0c07a9841c34ff3e"),
    ("live pull-request verdict", "PR #13869 is green"),
    ("check or review verdict", "the merge gate is failing"),
    ("readiness verdict", "ready = true"),
    ("writer assignment", "assigned to the runtime lane writer"),
    ("current-tree claim", "reproduced on current main"),
];

/// Exact set comparison for a reviewed v1 vocabulary.
fn ensure_exact_vocabulary(actual: &BTreeSet<&str>, expected: &[&str], label: &str) -> Result<()> {
    let expected_set: BTreeSet<&str> = expected.iter().copied().collect();
    if actual == &expected_set {
        return Ok(());
    }
    let missing: Vec<&&str> = expected_set.difference(actual).collect();
    let extra: Vec<&&str> = actual.difference(&expected_set).collect();
    bail!(
        "{label} must be exactly the reviewed v1 set; missing={missing:?} unexpected={extra:?}. \
         A count check would let a renamed value pass with the cardinality intact"
    );
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    schema_version: u64,
    planning_basis: String,
    population_status: String,
    population_successor: String,
    programme: Programme,
    authority_planes: Vec<AuthorityPlane>,
    role_vocabulary: Vec<RoleEntry>,
    release_horizon_ladder: Vec<LadderEntry>,
    claim_ceiling_ladder: Vec<LadderEntry>,
    role_claim_caps: Vec<RoleClaimCap>,
    edge_kinds: Vec<EdgeKind>,
    old_path_dispositions: Vec<OldPathDisposition>,
    stack_relations: Vec<ValueEntry>,
    parallel_dispositions: Vec<ValueEntry>,
    authorization_classes: Vec<AuthorizationClass>,
    generic_mechanics_boundary: GenericMechanicsBoundary,
    forbidden_mutable_fields: Vec<String>,
    global_conflict_key_sentinels: Vec<String>,
    forbidden_value_patterns: Vec<ForbiddenValuePattern>,
    schema_evolution: SchemaEvolution,
    determinism: Determinism,
    shared_mechanics_ruling: SharedMechanicsRuling,
    nodes: Vec<TrainNode>,
    limitations: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Programme {
    control_plane_issue: u64,
    runtime_architecture_issue: u64,
    integrated_product_issue: u64,
    schema_issue: u64,
    static_population_successor_issue: u64,
    shared_extraction_gate_issue: u64,
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
    selectable: bool,
    takes_dependency_edges: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ValueEntry {
    value: String,
    owns: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LadderEntry {
    rank: u64,
    value: String,
    owns: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct RoleClaimCap {
    role: String,
    max_claim: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct EdgeKind {
    kind: String,
    owns: String,
    serializes_implementation: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct OldPathDisposition {
    value: String,
    owns: String,
    requires_exit_owner: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct AuthorizationClass {
    id: String,
    subject: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct GenericMechanicsBoundary {
    note: String,
    generic_mechanics_sections: Vec<String>,
    forbidden_type_substrings: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ForbiddenValuePattern {
    pattern: String,
    owns: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct SchemaEvolution {
    unknown_version_policy: String,
    unknown_field_policy: String,
    additive_forward_policy: String,
    digest_exclusions: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Determinism {
    normalization_note: String,
    stable_id_rule: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct SharedMechanicsRuling {
    inventory: Vec<SharedMechanicsEntry>,
    reused: String,
    runtime_local_reason: String,
    extraction_gate_disposition: String,
    duplication_evidence_for_10554: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct SharedMechanicsEntry {
    subject: String,
    path: String,
    landed: bool,
    reusable_seam: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct TrainNode {
    stable_node_id: String,
    issue_ref: u64,
    controller_ref: Option<String>,
    role: String,
    lane: String,
    release_horizon: String,
    one_pr_proposition: String,
    claim_ceiling: String,
    authority_before: Vec<String>,
    authority_after: Vec<String>,
    consumed_authorities: Vec<String>,
    hard_dependencies: Vec<String>,
    evidence_dependencies: Vec<String>,
    authorization_dependencies: Vec<String>,
    consumer_edges: Vec<String>,
    supersedes: Vec<String>,
    superseded_by: Vec<String>,
    duplicate_of: Option<String>,
    exclusive_writer_conflict_keys: Vec<String>,
    shared_artifact_keys: Vec<String>,
    stack_relation: String,
    stack_target: Option<String>,
    parallel_disposition: String,
    old_path_disposition: String,
    old_path_exit_owner: Option<String>,
    required_falsifier_ids: Vec<String>,
    positive_proof_obligation_ids: Vec<String>,
    artifact_map_required: bool,
    checked_spec_disposition_required: bool,
    current_tree_probe_ids: Vec<String>,
    rollback_boundary: String,
    transfer_owner: Option<String>,
    return_to_issue_conditions: Vec<String>,
    stop_conditions: Vec<String>,
    limitations: Vec<String>,
}

// ---------------------------------------------------------------------------
// Canonical digest: recursive content walk, order-invariant, byte-ordinal
// sorting, SHA-256 uppercase hex. Same shape family as the landed cleanup-train
// projection so tooling stays comparable without coupling pins.
// ---------------------------------------------------------------------------

pub fn canonical_digest(value: &Value) -> Result<String> {
    let mut canonical = String::new();
    canonical_walk(value, &mut canonical)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{byte:02X}"));
    }
    Ok(hex)
}

fn canonical_walk(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("n;"),
        Value::Bool(flag) => {
            let _ = std::fmt::Write::write_fmt(
                out,
                format_args!("b:{};", if *flag { "True" } else { "False" }),
            );
        }
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                let _ = std::fmt::Write::write_fmt(out, format_args!("i:{signed};"));
            } else if let Some(unsigned) = number.as_u64() {
                let _ = std::fmt::Write::write_fmt(out, format_args!("i:{unsigned};"));
            } else {
                bail!("manifest canonicalization defines integers only; found {number}");
            }
        }
        // Escaping bound: only the backslash and the semicolon that terminates a
        // scalar token are escaped, matching the landed cleanup-train walk. The
        // container delimiters are not escaped inside string content; that stays
        // unambiguous because every scalar token is self-terminating and object
        // keys come from the strict field set rather than from input, but it is a
        // property of this schema rather than a general guarantee.
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

// ---------------------------------------------------------------------------
// Loading.
// ---------------------------------------------------------------------------

/// A loaded, digest-pinned, structurally validated runtime train contract.
#[derive(Debug, Clone)]
pub struct LoadedManifest {
    manifest: Manifest,
    canonical_digest_hex: String,
}

impl LoadedManifest {
    /// Bounded static facts of one node for downstream control-plane slices.
    pub fn node_static_fact(&self, stable_node_id: &str) -> Option<NodeStaticFact> {
        self.manifest.nodes.iter().find(|n| n.stable_node_id == stable_node_id).map(|node| {
            NodeStaticFact {
                stable_node_id: node.stable_node_id.clone(),
                issue_ref: node.issue_ref,
                role: node.role.clone(),
                lane: node.lane.clone(),
                release_horizon: node.release_horizon.clone(),
                claim_ceiling: node.claim_ceiling.clone(),
                one_pr_proposition: node.one_pr_proposition.clone(),
                hard_dependencies: node.hard_dependencies.clone(),
                evidence_dependencies: node.evidence_dependencies.clone(),
                authorization_dependencies: node.authorization_dependencies.clone(),
                consumer_edges: node.consumer_edges.clone(),
                exclusive_writer_conflict_keys: node.exclusive_writer_conflict_keys.clone(),
                old_path_disposition: node.old_path_disposition.clone(),
                old_path_exit_owner: node.old_path_exit_owner.clone(),
                rollback_boundary: node.rollback_boundary.clone(),
                stop_conditions: node.stop_conditions.clone(),
            }
        })
    }

    /// Every declared stable node id, in manifest order.
    /// Ascending by stable id, never manifest order. The canonical digest is
    /// order-insensitive, so two byte-orderings of one manifest share a digest;
    /// returning manifest order here would hand content-addressed consumers
    /// different sequences for the same digest.
    pub fn node_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> =
            self.manifest.nodes.iter().map(|n| n.stable_node_id.clone()).collect();
        ids.sort();
        ids
    }

    /// Ids of nodes whose role is selectable as work.
    pub fn selectable_node_ids(&self) -> Vec<String> {
        let selectable: BTreeSet<&str> = self
            .manifest
            .role_vocabulary
            .iter()
            .filter(|r| r.selectable)
            .map(|r| r.role.as_str())
            .collect();
        let mut ids: Vec<String> = self
            .manifest
            .nodes
            .iter()
            .filter(|n| selectable.contains(n.role.as_str()))
            .map(|n| n.stable_node_id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// The pinned canonical digest bound to these exact bytes.
    pub fn canonical_digest(&self) -> &str {
        &self.canonical_digest_hex
    }

    /// Total node count.
    pub fn node_count(&self) -> usize {
        self.manifest.nodes.len()
    }

    /// Population status; `v1` may only ever be a fixture subset.
    pub fn population_status(&self) -> &str {
        &self.manifest.population_status
    }
}

/// Bounded, cloned static facts. Deliberately carries no current, live, or
/// observed state: representing one would violate the contract this module owns.
#[derive(Debug, Clone)]
pub struct NodeStaticFact {
    pub stable_node_id: String,
    pub issue_ref: u64,
    pub role: String,
    pub lane: String,
    pub release_horizon: String,
    pub claim_ceiling: String,
    pub one_pr_proposition: String,
    pub hard_dependencies: Vec<String>,
    pub evidence_dependencies: Vec<String>,
    pub authorization_dependencies: Vec<String>,
    pub consumer_edges: Vec<String>,
    pub exclusive_writer_conflict_keys: Vec<String>,
    pub old_path_disposition: String,
    pub old_path_exit_owner: Option<String>,
    pub rollback_boundary: String,
    pub stop_conditions: Vec<String>,
}

/// Resolve the repository root without reaching into the binary-only utils
/// module: library modules compile against their own manifest dir.
fn project_root() -> Result<std::path::PathBuf> {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| color_eyre::eyre::eyre!("xtask must live in a workspace subdirectory"))
}

/// Load the workspace manifest, verifying the pinned digest binding.
pub fn load_manifest() -> Result<LoadedManifest> {
    let root = project_root()?;
    let path = root.join(MANIFEST_RELATIVE_PATH);
    load_manifest_from(&path)
}

/// Load a manifest from an explicit path (tests and fixtures).
pub fn load_manifest_from(path: &Path) -> Result<LoadedManifest> {
    let bytes = std::fs::read(path).with_context(|| {
        format!("failed to read lsp_runtime_train.v1 manifest at {}", path.display())
    })?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("manifest at {} is not valid JSON", path.display()))?;
    let digest = canonical_digest(&value)?;
    if digest != PINNED_CANONICAL_DIGEST {
        bail!(
            "lsp_runtime_train.v1 digest drift at {}: computed {digest} but pinned \
             {PINNED_CANONICAL_DIGEST}; a semantic revision must be classified through #10360 \
             and this pin re-derived deliberately",
            path.display()
        );
    }
    let manifest: Manifest = serde_json::from_value(value.clone()).with_context(|| {
        format!("manifest at {} violates the strict lsp_runtime_train.v1 schema", path.display())
    })?;
    validate_manifest(&manifest).with_context(|| {
        format!("manifest at {} fails lsp_runtime_train.v1 structural laws", path.display())
    })?;
    validate_no_mutable_live_facts(&value, &manifest).with_context(|| {
        format!("manifest at {} smuggles a mutable live fact into stable truth", path.display())
    })?;
    validate_no_mutable_live_values(&value, &manifest).with_context(|| {
        format!("manifest at {} smuggles mutable state through a prose value", path.display())
    })?;
    Ok(LoadedManifest { manifest, canonical_digest_hex: digest })
}

// ---------------------------------------------------------------------------
// Structural laws. Each law rejects with a distinct reason so the focused
// proof can discriminate the twelve falsifier classes #11036 requires.
// ---------------------------------------------------------------------------

fn validate_manifest(m: &Manifest) -> Result<()> {
    // Law 11a: schema identity. An unknown version fails closed rather than
    // being read under v1 semantics.
    if m.schema != SCHEMA_NAME {
        bail!("schema name mismatch: expected {SCHEMA_NAME}, found {}", m.schema);
    }
    if m.schema_version != SCHEMA_VERSION {
        bail!(
            "unknown schema_version {}: this reader implements only version {SCHEMA_VERSION} and \
             refuses to interpret another revision under v1 semantics",
            m.schema_version
        );
    }

    // Header wording.
    if m.planning_basis.trim().is_empty() {
        bail!("planning_basis must be worded");
    }
    if m.limitations.is_empty() {
        bail!("limitations must state what this revision does not establish");
    }

    // Population honesty: v1 is a fixture subset; claiming completeness would
    // take #11037's authority by assertion.
    if m.population_status != REQUIRED_POPULATION_STATUS {
        bail!(
            "population_status must be '{REQUIRED_POPULATION_STATUS}' in v1, found '{}'; \
             completing the graph is the successor's authority, not this contract's",
            m.population_status
        );
    }

    // Programme identity binds the reviewed issues.
    if m.programme.control_plane_issue != 10360
        || m.programme.schema_issue != 11036
        || m.programme.static_population_successor_issue != 11037
        || m.programme.shared_extraction_gate_issue != 10554
        || m.programme.runtime_architecture_issue != 7384
        || m.programme.integrated_product_issue != 9150
    {
        bail!(
            "programme block must bind #10360/#11036/#11037/#10554/#7384/#9150, found {}/{}/{}/{}/{}/{}",
            m.programme.control_plane_issue,
            m.programme.schema_issue,
            m.programme.static_population_successor_issue,
            m.programme.shared_extraction_gate_issue,
            m.programme.runtime_architecture_issue,
            m.programme.integrated_product_issue
        );
    }
    if m.programme.method_authority.trim().is_empty() {
        bail!("programme method authority must be worded");
    }

    // Authority planes: unique, fully worded, and the reviewed set is present.
    let mut planes: BTreeSet<&str> = BTreeSet::new();
    for plane in &m.authority_planes {
        if !planes.insert(plane.plane.as_str()) {
            bail!("duplicate authority plane: {}", plane.plane);
        }
        if plane.owns.trim().is_empty() || plane.never_substitutes.trim().is_empty() {
            bail!("authority plane {} carries an empty law", plane.plane);
        }
    }
    ensure_exact_vocabulary(&planes, &V1_AUTHORITY_PLANES, "authority_planes")?;

    // Closed vocabularies.
    let roles: BTreeSet<&str> = m.role_vocabulary.iter().map(|r| r.role.as_str()).collect();
    if roles.len() != m.role_vocabulary.len() {
        bail!("duplicate role in role_vocabulary");
    }
    for entry in &m.role_vocabulary {
        if entry.owns.trim().is_empty() {
            bail!("role {} carries an empty ownership law", entry.role);
        }
    }
    ensure_exact_vocabulary(&roles, &V1_ROLES, "role_vocabulary")?;

    let horizon_rank = ladder_ranks(&m.release_horizon_ladder, "release_horizon")?;
    let claim_rank = ladder_ranks(&m.claim_ceiling_ladder, "claim_ceiling")?;
    let horizon_values: BTreeSet<&str> = horizon_rank.keys().copied().collect();
    ensure_exact_vocabulary(&horizon_values, &V1_RELEASE_HORIZONS, "release_horizon_ladder")?;
    let ceiling_values: BTreeSet<&str> = claim_rank.keys().copied().collect();
    ensure_exact_vocabulary(&ceiling_values, &V1_CLAIM_CEILINGS, "claim_ceiling_ladder")?;

    let edge_kind_names: BTreeSet<&str> = m.edge_kinds.iter().map(|e| e.kind.as_str()).collect();
    if edge_kind_names.len() != m.edge_kinds.len() {
        bail!("edge_kinds declares a kind more than once");
    }
    ensure_exact_vocabulary(&edge_kind_names, &V1_EDGE_KINDS, "edge_kinds")?;
    for edge in &m.edge_kinds {
        if edge.owns.trim().is_empty() {
            bail!("edge kind {} carries an empty ownership law", edge.kind);
        }
    }
    // Only a hard edge may serialize implementation. Making evidence, consumer,
    // or authorization edges serialize is falsifier 3's collapse.
    for edge in &m.edge_kinds {
        let should_serialize = edge.kind == "hard";
        if edge.serializes_implementation != should_serialize {
            bail!(
                "edge kind '{}' declares serializes_implementation={}; only 'hard' edges order \
                 implementation, so the four edge classes would collapse",
                edge.kind,
                edge.serializes_implementation
            );
        }
    }

    // Collecting straight into a map would keep the last row for a repeated
    // value. Two orderings of conflicting duplicates share one digest (arrays
    // are sorted for canonicalization) while validating differently, so the
    // duplicate must fail before the map exists.
    let mut dispositions: BTreeMap<&str, bool> = BTreeMap::new();
    for entry in &m.old_path_dispositions {
        if dispositions.insert(entry.value.as_str(), entry.requires_exit_owner).is_some() {
            bail!(
                "old_path_dispositions declares '{}' more than once; a duplicate makes the exit \
                 rule depend on input order",
                entry.value
            );
        }
    }
    let disposition_values: BTreeSet<&str> = dispositions.keys().copied().collect();
    ensure_exact_vocabulary(
        &disposition_values,
        &V1_OLD_PATH_DISPOSITIONS,
        "old_path_dispositions",
    )?;
    for entry in &m.old_path_dispositions {
        if entry.owns.trim().is_empty() {
            bail!("old-path disposition {} carries an empty ownership law", entry.value);
        }
    }
    if dispositions.get("none") != Some(&false) {
        bail!("'none' must not require an exit owner");
    }

    let stack_values: BTreeSet<&str> = m.stack_relations.iter().map(|v| v.value.as_str()).collect();
    let parallel_values: BTreeSet<&str> =
        m.parallel_dispositions.iter().map(|v| v.value.as_str()).collect();
    if stack_values.len() != m.stack_relations.len() {
        bail!("stack_relations declares a value more than once");
    }
    if parallel_values.len() != m.parallel_dispositions.len() {
        bail!("parallel_dispositions declares a value more than once");
    }
    for entry in m.stack_relations.iter().chain(m.parallel_dispositions.iter()) {
        if entry.owns.trim().is_empty() {
            bail!("vocabulary entry {} carries an empty ownership law", entry.value);
        }
    }
    ensure_exact_vocabulary(&stack_values, &V1_STACK_RELATIONS, "stack_relations")?;
    ensure_exact_vocabulary(&parallel_values, &V1_PARALLEL_DISPOSITIONS, "parallel_dispositions")?;

    // Role claim caps cover exactly the role vocabulary.
    let capped: BTreeSet<&str> = m.role_claim_caps.iter().map(|c| c.role.as_str()).collect();
    if capped != roles {
        bail!(
            "role_claim_caps must cover exactly the role vocabulary; missing={:?} extra={:?}",
            roles.difference(&capped).collect::<Vec<_>>(),
            capped.difference(&roles).collect::<Vec<_>>()
        );
    }
    let mut cap_rank: BTreeMap<&str, u64> = BTreeMap::new();
    for cap in &m.role_claim_caps {
        match claim_rank.get(cap.max_claim.as_str()) {
            Some(rank) => {
                // A duplicate row would survive the set-coverage check above and
                // silently keep whichever copy is inserted last. Because
                // canonicalization sorts arrays, two orderings of the same
                // conflicting rows share a digest while validating differently:
                // validation must not depend on incidental input order.
                if cap_rank.insert(cap.role.as_str(), *rank).is_some() {
                    bail!(
                        "role_claim_caps declares role '{}' more than once; a duplicate cap makes \
                         the effective ceiling depend on input order",
                        cap.role
                    );
                }
            }
            None => bail!(
                "role_claim_caps entry '{}' invents max_claim '{}' outside the reviewed ladder",
                cap.role,
                cap.max_claim
            ),
        }
    }

    // Authorization classes: unique, hash-prefixed, worded.
    let mut authorization_ids: BTreeSet<&str> = BTreeSet::new();
    for class in &m.authorization_classes {
        if !class.id.starts_with('#') {
            bail!("authorization class id must start with '#': {}", class.id);
        }
        if class.subject.trim().is_empty() {
            bail!("authorization class {} carries an empty subject", class.id);
        }
        if !authorization_ids.insert(class.id.as_str()) {
            bail!("duplicate authorization class id {}", class.id);
        }
    }
    if authorization_ids.is_empty() {
        bail!("at least one authorization class must exist for external actions to reference");
    }

    // Law 9: generic mechanics carry graph semantics only.
    validate_generic_mechanics_boundary(m)?;

    // Law 10: extraction is justified by landed reuse, never by symmetry.
    validate_shared_mechanics_ruling(m)?;

    // Schema-evolution and determinism wording must be present and explicit.
    for (name, law) in [
        ("unknown_version_policy", &m.schema_evolution.unknown_version_policy),
        ("unknown_field_policy", &m.schema_evolution.unknown_field_policy),
        ("additive_forward_policy", &m.schema_evolution.additive_forward_policy),
    ] {
        if law.trim().is_empty() {
            bail!("schema_evolution.{name} is empty");
        }
    }
    if m.schema_evolution.digest_exclusions.is_empty() {
        bail!("schema_evolution.digest_exclusions must name what the semantic digest excludes");
    }
    if m.determinism.normalization_note.trim().is_empty()
        || m.determinism.stable_id_rule.trim().is_empty()
    {
        bail!("determinism laws must be worded");
    }
    if m.forbidden_mutable_fields.is_empty() {
        bail!("forbidden_mutable_fields must name the mutable facts this contract refuses");
    }
    // The sentinel set is a guard defined by the data it guards, so a repinned
    // manifest could keep the list non-empty while dropping every real
    // sentinel. Code owns the floor; the manifest may only add to it.
    let declared_sentinels: BTreeSet<&str> =
        m.global_conflict_key_sentinels.iter().map(String::as_str).collect();
    for required in V1_REQUIRED_CONFLICT_SENTINELS {
        if !declared_sentinels.contains(required) {
            bail!(
                "global_conflict_key_sentinels omits the reviewed sentinel '{required}'; a \
                 non-empty list is not the same as an enforcing one"
            );
        }
    }

    // Node-level laws.
    validate_nodes(
        m,
        &roles,
        &horizon_rank,
        &claim_rank,
        &cap_rank,
        &dispositions,
        &stack_values,
        &parallel_values,
        &authorization_ids,
    )?;

    // Positive obligations: the fixture set must exercise the whole contract.
    validate_coverage(m, &roles, &horizon_rank, &dispositions)?;

    Ok(())
}

fn ladder_ranks<'a>(entries: &'a [LadderEntry], label: &str) -> Result<BTreeMap<&'a str, u64>> {
    let mut by_rank: BTreeMap<u64, &str> = BTreeMap::new();
    let mut by_value: BTreeMap<&str, u64> = BTreeMap::new();
    for entry in entries {
        if entry.owns.trim().is_empty() {
            bail!("{label} ladder entry {} carries an empty ownership law", entry.value);
        }
        if by_rank.insert(entry.rank, entry.value.as_str()).is_some() {
            bail!("{label} ladder repeats rank {}", entry.rank);
        }
        if by_value.insert(entry.value.as_str(), entry.rank).is_some() {
            bail!("{label} ladder repeats value {}", entry.value);
        }
    }
    if by_rank.is_empty() {
        bail!("{label} ladder is empty");
    }
    for (expected, actual) in (0..by_rank.len() as u64).zip(by_rank.keys().copied()) {
        if expected != actual {
            bail!("{label} ladder ranks must be contiguous from zero; found a gap at {expected}");
        }
    }
    Ok(by_value)
}

fn validate_generic_mechanics_boundary(m: &Manifest) -> Result<()> {
    let boundary = &m.generic_mechanics_boundary;
    if boundary.note.trim().is_empty() {
        bail!("generic_mechanics_boundary.note must be worded");
    }
    if boundary.generic_mechanics_sections.is_empty()
        || boundary.forbidden_type_substrings.is_empty()
    {
        bail!("generic_mechanics_boundary must name both its sections and its forbidden types");
    }

    // Collect the wording of each declared generic-mechanics section.
    let mut wording: Vec<(&str, &str)> = Vec::new();
    for section in &boundary.generic_mechanics_sections {
        match section.as_str() {
            "role_vocabulary" => {
                wording.extend(m.role_vocabulary.iter().map(|e| (e.role.as_str(), e.owns.as_str())))
            }
            "release_horizon_ladder" => wording.extend(
                m.release_horizon_ladder.iter().map(|e| (e.value.as_str(), e.owns.as_str())),
            ),
            "claim_ceiling_ladder" => wording
                .extend(m.claim_ceiling_ladder.iter().map(|e| (e.value.as_str(), e.owns.as_str()))),
            "edge_kinds" => {
                wording.extend(m.edge_kinds.iter().map(|e| (e.kind.as_str(), e.owns.as_str())))
            }
            "old_path_dispositions" => wording.extend(
                m.old_path_dispositions.iter().map(|e| (e.value.as_str(), e.owns.as_str())),
            ),
            "stack_relations" => wording
                .extend(m.stack_relations.iter().map(|e| (e.value.as_str(), e.owns.as_str()))),
            "parallel_dispositions" => wording.extend(
                m.parallel_dispositions.iter().map(|e| (e.value.as_str(), e.owns.as_str())),
            ),
            other => bail!("generic_mechanics_sections names an unknown section '{other}'"),
        }
    }

    for (name, text) in wording {
        for forbidden in &boundary.forbidden_type_substrings {
            if text.contains(forbidden.as_str()) || name.contains(forbidden.as_str()) {
                bail!(
                    "generic schema mechanics entry '{name}' imports the implementation type \
                     '{forbidden}'; product vocabulary belongs in node payloads, not in the \
                     graph mechanics"
                );
            }
        }
    }
    Ok(())
}

fn validate_shared_mechanics_ruling(m: &Manifest) -> Result<()> {
    let ruling = &m.shared_mechanics_ruling;
    if ruling.inventory.is_empty() {
        bail!("shared_mechanics_ruling.inventory must record the landed train mechanics surveyed");
    }
    for entry in &ruling.inventory {
        if entry.subject.trim().is_empty()
            || entry.path.trim().is_empty()
            || entry.reusable_seam.trim().is_empty()
        {
            bail!("shared-mechanics inventory entry {} is not fully worded", entry.subject);
        }
    }
    for field in [&ruling.reused, &ruling.runtime_local_reason, &ruling.extraction_gate_disposition]
    {
        if field.trim().is_empty() {
            bail!("shared_mechanics_ruling must word its reuse, locality, and gate disposition");
        }
    }
    if !ruling.inventory.iter().any(|e| e.landed) {
        bail!(
            "shared_mechanics_ruling claims reuse but no inventory entry is landed; planned \
             schemas and similar prose are not extraction evidence"
        );
    }
    if ruling.duplication_evidence_for_10554.is_empty() {
        bail!(
            "deferring #10554 requires recording the concrete duplication this revision creates, \
             so the extraction gate can be judged on evidence rather than symmetry"
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_nodes(
    m: &Manifest,
    roles: &BTreeSet<&str>,
    horizon_rank: &BTreeMap<&str, u64>,
    claim_rank: &BTreeMap<&str, u64>,
    cap_rank: &BTreeMap<&str, u64>,
    dispositions: &BTreeMap<&str, bool>,
    stack_values: &BTreeSet<&str>,
    parallel_values: &BTreeSet<&str>,
    authorization_ids: &BTreeSet<&str>,
) -> Result<()> {
    if m.nodes.is_empty() {
        bail!("a contract with no fixture nodes proves nothing about its own expressiveness");
    }

    let role_meta: BTreeMap<&str, &RoleEntry> =
        m.role_vocabulary.iter().map(|r| (r.role.as_str(), r)).collect();

    // Unique stable ids.
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    for node in &m.nodes {
        if node.stable_node_id.trim().is_empty() {
            bail!("every node needs a stable_node_id");
        }
        if !ids.insert(node.stable_node_id.as_str()) {
            bail!("duplicate stable_node_id {}", node.stable_node_id);
        }
    }

    if !ids.contains(m.population_successor.as_str()) {
        bail!(
            "population_successor '{}' does not resolve to a declared node",
            m.population_successor
        );
    }

    for node in &m.nodes {
        let id = &node.stable_node_id;
        let role = match role_meta.get(node.role.as_str()) {
            Some(role) => *role,
            None => bail!("node {id} uses role '{}' outside the closed vocabulary", node.role),
        };
        let _ = roles;

        if !horizon_rank.contains_key(node.release_horizon.as_str()) {
            bail!("node {id} uses release_horizon '{}' outside the ladder", node.release_horizon);
        }
        if !stack_values.contains(node.stack_relation.as_str()) {
            bail!("node {id} uses stack_relation '{}' outside the vocabulary", node.stack_relation);
        }
        if !parallel_values.contains(node.parallel_disposition.as_str()) {
            bail!(
                "node {id} uses parallel_disposition '{}' outside the vocabulary",
                node.parallel_disposition
            );
        }
        if node.lane.trim().is_empty() {
            bail!("node {id} must name its lane");
        }

        // Law 2: a selectable node must carry a complete authority proposition.
        let ceiling_rank = match claim_rank.get(node.claim_ceiling.as_str()) {
            Some(rank) => *rank,
            None => {
                bail!("node {id} uses claim_ceiling '{}' outside the ladder", node.claim_ceiling)
            }
        };
        if let Some(cap) = cap_rank.get(node.role.as_str())
            && ceiling_rank > *cap
        {
            bail!(
                "node {id} claims ceiling '{}' above the cap its role '{}' allows",
                node.claim_ceiling,
                node.role
            );
        }

        if role.selectable {
            if node.one_pr_proposition.trim().is_empty()
                || states_no_proposition(&node.one_pr_proposition)
            {
                bail!(
                    "node {id} is selectable but states no one-PR proposition; a selectable node \
                     without a proposition cannot be reviewed or rolled back"
                );
            }
            if node.authority_before.is_empty() || node.authority_after.is_empty() {
                bail!(
                    "node {id} omits its authority delta; a selectable node must say what authority \
                     moves"
                );
            }
            if node.rollback_boundary.trim().is_empty() {
                bail!("node {id} omits its rollback boundary");
            }
            if node.stop_conditions.is_empty() {
                bail!("node {id} omits its stop boundary");
            }
        } else {
            if !states_no_proposition(&node.one_pr_proposition) {
                bail!(
                    "node {id} has non-selectable role '{}' but states a one-PR proposition; \
                     a controller, decision, external action, or historical node is never an \
                     implementation leaf",
                    node.role
                );
            }
            // Law 1: a non-selectable node may not carry integration probes.
            if !node.current_tree_probe_ids.is_empty() {
                bail!(
                    "node {id} has non-selectable role '{}' but declares current-tree probes; \
                     only a selectable leaf is integrated on a tree",
                    node.role
                );
            }
        }

        // Law 1: roles that group or record carry no dependency edges at all.
        if !role.takes_dependency_edges
            && (!node.hard_dependencies.is_empty()
                || !node.evidence_dependencies.is_empty()
                || !node.authorization_dependencies.is_empty()
                || !node.consumer_edges.is_empty())
        {
            bail!(
                "node {id} has role '{}', whose nodes carry no dependency edges; adding one makes \
                 a grouping node behave like a leaf",
                node.role
            );
        }

        // Law 3: the four edge classes must stay distinct per node.
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        for (class, targets) in [
            ("hard", &node.hard_dependencies),
            ("evidence", &node.evidence_dependencies),
            ("consumer", &node.consumer_edges),
        ] {
            for target in targets {
                if let Some(previous) = seen.insert(target.as_str(), class) {
                    bail!(
                        "node {id} lists '{target}' as both a {previous} and a {class} edge; the \
                         four edge classes are distinct and must not be conflated"
                    );
                }
            }
        }
        if node.hard_dependencies.contains(id)
            || node.evidence_dependencies.contains(id)
            || node.consumer_edges.contains(id)
        {
            bail!("node {id} depends on itself");
        }

        // Referential integrity for every node-valued reference.
        for (label, targets) in [
            ("hard_dependencies", &node.hard_dependencies),
            ("evidence_dependencies", &node.evidence_dependencies),
            ("consumer_edges", &node.consumer_edges),
            ("supersedes", &node.supersedes),
            ("superseded_by", &node.superseded_by),
        ] {
            for target in targets {
                if !ids.contains(target.as_str()) {
                    bail!("node {id} references unknown node '{target}' in {label}");
                }
            }
        }
        if let Some(controller) = &node.controller_ref {
            match m.nodes.iter().find(|n| &n.stable_node_id == controller) {
                None => bail!("node {id} names unknown controller_ref '{controller}'"),
                Some(target) if target.role != "controller" => bail!(
                    "node {id} names controller_ref '{controller}', whose role is '{}' rather than \
                     controller",
                    target.role
                ),
                Some(_) => {}
            }
        }
        if let Some(duplicate) = &node.duplicate_of {
            if !ids.contains(duplicate.as_str()) {
                bail!("node {id} names unknown duplicate_of '{duplicate}'");
            }
            if duplicate == id {
                bail!(
                    "node {id} declares itself its own duplicate; a duplicate relation names a \
                     different node or is absent"
                );
            }
        }

        // Law 4: an external action names its authorization class; nothing else
        // may claim one, and completion never implies it.
        if node.role == "external_action" {
            if node.authorization_dependencies.is_empty() {
                bail!(
                    "node {id} is an external action with no authorization class; external \
                     authorization can never be inferred from dependency completion"
                );
            }
        } else if !node.authorization_dependencies.is_empty() {
            bail!(
                "node {id} has role '{}' but claims an authorization class; only an external \
                 action carries one",
                node.role
            );
        }
        for class in &node.authorization_dependencies {
            if !authorization_ids.contains(class.as_str()) {
                bail!("node {id} names authorization class '{class}' outside the declared set");
            }
        }

        // An external action is performed by someone; a node that cannot be
        // done here must say who does it.
        match (node.role.as_str(), &node.transfer_owner) {
            ("external_action", None) => bail!(
                "node {id} is an external action with no transfer owner; an action outside this \
                 repository must name who performs it"
            ),
            (role, Some(owner)) if role != "external_action" => bail!(
                "node {id} has role '{role}' but names transfer owner '{owner}'; only an external \
                 action transfers out of this repository"
            ),
            _ => {}
        }

        // Every node states how it returns to its issue and what it does not
        // establish; silence here is how a contract becomes unfalsifiable.
        if node.return_to_issue_conditions.is_empty() {
            bail!("node {id} omits its return-to-issue conditions");
        }
        if node.limitations.is_empty() {
            bail!("node {id} omits its limitations");
        }

        // Proof obligations are declared, distinct, and required of leaves.
        if role.selectable && node.required_falsifier_ids.is_empty() {
            bail!(
                "node {id} is selectable but declares no falsifier; a leaf whose claim cannot be \
                 falsified cannot be reviewed"
            );
        }
        let falsifiers: BTreeSet<&str> =
            node.required_falsifier_ids.iter().map(String::as_str).collect();
        if falsifiers.len() != node.required_falsifier_ids.len() {
            bail!("node {id} repeats a falsifier id");
        }
        for positive in &node.positive_proof_obligation_ids {
            if falsifiers.contains(positive.as_str()) {
                bail!(
                    "node {id} lists '{positive}' as both a falsifier and a positive obligation; a \
                     negative control and a positive obligation are different proofs"
                );
            }
        }

        // Artifact and spec obligations follow selectability, not symmetry.
        if !role.selectable
            && (node.artifact_map_required || node.checked_spec_disposition_required)
        {
            bail!(
                "node {id} has non-selectable role '{}' but requires an artifact map or checked \
                 spec; only a leaf that lands a candidate owns artifacts",
                node.role
            );
        }
        if node.checked_spec_disposition_required && !node.artifact_map_required {
            bail!(
                "node {id} requires a checked spec disposition without an artifact map; the spec \
                 packet is part of the artifact set, not a parallel authority"
            );
        }

        // Law 5: an old path that survives names its exit owner.
        let requires_exit = match dispositions.get(node.old_path_disposition.as_str()) {
            Some(requires) => *requires,
            None => bail!(
                "node {id} uses old_path_disposition '{}' outside the vocabulary",
                node.old_path_disposition
            ),
        };
        match (&node.old_path_exit_owner, requires_exit) {
            (None, true) => bail!(
                "node {id} declares old_path_disposition '{}' without an exit owner; a migration \
                 whose old path survives must name who removes it",
                node.old_path_disposition
            ),
            (Some(owner), true) if !ids.contains(owner.as_str()) => {
                bail!("node {id} names unknown old_path_exit_owner '{owner}'")
            }
            (Some(owner), false) => bail!(
                "node {id} declares old_path_disposition '{}' but names exit owner '{owner}'; that \
                 disposition leaves no path to exit",
                node.old_path_disposition
            ),
            _ => {}
        }
        if node.role == "cutover" && node.old_path_disposition == "none" {
            bail!(
                "node {id} is a cutover with old_path_disposition 'none'; a cutover exists \
                 precisely to withdraw a prior path"
            );
        }

        // An ordering label needs a witness. Without this, a node can announce
        // serialization that nothing in the graph provides, and a consumer can
        // schedule work from an invented order.
        let shares_conflict_key = m.nodes.iter().any(|other| {
            other.stable_node_id != node.stable_node_id
                && other
                    .exclusive_writer_conflict_keys
                    .iter()
                    .any(|key| node.exclusive_writer_conflict_keys.contains(key))
        });
        match node.parallel_disposition.as_str() {
            "serialized_by_hard_dependency" if node.hard_dependencies.is_empty() => {
                bail!("node {id} claims serialization by hard dependency but declares no hard edge")
            }
            "serialized_by_conflict" if !shares_conflict_key => bail!(
                "node {id} claims serialization by conflict but shares no exclusive writer key \
                 with another node"
            ),
            "parallel_safe" if shares_conflict_key => bail!(
                "node {id} claims to be parallel-safe while sharing an exclusive writer key with \
                 another node"
            ),
            _ => {}
        }

        // A stack relation names the candidate it stands on, or it is a label
        // with nothing behind it.
        match (node.stack_relation.as_str(), &node.stack_target) {
            ("independent", Some(target)) => {
                bail!("node {id} is stack-independent but names stack target '{target}'")
            }
            ("independent", None) => {}
            (_, None) => bail!(
                "node {id} declares stack relation '{}' without naming the candidate it stacks on",
                node.stack_relation
            ),
            (_, Some(target)) => {
                if !ids.contains(target.as_str()) {
                    bail!("node {id} names unknown stack_target '{target}'");
                }
                if target == id {
                    bail!("node {id} stacks on itself");
                }
            }
        }

        // Law 8: conflict keys are semantic, never a global lock.
        for key in &node.exclusive_writer_conflict_keys {
            if m.global_conflict_key_sentinels.iter().any(|s| s == key) {
                bail!(
                    "node {id} declares the global conflict key '{key}'; one authority does not \
                     imply one global actor or lock"
                );
            }
            if key.trim().is_empty() {
                bail!("node {id} declares an empty conflict key");
            }
        }
    }

    // Law 3 (graph level): a consumer edge is never a prerequisite. If A names B
    // a consumer, B must actually read A through a hard or evidence edge.
    for node in &m.nodes {
        for consumer in &node.consumer_edges {
            let target = m
                .nodes
                .iter()
                .find(|n| &n.stable_node_id == consumer)
                .ok_or_else(|| color_eyre::eyre::eyre!("unresolved consumer {consumer}"))?;
            if !target.hard_dependencies.contains(&node.stable_node_id)
                && !target.evidence_dependencies.contains(&node.stable_node_id)
            {
                bail!(
                    "node {} names '{consumer}' a consumer, but '{consumer}' declares no hard or \
                     evidence edge back; a consumer edge is a reader, not a prerequisite",
                    node.stable_node_id
                );
            }
        }
    }

    // A node may only consume authority some declared dependency actually
    // produces. Without this, `consumed_authorities` is prose and a node can
    // claim to stand on an authority nothing in the graph establishes.
    for node in &m.nodes {
        if node.consumed_authorities.is_empty() {
            continue;
        }
        let mut available: BTreeSet<&str> = BTreeSet::new();
        for target in node.hard_dependencies.iter().chain(node.evidence_dependencies.iter()) {
            if let Some(dep) = m.nodes.iter().find(|n| &n.stable_node_id == target) {
                available.extend(dep.authority_after.iter().map(String::as_str));
            }
        }
        for consumed in &node.consumed_authorities {
            if !available.contains(consumed.as_str()) {
                bail!(
                    "node {} consumes the authority '{consumed}', which no hard or evidence \
                     dependency produces; an authority is consumed from a declared producer, \
                     never asserted",
                    node.stable_node_id
                );
            }
        }
    }

    // Shared artifacts need one explicit writer: concurrently selectable nodes
    // touching the same artifact must not also share an exclusive writer key.
    let mut by_artifact: BTreeMap<&str, Vec<&TrainNode>> = BTreeMap::new();
    for node in &m.nodes {
        for key in &node.shared_artifact_keys {
            if key.trim().is_empty() {
                bail!("node {} declares an empty shared artifact key", node.stable_node_id);
            }
            by_artifact.entry(key.as_str()).or_default().push(node);
        }
    }
    for (artifact, holders) in &by_artifact {
        // Reusing one key across concurrent holders is already rejected by the
        // per-node witness law. What remains for the artifact plane is that a
        // concurrently-written artifact must have an *identified* writer: a
        // parallel_safe holder with no key at all leaves the artifact with
        // concurrent writers and nothing naming them.
        for node in holders.iter().filter(|n| n.parallel_disposition == "parallel_safe") {
            if node.exclusive_writer_conflict_keys.is_empty() {
                bail!(
                    "node {} is a parallel_safe holder of shared artifact '{artifact}' but \
                     declares no exclusive writer key; a shared artifact needs one explicit writer",
                    node.stable_node_id
                );
            }
        }
    }

    // Supersession is symmetric, so a stale half-edge cannot survive.
    for node in &m.nodes {
        for superseded in &node.supersedes {
            let target = m
                .nodes
                .iter()
                .find(|n| &n.stable_node_id == superseded)
                .ok_or_else(|| color_eyre::eyre::eyre!("unresolved supersedes {superseded}"))?;
            if !target.superseded_by.contains(&node.stable_node_id) {
                bail!(
                    "node {} supersedes '{superseded}' but '{superseded}' does not record it; a \
                     stale supersession half-edge is rejected",
                    node.stable_node_id
                );
            }
        }
        // A node is never its own replacement. Self-reference satisfies
        // reciprocity trivially, so it must be rejected before that check.
        if node.supersedes.contains(&node.stable_node_id)
            || node.superseded_by.contains(&node.stable_node_id)
        {
            bail!(
                "node {} supersedes itself; a supersession names a different node",
                node.stable_node_id
            );
        }
        // The reverse direction needs its own check: referential integrity only
        // proves the id resolves, so a `superseded_by` entry whose alleged
        // successor does not record the transition would otherwise load as a
        // valid symmetric edge.
        for successor in &node.superseded_by {
            let target =
                m.nodes.iter().find(|n| &n.stable_node_id == successor).ok_or_else(|| {
                    color_eyre::eyre::eyre!("unresolved superseded_by {successor}")
                })?;
            if !target.supersedes.contains(&node.stable_node_id) {
                bail!(
                    "node {} claims to be superseded by '{successor}', but '{successor}' does not \
                     supersede it; a stale supersession half-edge is rejected",
                    node.stable_node_id
                );
            }
        }
    }

    // Law 6: two exclusive writers of one key are never both parallel-safe, and
    // the serialized one names an exact stack relation.
    let mut by_key: BTreeMap<&str, Vec<&TrainNode>> = BTreeMap::new();
    for node in &m.nodes {
        for key in &node.exclusive_writer_conflict_keys {
            by_key.entry(key.as_str()).or_default().push(node);
        }
    }
    for (key, holders) in &by_key {
        if holders.len() < 2 {
            continue;
        }
        // Note: "two parallel_safe holders of one key" needs no check here —
        // the per-node witness law above already rejects any parallel_safe node
        // that shares a key, so a branch for it would be unreachable.
        for holder in holders {
            if holder.parallel_disposition == "serialized_by_conflict"
                && holder.stack_relation == "independent"
            {
                bail!(
                    "node {} is serialized by the conflict key '{key}' but declares an independent \
                     stack relation; name the candidate it stacks on",
                    holder.stable_node_id
                );
            }
        }
        // A disposition only *claims* an ordering. Every pair of exclusive
        // writers must actually be ordered by a hard-dependency path, or the
        // manifest hands consumers two concurrent writers of one key while
        // asserting they are serialized.
        for (index, first) in holders.iter().enumerate() {
            for second in holders.iter().skip(index + 1) {
                if hard_reaches(m, &first.stable_node_id, &second.stable_node_id)
                    || hard_reaches(m, &second.stable_node_id, &first.stable_node_id)
                {
                    continue;
                }
                bail!(
                    "nodes {} and {} both write the exclusive key '{key}' but no hard-dependency \
                     path orders them; a serialized disposition must be backed by a real edge",
                    first.stable_node_id,
                    second.stable_node_id
                );
            }
        }
    }

    // A supersession cycle would make every node in it both predecessor and
    // successor, which reciprocity alone cannot catch.
    detect_supersession_cycle(m)?;

    // Hard edges must be acyclic: a cycle would make the spine unbuildable.
    detect_hard_cycle(m)?;

    Ok(())
}

/// Supersession must be a strict order: a cycle would make every node in it
/// both the replaced and the replacement.
fn detect_supersession_cycle(m: &Manifest) -> Result<()> {
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();

    fn walk<'a>(node: &'a str, m: &'a Manifest, state: &mut BTreeMap<&'a str, u8>) -> Result<()> {
        match state.get(node) {
            Some(1) => bail!("supersession cycle reached '{node}'"),
            Some(2) => return Ok(()),
            _ => {}
        }
        state.insert(node, 1);
        if let Some(found) = m.nodes.iter().find(|n| n.stable_node_id == node) {
            for target in &found.supersedes {
                walk(target.as_str(), m, state)?;
            }
        }
        state.insert(node, 2);
        Ok(())
    }

    for node in &m.nodes {
        walk(node.stable_node_id.as_str(), m, &mut state)?;
    }
    Ok(())
}

/// Does `from` reach `to` through hard dependencies? Cycle-safe: this runs
/// before `detect_hard_cycle`, so a malformed manifest must not spin here.
fn hard_reaches(m: &Manifest, from: &str, to: &str) -> bool {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = vec![from];
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(node) = m.nodes.iter().find(|n| n.stable_node_id == current) else {
            continue;
        };
        for target in &node.hard_dependencies {
            if target == to {
                return true;
            }
            stack.push(target.as_str());
        }
    }
    false
}

fn detect_hard_cycle(m: &Manifest) -> Result<()> {
    let deps: BTreeMap<&str, &Vec<String>> =
        m.nodes.iter().map(|n| (n.stable_node_id.as_str(), &n.hard_dependencies)).collect();
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();

    fn walk<'a>(
        node: &'a str,
        deps: &BTreeMap<&'a str, &'a Vec<String>>,
        state: &mut BTreeMap<&'a str, u8>,
    ) -> Result<()> {
        match state.get(node) {
            Some(1) => bail!("hard dependency cycle reached '{node}'"),
            Some(2) => return Ok(()),
            _ => {}
        }
        state.insert(node, 1);
        if let Some(targets) = deps.get(node) {
            for target in targets.iter() {
                let key = deps
                    .keys()
                    .find(|k| **k == target.as_str())
                    .copied()
                    .ok_or_else(|| color_eyre::eyre::eyre!("unresolved hard edge {target}"))?;
                walk(key, deps, state)?;
            }
        }
        state.insert(node, 2);
        Ok(())
    }

    for node in m.nodes.iter() {
        walk(node.stable_node_id.as_str(), &deps, &mut state)?;
    }
    Ok(())
}

/// Positive obligations: the fixture set must exercise every role, horizon, and
/// old-path disposition, or the contract's expressiveness is untested.
fn validate_coverage(
    m: &Manifest,
    roles: &BTreeSet<&str>,
    horizon_rank: &BTreeMap<&str, u64>,
    dispositions: &BTreeMap<&str, bool>,
) -> Result<()> {
    let used_roles: BTreeSet<&str> = m.nodes.iter().map(|n| n.role.as_str()).collect();
    let missing: Vec<&&str> = roles.difference(&used_roles).collect();
    if !missing.is_empty() {
        bail!("no fixture node exercises these roles: {missing:?}");
    }

    let used_horizons: BTreeSet<&str> =
        m.nodes.iter().map(|n| n.release_horizon.as_str()).collect();
    let declared_horizons: BTreeSet<&str> = horizon_rank.keys().copied().collect();
    let missing: Vec<&&str> = declared_horizons.difference(&used_horizons).collect();
    if !missing.is_empty() {
        bail!("no fixture node exercises these release horizons: {missing:?}");
    }

    let used_dispositions: BTreeSet<&str> =
        m.nodes.iter().map(|n| n.old_path_disposition.as_str()).collect();
    let declared: BTreeSet<&str> = dispositions.keys().copied().collect();
    let missing: Vec<&&str> = declared.difference(&used_dispositions).collect();
    if !missing.is_empty() {
        bail!("no fixture node exercises these old-path dispositions: {missing:?}");
    }

    // #11036 requires the fixtures to exercise stack and parallel dispositions
    // too. Declaring a value no fixture uses leaves that distinction unproven,
    // so the vocabulary carries only what the fixtures actually demonstrate.
    let declared_stacks: BTreeSet<&str> =
        m.stack_relations.iter().map(|s| s.value.as_str()).collect();
    let used_stacks: BTreeSet<&str> = m.nodes.iter().map(|n| n.stack_relation.as_str()).collect();
    let missing: Vec<&&str> = declared_stacks.difference(&used_stacks).collect();
    if !missing.is_empty() {
        bail!("no fixture node exercises these stack relations: {missing:?}");
    }

    let declared_parallel: BTreeSet<&str> =
        m.parallel_dispositions.iter().map(|p| p.value.as_str()).collect();
    let used_parallel: BTreeSet<&str> =
        m.nodes.iter().map(|n| n.parallel_disposition.as_str()).collect();
    let missing: Vec<&&str> = declared_parallel.difference(&used_parallel).collect();
    if !missing.is_empty() {
        bail!("no fixture node exercises these parallel dispositions: {missing:?}");
    }

    // Every edge class must be exercised somewhere, or the distinctions the
    // contract draws are untested by its own fixtures.
    if !m.nodes.iter().any(|n| !n.hard_dependencies.is_empty()) {
        bail!("no fixture node exercises a hard edge");
    }
    if !m.nodes.iter().any(|n| !n.evidence_dependencies.is_empty()) {
        bail!("no fixture node exercises an evidence edge");
    }
    if !m.nodes.iter().any(|n| !n.authorization_dependencies.is_empty()) {
        bail!("no fixture node exercises an authorization edge");
    }
    if !m.nodes.iter().any(|n| !n.consumer_edges.is_empty()) {
        bail!("no fixture node exercises a consumer edge");
    }
    Ok(())
}

/// Law 7: no mutable, live, observed, or released fact may appear as a key
/// anywhere in the stable manifest.
fn validate_no_mutable_live_facts(value: &Value, m: &Manifest) -> Result<()> {
    let forbidden: BTreeSet<&str> = m.forbidden_mutable_fields.iter().map(String::as_str).collect();
    let mut offender: Option<String> = None;
    scan_keys(value, &forbidden, &mut offender);
    match offender {
        Some(key) => bail!(
            "stable manifest carries the mutable live fact '{key}'; GitHub state, current SHAs, \
             readiness, writer assignment, telemetry, and release verdicts never become stable \
             graph truth"
        ),
        None => Ok(()),
    }
}

/// Law 7, value half: a forbidden *key* scan only closes the obvious door.
/// Mutable state is just as representable inside an accepted prose field, so
/// every string value is checked against the declared patterns too. The key
/// scan stays as defense in depth; this is the load-bearing half.
fn validate_no_mutable_live_values(value: &Value, m: &Manifest) -> Result<()> {
    if m.forbidden_value_patterns.is_empty() {
        bail!(
            "forbidden_value_patterns must name the mutable claims prose fields may not carry; \
             a key-name scan alone leaves state freely representable in accepted strings"
        );
    }
    let mut compiled: Vec<(regex::Regex, &str)> = Vec::new();
    for entry in &m.forbidden_value_patterns {
        if entry.owns.trim().is_empty() {
            bail!("forbidden value pattern '{}' carries an empty ownership law", entry.pattern);
        }
        let re = regex::Regex::new(&entry.pattern).map_err(|e| {
            color_eyre::eyre::eyre!("forbidden value pattern '{}' is invalid: {e}", entry.pattern)
        })?;
        compiled.push((re, entry.owns.as_str()));
    }

    // Behavioral floor: the declared patterns must actually reject each
    // reviewed class of mutable state. Comparing pattern *text* would freeze
    // the expressions; asserting what they catch keeps the semantics
    // code-owned while leaving the expressions reviewable in the manifest.
    for (semantic, probe) in V1_MUTABLE_STATE_PROBES {
        if !compiled.iter().any(|(re, _)| re.is_match(probe)) {
            bail!(
                "forbidden_value_patterns no longer reject a {semantic} (probe: \"{probe}\"); the \
                 declared patterns must enforce every reviewed class, not merely be non-empty"
            );
        }
    }

    let mut offender: Option<(String, String)> = None;
    scan_values(value, &compiled, &mut offender);
    match offender {
        Some((text, owns)) => bail!(
            "stable manifest carries {owns} in a prose value: \"{text}\"; a stable node describes \
             intent, never the state of a tree, candidate, check, or writer"
        ),
        None => Ok(()),
    }
}

fn scan_values(
    value: &Value,
    patterns: &[(regex::Regex, &str)],
    offender: &mut Option<(String, String)>,
) {
    if offender.is_some() {
        return;
    }
    match value {
        Value::String(text) => {
            for (re, owns) in patterns {
                if let Some(found) = re.find(text) {
                    let mut excerpt = found.as_str().to_string();
                    if excerpt.len() > 80 {
                        excerpt.truncate(80);
                    }
                    *offender = Some((excerpt, (*owns).to_string()));
                    return;
                }
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                scan_values(child, patterns, offender);
                if offender.is_some() {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                scan_values(item, patterns, offender);
                if offender.is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn scan_keys(value: &Value, forbidden: &BTreeSet<&str>, offender: &mut Option<String>) {
    if offender.is_some() {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if forbidden.contains(key.as_str()) {
                    *offender = Some(key.clone());
                    return;
                }
                scan_keys(child, forbidden, offender);
                if offender.is_some() {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                scan_keys(item, forbidden, offender);
                if offender.is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "lsp_runtime_train_manifest_tests.rs"]
mod tests;
