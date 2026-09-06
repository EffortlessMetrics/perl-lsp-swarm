//! `cargo xtask integration emacs train packet|review-packet|
//! reconcile-packet|packets --check` — the E06 actor-packet adapter of the
//! Emacs support train (#11719).
//!
//! This module is an **adapter, not an ontology**: it joins the stable
//! `emacs_train.v1` node (E01 #10918), the checked spec disposition (E02
//! #11717), the exact-tree context packet (E04 #11718 via the CTXENG engine
//! #11756) and the E01R revision frontier (#11770) into
//! `agent_implementation_packet.v1` (#10872) and `agent_review_packet.v1`
//! (#10881) documents, then renders them exclusively through the shared
//! fail-closed validators and projections. It never defines an Emacs-local
//! packet schema, never evaluates readiness beyond its checked inputs, never
//! reads or writes GitHub, never invokes models, and never schedules agents.
//!
//! Fail-closed law (the #11719 audit): missing required input blocks packet
//! eligibility with a typed refusal naming the exact missing input, rather
//! than generating plausible prose. Refusals print one diagnostic line and
//! exit `3`; instrument/law failures exit `1`.
//!
//! Packet instances are runtime-local outputs (stdout only). Nothing here is
//! tracked workflow state.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::tasks::agent_implementation_packet::{
    PacketProjection, render_to_string as render_builder_packet,
};
use crate::tasks::agent_review_packet::{
    ReviewProjection, render_to_string as render_review_packet,
};
use crate::tasks::emacs_train_context::model::{NodeContextPacket, TrainNode};
use crate::tasks::emacs_train_context::resolve::{
    EngineInputs, load_inputs_with_git, resolve_spec,
};
use crate::tasks::emacs_train_specs::{DEFAULT_LEDGER_PATH as SPECS_LEDGER_PATH, SpecsLedger};
use crate::tasks::module_train_live;
use crate::utils::project_root;

/// Shared builder-packet contract this adapter projects into (#10872).
const BUILDER_CONTRACT: &str = "agent_implementation_packet.v1";
/// Shared review-packet contract this adapter projects into (#10881).
const REVIEW_CONTRACT: &str = "agent_review_packet.v1";

/// Dispositions that permit a bounded coding packet (E02 vocabulary).
const BUILDER_DISPOSITIONS: [&str; 3] =
    ["SPEC_COMPILED", "EXISTING_CONTRACT_SUFFICIENT", "ISSUE_PLAN_SUFFICIENT"];

/// Dispositions that block every coding packet as an unproven/exit state.
const BLOCKING_DISPOSITIONS: [&str; 3] =
    ["NOT_PROVEN", "RETURN_TO_ISSUE", "HISTORICAL_OR_SUPERSEDED"];

/// Train roles that are direct bounded builder work for the bounded profile.
const BOUNDED_ROLES: [&str; 3] = ["implementation", "dogfood", "packet_adapter"];
/// Train roles the strong profile may additionally receive (one bounded
/// judgment / ISSUE_PLAN_SUFFICIENT leaf).
const STRONG_EXTRA_ROLES: [&str; 1] = ["specification"];

/// The fixed independent reviewer challenge questions of #11719, appended to
/// every Emacs review packet. They challenge; they never repeat the builder
/// summary.
const REVIEWER_CHALLENGE_QUESTIONS: &[(&str, &str)] = &[
    ("Q_one_authority", "Did the PR move exactly one declared authority/proposition?"),
    ("Q_actual_consumer", "Did it wire the actual consumer or only add substrate/helper text?"),
    (
        "Q_cross_satisfaction",
        "Can another client/server/version/root/generation satisfy the oracle?",
    ),
    ("Q_protocol_vs_host", "Did protocol/profile evidence get mistaken for host-visible behavior?"),
    ("Q_status0_lifecycle", "Did status-0 or force-kill get mistaken for clean lifecycle proof?"),
    (
        "Q_leaf_authority_reimplementation",
        "Did any leaf reimplement runner/subject/observation/receipt/profile/root/registry authority?",
    ),
    (
        "Q_evidence_widening",
        "Did local/manual/source/Linux evidence widen into public/stock/released/all-platform claims?",
    ),
    (
        "Q_evidence_substitution",
        "Did missing/instrument/partial evidence become unsupported/pass/absence?",
    ),
    ("Q_generated_outrun", "Did generated docs/registry state outrun the canonical input?"),
    ("Q_next_node_absorbed", "Did the next train node get absorbed?"),
    (
        "Q_defect_transfer",
        "Did a product defect get repaired inside a proof/certification leaf instead of transferred?",
    ),
];

/// The typed refusal of packet eligibility: the exact missing or blocking
/// input, never plausible prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub node_id: String,
    pub profile: String,
    pub code: &'static str,
    pub detail: String,
}

impl Refusal {
    fn new(node_id: &str, profile: &str, code: &'static str, detail: String) -> Self {
        Self { node_id: node_id.to_string(), profile: profile.to_string(), code, detail }
    }

    /// One-line typed diagnostic (stderr; exit code 3).
    fn line(&self) -> String {
        format!(
            "EMU_PACKET_REFUSED node={} profile={} code={} detail={}",
            self.node_id, self.profile, self.code, self.detail
        )
    }
}

/// Everything the adapter reads, loaded once: the context-engine inputs plus
/// the E02 checked disposition ledger and its digest.
pub struct AdapterInputs {
    pub engine: EngineInputs,
    pub specs: SpecsLedger,
    pub specs_digest: String,
}

/// Optional explicitly supplied live-candidate observation (#10930 shapes;
/// the adapter never observes GitHub itself).
#[derive(Debug, Clone)]
pub struct LiveObservation {
    pub candidate_state: String,
    pub digest: String,
    pub candidate_identity: Option<String>,
    pub collision_state: Option<String>,
}

/// Explicitly supplied review-candidate facts. The adapter is offline: exact
/// base/head/diff identities and the negative-control audit rows must come
/// from the caller; none of them is ever invented.
pub struct ReviewFacts {
    pub base: String,
    pub head: String,
    pub diff: String,
    /// falsifier_id -> criterion -> {status, evidence}
    pub controls: BTreeMap<String, BTreeMap<String, Value>>,
}

pub fn load_adapter_inputs(root: &Path) -> Result<AdapterInputs> {
    let engine = load_adapter_engine_inputs(root)?;
    complete_adapter_inputs(root, engine)
}

pub(crate) fn load_adapter_engine_inputs(root: &Path) -> Result<EngineInputs> {
    load_inputs_with_git(root, None)
        .with_context(|| "loading the E01/E01R/E04 context-engine inputs for the packet adapter")
}

pub(crate) fn complete_adapter_inputs(root: &Path, engine: EngineInputs) -> Result<AdapterInputs> {
    let ledger_path = root.join(SPECS_LEDGER_PATH);
    let ledger_bytes = std::fs::read(&ledger_path).with_context(|| {
        format!("reading the E02 checked disposition ledger {}", ledger_path.display())
    })?;
    let digest = sha256_hex(&ledger_bytes);
    let specs: SpecsLedger = serde_json::from_slice(&ledger_bytes).with_context(|| {
        format!("parsing the E02 checked disposition ledger {}", ledger_path.display())
    })?;
    ensure!(
        specs.schema == crate::tasks::emacs_train_specs::LEDGER_SCHEMA,
        "E02 ledger {} declares schema {:?}, expected emacs_train_specs.v1",
        ledger_path.display(),
        specs.schema
    );
    ensure!(
        specs.schema_version == 1,
        "E02 ledger {} declares schema_version {}, expected 1",
        ledger_path.display(),
        specs.schema_version
    );
    // Structural E02 laws the adapter depends on when trusting a record to
    // select a profile: no duplicate node records, every record matches a
    // manifest node's issue, and the ledger covers the manifest denominator.
    // (Full canonical-byte law enforcement stays owned by the E02 checker.)
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for record in &specs.records {
        ensure!(
            seen.insert(record.node_id.as_str()),
            "E02 ledger {} carries duplicate records for node {}",
            ledger_path.display(),
            record.node_id
        );
        let node = engine.manifest.nodes.iter().find(|node| node.node_id == record.node_id);
        let node = node.ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "E02 ledger {} carries a record for node {} which is not in the manifest",
                ledger_path.display(),
                record.node_id
            )
        })?;
        ensure!(
            node.issue == record.issue,
            "E02 ledger {} record for node {} declares issue {} but the manifest declares {}",
            ledger_path.display(),
            record.node_id,
            record.issue,
            node.issue
        );
    }
    for node in &engine.manifest.nodes {
        ensure!(
            specs.records.iter().any(|record| record.node_id == node.node_id),
            "E02 ledger {} does not cover manifest node {} (#{}); the denominator is incomplete",
            ledger_path.display(),
            node.node_id,
            node.issue
        );
    }
    Ok(AdapterInputs { engine, specs, specs_digest: digest })
}

