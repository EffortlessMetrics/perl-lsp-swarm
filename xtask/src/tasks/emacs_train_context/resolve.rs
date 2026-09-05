//! The exact-tree context resolver (CTXENG `#11756`).
//!
//! Deterministic, offline, fail-closed. For one stable train node the
//! resolver recomputes a bounded `emacs_node_context.v1` packet from exactly
//! three data inputs — the stable `emacs_train.v1` manifest (E01), the
//! `emacs_train_revision.v1` ledger (E01R), and the population mapping
//! document — validated against the exact current tree. Nothing is cached:
//! every run re-derives, so a stale packet cannot be emitted; and every
//! packet embeds the digests a consumer needs to detect reuse across trees.
//!
//! Laws are numbered `L##` and each maps to a falsifier of the governing
//! issues `#11718`/`#11756`. A law violation is a hard instrument failure:
//! the resolver refuses to emit a packet instead of degrading it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde_json::Value;

use super::digest::{composite_digest, git_identity, sha256_file, sha256_hex, title_fingerprint};
use super::model::{
    ContextBinding, KINDS, MANIFEST_SCHEMA_NAME, MAPPING_SCHEMA_NAME, MappingDocument,
    NodeContextPacket, NodeMapping, PACKET_SCHEMA_NAME, PacketBounds, PacketCheckedSpec,
    PacketComponent, PacketDependency, PacketDigest, PacketGap, PacketGenerated, PacketGraph,
    PacketInstruction, PacketNode, PacketNotAuthority, PacketOmission, PacketPrivacy,
    PacketProposition, PacketRevisionCurrency, PacketSpec, PacketTest, ROLES, SCHEMA_VERSION,
    TrainManifest, TrainNode,
};

/// Repository-relative locations of the engine's data inputs.
pub const MANIFEST_RELATIVE_PATH: &str = ".spec/10918-emacs-train-graph/train.manifest.json";
pub const LEDGER_RELATIVE_PATH: &str = ".spec/11770-emacs-train-revisions/revisions.ledger.json";
pub const MAPPING_RELATIVE_PATH: &str = ".spec/11756-emacs-context-engine/context.mappings.v1.json";
pub const ARCHITECTURE_BUNDLE: &str = ".spec/11716-emacs-support-architecture";
pub const LEDGER_SCHEMA_NAME: &str = "emacs_train_revision.v1";

/// Maximum bytes scanned per mapped file for symbol anchoring. Files larger
/// than this fail closed instead of being scanned unboundedly (L11).
const MAX_SYMBOL_SCAN_BYTES: u64 = 2 * 1024 * 1024;

/// Lanes whose population belongs to the substrate population leaf (#11757):
/// foundation/substrate, subjects, typed observation, adapters, profiles,
/// actual hosts and the root matrices. Plane-machinery lanes (stable-train,
/// spec-plane, context-plane, packet-plane, programme, dogfood) are not named
/// by either population leaf and route to the engine owner (#11756).
const SUBSTRATE_LANES: [&str; 9] = [
    "foundation",
    "subject",
    "observation",
    "adapter-eglot",
    "adapter-lsp",
    "profile-eglot",
    "profile-lsp",
    "actual-host-eglot",
    "actual-host-lsp",
];

/// Lanes whose population belongs to the projection population leaf (#11758):
/// the root matrices (including their fan-ins), public replay and projection
/// surfaces.
const PROJECTION_LANES: [&str; 5] =
    ["public", "projection", "root-eglot", "root-lsp", "root-fan-in"];

/// The outcome of resolving one node.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// A fully validated context packet (`status = "ok"`).
    Packet(Box<NodeContextPacket>),
    /// A precise, typed mapping blocker packet (`status = "mapping_gap"`).
    Gap(Box<NodeContextPacket>),
}

impl Resolution {
    pub fn packet(&self) -> &NodeContextPacket {
        match self {
            Resolution::Packet(packet) | Resolution::Gap(packet) => packet,
        }
    }

    pub fn is_gap(&self) -> bool {
        matches!(self, Resolution::Gap(_))
    }
}

/// Everything the resolver reads, loaded and validated once.
pub struct EngineInputs {
    pub manifest: TrainManifest,
    pub manifest_digest: String,
    pub ledger: Value,
    pub ledger_digest: String,
    pub mapping: MappingDocument,
    pub mapping_digest: String,
    pub architecture_digests: Vec<PacketDigest>,
    pub git_commit: String,
    pub git_tree: String,
}

pub fn load_inputs(root: &Path) -> Result<EngineInputs> {
    load_inputs_with_git(root, None)
}

