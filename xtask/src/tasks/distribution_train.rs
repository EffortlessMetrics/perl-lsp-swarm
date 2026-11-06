//! `cargo xtask distribution-train check|graph|explain|next` — the checked
//! distribution install-train dependency graph (#10712).
//!
//! This module consumes `.spec/10712-distribution-train-graph/distribution_train.v1.json`
//! strictly as checked DATA and provides, offline:
//!
//! * a fail-closed loader: strict schema (`deny_unknown_fields`), closed
//!   vocabularies, structural identity/dependency laws, and a pinned canonical
//!   digest, so mutable work-state facts (PR numbers, head SHAs, review or CI
//!   state) cannot enter the stable graph silently;
//! * deterministic `check`, `graph`, `explain`, and current-tree-only `next`
//!   projections over that graph.
//!
//! Law boundaries (from #10712): the manifest is architecture, not live
//! work-state telemetry. Controllers and release handoffs are never PR units;
//! satellites are references, never copied internal nodes; `current_tree_status`
//! is a reviewed repository-side snapshot, never GitHub issue state — live
//! candidate/writer observation belongs to #10726. No command here reads
//! GitHub, mutates anything, or evaluates installed/public evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail, eyre};
use serde::Deserialize;
use serde_json::Value;

use crate::utils::project_root;
use xtask::import_cleanup_train_manifest::canonical_digest;

/// Repository-relative location of the stable distribution train manifest (#10712).
pub const MANIFEST_RELATIVE_PATH: &str =
    ".spec/10712-distribution-train-graph/distribution_train.v1.json";

/// Schema identity consumed by this model.
pub const SCHEMA_NAME: &str = "distribution_train.v1";
pub const SCHEMA_VERSION: u64 = 1;

/// Pinned canonical digest of the current `distribution_train.v1` revision.
///
/// Canonicalization: recursive content walk with byte-ordinal ordering (the
/// shared `canonical_digest` helper; array order is normalized). Any manifest
/// revision — including a `current_tree_status` refresh — must move this pin
/// deliberately together with the manifest bytes; patching around it silently
/// is exactly what the pin exists to prevent.
pub const PINNED_CANONICAL_DIGEST: &str =
    "115910D28DB422CB7CF8FD52BAA1F1D99AC2DE7778FF8B51177834BA74DA79E9";

/// Node kinds that group, gate, or hand off. They are never PR units: they
/// carry `do_not_build` rules, take no builder proof commands, and are never
/// valid hard dependencies of buildable nodes.
const GOVERNANCE_KINDS: [&str; 2] = ["controller", "release_handoff"];

/// Node kinds that name a genuine one-PR buildable proposition.
const BUILDABLE_KINDS: [&str; 4] = ["contract", "implementation", "adapter", "proof"];

