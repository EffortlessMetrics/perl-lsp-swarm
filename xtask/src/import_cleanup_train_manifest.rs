//! The canonical imports-cleanup train manifest (`import_cleanup_train.v1`).
//!
//! Slice C01/#ICT-1 of issue #11084 under the #11081 execution-control plane.
//! This module consumes `.spec/11084-import-cleanup-train-manifest/import_cleanup_train.v1.json`
//! strictly as DATA and provides, offline:
//!
//! * a fail-closed loader: strict schema (`deny_unknown_fields`), structural
//!   identity laws, closed vocabularies, an ordered promotion lattice, ceiling
//!   caps, and a pinned canonical digest (any tampering or un-classified
//!   revision fails loudly);
//! * the typed node model downstream control slices (#11088 validation depth,
//!   #11091 packets, #11094 state, #11098 frontier, #11101 rendering,
//!   #11105 observation, #11113 receipts, #11122 dogfooding) consume through
//!   bounded accessors.
//!
//! Claim ceiling (ICT-C01): topology and contracts only. No validator
//! subcommand, current-tree probe, readiness computation, live GitHub access,
//! product behavior, support claim, or external mutation belongs here — adding
//! one violates the issue's non-goals and the guard test named
//! `no_validator_command_surface_is_added`.
//!
//! Boundaries honored: no network, no scheduling, no agent launch, no product
//! mutation, no support inference.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sha2::Digest as ShaDigest;
use sha2::Sha256;

/// Repository-relative location of the stable cleanup train manifest (#11084).
pub const MANIFEST_RELATIVE_PATH: &str =
    ".spec/11084-import-cleanup-train-manifest/import_cleanup_train.v1.json";

/// Schema identity consumed by this model.
pub const SCHEMA_NAME: &str = "import_cleanup_train.v1";
pub const SCHEMA_VERSION: u64 = 1;

/// Pinned canonical digest of the current `import_cleanup_train.v1` revision.
///
/// Canonicalization: recursive content walk with byte-ordinal ordering (see
/// `canonical_digest`). Any classified revision routed through #11081 must
/// move this pin deliberately together with the manifest bytes; patching
/// around it silently is exactly what the pin exists to prevent.
pub const PINNED_CANONICAL_DIGEST: &str =
    "19A77C5027C0D7ADB26F283B182D1ED519A39C597F44528196043B8970F77148";

const DEP_CLASSES: [&str; 4] = ["hard", "evidence", "external", "optional"];