/// Core loader with an optional git-identity override used only by the
/// falsifier fixtures (synthetic trees are not git repositories). Production
/// callers always pass `None`, which binds through local git and fails
/// closed without it.
pub(crate) fn load_inputs_with_git(
    root: &Path,
    git: Option<(String, String)>,
) -> Result<EngineInputs> {
    let manifest_path = root.join(MANIFEST_RELATIVE_PATH);
    let ledger_path = root.join(LEDGER_RELATIVE_PATH);
    let mapping_path = root.join(MAPPING_RELATIVE_PATH);

    let manifest_bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("reading stable train manifest {}", manifest_path.display()))?;
    let manifest: TrainManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parsing {} as typed {} (L01: a malformed or extended stable manifest fails closed)",
            manifest_path.display(),
            MANIFEST_SCHEMA_NAME
        )
    })?;
    validate_manifest(&manifest).with_context(|| {
        format!(
            "validating the stable train manifest {} (structural laws L01-L04)",
            manifest_path.display()
        )
    })?;

    let ledger_bytes = std::fs::read(&ledger_path)
        .with_context(|| format!("reading revision ledger {}", ledger_path.display()))?;
    let ledger: Value = serde_json::from_slice(&ledger_bytes)
        .with_context(|| format!("parsing revision ledger {}", ledger_path.display()))?;
    let ledger_schema = ledger.get("schema").and_then(Value::as_str).unwrap_or_default();
    if ledger_schema != LEDGER_SCHEMA_NAME {
        // L05: the revision plane is consumed as data under its own schema;
        // anything else is a wrong subject, not a degraded ledger.
        bail!(
            "L05 wrong subject: revision ledger {} declares schema '{ledger_schema}', expected \
             '{LEDGER_SCHEMA_NAME}'",
            ledger_path.display()
        );
    }

    let mapping_bytes = std::fs::read(&mapping_path)
        .with_context(|| format!("reading context mapping document {}", mapping_path.display()))?;
    let mapping: MappingDocument = serde_json::from_slice(&mapping_bytes).with_context(|| {
        format!(
            "parsing {} as typed {} (L06: unknown mapping fields fail closed)",
            mapping_path.display(),
            MAPPING_SCHEMA_NAME
        )
    })?;
    validate_mapping(&manifest, &mapping).with_context(|| {
        format!(
            "validating the population mapping document {} (laws L06-L09)",
            mapping_path.display()
        )
    })?;

    let architecture_digests = digest_bundle_files(root, ARCHITECTURE_BUNDLE)?;
    let (git_commit, git_tree) = match git {
        Some(identity) => identity,
        None => git_identity(root).with_context(|| {
            "binding the engine to an exact tree: local git identity is mandatory (L10: an \
             unbound packet is never emitted)"
        })?,
    };

    Ok(EngineInputs {
        manifest,
        manifest_digest: sha256_hex(&manifest_bytes),
        ledger,
        ledger_digest: sha256_hex(&ledger_bytes),
        mapping,
        mapping_digest: sha256_hex(&mapping_bytes),
        architecture_digests,
        git_commit,
        git_tree,
    })
}

// ---------------------------------------------------------------------------
// Structural laws over the stable inputs.
// ---------------------------------------------------------------------------

/// L01 schema identity, L02 unique node identities, L03 title fingerprint
/// binding, L04 edge symmetry/existence.
pub(crate) fn validate_manifest(manifest: &TrainManifest) -> Result<()> {
    if manifest.schema != MANIFEST_SCHEMA_NAME {
        bail!(
            "L01 wrong subject: manifest schema '{}' does not match '{MANIFEST_SCHEMA_NAME}'",
            manifest.schema
        );
    }
    if manifest.schema_version != SCHEMA_VERSION {
        bail!(
            "L01 wrong subject: manifest schema_version {} does not match {SCHEMA_VERSION}",
            manifest.schema_version
        );
    }
    let mut seen_ids = BTreeMap::new();
    let mut seen_issues = BTreeMap::new();
    for node in &manifest.nodes {
        if seen_ids.insert(&node.node_id, node.issue).is_some() {
            bail!("L02 duplicate node_id '{}' in the stable manifest", node.node_id);
        }
        if seen_issues.insert(node.issue, &node.node_id).is_some() {
            bail!(
                "L02 duplicate issue {} shared by nodes '{}' and '{}' in the stable manifest",
                node.issue,
                seen_issues[&node.issue],
                node.node_id
            );
        }
        let fingerprint = title_fingerprint(&node.title);
        if fingerprint != node.title_fingerprint {
            // L03: the manifest bytes and the node identity they claim must
            // agree; a silent title edit is a semantic revision, not metadata.
            bail!(
                "L03 stale identity: node {} title fingerprint {} but exact title recompute is \
                 {fingerprint}; route the movement through the E01R ledger",
                node.node_id,
                node.title_fingerprint
            );
        }
    }
    for node in &manifest.nodes {
        for dependency in &node.dependencies {
            if dependency.target.starts_with('#') {
                continue;
            }
            if manifest.node(&dependency.target).is_none() {
                bail!(
                    "L04 dangling edge: node {} depends on unknown target '{}'",
                    node.node_id,
                    dependency.target
                );
            }
        }
        for successor in &node.successors {
            if manifest.node(successor).is_none() {
                bail!(
                    "L04 dangling edge: node {} names unknown successor '{successor}'",
                    node.node_id
                );
            }
        }
    }
    for node in &manifest.nodes {
        for dependency in &node.dependencies {
            let Some(target) = manifest.node(&dependency.target) else {
                continue;
            };
            if !target.successors.contains(&node.node_id) {
                bail!(
                    "L04 asymmetric edge: {} -> {} is declared as a dependency but the \
                     target does not list it as a successor",
                    node.node_id,
                    dependency.target
                );
            }
        }
    }
    Ok(())
}