/// Statuses of nodes that are out of the running frontier.
const SETTLED_STATUSES: [&str; 4] =
    ["landed_current_tree", "superseded", "transferred", "deferred"];

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    schema: String,
    schema_version: u64,
    planning_basis: String,
    status_authority: String,
    programme: Programme,
    vocabulary: Vocabulary,
    non_builder_issues: Vec<u64>,
    parallel_groups: Vec<ParallelGroup>,
    nodes: Vec<Node>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Programme {
    parent_programme_issue: u64,
    train_graph_issue: u64,
    spec_controller_issue: u64,
    public_route_controller_issue: u64,
    release_integration_issue: u64,
    method_authority: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Vocabulary {
    node_kinds: Vec<String>,
    lanes: Vec<String>,
    dependency_classes: Vec<String>,
    spec_dispositions: Vec<String>,
    current_tree_statuses: Vec<String>,
    pub(crate) product_units: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ParallelGroup {
    id: String,
    join_after: String,
    members: Vec<String>,
    rule: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Node {
    pub(crate) issue_number: u64,
    pub(crate) node_id: String,
    pub(crate) title: String,
    pub(crate) lane: String,
    pub(crate) kind: String,
    pub(crate) one_pr_outcome: String,
    #[serde(default)]
    pub(crate) hard_dependencies: Vec<String>,
    #[serde(default)]
    pub(crate) evidence_dependencies: Vec<String>,
    #[serde(default)]
    pub(crate) optional_dependencies: Vec<String>,
    #[serde(default)]
    pub(crate) subject_authorities: Vec<String>,
    #[serde(default)]
    pub(crate) product_units: Vec<String>,
    #[serde(default)]
    pub(crate) consumes_contracts: Vec<String>,
    #[serde(default)]
    pub(crate) produces_contracts: Vec<String>,
    #[serde(default)]
    pub(crate) conflict_key: Option<String>,
    #[serde(default)]
    pub(crate) parallel_group: Option<String>,
    pub(crate) spec_disposition: String,
    pub(crate) spec_ref: String,
    #[serde(default)]
    pub(crate) falsifier_refs: Vec<String>,
    #[serde(default)]
    pub(crate) proof_commands: Vec<String>,
    #[serde(default)]
    pub(crate) proof_owner: Option<String>,
    pub(crate) current_tree_status: String,
    #[serde(default)]
    pub(crate) supersedes: Vec<String>,
    #[serde(default)]
    pub(crate) superseded_by: Vec<String>,
    #[serde(default)]
    pub(crate) legacy_paths_retired: Vec<String>,
    pub(crate) release_critical: bool,
    #[serde(default)]
    pub(crate) promotion_trigger: Option<String>,
    #[serde(default)]
    pub(crate) limitations: Vec<String>,
    #[serde(default)]
    pub(crate) do_not_build: Vec<String>,
}

impl Node {
    fn is_governance(&self) -> bool {
        GOVERNANCE_KINDS.contains(&self.kind.as_str())
    }

    fn is_buildable(&self) -> bool {
        BUILDABLE_KINDS.contains(&self.kind.as_str())
    }

    fn is_settled(&self) -> bool {
        SETTLED_STATUSES.contains(&self.current_tree_status.as_str())
    }

    /// True when the node can start now: all hard dependencies are
    /// `landed_current_tree` in the checked snapshot.
    fn hard_ready(&self, landed: &BTreeSet<String>) -> bool {
        self.hard_dependencies.iter().all(|dep| landed.contains(dep))
    }
}

/// Parsed, digest-pinned manifest plus the derived lookup indexes.
#[derive(Debug)]
pub struct TrainGraph {
    manifest: Manifest,
    by_id: BTreeMap<String, usize>,
    landed: BTreeSet<String>,
}

/// One structural violation, prefixed by the law that caught it.
fn law(id: &str, node: &str, detail: &str) -> String {
    format!("law {id} [{node}]: {detail}")
}

impl TrainGraph {
    /// Parse and fully validate manifest bytes, including the canonical
    /// digest pin.
    pub fn parse(bytes: &str) -> Result<Self> {
        let value: Value =
            serde_json::from_str(bytes).wrap_err(format!("{SCHEMA_NAME}: manifest is not JSON"))?;
        let digest = canonical_digest(&value)?;
        if digest != PINNED_CANONICAL_DIGEST {
            bail!(
                "{SCHEMA_NAME}: canonical digest {digest} does not match the pinned \
                 revision {PINNED_CANONICAL_DIGEST}; move the pin deliberately together \
                 with the manifest bytes"
            );
        }
        let manifest: Manifest = serde_json::from_value(value)
            .map_err(|error| eyre!("{SCHEMA_NAME}: manifest rejected: {error}"))?;
        let graph = Self::from_manifest(manifest)?;
        let violations = graph.laws();
        if !violations.is_empty() {
            bail!(
                "{SCHEMA_NAME}: {} structural violation(s):\n  - {}",
                violations.len(),
                violations.join("\n  - ")
            );
        }
        Ok(graph)
    }

    /// Load and validate the committed manifest from the repository root.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(MANIFEST_RELATIVE_PATH);
        let bytes = std::fs::read_to_string(&path)
            .wrap_err(format!("reading train manifest {}", path.display()))?;
        Self::parse(&bytes)
    }

    pub(crate) fn from_manifest(manifest: Manifest) -> Result<Self> {
        let mut by_id = BTreeMap::new();
        for (index, node) in manifest.nodes.iter().enumerate() {
            if by_id.insert(node.node_id.clone(), index).is_some() {
                bail!("{SCHEMA_NAME}: duplicate node_id {}", node.node_id);
            }
        }
        let landed = manifest
            .nodes
            .iter()
            .filter(|node| node.current_tree_status == "landed_current_tree")
            .map(|node| node.node_id.clone())
            .collect();
        Ok(Self { manifest, by_id, landed })
    }

    pub(crate) fn node(&self, node_id: &str) -> Option<&Node> {
        self.by_id.get(node_id).map(|&index| &self.manifest.nodes[index])
    }

    /// Resolve `10333` or `#10333` to a node.
    pub fn resolve(&self, subject: &str) -> Result<&Node> {
        let id = subject.trim_start_matches('#');
        self.node(id).ok_or_else(|| {
            eyre!(
                "no train node {subject}; run `cargo xtask distribution-train graph` for the roster"
            )
        })
    }

    pub(crate) fn nodes(&self) -> &[Node] {
        &self.manifest.nodes
    }

    fn node_kind(&self, node_id: &str) -> Option<&str> {
        self.node(node_id).map(|node| node.kind.as_str())
    }

    /// All structural laws over the parsed manifest. Pure: no filesystem access.
    pub(crate) fn laws(&self) -> Vec<String> {
        let mut violations = Vec::new();
        let m = &self.manifest;

        // L01 schema identity.
        if m.schema != SCHEMA_NAME || m.schema_version != SCHEMA_VERSION {
            violations.push(law(
                "L01",
                "manifest",
                &format!(
                    "expected {SCHEMA_NAME} v{SCHEMA_VERSION}, found {} v{}",
                    m.schema, m.schema_version
                ),
            ));
        }

        // L00 the reviewed snapshot and its status authority are recorded
        // contract text, never empty.
        if m.planning_basis.trim().is_empty() {
            violations.push(law("L00", "manifest", "planning_basis is empty"));
        }
        if m.status_authority.trim().is_empty() {
            violations.push(law("L00", "manifest", "status_authority is empty"));
        }
        if m.vocabulary.dependency_classes != ["hard", "evidence", "optional"] {
            violations.push(law(
                "L02",
                "manifest",
                "dependency_classes are contract and order-significant: expected [hard, evidence, optional]",
            ));
        }

        // L06 reject-listed issues are never implementation nodes
        // (#10712 falsifier 1).
        for issue in &m.non_builder_issues {
            let id = issue.to_string();
            match self.node(&id) {
                Some(node) if node.is_governance() && node.spec_disposition == "NO_SPEC_DELTA" => {}
                Some(node) => violations.push(law(
                    "L06",
                    &id,
                    &format!(
                        "non-builder issue encoded as kind '{}' with disposition '{}'",
                        node.kind, node.spec_disposition
                    ),
                )),
                None => {
                    violations.push(law("L06", &id, "non-builder issue is missing from the graph"))
                }
            }
        }

        // L17 the programme identity issues exist as encoded nodes.
        for (role, issue) in [
            ("parent_programme_issue", m.programme.parent_programme_issue),
            ("train_graph_issue", m.programme.train_graph_issue),
            ("spec_controller_issue", m.programme.spec_controller_issue),
            ("public_route_controller_issue", m.programme.public_route_controller_issue),
            ("release_integration_issue", m.programme.release_integration_issue),
        ] {
            if self.node(&issue.to_string()).is_none() {
                violations.push(law(
                    "L17",
                    "manifest",
                    &format!("programme {role} #{issue} is not an encoded node"),
                ));
            }
        }
        if m.programme.method_authority.trim().is_empty() {
            violations.push(law("L17", "manifest", "method_authority is empty"));
        }

        // L02 closed vocabularies.
        for node in &m.nodes {
            if !m.vocabulary.node_kinds.contains(&node.kind) {
                violations.push(law(
                    "L02",
                    &node.node_id,
                    &format!("unknown kind '{}'", node.kind),
                ));
            }
            if !m.vocabulary.lanes.contains(&node.lane) {
                violations.push(law(
                    "L02",
                    &node.node_id,
                    &format!("unknown lane '{}'", node.lane),
                ));
            }
            if !m.vocabulary.spec_dispositions.contains(&node.spec_disposition) {
                violations.push(law(
                    "L02",
                    &node.node_id,
                    &format!("unknown spec disposition '{}'", node.spec_disposition),
                ));
            }
            if !m.vocabulary.current_tree_statuses.contains(&node.current_tree_status) {
                violations.push(law(
                    "L02",
                    &node.node_id,
                    &format!("unknown current-tree status '{}'", node.current_tree_status),
                ));
            }
            for unit in &node.product_units {
                if !m.vocabulary.product_units.contains(unit) {
                    violations.push(law(
                        "L02",
                        &node.node_id,
                        &format!("unknown product unit '{unit}'"),
                    ));
                }
            }
        }

        for node in &m.nodes {
            // L03 node identity is its issue number.
            if node.node_id != node.issue_number.to_string() {
                violations.push(law("L03", &node.node_id, "node_id must equal the issue number"));
            }

            // L04 dependency classes are disjoint and reference existing nodes;
            // no self-dependency.
            let mut seen = BTreeSet::new();
            for (class, deps) in [
                ("hard", &node.hard_dependencies),
                ("evidence", &node.evidence_dependencies),
                ("optional", &node.optional_dependencies),
            ] {
                for dep in deps {
                    if dep == &node.node_id {
                        violations.push(law("L04", &node.node_id, "self-dependency"));
                    }
                    if !seen.insert(dep.as_str()) {
                        violations.push(law(
                            "L04",
                            &node.node_id,
                            &format!("dependency {dep} appears in more than one class"),
                        ));
                    }
                    if self.node(dep).is_none() {
                        violations.push(law(
                            "L04",
                            &node.node_id,
                            &format!("{class} dependency {dep} is not a manifest node"),
                        ));
                    }
                }
            }

            // L05 handled after the loop (needs the whole edge set).

            // L06/L07 governance nodes are never PR units.
            if node.is_governance() {
                if node.do_not_build.is_empty() {
                    violations.push(law(
                        "L07",
                        &node.node_id,
                        "controller/release-handoff node carries no do_not_build rule",
                    ));
                }
                if !node.proof_commands.is_empty() {
                    violations.push(law(
                        "L07",
                        &node.node_id,
                        "controller/release-handoff node carries builder proof commands",
                    ));
                }
            }
            if node.spec_disposition == "NO_SPEC_DELTA" && node.is_buildable() {
                violations.push(law(
                    "L07",
                    &node.node_id,
                    "buildable node cannot carry NO_SPEC_DELTA",
                ));
            }
            if node.spec_disposition != "NO_SPEC_DELTA" && node.is_governance() {
                violations.push(law(
                    "L07",
                    &node.node_id,
                    "controller/release-handoff node must carry NO_SPEC_DELTA",
                ));
            }

            // L08 hard dependencies of buildable nodes are buildable kinds.
            if node.is_buildable() {
                for dep in &node.hard_dependencies {
                    match self.node_kind(dep) {
                        Some(kind) if BUILDABLE_KINDS.contains(&kind) => {}
                        Some(kind) => violations.push(law(
                            "L08",
                            &node.node_id,
                            &format!(
                                "hard dependency {dep} is kind '{kind}', which is never a PR unit"
                            ),
                        )),
                        None => {}
                    }
                }
            }

            // L09 buildable nodes own their proof; contracts own their output.
            if node.is_buildable() && node.proof_owner.is_none() {
                violations.push(law("L09", &node.node_id, "buildable node has no proof owner"));
            }
            if node.kind == "contract" && node.produces_contracts.is_empty() {
                violations.push(law("L09", &node.node_id, "contract node produces no contract"));
            }

            // L10 spec_ref shape per disposition.
            match node.spec_disposition.as_str() {
                "SPEC_COMPILED" if !node.spec_ref.starts_with(".spec/") => {
                    violations.push(law(
                        "L10",
                        &node.node_id,
                        "SPEC_COMPILED spec_ref must name a .spec/ packet or bundle",
                    ));
                }
                "NO_SPEC_DELTA" | "ISSUE_PLAN_SUFFICIENT"
                    if node.spec_ref != format!("#{}", node.issue_number) =>
                {
                    // Exactly the owning issue: a `#`-prefixed reference to a
                    // different (or malformed) issue is not a node link.
                    violations.push(law(
                        "L10",
                        &node.node_id,
                        &format!(
                            "{} spec_ref must reference the owning issue #{}, not {}",
                            node.spec_disposition, node.issue_number, node.spec_ref
                        ),
                    ));
                }
                "EXISTING_CONTRACT_SUFFICIENT"
                    if !node.spec_ref.starts_with('#')
                        && !node.spec_ref.starts_with(".spec/")
                        && !node.spec_ref.starts_with("schemas/")
                        && !node.spec_ref.starts_with("policy/")
                        && !node.spec_ref.starts_with("xtask/")
                        && !node.spec_ref.starts_with("scripts/") =>
                {
                    violations.push(law(
                            "L10",
                            &node.node_id,
                            "EXISTING_CONTRACT_SUFFICIENT spec_ref must name an issue or a repository contract path",
                        ));
                }
                "RETURN_TO_ISSUE" => {}
                _ => {}
            }

            // L11 readiness is consistent with checked hard-dep landing.
            if node.current_tree_status == "ready" && !node.hard_ready(&self.landed) {
                violations.push(law(
                    "L11",
                    &node.node_id,
                    "status ready but not every hard dependency is landed_current_tree",
                ));
            }

            // L12 deferred nodes are never hard predecessors; npm stays deferred.
            for dep in &node.hard_dependencies {
                if let Some(target) = self.node(dep)
                    && target.current_tree_status == "deferred"
                {
                    violations.push(law(
                            "L12",
                            &node.node_id,
                            &format!("hard dependency {dep} is deferred; deferred channels never gate required work"),
                        ));
                }
            }
            if node.issue_number == 8301
                && (node.current_tree_status != "deferred" || node.release_critical)
            {
                violations.push(law(
                    "L12",
                    &node.node_id,
                    "npm (#8301) must stay deferred and never release-critical",
                ));
            }

            // L13 satellites are references, never copied internal nodes.
            if node.lane == "satellite" && !node.is_governance() {
                violations.push(law(
                    "L13",
                    &node.node_id,
                    "satellite lane is reserved for controller/handoff references",
                ));
            }

            // L15 supersession symmetry.
            for superseded in &node.supersedes {
                match self.node(superseded) {
                    Some(target) => {
                        if !target.superseded_by.contains(&node.node_id) {
                            violations.push(law(
                                "L15",
                                &node.node_id,
                                &format!("supersedes {superseded} but {superseded} does not record the supersession"),
                            ));
                        }
                    }
                    None => violations.push(law(
                        "L15",
                        &node.node_id,
                        &format!("supersedes unknown node {superseded}"),
                    )),
                }
            }
            for predecessor in &node.superseded_by {
                match self.node(predecessor) {
                    Some(target) => {
                        if !target.supersedes.contains(&node.node_id) {
                            violations.push(law(
                                "L15",
                                &node.node_id,
                                &format!("superseded_by {predecessor} but {predecessor} does not claim the supersession"),
                            ));
                        }
                    }
                    None => violations.push(law(
                        "L15",
                        &node.node_id,
                        &format!("superseded_by unknown node {predecessor}"),
                    )),
                }
            }

            // L16 a retired legacy path must name its promotion trigger.
            if !node.legacy_paths_retired.is_empty() && node.promotion_trigger.is_none() {
                violations.push(law(
                    "L16",
                    &node.node_id,
                    "legacy_paths_retired without a promotion_trigger",
                ));
            }
        }

        // L13b: no node may hard-depend on a satellite.
        for node in &m.nodes {
            for dep in &node.hard_dependencies {
                if let Some(target) = self.node(dep)
                    && target.lane == "satellite"
                {
                    violations.push(law(
                        "L13",
                        &node.node_id,
                        &format!("hard dependency {dep} targets a satellite reference"),
                    ));
                }
            }
        }

        // L14 parallel groups agree with node membership.
        let mut grouped: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for node in &m.nodes {
            if let Some(group) = &node.parallel_group {
                grouped.entry(group.as_str()).or_default().push(&node.node_id);
            }
        }
        for group in &m.parallel_groups {
            if self.node(&group.join_after).is_none() {
                violations.push(law(
                    "L14",
                    &group.id,
                    &format!("join_after {} is not a manifest node", group.join_after),
                ));
            }
            if group.rule.trim().is_empty() {
                violations.push(law("L14", &group.id, "parallel group carries no rule"));
            }
            let mut declared: Vec<&str> = group.members.iter().map(String::as_str).collect();
            let mut carried: Vec<&str> =
                grouped.get(group.id.as_str()).cloned().unwrap_or_default();
            declared.sort();
            carried.sort();
            if declared != carried {
                violations.push(law(
                    "L14",
                    &group.id,
                    &format!(
                        "declared members {declared:?} != nodes carrying the group {carried:?}"
                    ),
                ));
            }
        }
        for group in grouped.keys() {
            if !m.parallel_groups.iter().any(|g| &g.id == group) {
                violations.push(law(
                    "L14",
                    group,
                    "node carries a parallel_group that is not declared",
                ));
            }
        }

        // L05 acyclic hard-dependency graph (iterative depth-first).
        let mut state: BTreeMap<&str, u8> = BTreeMap::new();
        for node in &m.nodes {
            if let Some(cycle) = Self::find_cycle(&node.node_id, self, &mut state, &mut Vec::new())
            {
                violations.push(law("L05", "manifest", &format!("hard-dependency cycle {cycle}")));
                break;
            }
        }

        violations
    }

    fn find_cycle<'a>(
        node_id: &'a str,
        graph: &'a TrainGraph,
        state: &mut BTreeMap<&'a str, u8>,
        stack: &mut Vec<String>,
    ) -> Option<String> {
        match state.get(node_id) {
            Some(1) => {
                let start = stack.iter().position(|id| id == node_id).unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                cycle.push(node_id.to_string());
                return Some(cycle.join(" -> "));
            }
            Some(_) => return None,
            None => {}
        }
        let node = graph.node(node_id)?;
        state.insert(node_id, 1);
        stack.push(node_id.to_string());
        for dep in &node.hard_dependencies {
            if let Some(cycle) = Self::find_cycle(dep, graph, state, stack) {
                return Some(cycle);
            }
        }
        stack.pop();
        state.insert(node_id, 2);
        None
    }

    /// Filesystem laws that need the repository root: SPEC_COMPILED and
    /// EXISTING_CONTRACT_SUFFICIENT spec_refs must resolve in this tree.
    fn disk_laws(&self, root: &Path) -> Vec<String> {
        let mut violations = Vec::new();
        for node in &self.manifest.nodes {
            let resolves = match node.spec_disposition.as_str() {
                "SPEC_COMPILED" | "EXISTING_CONTRACT_SUFFICIENT" => {
                    let raw = node.spec_ref.split('#').next().unwrap_or(&node.spec_ref);
                    root.join(raw).exists()
                }
                _ => true,
            };
            if !resolves {
                violations.push(law(
                    "L10",
                    &node.node_id,
                    &format!("spec_ref {} does not resolve in this tree", node.spec_ref),
                ));
            }
        }
        violations
    }

    /// The checked current-tree frontier: buildable nodes not yet settled
    /// whose hard dependencies are all `landed_current_tree`, grouped by lane.
    pub fn next_frontier(&self) -> BTreeMap<&str, Vec<&Node>> {
        let mut frontier: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
        for node in &self.manifest.nodes {
            // The frontier names implementable work: governance nodes and
            // non-buildable kinds (claims, evidence joins) never enter it.
            if node.is_settled() || node.is_governance() || !node.is_buildable() {
                continue;
            }
            if node.hard_ready(&self.landed) {
                frontier.entry(node.lane.as_str()).or_default().push(node);
            }
        }
        for lane in frontier.values_mut() {
            lane.sort_by_key(|node| node.issue_number);
        }
        frontier
    }
}

/// `cargo xtask distribution-train <command>`.
#[derive(Subcommand, Debug)]
pub enum TrainCommand {
    /// Validate the checked graph against its fail-closed laws and print a
    /// deterministic summary.
    Check {
        /// Print a machine-readable JSON summary.
        #[arg(long)]
        json: bool,
    },
    /// Print the deterministic builder-oriented graph view.
    Graph,
    /// Explain one node (node id or `#issue`) for a coding agent.
    Explain {
        /// Node id or issue reference, e.g. `10333` or `#10333`.
        subject: String,
    },
    /// Print the current-tree hard-dependency-ready frontier (checked
    /// statuses only; live candidate/writer state belongs to #10726).
    Next,
}

/// Run one `distribution-train` command.
pub fn run(command: TrainCommand) -> Result<()> {
    let root = project_root()?;
    let graph = TrainGraph::load(&root)?;
    match command {
        TrainCommand::Check { json } => run_check(&graph, &root, json),
        TrainCommand::Graph => {
            println!("{}", render_graph(&graph));
            Ok(())
        }
        TrainCommand::Explain { subject } => {
            let node = graph.resolve(&subject)?;
            println!("{}", render_explain(node));
            Ok(())
        }
        TrainCommand::Next => {
            println!("{}", render_next(&graph));
            Ok(())
        }
    }
}

fn run_check(graph: &TrainGraph, root: &Path, json: bool) -> Result<()> {
    let mut violations = graph.laws();
    violations.extend(graph.disk_laws(root));
    if !violations.is_empty() {
        bail!(
            "distribution-train check failed with {} violation(s):\n  - {}",
            violations.len(),
            violations.join("\n  - ")
        );
    }
    let nodes = graph.nodes();
    let summary = format!(
        "distribution_train.v1 check: OK nodes={} governance={} buildable={} landed={} digest={}",
        nodes.len(),
        nodes.iter().filter(|n| n.is_governance()).count(),
        nodes.iter().filter(|n| n.is_buildable()).count(),
        nodes.iter().filter(|n| n.current_tree_status == "landed_current_tree").count(),
        &PINNED_CANONICAL_DIGEST[..12],
    );
    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": SCHEMA_NAME,
                "result": "ok",
                "nodes": nodes.len(),
                "pinned_digest": PINNED_CANONICAL_DIGEST,
            })
        );
    } else {
        println!("{summary}");
    }
    Ok(())
}