#[derive(Debug, Subcommand)]
pub enum EmacsTrainPacketCommand {
    /// Render the bounded builder packet for one node and actor profile.
    /// Fail-closed: any missing required input refuses with a typed reason
    /// (exit 3) instead of rendering plausible prose.
    Packet {
        /// Node id, alias or issue number.
        node: String,
        /// Actor profile (E06 vocabulary).
        #[arg(long, default_value = "coding_agent_bounded")]
        profile: String,
        /// Output format: machine | markdown | compact.
        #[arg(long, default_value = "machine")]
        format: String,
        /// Optional explicitly supplied live observation JSON
        /// ({candidate_state, digest, candidate_identity?, collision_state?}).
        #[arg(long)]
        live_observation: Option<PathBuf>,
    },
    /// Render the independent reviewer packet (#10881) for one node. The
    /// adapter is offline: exact candidate identities (base/head/diff) and
    /// the negative-control audit rows must be supplied explicitly.
    ReviewPacket {
        /// Node id, alias or issue number.
        node: String,
        /// Exact base the candidate is reviewed against.
        #[arg(long)]
        base: Option<String>,
        /// Exact candidate head under review.
        #[arg(long)]
        head: Option<String>,
        /// Exact diff identity (digest) of the reviewed changes.
        #[arg(long)]
        diff: Option<String>,
        /// Negative-control audit JSON: falsifier_id -> criterion ->
        /// {status: established, evidence}.
        #[arg(long)]
        controls: Option<PathBuf>,
        /// Output format: machine | markdown | compact.
        #[arg(long, default_value = "machine")]
        format: String,
    },
    /// Render the reconciliation packet for one node. Requires explicitly
    /// supplied exact candidate facts; no live observation means a typed
    /// refusal, never an assumed vacancy.
    ReconcilePacket {
        /// Node id, alias or issue number.
        node: String,
        /// Exact candidate facts JSON: an array of {identity, state, facts}.
        #[arg(long)]
        candidates: Option<PathBuf>,
        /// Output format: machine | markdown | compact.
        #[arg(long, default_value = "machine")]
        format: String,
    },
    /// Validate the whole packet denominator: every stable node either
    /// renders at least one valid shared-contract packet or refuses with a
    /// typed reason, and every render is byte-deterministic.
    Packets {
        /// Run the denominator validation.
        #[arg(long)]
        check: bool,
        /// Optional explicitly supplied live observation JSON applied to every
        /// node. Without it, coding profiles refuse with a typed
        /// `NO_LIVE_OBSERVATION`: the denominator reports eligibility up to the
        /// live boundary rather than assuming every claim is vacant.
        #[arg(long)]
        live_observation: Option<PathBuf>,
    },
}

pub fn run(command: EmacsTrainPacketCommand) -> Result<()> {
    let root = project_root()
        .with_context(|| "locating the repository root for the emacs train packet adapter")?;
    match command {
        EmacsTrainPacketCommand::Packet { node, profile, format, live_observation } => {
            let inputs = load_adapter_inputs(&root)?;
            let live = match &live_observation {
                Some(path) => Some(parse_live_observation(path)?),
                None => None,
            };
            match compose_builder_packet(&root, &inputs, &node, &profile, live.as_ref()) {
                Ok(doc) => {
                    let rendered = render_with_format(&doc, &format)?;
                    println!("{rendered}");
                    Ok(())
                }
                Err(refusal) => {
                    eprintln!("{}", refusal.line());
                    std::process::exit(3);
                }
            }
        }
        EmacsTrainPacketCommand::ReviewPacket { node, base, head, diff, controls, format } => {
            let inputs = load_adapter_inputs(&root)?;
            let facts = ReviewFacts {
                base: base.unwrap_or_default(),
                head: head.unwrap_or_default(),
                diff: diff.unwrap_or_default(),
                controls: match &controls {
                    Some(path) => parse_controls(path)?,
                    None => BTreeMap::new(),
                },
            };
            match compose_review_packet(&root, &inputs, &node, &facts) {
                Ok(doc) => {
                    let rendered = render_review_with_format(&doc, &format)?;
                    println!("{rendered}");
                    Ok(())
                }
                Err(refusal) => {
                    eprintln!("{}", refusal.line());
                    std::process::exit(3);
                }
            }
        }
        EmacsTrainPacketCommand::ReconcilePacket { node, candidates, format } => {
            let inputs = load_adapter_inputs(&root)?;
            let candidates = match &candidates {
                Some(path) => Some(parse_candidates(path)?),
                None => None,
            };
            match compose_reconcile_packet(&root, &inputs, &node, candidates.as_ref()) {
                Ok(doc) => {
                    let rendered = render_with_format(&doc, &format)?;
                    println!("{rendered}");
                    Ok(())
                }
                Err(refusal) => {
                    eprintln!("{}", refusal.line());
                    std::process::exit(3);
                }
            }
        }
        EmacsTrainPacketCommand::Packets { check, live_observation } => {
            if !check {
                bail!(
                    "refusing an implicit denominator run: pass --check to validate that every \
                     stable node renders a shared-contract packet or refuses with a typed reason"
                );
            }
            let live = match &live_observation {
                Some(path) => Some(parse_live_observation(path)?),
                None => None,
            };
            run_packets_check(&root, live.as_ref())
        }
    }
}

// ---------------------------------------------------------------------------
// Builder packet composition (#10872 projection).
// ---------------------------------------------------------------------------

/// Compose the builder packet for one node and profile. Every join input is
/// re-derived from the exact tree; a missing or blocking input is a typed
/// `Refusal`, never a degraded packet.
pub fn compose_builder_packet(
    root: &Path,
    inputs: &AdapterInputs,
    subject: &str,
    profile: &str,
    live: Option<&LiveObservation>,
) -> Result<Value, Refusal> {
    compose_packet_document(root, inputs, subject, profile, live, LiveGate::Enforced)
}

/// Whether a composition admits a repository writer and must therefore carry a
/// current live candidate observation.
///
/// `Anchored` is used only by the review and reconciliation routes: both
/// compose a builder document to anchor the claim they challenge, and neither
/// emits a coding packet or a repository write boundary of its own. Admitting
/// them through the live gate would refuse a read-only review for the absence
/// of an observation it never acts on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LiveGate {
    Enforced,
    Anchored,
}

