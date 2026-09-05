//! Canonical validator and deterministic projection surface for the
//! authority-transfer stable programme graph (#11697, leaf AT01 of #11696).
//!
//! The graph owns accepted programme topology and semantic work ownership
//! only. It must never carry current issue state, PR or branch identity, main
//! SHAs, proof verdicts, readiness, assignees or leases, completion estimates,
//! or model routing; those belong to #11698/#11699 projections keyed to this
//! graph. Validation therefore fails closed on mutable-state leakage, unknown
//! fields, unknown edge/profile semantics, duplicate registry identities,
//! artifact-owner or authority-input misbinding, incomplete or split fan-in
//! denominators, unordered exclusive writers, scheduling cycles over the
//! combined hard/parallel-after relation, hardened optional-live edges,
//! retirement without a resolvable declared predecessor exit, and controllers
//! outside the controller class, and normalizes deterministically to
//! byte-identical output.
//!
//! Surfaces:
//!
//! ```text
//! authority-transfer-graph check
//! authority-transfer-graph graph
//! authority-transfer-graph explain <node-id>
//! authority-transfer-graph normalized-manifest
//! ```
//!
//! Exit contract: 0 = valid (and projection current under `check`), 2 = typed
//! rejection or projection drift, 3 = instrument failure (unreadable or
//! syntactically malformed input never resolves to a valid graph). Every
//! surface maps the two failure shapes explicitly: a well-formed document that
//! violates schema or policy is a typed exit-2 rejection on `check`, `graph`,
//! `explain`, and `normalized-manifest` alike; only read/syntax breakage is an
//! instrument failure. Under `check` an instrument failure takes precedence
//! over any typed rejection, and instrument failures never count as
//! rejections.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const GRAPH_SCHEMA: &str = "authority_transfer_programme_graph.v1";
const FIXTURE_SCHEMA: &str = "authority_transfer_programme_fixture.v1";
const EXIT_REJECTED: i32 = 2;
const EXIT_NOT_PROVEN: i32 = 3;
const MAX_GRAPH_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FIXTURE_BYTES: u64 = 512 * 1024;

const DEFAULT_MANIFEST: &str = ".ci/authority-transfer-programme/graph.v1.json";
const DEFAULT_GENERATED: &str =
    ".ci/authority-transfer-programme/generated/normalized-graph.v1.json";
const DEFAULT_FIXTURES_DIR: &str = ".ci/authority-transfer-programme/fixtures";

/// Current-truth vocabulary that must never appear as a key inside the stable
/// graph document. Values are inert data; these names are reserved because a
/// key of this shape can only encode mutable observations.
const BANNED_CURRENT_STATE_KEYS: [&str; 21] = [
    "state",
    "pull_request",
    "branch",
    "main_sha",
    "base_sha",
    "head_sha",
    "current_frontier",
    "readiness",
    "ready",
    "assignee",
    "lease",
    "percent_complete",
    "completion_percentage",
    "estimated_completion",
    "model",
    "provider",
    "token_routing",
    "proof_status",
    "verdict",
    "merged_at",
    "closed_at",
];

/// A node key outside this set can only smuggle programme-local copies of
/// domain policy owned by a canonical issue or contract.
const NODE_ALLOWED_KEYS: [&str; 21] = [
    "node_id",
    "issue",
    "controller",
    "rail",
    "phase",
    "kind",
    "buildable",
    "observation_optional",
    "edges",
    "operation_profile",
    "evidence_profile",
    "authority_inputs",
    "authority_outputs",
    "conflicts",
    "claim_ceiling",
    "non_claims",
    "first_falsifier",
    "artifacts",
    "predecessor_exit",
    "handoff",
    "terminal_relation",
];