fn render_graph(graph: &TrainGraph) -> String {
    let mut lanes: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for node in graph.nodes() {
        lanes.entry(node.lane.as_str()).or_default().push(node);
    }
    for lane_nodes in lanes.values_mut() {
        lane_nodes.sort_by_key(|node| node.issue_number);
    }
    let mut out = String::from(
        "distribution_train.v1 builder graph (stable topology; live state is #10726's):\n",
    );
    for (lane, lane_nodes) in lanes {
        out.push_str(&format!("\n## {lane}\n"));
        for node in lane_nodes {
            out.push_str(&format!(
                "- {} [{}] {} status={} spec={} critical={}\n",
                node.node_id,
                node.kind,
                node.title,
                node.current_tree_status,
                node.spec_disposition,
                node.release_critical,
            ));
            if !node.hard_dependencies.is_empty() {
                out.push_str(&format!("  hard: {}\n", node.hard_dependencies.join(", ")));
            }
            if !node.evidence_dependencies.is_empty() {
                out.push_str(&format!("  evidence: {}\n", node.evidence_dependencies.join(", ")));
            }
            if !node.optional_dependencies.is_empty() {
                out.push_str(&format!("  optional: {}\n", node.optional_dependencies.join(", ")));
            }
        }
    }
    out
}