fn compose_packet_document(
    root: &Path,
    inputs: &AdapterInputs,
    subject: &str,
    profile: &str,
    live: Option<&LiveObservation>,
    live_gate: LiveGate,
) -> Result<Value, Refusal> {
    let node = resolve_train_node(inputs, subject).map_err(|error| {
        Refusal::new(subject, profile, "NODE_RESOLUTION_FAILED", error.to_string())
    })?;
    let disposition_record =
        inputs.specs.records.iter().find(|record| record.node_id == node.node_id);
    let disposition = disposition_record
        .map(|record| record.disposition.to_string())
        .unwrap_or_else(|| node.spec.disposition.clone());
    let is_coding = profile == "coding_agent_bounded" || profile == "coding_agent_strong";

    // Spec plane: the disposition selects the profile (conditional by
    // construction); missing or non-builder dispositions block eligibility.
    if disposition_record.is_none() {
        return Err(Refusal::new(
            &node.node_id,
            profile,
            "MISSING_SPEC_DISPOSITION",
            format!(
                "node {} (#{}) has no record in the E02 checked disposition ledger {}; \
                 missing required input blocks packet eligibility",
                node.node_id, node.issue, SPECS_LEDGER_PATH
            ),
        ));
    }
    let allowed_profiles = allowed_profiles_for(node, &disposition);
    if BLOCKING_DISPOSITIONS.contains(&disposition.as_str())
        && (is_coding || profile == "maintainer_external_action")
    {
        return Err(Refusal::new(
            &node.node_id,
            profile,
            "SPEC_DISPOSITION_NOT_BUILDER",
            format!(
                "spec disposition {disposition} does not permit a coding packet; the premise \
                 returns to its owner (#{})",
                node.issue
            ),
        ));
    }
    if !allowed_profiles.contains(&profile) {
        return Err(Refusal::new(
            &node.node_id,
            profile,
            "PROFILE_NOT_PERMITTED",
            format!(
                "train role {} with spec disposition {} permits profiles [{}]; the requested \
                 profile is not permitted for this node",
                node.train_role,
                disposition,
                allowed_profiles.join(", ")
            ),
        ));
    }

    // Context plane: exact-tree context must be complete and unambiguous.
    let resolution = resolve_spec(root, &inputs.engine, &node.node_id).map_err(|error| {
        Refusal::new(&node.node_id, profile, "CONTEXT_RESOLUTION_FAILED", error.to_string())
    })?;
    if resolution.is_gap() {
        let gap = resolution
            .packet()
            .gaps
            .first()
            .map(|gap| format!("{} (owner #{})", gap.reason, gap.owner_issue))
            .unwrap_or_else(|| "unspecified mapping gap".to_string());
        return Err(Refusal::new(
            &node.node_id,
            profile,
            "CONTEXT_MAPPING_GAP",
            format!(
                "the E04 exact-tree context carries a typed mapping blocker: {gap}; packet \
                 eligibility is blocked"
            ),
        ));
    }
    let context = resolution.packet();

    // Frontier: hard dependencies must be current on the offline spec plane;
    // external hard targets are honestly unverifiable offline.
    let blocking_edges = hard_dependency_blockers(root, inputs, node);
    if is_coding && !blocking_edges.is_empty() {
        let edges = blocking_edges
            .iter()
            .map(|edge| format!("{} ({})", edge.0, edge.1))
            .collect::<Vec<_>>();
        return Err(Refusal::new(
            &node.node_id,
            profile,
            "HARD_DEPENDENCY_NOT_CURRENT",
            format!(
                "hard implementation dependencies are not current: [{}]; landing currency \
                 beyond the offline spec plane requires #10923/#10930",
                edges.join("; ")
            ),
        ));
    }

    // A coding packet admits a repository writer, so it requires a current
    // candidate observation. #11719: "No live observation means no coding
    // packet assuming vacancy." `not_observed` records that nobody looked --
    // it is not evidence that the claim is free.
    if is_coding && live_gate == LiveGate::Enforced {
        let observed = live.filter(|live| live.candidate_state == "observed");
        if observed.is_none() {
            let detail = match live {
                None => "no live candidate observation was supplied (--live-observation); a \
                         coding packet must not assume the claim is vacant"
                    .to_string(),
                Some(live) => format!(
                    "the supplied live observation is {}; only an explicit complete \
                     `observed` observation establishes whether a candidate exists, and \
                     absence of knowledge is never vacancy",
                    live.candidate_state
                ),
            };
            return Err(Refusal::new(
                &node.node_id,
                profile,
                "NO_LIVE_OBSERVATION",
                format!("{detail} (#10930 unlanded)"),
            ));
        }
    }

    let observed_tree = context.binding.git_commit.clone();
    let node_kind = node_kind_of(node);
    let write_boundary = if is_coding { "repository_candidate_branch" } else { "none" };

    let implementation_paths: Vec<String> =
        context.components.iter().map(|component| component.path.clone()).collect();
    let write_paths: Vec<String> = if context.write_set.is_empty() {
        implementation_paths.clone()
    } else {
        context.write_set.clone()
    };
    if write_boundary == "repository_candidate_branch" && write_paths.is_empty() {
        return Err(Refusal::new(
            &node.node_id,
            profile,
            "NO_WRITE_SURFACE",
            "the node has no exact-tree write surface; a repository-writing actor cannot be \
             admitted"
                .to_string(),
        ));
    }

    let mut unproven = vec![
        node.rollback.not_proven.clone(),
        "landing currency of hard dependencies beyond the offline spec plane \
         (#10923/#10930 unlanded)"
            .to_string(),
    ];
    if live.is_none() {
        unproven.push(
            "live candidate state beyond this packet is not observed (#10930 unlanded); absence \
             of knowledge is never vacancy"
                .to_string(),
        );
    }

    let mut non_goals = vec![
        "no Emacs-local packet schema: the shared #10872/#10881 contracts are consumed unchanged"
            .to_string(),
        "no readiness evaluation beyond the checked E01/E02/E04/E01R inputs".to_string(),
        "no model invocation, GitHub mutation, or scheduling".to_string(),
    ];
    non_goals.extend(node.forbidden_adjacent_owners.iter().cloned());

    let probe = current_tree_probe(context, &observed_tree, &implementation_paths);

    let doc = json!({
        "schema": BUILDER_CONTRACT,
        "schema_version": 1,
        "packet_id": format!("emacs-train/{}/{}", node.node_id, profile),
        "repository": {
            "name": "perl-lsp-swarm",
            "observed_tree": observed_tree,
        },
        "programme": {
            "name": "emacs-train",
            "manifest": "emacs_train.v1",
            "manifest_version": format!("sha256:{}", short(&inputs.engine.manifest_digest, 12)),
        },
        "work": {
            "owning_issue": format!("#{}", node.issue),
            "node_id": node.node_id,
            "proposition_id": proposition_id(node),
            "profile": profile,
            "profile_conditional": true,
            "profile_decision": {
                "selecting_authority": format!("#11717 checked spec disposition ({})", node.spec.spec_authority),
                "selected_value": disposition,
            },
            "node_kind": node_kind,
            "bounded_leaf_manifest_ref": if node_kind == "bounded_implementation_leaf" {
                Value::Null
            } else {
                json!(format!("emacs_train.v1 node row {} (#{})", node.node_id, node.issue))
            },
            "result_sentence": node.one_pr_outcome,
            "claim_ceiling": node.claim_ceiling,
            "unproven": unproven,
            "non_goals": non_goals,
            "successors": successor_identities(inputs, node),
        },
        "actor": {
            "role": profile,
            "write_boundary": write_boundary,
        },
        "frontier": frontier_object(inputs, node, &blocking_edges),
        "current_tree_probe": probe,
        "live_observation": live.map(live_observation_json),
        "authorities": authorities_object(inputs, node),
        "surfaces": surfaces_object(node, &implementation_paths, context, write_boundary, &write_paths),
        "proof": proof_object(node),
        "verification": verification_object(node, context),
        "delivery": delivery_object(node),
        "stop": stop_object(node),
    });

    // Zero-drift law: the document must satisfy the shared closed contract,
    // validated and rendered only through #10872's own fail-closed layer.
    render_builder_packet(&doc, PacketProjection::Machine)
        .map_err(|error| {
            Refusal::new(
                &node.node_id,
                profile,
                "SHARED_CONTRACT_VALIDATION_FAILED",
                format!("the composed packet violates the shared #10872 contract: {error:#}"),
            )
        })
        .map(|_| remove_null_live_observation(doc))
}

/// Profiles the E01 role + E02 disposition permit for one node. Controller,
/// fan-in, historical and external nodes never receive a coding packet
/// (#11719 falsifier 1).
fn allowed_profiles_for(node: &TrainNode, disposition: &str) -> Vec<&'static str> {
    let mut profiles: Vec<&'static str> = Vec::new();
    let role = node.train_role.as_str();
    let builder_disposition = BUILDER_DISPOSITIONS.contains(&disposition);
    if builder_disposition && BOUNDED_ROLES.contains(&role) {
        profiles.push("coding_agent_bounded");
    }
    if builder_disposition
        && (BOUNDED_ROLES.contains(&role)
            || STRONG_EXTRA_ROLES.contains(&role) && disposition == "ISSUE_PLAN_SUFFICIENT")
    {
        profiles.push("coding_agent_strong");
    }
    if disposition == "EXTERNAL_OR_MANUAL_NO_CODING_SPEC" || node.lane.contains("external") {
        profiles.push("maintainer_external_action");
    }
    profiles.push("read_only_reviewer");
    profiles.push("reconciliation_only");
    profiles
}

fn node_kind_of(node: &TrainNode) -> &'static str {
    match node.train_role.as_str() {
        "controller" => "controller",
        "fan_in" => "fan_in",
        _ => "bounded_implementation_leaf",
    }
}

fn proposition_id(node: &TrainNode) -> String {
    let slug: String =
        node.one_pr_outcome
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
            })
            .collect();
    let compact = slug.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    let compact = if compact.is_empty() { node.node_id.to_lowercase() } else { compact };
    format!("P_{}", truncate_chars(&compact, 48))
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max { value.to_string() } else { value.chars().take(max).collect() }
}