/// L06 mapping schema/node consistency, L07 vocabulary bounds, L08 status
/// shape, L09 declared bounds sanity.
pub(crate) fn validate_mapping(manifest: &TrainManifest, mapping: &MappingDocument) -> Result<()> {
    if mapping.schema != MAPPING_SCHEMA_NAME {
        bail!(
            "L06 wrong subject: mapping schema '{}' does not match '{MAPPING_SCHEMA_NAME}'",
            mapping.schema
        );
    }
    if mapping.schema_version != SCHEMA_VERSION {
        bail!(
            "L06 wrong subject: mapping schema_version {} does not match {SCHEMA_VERSION}",
            mapping.schema_version
        );
    }
    if mapping.consumed_manifest.schema != MANIFEST_SCHEMA_NAME {
        bail!(
            "L06 wrong subject: mapping declares consumed manifest schema '{}', expected \
             '{MANIFEST_SCHEMA_NAME}'",
            mapping.consumed_manifest.schema
        );
    }
    if mapping.consumed_ledger.schema != LEDGER_SCHEMA_NAME {
        bail!(
            "L06 wrong subject: mapping declares consumed ledger schema '{}', expected \
             '{LEDGER_SCHEMA_NAME}'",
            mapping.consumed_ledger.schema
        );
    }
    let bounds = &mapping.bounds;
    if bounds.max_components_per_node == 0
        || bounds.max_tests_per_node == 0
        || bounds.max_read_set == 0
        || bounds.max_write_set == 0
        || bounds.max_not_authority == 0
    {
        bail!(
            "L09 invalid bounds: every maximum must be positive so a declared minimum context is enforceable"
        );
    }
    let mut seen = BTreeMap::new();
    for node_mapping in &mapping.nodes {
        if seen.insert(node_mapping.node_id.as_str(), ()).is_some() {
            bail!("L02 duplicate mapping entry for node '{}'", node_mapping.node_id);
        }
        let Some(node) = manifest.node(&node_mapping.node_id) else {
            bail!(
                "L06 unknown node: mapping entry '{}' has no stable manifest node",
                node_mapping.node_id
            );
        };
        match node_mapping.status.as_str() {
            "mapped" => validate_mapped_node(node, node_mapping, mapping)?,
            "unmapped" => {
                let Some(blocker) = &node_mapping.blocker else {
                    bail!(
                        "L08 imprecise blocker: unmapped node '{}' must carry a reason, owner \
                         issue and action",
                        node_mapping.node_id
                    );
                };
                if !node_mapping.components.is_empty()
                    || !node_mapping.tests.is_empty()
                    || !node_mapping.generated.is_empty()
                {
                    bail!(
                        "L08 inconsistent status: unmapped node '{}' carries population content",
                        node_mapping.node_id
                    );
                }
                if blocker.action != "return_to_issue" {
                    bail!(
                        "L08 unknown blocker action '{}' for node '{}'",
                        blocker.action,
                        node_mapping.node_id
                    );
                }
                if manifest.node_by_issue(blocker.owner_issue).is_none() {
                    bail!(
                        "L08 blocker owner issue {} of node '{}' is not a stable train node",
                        blocker.owner_issue,
                        node_mapping.node_id
                    );
                }
            }
            other => {
                bail!("L08 unknown mapping status '{other}' for node '{}'", node_mapping.node_id)
            }
        }
    }
    // L12 (global): no two nodes may claim the same production symbol anchor,
    // and a same-named symbol can never satisfy two different owners.
    let mut production_symbols: BTreeMap<&str, &str> = BTreeMap::new();
    for node_mapping in &mapping.nodes {
        if node_mapping.status != "mapped" {
            continue;
        }
        for component in &node_mapping.components {
            if component.role != "production" {
                continue;
            }
            let Some(symbol) = &component.symbol else {
                continue;
            };
            if let Some(owner) = production_symbols.insert(symbol.as_str(), &node_mapping.node_id) {
                bail!(
                    "L12 ambiguous ownership: production symbol '{symbol}' is claimed by both \
                     '{owner}' and '{}'; same-named symbols cannot satisfy two owners",
                    node_mapping.node_id
                );
            }
        }
    }
    Ok(())
}