fn render_explain(node: &Node) -> String {
    let mut out = String::new();
    out.push_str(&format!("# train node {} — {}\n", node.node_id, node.title));
    out.push_str(&format!(
        "issue: #{}  lane: {}  kind: {}  release_critical: {}\n",
        node.issue_number, node.lane, node.kind, node.release_critical
    ));
    out.push_str(&format!("one-PR proposition: {}\n", node.one_pr_outcome));
    out.push_str(&format!(
        "current-tree status (checked snapshot, not GitHub): {}\n",
        node.current_tree_status
    ));
    out.push_str(&format!("spec disposition: {} -> {}\n", node.spec_disposition, node.spec_ref));
    if let Some(key) = &node.conflict_key {
        out.push_str(&format!("conflict key: {key}\n"));
    }
    if let Some(group) = &node.parallel_group {
        out.push_str(&format!("parallel group: {group}\n"));
    }
    if !node.subject_authorities.is_empty() {
        out.push_str(&format!("authorities: {}\n", node.subject_authorities.join(", ")));
    }
    if !node.product_units.is_empty() {
        out.push_str(&format!("product units: {}\n", node.product_units.join(", ")));
    }
    if !node.consumes_contracts.is_empty() {
        out.push_str(&format!("consumes: {}\n", node.consumes_contracts.join(", ")));
    }
    if !node.produces_contracts.is_empty() {
        out.push_str(&format!("produces: {}\n", node.produces_contracts.join(", ")));
    }
    out.push_str(&format!(
        "hard predecessors: {}\n",
        if node.hard_dependencies.is_empty() {
            "(none)".into()
        } else {
            node.hard_dependencies.join(", ")
        }
    ));
    if !node.evidence_dependencies.is_empty() {
        out.push_str(&format!(
            "evidence predecessors: {}\n",
            node.evidence_dependencies.join(", ")
        ));
    }
    if !node.optional_dependencies.is_empty() {
        out.push_str(&format!(
            "optional predecessors: {}\n",
            node.optional_dependencies.join(", ")
        ));
    }
    if !node.falsifier_refs.is_empty() {
        out.push_str(&format!("falsifiers: {}\n", node.falsifier_refs.join(", ")));
    }
    if let Some(owner) = &node.proof_owner {
        out.push_str(&format!("proof owner: {owner}\n"));
    }
    if !node.proof_commands.is_empty() {
        out.push_str(&format!("proof commands: {}\n", node.proof_commands.join(" && ")));
    }
    if !node.supersedes.is_empty() {
        out.push_str(&format!("supersedes: {}\n", node.supersedes.join(", ")));
    }
    if !node.superseded_by.is_empty() {
        out.push_str(&format!("superseded by: {}\n", node.superseded_by.join(", ")));
    }
    if !node.legacy_paths_retired.is_empty() {
        out.push_str(&format!("legacy paths retired: {}\n", node.legacy_paths_retired.join("; ")));
    }
    if let Some(trigger) = &node.promotion_trigger {
        out.push_str(&format!("promotion trigger: {trigger}\n"));
    }
    for limitation in &node.limitations {
        out.push_str(&format!("limitation: {limitation}\n"));
    }
    for rule in &node.do_not_build {
        out.push_str(&format!("do not build: {rule}\n"));
    }
    if node.spec_disposition == "SPEC_COMPILED" {
        out.push_str(&format!(
            "next preparation route: cargo xtask distribution-spec explain {}\n",
            node.node_id
        ));
    } else if node.is_buildable() {
        out.push_str(&format!(
            "next preparation route: read {} and the exact producer contracts it names\n",
            node.spec_ref
        ));
    } else {
        out.push_str("next preparation route: none; this node is never a PR unit\n");
    }
    out
}