fn short(hex: &str, characters: usize) -> String {
    hex.chars().take(characters).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn resolve_train_node<'a>(inputs: &'a AdapterInputs, subject: &str) -> Result<&'a TrainNode> {
    let trimmed = subject.trim();
    let bare = trimmed.trim_start_matches('#');
    let mut candidates: Vec<&TrainNode> = inputs
        .engine
        .manifest
        .nodes
        .iter()
        .filter(|node| {
            node.node_id.eq_ignore_ascii_case(trimmed)
                || node.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(trimmed))
                || bare.parse::<u64>().map(|issue| node.issue == issue).unwrap_or(false)
        })
        .collect();
    candidates.sort_by_key(|node| node.node_id.clone());
    candidates.dedup_by_key(|node| node.node_id.clone());
    match candidates.as_slice() {
        [node] => Ok(node),
        [] => bail!("subject {subject:?} resolves to no node in emacs_train.v1"),
        many => bail!(
            "subject {subject:?} is ambiguous between {}",
            many.iter()
                .map(|node| format!("{} (#{} node)", node.node_id, node.issue))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Offline hard-dependency currency. A checked disposition alone establishes
/// specability, not landing: only a disposition that declares an already
/// landed contract consumed unchanged (`EXISTING_CONTRACT_SUFFICIENT`) or a
/// full exact-tree context packet for the dependency (its declared surfaces
/// verified on the observed tree by the E04 engine) is offline landing
/// evidence. External targets are honestly unverifiable offline.
fn hard_dependency_blockers(
    root: &Path,
    inputs: &AdapterInputs,
    node: &TrainNode,
) -> Vec<(String, String)> {
    let mut blockers = Vec::new();
    for dependency in &node.dependencies {
        if dependency.class != "hard" {
            continue;
        }
        let target = dependency.target.trim_start_matches('#');
        let target_node = inputs.engine.manifest.nodes.iter().find(|candidate| {
            candidate.node_id.eq_ignore_ascii_case(target) || candidate.issue.to_string() == target
        });
        let Some(target_node) = target_node else {
            blockers.push((
                dependency.target.clone(),
                "external hard dependency: landing currency is not observable offline \
                 (#10923/#10930 unlanded). No input to this adapter can establish it -- \
                 `--live-observation` attests the *candidate*, not a dependency's landing -- \
                 so this blocks until #10923/#10930 provide the instrument"
                    .to_string(),
            ));
            continue;
        };
        let disposition = inputs
            .specs
            .records
            .iter()
            .find(|record| record.node_id == target_node.node_id)
            .map(|record| record.disposition.to_string());
        let Some(disposition) = disposition else {
            blockers.push((
                dependency.target.clone(),
                "no E02 checked disposition record for the dependency node".to_string(),
            ));
            continue;
        };
        if BLOCKING_DISPOSITIONS.contains(&disposition.as_str()) {
            blockers.push((
                dependency.target.clone(),
                format!("checked spec disposition {disposition} does not establish currentness"),
            ));
            continue;
        }
        if disposition == "EXISTING_CONTRACT_SUFFICIENT" {
            // Declared landed contract consumed unchanged: landing evidence.
            continue;
        }
        // A resolvable E04 context proves the dependency's declared surfaces
        // exist on the observed tree. It does NOT prove the prerequisite work
        // finished -- surfaces can exist while the behavior behind them is
        // still being built. Reading a non-gap context as landing evidence is
        // the same error as reading `SPEC_COMPILED`/`ISSUE_PLAN_SUFFICIENT` as
        // currentness, which was already corrected at the disposition layer
        // above; `EXISTING_CONTRACT_SUFFICIENT` remains the only offline
        // disposition that declares landing.
        match resolve_spec(root, &inputs.engine, &target_node.node_id) {
            Ok(resolution) => {
                let detail = if resolution.is_gap() {
                    "and the E04 exact-tree context carries a typed blocker for it"
                } else {
                    "and a resolvable E04 context proves only that its declared surfaces exist \
                     on the observed tree, not that the work landed"
                };
                blockers.push((
                    dependency.target.clone(),
                    format!(
                        "checked spec disposition {disposition} establishes specability, not \
                         landing, {detail}; offline landing currency needs #10923/#10930"
                    ),
                ));
            }
            Err(error) => blockers.push((
                dependency.target.clone(),
                format!("resolving the dependency's exact-tree context failed: {error:#}"),
            )),
        }
    }
    blockers
}

fn frontier_object(
    inputs: &AdapterInputs,
    node: &TrainNode,
    blockers: &[(String, String)],
) -> Value {
    let decision = if blockers.is_empty() { "ready" } else { "blocked" };
    json!({
        "decision": decision,
        "digest": format!(
            "frontier:emacs-train:{}:{}:{}:sha256:{}",
            short(&inputs.engine.manifest_digest, 12),
            decision,
            node.node_id,
            short(&inputs.engine.ledger_digest, 12)
        ),
        "blocking_edges": blockers
            .iter()
            .map(|(edge, reason)| json!({"edge": edge, "reason": reason}))
            .collect::<Vec<_>>(),
    })
}

fn current_tree_probe(
    context: &NodeContextPacket,
    observed_tree: &str,
    implementation_paths: &[String],
) -> Value {
    let subject = implementation_paths
        .first()
        .cloned()
        .unwrap_or_else(|| format!("emacs-train/{}", context.node.node_id));
    let sha = context
        .components
        .first()
        .map(|component| component.sha256.clone())
        .unwrap_or_else(|| context.binding.input_digest.clone());
    json!({
        "subject": subject,
        "result": "present-on-observed-tree",
        "digest": format!("probe:{}:{}:sha256:{}", subject, observed_tree, short(&sha, 16)),
    })
}

fn live_observation_json(live: &LiveObservation) -> Value {
    let mut object = json!({
        "candidate_state": live.candidate_state,
        "digest": live.digest,
    });
    if live.candidate_state == "observed" {
        object["candidate_identity"] = json!(live.candidate_identity.clone().unwrap_or_default());
        if let Some(collision) = &live.collision_state {
            object["collision_state"] = json!(collision);
        }
    }
    object
}

/// `json!` renders `None` as null; the shared contract requires a present
/// live_observation to be well-formed and a bounded_leaf_manifest_ref to be
/// a non-empty string, so absent optionals are removed entirely (absence is
/// the valid form).
fn remove_null_live_observation(mut doc: Value) -> Value {
    if let Some(object) = doc.as_object_mut() {
        if object.get("live_observation").map(Value::is_null).unwrap_or(false) {
            object.remove("live_observation");
        }
        if let Some(work) = object.get_mut("work").and_then(Value::as_object_mut)
            && work.get("bounded_leaf_manifest_ref").map(Value::is_null).unwrap_or(false)
        {
            work.remove("bounded_leaf_manifest_ref");
        }
    }
    doc
}

fn authorities_object(inputs: &AdapterInputs, node: &TrainNode) -> Value {
    let architecture_digest = inputs
        .engine
        .architecture_digests
        .first()
        .map(|digest| digest.sha256.clone())
        .unwrap_or_else(|| inputs.engine.manifest_digest.clone());
    let mut must_not_be_reimplemented = vec![
        json!({
            "ref": "#10872",
            "subject": "shared builder-packet contract this adapter projects into",
            "proof_kind": "external_owner_note",
        }),
        json!({
            "ref": "#10881",
            "subject": "shared adversarial review/closure contracts this adapter projects into",
            "proof_kind": "external_owner_note",
        }),
    ];
    for authority in &node.consumed_authorities {
        if ["#10872", "#10881"].contains(&authority.as_str()) {
            continue;
        }
        must_not_be_reimplemented.push(json!({
            "ref": authority,
            "subject": "consumed authority declared by the stable manifest; consumed, never reimplemented",
            "proof_kind": "external_owner_note",
        }));
    }
    json!({
        "must_be_current": [
            {
                "ref": "#11716",
                "subject": "E00 durable architecture fixing the adapter boundary",
                "proof_kind": "spec_disposition",
                "proof_digest": format!("sha256:{}", short(&architecture_digest, 16)),
            },
            {
                "ref": "#10918",
                "subject": "E01 stable emacs_train.v1 graph",
                "proof_kind": "current_tree_probe",
                "proof_tree": inputs.engine.git_commit,
            },
            {
                "ref": "#11770",
                "subject": "E01R emacs_train_revision.v1 ledger",
                "proof_kind": "frontier_digest",
                "proof_digest": format!("sha256:{}", short(&inputs.engine.ledger_digest, 16)),
            },
            {
                "ref": "#11717",
                "subject": "E02 checked spec disposition ledger",
                "proof_kind": "spec_disposition",
                "proof_digest": format!("sha256:{}", short(&inputs.specs_digest, 16)),
            },
        ],
        "may_be_mined": [],
        "must_not_be_reimplemented": must_not_be_reimplemented,
        "consumer_fan_in": successor_identities(inputs, node)
            .into_iter()
            .map(|successor| json!({
                "ref": successor,
                "subject": "successor node consuming this node's authority",
                "proof_kind": "external_owner_note",
            }))
            .collect::<Vec<_>>(),
        "external_manual_owner": [
            {
                "ref": "maintainer/merger",
                "subject": "review, merge and external-action authority",
                "proof_kind": "external_owner_note",
            },
        ],
    })
}

fn successor_identities(inputs: &AdapterInputs, node: &TrainNode) -> Vec<String> {
    node.successors
        .iter()
        .map(|successor| {
            let bare = successor.trim_start_matches('#');
            inputs
                .engine
                .manifest
                .nodes
                .iter()
                .find(|candidate| {
                    candidate.node_id.eq_ignore_ascii_case(bare)
                        || candidate.issue.to_string() == bare
                })
                .map(|candidate| format!("#{}", candidate.issue))
                .unwrap_or_else(|| successor.clone())
        })
        .collect()
}

fn surfaces_object(
    node: &TrainNode,
    implementation_paths: &[String],
    context: &NodeContextPacket,
    write_boundary: &str,
    write_paths: &[String],
) -> Value {
    let writer_slots = if write_boundary == "repository_candidate_branch" {
        vec![json!({
            "key": node.writer.conflict_key,
            "paths": write_paths,
        })]
    } else {
        Vec::new()
    };
    let docs_fragments: Vec<String> = if node.obligations.docs.starts_with("none") {
        Vec::new()
    } else {
        vec![node.obligations.docs.clone()]
    };
    json!({
        "implementation_paths": implementation_paths,
        "tests_fixtures": context.tests.iter().map(|test| test.path.clone()).collect::<Vec<_>>(),
        "generated_artifacts": context.generated.iter().map(|generated| generated.output.clone()).collect::<Vec<_>>(),
        "docs_fragments": docs_fragments,
        "writer_slots": writer_slots,
        "forbidden_adjacent": node
            .forbidden_adjacent_owners
            .clone()
            .into_iter()
            .chain([
                "schemas/agent_implementation_packet.v1.schema.json".to_string(),
                "schemas/agent_review_packet.v1.schema.json".to_string(),
            ])
            .collect::<Vec<String>>(),
    })
}

fn proof_object(node: &TrainNode) -> Value {
    json!({
        "falsifiers": [
            {"id": "F_first", "stage": "focused", "statement": node.first_falsifier},
            {"id": "F_opposite", "stage": "focused", "statement": node.controls.opposite},
            {"id": "F_stale", "stage": "routed", "statement": node.controls.stale},
            {"id": "F_wrong_subject", "stage": "focused", "statement": node.controls.wrong_subject},
            {"id": "F_fault", "stage": "focused", "statement": node.controls.fault},
        ],
        "positive_discriminator": node.controls.positive,
        "mutation_controls": [
            node.controls.mutation,
            node.controls.wrong_subject,
        ],
        "terminal_outcomes": [
            "DELIVERED_REVIEWABLE_PR",
            "BLOCKED_MISSING_INPUT",
            "NOT_PROVEN",
        ],
        "cleanup_retention": node.rollback.rollback,
    })
}

fn verification_object(node: &TrainNode, context: &NodeContextPacket) -> Value {
    let mut steps = vec![json!({
        "command_id": format!("emacs-train.{}.focused", node.node_id),
        "command": node.proof.focused,
        "scope": "focused_proof",
    })];
    for (index, generated) in context.generated.iter().enumerate() {
        steps.push(json!({
            "command_id": format!("emacs-train.{}.generated.{}", node.node_id, index),
            "command": generated.stale_check,
            "scope": "generation",
            "second_run_no_diff": true,
        }));
    }
    if !node.obligations.docs.starts_with("none") {
        steps.push(json!({
            "command_id": format!("emacs-train.{}.docs", node.node_id),
            "command": node.obligations.docs,
            "scope": "docs_check",
        }));
    }
    steps.push(json!({
        "command_id": "git.diff_check",
        "command": "git diff --check",
        "scope": "diff_check",
    }));
    json!({"steps": steps})
}

fn delivery_object(node: &TrainNode) -> Value {
    let old_path_disposition =
        if node.exits.old_path.starts_with("none") { "none" } else { "replaced" };
    let mut limitations = node.limitations.clone();
    limitations.push(
        "offline composition: live candidate state is not observed (#10930 unlanded)".to_string(),
    );
    let remaining = if node.successors.is_empty() {
        node.rollback.stop.clone()
    } else {
        format!("{}; then hand off to successor {}", node.rollback.stop, node.successors.join(", "))
    };
    json!({
        "definition": "reviewable_draft_pr_and_handoff",
        "branch_suggestion": format!("tooling/{}", node.writer.conflict_key),
        "pr_title_suggestion": format!("tooling(emacs-train): {}", truncate_chars(&node.one_pr_outcome, 64)),
        "pr_body_fields": ["change", "proof", "boundaries"],
        "old_path_disposition": old_path_disposition,
        "limitations": limitations,
        "remaining_blocker_or_next": remaining,
    })
}

fn stop_object(node: &TrainNode) -> Value {
    let mut conditions = vec![
        node.rollback.stop.clone(),
        "no model invocation, GitHub mutation, or scheduling (E06 adapter boundary)".to_string(),
    ];
    if !node.successors.is_empty() {
        conditions.push(format!("stop before successor {}", node.successors.join(", ")));
    }
    json!({
        "conditions": conditions,
        "permitted_terminal_actions": [],
    })
}

// ---------------------------------------------------------------------------
// Review packet composition (#10881 projection).
// ---------------------------------------------------------------------------

pub fn compose_review_packet(
    root: &Path,
    inputs: &AdapterInputs,
    subject: &str,
    facts: &ReviewFacts,
) -> Result<Value, Refusal> {
    const PROFILE: &str = "read_only_reviewer";
    // The reviewer packet anchors the builder packet it challenges: compose
    // it first and propagate its refusal honestly.
    // Anchor the packet this review challenges.  A node that permits a coding
    // profile is reviewed against the one it permits; a controller, fan-in,
    // dogfood or external node permits no coding profile at all, so forcing one
    // would refuse the very `read_only_reviewer` packet those nodes do permit
    // and leave them unreviewable.  The reviewer profile is therefore the last
    // anchor rather than an omitted one.
    //
    // Every refusal in the chain is kept, so a caller sees which profiles were
    // attempted rather than only the one that happened to be tried last.
    let mut refusals: Vec<Refusal> = Vec::new();
    let mut builder_doc = None;
    for anchor in ["coding_agent_bounded", "coding_agent_strong", PROFILE] {
        match compose_packet_document(root, inputs, subject, anchor, None, LiveGate::Anchored) {
            Ok(doc) => {
                builder_doc = Some(doc);
                break;
            }
            Err(refusal) => refusals.push(refusal),
        }
    }
    let builder_doc = match builder_doc {
        Some(doc) => doc,
        None => {
            let mut last = refusals.pop().unwrap_or_else(|| {
                Refusal::new(subject, PROFILE, "NODE_RESOLUTION_FAILED", "no anchor".to_string())
            });
            if !refusals.is_empty() {
                let earlier = refusals
                    .iter()
                    .map(|refusal| {
                        format!("{} refused {}: {}", refusal.profile, refusal.code, refusal.detail)
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                last.detail = format!("{}; earlier anchors: {earlier}", last.detail);
            }
            return Err(last);
        }
    };
    let node = resolve_train_node(inputs, subject).map_err(|error| {
        Refusal::new(subject, PROFILE, "NODE_RESOLUTION_FAILED", error.to_string())
    })?;
    let context = resolve_spec(root, &inputs.engine, &node.node_id)
        .map_err(|error| {
            Refusal::new(&node.node_id, PROFILE, "CONTEXT_RESOLUTION_FAILED", error.to_string())
        })?
        .packet()
        .clone();

    if facts.base.trim().is_empty() || facts.head.trim().is_empty() || facts.diff.trim().is_empty()
    {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "MISSING_CANDIDATE_IDENTITY",
            "review-packet requires the exact supplied candidate identity (--base, --head, \
             --diff); the offline adapter never invents a candidate"
                .to_string(),
        ));
    }
    if facts.head.trim() != context.binding.git_commit {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "HEAD_TREE_MISMATCH",
            format!(
                "the supplied candidate head {} is not the observed checkout {}; the offline \
                 adapter composes evidence from the exact tree it runs on — run it from the \
                 candidate checkout so context, obligations and review evidence bind one tree",
                facts.head.trim(),
                context.binding.git_commit
            ),
        ));
    }
    if facts.controls.is_empty() {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "MISSING_NEGATIVE_CONTROL_EVIDENCE",
            "review-packet requires the supplied negative-control audit rows (--controls): one \
             complete row per carried falsifier; unestablished evidence is a finding, never a pass"
                .to_string(),
        ));
    }
    let builder_falsifiers: Vec<(String, String, String)> = builder_doc
        .get("proof")
        .and_then(Value::as_object)
        .and_then(|proof| proof.get("falsifiers"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    Some((
                        entry.get("id")?.as_str()?.to_string(),
                        entry.get("stage")?.as_str()?.to_string(),
                        entry.get("statement")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let negative_controls =
        build_negative_controls(&node.node_id, PROFILE, &builder_falsifiers, facts)?;

    let machine =
        render_builder_packet(&builder_doc, PacketProjection::Machine).map_err(|error| {
            Refusal::new(&node.node_id, PROFILE, "BUILDER_PACKET_INVALID", format!("{error:#}"))
        })?;
    let builder_digest = format!("sha256:{}", short(&sha256_hex(machine.as_bytes()), 16));

    let mut stage_questions: Vec<Value> = node
        .review_forward
        .questions
        .iter()
        .enumerate()
        .map(|(index, question)| json!({"id": format!("Q_forward_{index}"), "question": question}))
        .collect();
    for (id, question) in REVIEWER_CHALLENGE_QUESTIONS {
        stage_questions.push(json!({"id": id, "question": question}));
    }

    let implementation_paths: Vec<String> =
        context.components.iter().map(|component| component.path.clone()).collect();
    let tests: Vec<String> = context.tests.iter().map(|test| test.path.clone()).collect();
    if tests.is_empty() {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "MISSING_TEST_OBLIGATION",
            "the #10881 review contract requires at least one current tests/mutations \
             obligation; the exact-tree context maps no test surface for this node"
                .to_string(),
        ));
    }

    let public_lane = node.lane.contains("public") || node.lane.contains("release");
    let doc = json!({
        "schema": REVIEW_CONTRACT,
        "schema_version": 1,
        "packet_id": format!("emacs-train/{}/review", node.node_id),
        "subject": {
            "repository": {
                "name": "perl-lsp-swarm",
                "base": facts.base,
                "head": facts.head,
                "tree": context.binding.git_tree,
                "diff": facts.diff,
            },
            "programme": {
                "name": "emacs-train",
                "stage": node.node_id,
                "proposition": builder_doc
                    .get("work")
                    .and_then(|work| work.get("proposition_id"))
                    .and_then(Value::as_str)
                    .unwrap_or(&proposition_id(node)),
                "profile": PROFILE,
            },
            "owning_issue": format!("#{}", node.issue),
            "builder_packet": {
                "contract": BUILDER_CONTRACT,
                "digest": builder_digest,
            },
            "live_observation": {
                "candidate_state": "not_observed",
                "digest": format!("sha256:offline-{}", short(&inputs.engine.manifest_digest, 12)),
            },
            "changed": {
                "authorities": implementation_paths
                    .iter()
                    .map(|path| json!({"ref": path, "subject": "exact-tree declared write/component surface"}))
                    .collect::<Vec<_>>(),
                "evidence": [
                    {
                        "kind": "spec_disposition_receipt",
                        "identity": format!(
                            "#11717 checked spec disposition@sha256:{}",
                            short(&inputs.specs_digest, 12)
                        ),
                    },
                    {
                        // Real computed evidence, never a synthesized test
                        // run: the exact-tree context binding this review was
                        // composed against.
                        "kind": "exact_tree_context_receipt",
                        "identity": format!(
                            "emacs_node_context.v1 input sha256:{}@{}",
                            short(&context.binding.input_digest, 12),
                            context.binding.git_tree
                        ),
                    },
                ],
                "migrated_seams": [],
            },
        },
        "challenge": {
            "primary_proposition": node.authority_after,
            "falsifiers": builder_falsifiers
                .iter()
                .map(|(id, stage, statement)| json!({"id": id, "stage": stage, "statement": statement}))
                .collect::<Vec<_>>(),
            "stage_questions": stage_questions,
        },
        "lenses": review_lenses(public_lane),
        "negative_controls": negative_controls,
        "old_paths": [],
        "obligations": {
            "spec_ledger_ids": [
                {
                    "ref": SPECS_LEDGER_PATH,
                    "identity": format!("sha256:{}", short(&inputs.specs_digest, 16)),
                },
            ],
            "fixture_expectation_manifests": context
                .checked_specs
                .iter()
                .flat_map(|checked| {
                    checked.files.iter().map(move |file| {
                        json!({
                            "ref": format!("{}#{}", checked.bundle, file.path),
                            "identity": format!("sha256:{}", short(&file.sha256, 16)),
                        })
                    })
                })
                .collect::<Vec<_>>(),
            "tests_mutations": context
                .tests
                .iter()
                .map(|test| json!({
                    "ref": test.path,
                    "identity": format!("sha256:{}", short(&test.sha256, 16)),
                }))
                .collect::<Vec<_>>(),
            "generated_artifacts": context
                .generated
                .iter()
                .map(|generated| json!({
                    "ref": generated.output,
                    "identity": generated.generator.clone(),
                }))
                .collect::<Vec<_>>(),
            "docs_projections": if node.obligations.docs.starts_with("none") {
                Vec::new()
            } else {
                vec![json!({
                    "ref": node.obligations.docs,
                    "identity": format!("emacs-train.{}.docs", node.node_id),
                })]
            },
            "change_fragments": if node.obligations.changelog.starts_with("none") {
                Vec::new()
            } else {
                vec![json!({
                    "ref": node.obligations.changelog,
                    "identity": format!("emacs-train.{}.changelog", node.node_id),
                })]
            },
        },
        "roles": [
            {
                "role": "builder_self_review",
                "required": false,
                "obligation": "the builder's own read is never sufficient; correlated reads create false convergence",
            },
            {
                "role": "adversarial_challenger",
                "required": true,
                "obligation": "challenge authority boundaries, evidence identity and claim widening independently of the builder narrative",
            },
            {
                "role": "specialist",
                "required": false,
                "obligation": "Emacs client/platform specialists extend, never replace, the adversarial read",
            },
            {
                "role": "evidence_worker",
                "required": true,
                "obligation": "verify every digest, receipt and currentness anchor against the observed tree",
            },
        ],
        "lifecycle": {
            "graceful_cleanup_claimed": false,
        },
    });

    render_review_packet(&doc, ReviewProjection::Machine)
        .map_err(|error| {
            Refusal::new(
                &node.node_id,
                PROFILE,
                "SHARED_CONTRACT_VALIDATION_FAILED",
                format!("the composed packet violates the shared #10881 contract: {error:#}"),
            )
        })
        .map(|_| doc)
}

/// Validate and shape the supplied negative-control audit rows: every
/// carried falsifier needs one complete row, and only established criteria
/// with evidence may enter the packet (not_established is a finding the
/// composer refuses to paper over).
fn build_negative_controls(
    node_id: &str,
    profile: &str,
    falsifiers: &[(String, String, String)],
    facts: &ReviewFacts,
) -> Result<Value, Refusal> {
    const CRITERIA: [&str; 6] = [
        "exists",
        "red_before_or_mutation_evidence",
        "passes_only_intended_implementation",
        "correct_subject_and_generation",
        "independent_expectation_source",
        "alternate_subject_exclusion",
    ];
    let mut rows = Vec::new();
    for (id, _, _) in falsifiers {
        let Some(supplied) = facts.controls.get(id) else {
            return Err(Refusal::new(
                node_id,
                profile,
                "CONTROL_FALSIFIER_UNCOVERED",
                format!(
                    "falsifier {id} has no supplied negative-control audit row; a declared \
                     falsifier may not lose its load-bearing audit"
                ),
            ));
        };
        let mut checks = serde_json::Map::new();
        for criterion in CRITERIA {
            let Some(entry) = supplied.get(criterion) else {
                return Err(Refusal::new(
                    node_id,
                    profile,
                    "CONTROL_CRITERION_INCOMPLETE",
                    format!("criterion {criterion} of falsifier {id} is not supplied"),
                ));
            };
            let status = entry.get("status").and_then(Value::as_str).unwrap_or_default();
            let evidence = entry.get("evidence").and_then(Value::as_str).unwrap_or_default();
            if status != "established" || evidence.trim().is_empty() {
                return Err(Refusal::new(
                    node_id,
                    profile,
                    "CONTROL_NOT_ESTABLISHED",
                    format!(
                        "criterion {criterion} of falsifier {id} is not established with \
                         evidence; unestablished controls are findings, never passes"
                    ),
                ));
            }
            checks.insert(
                criterion.to_string(),
                json!({"status": "established", "evidence": evidence}),
            );
        }
        rows.push(json!({"falsifier_id": id, "checks": Value::Object(checks)}));
    }
    Ok(Value::Array(rows))
}

fn review_lenses(public_lane: bool) -> Value {
    let mut lenses = vec![
        lens("semantic_correctness", None, None),
        lens(
            "architecture_authority_duplication",
            None,
            Some(&[
                (
                    "R_no_second_packet_schema",
                    "the adapter supplies fields only; any Emacs-local packet schema beside #10872/#10881 is duplicate authority",
                ),
                (
                    "R_registry_substitution",
                    "registry/docs/certification leaves repairing product behavior are challenged as authority substitution",
                ),
            ]),
        ),
        lens(
            "subject_evidence_identity",
            None,
            Some(&[
                (
                    "R_client_cross_satisfaction",
                    "another client/server/version/generation must not satisfy the oracle meant for the exact subject",
                ),
                (
                    "R_forbidden_substitutes",
                    "the Emacs forbidden substitutes list (runner/profile validity as host support, manifest identity as runtime identity, perl-mode as cperl-mode, Linux as macOS/Windows/TRAMP, ...) must be checked against the candidate",
                ),
            ]),
        ),
        lens(
            "lifecycle_currentness_concurrency",
            None,
            Some(&[(
                "R_shutdown_vs_descendant_cleanup",
                "shutdown_completed/status 0 must not be mistaken for descendant cleanup; stale tree/spec/context/live inputs must leave the packet non-pass",
            )]),
        ),
        lens(
            "security_trust_boundary",
            None,
            Some(&[(
                "R_no_provider_private_state",
                "provider/model/private prompt/token/local absolute path material must never enter durable output",
            )]),
        ),
        lens(
            "resource_retention_cleanup",
            None,
            Some(&[(
                "R_cleanup_boundary",
                "cleanup/process authority (#8734) is preserved; cleanup or missing/instrument evidence must not become green",
            )]),
        ),
        lens(
            "platform_runtime_portability",
            None,
            Some(&[(
                "R_platform_truth",
                "Linux evidence must not widen into macOS/Windows/TRAMP claims",
            )]),
        ),
        lens(
            "spec_test_docs_consistency",
            None,
            Some(&[(
                "R_generated_currentness",
                "generated docs/registry/spec outputs must not outrun their canonical inputs; #11360/#11361 observation and receipt ownership is mapped, not manufactured",
            )]),
        ),
    ];
    if public_lane {
        lenses.push(lens(
            "release_external_boundary",
            None,
            Some(&[(
                "R_public_receipt_boundary",
                "local receipt must not widen into public/release-candidate receipt; source head is not a released package; upstream accepted is not released built-in",
            )]),
        ));
    } else {
        lenses.push(lens(
            "release_external_boundary",
            Some("node lane is not a public/release lane; no release or external stage is claimed"),
            None,
        ));
    }
    Value::Array(lenses)
}

fn lens(name: &str, reason: Option<&str>, refinements: Option<&[(&str, &str)]>) -> Value {
    let mut object = json!({"lens": name});
    match reason {
        Some(reason) => {
            object["applicability"] = json!("not_applicable");
            object["reason"] = json!(reason);
        }
        None => {
            object["applicability"] = json!("required");
        }
    }
    if let Some(refinements) = refinements {
        object["refinements"] = Value::Array(
            refinements
                .iter()
                .map(|(id, statement)| json!({"id": id, "statement": statement}))
                .collect(),
        );
    }
    object
}

// ---------------------------------------------------------------------------
// Reconciliation packet composition (#10872 projection, no coding authority).
// ---------------------------------------------------------------------------

pub fn compose_reconcile_packet(
    root: &Path,
    inputs: &AdapterInputs,
    subject: &str,
    candidates: Option<&Value>,
) -> Result<Value, Refusal> {
    const PROFILE: &str = "reconciliation_only";
    let node = resolve_train_node(inputs, subject).map_err(|error| {
        Refusal::new(subject, PROFILE, "NODE_RESOLUTION_FAILED", error.to_string())
    })?;
    let Some(candidates) = candidates else {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "NO_LIVE_OBSERVATION",
            "reconciliation requires explicitly supplied exact candidate facts (--candidates); \
             no live observation means no packet assuming vacancy (#10930 unlanded)"
                .to_string(),
        ));
    };
    let Some(entries) = candidates.as_array() else {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "MALFORMED_CANDIDATE_FACTS",
            "candidate facts must be a JSON array of {identity, state, facts}".to_string(),
        ));
    };
    if entries.is_empty() {
        return Err(Refusal::new(
            &node.node_id,
            PROFILE,
            "MALFORMED_CANDIDATE_FACTS",
            "reconciliation requires at least one supplied candidate; an empty set is not an \
             observation of vacancy"
                .to_string(),
        ));
    }
    let mut blocking_edges = Vec::new();
    let mut seen_identities: BTreeSet<String> = BTreeSet::new();
    // The adjudication the reviewer must make is over the candidates' *facts*
    // (dirty/unpushed work, stack, base, ownership, salvage), not their names.
    // Carry those facts into the packet and into the frontier identity so two
    // candidate sets that differ only in facts can never render the same bytes.
    let mut candidate_binding = String::new();
    for entry in entries {
        let identity = entry.get("identity").and_then(Value::as_str).unwrap_or_default().trim();
        if identity.is_empty() {
            return Err(Refusal::new(
                &node.node_id,
                PROFILE,
                "MALFORMED_CANDIDATE_FACTS",
                "every supplied candidate fact requires a non-empty exact identity".to_string(),
            ));
        }
        if !seen_identities.insert(identity.to_string()) {
            return Err(Refusal::new(
                &node.node_id,
                PROFILE,
                "MALFORMED_CANDIDATE_FACTS",
                format!(
                    "candidate identity {identity} is supplied twice; each candidate must be \
                     adjudicated exactly once"
                ),
            ));
        }
        // `state` is adjudicated against the repository's existing closed
        // candidate-state law rather than a second vocabulary defined here:
        // restating it would be exactly the duplicate authority this adapter's
        // "adapter, not an ontology" boundary rules out.
        //
        // That law records `candidate_flags: Vec<String>` -- independent flags,
        // not one collapsed signal -- so a candidate may carry several (a
        // branch can be both `stale_base` and `dirty_or_unpushed_unique_work`,
        // which is exactly when keep/rewrite/drop/transfer stops being
        // obvious). Both a single flag and an array of flags are accepted;
        // every flag must appear in the law, and the set is normalized so the
        // packet identity does not depend on the order they were supplied in.
        let raw_states: Vec<&str> = match entry.get("state") {
            Some(Value::String(single)) => vec![single.as_str()],
            Some(Value::Array(many)) => {
                let mut collected = Vec::new();
                for flag in many {
                    let Some(flag) = flag.as_str() else {
                        return Err(Refusal::new(
                            &node.node_id,
                            PROFILE,
                            "MALFORMED_CANDIDATE_FACTS",
                            format!("candidate {identity} supplies a non-string state flag"),
                        ));
                    };
                    collected.push(flag);
                }
                collected
            }
            _ => Vec::new(),
        };
        let mut flags: Vec<&str> =
            raw_states.into_iter().map(str::trim).filter(|flag| !flag.is_empty()).collect();
        flags.sort_unstable();
        flags.dedup();
        if flags.is_empty() {
            return Err(Refusal::new(
                &node.node_id,
                PROFILE,
                "MALFORMED_CANDIDATE_FACTS",
                format!(
                    "candidate {identity} supplies no state; an absent state must not be \
                     recorded as an observed one"
                ),
            ));
        }
        for flag in &flags {
            if !module_train_live::CANDIDATE_STATES.contains(flag) {
                return Err(Refusal::new(
                    &node.node_id,
                    PROFILE,
                    "MALFORMED_CANDIDATE_FACTS",
                    format!(
                        "candidate {identity} supplies state flag {flag}, which is not in the \
                         closed candidate-state vocabulary (module_train_live::CANDIDATE_STATES); \
                         the frontier digest must not content-address an uninterpretable token"
                    ),
                ));
            }
        }
        let state = flags.join(",");
        let facts = entry.get("facts").and_then(Value::as_str).unwrap_or_default().trim();
        if facts.is_empty() {
            return Err(Refusal::new(
                &node.node_id,
                PROFILE,
                "MALFORMED_CANDIDATE_FACTS",
                format!(
                    "candidate {identity} supplies no facts; keep/rewrite/drop/transfer cannot \
                     be adjudicated from an identity and a state alone"
                ),
            ));
        }
        candidate_binding.push_str(identity);
        candidate_binding.push('\u{1f}');
        candidate_binding.push_str(&state);
        candidate_binding.push('\u{1f}');
        candidate_binding.push_str(facts);
        candidate_binding.push('\u{1e}');
        blocking_edges.push(json!({
            "edge": identity,
            "reason": format!(
                "supplied candidate fact ({state}): {facts}; requires explicit \
                 keep/rewrite/drop/transfer adjudication before any coding packet"
            ),
        }));
    }
    let candidates_digest = sha256_hex(candidate_binding.as_bytes());

    // The reconciliation packet is a shared-contract document: it carries no
    // coding authority (write boundary none) and blocks coding until exact
    // candidate ownership is resolved.
    let mut doc = compose_packet_document(root, inputs, subject, PROFILE, None, LiveGate::Anchored)
        .map_err(|mut refusal| {
            // The only acceptable refusal class here is a coding eligibility
            // rule that does not apply to a non-coding reconciliation packet.
            refusal.profile = PROFILE.to_string();
            refusal
        })?;
    if let Some(object) = doc.as_object_mut() {
        object.insert(
            "frontier".to_string(),
            json!({
                "decision": "blocked",
                "digest": format!(
                    "reconcile:emacs-train:{}:{}:supplied:sha256:{}",
                    short(&inputs.engine.manifest_digest, 12),
                    node.node_id,
                    short(&candidates_digest, 16)
                ),
                "blocking_edges": blocking_edges,
            }),
        );
        if let Some(work) = object.get_mut("work").and_then(Value::as_object_mut) {
            work.insert(
                "result_sentence".to_string(),
                json!(format!(
                    "Resolve exact candidate ownership for node {} (#{}): keep, rewrite, drop or \
                     transfer each supplied candidate; no coding packet until ownership is exact.",
                    node.node_id, node.issue
                )),
            );
            work.insert(
                "non_goals".to_string(),
                json!([
                    "no coding packet until exact candidate ownership is resolved",
                    "no ranking by recency, author, provider, title or diff size",
                    "dirty/unpushed unique work is not disposable",
                    "no model invocation, GitHub mutation, or scheduling",
                ]),
            );
        }
    }
    render_builder_packet(&doc, PacketProjection::Machine)
        .map_err(|error| {
            Refusal::new(
                &node.node_id,
                PROFILE,
                "SHARED_CONTRACT_VALIDATION_FAILED",
                format!("the composed packet violates the shared #10872 contract: {error:#}"),
            )
        })
        .map(|_| doc)
}