/// Roles that group, close, or govern; they never take dependency edges and
/// never hold writer slots.
const CONTROL_ROLES: [&str; 2] = ["controller", "train_control"];

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    schema_version: u64,
    planning_basis: String,
    programme: Programme,
    authority_planes: Vec<AuthorityPlane>,
    role_vocabulary: Vec<RoleEntry>,
    operation_family_vocabulary: Vec<ValueEntry>,
    product_context_vocabulary: Vec<ValueEntry>,
    wire_kind_vocabulary: Vec<ValueEntry>,
    application_ceiling_ladder: Vec<LadderEntry>,
    claim_ceiling_ladder: Vec<LadderEntry>,
    dependency_classes: Vec<String>,
    dependency_class_semantics: BTreeMap<String, String>,
    edge_role_lattice_note: String,
    edge_role_lattice: Vec<LatticePair>,
    stage_transfer_bans: TransferBans,
    role_claim_caps: Vec<RoleClaimCap>,
    external_authorities: Vec<ExternalAuthority>,
    evidence_semantics: EvidenceSemantics,
    omission_policy: String,
    refusal_surfaces: Vec<String>,
    verification_surface: VerificationSurface,
    revision_governance: RevisionGovernance,
    contained_legacy_rows: Vec<LegacyRow>,
    nodes: Vec<TrainNode>,
    limitations: Vec<String>,
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
struct LatticePair {
    from_role: String,
    to_role: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct TransferBans {
    note: String,
    bans: Vec<BanEntry>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct BanEntry {
    rule: String,
    encodes: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct RoleClaimCap {
    role: String,
    max_claim: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ExternalAuthority {
    id: String,
    subject: String,
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
struct VerificationSurface {
    note: String,
    allowed_command_prefixes: Vec<String>,
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
struct LegacyRow {
    row: String,
    absorbed_by: String,
    disposition: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct TrainNode {
    node_id: String,
    issue: u64,
    title: String,
    role: String,
    status: String,
    operation_family: String,
    product_context: String,
    wire_kind: String,
    application_ceiling: String,
    claim_ceiling: String,
    programme_phase: String,
    controller_chain: Vec<String>,
    lane: String,
    one_pr_outcome: String,
    rollback_unit: String,
    dependencies: Vec<Dependency>,
    authorities_consumed: Vec<String>,
    authority_before: String,
    authority_after: String,
    writer_slot: String,
    conflict_key: String,
    allowed_components: Vec<String>,
    owned_paths: Vec<String>,
    forbidden_adjacent: Vec<ForbiddenAdjacent>,
    spec: NodeSpec,
    first_falsifier: String,
    plausible_wrong_implementation: String,
    proofs: Proofs,
    focused_proof_owner: String,
    proof_command_templates: Vec<String>,
    observations: Vec<String>,
    review_forward_questions: Vec<String>,
    documentation_impact: String,
    docs_owner: String,
    changelog_disposition: String,
    adjacent_defect_transfer: DefectTransfer,
    stop_conditions: Vec<String>,
    return_to_issue: bool,
    next_consumers: Vec<String>,
    superseded_by: Vec<String>,
    transferred_to: String,
    expansion_policy: String,
    probe_owner: String,
    pr_contract: PrContract,
    limitations: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Dependency {
    target: String,
    class: String,
    provenance: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct ForbiddenAdjacent {
    component: String,
    owner_node: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct NodeSpec {
    requirement: String,
    owner: String,
    packet: String,
    stale_check: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Proofs {
    positive: Vec<String>,
    negative: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct DefectTransfer {
    subject: String,
    first_divergence: String,
    owner: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct PrContract {
    title_prefix: String,
    handoff_fields: Vec<String>,
}

// ---------------------------------------------------------------------------
// Canonical digest: recursive content walk, order-invariant, byte-ordinal
// sorting, SHA-256 uppercase hex (same shape family as the module-train
// projection so tooling stays comparable without coupling pins).
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
// Loading: read bytes, parse the full value tree (digest input), verify the
// pinned digest binding, strictly deserialize, then enforce structural laws.
// ---------------------------------------------------------------------------

/// A loaded, digest-pinned, structurally validated cleanup train manifest.
#[derive(Debug, Clone)]
pub struct LoadedManifest {
    manifest: Manifest,
    canonical_digest_hex: String,
}

impl LoadedManifest {
    /// Bounded static facts of one node, exposed for downstream control slices.
    /// This additive read accessor changes no semantics and creates no second
    /// topology authority.
    pub fn node_static_fact(&self, node_id: &str) -> Option<NodeStaticFact> {
        self.manifest.nodes.iter().find(|n| n.node_id == node_id).map(|node| NodeStaticFact {
            node_id: node.node_id.clone(),
            issue: node.issue,
            title: node.title.clone(),
            role: node.role.clone(),
            status: node.status.clone(),
            operation_family: node.operation_family.clone(),
            product_context: node.product_context.clone(),
            wire_kind: node.wire_kind.clone(),
            claim_ceiling: node.claim_ceiling.clone(),
            first_falsifier: node.first_falsifier.clone(),
            plausible_wrong_implementation: node.plausible_wrong_implementation.clone(),
            rollback_unit: node.rollback_unit.clone(),
            conflict_key: node.conflict_key.clone(),
            writer_slot: node.writer_slot.clone(),
            dependencies: node
                .dependencies
                .iter()
                .map(|d| (d.target.clone(), d.class.clone()))
                .collect(),
        })
    }

    /// All node ids, guaranteed ascending (a loader law).
    pub fn node_ids(&self) -> Vec<String> {
        self.manifest.nodes.iter().map(|n| n.node_id.clone()).collect()
    }

    /// The pinned canonical digest bound to these exact bytes.
    pub fn canonical_digest(&self) -> &str {
        &self.canonical_digest_hex
    }

    /// Total node count.
    pub fn node_count(&self) -> usize {
        self.manifest.nodes.len()
    }
}

#[derive(Debug, Clone)]
pub struct NodeStaticFact {
    pub node_id: String,
    pub issue: u64,
    pub title: String,
    pub role: String,
    pub status: String,
    pub operation_family: String,
    pub product_context: String,
    pub wire_kind: String,
    pub claim_ceiling: String,
    pub first_falsifier: String,
    pub plausible_wrong_implementation: String,
    pub rollback_unit: String,
    pub conflict_key: String,
    pub writer_slot: String,
    pub dependencies: Vec<(String, String)>,
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
        format!("failed to read import_cleanup_train.v1 manifest at {}", path.display())
    })?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("manifest at {} is not valid JSON", path.display()))?;
    let digest = canonical_digest(&value)?;
    if digest != PINNED_CANONICAL_DIGEST {
        bail!(
            "import_cleanup_train.v1 digest drift at {}: computed {digest} but pinned \
             {PINNED_CANONICAL_DIGEST}; a semantic revision must be classified through #11081 \
             and this pin re-derived deliberately",
            path.display()
        );
    }
    let manifest: Manifest = serde_json::from_value(value).with_context(|| {
        format!("manifest at {} violates the strict import_cleanup_train.v1 schema", path.display())
    })?;
    validate_manifest(&manifest).with_context(|| {
        format!("manifest at {} fails import_cleanup_train.v1 structural laws", path.display())
    })?;
    Ok(LoadedManifest { manifest, canonical_digest_hex: digest })
}

// ---------------------------------------------------------------------------
// Structural laws.
// ---------------------------------------------------------------------------

fn validate_manifest(m: &Manifest) -> Result<()> {
    if m.schema != SCHEMA_NAME {
        bail!("schema name mismatch: expected {SCHEMA_NAME}, found {}", m.schema);
    }
    if m.schema_version != SCHEMA_VERSION {
        bail!("schema_version mismatch: expected {SCHEMA_VERSION}, found {}", m.schema_version);
    }

    // Header-level worded laws.
    if m.planning_basis.trim().is_empty()
        || m.omission_policy.trim().is_empty()
        || m.edge_role_lattice_note.trim().is_empty()
    {
        bail!(
            "header identity fields (planning basis, omission policy, lattice note) must be worded"
        );
    }
    if m.stage_transfer_bans.note.trim().is_empty() || m.stage_transfer_bans.bans.is_empty() {
        bail!("stage_transfer_bans must carry a note and at least one enforced ban");
    }
    for ban in &m.stage_transfer_bans.bans {
        if ban.rule.trim().is_empty() || ban.encodes.trim().is_empty() {
            bail!("stage transfer bans must name their rule and the identity law encoded");
        }
    }

    // Programme identity.
    if m.programme.parent_programme_issue != 8277
        || m.programme.controller_issue != 11081
        || m.programme.evidence_controller_issue != 8336
    {
        bail!(
            "programme block must bind #8277 / #11081 / #8336, found {}/{}/{}",
            m.programme.parent_programme_issue,
            m.programme.controller_issue,
            m.programme.evidence_controller_issue
        );
    }
    if m.programme.home_programme.trim().is_empty()
        || m.programme.method_authority.trim().is_empty()
    {
        bail!("programme home/method authority must be worded");
    }

    // Authority planes: fully worded, unique.
    let mut planes: BTreeSet<&str> = BTreeSet::new();
    for plane in &m.authority_planes {
        if !planes.insert(plane.plane.as_str()) {
            bail!("duplicate authority plane: {}", plane.plane);
        }
        if plane.owns.trim().is_empty() || plane.never_substitutes.trim().is_empty() {
            bail!("authority plane {} carries an empty law", plane.plane);
        }
    }
    if planes.len() < 5 {
        bail!(
            "import_cleanup_train.v1 requires its reviewed non-substitution planes, found {}",
            planes.len()
        );
    }

    // Closed vocabularies: entries are pairs of value plus ownership wording.
    let roles: BTreeSet<&str> = m.role_vocabulary.iter().map(|r| r.role.as_str()).collect();
    for entry in &m.role_vocabulary {
        if entry.owns.trim().is_empty() {
            bail!("role {} carries an empty ownership law", entry.role);
        }
    }
    ensure_vocabulary(&roles, "role")?;
    for entry in m
        .operation_family_vocabulary
        .iter()
        .chain(m.product_context_vocabulary.iter())
        .chain(m.wire_kind_vocabulary.iter())
    {
        if entry.owns.trim().is_empty() {
            bail!("vocabulary entry {} carries an empty ownership law", entry.value);
        }
    }
    let families: BTreeSet<&str> =
        m.operation_family_vocabulary.iter().map(|v| v.value.as_str()).collect();
    ensure_vocabulary(&families, "operation_family")?;
    let contexts: BTreeSet<&str> =
        m.product_context_vocabulary.iter().map(|v| v.value.as_str()).collect();
    ensure_vocabulary(&contexts, "product_context")?;
    let wires: BTreeSet<&str> = m.wire_kind_vocabulary.iter().map(|v| v.value.as_str()).collect();
    ensure_vocabulary(&wires, "wire_kind")?;

    // Ladders: contiguous ranks from zero, unique values.
    let app_ranks = ladder_ranks(&m.application_ceiling_ladder, "application_ceiling")?;
    let app_rank_of = rank_lookup(&m.application_ceiling_ladder);
    let claim_ranks = ladder_ranks(&m.claim_ceiling_ladder, "claim_ceiling")?;
    let claim_rank_of = rank_lookup(&m.claim_ceiling_ladder);

    // Dependency class law: the four #10858 classes, fixed by contract,
    // IN THIS ORDER. Canonicalization sorts arrays, so sequence here is
    // carried by this law, not by the digest pin; other top-level lists are
    // order-free by reviewed intent (identity planes, vocabularies, nodes'
    // presentation fields), which is why they stay set/set-like.
    if m.dependency_classes != DEP_CLASSES {
        bail!(
            "dependency_classes are contract and order-significant: expected {DEP_CLASSES:?}, found {:?}",
            m.dependency_classes
        );
    }
    let classes: BTreeSet<&str> = m.dependency_classes.iter().map(String::as_str).collect();
    if m.dependency_class_semantics.len() != 4 {
        bail!("every dependency class needs worded semantics");
    }
    for (class, wording) in &m.dependency_class_semantics {
        if !classes.contains(class.as_str()) || wording.trim().is_empty() {
            bail!("dependency class semantics unknown or empty for {class}");
        }
    }

    // Promotion lattice: valid roles, directed, unique.
    let mut lattice: BTreeSet<(&str, &str)> = BTreeSet::new();
    for pair in &m.edge_role_lattice {
        if !roles.contains(pair.from_role.as_str()) || !roles.contains(pair.to_role.as_str()) {
            bail!("edge lattice references unknown role: {} -> {}", pair.from_role, pair.to_role);
        }
        if pair.from_role == pair.to_role {
            bail!(
                "lattice pairs must not be reflexive; equal roles are handled by the pipeline rule"
            );
        }
        if !lattice.insert((pair.from_role.as_str(), pair.to_role.as_str())) {
            bail!("duplicate lattice pair {} -> {}", pair.from_role, pair.to_role);
        }
    }
    if lattice.is_empty() {
        bail!("edge_role_lattice is empty; ordered stages cannot hold");
    }

    // Claim caps cover exactly the declared roles; an unknown cap value fails
    // closed instead of silently acting unlimited against the ladder.
    let capped: BTreeSet<&str> = m.role_claim_caps.iter().map(|c| c.role.as_str()).collect();
    if capped != roles {
        bail!(
            "role_claim_caps must cover exactly the role vocabulary; missing={:?} extra={:?}",
            roles.difference(&capped).collect::<Vec<_>>(),
            capped.difference(&roles).collect::<Vec<_>>()
        );
    }
    let mut claim_cap_rank: BTreeMap<&str, u64> = BTreeMap::new();
    for cap in &m.role_claim_caps {
        let rank = match claim_ranks.get(cap.max_claim.as_str()) {
            Some(rank) => *rank,
            None => bail!(
                "role_claim_caps entry '{}' invents max_claim '{}' outside the reviewed ladder",
                cap.role,
                cap.max_claim
            ),
        };
        claim_cap_rank.insert(cap.role.as_str(), rank);
    }

    // External authorities: unique, hash-prefixed, worded.
    let mut authority_ids: BTreeSet<&str> = BTreeSet::new();
    for authority in &m.external_authorities {
        if !authority.id.starts_with('#') {
            bail!("external authority id must start with '#': {}", authority.id);
        }
        if authority.subject.trim().is_empty() {
            bail!("external authority {} carries an empty subject", authority.id);
        }
        if !authority_ids.insert(authority.id.as_str()) {
            bail!("duplicate external authority id {}", authority.id);
        }
    }

    // Evidence semantics: the four laws worded.
    for (name, law) in [
        ("not_proven_law", &m.evidence_semantics.not_proven_law),
        ("optional_visibility", &m.evidence_semantics.optional_visibility),
        ("metadata_only_rule", &m.evidence_semantics.metadata_only_rule),
        ("issue_identity_rule", &m.evidence_semantics.issue_identity_rule),
    ] {
        if law.trim().is_empty() {
            bail!("evidence_semantics.{name} is empty");
        }
    }

    // Command-surface honesty: frozen to what main actually proves today.
    if m.verification_surface.note.trim().is_empty()
        || m.verification_surface.allowed_command_prefixes.is_empty()
    {
        bail!("verification_surface must record the proven command shapes");
    }

    // Refusal surfaces: known decision/admission/assessment nodes.
    let required_refusal_roles = ["semantic_decision", "product_admission", "semantic_assessment"];
    if m.refusal_surfaces.is_empty() {
        bail!("refusal_surfaces cannot be empty: infra never forces a positive cohort");
    }
    for id in &m.refusal_surfaces {
        match m.nodes.iter().find(|n| &n.node_id == id) {
            Some(node) if required_refusal_roles.contains(&node.role.as_str()) => {}
            Some(node) => bail!(
                "refusal surface {id} carries role {}, expected a decision/admission/assessment row",
                node.role
            ),
            None => bail!("refusal surface {id} does not resolve to a node"),
        }
    }

    // Contained legacy rows absorb into the registered authority guard.
    for row in &m.contained_legacy_rows {
        if row.row.trim().is_empty() || row.disposition.trim().is_empty() {
            bail!("contained legacy rows must stay worded");
        }
        match m.nodes.iter().find(|n| n.node_id == row.absorbed_by) {
            Some(node) if node.role == "authority_guard" => {}
            Some(node) => bail!(
                "legacy row absorbs into {} with role {}; expected the authority_guard",
                row.absorbed_by,
                node.role
            ),
            None => bail!("legacy row absorbs into unknown node {}", row.absorbed_by),
        }
    }

    // Node inventory laws.
    if m.nodes.len() < 60 {
        bail!("the complete #8277 graph demands at least 60 nodes, found {}", m.nodes.len());
    }
    // Exactly-four independent containments, exact issues, zero inbound
    // satisfaction between them (edit-context isolation plane).
    let containment_issues: BTreeSet<u64> =
        m.nodes.iter().filter(|n| n.role == "containment").map(|n| n.issue).collect();
    let wanted: BTreeSet<u64> = [8305, 10690, 11079, 11158].into_iter().collect();
    if containment_issues != wanted {
        bail!(
            "the four containment withdrawals must be exactly #8305/#10690/#11079/#11158, found {:?}",
            containment_issues
        );
    }

    // Wire-authority shape (identity laws 6 and 7): one shared all-operations
    // WorkspaceEdit authority plus at most one route per operation family, and
    // exactly one governed completion adapter. Checked before per-node laws so
    // flips land on their governing guard.
    let mut ws_families: BTreeSet<&str> = BTreeSet::new();
    let mut has_shared_authority = false;
    for node in m.nodes.iter().filter(|n| n.status == "active" && n.role == "code_action_adapter") {
        if node.wire_kind != "workspace_edit" {
            bail!(
                "code-action adapter {} must project through workspace_edit, found {}",
                node.node_id,
                node.wire_kind
            );
        }
        if node.operation_family == "all_operations" {
            has_shared_authority = true;
        }
        if !ws_families.insert(node.operation_family.as_str()) {
            bail!(
                "duplicate live WorkspaceEdit route for operation family {} (adapter collapse)",
                node.operation_family
            );
        }
    }
    if !has_shared_authority {
        bail!(
            "the shared all-operations WorkspaceEdit authority (#10667 lane) is missing from the active graph"
        );
    }
    let completion_adapters: Vec<_> =
        m.nodes.iter().filter(|n| n.status == "active" && n.role == "completion_adapter").collect();
    match completion_adapters.as_slice() {
        [only]
            if only.wire_kind == "completion_item"
                && only.product_context == "completion_item"
                && only.operation_family == "add_missing" => {}
        _ => bail!(
            "exactly one governed completion_adapter projecting add_missing plans through \
             completion_item edits is required"
        ),
    }

    // Context-collapse bans among active buildable rows sharing add_missing:
    // one visible consumer row per wire context; the shared internal-plan
    // spine stays stage-separated by role.
    let add_missing_active: Vec<_> = m
        .nodes
        .iter()
        .filter(|n| {
            n.status == "active"
                && !CONTROL_ROLES.contains(&n.role.as_str())
                // Containments are independent withdrawals, exactly the rows
                // allowed to share a context without satisfying anything.
                && n.role != "containment"
                && n.operation_family == "add_missing"
        })
        .collect();
    let mut completion_rows = 0;
    let mut action_rows = 0;
    let mut spine_seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for node in &add_missing_active {
        match node.product_context.as_str() {
            "completion_item" => completion_rows += 1,
            "code_action" => action_rows += 1,
            "internal_plan" => {
                // Bind the insert before testing it (#12910): the row must be
                // recorded on its first sighting whether or not this arm bails,
                // and a mutating call reads as a pure condition in a guard.
                let first_sighting =
                    spine_seen.insert((node.product_context.as_str(), node.role.as_str()));
                if !first_sighting {
                    bail!(
                        "add_missing context collapse: two internal-plan rows share role {} and collapse stages",
                        node.role
                    );
                }
            }
            _ => {}
        }
    }
    if completion_rows > 1 || action_rows > 1 {
        bail!(
            "add_missing context collapse: code_action rows={action_rows}, completion_item rows={completion_rows}"
        );
    }

    // Revision governance binds to the declared control-plane row (#11081):
    // the digest pin in this module is one deliberate half of that contract.
    match m.nodes.iter().find(|n| n.node_id == m.revision_governance.owner_node) {
        Some(owner) if owner.issue == m.revision_governance.owner_issue => {}
        Some(owner) => bail!(
            "revision governance names {} but pins issue {}; the row carries #{}",
            m.revision_governance.owner_node,
            m.revision_governance.owner_issue,
            owner.issue
        ),
        None => bail!(
            "revision governance owner node {} does not exist",
            m.revision_governance.owner_node
        ),
    }
    if m.revision_governance.owner_issue != 11081 {
        bail!("cleanup train revisions classify through #11081 only");
    }
    for (field, value) in [
        ("invalidates", &m.revision_governance.invalidates),
        ("never", &m.revision_governance.never),
        ("metadata_only", &m.revision_governance.metadata_only),
    ] {
        if value.trim().is_empty() {
            bail!("revision_governance.{field} is empty");
        }
    }

    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    let mut seen_issues: BTreeSet<u64> = BTreeSet::new();
    let mut seen_keys: BTreeSet<&str> = BTreeSet::new();
    for window in m.nodes.windows(2) {
        if window[0].node_id >= window[1].node_id {
            bail!(
                "nodes must appear in strictly ascending node_id order; found {} before {}",
                window[0].node_id,
                window[1].node_id
            );
        }
    }
    let by_id: BTreeMap<&str, &TrainNode> =
        m.nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    for node in &m.nodes {
        if !seen_ids.insert(node.node_id.as_str()) {
            bail!("duplicate node_id {}", node.node_id);
        }
        if !seen_issues.insert(node.issue) {
            bail!("duplicate issue identity {} at {}", node.issue, node.node_id);
        }
        if !seen_keys.insert(node.conflict_key.as_str()) {
            bail!("duplicate writer conflict key '{}' at {}", node.conflict_key, node.node_id);
        }
        if !roles.contains(node.role.as_str()) {
            bail!("unknown role '{}' at {}", node.role, node.node_id);
        }
        if node.status != "active" && node.status != "superseded" {
            bail!("unknown status '{}' at {}", node.status, node.node_id);
        }
        if !families.contains(node.operation_family.as_str()) {
            bail!("unknown operation_family '{}' at {}", node.operation_family, node.node_id);
        }
        if !contexts.contains(node.product_context.as_str()) {
            bail!("unknown product_context '{}' at {}", node.product_context, node.node_id);
        }
        if !wires.contains(node.wire_kind.as_str()) {
            bail!("unknown wire_kind '{}' at {}", node.wire_kind, node.node_id);
        }

        // Ceiling ranks within ladder + role caps; unknown values fail closed
        // instead of silently acting unlimited against the ladder.
        let app_cap = application_cap_for_role(node.role.as_str());
        let app_value_rank = match app_rank_of.get(node.application_ceiling.as_str()) {
            Some(rank) => *rank,
            None => bail!(
                "application_ceiling '{}' at {} is outside the reviewed ladder",
                node.application_ceiling,
                node.node_id
            ),
        };
        if app_value_rank > *app_ranks.get(app_cap).unwrap_or(&u64::MAX) {
            bail!(
                "application_ceiling '{}' exceeds role cap '{app_cap}' at {}",
                node.application_ceiling,
                node.node_id
            );
        }
        let claim_value_rank = match claim_rank_of.get(node.claim_ceiling.as_str()) {
            Some(rank) => *rank,
            None => bail!(
                "claim_ceiling '{}' at {} is outside the reviewed ladder",
                node.claim_ceiling,
                node.node_id
            ),
        };
        let claim_cap = claim_cap_rank.get(node.role.as_str()).copied().unwrap_or(u64::MAX);
        if claim_value_rank > claim_cap {
            bail!(
                "claim_ceiling '{}' exceeds the reviewed cap '{}' at {}",
                node.claim_ceiling,
                node.role,
                node.node_id
            );
        }

        // Wire coherence: payload kinds live inside their contexts only.
        wire_coherence(node)?;

        // Writer laws: pure grouping/history rows never hold a slot; an
        // otherwise-buildable row may opt out only when it is explicitly
        // transferred elsewhere (a parked deferral).
        let is_grouping = node.role == "controller" || node.status == "superseded";
        if is_grouping && node.writer_slot != "none" {
            bail!(
                "writer_slot law broken at {}: role {} status {} writer_slot {}",
                node.node_id,
                node.role,
                node.status,
                node.writer_slot
            );
        }
        if !is_grouping
            && node.writer_slot == "none"
            && node.transferred_to.trim().eq_ignore_ascii_case("none")
        {
            bail!(
                "node {} parks its writer slot without transferring the work anywhere",
                node.node_id
            );
        }
        if (node.role == "controller" || node.status == "superseded")
            && !node.dependencies.is_empty()
        {
            bail!("governance/history rows carry no dependency edges; offending {}", node.node_id);
        }
        if !matches!(node.writer_slot.as_str(), "none" | "A" | "B" | "C" | "D") {
            bail!("unknown writer capacity slot '{}' at {}", node.writer_slot, node.node_id);
        }

        // Supersession closure.
        if node.status == "superseded" {
            if node.superseded_by.is_empty() {
                bail!("superseded node {} lacks its successor disposition", node.node_id);
            }
            for successor in &node.superseded_by {
                match by_id.get(successor.as_str()) {
                    Some(target) if target.status == "active" => {}
                    Some(_) => bail!("successor {successor} of {} is not active", node.node_id),
                    None => bail!("successor {successor} of {} does not exist", node.node_id),
                }
            }
        } else if !node.superseded_by.is_empty() {
            bail!("active node {} cannot declare itself superseded", node.node_id);
        }

        // Dependencies.
        let mut dep_targets: BTreeSet<&str> = BTreeSet::new();
        for dep in &node.dependencies {
            if !DEP_CLASSES.contains(&dep.class.as_str()) {
                bail!(
                    "unknown dependency class '{}' at {} -> {}",
                    dep.class,
                    node.node_id,
                    dep.target
                );
            }
            if dep.provenance.trim().is_empty() {
                bail!("edge {} -> {} carries an empty provenance", node.node_id, dep.target);
            }
            if !dep_targets.insert(dep.target.as_str()) {
                bail!("more than one dependency to target {} at {}", dep.target, node.node_id);
            }
            if dep.target.starts_with('#') {
                if !authority_ids.contains(dep.target.as_str()) {
                    bail!("{} depends on unknown external authority {}", node.node_id, dep.target);
                }
                if dep.class != "external" {
                    bail!(
                        "authority edges stay authorization-class; found '{}' at {} -> {}",
                        dep.class,
                        node.node_id,
                        dep.target
                    );
                }
            } else {
                if dep.target == node.node_id {
                    bail!("self-dependency at {}", node.node_id);
                }
                let target_node = match by_id.get(dep.target.as_str()) {
                    Some(t) => *t,
                    None => bail!("{} depends on unknown node {}", node.node_id, dep.target),
                };
                if target_node.status == "superseded" {
                    bail!(
                        "active node {} depends on superseded generation {}",
                        node.node_id,
                        dep.target
                    );
                }
                if dep.class == "hard"
                    && node.role != target_node.role
                    && !CONTROL_ROLES.contains(&node.role.as_str())
                    && !lattice.contains(&(target_node.role.as_str(), node.role.as_str()))
                {
                    bail!(
                        "hard edge {} ({}) -> {} ({}) leaves the reviewed promotion lattice",
                        node.node_id,
                        node.role,
                        dep.target,
                        target_node.role
                    );
                }
                // Law 9 (immediate edge): external perlimports can never sit
                // directly under a native client/replay/claims consumer.
                if matches!(
                    node.role.as_str(),
                    "actual_client" | "installed_replay" | "claim_closeout"
                ) && (target_node.product_context == "external_compatibility"
                    || target_node.role == "external_compatibility")
                {
                    bail!(
                        "external compatibility stage feeds native evidence at {} -> {}",
                        node.node_id,
                        dep.target
                    );
                }
            }
        }

        // Per-node contract completeness and uniqueness.
        validate_node_contracts(m, node, &by_id)?;
    }

    // Exactly-four independent containments plus adapters stay guarded above;
    // per-node laws have run by now.
    // Acyclicity over hard node edges.
    let mut colour: BTreeMap<&str, u8> = BTreeMap::new();
    for id in by_id.keys() {
        colour.insert(id, 0);
    }
    for id in by_id.keys().copied().collect::<Vec<_>>() {
        visit_acyclic(id, &by_id, &mut colour)?;
    }

    // Law 9 over the complete hard dependency closure: no external-compat row
    // may sit ANYWHERE beneath a native client/replay/claims consumer, however
    // indirectly. Immediate edges alone would admit two-hop laundering.
    for node in &m.nodes {
        if !matches!(node.role.as_str(), "actual_client" | "installed_replay" | "claim_closeout") {
            continue;
        }
        let mut visited: BTreeSet<&str> = BTreeSet::new();
        let mut stack: Vec<&str> = Vec::new();
        for dep in &node.dependencies {
            if dep.class == "hard" && !dep.target.starts_with('#') {
                stack.push(dep.target.as_str());
            }
        }
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let current_node = match by_id.get(current) {
                Some(n) => *n,
                None => bail!("closure walk left the graph at {current}"),
            };
            if current_node.product_context == "external_compatibility"
                || current_node.role == "external_compatibility"
            {
                bail!(
                    "external compatibility stage sits inside the native evidence closure of {} via {}",
                    node.node_id,
                    current
                );
            }
            for dep in &current_node.dependencies {
                if dep.class == "hard" && !dep.target.starts_with('#') {
                    stack.push(dep.target.as_str());
                }
            }
        }
    }

    // Manifest-level limitations stay worded (visibility is a contract).
    for limitation in &m.limitations {
        if limitation.trim().is_empty() {
            bail!("top-level limitations carry an empty entry");
        }
    }

    // Cross-row discrimination laws: falsifiers and plausible-wrong
    // implementations are identities, never boilerplate (identity law 10).
    let mut falsifiers: BTreeSet<&str> = BTreeSet::new();
    let mut wrongs: BTreeSet<&str> = BTreeSet::new();
    for node in &m.nodes {
        if node.status != "active" || CONTROL_ROLES.contains(&node.role.as_str()) {
            continue;
        }
        if !falsifiers.insert(node.first_falsifier.trim()) {
            bail!("first_falsifier must stay unique across nodes; duplicated at {}", node.node_id);
        }
        if !wrongs.insert(node.plausible_wrong_implementation.trim()) {
            bail!(
                "plausible_wrong_implementation must stay unique across nodes; duplicated at {}",
                node.node_id
            );
        }
    }

    Ok(())
}

fn ensure_vocabulary(vocab: &BTreeSet<&str>, kind: &str) -> Result<()> {
    if vocab.is_empty() {
        bail!("{kind} vocabulary is empty");
    }
    Ok(())
}

fn ladder_ranks<'a>(ladder: &'a [LadderEntry], kind: &str) -> Result<BTreeMap<&'a str, u64>> {
    let mut map = BTreeMap::new();
    for (index, entry) in ladder.iter().enumerate() {
        if entry.rank != index as u64 {
            bail!("{kind} ladder ranks must be contiguous from zero");
        }
        if entry.owns.trim().is_empty() {
            bail!("{kind} ladder entry {} carries an empty ownership law", entry.value);
        }
        if map.insert(entry.value.as_str(), entry.rank).is_some() {
            bail!("{kind} ladder duplicates value {}", entry.value);
        }
    }
    Ok(map)
}

fn rank_lookup(ladder: &[LadderEntry]) -> BTreeMap<&str, u64> {
    ladder.iter().map(|e| (e.value.as_str(), e.rank)).collect()
}

/// Maximum application-evidence rank each role may ever reach. These are the
/// reviewed stage-transfer caps from identity laws 7/8; a manifest revision
/// that widens them must revise this law deliberately, together.
fn application_cap_for_role(role: &str) -> &'static str {
    match role {
        "code_action_adapter" | "completion_adapter" | "external_compatibility" => "returned",
        "proof_harness" | "exact_process" => "reference_applied",
        "actual_client" => "actual_client_applied",
        "installed_replay" => "installed_applied",
        _ => "none",
    }
}

fn wire_coherence(node: &TrainNode) -> Result<()> {
    match node.wire_kind.as_str() {
        "workspace_edit"
            if !matches!(
                node.product_context.as_str(),
                "code_action" | "all_contexts" | "external_compatibility"
            ) =>
        {
            bail!(
                "WorkspaceEdit payload outside a compatible context at {} ({})",
                node.node_id,
                node.product_context
            );
        }
        "completion_item" if node.product_context != "completion_item" => {
            bail!(
                "CompletionItem payload requires the completion_item context at {} ({})",
                node.node_id,
                node.product_context
            );
        }
        _ => {}
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
            if dep.class == "hard" && !dep.target.starts_with('#') {
                visit_acyclic(dep.target.as_str(), by_id, colour)?;
            }
        }
    }
    colour.insert(id, 2);
    Ok(())
}

/// Worded-contract completeness plus cross-row uniqueness discriminators.
fn validate_node_contracts(
    m: &Manifest,
    node: &TrainNode,
    by_id: &BTreeMap<&str, &TrainNode>,
) -> Result<()> {
    let id = node.node_id.as_str();
    let non_empty = |field: &str, value: &str| -> Result<()> {
        if value.trim().is_empty() {
            bail!("node {id} carries an empty {field}");
        }
        Ok(())
    };
    if node.status == "active" && !node.return_to_issue {
        bail!(
            "active node {id} must keep the return-to-issue boundary visible (set return_to_issue)"
        );
    }
    for (field, value) in [
        ("title", node.title.as_str()),
        ("programme_phase", node.programme_phase.as_str()),
        ("lane", node.lane.as_str()),
        ("one_pr_outcome", node.one_pr_outcome.as_str()),
        ("rollback_unit", node.rollback_unit.as_str()),
        ("authority_before", node.authority_before.as_str()),
        ("authority_after", node.authority_after.as_str()),
        ("conflict_key", node.conflict_key.as_str()),
        ("first_falsifier", node.first_falsifier.as_str()),
        ("plausible_wrong_implementation", node.plausible_wrong_implementation.as_str()),
        ("focused_proof_owner", node.focused_proof_owner.as_str()),
        ("documentation_impact", node.documentation_impact.as_str()),
        ("docs_owner", node.docs_owner.as_str()),
        ("changelog_disposition", node.changelog_disposition.as_str()),
        ("transferred_to", node.transferred_to.as_str()),
        ("expansion_policy", node.expansion_policy.as_str()),
        ("probe_owner", node.probe_owner.as_str()),
    ] {
        non_empty(field, value)?;
    }
    if !node.spec.owner.starts_with('#') {
        bail!("node {id} spec owner must be an issue reference");
    }
    for (field, value) in [
        ("spec.packet", node.spec.packet.as_str()),
        ("spec.stale_check", node.spec.stale_check.as_str()),
        ("adjacent_defect_transfer.subject", node.adjacent_defect_transfer.subject.as_str()),
        (
            "adjacent_defect_transfer.first_divergence",
            node.adjacent_defect_transfer.first_divergence.as_str(),
        ),
        ("adjacent_defect_transfer.owner", node.adjacent_defect_transfer.owner.as_str()),
        ("pr_contract.title_prefix", node.pr_contract.title_prefix.as_str()),
    ] {
        non_empty(field, value)?;
    }
    if !matches!(
        node.spec.requirement.as_str(),
        "none"
            | "issue_plan_sufficient"
            | "existing_contract_sufficient"
            | "generated_spec"
            | "ADR_or_spec_update"
    ) {
        bail!("node {id} uses unknown spec_requirement {}", node.spec.requirement);
    }
    let governance_row = CONTROL_ROLES.contains(&node.role.as_str()) || node.status == "superseded";
    if node.spec.requirement == "none" && !governance_row {
        bail!(
            "node {id} ({}) refuses to name its spec requirement; vague instructions are invalid",
            node.role
        );
    }
    if !matches!(node.documentation_impact.as_str(), "none" | "generated" | "authored_bounded") {
        bail!("node {id} uses unknown documentation_impact {}", node.documentation_impact);
    }
    for (field, entries) in [
        ("authorities_consumed", &node.authorities_consumed),
        ("controller_chain", &node.controller_chain),
        ("allowed_components", &node.allowed_components),
        ("owned_paths", &node.owned_paths),
        ("stop_conditions", &node.stop_conditions),
        ("observations", &node.observations),
        ("review_forward_questions", &node.review_forward_questions),
        ("next_consumers", &node.next_consumers),
        ("pr_contract.handoff_fields", &node.pr_contract.handoff_fields),
        ("limitations", &node.limitations),
        ("proof_command_templates", &node.proof_command_templates),
    ] {
        for entry in entries {
            non_empty(field, entry)?;
        }
    }
    for reference in node.controller_chain.iter().chain(node.next_consumers.iter()) {
        // "none: …" entries are documented terminal markers, not references.
        if reference.starts_with("none") || reference.starts_with("n/a") {
            continue;
        }
        if !by_id.contains_key(reference.as_str()) {
            bail!(
                "node {id} references unknown node {reference} in controller_chain/next_consumers"
            );
        }
    }
    for component in &node.forbidden_adjacent {
        non_empty("forbidden_adjacent.component", component.component.as_str())?;
        if !by_id.contains_key(component.owner_node.as_str()) && !component.owner_node.contains(' ')
        {
            bail!("node {id} forbids adjacency to unknown owner node {}", component.owner_node);
        }
    }
    for alias in node.authorities_consumed.iter() {
        if !alias.starts_with('#') {
            bail!("node {id} consumes a non-issue authority: {alias}");
        }
    }

    // Proof-shape obligations.
    let evidence_consumer = matches!(
        node.role.as_str(),
        "semantic_assessment" | "semantic_decision" | "product_admission"
    );
    let evidence_holder = matches!(
        node.role.as_str(),
        "proof_harness"
            | "exact_process"
            | "actual_client"
            | "installed_replay"
            | "representative_corpus"
    );
    let in_refusal_registry = m.refusal_surfaces.iter().any(|s| s == id);
    let proof_obligated =
        node.status == "active" && (evidence_consumer || evidence_holder || in_refusal_registry);
    if proof_obligated && node.proofs.negative.is_empty() {
        bail!(
            "node {id} ({}/{}) must publish at least one negative proof id",
            node.role,
            node.status
        );
    }
    if evidence_holder && node.proofs.positive.is_empty() {
        bail!("evidence-holder {id} must publish at least one positive proof id");
    }

    // Command-surface honesty: only commands proven on the current surface.
    for template in &node.proof_command_templates {
        let matched = m
            .verification_surface
            .allowed_command_prefixes
            .iter()
            .any(|prefix| template.starts_with(prefix.as_str()));
        if !matched {
            bail!(
                "node {id} records command '{template}' outside the proven verification surface; \
                 borrow only commands current main demonstrates"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "import_cleanup_train_manifest_tests.rs"]
mod tests;