fn render_next(graph: &TrainGraph) -> String {
    let frontier = graph.next_frontier();
    let mut out = String::from(
        "distribution_train.v1 next (checked current-tree frontier; live candidate/writer state is #10726's):\n",
    );
    if frontier.is_empty() {
        out.push_str(
            "ready (0): no buildable node has every hard dependency landed_current_tree\n",
        );
    }
    for (lane, nodes) in &frontier {
        out.push_str(&format!("\nready lane {lane} ({}):\n", nodes.len()));
        for node in nodes {
            let group =
                node.parallel_group.as_ref().map(|g| format!(" group={g}")).unwrap_or_default();
            out.push_str(&format!(
                "  - {} [{}] {}{}\n",
                node.node_id, node.kind, node.title, group
            ));
        }
    }
    let blocked = graph
        .nodes()
        .iter()
        .filter(|node| {
            !node.is_settled() && !node.is_governance() && !node.hard_ready(&graph.landed)
        })
        .count();
    out.push_str(&format!("\nblocked_hard_dependency (behind the frontier): {blocked}\n"));
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "test-only fixtures and static patterns; production paths stay panic-free"
    )]
    use super::*;

    fn load_graph() -> TrainGraph {
        let root = project_root().expect("project root");
        TrainGraph::load(&root).expect("committed manifest must load and pass all laws")
    }

    fn mutated<F: FnOnce(&mut Value)>(mutate: F) -> String {
        // Parse the committed manifest, apply the mutation, and report the
        // loader's rejection as a string so each law has a named falsifier.
        let root = project_root().expect("project root");
        let bytes = std::fs::read_to_string(root.join(MANIFEST_RELATIVE_PATH)).expect("manifest");
        let mut value: Value = serde_json::from_str(&bytes).expect("json");
        mutate(&mut value);
        match serde_json::from_value::<Manifest>(value) {
            Ok(manifest) => match TrainGraph::from_manifest(manifest) {
                Ok(graph) => {
                    let violations = graph.laws();
                    assert!(!violations.is_empty(), "mutation was not rejected by any law");
                    violations.join("\n")
                }
                Err(error) => error.to_string(),
            },
            Err(error) => error.to_string(),
        }
    }

    fn node_index<'a>(value: &'a mut Value, node_id: &str) -> &'a mut Value {
        value
            .get_mut("nodes")
            .and_then(|nodes| nodes.as_array_mut())
            .and_then(|nodes| {
                nodes
                    .iter_mut()
                    .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
            })
            .expect("node present")
    }

    #[test]
    fn committed_manifest_passes_every_law() {
        let graph = load_graph();
        assert!(graph.laws().is_empty());
        let root = project_root().expect("project root");
        assert!(graph.disk_laws(&root).is_empty());
    }

    #[test]
    fn check_is_second_run_clean() {
        let graph = load_graph();
        let first = render_graph(&graph);
        let second = render_graph(&graph);
        assert_eq!(first, second);
        let first_next = render_next(&graph);
        let second_next = render_next(&graph);
        assert_eq!(first_next, second_next);
    }

    #[test]
    fn graph_and_next_are_deterministic_under_node_reordering() {
        // Array order must not change any projection: canonicalization sorts.
        let root = project_root().expect("project root");
        let bytes = std::fs::read_to_string(root.join(MANIFEST_RELATIVE_PATH)).expect("manifest");
        let mut value: Value = serde_json::from_str(&bytes).expect("json");
        let nodes = value.get_mut("nodes").and_then(Value::as_array_mut).expect("nodes array");
        nodes.reverse();
        let manifest: Manifest =
            serde_json::from_value(value).expect("reordered manifest still parses");
        let reordered = TrainGraph::from_manifest(manifest).expect("reordered graph");
        let committed = load_graph();
        assert_eq!(render_graph(&committed), render_graph(&reordered));
        assert_eq!(render_next(&committed), render_next(&reordered));
    }

    #[test]
    fn controller_issue_cannot_become_an_implementation_node() {
        // Falsifier 1: make #5903 an implementation node.
        let report = mutated(|value| {
            let node = node_index(value, "5903");
            node["kind"] = Value::String("implementation".into());
        });
        assert!(report.contains("cannot carry NO_SPEC_DELTA"), "unexpected rejection: {report}");
    }

    #[test]
    fn cyclic_hard_dependency_is_rejected() {
        // Falsifier: a hard edge that waits on its own descendant.
        let report = mutated(|value| {
            node_index(value, "11434")["hard_dependencies"] =
                serde_json::json!(["10333", "11432", "11443"]);
        });
        assert!(report.contains("L05") && report.contains("cycle"), "unexpected: {report}");
    }

    #[test]
    fn open_pr_head_sha_field_is_rejected() {
        // Falsifier 12: inject mutable work-state into the stable manifest.
        let root = project_root().expect("project root");
        let bytes = std::fs::read_to_string(root.join(MANIFEST_RELATIVE_PATH)).expect("manifest");
        let mut value: Value = serde_json::from_str(&bytes).expect("json");
        node_index(&mut value, "10333")["candidate_head_sha"] = Value::String("abc".into());
        let error = serde_json::from_value::<Manifest>(value).expect_err("unknown field");
        assert!(error.to_string().contains("candidate_head_sha"), "unexpected error: {error}");
    }

    #[test]
    fn npm_cannot_gate_required_work_or_go_release_critical() {
        // Falsifier 10: make npm release-critical while #8301 is deferred.
        let report = mutated(|value| {
            node_index(value, "8301")["release_critical"] = Value::Bool(true);
        });
        assert!(report.contains("L12"), "unexpected: {report}");
    }

    #[test]
    fn deferred_channel_cannot_become_a_hard_predecessor() {
        let report = mutated(|value| {
            node_index(value, "11443")["hard_dependencies"] =
                serde_json::json!(["11434", "11436", "11437", "11438", "11439", "11441", "8301"]);
        });
        assert!(report.contains("L12"), "unexpected: {report}");
    }

    #[test]
    fn satellite_cannot_become_an_internal_train_node() {
        // Falsifier 9: pull a satellite into the internal graph.
        let report = mutated(|value| {
            let node = node_index(value, "8970");
            node["kind"] = Value::String("implementation".into());
        });
        assert!(report.contains("L13") || report.contains("NO_SPEC_DELTA"), "unexpected: {report}");
    }

    #[test]
    fn ready_status_requires_landed_hard_dependencies() {
        // Falsifier 11-adjacent: readiness without tree-truth predecessors.
        let report = mutated(|value| {
            node_index(value, "11445")["current_tree_status"] = Value::String("ready".into());
        });
        assert!(report.contains("L11"), "unexpected: {report}");
    }

    #[test]
    fn controller_packet_presence_is_a_law_violation() {
        // Falsifier: a controller claiming a compiled packet.
        let report = mutated(|value| {
            let node = node_index(value, "8087");
            node["spec_disposition"] = Value::String("SPEC_COMPILED".into());
            node["spec_ref"] = Value::String(".spec/8087-controller/".into());
        });
        assert!(report.contains("L07"), "unexpected: {report}");
    }

    #[test]
    fn missing_spec_disposition_is_rejected() {
        // Falsifier 14: omit a required spec disposition.
        let root = project_root().expect("project root");
        let bytes = std::fs::read_to_string(root.join(MANIFEST_RELATIVE_PATH)).expect("manifest");
        let mut value: Value = serde_json::from_str(&bytes).expect("json");
        let node = node_index(&mut value, "11432");
        node.as_object_mut().expect("object").remove("spec_disposition");
        let error = serde_json::from_value::<Manifest>(value).expect_err("missing field");
        assert!(error.to_string().contains("spec_disposition"), "unexpected: {error}");
    }

    #[test]
    fn tampered_manifest_fails_the_digest_pin() {
        let root = project_root().expect("project root");
        let bytes = std::fs::read_to_string(root.join(MANIFEST_RELATIVE_PATH)).expect("manifest");
        let mut value: Value = serde_json::from_str(&bytes).expect("json");
        let node = node_index(&mut value, "10333");
        node["one_pr_outcome"] = Value::String("tampered".into());
        let tampered = serde_json::to_string(&value).expect("serialize");
        let error = TrainGraph::parse(&tampered).expect_err("tampered manifest");
        assert!(error.to_string().contains("pinned"), "unexpected: {error}");
    }

    #[test]
    fn next_frontier_is_the_exact_checked_ready_set() {
        // Determinism/falsifier 15: the frontier is exactly the buildable,
        // unsettled nodes whose hard dependencies are all landed_current_tree
        // in the checked snapshot — lane-grouped, issue-ordered.
        let graph = load_graph();
        let frontier = graph.next_frontier();
        let ready: Vec<&str> =
            frontier.values().flatten().map(|node| node.node_id.as_str()).collect();
        assert_eq!(
            ready,
            vec![
                "8338", "9022", "10726", "11161", "11485", "7851", "7880", "9847", "9925", "10073",
                "11432", "7315", "9090", "10243", "9104", "9013",
            ],
            "unexpected frontier: {ready:?}"
        );
        assert!(
            frontier
                .get("public_route")
                .expect("route lane")
                .iter()
                .all(|node| node.node_id == "11432")
        );
    }

    #[test]
    fn supersession_is_symmetric_for_the_v1_boundary() {
        let graph = load_graph();
        let v2 = graph.node("10333").expect("v2 node");
        let v1 = graph.node("6355").expect("v1 node");
        assert!(v2.supersedes.contains(&"6355".to_string()));
        assert!(v1.superseded_by.contains(&"10333".to_string()));
    }
}