// ---------------------------------------------------------------------------
// Denominator check.
// ---------------------------------------------------------------------------

/// Refusal codes that express typed packet-eligibility blockers rather
/// than instrument or shared-contract failures.
fn is_eligibility_refusal(code: &str) -> bool {
    matches!(
        code,
        "MISSING_SPEC_DISPOSITION"
            | "PROFILE_NOT_PERMITTED"
            | "SPEC_DISPOSITION_NOT_BUILDER"
            | "CONTEXT_MAPPING_GAP"
            | "HARD_DEPENDENCY_NOT_CURRENT"
            | "NO_WRITE_SURFACE"
            | "NO_LIVE_OBSERVATION"
    )
}

fn run_packets_check(root: &Path, live: Option<&LiveObservation>) -> Result<()> {
    let inputs = load_adapter_inputs(root)?;
    let mut rendered = 0usize;
    let mut refused = 0usize;
    let mut refusal_lines = Vec::new();
    for node in &inputs.engine.manifest.nodes {
        let mut node_rendered = false;
        for profile in ["coding_agent_bounded", "coding_agent_strong"] {
            match compose_builder_packet(root, &inputs, &node.node_id, profile, live) {
                Ok(doc) => {
                    // Determinism: two independent renders must be
                    // byte-identical, and the shared contract must accept the
                    // document unchanged (zero drift).
                    let first = render_builder_packet(&doc, PacketProjection::Machine)
                        .with_context(|| {
                            format!("rendering the packet of node {} twice (first)", node.node_id)
                        })?;
                    let second = render_builder_packet(&doc, PacketProjection::Machine)
                        .with_context(|| {
                            format!("rendering the packet of node {} twice (second)", node.node_id)
                        })?;
                    ensure!(
                        first == second,
                        "non-deterministic packet render for node {}/{}",
                        node.node_id,
                        profile
                    );
                    if !node_rendered {
                        println!(
                            "EMU_PACKET node={} profile={} status=ok bytes={}",
                            node.node_id,
                            profile,
                            first.len()
                        );
                    }
                    node_rendered = true;
                }
                Err(refusal) => {
                    if is_eligibility_refusal(refusal.code) {
                        refusal_lines.push(refusal.line());
                    } else {
                        // Instrument/shared-contract failures are not typed
                        // eligibility refusals; a green denominator must
                        // never hide them.
                        bail!(
                            "packet denominator instrument failure ({}): {}",
                            refusal.code,
                            refusal.line()
                        );
                    }
                }
            }
        }
        if node_rendered {
            rendered += 1;
        } else {
            refused += 1;
        }
    }
    for line in &refusal_lines {
        println!("{line}");
    }
    println!(
        "EMU_PACKETS_CHECK=OK rendered={rendered} refused={refused} total={} \
         manifest_sha256={} spec_ledger_sha256={} tree={}",
        inputs.engine.manifest.nodes.len(),
        inputs.engine.manifest_digest,
        inputs.specs_digest,
        inputs.engine.git_tree
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Supplied-fact parsing (never invention).
// ---------------------------------------------------------------------------

fn parse_live_observation(path: &Path) -> Result<LiveObservation> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading live observation {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing live observation {}", path.display()))?;
    let state =
        value.get("candidate_state").and_then(Value::as_str).unwrap_or_default().to_string();
    ensure!(
        ["not_observed", "observed"].contains(&state.as_str()),
        "live observation candidate_state must be not_observed or observed"
    );
    let digest = value.get("digest").and_then(Value::as_str).unwrap_or_default().to_string();
    ensure!(!digest.is_empty(), "live observation digest is required");
    // A digest of all zeroes binds to nothing, so no consumer can tell a real
    // sweep from a fabricated one.  `--live-observation` is a public flag and
    // the composition fixture is a checked-in path, so the invocation is
    // copyable: refuse the placeholder rather than let it reach a `ready`
    // packet as evidence.
    let digest_body = digest.rsplit(':').next().unwrap_or_default();
    ensure!(
        !digest_body.is_empty() && digest_body.chars().any(|character| character != '0'),
        "live observation digest {digest} binds to nothing; an all-zero digest is a placeholder, \
         not evidence of an observation"
    );
    let identity = value.get("candidate_identity").and_then(Value::as_str).map(str::to_string);
    if state == "observed" {
        ensure!(
            identity.as_deref().map(|identity| !identity.is_empty()).unwrap_or(false),
            "an observed candidate requires its exact identity"
        );
    }
    // Unknown keys are refused rather than dropped: a misspelled caller fact
    // that disappears silently weakens exactly the fail-closed diagnostics this
    // adapter exists to provide.
    if let Some(object) = value.as_object() {
        const KNOWN_KEYS: &[&str] =
            &["candidate_state", "digest", "candidate_identity", "collision_state"];
        let unknown: Vec<&str> =
            object.keys().map(String::as_str).filter(|key| !KNOWN_KEYS.contains(key)).collect();
        ensure!(
            unknown.is_empty(),
            "live observation carries unknown field(s) [{}]; supported fields are [{}]",
            unknown.join(", "),
            KNOWN_KEYS.join(", ")
        );
    }
    Ok(LiveObservation {
        candidate_state: state,
        digest,
        candidate_identity: identity,
        collision_state: value.get("collision_state").and_then(Value::as_str).map(str::to_string),
    })
}

fn parse_controls(path: &Path) -> Result<BTreeMap<String, BTreeMap<String, Value>>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading negative controls {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing negative controls {}", path.display()))?;
    let Some(object) = value.as_object() else {
        bail!("negative controls must be an object keyed by falsifier id");
    };
    let mut controls = BTreeMap::new();
    for (falsifier, criteria) in object {
        let Some(criteria) = criteria.as_object() else {
            bail!("negative controls entry {falsifier} must map criterion -> result");
        };
        let mut map = BTreeMap::new();
        for (criterion, result) in criteria {
            map.insert(criterion.clone(), result.clone());
        }
        controls.insert(falsifier.clone(), map);
    }
    Ok(controls)
}

fn parse_candidates(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading candidate facts {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing candidate facts {}", path.display()))
}

fn render_with_format(doc: &Value, format: &str) -> Result<String> {
    match format {
        "machine" | "json" => render_builder_packet(doc, PacketProjection::Machine),
        "markdown" | "md" => render_builder_packet(doc, PacketProjection::Markdown),
        "compact" => render_builder_packet(doc, PacketProjection::Compact),
        other => bail!("unknown format '{other}': expected machine, markdown or compact"),
    }
}

fn render_review_with_format(doc: &Value, format: &str) -> Result<String> {
    match format {
        "machine" | "json" => render_review_packet(doc, ReviewProjection::Machine),
        "markdown" | "md" => render_review_packet(doc, ReviewProjection::Markdown),
        "compact" => render_review_packet(doc, ReviewProjection::Compact),
        other => bail!("unknown format '{other}': expected machine, markdown or compact"),
    }
}

// ---------------------------------------------------------------------------
// Tests: falsifier fixtures against deliberately wrong inputs.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use serde_json::{Value, json};

    pub(crate) fn write_json(root: &std::path::Path, relative: &str, value: &Value) -> Result<()> {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }

    pub(crate) fn write_text(root: &std::path::Path, relative: &str, text: &str) -> Result<()> {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }

    pub(crate) fn specs_ledger(records: &[Value]) -> Value {
        json!({
            "schema": "emacs_train_specs.v1",
            "schema_version": 1,
            "programme": {
                "parent_programme_issue": 7979,
                "controller_issue": 8706,
                "durable_architecture_issue": 11716,
                "stable_train_issue": 10918,
                "governing_issue": 11717,
                "engine_issue": 11751,
                "method_authority": "#3983",
                "consumed_manifest": "emacs_train.v1@fixture"
            },
            "records": records
        })
    }

    pub(crate) fn disposition_record(node_id: &str, issue: u64, disposition: &str) -> Value {
        json!({
            "node_id": node_id,
            "issue": issue,
            "train_role": "implementation",
            "disposition": disposition,
            "disposition_provenance": "manifest",
            "authority_after": "fixture authority",
            "spec_owner": "#11717"
        })
    }
}