fn validate_mapped_node(
    _node: &TrainNode,
    node_mapping: &NodeMapping,
    mapping: &MappingDocument,
) -> Result<()> {
    let bounds = &mapping.bounds;
    if node_mapping.components.len() > bounds.max_components_per_node {
        bail!(
            "L07 bounds violation: node '{}' declares {} components, maximum is {}",
            node_mapping.node_id,
            node_mapping.components.len(),
            bounds.max_components_per_node
        );
    }
    if node_mapping.tests.len() > bounds.max_tests_per_node {
        bail!(
            "L07 bounds violation: node '{}' declares {} tests, maximum is {}",
            node_mapping.node_id,
            node_mapping.tests.len(),
            bounds.max_tests_per_node
        );
    }
    if node_mapping.read_set.len() > bounds.max_read_set {
        bail!(
            "L07 bounds violation: node '{}' declares a read set of {}, maximum is {}",
            node_mapping.node_id,
            node_mapping.read_set.len(),
            bounds.max_read_set
        );
    }
    if node_mapping.write_set.len() > bounds.max_write_set {
        bail!(
            "L07 bounds violation: node '{}' declares a write set of {}, maximum is {}",
            node_mapping.node_id,
            node_mapping.write_set.len(),
            bounds.max_write_set
        );
    }
    if node_mapping.not_authority.len() > bounds.max_not_authority {
        bail!(
            "L07 bounds violation: node '{}' declares {} not-authority rows, maximum is {}",
            node_mapping.node_id,
            node_mapping.not_authority.len(),
            bounds.max_not_authority
        );
    }
    let mut component_ids: BTreeMap<String, ()> = BTreeMap::new();
    let mut anchors: BTreeMap<String, String> = BTreeMap::new();
    for component in &node_mapping.components {
        if component_ids.insert(component.component_id.clone(), ()).is_some() {
            bail!(
                "L02 duplicate component_id '{}' in node '{}'",
                component.component_id,
                node_mapping.node_id
            );
        }
        if !ROLES.contains(&component.role.as_str()) {
            bail!(
                "L07 unknown role '{}' on component '{}' of node '{}'",
                component.role,
                component.component_id,
                node_mapping.node_id
            );
        }
        if !KINDS.contains(&component.kind.as_str()) {
            bail!(
                "L07 unknown kind '{}' on component '{}' of node '{}'",
                component.kind,
                component.component_id,
                node_mapping.node_id
            );
        }
        let anchor = format!("{}:{}", component.path, component.symbol.clone().unwrap_or_default());
        if anchors.insert(anchor.clone(), component.role.clone()).is_some() {
            bail!("L02 duplicate path/symbol anchor '{anchor}' in node '{}'", node_mapping.node_id);
        }
        if let Some(kind) = component.symbol_kind.as_deref() {
            const SYMBOL_KINDS: [&str; 4] =
                ["rust_item", "elisp_defun", "elisp_defconst", "text_contains"];
            if !SYMBOL_KINDS.contains(&kind) {
                bail!(
                    "L07 unknown symbol_kind '{kind}' on component '{}' of node '{}'",
                    component.component_id,
                    node_mapping.node_id
                );
            }
        } else if component.symbol.is_some() {
            bail!(
                "L07 symbol without symbol_kind on component '{}' of node '{}'",
                component.component_id,
                node_mapping.node_id
            );
        }
        if component.symbol.is_none() && component.symbol_kind.is_some() {
            bail!(
                "L07 symbol_kind without symbol on component '{}' of node '{}'",
                component.component_id,
                node_mapping.node_id
            );
        }
    }
    // L13 minimum context: checked after vocabulary validation so an invalid
    // role fails as a schema defect, not as a missing production seam.
    let has_production =
        node_mapping.components.iter().any(|component| component.role == "production");
    if !has_production && node_mapping.no_production_component_reason.is_none() {
        bail!(
            "L13 minimum context: mapped node '{}' has no production component and no \
             no_production_component_reason",
            node_mapping.node_id
        );
    }
    for write in &node_mapping.write_set {
        // L14 broad-write refusal: an ambiguous or lazy mapping must never
        // become permission to edit a whole directory.
        if write.ends_with('/')
            || write.contains("..")
            || !write.contains('/')
            || write.split('/').count() < 2
            || write.ends_with("/*")
        {
            bail!(
                "L14 broad write set: node '{}' declares '{write}'; expected write sets name \
                 exact files (path with at least two segments, no wildcard, no traversal)",
                node_mapping.node_id
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-node resolution.
// ---------------------------------------------------------------------------

/// Resolve the context (or precise typed blocker) for one node id or issue
/// number. `spec` accepts `CTXENG`, `11756`, or `#11756`.
pub fn resolve_spec(root: &Path, inputs: &EngineInputs, spec: &str) -> Result<Resolution> {
    let trimmed = spec.trim().trim_start_matches('#');
    let node = if let Ok(issue) = trimmed.parse::<u64>() {
        inputs
            .manifest
            .node_by_issue(issue)
            .ok_or_else(|| color_eyre::eyre::eyre!("no stable train node with issue {issue}"))?
    } else {
        inputs
            .manifest
            .node(trimmed)
            .ok_or_else(|| color_eyre::eyre::eyre!("no stable train node with id '{trimmed}'"))?
    };
    resolve_node(root, inputs, node)
}

pub fn resolve_node(root: &Path, inputs: &EngineInputs, node: &TrainNode) -> Result<Resolution> {
    let mapping_entry = inputs.mapping.nodes.iter().find(|entry| entry.node_id == node.node_id);

    let base = BasePacket { inputs, node, mapping_entry };

    match mapping_entry {
        Some(entry) if entry.status == "mapped" => resolve_mapped(root, base, entry),
        Some(entry) => {
            let blocker = entry
                .blocker
                .as_ref()
                .ok_or_else(|| color_eyre::eyre::eyre!("L08: unmapped entry without a blocker"))?;
            Ok(Resolution::Gap(build_gap_packet(root, base, &blocker.reason, blocker.owner_issue)?))
        }
        None => {
            // The node is inside the stable denominator but has no population
            // mapping entry yet: a precise blocker naming the owning
            // population leaf, never a silent skip and never a guessed path.
            let owner = auto_population_owner(inputs, node);
            let reason = format!(
                "no exact-tree population mapping exists for this node yet; context-plane \
                 population is pending (owner #{owner})"
            );
            Ok(Resolution::Gap(build_gap_packet(root, base, &reason, owner)?))
        }
    }
}

struct BasePacket<'a> {
    inputs: &'a EngineInputs,
    node: &'a TrainNode,
    mapping_entry: Option<&'a NodeMapping>,
}

/// Deterministic population ownership for nodes without a mapping entry.
fn auto_population_owner(inputs: &EngineInputs, node: &TrainNode) -> u64 {
    if SUBSTRATE_LANES.contains(&node.lane.as_str()) {
        inputs.mapping.population_ownership.substrate_population
    } else if PROJECTION_LANES.contains(&node.lane.as_str()) {
        inputs.mapping.population_ownership.projection_population
    } else {
        inputs.mapping.population_ownership.engine
    }
}

fn build_gap_packet(
    root: &Path,
    base: BasePacket<'_>,
    reason: &str,
    owner_issue: u64,
) -> Result<Box<NodeContextPacket>> {
    let instructions = instruction_chain(root, &[])?;
    Ok(Box::new(NodeContextPacket {
        schema: PACKET_SCHEMA_NAME.to_owned(),
        schema_version: SCHEMA_VERSION,
        status: "mapping_gap".to_owned(),
        binding: build_binding(base.inputs, &[]),
        node: packet_node(base.node),
        proposition: packet_proposition(base.node),
        spec: packet_spec(base.node),
        graph: packet_graph(base.inputs, base.node),
        revision_currency: revision_currency(base.inputs, base.node),
        instructions,
        checked_specs: checked_specs(root, base.node, base.mapping_entry)?,
        components: Vec::new(),
        tests: Vec::new(),
        generated: Vec::new(),
        read_set: Vec::new(),
        write_set: Vec::new(),
        not_authority: Vec::new(),
        gaps: vec![PacketGap {
            subject: "exact-tree population mapping".to_owned(),
            reason: reason.to_owned(),
            owner_issue,
            action: "return_to_issue".to_owned(),
        }],
        bounds: packet_bounds(base.inputs, Vec::new()),
        privacy: packet_privacy(),
    }))
}

fn resolve_mapped(root: &Path, base: BasePacket<'_>, entry: &NodeMapping) -> Result<Resolution> {
    let node = base.node;
    let engine = base.inputs;

    // Family constraint (L15): client-family material may only appear in a
    // node of the same family; Eglot material can never satisfy an lsp-mode
    // node and vice versa.
    let node_family = node
        .lane
        .strip_suffix("eglot")
        .map(|_| "eglot")
        .or_else(|| node.lane.strip_suffix("lsp").map(|_| "lsp"));
    for component in &entry.components {
        if let Some(family) = &component.client_family {
            let Some(expected) = node_family else {
                bail!(
                    "L15 cross-client: component '{}' of node '{}' declares client family \
                     '{family}' but lane '{}' is not client-scoped",
                    component.component_id,
                    node.node_id,
                    node.lane
                );
            };
            if family != expected {
                bail!(
                    "L15 cross-client: component '{}' of node '{}' (lane {}) declares client \
                     family '{family}' but the node's family is '{expected}'",
                    component.component_id,
                    node.node_id,
                    node.lane
                );
            }
        }
    }

    // Components, tests and generated inputs are validated against the exact
    // tree: every declared path must exist and every declared symbol must be
    // anchored at that exact path (L16 stale-path / same-name-symbol law).
    let mut components = Vec::with_capacity(entry.components.len());
    for component in &entry.components {
        let path = validate_relative_path(&component.path)?;
        let absolute = root.join(&path);
        if !absolute.is_file() {
            bail!(
                "L16 stale path: component '{}' of node '{}' declares '{}' which does not \
                 exist as a file on this tree",
                component.component_id,
                node.node_id,
                component.path
            );
        }
        let sha256 = sha256_file(&absolute)?;
        if let Some(symbol) = &component.symbol {
            let kind = component.symbol_kind.as_deref().unwrap_or("text_contains");
            verify_symbol(&absolute, &component.path, kind, symbol)
                .with_context(|| format!("validating component '{}'", component.component_id))?;
        }
        components.push(PacketComponent {
            component_id: component.component_id.clone(),
            role: component.role.clone(),
            kind: component.kind.clone(),
            path: component.path.clone(),
            symbol: component.symbol.clone(),
            symbol_kind: component.symbol_kind.clone(),
            client_family: component.client_family.clone(),
            sha256,
            notes: component.notes.clone(),
        });
    }

    let mut tests = Vec::with_capacity(entry.tests.len());
    for test in &entry.tests {
        if !["falsifier", "control", "positive"].contains(&test.kind.as_str()) {
            bail!("L07 unknown test kind '{}' on node '{}'", test.kind, node.node_id);
        }
        let path = validate_relative_path(&test.path)?;
        if !root.join(&path).is_file() {
            bail!(
                "L16 stale path: test '{}' of node '{}' declares '{}' which does not exist as \
                 a file on this tree",
                test.selector.as_deref().unwrap_or("-"),
                node.node_id,
                test.path
            );
        }
        let sha256 = sha256_file(&root.join(&path))?;
        tests.push(PacketTest {
            path: test.path.clone(),
            selector: test.selector.clone(),
            kind: test.kind.clone(),
            sha256,
        });
    }

    for generated in &entry.generated {
        let input = validate_relative_path(&generated.input)?;
        let output = validate_relative_path(&generated.output)?;
        let generator = validate_relative_path(&generated.generator)?;
        // L17: generated output is current-tree evidence only. Both sides and
        // the generator must exist; a generated artifact can never satisfy a
        // production role (enforced by the role/kind table at mapping
        // validation) and is never executed here.
        for (label, path) in [("input", input), ("output", output), ("generator", generator)] {
            if !root.join(&path).is_file() {
                bail!(
                    "L17 stale generated surface: {label} '{}' of node '{}' does not exist on \
                     this tree",
                    path.display(),
                    node.node_id
                );
            }
        }
    }

    // Role/kind law (L18): a helper, fixture, schema, spec, generated or doc
    // file can never be presented as a production implementation.
    for component in &entry.components {
        let allowed = role_allows_kind(&component.role, &component.kind);
        if !allowed {
            bail!(
                "L18 helper-as-production: component '{}' of node '{}' declares role '{}' with \
                 kind '{}'; this role cannot be satisfied by that kind",
                component.component_id,
                node.node_id,
                component.role,
                component.kind
            );
        }
    }

    // Read sets must exist on the exact tree (L16): they are navigation, not
    // wishes. Write sets are expected writes and may not exist yet, but they
    // passed the broad-write law at mapping validation.
    for read in &entry.read_set {
        let path = validate_relative_path(read)?;
        if !root.join(&path).is_file() {
            bail!(
                "L16 stale read set: '{}' of node '{}' does not exist on this tree",
                read,
                node.node_id
            );
        }
    }

    let instruction_paths: Vec<String> = entry
        .components
        .iter()
        .map(|component| component.path.clone())
        .chain(entry.tests.iter().map(|test| test.path.clone()))
        .collect();
    let instructions = instruction_chain(root, &instruction_paths)?;

    let mut packet = NodeContextPacket {
        schema: PACKET_SCHEMA_NAME.to_owned(),
        schema_version: SCHEMA_VERSION,
        status: "ok".to_owned(),
        binding: build_binding(engine, &components),
        node: packet_node(node),
        proposition: packet_proposition(node),
        spec: packet_spec(node),
        graph: packet_graph(engine, node),
        revision_currency: revision_currency(engine, node),
        instructions,
        checked_specs: checked_specs(root, node, base.mapping_entry)?,
        components,
        tests,
        generated: entry
            .generated
            .iter()
            .map(|generated| PacketGenerated {
                input: generated.input.clone(),
                output: generated.output.clone(),
                generator: generated.generator.clone(),
                stale_check: generated.stale_check.clone(),
            })
            .collect(),
        read_set: entry.read_set.clone(),
        write_set: entry.write_set.clone(),
        not_authority: entry
            .not_authority
            .iter()
            .map(|row| PacketNotAuthority {
                path_or_symbol: row.path_or_symbol.clone(),
                reason: row.reason.clone(),
                owner: row.owner.clone(),
            })
            .collect(),
        gaps: Vec::new(),
        bounds: packet_bounds(engine, Vec::new()),
        privacy: packet_privacy(),
    };

    // L13: explicit declared gaps stay visible instead of disappearing.
    packet.gaps = entry
        .blocker
        .as_ref()
        .map(|blocker| {
            vec![PacketGap {
                subject: "declared residual gap".to_owned(),
                reason: blocker.reason.clone(),
                owner_issue: blocker.owner_issue,
                action: blocker.action.clone(),
            }]
        })
        .unwrap_or_default();

    Ok(Resolution::Packet(Box::new(packet)))
}

/// L18 role/kind law table. Production seams are only implementable by real
/// implementation sources; tests/fixtures/schemas/specs/generated/docs are
/// evidence surfaces with their own roles.
fn role_allows_kind(role: &str, kind: &str) -> bool {
    match role {
        "production" => matches!(kind, "rust_source" | "elisp_source" | "script"),
        "test_falsifier" | "test_control" => matches!(kind, "rust_test" | "elisp_test"),
        "fixture" => kind == "fixture",
        "schema" => kind == "schema_json",
        "spec" => kind == "spec_bundle",
        "population_data" => kind == "json_data",
        "generated_output" => kind == "generated",
        "doc" => kind == "doc",
        "policy" => kind == "policy_toml",
        "script" => kind == "script",
        "instruction" => kind == "doc",
        _ => false,
    }
}

fn validate_relative_path(path: &str) -> Result<PathBuf> {
    const MAX_PATH_BYTES: usize = 512;
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || path.contains("..")
        || path.contains('\\')
        || path.starts_with('/')
        || path.ends_with('/')
    {
        // L11 privacy/bounds: only normalized repository-relative paths are
        // ever accepted or emitted.
        bail!("L11 invalid path '{path}': expected a normalized repository-relative path");
    }
    if path.len() > MAX_PATH_BYTES {
        // L11 bounds: emitted packets stay bounded in bytes as well as in
        // entry counts; an oversized path is a mapping defect, not a bigger
        // valid packet.
        bail!(
            "L11 bound exceeded: path is {} bytes, above the {MAX_PATH_BYTES}-byte ceiling",
            path.len()
        );
    }
    Ok(candidate.to_path_buf())
}

/// L16 symbol anchoring. A symbol is verified at its exact declared path with
/// a bounded line scan; a same-named symbol anywhere else cannot satisfy the
/// mapping.
fn verify_symbol(absolute: &Path, relative: &str, kind: &str, symbol: &str) -> Result<()> {
    let metadata = std::fs::metadata(absolute)
        .with_context(|| format!("statting {relative} for symbol anchoring"))?;
    if metadata.len() > MAX_SYMBOL_SCAN_BYTES {
        bail!(
            "L11 bound exceeded: {relative} is {} bytes, above the {MAX_SYMBOL_SCAN_BYTES}-byte \
             scan ceiling",
            metadata.len()
        );
    }
    let text = std::fs::read_to_string(absolute)
        .with_context(|| format!("reading {relative} for symbol anchoring"))?;
    let anchored = text.lines().any(|line| match kind {
        "rust_item" => rust_line_declares(line, symbol),
        "elisp_defun" => elisp_line_declares(line, "defun", symbol),
        "elisp_defconst" => elisp_line_declares(line, "defconst", symbol),
        _ => text.contains(symbol),
    });
    if !anchored {
        bail!(
            "L16 stale or wrong-crate mapping: symbol '{symbol}' ({kind}) is not anchored at \
             {relative} on this tree; a same-named symbol elsewhere cannot satisfy the mapping"
        );
    }
    Ok(())
}

fn rust_line_declares(line: &str, symbol: &str) -> bool {
    let mut trimmed = line.trim_start();
    loop {
        // Strip visibility modifiers without accepting them as the item.
        if let Some(rest) = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub(super) "))
            .or_else(|| trimmed.strip_prefix("pub "))
        {
            trimmed = rest;
            continue;
        }
        break;
    }
    const ITEM_KEYWORDS: [&str; 7] = ["fn", "struct", "enum", "const", "static", "trait", "mod"];
    for keyword in ITEM_KEYWORDS {
        let prefix = format!("{keyword} ");
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let name: String =
                rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            return name == symbol;
        }
    }
    false
}

fn elisp_line_declares(line: &str, form: &str, symbol: &str) -> bool {
    let trimmed = line.trim_start();
    let prefix = format!("({form} ");
    if let Some(rest) = trimmed.strip_prefix(&prefix) {
        let name: String =
            rest.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
        return name == symbol;
    }
    false
}

/// Recompute the applicable `AGENTS.md` instruction chain for the mapped
/// paths: every `AGENTS.md` on a root-to-file directory chain is mandatory
/// and digest-bound (L19). The repository-level file is always required.
fn instruction_chain(root: &Path, mapped_paths: &[String]) -> Result<Vec<PacketInstruction>> {
    let mut required_dirs: BTreeMap<String, ()> = BTreeMap::new();
    for path in mapped_paths {
        let mut current = Path::new(path);
        while let Some(parent) = current.parent() {
            required_dirs.insert(parent.to_string_lossy().replace('\\', "/"), ());
            current = parent;
        }
    }
    let mut chain: BTreeMap<String, PacketInstruction> = BTreeMap::new();
    for dir in required_dirs.keys() {
        let candidate = format!("{dir}/AGENTS.md");
        let absolute = root.join(&candidate);
        if absolute.is_file() {
            let sha256 = sha256_file(&absolute)?;
            let scope =
                if dir.is_empty() { "repository".to_owned() } else { "package-local".to_owned() };
            chain.insert(candidate.clone(), PacketInstruction { path: candidate, sha256, scope });
        }
    }
    let root_agents = root.join("AGENTS.md");
    if !root_agents.is_file() {
        bail!(
            "L19 missing instruction: the repository-level AGENTS.md does not exist; applicable \
             instructions are mandatory and fail closed"
        );
    }
    let entry = chain.entry("AGENTS.md".to_owned()).or_insert(PacketInstruction {
        path: "AGENTS.md".to_owned(),
        sha256: String::new(),
        scope: "repository".to_owned(),
    });
    entry.sha256 = sha256_file(&root_agents)?;
    Ok(chain.into_values().collect())
}

fn build_binding(inputs: &EngineInputs, components: &[PacketComponent]) -> ContextBinding {
    let mut binding_inputs: Vec<(String, String)> = vec![
        ("manifest".to_owned(), inputs.manifest_digest.clone()),
        ("ledger".to_owned(), inputs.ledger_digest.clone()),
        ("mapping".to_owned(), inputs.mapping_digest.clone()),
    ];
    for digest in &inputs.architecture_digests {
        binding_inputs.push((format!("architecture:{}", digest.path), digest.sha256.clone()));
    }
    for component in components {
        binding_inputs.push((format!("component:{}", component.path), component.sha256.clone()));
    }
    ContextBinding {
        git_commit: inputs.git_commit.clone(),
        git_tree: inputs.git_tree.clone(),
        manifest_digest: inputs.manifest_digest.clone(),
        ledger_digest: inputs.ledger_digest.clone(),
        architecture_digests: inputs.architecture_digests.clone(),
        mapping_digest: inputs.mapping_digest.clone(),
        input_digest: composite_digest(&binding_inputs),
    }
}

fn packet_node(node: &TrainNode) -> PacketNode {
    PacketNode {
        node_id: node.node_id.clone(),
        issue: node.issue,
        title: node.title.clone(),
        title_fingerprint: node.title_fingerprint.clone(),
        aliases: node.aliases.clone(),
        train_role: node.train_role.clone(),
        lane: node.lane.clone(),
        chain_home: node.chain.home.clone(),
        chain_controller: node.chain.controller.clone(),
        buildable: node.buildable,
        conflict_key: node.writer.conflict_key.clone(),
        parallel_group: node.writer.parallel_group.clone(),
        stack_relation: node.writer.stack_relation.clone(),
    }
}

fn packet_proposition(node: &TrainNode) -> PacketProposition {
    PacketProposition {
        one_pr_outcome: node.one_pr_outcome.clone(),
        claim_ceiling: node.claim_ceiling.clone(),
        authority_before: node.authority_before.clone(),
        authority_after: node.authority_after.clone(),
    }
}

fn packet_spec(node: &TrainNode) -> PacketSpec {
    PacketSpec {
        disposition: node.spec.disposition.clone(),
        owner: node.spec.owner.clone(),
        stale_policy: node.spec.stale_policy.clone(),
        spec_authority: node.spec.spec_authority.clone(),
        first_falsifier: node.first_falsifier.clone(),
        control_opposite: node.controls.opposite.clone(),
        control_stale: node.controls.stale.clone(),
        proof_focused: node.proof.focused.clone(),
        proof_routed: node.proof.routed.clone(),
    }
}

fn packet_graph(inputs: &EngineInputs, node: &TrainNode) -> PacketGraph {
    PacketGraph {
        dependencies: node
            .dependencies
            .iter()
            .map(|dependency| PacketDependency {
                target: dependency.target.clone(),
                class: dependency.class.clone(),
                provenance: dependency.provenance.clone(),
                resolved_issue: if dependency.target.starts_with('#') {
                    dependency.target.trim_start_matches('#').parse::<u64>().ok()
                } else {
                    inputs.manifest.node(&dependency.target).map(|target| target.issue)
                },
            })
            .collect(),
        successors: node.successors.clone(),
        consumed_authorities: node.consumed_authorities.clone(),
        allowed_components: node.allowed_components.clone(),
        forbidden_adjacent_owners: node.forbidden_adjacent_owners.clone(),
    }
}

/// Revision currency from the E01R ledger, consumed as data: the latest
/// revision entry that structurally references this node, with the ledger
/// digest binding the answer to exact bytes.
fn revision_currency(inputs: &EngineInputs, node: &TrainNode) -> PacketRevisionCurrency {
    let mut latest: Option<(u64, &str, &str, &str)> = None;
    if let Some(revisions) = inputs.ledger.get("revisions").and_then(Value::as_array) {
        for revision in revisions {
            // Entries missing identity fields are skipped rather than guessed:
            // a ledger defect is surfaced by the ledger's own checker.
            let Some(sequence) = revision.get("sequence").and_then(Value::as_u64) else {
                continue;
            };
            if revision_references_node(revision, &node.node_id) {
                let better =
                    latest.as_ref().map(|(current, _, _, _)| sequence > *current).unwrap_or(true);
                if better {
                    let entry_id = revision
                        .get("entry_id")
                        .and_then(Value::as_str)
                        .unwrap_or("<missing entry_id>");
                    let kind = revision
                        .get("revision_kind")
                        .and_then(Value::as_str)
                        .unwrap_or("<missing revision_kind>");
                    let class = revision
                        .get("semantic_class")
                        .and_then(Value::as_str)
                        .unwrap_or("<missing semantic_class>");
                    latest = Some((sequence, entry_id, kind, class));
                }
            }
        }
    }
    PacketRevisionCurrency {
        ledger_schema: LEDGER_SCHEMA_NAME.to_owned(),
        ledger_digest: inputs.ledger_digest.clone(),
        latest_entry_id: latest.map(|(_, id, _, _)| id.to_owned()),
        latest_sequence: latest.map(|(sequence, _, _, _)| sequence),
        latest_revision_kind: latest.map(|(_, _, kind, _)| kind.to_owned()),
        latest_semantic_class: latest.map(|(_, _, _, class)| class.to_owned()),
        note: "consumed as data from the E01R ledger; semantic movement is classified there and \
               invalidates this packet through the ledger digest"
            .to_owned(),
    }
}

fn revision_references_node(revision: &Value, node_id: &str) -> bool {
    fn collect_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
        match value {
            Value::String(text) => out.push(text.as_str()),
            Value::Array(items) => {
                for item in items {
                    collect_strings(item, out);
                }
            }
            Value::Object(map) => {
                for (key, item) in map {
                    // Identity-preservation prose describes movement but is
                    // not a structured reference; every other string field is
                    // scanned so wiring, invalidations and successors count.
                    if key == "reason"
                        || key == "basis"
                        || key == "detail"
                        || key == "proposition_before"
                        || key == "work"
                        || key == "cell"
                        || key == "ruling_evidence"
                    {
                        continue;
                    }
                    collect_strings(item, out);
                }
            }
            _ => {}
        }
    }
    let mut strings = Vec::new();
    collect_strings(revision, &mut strings);
    strings.contains(&node_id)
}

fn checked_specs(
    root: &Path,
    node: &TrainNode,
    mapping_entry: Option<&NodeMapping>,
) -> Result<Vec<PacketCheckedSpec>> {
    let mut bundles: BTreeMap<String, Vec<PacketDigest>> = BTreeMap::new();
    // The node's own landed bundle, discovered by issue prefix.
    let spec_root = root.join(".spec");
    if spec_root.is_dir() {
        let prefix = format!("{}-", node.issue);
        let mut dir_names: Vec<String> = std::fs::read_dir(&spec_root)
            .with_context(|| "listing .spec for checked bundle discovery")?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && entry.path().is_dir() { Some(name) } else { None }
            })
            .collect();
        dir_names.sort();
        for name in &dir_names {
            let files = digest_bundle_files(root, &format!(".spec/{name}"))?;
            bundles.insert(name.clone(), files);
        }
    }
    // Explicit per-node spec references from the mapping document.
    if let Some(entry) = mapping_entry {
        for spec in &entry.specs {
            if bundles.contains_key(spec) {
                continue;
            }
            let files = digest_bundle_files(root, &format!(".spec/{spec}"))?;
            bundles.insert(spec.clone(), files);
        }
    }
    Ok(bundles.into_iter().map(|(bundle, files)| PacketCheckedSpec { bundle, files }).collect())
}