#[derive(Debug, Parser)]
#[command(name = "authority-transfer-graph")]
#[command(about = "Validate the stable authority-transfer programme graph (#11697)")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate the stable graph, every shift-left fixture, and projection currency.
    Check {
        /// Stable graph document; defaults to the repository canonical manifest.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Directory of programme fixtures; defaults to the canonical directory.
        #[arg(long)]
        fixtures_dir: Option<PathBuf>,
        /// Committed normalized projection compared byte-for-byte.
        #[arg(long)]
        generated: Option<PathBuf>,
    },
    /// Print a deterministic structural summary of the stable graph.
    Graph {
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Explain one node without reporting any current readiness.
    Explain {
        /// Stable node ID, e.g. AT01.
        node_id: String,
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Print the canonical normalized graph (byte-identical across runs).
    NormalizedManifest {
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Code {
    SchemaRejection,
    StateLeakage,
    EmbeddedDomainPolicy,
    DuplicateNodeId,
    DuplicateFalsifierId,
    DuplicateEvidenceProfile,
    DuplicateOperationProfile,
    DuplicateRail,
    UnknownRail,
    ControllerReferenceUnknown,
    ControllerKindMismatch,
    ControllerMarkedBuildable,
    GovernanceNodeBuildable,
    LeafMissingClaimCeiling,
    LeafMissingFirstFalsifier,
    LeafMissingHandoff,
    UnknownProfile,
    ArtifactOwnerMismatch,
    ArtifactIdMultiOwner,
    AuthorityOutputMultiOwner,
    AuthorityInputUnowned,
    AuthorityInputUnrelated,
    UnknownEdgeTarget,
    HandoffTargetUnknown,
    ConsumerWithoutAcceptedStore,
    FaninDenominatorIncomplete,
    LiveEnforcementBeforeAdvisoryAuthority,
    RetirementWithoutPredecessorExit,
    PredecessorIdentityUnresolved,
    PredecessorConsumerUnknown,
    OptionalEdgePromotedHard,
    ParallelExclusiveConflict,
    ParallelScheduleCycle,
    HardDependencyCycle,
    ProjectionDrift,
}

impl Code {
    fn as_str(self) -> &'static str {
        match self {
            Self::SchemaRejection => "SCHEMA_REJECTION",
            Self::StateLeakage => "STATE_LEAKAGE",
            Self::EmbeddedDomainPolicy => "EMBEDDED_DOMAIN_POLICY",
            Self::DuplicateNodeId => "DUPLICATE_NODE_ID",
            Self::DuplicateFalsifierId => "DUPLICATE_FALSIFIER_ID",
            Self::DuplicateEvidenceProfile => "DUPLICATE_EVIDENCE_PROFILE",
            Self::DuplicateOperationProfile => "DUPLICATE_OPERATION_PROFILE",
            Self::DuplicateRail => "DUPLICATE_RAIL",
            Self::UnknownRail => "UNKNOWN_RAIL",
            Self::ControllerReferenceUnknown => "CONTROLLER_REFERENCE_UNKNOWN",
            Self::ControllerKindMismatch => "CONTROLLER_KIND_MISMATCH",
            Self::ControllerMarkedBuildable => "CONTROLLER_MARKED_BUILDABLE",
            Self::GovernanceNodeBuildable => "GOVERNANCE_NODE_BUILDABLE",
            Self::LeafMissingClaimCeiling => "LEAF_MISSING_CLAIM_CEILING",
            Self::LeafMissingFirstFalsifier => "LEAF_MISSING_FIRST_FALSIFIER",
            Self::LeafMissingHandoff => "LEAF_MISSING_HANDOFF",
            Self::UnknownProfile => "UNKNOWN_PROFILE",
            Self::ArtifactOwnerMismatch => "ARTIFACT_OWNER_MISMATCH",
            Self::ArtifactIdMultiOwner => "ARTIFACT_ID_MULTI_OWNER",
            Self::AuthorityOutputMultiOwner => "AUTHORITY_OUTPUT_MULTI_OWNER",
            Self::AuthorityInputUnowned => "AUTHORITY_INPUT_UNOWNED",
            Self::AuthorityInputUnrelated => "AUTHORITY_INPUT_UNRELATED",
            Self::UnknownEdgeTarget => "UNKNOWN_EDGE_TARGET",
            Self::HandoffTargetUnknown => "HANDOFF_TARGET_UNKNOWN",
            Self::ConsumerWithoutAcceptedStore => "CONSUMER_WITHOUT_ACCEPTED_STORE",
            Self::FaninDenominatorIncomplete => "FANIN_DENOMINATOR_INCOMPLETE",
            Self::LiveEnforcementBeforeAdvisoryAuthority => {
                "LIVE_ENFORCEMENT_BEFORE_ADVISORY_AUTHORITY"
            }
            Self::RetirementWithoutPredecessorExit => "RETIREMENT_WITHOUT_PREDECESSOR_EXIT",
            Self::PredecessorIdentityUnresolved => "PREDECESSOR_IDENTITY_UNRESOLVED",
            Self::PredecessorConsumerUnknown => "PREDECESSOR_CONSUMER_UNKNOWN",
            Self::OptionalEdgePromotedHard => "OPTIONAL_EDGE_PROMOTED_HARD",
            Self::ParallelExclusiveConflict => "PARALLEL_EXCLUSIVE_CONFLICT",
            Self::ParallelScheduleCycle => "PARALLEL_SCHEDULE_CYCLE",
            Self::HardDependencyCycle => "HARD_DEPENDENCY_CYCLE",
            Self::ProjectionDrift => "PROJECTION_DRIFT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Violation {
    code: Code,
    subject: String,
    detail: String,
}

impl Violation {
    fn new(code: Code, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { code, subject: subject.into(), detail: detail.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NodeKind {
    Controller,
    Containment,
    Contract,
    Compiler,
    EvidenceRegistry,
    Evaluator,
    AdvisoryIntegration,
    LiveEnforcement,
    SourceAdapter,
    Coordinator,
    AcceptedStore,
    ConsumerCutover,
    ExactProcessFanin,
    Retirement,
    Dogfood,
}

impl NodeKind {
    /// Roles that are governance, never builder-frontier work.
    fn is_governance(self) -> bool {
        matches!(self, Self::Controller | Self::ExactProcessFanin | Self::LiveEnforcement)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Controller => "controller",
            Self::Containment => "containment",
            Self::Contract => "contract",
            Self::Compiler => "compiler",
            Self::EvidenceRegistry => "evidence_registry",
            Self::Evaluator => "evaluator",
            Self::AdvisoryIntegration => "advisory_integration",
            Self::LiveEnforcement => "live_enforcement",
            Self::SourceAdapter => "source_adapter",
            Self::Coordinator => "coordinator",
            Self::AcceptedStore => "accepted_store",
            Self::ConsumerCutover => "consumer_cutover",
            Self::ExactProcessFanin => "exact_process_fanin",
            Self::Retirement => "retirement",
            Self::Dogfood => "dogfood",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthorityMode {
    Exclusive,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TerminalRelation {
    CloseLeaf,
    AdvanceController,
    ManualExternal,
    None,
}

impl TerminalRelation {
    fn as_str(self) -> &'static str {
        match self {
            Self::CloseLeaf => "close_leaf",
            Self::AdvanceController => "advance_controller",
            Self::ManualExternal => "manual_external",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Edges {
    hard: Vec<String>,
    evidence: Vec<String>,
    optional: Vec<String>,
    parallel_after: Vec<String>,
    fan_in: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityOutput {
    key: String,
    mode: AuthorityMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conflicts {
    exclusive: Vec<String>,
    shared: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    id: String,
    owner: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PredecessorExit {
    /// Declared stable node identities being retired. Free prose cannot be
    /// resolved or falsified, so every predecessor identity must name a node.
    predecessor: Vec<String>,
    consumers: Vec<String>,
    exit_condition: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    node_id: String,
    issue: u64,
    controller: Option<String>,
    rail: String,
    phase: String,
    kind: NodeKind,
    buildable: bool,
    observation_optional: bool,
    edges: Edges,
    operation_profile: String,
    evidence_profile: String,
    authority_inputs: Vec<String>,
    authority_outputs: Vec<AuthorityOutput>,
    conflicts: Conflicts,
    claim_ceiling: String,
    non_claims: Vec<String>,
    first_falsifier: String,
    artifacts: Vec<Artifact>,
    predecessor_exit: Option<PredecessorExit>,
    handoff: Option<String>,
    terminal_relation: TerminalRelation,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Falsifier {
    id: String,
    statement: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgrammeGraph {
    schema_version: String,
    programme_issue: u64,
    rails: Vec<String>,
    operation_profiles: Vec<String>,
    evidence_profiles: Vec<String>,
    first_falsifiers: Vec<Falsifier>,
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureEnvelope {
    schema_version: String,
    #[allow(dead_code)]
    description: String,
    expected_code: String,
    graph: Value,
}

#[derive(Debug, Serialize)]
struct NormalizedGraph {
    schema_version: &'static str,
    programme_issue: u64,
    rails: BTreeSet<String>,
    operation_profiles: BTreeSet<String>,
    evidence_profiles: BTreeSet<String>,
    first_falsifiers: BTreeMap<String, String>,
    nodes: Vec<NormalizedNode>,
}

#[derive(Debug, Serialize)]
struct NormalizedNode {
    node_id: String,
    issue: u64,
    controller: Option<String>,
    rail: String,
    phase: String,
    kind: &'static str,
    buildable: bool,
    observation_optional: bool,
    edges: NormalizedEdges,
    operation_profile: String,
    evidence_profile: String,
    authority_inputs: BTreeSet<String>,
    authority_outputs: BTreeMap<String, &'static str>,
    conflicts: NormalizedConflicts,
    claim_ceiling: String,
    non_claims: BTreeSet<String>,
    first_falsifier: String,
    artifacts: BTreeMap<String, String>,
    predecessor_exit: Option<NormalizedPredecessorExit>,
    handoff: Option<String>,
    terminal_relation: &'static str,
}

#[derive(Debug, Serialize)]
struct NormalizedEdges {
    hard: BTreeSet<String>,
    evidence: BTreeSet<String>,
    optional: BTreeSet<String>,
    parallel_after: BTreeSet<String>,
    fan_in: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct NormalizedConflicts {
    exclusive: BTreeSet<String>,
    shared: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct NormalizedPredecessorExit {
    predecessor: BTreeSet<String>,
    consumers: BTreeSet<String>,
    exit_condition: String,
}

fn main() {
    if let Err(error) = run_cli() {
        match error.downcast::<CliRejection>() {
            Ok(rejection) => {
                report_rejections(&rejection.0);
                exit(EXIT_REJECTED);
            }
            Err(error) => {
                eprintln!("INSTRUMENT_FAILURE authority-transfer-graph: {error}");
                exit(EXIT_NOT_PROVEN);
            }
        }
    }
}

/// A well-formed graph document that violates schema or policy. This is a
/// typed exit-2 rejection on every CLI surface, never an instrument failure.
#[derive(Debug)]
struct CliRejection(Vec<Violation>);

impl std::fmt::Display for CliRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rejected: {}",
            self.0.iter().map(|v| v.code.as_str()).collect::<Vec<_>>().join(",")
        )
    }
}

impl std::error::Error for CliRejection {}

fn report_rejections(violations: &[Violation]) {
    for violation in violations {
        eprintln!("{} {} {}", violation.code.as_str(), violation.subject, violation.detail);
    }
}

fn run_cli() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let root = repo_root()?;
    match args.command {
        Command::Check { manifest, fixtures_dir, generated } => {
            run_check(
                root.join(manifest.unwrap_or_else(|| PathBuf::from(DEFAULT_MANIFEST))),
                root.join(fixtures_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_FIXTURES_DIR))),
                root.join(generated.unwrap_or_else(|| PathBuf::from(DEFAULT_GENERATED))),
            );
            Ok(())
        }
        Command::Graph { manifest } => {
            let path = root.join(manifest.unwrap_or_else(|| PathBuf::from(DEFAULT_MANIFEST)));
            let graph = read_surface_graph(&path)?;
            if report_if_rejected(&validate(&graph)) {
                exit(EXIT_REJECTED);
            }
            println!("{}", graph_summary(&normalize(&graph)));
            Ok(())
        }
        Command::Explain { node_id, manifest } => {
            let path = root.join(manifest.unwrap_or_else(|| PathBuf::from(DEFAULT_MANIFEST)));
            let graph = read_surface_graph(&path)?;
            if report_if_rejected(&validate(&graph)) {
                exit(EXIT_REJECTED);
            }
            match render_explain(&graph, &node_id) {
                Some(rendered) => {
                    println!("{rendered}");
                    Ok(())
                }
                None => {
                    eprintln!("EXPLAIN_TARGET_UNKNOWN `{node_id}` is not a declared node");
                    exit(EXIT_REJECTED);
                }
            }
        }
        Command::NormalizedManifest { manifest } => {
            let path = root.join(manifest.unwrap_or_else(|| PathBuf::from(DEFAULT_MANIFEST)));
            let graph = read_surface_graph(&path)?;
            if report_if_rejected(&validate(&graph)) {
                exit(EXIT_REJECTED);
            }
            print!("{}", normalized_json(&graph));
            Ok(())
        }
    }
}

/// Reports typed violations on stderr. Returns true when the caller must exit
/// with the typed-rejection status instead of continuing.
fn report_if_rejected(violations: &[Violation]) -> bool {
    if violations.is_empty() {
        return false;
    }
    report_rejections(violations);
    true
}

fn run_check(manifest: PathBuf, fixtures_dir: PathBuf, generated: PathBuf) {
    let mut rejected = false;
    let mut instrument_failure = false;

    match read_bounded(&manifest, MAX_GRAPH_BYTES, "stable programme graph") {
        Ok(raw) => match parse_graph_document(&raw) {
            Err(syntax_error) => {
                // Read/syntax breakage is an instrument failure, never a typed
                // rejection; instrument failure takes precedence at exit.
                instrument_failure = true;
                println!(
                    "FAIL stable-programme-graph instrument failure: parsing {}: {syntax_error}",
                    manifest.display()
                );
            }
            Ok(Err(violations)) => {
                rejected = true;
                report_check_rows("stable-programme-graph", &violations);
            }
            Ok(Ok(graph)) => {
                let mut violations = validate(&graph);
                match fs::read(&generated) {
                    Ok(committed) => {
                        if committed != normalized_bytes(&graph) {
                            violations.push(Violation::new(
                                Code::ProjectionDrift,
                                generated.display().to_string(),
                                "committed normalized projection differs from regenerated bytes"
                                    .to_string(),
                            ));
                        }
                    }
                    Err(error) => {
                        violations.push(Violation::new(
                            Code::ProjectionDrift,
                            generated.display().to_string(),
                            format!("committed projection unreadable: {error}"),
                        ));
                    }
                }
                report_check_rows("stable-programme-graph", &violations);
                rejected |= !violations.is_empty();
            }
        },
        Err(error) => {
            instrument_failure = true;
            println!("FAIL stable-programme-graph instrument failure: {error}");
        }
    }

    match collected_fixtures(&fixtures_dir) {
        Ok(paths) => {
            for path in paths {
                match evaluate_fixture(&path) {
                    Ok(()) => println!("PASS {}", path.display()),
                    Err(FixtureError::Rejected(rows)) => {
                        rejected = true;
                        report_check_rows(&path.display().to_string(), &rows);
                    }
                    Err(FixtureError::Instrument(error)) => {
                        instrument_failure = true;
                        println!("FAIL {} instrument failure: {error}", path.display());
                    }
                }
            }
        }
        Err(error) => {
            instrument_failure = true;
            println!("FAIL fixtures inventory: {error}");
        }
    }

    if instrument_failure {
        exit(EXIT_NOT_PROVEN);
    }
    if rejected {
        exit(EXIT_REJECTED);
    }
    println!("PASS authority-transfer programme graph, fixtures, and projection");
}

fn report_check_rows(subject: &str, violations: &[Violation]) {
    if violations.is_empty() {
        return;
    }
    println!("FAIL {subject}");
    for violation in violations {
        println!("  {} {} {}", violation.code.as_str(), violation.subject, violation.detail);
    }
}

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_bounded(path: &Path, limit: u64, what: &str) -> Result<Vec<u8>, io::Error> {
    let meta = fs::metadata(path)
        .map_err(|error| io::Error::other(format!("{what} {}: {error}", path.display())))?;
    if meta.len() > limit {
        return Err(io::Error::other(format!(
            "{what} {} exceeds the {limit}-byte bound",
            path.display()
        )));
    }
    fs::read(path)
        .map_err(|error| io::Error::other(format!("reading {what} {}: {error}", path.display())))
}

/// Failure shape for the read surfaces. Instrument covers unreadable or
/// syntactically malformed input; Rejected covers a well-formed document that
/// violates schema, state-leakage policy, or node-key policy.
enum LoadFailure {
    Instrument(io::Error),
    Rejected(Vec<Violation>),
}

/// Loads a graph for `graph` / `explain` / `normalized-manifest`, mapping a
/// typed document rejection onto the exit-2 surface instead of erasing it
/// into an instrument failure.
fn read_surface_graph(path: &Path) -> Result<ProgrammeGraph, Box<dyn Error>> {
    match load_graph_or_reject(path) {
        Ok(graph) => Ok(graph),
        Err(LoadFailure::Rejected(violations)) => Err(Box::new(CliRejection(violations))),
        Err(LoadFailure::Instrument(error)) => Err(error.into()),
    }
}

fn load_graph_or_reject(path: &Path) -> Result<ProgrammeGraph, LoadFailure> {
    let raw = read_bounded(path, MAX_GRAPH_BYTES, "stable programme graph")
        .map_err(LoadFailure::Instrument)?;
    match parse_graph_document(&raw) {
        Err(syntax_error) => Err(LoadFailure::Instrument(io::Error::other(format!(
            "parsing {}: {syntax_error}",
            path.display()
        )))),
        Ok(Ok(graph)) => Ok(graph),
        Ok(Err(violations)) => Err(LoadFailure::Rejected(violations)),
    }
}

/// Parse a stable graph document. Syntactic breakage is an instrument error;
/// a well-formed document that violates the schema is a typed rejection list.
fn parse_graph_document(raw: &[u8]) -> Result<Result<ProgrammeGraph, Vec<Violation>>, String> {
    let document: Value = serde_json::from_slice(raw).map_err(|error| error.to_string())?;

    let mut violations = scan_current_state_leakage(&document);
    violations.extend(scan_embedded_domain_policy(&document));
    if !violations.is_empty() {
        return Ok(Err(violations));
    }

    match serde_json::from_value::<ProgrammeGraph>(document) {
        Ok(graph) if graph.schema_version == GRAPH_SCHEMA => Ok(Ok(graph)),
        Ok(_) => Ok(Err(vec![Violation::new(
            Code::SchemaRejection,
            "schema_version",
            format!("expected {GRAPH_SCHEMA}"),
        )])),
        Err(error) => {
            Ok(Err(vec![Violation::new(Code::SchemaRejection, "document", error.to_string())]))
        }
    }
}

fn collect_object_keys(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                visit(key);
                collect_object_keys(child, visit);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_object_keys(item, visit);
            }
        }
        _ => {}
    }
}

fn scan_current_state_leakage(document: &Value) -> Vec<Violation> {
    let mut hits = Vec::new();
    collect_object_keys(document, &mut |key| {
        if BANNED_CURRENT_STATE_KEYS.contains(&key) {
            hits.push(key.to_string());
        }
    });
    hits.sort();
    hits.dedup();
    hits.into_iter()
        .map(|key| {
            Violation::new(
                Code::StateLeakage,
                key,
                "current-state vocabulary is forbidden inside the stable graph".to_string(),
            )
        })
        .collect()
}

fn scan_embedded_domain_policy(document: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Some(nodes) = document.get("nodes").and_then(Value::as_array) else {
        return violations;
    };
    for node in nodes {
        let Some(object) = node.as_object() else {
            continue;
        };
        let id = object.get("node_id").and_then(Value::as_str).unwrap_or("<unknown>");
        // A banned current-state key is the more precise rejection reason, so
        // an otherwise-unknown field is only a domain-policy copy when no
        // reserved key explains it.
        let leaks_state =
            object.keys().any(|key| BANNED_CURRENT_STATE_KEYS.contains(&key.as_str()));
        if leaks_state {
            continue;
        }
        let extra = object
            .keys()
            .filter(|key| !NODE_ALLOWED_KEYS.contains(&key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(first) = extra.first() {
            violations.push(Violation::new(
                Code::EmbeddedDomainPolicy,
                id.to_string(),
                format!(
                    "field `{first}` is outside the node schema; import domain semantics by reference instead"
                ),
            ));
        }
    }
    violations.sort_by(|a, b| a.subject.cmp(&b.subject));
    violations
}

fn validate(graph: &ProgrammeGraph) -> Vec<Violation> {
    let mut index: BTreeMap<&str, &Node> = BTreeMap::new();
    let mut duplicated: BTreeSet<&str> = BTreeSet::new();
    for node in &graph.nodes {
        if index.insert(node.node_id.as_str(), node).is_some() {
            duplicated.insert(node.node_id.as_str());
        }
    }
    let known = |id: &str| index.contains_key(id);

    let op_profiles: BTreeSet<&str> = graph.operation_profiles.iter().map(String::as_str).collect();
    let ev_profiles: BTreeSet<&str> = graph.evidence_profiles.iter().map(String::as_str).collect();
    let falsifiers: BTreeSet<&str> = graph.first_falsifiers.iter().map(|f| f.id.as_str()).collect();
    let rails: BTreeSet<&str> = graph.rails.iter().map(String::as_str).collect();

    let mut violations = Vec::new();
    for id in &duplicated {
        violations.push(Violation::new(
            Code::DuplicateNodeId,
            (*id).to_string(),
            "stable node ID declared more than once".to_string(),
        ));
    }

    // Registry IDs are canonical keys. Silent set-insertion would accept
    // duplicate rails, profiles, and falsifier rows and let normalization
    // keep whichever duplicate row appears last, so every registry identity
    // must be unique before reference validation runs.
    fn repeated<'a>(rows: impl Iterator<Item = &'a String>) -> Vec<&'a str> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows {
            *counts.entry(row.as_str()).or_default() += 1;
        }
        counts.into_iter().filter(|(_, count)| *count > 1).map(|(id, _)| id).collect()
    }

    for rail in repeated(graph.rails.iter()) {
        violations.push(Violation::new(
            Code::DuplicateRail,
            rail,
            format!("rail `{rail}` is declared more than once"),
        ));
    }
    for profile in repeated(graph.operation_profiles.iter()) {
        violations.push(Violation::new(
            Code::DuplicateOperationProfile,
            profile,
            format!("operation profile `{profile}` is registered more than once"),
        ));
    }
    for profile in repeated(graph.evidence_profiles.iter()) {
        violations.push(Violation::new(
            Code::DuplicateEvidenceProfile,
            profile,
            format!("evidence profile `{profile}` is registered more than once"),
        ));
    }
    for falsifier in repeated(graph.first_falsifiers.iter().map(|falsifier| &falsifier.id)) {
        violations.push(Violation::new(
            Code::DuplicateFalsifierId,
            falsifier,
            format!(
                "first falsifier `{falsifier}` is registered with competing statements; \
                 normalization would silently keep the last row"
            ),
        ));
    }

    let mut artifact_owners: BTreeMap<&str, &str> = BTreeMap::new();
    let mut authority_owners: BTreeMap<&str, &str> = BTreeMap::new();

    for node in &graph.nodes {
        let id = node.node_id.as_str();

        if !rails.contains(node.rail.as_str()) {
            violations.push(Violation::new(
                Code::UnknownRail,
                id,
                format!("rail `{}` is not declared", node.rail),
            ));
        }
        if let Some(controller) = &node.controller {
            match index.get(controller.as_str()) {
                None => violations.push(Violation::new(
                    Code::ControllerReferenceUnknown,
                    id,
                    format!("controller `{controller}` is not a declared node"),
                )),
                Some(target) if target.kind != NodeKind::Controller => {
                    violations.push(Violation::new(
                        Code::ControllerKindMismatch,
                        id,
                        format!(
                            "controller `{controller}` is kind `{}`, not a controller-class node",
                            target.kind.as_str()
                        ),
                    ));
                }
                Some(_) => {}
            }
        }

        if node.kind == NodeKind::Controller && node.buildable {
            violations.push(Violation::new(
                Code::ControllerMarkedBuildable,
                id,
                "controllers are non-builder nodes".to_string(),
            ));
        }
        if matches!(node.kind, NodeKind::ExactProcessFanin | NodeKind::LiveEnforcement)
            && node.buildable
        {
            violations.push(Violation::new(
                Code::GovernanceNodeBuildable,
                id,
                format!("`{}` nodes are governance roles, never builders", node.kind.as_str()),
            ));
        }

        // Leaf completeness applies to executable work only; governance rows
        // carry their own ceiling and are earned through fan-in instead.
        if node.buildable && !node.kind.is_governance() {
            if node.claim_ceiling.trim().is_empty() {
                violations.push(Violation::new(
                    Code::LeafMissingClaimCeiling,
                    id,
                    "buildable leaves require a bounded claim ceiling".to_string(),
                ));
            }
            if !falsifiers.contains(node.first_falsifier.as_str()) {
                violations.push(Violation::new(
                    Code::LeafMissingFirstFalsifier,
                    id,
                    format!("first falsifier `{}` is not registered", node.first_falsifier),
                ));
            }
            if node.handoff.is_none() {
                violations.push(Violation::new(
                    Code::LeafMissingHandoff,
                    id,
                    "buildable leaves require an explicit downstream handoff".to_string(),
                ));
            }
        }

        if !op_profiles.contains(node.operation_profile.as_str())
            || !ev_profiles.contains(node.evidence_profile.as_str())
        {
            violations.push(Violation::new(
                Code::UnknownProfile,
                id,
                format!(
                    "operation `{}` / evidence `{}` must both be registered",
                    node.operation_profile, node.evidence_profile
                ),
            ));
        }

        for artifact in &node.artifacts {
            // Ownership is indexed under the declaring node's stable ID, so a
            // declaration naming any other node — known or not — would emit a
            // normalized graph whose owner contradicts the uniqueness law.
            if artifact.owner != id {
                violations.push(Violation::new(
                    Code::ArtifactOwnerMismatch,
                    id,
                    format!(
                        "artifact `{}` declares owner `{}`, but ownership binds to the declaring node",
                        artifact.id, artifact.owner
                    ),
                ));
            }
            if let Some(previous) = artifact_owners.insert(artifact.id.as_str(), id) {
                violations.push(Violation::new(
                    Code::ArtifactIdMultiOwner,
                    artifact.id.clone(),
                    format!("declared by `{previous}` and `{id}`"),
                ));
            }
        }

        for output in &node.authority_outputs {
            if let Some(previous) = authority_owners.insert(output.key.as_str(), id) {
                violations.push(Violation::new(
                    Code::AuthorityOutputMultiOwner,
                    output.key.clone(),
                    format!("owned by `{previous}` and `{id}`"),
                ));
            }
        }

        if node.kind == NodeKind::ConsumerCutover
            && !node.edges.hard.iter().any(|target| {
                index.get(target.as_str()).is_some_and(|dep| dep.kind == NodeKind::AcceptedStore)
            })
        {
            violations.push(Violation::new(
                Code::ConsumerWithoutAcceptedStore,
                id,
                "consumer cutovers require a hard dependency on an accepted store".to_string(),
            ));
        }

        if node.kind == NodeKind::LiveEnforcement
            && !node.edges.hard.iter().any(|target| {
                index
                    .get(target.as_str())
                    .is_some_and(|dep| dep.kind == NodeKind::AdvisoryIntegration)
            })
        {
            violations.push(Violation::new(
                Code::LiveEnforcementBeforeAdvisoryAuthority,
                id,
                "required live enforcement needs a hard edge to advisory evidence/promotion authority"
                    .to_string(),
            ));
        }

        if node.kind == NodeKind::Retirement {
            match &node.predecessor_exit {
                None => violations.push(Violation::new(
                    Code::RetirementWithoutPredecessorExit,
                    id,
                    "retirement requires predecessor identity and an exit condition".to_string(),
                )),
                Some(predecessor_exit) => {
                    if predecessor_exit.predecessor.is_empty()
                        || predecessor_exit.exit_condition.trim().is_empty()
                    {
                        violations.push(Violation::new(
                            Code::RetirementWithoutPredecessorExit,
                            id,
                            "predecessor identity and exit condition must be non-empty".to_string(),
                        ));
                    }
                    for predecessor in &predecessor_exit.predecessor {
                        if !known(predecessor) {
                            violations.push(Violation::new(
                                Code::PredecessorIdentityUnresolved,
                                id,
                                format!("retired predecessor identity `{predecessor}` is not a declared node"),
                            ));
                        }
                    }
                    for consumer in &predecessor_exit.consumers {
                        if !known(consumer) {
                            violations.push(Violation::new(
                                Code::PredecessorConsumerUnknown,
                                id,
                                format!("predecessor consumer `{consumer}` is not declared"),
                            ));
                        }
                    }
                }
            }
        }

        for target in node
            .edges
            .hard
            .iter()
            .chain(&node.edges.evidence)
            .chain(&node.edges.optional)
            .chain(&node.edges.parallel_after)
            .chain(&node.edges.fan_in)
        {
            if !known(target) {
                violations.push(Violation::new(
                    Code::UnknownEdgeTarget,
                    id,
                    format!("edge target `{target}` is not declared"),
                ));
            }
        }

        if let Some(handoff) = &node.handoff
            && !known(handoff)
        {
            violations.push(Violation::new(
                Code::HandoffTargetUnknown,
                id,
                format!("handoff `{handoff}` is not declared"),
            ));
        }

        for optional_target in &node.edges.optional {
            if node.edges.hard.contains(optional_target) {
                violations.push(Violation::new(
                    Code::OptionalEdgePromotedHard,
                    id,
                    format!("`{optional_target}` is listed as both hard and optional"),
                ));
            }
        }
    }

    // Optional live observation may never become somebody's hard prerequisite.
    for node in graph.nodes.iter().filter(|node| node.observation_optional) {
        for candidate in index.values() {
            if candidate.edges.hard.contains(&node.node_id) {
                violations.push(Violation::new(
                    Code::OptionalEdgePromotedHard,
                    candidate.node_id.clone(),
                    format!("hard edge targets optional observation node `{}`", node.node_id),
                ));
            }
        }
    }

    // Every authority anybody consumes must have exactly one owner, and the
    // consumer must be joined to that owner through a dependency-carrying
    // relationship (hard, evidence, or fan-in). Global existence alone would
    // let the topology and the authority projection contradict each other.
    // Optional edges are excluded because optional observation may never be a
    // prerequisite, and parallel_after is pure ordering, not dependence. A
    // node consuming its own output is exempt from the join requirement.
    for node in &graph.nodes {
        for input in &node.authority_inputs {
            let Some(owner_id) = authority_owners.get(input.as_str()) else {
                violations.push(Violation::new(
                    Code::AuthorityInputUnowned,
                    node.node_id.clone(),
                    format!("authority input `{input}` has no owning node"),
                ));
                continue;
            };
            let joined = *owner_id == node.node_id.as_str()
                || node
                    .edges
                    .hard
                    .iter()
                    .chain(&node.edges.evidence)
                    .chain(&node.edges.fan_in)
                    .any(|target| target == owner_id);
            if !joined {
                violations.push(Violation::new(
                    Code::AuthorityInputUnrelated,
                    node.node_id.clone(),
                    format!(
                        "authority input `{input}` is owned by `{owner_id}`, but no hard/evidence/fan-in edge joins this node to it"
                    ),
                ));
            }
        }
    }

    // Every consumer cutover belongs to the exact-process denominator, and one
    // single fan-in must own the complete declared denominator: splitting the
    // rows across partial fan-ins hides an unproven integration point.
    let fanned: BTreeSet<&str> =
        graph.nodes.iter().flat_map(|node| node.edges.fan_in.iter()).map(String::as_str).collect();
    for node in &graph.nodes {
        if node.kind == NodeKind::ConsumerCutover && !fanned.contains(node.node_id.as_str()) {
            violations.push(Violation::new(
                Code::FaninDenominatorIncomplete,
                node.node_id.clone(),
                "consumer cutover is missing from every exact-process fan-in denominator"
                    .to_string(),
            ));
        }
    }
    let cutovers: BTreeSet<&str> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::ConsumerCutover)
        .map(|node| node.node_id.as_str())
        .collect();
    if !cutovers.is_empty() {
        let complete_fanin_exists = graph.nodes.iter().any(|node| {
            node.kind == NodeKind::ExactProcessFanin
                && cutovers
                    .iter()
                    .all(|cutover| node.edges.fan_in.iter().any(|target| target == cutover))
        });
        if !complete_fanin_exists {
            violations.push(Violation::new(
                Code::FaninDenominatorIncomplete,
                "consumer_cutover_denominator".to_string(),
                format!(
                    "no single exact-process fan-in covers the complete declared \
                     {}-row consumer_cutover denominator",
                    cutovers.len()
                ),
            ));
        }
    }

    violations.extend(parallel_exclusive_conflicts(graph, &index, &duplicated));
    if let Some((code, member, detail)) = scheduling_cycle(&graph.nodes) {
        violations.push(Violation::new(code, member, detail));
    }

    violations
        .sort_by(|a, b| (&a.code, &a.subject, &a.detail).cmp(&(&b.code, &b.subject, &b.detail)));
    violations.dedup();
    violations
}

/// Two nodes sharing an exclusive conflict key are serialized only when a hard
/// or parallel-after ordering relationship exists between them.
fn parallel_exclusive_conflicts<'a>(
    graph: &'a ProgrammeGraph,
    index: &BTreeMap<&str, &Node>,
    duplicated: &BTreeSet<&'a str>,
) -> Vec<Violation> {
    let mut by_key: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in &graph.nodes {
        if duplicated.contains(node.node_id.as_str()) {
            continue;
        }
        for key in &node.conflicts.exclusive {
            by_key.entry(key.as_str()).or_default().push(&node.node_id);
        }
    }

    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for node in &graph.nodes {
        let entry = adjacency.entry(node.node_id.as_str()).or_default();
        for target in node.edges.hard.iter().chain(&node.edges.parallel_after) {
            entry.insert(target.as_str());
        }
    }

    let mut violations = Vec::new();
    for (key, mut owners) in by_key {
        owners.sort();
        owners.dedup();
        for (position, left) in owners.iter().enumerate() {
            for right in &owners[position + 1..] {
                if reaches(&adjacency, left, right) || reaches(&adjacency, right, left) {
                    continue;
                }
                violations.push(Violation::new(
                    Code::ParallelExclusiveConflict,
                    format!("{left}+{right}"),
                    format!("unordered writers share exclusive conflict key `{key}`"),
                ));
            }
        }
    }
    let _ = index;
    violations
}

fn reaches(adjacency: &BTreeMap<&str, BTreeSet<&str>>, from: &str, to: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut queue = vec![from];
    while let Some(current) = queue.pop() {
        if current == to {
            return true;
        }
        if !seen.insert(current) {
            continue;
        }
        if let Some(nexts) = adjacency.get(current) {
            for next in nexts {
                queue.push(next);
            }
        }
    }
    false
}

/// Iterative three-color cycle search over the combined scheduling relation
/// used by `parallel_exclusive_conflicts`: hard dependencies plus
/// parallel-after ordering. A cycle in either class — or across both —
/// authorizes an impossible serialization schedule, so the closing edge's
/// class selects the reported code.
fn scheduling_cycle(nodes: &[Node]) -> Option<(Code, String, String)> {
    #[derive(Clone, Copy)]
    enum EdgeClass {
        Hard,
        ParallelAfter,
    }

    let mut adjacency: BTreeMap<&str, Vec<(&str, EdgeClass)>> = BTreeMap::new();
    for node in nodes {
        let entry = adjacency.entry(node.node_id.as_str()).or_default();
        entry.extend(node.edges.hard.iter().map(|target| (target.as_str(), EdgeClass::Hard)));
        entry.extend(
            node.edges
                .parallel_after
                .iter()
                .map(|target| (target.as_str(), EdgeClass::ParallelAfter)),
        );
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Grey,
        Black,
    }
    let mut color: BTreeMap<&str, Color> = adjacency.keys().map(|id| (*id, Color::White)).collect();

    for start in adjacency.keys().copied().collect::<Vec<_>>() {
        if color.get(start) != Some(&Color::White) {
            continue;
        }
        color.insert(start, Color::Grey);
        let mut stack = vec![(start, 0usize)];
        while let Some((current, position)) = stack.pop() {
            let nexts = adjacency.get(current).cloned().unwrap_or_default();
            if position < nexts.len() {
                stack.push((current, position + 1));
                let (next, class) = nexts[position];
                match color.get(next).copied().unwrap_or(Color::Black) {
                    Color::White => {
                        color.insert(next, Color::Grey);
                        stack.push((next, 0));
                    }
                    Color::Grey => {
                        let (code, label) = match class {
                            EdgeClass::Hard => (Code::HardDependencyCycle, "hard"),
                            EdgeClass::ParallelAfter => {
                                (Code::ParallelScheduleCycle, "parallel_after")
                            }
                        };
                        return Some((
                            code,
                            next.to_string(),
                            format!("{label} edge back into `{current}`"),
                        ));
                    }
                    Color::Black => {}
                }
            } else {
                color.insert(current, Color::Black);
            }
        }
    }
    None
}

fn normalize(graph: &ProgrammeGraph) -> NormalizedGraph {
    let mut nodes: Vec<NormalizedNode> = graph
        .nodes
        .iter()
        .map(|node| NormalizedNode {
            node_id: node.node_id.clone(),
            issue: node.issue,
            controller: node.controller.clone(),
            rail: node.rail.clone(),
            phase: node.phase.clone(),
            kind: node.kind.as_str(),
            buildable: node.buildable,
            observation_optional: node.observation_optional,
            edges: NormalizedEdges {
                hard: node.edges.hard.iter().cloned().collect(),
                evidence: node.edges.evidence.iter().cloned().collect(),
                optional: node.edges.optional.iter().cloned().collect(),
                parallel_after: node.edges.parallel_after.iter().cloned().collect(),
                fan_in: node.edges.fan_in.iter().cloned().collect(),
            },
            operation_profile: node.operation_profile.clone(),
            evidence_profile: node.evidence_profile.clone(),
            authority_inputs: node.authority_inputs.iter().cloned().collect(),
            authority_outputs: node
                .authority_outputs
                .iter()
                .map(|output| {
                    (
                        output.key.clone(),
                        match output.mode {
                            AuthorityMode::Exclusive => "exclusive",
                            AuthorityMode::Shared => "shared",
                        },
                    )
                })
                .collect(),
            conflicts: NormalizedConflicts {
                exclusive: node.conflicts.exclusive.iter().cloned().collect(),
                shared: node.conflicts.shared.iter().cloned().collect(),
            },
            claim_ceiling: node.claim_ceiling.clone(),
            non_claims: node.non_claims.iter().cloned().collect(),
            first_falsifier: node.first_falsifier.clone(),
            artifacts: node
                .artifacts
                .iter()
                .map(|artifact| (artifact.id.clone(), artifact.owner.clone()))
                .collect(),
            predecessor_exit: node.predecessor_exit.as_ref().map(|exit_condition| {
                NormalizedPredecessorExit {
                    predecessor: exit_condition.predecessor.iter().cloned().collect(),
                    consumers: exit_condition.consumers.iter().cloned().collect(),
                    exit_condition: exit_condition.exit_condition.clone(),
                }
            }),
            handoff: node.handoff.clone(),
            terminal_relation: node.terminal_relation.as_str(),
        })
        .collect();
    nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    NormalizedGraph {
        schema_version: GRAPH_SCHEMA,
        programme_issue: graph.programme_issue,
        rails: graph.rails.iter().cloned().collect(),
        operation_profiles: graph.operation_profiles.iter().cloned().collect(),
        evidence_profiles: graph.evidence_profiles.iter().cloned().collect(),
        first_falsifiers: graph
            .first_falsifiers
            .iter()
            .map(|falsifier| (falsifier.id.clone(), falsifier.statement.clone()))
            .collect(),
        nodes,
    }
}

/// Canonical committed bytes: pretty JSON plus exactly one trailing newline,
/// so `normalized-manifest > file` reproduces the projection byte-for-byte.
fn normalized_bytes(graph: &ProgrammeGraph) -> Vec<u8> {
    let mut bytes =
        serde_json::to_string_pretty(&normalize(graph)).unwrap_or_default().into_bytes();
    bytes.push(b'\n');
    bytes
}

fn normalized_json(graph: &ProgrammeGraph) -> String {
    String::from_utf8(normalized_bytes(graph)).unwrap_or_default()
}

enum FixtureError {
    Rejected(Vec<Violation>),
    Instrument(Box<dyn Error>),
}

/// A pinned-reason fixture accepts only when the mini graph validates clean
/// (`PASS`) or fails with exactly one distinct typed reason.
fn evaluate_fixture(path: &Path) -> Result<(), FixtureError> {
    let raw = read_bounded(path, MAX_FIXTURE_BYTES, "programme fixture")
        .map_err(|error| FixtureError::Instrument(error.into()))?;
    let envelope: FixtureEnvelope =
        serde_json::from_slice(&raw).map_err(|error| FixtureError::Instrument(error.into()))?;
    if envelope.schema_version != FIXTURE_SCHEMA {
        return Err(FixtureError::Instrument(
            io::Error::other(format!(
                "unsupported fixture schema {}; expected {FIXTURE_SCHEMA}",
                envelope.schema_version
            ))
            .into(),
        ));
    }
    let encoded = serde_json::to_vec(&envelope.graph)
        .map_err(|error| FixtureError::Instrument(error.into()))?;
    let outcome =
        parse_graph_document(&encoded).map_err(|error| FixtureError::Instrument(error.into()))?;

    let mut violations = match outcome {
        Ok(graph) => validate(&graph),
        Err(parsed) => parsed,
    };
    violations
        .sort_by(|a, b| (&a.code, &a.subject, &a.detail).cmp(&(&b.code, &b.subject, &b.detail)));

    if envelope.expected_code == "PASS" {
        if violations.is_empty() {
            return Ok(());
        }
        return Err(FixtureError::Rejected(violations));
    }

    let reasons: BTreeSet<&str> = violations.iter().map(|v| v.code.as_str()).collect();
    if reasons.len() == 1 && reasons.contains(&envelope.expected_code.as_str()) {
        return Ok(());
    }
    Err(FixtureError::Rejected(violations))
}

fn collected_fixtures(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    let entries = fs::read_dir(dir)
        .map_err(|error| io::Error::other(format!("reading {}: {error}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| io::Error::other(format!("fixture entry: {error}")))?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "json") {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(io::Error::other(format!(
            "no programme fixtures found under {}",
            dir.display()
        ))
        .into());
    }
    Ok(paths)
}

fn graph_summary(normalized: &NormalizedGraph) -> String {
    let mut by_rail: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let mut buildable = 0usize;
    let mut edge_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &normalized.nodes {
        *by_rail.entry(node.rail.as_str()).or_default() += 1;
        *by_kind.entry(node.kind).or_default() += 1;
        if node.buildable {
            buildable += 1;
        }
        for (class, targets) in [
            ("hard", &node.edges.hard),
            ("evidence", &node.edges.evidence),
            ("optional", &node.edges.optional),
            ("parallel_after", &node.edges.parallel_after),
            ("fan_in", &node.edges.fan_in),
        ] {
            *edge_counts.entry(class).or_default() += targets.len();
        }
    }
    let summary = serde_json::json!({
        "schema_version": normalized.schema_version,
        "programme_issue": normalized.programme_issue,
        "nodes": normalized.nodes.len(),
        "buildable": buildable,
        "by_rail": by_rail,
        "by_kind": by_kind,
        "edges": edge_counts,
        "operation_profiles": normalized.operation_profiles.len(),
        "evidence_profiles": normalized.evidence_profiles.len(),
        "first_falsifiers": normalized.first_falsifiers.len(),
        "node_ids": normalized.nodes.iter().map(|n| n.node_id.as_str()).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&summary).unwrap_or_default()
}

fn render_explain(graph: &ProgrammeGraph, node_id: &str) -> Option<String> {
    let node = graph.nodes.iter().find(|candidate| candidate.node_id == node_id)?;
    let falsifier =
        graph.first_falsifiers.iter().find(|candidate| candidate.id == node.first_falsifier);

    let mut lines = Vec::new();
    lines.push(format!(
        "node {} — {} (rail {}, phase {})",
        node.node_id,
        node.kind.as_str(),
        node.rail,
        node.phase
    ));
    lines.push(format!(
        "issue: {}  controller: {}  buildable: {}  observation_optional: {}",
        node.issue,
        node.controller.as_deref().unwrap_or("<none>"),
        if node.buildable { "yes" } else { "no" },
        if node.observation_optional { "yes" } else { "no" }
    ));
    lines.push(format!("claim ceiling: {}", node.claim_ceiling));
    if node.non_claims.is_empty() {
        lines.push("non-claims: <none declared>".to_string());
    } else {
        lines.push("non-claims:".to_string());
        for claim in &node.non_claims {
            lines.push(format!("  - {claim}"));
        }
    }
    lines.push(format!("authority inputs: {}", join_sorted(&node.authority_inputs)));
    lines.push(format!(
        "authority outputs: {}",
        node.authority_outputs
            .iter()
            .map(|output| format!(
                "{} [{}]",
                output.key,
                match output.mode {
                    AuthorityMode::Exclusive => "exclusive",
                    AuthorityMode::Shared => "shared",
                }
            ))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    lines.push(format!(
        "conflicts exclusive: {}  shared: {}",
        join_sorted(&node.conflicts.exclusive),
        join_sorted(&node.conflicts.shared)
    ));
    lines.push("edges:".to_string());
    lines.push(format!("  hard:           {}", join_sorted(&node.edges.hard)));
    lines.push(format!("  evidence:       {}", join_sorted(&node.edges.evidence)));
    lines.push(format!("  optional:       {}", join_sorted(&node.edges.optional)));
    lines.push(format!("  parallel_after: {}", join_sorted(&node.edges.parallel_after)));
    lines.push(format!("  fan_in:         {}", join_sorted(&node.edges.fan_in)));
    if node.artifacts.is_empty() {
        lines.push("artifacts: <none declared>".to_string());
    } else {
        lines.push("artifacts:".to_string());
        for artifact in &node.artifacts {
            lines.push(format!("  {} (owner {})", artifact.id, artifact.owner));
        }
    }
    match falsifier {
        Some(falsifier) => {
            lines.push(format!("first falsifier {}: {}", falsifier.id, falsifier.statement))
        }
        None => lines.push(format!("first falsifier {}: <unregistered>", node.first_falsifier)),
    }
    match &node.predecessor_exit {
        Some(predecessor_exit) => lines.push(format!(
            "predecessor exit: {{{}}} consumers {{{}}} exit when {}",
            join_sorted(&predecessor_exit.predecessor),
            join_sorted(&predecessor_exit.consumers),
            predecessor_exit.exit_condition
        )),
        None => lines.push("predecessor exit: <none>".to_string()),
    }
    lines.push(format!("permitted terminal relation: {}", node.terminal_relation.as_str()));
    lines.push(format!("downstream handoff: {}", node.handoff.as_deref().unwrap_or("<none>")));
    lines.push("(current readiness intentionally absent from the stable graph)".to_string());
    Some(lines.join("\n"))
}

fn join_sorted(values: &[String]) -> String {
    let mut sorted: Vec<&str> = values.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    if sorted.is_empty() { "<none>".to_string() } else { sorted.join(", ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
    }

    fn canonical_manifest_path() -> Result<PathBuf, Box<dyn Error>> {
        Ok(repo_root()?.join(DEFAULT_MANIFEST))
    }

    fn load_canonical_graph() -> Result<ProgrammeGraph, Box<dyn Error>> {
        let raw = fs::read(canonical_manifest_path()?)?;
        match parse_graph_document(&raw) {
            Ok(Ok(graph)) => Ok(graph),
            Ok(Err(violations)) => Err(io::Error::other(format!(
                "canonical graph rejected: {:?}",
                violations.iter().map(|violation| violation.code.as_str()).collect::<Vec<_>>()
            ))
            .into()),
            Err(error) => Err(error.into()),
        }
    }

    fn fixture_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        Ok(repo_root()?.join(DEFAULT_FIXTURES_DIR).join(name))
    }

    fn describe_fixture_error(error: &FixtureError) -> String {
        match error {
            FixtureError::Rejected(violations) => format!(
                "rejected with {:?}",
                violations.iter().map(|violation| violation.code.as_str()).collect::<Vec<_>>()
            ),
            FixtureError::Instrument(error) => format!("instrument failure: {error}"),
        }
    }

    const INVALID_FIXTURES: [(&str, &str); 25] = [
        ("invalid-artifact-owner-foreign.json", "ARTIFACT_OWNER_MISMATCH"),
        ("invalid-artifact-unowned.json", "ARTIFACT_OWNER_MISMATCH"),
        ("invalid-authority-input-unrelated.json", "AUTHORITY_INPUT_UNRELATED"),
        ("invalid-consumer-before-accepted-store.json", "CONSUMER_WITHOUT_ACCEPTED_STORE"),
        ("invalid-controller-buildable.json", "CONTROLLER_MARKED_BUILDABLE"),
        ("invalid-controller-kind-mismatch.json", "CONTROLLER_KIND_MISMATCH"),
        ("invalid-current-state-leak.json", "STATE_LEAKAGE"),
        ("invalid-duplicate-falsifier-reversed-order.json", "DUPLICATE_FALSIFIER_ID"),
        ("invalid-duplicate-node-id.json", "DUPLICATE_NODE_ID"),
        ("invalid-fanin-denominator-missing-consumer.json", "FANIN_DENOMINATOR_INCOMPLETE"),
        ("invalid-fanin-denominator-split.json", "FANIN_DENOMINATOR_INCOMPLETE"),
        ("invalid-hard-cycle.json", "HARD_DEPENDENCY_CYCLE"),
        ("invalid-leaf-missing-ceiling.json", "LEAF_MISSING_CLAIM_CEILING"),
        ("invalid-leaf-missing-falsifier.json", "LEAF_MISSING_FIRST_FALSIFIER"),
        ("invalid-live-without-advisory.json", "LIVE_ENFORCEMENT_BEFORE_ADVISORY_AUTHORITY"),
        ("invalid-multi-owner-authority.json", "AUTHORITY_OUTPUT_MULTI_OWNER"),
        ("invalid-optional-live-hardened.json", "OPTIONAL_EDGE_PROMOTED_HARD"),
        ("invalid-parallel-after-cycle.json", "PARALLEL_SCHEDULE_CYCLE"),
        ("invalid-parallel-shared-catalog-writers.json", "PARALLEL_EXCLUSIVE_CONFLICT"),
        ("invalid-retirement-predecessor-unresolved.json", "PREDECESSOR_IDENTITY_UNRESOLVED"),
        ("invalid-retirement-without-exit.json", "RETIREMENT_WITHOUT_PREDECESSOR_EXIT"),
        ("invalid-unknown-edge-target.json", "UNKNOWN_EDGE_TARGET"),
        ("invalid-unknown-profile.json", "UNKNOWN_PROFILE"),
        ("invalid-unknown-schema-field.json", "SCHEMA_REJECTION"),
        ("invalid-unowned-authority-input.json", "AUTHORITY_INPUT_UNOWNED"),
    ];

    /// External pin: each compile-time row must equal the envelope's own
    /// expectation, otherwise updating a fixture's `expected_code` to whatever
    /// the validator currently emits would silently pass this inventory.
    #[test]
    fn external_fixture_pins_match_envelope_expectations() -> Result<(), Box<dyn Error>> {
        for (name, pinned) in INVALID_FIXTURES {
            let raw = fs::read(fixture_path(name)?)?;
            let envelope: FixtureEnvelope = serde_json::from_slice(&raw)?;
            assert_eq!(envelope.expected_code, pinned, "external pin drift for fixture {name}");
        }
        Ok(())
    }

    #[test]
    fn every_shift_left_fixture_rejects_for_one_exact_typed_reason() -> Result<(), Box<dyn Error>> {
        for (name, _) in INVALID_FIXTURES {
            evaluate_fixture(&fixture_path(name)?)
                .map_err(|error| format!("fixture {name}: {}", describe_fixture_error(&error)))?;
        }
        Ok(())
    }

    #[test]
    fn positive_control_mini_graph_is_accepted() -> Result<(), Box<dyn Error>> {
        assert!(evaluate_fixture(&fixture_path("valid-mini-graph.json")?).is_ok());
        Ok(())
    }

    #[test]
    fn canonical_stable_programme_graph_validates_clean() -> Result<(), Box<dyn Error>> {
        let graph = load_canonical_graph()?;
        let violations = validate(&graph);
        assert!(violations.is_empty(), "canonical graph must validate clean, got {violations:?}");
        Ok(())
    }

    #[test]
    fn committed_normalized_projection_is_byte_identical() -> Result<(), Box<dyn Error>> {
        let graph = load_canonical_graph()?;
        let committed = fs::read(repo_root()?.join(DEFAULT_GENERATED))?;
        assert_eq!(
            committed,
            normalized_bytes(&graph),
            "regenerated normalized projection drifted from committed bytes"
        );
        Ok(())
    }

    #[test]
    fn normalization_survives_document_ordering_changes() -> Result<(), Box<dyn Error>> {
        let graph = load_canonical_graph()?;
        let baseline = normalized_json(&graph);

        let mut shuffled = clone_graph(&graph);
        shuffled.nodes.reverse();
        for node in &mut shuffled.nodes {
            node.edges.hard.reverse();
            node.authority_inputs.reverse();
            node.conflicts.exclusive.reverse();
        }
        shuffled.rails.reverse();
        shuffled.operation_profiles.reverse();
        assert_eq!(
            baseline,
            normalized_json(&shuffled),
            "input ordering must not move normalized bytes"
        );
        Ok(())
    }

    fn clone_graph(graph: &ProgrammeGraph) -> ProgrammeGraph {
        graph.clone()
    }

    #[test]
    fn stable_bytes_ignore_current_tree_movement() -> Result<(), Box<dyn Error>> {
        let graph = load_canonical_graph()?;
        let baseline = normalized_json(&graph);

        // A decoy current-tree fixture carrying mutated states lives beside
        // the manifest; the stable graph must not read any of it.
        let decoy_dir = tempfile::tempdir()?;
        let decoy = decoy_dir.path().join("codex-train-copy.v1.json");
        fs::write(
            &decoy,
            r#"{"schema_version":"zed_codex_train.v1","stages":[{"id":"P00","state":"ready"}]}"#,
        )?;
        assert_eq!(baseline, normalized_json(&graph));
        Ok(())
    }

    #[test]
    fn malformed_input_fails_closed_instead_of_resolving_to_a_valid_graph() {
        let truncated: &[u8] = b"{\"schema_version\": \"authority_transfer_pro";
        assert!(
            parse_graph_document(truncated).is_err(),
            "syntactic breakage must be an instrument error, never a valid graph"
        );
    }

    /// Direct CLI control: syntactically broken JSON stays an instrument
    /// failure (exit 3) and never becomes a typed rejection.
    #[test]
    fn read_surface_classifies_malformed_json_as_instrument_failure() -> Result<(), Box<dyn Error>>
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("malformed.v1.json");
        fs::write(&path, b"{\"schema_version\": \"authority_transfer_pro")?;
        match load_graph_or_reject(&path) {
            Err(LoadFailure::Instrument(_)) => Ok(()),
            Err(LoadFailure::Rejected(violations)) => Err(io::Error::other(format!(
                "malformed JSON must not classify as rejection: {:?}",
                violations.iter().map(|v| v.code.as_str()).collect::<Vec<_>>()
            ))
            .into()),
            Ok(_) => Err(io::Error::other("malformed JSON resolved to a valid graph").into()),
        }
    }

    /// Direct CLI control: a well-formed document that violates the schema is
    /// a typed exit-2 rejection on the read surfaces, never an instrument
    /// failure.
    #[test]
    fn read_surface_classifies_well_formed_rejection_as_typed() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("wrong-schema.v1.json");
        fs::write(&path, br#"{"schema_version": "authority_transfer_programme_fixture.v1"}"#)?;
        match load_graph_or_reject(&path) {
            Err(LoadFailure::Rejected(violations)) => {
                assert_eq!(violations.len(), 1);
                assert_eq!(
                    violations[0].code.as_str(),
                    "SCHEMA_REJECTION",
                    "well-formed wrong-schema documents must reject typed"
                );
                Ok(())
            }
            Err(LoadFailure::Instrument(error)) => {
                Err(io::Error::other(format!("typed rejection erased as instrument: {error}"))
                    .into())
            }
            Ok(_) => Err(io::Error::other("schema-violating document accepted").into()),
        }
    }

    #[test]
    fn explain_reports_structure_without_readiness() -> Result<(), Box<dyn Error>> {
        let graph = load_canonical_graph()?;
        let node_id = graph
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .next()
            .ok_or_else(|| io::Error::other("canonical graph has no nodes"))?;
        let rendered = render_explain(&graph, &node_id)
            .ok_or_else(|| io::Error::other("explain target missing"))?;

        for label in [
            "claim ceiling:",
            "authority inputs:",
            "authority outputs:",
            "conflicts exclusive:",
            "edges:",
            "artifacts",
            "first falsifier",
            "predecessor exit",
            "permitted terminal relation:",
            "downstream handoff:",
        ] {
            assert!(rendered.contains(label), "explain lacks `{label}`");
        }
        assert!(
            !rendered.contains("readiness:") && !rendered.contains("ready:"),
            "explain must not report readiness state"
        );
        Ok(())
    }

    #[test]
    fn unknown_explain_target_is_a_rejection_not_a_readiness_report() -> Result<(), Box<dyn Error>>
    {
        let graph = load_canonical_graph()?;
        assert!(render_explain(&graph, "GHOST").is_none());
        Ok(())
    }
}