fn digest_bundle_files(root: &Path, bundle_relative: &str) -> Result<Vec<PacketDigest>> {
    let bundle_root = root.join(bundle_relative);
    let mut names: Vec<String> = std::fs::read_dir(&bundle_root)
        .with_context(|| format!("listing checked bundle {}", bundle_root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.path().is_file() { Some(name) } else { None }
        })
        .collect();
    names.sort();
    let mut files = Vec::with_capacity(names.len());
    for name in names {
        let path = format!("{bundle_relative}/{name}");
        files.push(PacketDigest { sha256: sha256_file(&root.join(&path))?, path });
    }
    Ok(files)
}

fn packet_bounds(inputs: &EngineInputs, omissions: Vec<PacketOmission>) -> PacketBounds {
    let bounds = &inputs.mapping.bounds;
    PacketBounds {
        max_components_per_node: bounds.max_components_per_node,
        max_tests_per_node: bounds.max_tests_per_node,
        max_read_set: bounds.max_read_set,
        max_write_set: bounds.max_write_set,
        max_not_authority: bounds.max_not_authority,
        omitted: omissions,
    }
}

fn packet_privacy() -> PacketPrivacy {
    PacketPrivacy {
        repository_relative_paths_only: true,
        no_source_text_embedded: true,
        no_absolute_paths: true,
        no_logs_or_credentials: true,
        note: "packets carry normalized repository-relative paths, digests and bounded symbol \
               anchors only; no source text, raw logs, prompts, credentials or absolute local \
               paths are ever emitted"
            .to_owned(),
    }
}
