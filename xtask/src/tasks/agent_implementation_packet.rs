//! Validate and render the shared bounded builder-packet contract
//! (`agent_implementation_packet.v1`, issue #10872).
//!
//! This module owns the closed shared packet vocabulary, the deterministic
//! validation invariants, and the canonical machine, Markdown, and compact
//! projections. It converts an already-selected programme node plus explicit
//! supplied inputs into one model-neutral work contract.
//!
//! It deliberately owns no node selection, readiness evaluation, GitHub or
//! network observation, agent assignment, lease tracking, frontier mutation,
//! merge, or release authority. Every required input is supplied by the
//! caller; missing input fails validation instead of producing plausible
//! prose. Packet instances are runtime-local outputs: the render path writes
//! projections to stdout only and never writes tracked repository state.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const SCHEMA_PATH: &str = "schemas/agent_implementation_packet.v1.schema.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/agent_implementation_packet.v1.schema.json";
const SCHEMA_NAME: &str = "agent_implementation_packet.v1";

const FIXTURE_DIR: &str = "fixtures/agent_implementation_packet";
const GOLDEN_DIR: &str = "fixtures/agent_implementation_packet/golden";

const VALID_FIXTURES: &[&str] = &[
    "bounded_leaf_offline.v1.json",
    "consumer_emacs_e06_shape.v1.json",
    "live_observed_candidate.v1.json",
];
const SHUFFLED_FIXTURE: &str = "shuffled/bounded_leaf_offline_shuffled.v1.json";

/// Closed work-node vocabulary. Only a bounded implementation leaf is direct
/// builder work; other kinds need an explicit programme-manifest bounded-leaf
/// declaration.
const NODE_KINDS: &[&str] = &["bounded_implementation_leaf", "controller", "fan_in"];

const FRONTIER_DECISIONS: &[&str] = &["ready", "blocked"];

/// Closed authority-map groups.
const AUTHORITY_GROUPS: &[&str] = &[
    "must_be_current",
    "may_be_mined",
    "must_not_be_reimplemented",
    "consumer_fan_in",
    "external_manual_owner",
];

/// Closed provenance of an authority's currentness. A historical branch is
/// never a landed authority.
const PROOF_KINDS: &[&str] = &[
    "current_tree_probe",
    "spec_disposition",
    "frontier_digest",
    "live_observation",
    "historical_branch",
    "external_owner_note",
];

/// Proof kinds that may anchor a "described as landed" authority.
const CURRENTNESS_PROOF_KINDS: &[&str] =
    &["current_tree_probe", "spec_disposition", "frontier_digest", "live_observation"];

const CANDIDATE_STATES: &[&str] = &["not_observed", "observed"];

const VERIFICATION_SCOPES: &[&str] = &[
    "focused_proof",
    "generation",
    "architecture_policy",
    "file_policy",
    "docs_check",
    "format",
    "clippy",
    "typecheck",
    "diff_check",
];

const OLD_PATH_DISPOSITIONS: &[&str] = &["none", "replaced", "reused", "retired", "superseded"];

const PERMITTED_TERMINAL_ACTIONS: &[&str] =
    &["merge_after_review", "external_submission", "release_publication"];

const WRITE_BOUNDARIES: &[&str] = &["none", "repository_candidate_branch"];

/// Closed delivery definition. A local diff, helper-only substrate,
/// unpushed commit, or unrelated green check is not delivered.
const DELIVERY_DEFINITION: &str = "reviewable_draft_pr_and_handoff";

/// Closed forbidden mutable-state key family. A durable packet document
/// never embeds assignment, lease, liveness, or frontier-cursor state.
const MUTABLE_STATE_KEYS: &[&str] = &[
    "lease",
    "lease_owner",
    "assignment",
    "assigned_agent",
    "agent_id",
    "wake_event",
    "liveness",
    "heartbeat",
    "task_order",
    "active_goal",
    "frontier_cursor",
    "next_wake",
    "owner_token",
];

/// Closed banned generic proof/verification statements. "Add tests" or "run
/// the workspace" is not a falsifier or a verification route.
const GENERIC_STATEMENTS: &[&str] = &[
    "add tests",
    "add more tests",
    "run the workspace",
    "run all tests",
    "run the test suite",
    "make ci green",
];

/// Closed root field set of a packet document.
const ROOT_KEYS: &[&str] = &[
    "schema",
    "schema_version",
    "packet_id",
    "repository",
    "programme",
    "work",
    "actor",
    "frontier",
    "current_tree_probe",
    "live_observation",
    "authorities",
    "surfaces",
    "proof",
    "verification",
    "delivery",
    "stop",
    "metadata",
];

/// Required inputs the caller must supply; absence fails generation
/// fail-closed instead of being inferred.
const REQUIRED_SECTIONS: &[&str] = &[
    "packet_id",
    "repository",
    "programme",
    "work",
    "actor",
    "frontier",
    "current_tree_probe",
    "authorities",
    "surfaces",
    "proof",
    "verification",
    "delivery",
    "stop",
];

const WORK_KEYS: &[&str] = &[
    "owning_issue",
    "node_id",
    "proposition_id",
    "profile",
    "profile_conditional",
    "profile_decision",
    "node_kind",
    "bounded_leaf_manifest_ref",
    "result_sentence",
    "claim_ceiling",
    "unproven",
    "non_goals",
    "successors",
];

const AUTHORITY_ENTRY_KEYS: &[&str] =
    &["ref", "subject", "proof_kind", "proof_tree", "proof_digest"];
const LIVE_OBSERVATION_KEYS: &[&str] =
    &["candidate_state", "digest", "candidate_identity", "collision_state"];
const SURFACE_KEYS: &[&str] = &[
    "implementation_paths",
    "tests_fixtures",
    "generated_artifacts",
    "docs_fragments",
    "writer_slots",
    "forbidden_adjacent",
];
const PROOF_KEYS: &[&str] = &[
    "falsifiers",
    "positive_discriminator",
    "mutation_controls",
    "terminal_outcomes",
    "cleanup_retention",
];
const VERIFICATION_STEP_KEYS: &[&str] = &["command_id", "command", "scope", "second_run_no_diff"];
const DELIVERY_KEYS: &[&str] = &[
    "definition",
    "branch_suggestion",
    "pr_title_suggestion",
    "pr_body_fields",
    "old_path_disposition",
    "limitations",
    "remaining_blocker_or_next",
];
const STOP_KEYS: &[&str] = &["conditions", "permitted_terminal_actions", "authority"];

/// One deterministic validation violation with a stable reason code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Violation {
    code: String,
    detail: String,
}

impl Violation {
    fn new(code: &str, detail: String) -> Self {
        Self { code: code.to_string(), detail }
    }
}

/// The three deterministic model-neutral projections. Every rendering
/// preserves the full semantic packet; wrappers change syntax only.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum PacketProjection {
    /// Canonical machine packet (canonical JSON bytes).
    Machine,
    /// Phone-readable Markdown.
    Markdown,
    /// Compact agent prompt.
    Compact,
}

impl PacketProjection {
    fn name(self) -> &'static str {
        match self {
            PacketProjection::Machine => "machine",
            PacketProjection::Markdown => "markdown",
            PacketProjection::Compact => "compact",
        }
    }
}

fn as_str_map(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn non_empty_string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    non_empty_string(object.get(key))
}

fn string_array(object: &Map<String, Value>, key: &str) -> Vec<String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn object_array<'a>(object: &'a Map<String, Value>, key: &str) -> Vec<&'a Map<String, Value>> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default()
}

fn is_generic_statement(statement: &str) -> bool {
    let normalized = statement.trim().to_lowercase();
    GENERIC_STATEMENTS.contains(&normalized.as_str())
}

/// Recursively reject the closed mutable-state key family at any depth,
/// including caller metadata: a durable packet document never carries
/// assignment, lease, liveness, or frontier-cursor state.
fn scan_mutable_state(value: &Value, where_: &str, violations: &mut Vec<Violation>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if MUTABLE_STATE_KEYS.contains(&key.as_str()) {
                    violations.push(Violation::new(
                        "mutable_state_embedded",
                        format!("{where_}: mutable live-state field {key}"),
                    ));
                }
                scan_mutable_state(child, &format!("{where_}.{key}"), violations);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                scan_mutable_state(item, &format!("{where_}[{index}]"), violations);
            }
        }
        _ => {}
    }
}

fn check_unknown_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            violations
                .push(Violation::new("unknown_field", format!("{where_}: unexpected field {key}")));
        }
    }
}

fn require_non_empty(
    object: &Map<String, Value>,
    key: &str,
    code: &str,
    where_: &str,
    violations: &mut Vec<Violation>,
) -> Option<String> {
    match string_field(object, key) {
        Some(value) => Some(value.to_string()),
        None => {
            violations
                .push(Violation::new(code, format!("{where_}: {key} must be a non-empty string")));
            None
        }
    }
}

/// Validate one packet document. Returns every violation, deterministically
/// ordered. An empty result means the document satisfies the shared closed
/// contract and may be rendered.
fn validate_document(doc: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Some(root) = as_str_map(doc) else {
        return vec![Violation::new("not_an_object", "document must be a JSON object".to_string())];
    };

    if string_field(root, "schema") != Some(SCHEMA_NAME) {
        violations.push(Violation::new("wrong_schema", format!("schema must be {SCHEMA_NAME}")));
        return violations;
    }
    if root.get("schema_version").and_then(Value::as_i64) != Some(1) {
        violations
            .push(Violation::new("wrong_schema_version", "schema_version must be 1".to_string()));
    }
    require_non_empty(root, "packet_id", "missing_packet_id", "document", &mut violations);
    check_unknown_keys(root, ROOT_KEYS, "document", &mut violations);
    scan_mutable_state(doc, "document", &mut violations);

    for section in REQUIRED_SECTIONS {
        match root.get(*section) {
            None => violations.push(Violation::new(
                "missing_required_input",
                format!("document: required input {section} was not supplied"),
            )),
            Some(value)
                // packet_id is the one scalar required input; every other
                // required section is object-typed and a non-object value
                // must fail closed instead of silently skipping validation.
                if *section != "packet_id" && as_str_map(value).is_none() =>
            {
                violations.push(Violation::new(
                    "malformed_section",
                    format!("document: required input {section} must be an object"),
                ))
            }
            Some(_) => {}
        }
    }

    // Repository and programme identities.
    if let Some(repository) = root.get("repository").and_then(as_str_map) {
        check_unknown_keys(repository, &["name", "observed_tree"], "repository", &mut violations);
        require_non_empty(
            repository,
            "name",
            "missing_repository_name",
            "repository",
            &mut violations,
        );
        require_non_empty(
            repository,
            "observed_tree",
            "missing_observed_tree",
            "repository",
            &mut violations,
        );
    }
    if let Some(programme) = root.get("programme").and_then(as_str_map) {
        check_unknown_keys(
            programme,
            &["name", "manifest", "manifest_version"],
            "programme",
            &mut violations,
        );
        require_non_empty(
            programme,
            "name",
            "missing_programme_name",
            "programme",
            &mut violations,
        );
        require_non_empty(programme, "manifest", "missing_manifest", "programme", &mut violations);
        require_non_empty(
            programme,
            "manifest_version",
            "missing_manifest_version",
            "programme",
            &mut violations,
        );
    }

    // Work identity, claim ceiling, node kind, profile conditionality.
    if let Some(work) = root.get("work").and_then(as_str_map) {
        check_unknown_keys(work, WORK_KEYS, "work", &mut violations);
        for (key, code) in [
            ("owning_issue", "missing_owning_issue"),
            ("node_id", "missing_node_id"),
            ("proposition_id", "missing_proposition_id"),
            ("profile", "missing_profile"),
            ("result_sentence", "missing_result_sentence"),
            ("claim_ceiling", "missing_claim_ceiling"),
        ] {
            require_non_empty(work, key, code, "work", &mut violations);
        }
        if let Some(kind) = string_field(work, "node_kind") {
            if !NODE_KINDS.contains(&kind) {
                violations.push(Violation::new(
                    "unknown_node_kind",
                    format!("work: unknown node kind {kind}"),
                ));
            } else if kind != "bounded_implementation_leaf"
                && string_field(work, "bounded_leaf_manifest_ref").is_none()
            {
                violations.push(Violation::new(
                    "non_buildable_node",
                    format!(
                        "work: node kind {kind} is not direct builder work without an explicit bounded_leaf_manifest_ref"
                    ),
                ));
            }
        } else {
            violations.push(Violation::new(
                "missing_node_kind",
                "work: node_kind is required".to_string(),
            ));
        }
        match work.get("profile_conditional").and_then(Value::as_bool) {
            Some(true) => {
                let decision = work.get("profile_decision").and_then(as_str_map);
                match decision {
                    Some(decision) => {
                        check_unknown_keys(
                            decision,
                            &["selecting_authority", "selected_value"],
                            "work.profile_decision",
                            &mut violations,
                        );
                        require_non_empty(
                            decision,
                            "selecting_authority",
                            "missing_profile_decision_field",
                            "work.profile_decision",
                            &mut violations,
                        );
                        require_non_empty(
                            decision,
                            "selected_value",
                            "missing_profile_decision_field",
                            "work.profile_decision",
                            &mut violations,
                        );
                    }
                    None => violations.push(Violation::new(
                        "missing_profile_decision",
                        "work: a conditional profile must carry its selecting decision".to_string(),
                    )),
                }
            }
            Some(false) => {
                if work.contains_key("profile_decision") {
                    violations.push(Violation::new(
                        "ambiguous_profile_decision",
                        "work: profile is declared unconditional but carries a decision"
                            .to_string(),
                    ));
                }
            }
            None => violations.push(Violation::new(
                "missing_profile_conditionality",
                "work: profile_conditional is required; absent conditionality is ambiguity"
                    .to_string(),
            )),
        }
        if string_array(work, "non_goals").is_empty() {
            violations.push(Violation::new(
                "missing_non_goals",
                "work: at least one explicit non-goal is required".to_string(),
            ));
        }
    }

    // Actor and external-write boundary.
    if let Some(actor) = root.get("actor").and_then(as_str_map) {
        check_unknown_keys(actor, &["role", "write_boundary"], "actor", &mut violations);
        require_non_empty(actor, "role", "missing_actor_role", "actor", &mut violations);
        match string_field(actor, "write_boundary") {
            Some(boundary) if WRITE_BOUNDARIES.contains(&boundary) => {}
            Some(boundary) => violations.push(Violation::new(
                "unknown_write_boundary",
                format!("actor: unknown write boundary {boundary}"),
            )),
            None => violations.push(Violation::new(
                "missing_write_boundary",
                "actor: write_boundary is required".to_string(),
            )),
        }
    }

    // Offline frontier decision: blocked nodes name their exact blocking
    // edges; ready nodes carry none.
    if let Some(frontier) = root.get("frontier").and_then(as_str_map) {
        check_unknown_keys(
            frontier,
            &["decision", "digest", "blocking_edges"],
            "frontier",
            &mut violations,
        );
        require_non_empty(
            frontier,
            "digest",
            "missing_frontier_digest",
            "frontier",
            &mut violations,
        );
        let decision = string_field(frontier, "decision");
        match decision {
            Some(decision) if FRONTIER_DECISIONS.contains(&decision) => {
                let blocking_edges = object_array(frontier, "blocking_edges");
                for edge in &blocking_edges {
                    check_unknown_keys(
                        edge,
                        &["edge", "reason"],
                        "frontier.blocking_edges",
                        &mut violations,
                    );
                    require_non_empty(
                        edge,
                        "edge",
                        "malformed_blocking_edge",
                        "frontier.blocking_edges",
                        &mut violations,
                    );
                    require_non_empty(
                        edge,
                        "reason",
                        "malformed_blocking_edge",
                        "frontier.blocking_edges",
                        &mut violations,
                    );
                }
                if decision == "blocked" && blocking_edges.is_empty() {
                    violations.push(Violation::new(
                        "missing_blocking_edge",
                        "frontier: a blocked node must name its exact blocking edges".to_string(),
                    ));
                }
                if decision == "ready" && !blocking_edges.is_empty() {
                    violations.push(Violation::new(
                        "ambiguous_frontier_state",
                        "frontier: a ready node cannot carry blocking edges".to_string(),
                    ));
                }
            }
            Some(decision) => violations.push(Violation::new(
                "unknown_frontier_decision",
                format!("frontier: unknown decision {decision}"),
            )),
            None => violations.push(Violation::new(
                "missing_frontier_decision",
                "frontier: decision is required".to_string(),
            )),
        }
    }

    // Exact current-tree subject/probe result.
    if let Some(probe) = root.get("current_tree_probe").and_then(as_str_map) {
        check_unknown_keys(
            probe,
            &["subject", "result", "digest"],
            "current_tree_probe",
            &mut violations,
        );
        for (key, code) in [
            ("subject", "missing_tree_probe_subject"),
            ("result", "missing_tree_probe_result"),
            ("digest", "missing_tree_probe_digest"),
        ] {
            require_non_empty(probe, key, code, "current_tree_probe", &mut violations);
        }
    }

    // Optional live observation: absence of knowledge is never fabricated as
    // vacancy and observation is never invented.
    if let Some(live) = root.get("live_observation").and_then(as_str_map) {
        check_unknown_keys(live, LIVE_OBSERVATION_KEYS, "live_observation", &mut violations);
        require_non_empty(
            live,
            "digest",
            "missing_observation_digest",
            "live_observation",
            &mut violations,
        );
        match string_field(live, "candidate_state") {
            Some(state) if CANDIDATE_STATES.contains(&state) => {
                if state == "observed" {
                    require_non_empty(
                        live,
                        "candidate_identity",
                        "incomplete_observation",
                        "live_observation",
                        &mut violations,
                    );
                } else if live.contains_key("candidate_identity")
                    || live.contains_key("collision_state")
                {
                    violations.push(Violation::new(
                        "fabricated_observation",
                        "live_observation: not_observed must not fabricate candidate or collision facts".to_string(),
                    ));
                }
            }
            Some(state) => violations.push(Violation::new(
                "unknown_candidate_state",
                format!("live_observation: unknown candidate state {state}"),
            )),
            None => violations.push(Violation::new(
                "missing_candidate_state",
                "live_observation: candidate_state is required".to_string(),
            )),
        }
    }

    validate_authorities(root, &mut violations);
    validate_surfaces(root, &mut violations);
    validate_proof(root, &mut violations);
    validate_verification(root, &mut violations);
    validate_delivery(root, &mut violations);
    validate_stop(root, &mut violations);

    violations
}

/// Authority map: an authority described as landed must carry a currentness
/// proof anchored to the observed tree; historical branches never become
/// current authorities; one ref never occupies two groups.
fn validate_authorities(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(authorities) = root.get("authorities").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(authorities, AUTHORITY_GROUPS, "authorities", violations);
    let observed_tree = root
        .get("repository")
        .and_then(as_str_map)
        .and_then(|repository| string_field(repository, "observed_tree"))
        .map(str::to_string);
    let mut seen_refs: BTreeMap<String, String> = BTreeMap::new();
    for group in AUTHORITY_GROUPS {
        for entry in object_array(authorities, group) {
            let where_ = format!("authorities.{group}");
            check_unknown_keys(entry, AUTHORITY_ENTRY_KEYS, &where_, violations);
            let reference =
                require_non_empty(entry, "ref", "missing_authority_ref", &where_, violations);
            require_non_empty(entry, "subject", "missing_authority_subject", &where_, violations);
            let proof_kind = string_field(entry, "proof_kind");
            match proof_kind {
                Some(kind) if PROOF_KINDS.contains(&kind) => {
                    if *group == "must_be_current" && !CURRENTNESS_PROOF_KINDS.contains(&kind) {
                        violations.push(Violation::new(
                            "stale_authority",
                            format!("{where_}: proof kind {kind} cannot anchor a landed authority"),
                        ));
                    }
                    // Digest-anchored landed authorities carry the supplied
                    // digest; a live-observation anchor additionally requires
                    // the packet's live observation to exist and be observed.
                    if *group == "must_be_current"
                        && matches!(
                            kind,
                            "spec_disposition" | "frontier_digest" | "live_observation"
                        )
                        && string_field(entry, "proof_digest").is_none()
                    {
                        violations.push(Violation::new(
                            "authority_currentness_missing",
                            format!("{where_}: {kind} landed authority must carry its supplied proof_digest"),
                        ));
                    }
                    if *group == "must_be_current"
                        && kind == "live_observation"
                        && root
                            .get("live_observation")
                            .and_then(as_str_map)
                            .and_then(|live| string_field(live, "candidate_state"))
                            != Some("observed")
                    {
                        violations.push(Violation::new(
                            "authority_currentness_missing",
                            format!("{where_}: live_observation landed authority requires an observed live_observation section"),
                        ));
                    }
                    if kind == "current_tree_probe" || *group == "may_be_mined" {
                        match string_field(entry, "proof_tree") {
                            Some(tree) => {
                                if *group == "must_be_current"
                                    && kind == "current_tree_probe"
                                    && observed_tree.as_deref() != Some(tree)
                                {
                                    violations.push(Violation::new(
                                        "stale_authority",
                                        format!(
                                            "{where_}: currentness proof tree {tree} is not the observed tree {}",
                                            observed_tree.as_deref().unwrap_or("?")
                                        ),
                                    ));
                                }
                            }
                            None => violations.push(Violation::new(
                                "authority_currentness_missing",
                                format!("{where_}: {kind} authority must anchor its proof tree"),
                            )),
                        }
                    }
                }
                Some(kind) => violations.push(Violation::new(
                    "unknown_proof_kind",
                    format!("{where_}: unknown proof kind {kind}"),
                )),
                None => violations.push(Violation::new(
                    "missing_proof_kind",
                    format!("{where_}: proof_kind is required"),
                )),
            }
            if let Some(reference) = reference {
                if let Some(previous) = seen_refs.get(&reference) {
                    violations.push(Violation::new(
                        "duplicate_authority_ref",
                        format!(
                            "{where_}: authority {reference} also appears in authorities.{previous}"
                        ),
                    ));
                } else {
                    seen_refs.insert(reference, (*group).to_string());
                }
            }
        }
    }
}

/// Bounded repository surfaces: overlapping writer/conflict keys fail closed
/// instead of staying hidden, and a writing actor names its slots.
fn validate_surfaces(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(surfaces) = root.get("surfaces").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(surfaces, SURFACE_KEYS, "surfaces", violations);
    let writer_slots = object_array(surfaces, "writer_slots");
    let mut keys: BTreeSet<String> = BTreeSet::new();
    let mut path_owners: BTreeMap<String, String> = BTreeMap::new();
    for slot in &writer_slots {
        check_unknown_keys(slot, &["key", "paths"], "surfaces.writer_slots", violations);
        let key = require_non_empty(
            slot,
            "key",
            "missing_writer_key",
            "surfaces.writer_slots",
            violations,
        );
        if let Some(key_value) = key.as_deref()
            && !keys.insert(key_value.to_string())
        {
            violations.push(Violation::new(
                "overlapping_writer_keys",
                format!("surfaces.writer_slots: duplicate writer key {key_value}"),
            ));
        }
        let mut slot_paths: BTreeSet<String> = BTreeSet::new();
        for path in string_array(slot, "paths") {
            if !slot_paths.insert(path.clone()) {
                violations.push(Violation::new(
                    "overlapping_writer_keys",
                    format!("surfaces.writer_slots: path {path} listed twice in one slot"),
                ));
            }
            if let Some(owner) = path_owners.get(&path) {
                violations.push(Violation::new(
                    "overlapping_writer_keys",
                    format!(
                        "surfaces.writer_slots: path {path} claimed by writer keys {owner} and {}",
                        key.as_deref().unwrap_or("?")
                    ),
                ));
            } else {
                path_owners.insert(path, key.clone().unwrap_or_else(|| "?".to_string()));
            }
        }
        if string_array(slot, "paths").is_empty() {
            violations.push(Violation::new(
                "missing_writer_paths",
                "surfaces.writer_slots: each slot names at least one path".to_string(),
            ));
        }
    }
    let write_boundary = root
        .get("actor")
        .and_then(as_str_map)
        .and_then(|actor| string_field(actor, "write_boundary"));
    if write_boundary == Some("repository_candidate_branch") && writer_slots.is_empty() {
        violations.push(Violation::new(
            "missing_writer_slots",
            "surfaces: a repository-writing actor must declare its writer slots".to_string(),
        ));
    }
}

/// Shift-left proof packet: at least one stage-specific discriminating
/// falsifier, a positive discriminator, and mutation controls. Generic
/// statements fail closed.
fn validate_proof(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(proof) = root.get("proof").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(proof, PROOF_KEYS, "proof", violations);
    let falsifiers = object_array(proof, "falsifiers");
    if falsifiers.is_empty() {
        violations.push(Violation::new(
            "missing_falsifier",
            "proof: at least one discriminating falsifier is required".to_string(),
        ));
    }
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for falsifier in &falsifiers {
        check_unknown_keys(
            falsifier,
            &["id", "stage", "statement"],
            "proof.falsifiers",
            violations,
        );
        let id = require_non_empty(
            falsifier,
            "id",
            "missing_falsifier_id",
            "proof.falsifiers",
            violations,
        );
        if let Some(id) = id
            && !ids.insert(id.clone())
        {
            violations.push(Violation::new(
                "duplicate_falsifier_id",
                format!("proof.falsifiers: duplicate id {id}"),
            ));
        }
        require_non_empty(
            falsifier,
            "stage",
            "missing_falsifier_stage",
            "proof.falsifiers",
            violations,
        );
        if let Some(statement) = string_field(falsifier, "statement") {
            if is_generic_statement(statement) {
                violations.push(Violation::new(
                    "generic_falsifier",
                    format!("proof.falsifiers: generic statement {statement:?} is not a falsifier"),
                ));
            }
        } else {
            violations.push(Violation::new(
                "missing_falsifier_statement",
                "proof.falsifiers: statement is required".to_string(),
            ));
        }
    }
    require_non_empty(
        proof,
        "positive_discriminator",
        "missing_positive_discriminator",
        "proof",
        violations,
    );
    if string_array(proof, "mutation_controls").is_empty() {
        violations.push(Violation::new(
            "missing_mutation_control",
            "proof: at least one negative/mutation control is required".to_string(),
        ));
    }
    if string_array(proof, "terminal_outcomes").is_empty() {
        violations.push(Violation::new(
            "missing_terminal_outcomes",
            "proof: terminal outcome vocabulary is required".to_string(),
        ));
    }
}

/// Verification route: focused proof first, repository-owned command
/// identities, diff hygiene, and a second-run no-diff generation step for
/// every touched generated artifact.
fn validate_verification(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(verification) = root.get("verification").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(verification, &["steps"], "verification", violations);
    let steps = object_array(verification, "steps");
    if steps.is_empty() {
        violations.push(Violation::new(
            "missing_verification_steps",
            "verification: at least one step is required".to_string(),
        ));
    }
    for (index, step) in steps.iter().enumerate() {
        let where_ = format!("verification.steps[{index}]");
        check_unknown_keys(step, VERIFICATION_STEP_KEYS, &where_, violations);
        require_non_empty(step, "command_id", "missing_command_id", &where_, violations);
        if let Some(command) = string_field(step, "command") {
            if is_generic_statement(command) {
                violations.push(Violation::new(
                    "generic_verification",
                    format!("{where_}: generic command {command:?} is not a verification route"),
                ));
            }
        } else {
            violations
                .push(Violation::new("missing_command", format!("{where_}: command is required")));
        }
        match string_field(step, "scope") {
            Some(scope) if VERIFICATION_SCOPES.contains(&scope) => {}
            Some(scope) => violations.push(Violation::new(
                "unknown_verification_scope",
                format!("{where_}: unknown scope {scope}"),
            )),
            None => violations.push(Violation::new(
                "missing_verification_scope",
                format!("{where_}: scope is required"),
            )),
        }
    }
    if let Some(first) = steps.first()
        && string_field(first, "scope") != Some("focused_proof")
    {
        violations.push(Violation::new(
            "verification_not_focused_first",
            "verification: focused proof must come first".to_string(),
        ));
    }
    if !steps.iter().any(|step| string_field(step, "scope") == Some("diff_check")) {
        violations.push(Violation::new(
            "missing_diff_check",
            "verification: git diff --check hygiene is required".to_string(),
        ));
    }
    let generated = root
        .get("surfaces")
        .and_then(as_str_map)
        .map(|surfaces| string_array(surfaces, "generated_artifacts"))
        .unwrap_or_default();
    if !generated.is_empty() {
        let covers_generation = steps.iter().any(|step| {
            string_field(step, "scope") == Some("generation")
                && step.get("second_run_no_diff").and_then(Value::as_bool) == Some(true)
        });
        if !covers_generation {
            violations.push(Violation::new(
                "missing_generated_obligation",
                "verification: touched generated artifacts require a second-run no-diff generation step".to_string(),
            ));
        }
    }
    let docs = root
        .get("surfaces")
        .and_then(as_str_map)
        .map(|surfaces| string_array(surfaces, "docs_fragments"))
        .unwrap_or_default();
    if !docs.is_empty()
        && !steps.iter().any(|step| string_field(step, "scope") == Some("docs_check"))
    {
        violations.push(Violation::new(
            "missing_docs_obligation",
            "verification: touched docs fragments require a docs check step".to_string(),
        ));
    }
}

/// Delivery: the repository's review-forward result is a reviewable draft PR
/// and handoff; a local diff is not delivered.
fn validate_delivery(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(delivery) = root.get("delivery").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(delivery, DELIVERY_KEYS, "delivery", violations);
    if string_field(delivery, "definition") != Some(DELIVERY_DEFINITION) {
        violations.push(Violation::new(
            "undelivered_claim",
            format!("delivery: definition must be {DELIVERY_DEFINITION}"),
        ));
    }
    require_non_empty(
        delivery,
        "branch_suggestion",
        "missing_delivery_fields",
        "delivery",
        violations,
    );
    require_non_empty(
        delivery,
        "pr_title_suggestion",
        "missing_delivery_fields",
        "delivery",
        violations,
    );
    if string_array(delivery, "pr_body_fields").is_empty() {
        violations.push(Violation::new(
            "missing_delivery_fields",
            "delivery: pr_body_fields is required".to_string(),
        ));
    }
    match string_field(delivery, "old_path_disposition") {
        Some(disposition) if OLD_PATH_DISPOSITIONS.contains(&disposition) => {}
        Some(disposition) => violations.push(Violation::new(
            "unknown_old_path_disposition",
            format!("delivery: unknown old path disposition {disposition}"),
        )),
        None => violations.push(Violation::new(
            "missing_delivery_fields",
            "delivery: old_path_disposition is required".to_string(),
        )),
    }
    require_non_empty(
        delivery,
        "remaining_blocker_or_next",
        "missing_delivery_fields",
        "delivery",
        violations,
    );
}

/// Stop boundary: terminal actions beyond the reviewable draft PR require an
/// explicit authority and a matching write boundary.
fn validate_stop(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(stop) = root.get("stop").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(stop, STOP_KEYS, "stop", violations);
    if string_array(stop, "conditions").is_empty() {
        violations.push(Violation::new(
            "missing_stop_conditions",
            "stop: at least one stop condition is required".to_string(),
        ));
    }
    let actions = string_array(stop, "permitted_terminal_actions");
    for action in &actions {
        if !PERMITTED_TERMINAL_ACTIONS.contains(&action.as_str()) {
            violations.push(Violation::new(
                "unknown_terminal_action",
                format!("stop: unknown terminal action {action}"),
            ));
        }
    }
    if !actions.is_empty() {
        let authority = string_field(stop, "authority");
        if authority.is_none() {
            violations.push(Violation::new(
                "unauthorized_stop_boundary",
                "stop: terminal actions require an explicit authority reference".to_string(),
            ));
        } else if let Some(reference) = authority {
            let resolves = root
                .get("authorities")
                .and_then(as_str_map)
                .map(|authorities| {
                    ["must_be_current", "external_manual_owner"].iter().any(|group| {
                        object_array(authorities, group)
                            .iter()
                            .any(|entry| string_field(entry, "ref") == Some(reference))
                    })
                })
                .unwrap_or(false);
            if !resolves {
                violations.push(Violation::new(
                    "unresolved_stop_authority",
                    format!("stop: terminal-action authority {reference:?} does not resolve in the authority map"),
                ));
            }
        }
        let write_boundary = root
            .get("actor")
            .and_then(as_str_map)
            .and_then(|actor| string_field(actor, "write_boundary"));
        if actions.iter().any(|action| action == "merge_after_review")
            && write_boundary != Some("repository_candidate_branch")
        {
            violations.push(Violation::new(
                "unauthorized_stop_boundary",
                "stop: merge_after_review requires a repository_candidate_branch write boundary"
                    .to_string(),
            ));
        }
    }
}

/// Canonical semantic value: strips non-semantic metadata and sorts every
/// order-insensitive array. Verification step order is semantic (focused
/// proof first) and is preserved.
fn canonical_value(doc: &Value) -> Value {
    let Some(root) = as_str_map(doc) else {
        return Value::Null;
    };
    let mut canonical = Map::new();
    for (key, value) in root {
        if key == "metadata" {
            continue;
        }
        let ordered = match value {
            Value::Object(work) if key == "work" => {
                canonical_with_sorted_sets(work, &["unproven", "non_goals", "successors"])
            }
            Value::Object(frontier) if key == "frontier" => {
                let mut sorted = Map::new();
                for (frontier_key, frontier_value) in frontier {
                    let ordered = match frontier_value {
                        Value::Array(items) if frontier_key == "blocking_edges" => {
                            sort_by_field(items, "edge")
                        }
                        other => other.clone(),
                    };
                    sorted.insert(frontier_key.clone(), ordered);
                }
                Value::Object(sorted)
            }
            Value::Object(authorities) if key == "authorities" => {
                let mut sorted = Map::new();
                for (group, entries) in authorities {
                    let ordered = match entries {
                        Value::Array(items) => sort_by_field(items, "ref"),
                        other => other.clone(),
                    };
                    sorted.insert(group.clone(), ordered);
                }
                Value::Object(sorted)
            }
            Value::Object(surfaces) if key == "surfaces" => {
                let mut sorted = Map::new();
                for (surface_key, surface_value) in surfaces {
                    let ordered = match surface_value {
                        Value::Array(items)
                            if matches!(
                                surface_key.as_str(),
                                "implementation_paths"
                                    | "tests_fixtures"
                                    | "generated_artifacts"
                                    | "docs_fragments"
                                    | "forbidden_adjacent"
                            ) =>
                        {
                            sorted_string_value(items)
                        }
                        Value::Array(items) if surface_key == "writer_slots" => {
                            let mut slots: Vec<Value> = items
                                .iter()
                                .map(|slot| match as_str_map(slot) {
                                    Some(slot) => {
                                        let mut canonical_slot = slot.clone();
                                        if let Some(paths) = sorted_string_set(slot, "paths") {
                                            canonical_slot.insert("paths".to_string(), paths);
                                        }
                                        Value::Object(canonical_slot)
                                    }
                                    None => slot.clone(),
                                })
                                .collect();
                            slots.sort_by_key(|slot| {
                                slot.as_object()
                                    .and_then(|object| object.get("key"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string()
                            });
                            Value::Array(slots)
                        }
                        other => other.clone(),
                    };
                    sorted.insert(surface_key.clone(), ordered);
                }
                Value::Object(sorted)
            }
            Value::Object(proof) if key == "proof" => {
                let mut sorted = Map::new();
                for (proof_key, proof_value) in proof {
                    let ordered = match proof_value {
                        Value::Array(items) if proof_key == "falsifiers" => {
                            sort_by_field(items, "id")
                        }
                        Value::Array(items)
                            if proof_key == "mutation_controls"
                                || proof_key == "terminal_outcomes" =>
                        {
                            sorted_string_value(items)
                        }
                        other => other.clone(),
                    };
                    sorted.insert(proof_key.clone(), ordered);
                }
                Value::Object(sorted)
            }
            Value::Object(delivery) if key == "delivery" => {
                canonical_with_sorted_sets(delivery, &["pr_body_fields", "limitations"])
            }
            Value::Object(stop) if key == "stop" => {
                canonical_with_sorted_sets(stop, &["conditions", "permitted_terminal_actions"])
            }
            // verification: step order is the semantic route; keep it.
            other => other.clone(),
        };
        canonical.insert(key.clone(), ordered);
    }
    Value::Object(canonical)
}

fn canonical_with_sorted_sets(object: &Map<String, Value>, set_fields: &[&str]) -> Value {
    let mut canonical = Map::new();
    for (key, value) in object {
        let ordered = match value {
            Value::Array(items) if set_fields.contains(&key.as_str()) => sorted_string_value(items),
            other => other.clone(),
        };
        canonical.insert(key.clone(), ordered);
    }
    Value::Object(canonical)
}

fn canonical_form(doc: &Value) -> String {
    canonical_value(doc).to_string()
}

fn sorted_string_value(items: &[Value]) -> Value {
    let mut sorted: Vec<String> =
        items.iter().filter_map(Value::as_str).map(str::to_string).collect();
    sorted.sort();
    sorted.dedup();
    Value::Array(sorted.into_iter().map(Value::String).collect())
}

fn sorted_string_set(object: &Map<String, Value>, field: &str) -> Option<Value> {
    Some(sorted_string_value(object.get(field)?.as_array()?))
}

fn sort_by_field(items: &[Value], field: &str) -> Value {
    let mut sorted: Vec<&Value> = items.iter().collect();
    sorted.sort_by_key(|item| {
        item.as_object()
            .and_then(|object| object.get(field))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    Value::Array(sorted.into_iter().cloned().collect())
}

/// Anchors that every projection must preserve verbatim. A rendering that
/// drops any of them has dropped packet semantics, not just syntax (#10872
/// negative control 12).
fn semantic_anchors(doc: &Value) -> Vec<String> {
    let Some(root) = as_str_map(doc) else {
        return Vec::new();
    };
    let mut anchors = Vec::new();
    let mut push = |value: Option<&str>| {
        if let Some(value) = value {
            anchors.push(value.to_string());
        }
    };
    if let Some(repository) = root.get("repository").and_then(as_str_map) {
        push(string_field(repository, "observed_tree"));
    }
    if let Some(work) = root.get("work").and_then(as_str_map) {
        for key in [
            "owning_issue",
            "node_id",
            "proposition_id",
            "profile",
            "result_sentence",
            "claim_ceiling",
        ] {
            push(string_field(work, key));
        }
    }
    if let Some(frontier) = root.get("frontier").and_then(as_str_map) {
        push(string_field(frontier, "digest"));
        for edge in object_array(frontier, "blocking_edges") {
            push(string_field(edge, "edge"));
            push(string_field(edge, "reason"));
        }
    }
    if let Some(live) = root.get("live_observation").and_then(as_str_map) {
        push(string_field(live, "candidate_state"));
        push(string_field(live, "candidate_identity"));
    }
    if let Some(authorities) = root.get("authorities").and_then(as_str_map) {
        for group in AUTHORITY_GROUPS {
            for entry in object_array(authorities, group) {
                push(string_field(entry, "ref"));
            }
        }
    }
    if let Some(proof) = root.get("proof").and_then(as_str_map) {
        push(string_field(proof, "positive_discriminator"));
        for falsifier in object_array(proof, "falsifiers") {
            push(string_field(falsifier, "id"));
            push(string_field(falsifier, "statement"));
        }
    }
    if let Some(verification) = root.get("verification").and_then(as_str_map) {
        for step in object_array(verification, "steps") {
            push(string_field(step, "command_id"));
            push(string_field(step, "command"));
        }
    }
    if let Some(delivery) = root.get("delivery").and_then(as_str_map) {
        push(string_field(delivery, "definition"));
    }
    if let Some(stop) = root.get("stop").and_then(as_str_map) {
        for condition in string_array(stop, "conditions") {
            anchors.push(condition);
        }
    }
    anchors
}

/// Check one rendering preserves every semantic anchor.
fn projection_completeness_violations(doc: &Value, rendering: &str, label: &str) -> Vec<Violation> {
    semantic_anchors(doc)
        .into_iter()
        .filter(|anchor| !rendering.contains(anchor.as_str()))
        .map(|anchor| {
            Violation::new(
                "projection_dropped_semantics",
                format!("{label}: dropped semantic anchor {anchor:?}"),
            )
        })
        .collect()
}

/// Render the canonical machine projection (canonical JSON bytes). Callers
/// must validate first; rendering an invalid packet fails closed.
fn render_machine(doc: &Value) -> String {
    canonical_form(doc)
}

/// Render the phone-readable Markdown projection. Every section preserves
/// the semantic packet verbatim.
fn render_markdown(doc: &Value) -> String {
    let Some(root) = as_str_map(doc) else {
        return String::new();
    };
    let str_of = |section: &str, key: &str| {
        root.get(section)
            .and_then(as_str_map)
            .and_then(|object| string_field(object, key))
            .unwrap_or("")
    };
    let mut out = String::new();
    out.push_str("# Builder packet: ");
    out.push_str(str_of("work", "node_id"));
    out.push_str("\n\n## Work identity\n\n");
    out.push_str(&format!("- packet: {}\n", string_field(root, "packet_id").unwrap_or("")));
    out.push_str(&format!(
        "- repository: {} @ {}\n",
        str_of("repository", "name"),
        str_of("repository", "observed_tree")
    ));
    out.push_str(&format!(
        "- programme: {} {} v{}\n",
        str_of("programme", "name"),
        str_of("programme", "manifest"),
        str_of("programme", "manifest_version")
    ));
    out.push_str(&format!(
        "- node/proposition/profile: {} / {} / {}\n",
        str_of("work", "node_id"),
        str_of("work", "proposition_id"),
        str_of("work", "profile")
    ));
    out.push_str(&format!("- owning issue: {}\n", str_of("work", "owning_issue")));
    out.push_str(&format!(
        "- actor: {} (write boundary: {})\n",
        str_of("actor", "role"),
        str_of("actor", "write_boundary")
    ));
    if let Some(frontier) = root.get("frontier").and_then(as_str_map) {
        out.push_str(&format!(
            "- frontier: {} ({})\n",
            string_field(frontier, "decision").unwrap_or(""),
            string_field(frontier, "digest").unwrap_or("")
        ));
        for edge in object_array(frontier, "blocking_edges") {
            out.push_str(&format!(
                "  - blocked by {} ({})\n",
                string_field(edge, "edge").unwrap_or(""),
                string_field(edge, "reason").unwrap_or("")
            ));
        }
    }
    match root
        .get("live_observation")
        .and_then(as_str_map)
        .and_then(|live| string_field(live, "candidate_state").map(str::to_string))
    {
        Some(state) => {
            out.push_str(&format!("- candidate state: {state}\n"));
            if let Some(live) = root.get("live_observation").and_then(as_str_map) {
                if let Some(identity) = string_field(live, "candidate_identity") {
                    out.push_str(&format!("  - candidate: {identity}\n"));
                }
                if let Some(collision) = string_field(live, "collision_state") {
                    out.push_str(&format!("  - collision: {collision}\n"));
                }
            }
        }
        None => out.push_str("- candidate state: not_observed (no live observation supplied)\n"),
    }
    out.push_str(&format!(
        "- current-tree probe: {} -> {}\n",
        str_of("current_tree_probe", "subject"),
        str_of("current_tree_probe", "result")
    ));

    out.push_str("\n## Result and claim ceiling\n\n");
    out.push_str(&format!("{}\n\n", str_of("work", "result_sentence")));
    out.push_str(&format!("Claim ceiling: {}\n\n", str_of("work", "claim_ceiling")));
    for item in root
        .get("work")
        .and_then(as_str_map)
        .map(|work| string_array(work, "unproven"))
        .unwrap_or_default()
    {
        out.push_str(&format!("- remains unproven: {item}\n"));
    }
    for item in root
        .get("work")
        .and_then(as_str_map)
        .map(|work| string_array(work, "non_goals"))
        .unwrap_or_default()
    {
        out.push_str(&format!("- non-goal: {item}\n"));
    }

    out.push_str("\n## Authorities\n\n");
    if let Some(authorities) = root.get("authorities").and_then(as_str_map) {
        for group in AUTHORITY_GROUPS {
            for entry in object_array(authorities, group) {
                out.push_str(&format!(
                    "- [{group}] {} — {}\n",
                    string_field(entry, "ref").unwrap_or(""),
                    string_field(entry, "subject").unwrap_or("")
                ));
            }
        }
    }

    out.push_str("\n## Bounded repository surfaces\n\n");
    if let Some(surfaces) = root.get("surfaces").and_then(as_str_map) {
        for (label, key) in [
            ("implementation", "implementation_paths"),
            ("tests/fixtures", "tests_fixtures"),
            ("generated", "generated_artifacts"),
            ("docs", "docs_fragments"),
            ("forbidden adjacent", "forbidden_adjacent"),
        ] {
            for item in string_array(surfaces, key) {
                out.push_str(&format!("- {label}: {item}\n"));
            }
        }
        for slot in object_array(surfaces, "writer_slots") {
            out.push_str(&format!(
                "- writer slot {}: {}\n",
                string_field(slot, "key").unwrap_or(""),
                string_array(slot, "paths").join(", ")
            ));
        }
    }

    out.push_str("\n## Shift-left proof\n\n");
    if let Some(proof) = root.get("proof").and_then(as_str_map) {
        for falsifier in object_array(proof, "falsifiers") {
            out.push_str(&format!(
                "- falsifier {} [{}]: {}\n",
                string_field(falsifier, "id").unwrap_or(""),
                string_field(falsifier, "stage").unwrap_or(""),
                string_field(falsifier, "statement").unwrap_or("")
            ));
        }
        out.push_str(&format!(
            "- positive discriminator: {}\n",
            string_field(proof, "positive_discriminator").unwrap_or("")
        ));
        for control in string_array(proof, "mutation_controls") {
            out.push_str(&format!("- mutation control: {control}\n"));
        }
        for outcome in string_array(proof, "terminal_outcomes") {
            out.push_str(&format!("- terminal outcome: {outcome}\n"));
        }
    }

    out.push_str("\n## Verification route\n\n");
    if let Some(verification) = root.get("verification").and_then(as_str_map) {
        for (index, step) in object_array(verification, "steps").iter().enumerate() {
            let no_diff = if step.get("second_run_no_diff").and_then(Value::as_bool) == Some(true) {
                " (second run: no diff)"
            } else {
                ""
            };
            out.push_str(&format!(
                "{}. [{}] {} — `{}`{}\n",
                index + 1,
                string_field(step, "scope").unwrap_or(""),
                string_field(step, "command_id").unwrap_or(""),
                string_field(step, "command").unwrap_or(""),
                no_diff
            ));
        }
    }

    out.push_str("\n## Delivery and handoff\n\n");
    if let Some(delivery) = root.get("delivery").and_then(as_str_map) {
        out.push_str(&format!(
            "Delivered means: {}\n\n",
            string_field(delivery, "definition").unwrap_or("")
        ));
        out.push_str(&format!(
            "- branch: {}\n- PR title: {}\n",
            string_field(delivery, "branch_suggestion").unwrap_or(""),
            string_field(delivery, "pr_title_suggestion").unwrap_or("")
        ));
        for field in string_array(delivery, "pr_body_fields") {
            out.push_str(&format!("- PR body field: {field}\n"));
        }
        for limitation in string_array(delivery, "limitations") {
            out.push_str(&format!("- limitation: {limitation}\n"));
        }
        out.push_str(&format!(
            "- next: {}\n",
            string_field(delivery, "remaining_blocker_or_next").unwrap_or("")
        ));
    }

    out.push_str("\n## Stop conditions\n\n");
    if let Some(stop) = root.get("stop").and_then(as_str_map) {
        for condition in string_array(stop, "conditions") {
            out.push_str(&format!("- stop: {condition}\n"));
        }
        let actions = string_array(stop, "permitted_terminal_actions");
        if actions.is_empty() {
            out.push_str("- no terminal action beyond the reviewable draft PR is permitted\n");
        } else {
            out.push_str(&format!(
                "- permitted terminal actions: {} (authority: {})\n",
                actions.join(", "),
                string_field(stop, "authority").unwrap_or("")
            ));
        }
    }
    out
}

/// Render the compact agent prompt. Compact means fewer decorations, never
/// fewer semantics: every anchor of the Markdown projection is preserved.
fn render_compact(doc: &Value) -> String {
    let Some(root) = as_str_map(doc) else {
        return String::new();
    };
    let str_of = |section: &str, key: &str| {
        root.get(section)
            .and_then(as_str_map)
            .and_then(|object| string_field(object, key))
            .unwrap_or("")
    };
    let mut out = String::new();
    out.push_str("WORK PACKET ");
    out.push_str(str_of("work", "node_id"));
    out.push_str(" | ");
    out.push_str(&format!(
        "{}/{}@{}",
        str_of("repository", "name"),
        str_of("programme", "manifest"),
        str_of("repository", "observed_tree")
    ));
    out.push('\n');
    out.push_str(&format!(
        "RESULT: {}\nCEILING: {}\n",
        str_of("work", "result_sentence"),
        str_of("work", "claim_ceiling")
    ));
    out.push_str(&format!(
        "ISSUE: {} | PROFILE: {} | ACTOR: {} (writes: {})\n",
        str_of("work", "owning_issue"),
        str_of("work", "profile"),
        str_of("actor", "role"),
        str_of("actor", "write_boundary")
    ));
    if let Some(frontier) = root.get("frontier").and_then(as_str_map) {
        out.push_str(&format!(
            "FRONTIER: {} {}\n",
            string_field(frontier, "decision").unwrap_or(""),
            string_field(frontier, "digest").unwrap_or("")
        ));
        for edge in object_array(frontier, "blocking_edges") {
            out.push_str(&format!(
                "BLOCKED-BY: {} ({})\n",
                string_field(edge, "edge").unwrap_or(""),
                string_field(edge, "reason").unwrap_or("")
            ));
        }
    }
    match root.get("live_observation").and_then(as_str_map) {
        Some(live) => {
            out.push_str(&format!(
                "CANDIDATE: {}\n",
                string_field(live, "candidate_state").unwrap_or("")
            ));
            if let Some(identity) = string_field(live, "candidate_identity") {
                out.push_str(&format!("CANDIDATE-ID: {identity}\n"));
            }
        }
        None => out.push_str("CANDIDATE: not_observed\n"),
    }
    if let Some(authorities) = root.get("authorities").and_then(as_str_map) {
        let mut lines: Vec<String> = Vec::new();
        for group in AUTHORITY_GROUPS {
            for entry in object_array(authorities, group) {
                lines.push(format!(
                    "AUTHORITY[{group}]: {}",
                    string_field(entry, "ref").unwrap_or("")
                ));
            }
        }
        out.push_str("AUTHORITIES:\n");
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    if let Some(proof) = root.get("proof").and_then(as_str_map) {
        for falsifier in object_array(proof, "falsifiers") {
            out.push_str(&format!(
                "FALSIFIER {}: {}\n",
                string_field(falsifier, "id").unwrap_or(""),
                string_field(falsifier, "statement").unwrap_or("")
            ));
        }
        out.push_str(&format!(
            "POSITIVE: {}\n",
            string_field(proof, "positive_discriminator").unwrap_or("")
        ));
    }
    if let Some(verification) = root.get("verification").and_then(as_str_map) {
        out.push_str("VERIFY (in order):\n");
        for step in object_array(verification, "steps") {
            let no_diff = if step.get("second_run_no_diff").and_then(Value::as_bool) == Some(true) {
                "; second run must produce no diff"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {}: {} [{}]{}\n",
                string_field(step, "command_id").unwrap_or(""),
                string_field(step, "command").unwrap_or(""),
                string_field(step, "scope").unwrap_or(""),
                no_diff
            ));
        }
    }
    if let Some(stop) = root.get("stop").and_then(as_str_map) {
        for condition in string_array(stop, "conditions") {
            out.push_str(&format!("STOP: {condition}\n"));
        }
        let actions = string_array(stop, "permitted_terminal_actions");
        if actions.is_empty() {
            out.push_str("TERMINAL: none — stop at the reviewable draft PR\n");
        } else {
            out.push_str(&format!(
                "TERMINAL: {} (authority: {})\n",
                actions.join(","),
                string_field(stop, "authority").unwrap_or("")
            ));
        }
    }
    out.push_str(&format!("DELIVERED-MEANS: {}\n", str_of("delivery", "definition")));
    out
}

/// Render one projection of a caller-supplied packet document, fail-closed:
/// any validation violation aborts rendering instead of producing plausible
/// prose. Projections go to stdout only; no repository file is written.
pub fn render_to_string(doc: &Value, projection: PacketProjection) -> Result<String> {
    let violations = validate_document(doc);
    if !violations.is_empty() {
        let codes: Vec<&str> = violations.iter().map(|violation| violation.code.as_str()).collect();
        bail!(
            "packet failed validation; refusing to render plausible prose ({}): {:?}",
            codes.len(),
            codes
        );
    }
    Ok(match projection {
        PacketProjection::Machine => render_machine(doc),
        PacketProjection::Markdown => render_markdown(doc),
        PacketProjection::Compact => render_compact(doc),
    })
}

fn validate_schema_file(root: &Path) -> Result<Vec<String>> {
    let path = root.join(SCHEMA_PATH);
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let schema: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut violations = Vec::new();
    let Some(object) = as_str_map(&schema) else {
        bail!("schema file must be an object");
    };
    if string_field(object, "$id") != Some(SCHEMA_ID) {
        violations.push(format!("$id must be {SCHEMA_ID}"));
    }
    let enum_of = |path_segments: &[&str]| -> Option<Vec<String>> {
        let mut value = &schema;
        for segment in path_segments {
            value = value.get(*segment)?;
        }
        value
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
    };
    let expected: &[(&[&str], &[&str], &str)] = &[
        (&["$defs", "node_kind", "enum"], NODE_KINDS, "node kinds"),
        (&["$defs", "frontier_decision", "enum"], FRONTIER_DECISIONS, "frontier decisions"),
        (&["$defs", "authority_group", "enum"], AUTHORITY_GROUPS, "authority groups"),
        (&["$defs", "proof_kind", "enum"], PROOF_KINDS, "proof kinds"),
        (&["$defs", "candidate_state", "enum"], CANDIDATE_STATES, "candidate states"),
        (&["$defs", "verification_scope", "enum"], VERIFICATION_SCOPES, "verification scopes"),
        (
            &["$defs", "old_path_disposition", "enum"],
            OLD_PATH_DISPOSITIONS,
            "old path dispositions",
        ),
        (
            &["$defs", "permitted_terminal_action", "enum"],
            PERMITTED_TERMINAL_ACTIONS,
            "permitted terminal actions",
        ),
        (&["$defs", "write_boundary", "enum"], WRITE_BOUNDARIES, "write boundaries"),
    ];
    for (path_segments, expected_values, label) in expected {
        match enum_of(path_segments) {
            Some(values)
                if values.iter().map(String::as_str).eq(expected_values.iter().copied()) => {}
            Some(values) => violations.push(format!(
                "schema {label} enum drifted from the pinned closed vocabulary: {values:?}"
            )),
            None => violations.push(format!(
                "schema is missing the {} enum at {}",
                label,
                path_segments.join(".")
            )),
        }
    }
    Ok(violations)
}

fn load_json(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn violation_codes(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(|violation| violation.code.as_str()).collect()
}

fn golden_path(stem: &str, projection: PacketProjection) -> String {
    match projection {
        PacketProjection::Machine => format!("{GOLDEN_DIR}/{stem}.machine.json"),
        PacketProjection::Markdown => format!("{GOLDEN_DIR}/{stem}.markdown.md"),
        PacketProjection::Compact => format!("{GOLDEN_DIR}/{stem}.compact.txt"),
    }
}

/// Entry point: validate the closed contract schema, the valid fixtures, the
/// fail-closed negative controls, the canonical-semantics control, and the
/// deterministic golden projections. `update_golden` rewrites the golden
/// vectors (an explicit writer action, never live packet state).
pub fn run(update_golden: bool) -> Result<()> {
    let root = project_root()?;
    let mut failures: Vec<String> = Vec::new();

    for violation in validate_schema_file(&root)? {
        failures.push(format!("{SCHEMA_PATH}: {violation}"));
    }

    let fixture_dir = root.join(FIXTURE_DIR);
    for name in VALID_FIXTURES {
        let doc = load_json(&fixture_dir.join(name))?;
        let violations = validate_document(&doc);
        if !violations.is_empty() {
            failures.push(format!(
                "{name}: expected a valid packet, got {:?}",
                violation_codes(&violations)
            ));
            continue;
        }
        let stem = name.trim_end_matches(".v1.json");
        let machine = render_machine(&doc);
        let markdown = render_markdown(&doc);
        let compact = render_compact(&doc);
        // Determinism: a second render produces identical bytes.
        if render_machine(&doc) != machine
            || render_markdown(&doc) != markdown
            || render_compact(&doc) != compact
        {
            failures.push(format!("{name}: projections are not deterministic"));
        }
        // Projection completeness: no rendering drops packet semantics.
        for (label, rendering) in [("markdown", &markdown), ("compact", &compact)] {
            for violation in projection_completeness_violations(&doc, rendering, label) {
                failures.push(format!("{name}: {violation:?}"));
            }
        }
        let rendered = [
            (PacketProjection::Machine, machine),
            (PacketProjection::Markdown, markdown),
            (PacketProjection::Compact, compact),
        ];
        if update_golden {
            for (projection, text) in &rendered {
                let golden_file = root.join(golden_path(stem, *projection));
                if let Some(parent) = golden_file.parent().filter(|p| !p.as_os_str().is_empty()) {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::write(&golden_file, text)
                    .with_context(|| format!("failed to write {}", golden_file.display()))?;
            }
            continue;
        }
        for (projection, text) in &rendered {
            let golden_file = root.join(golden_path(stem, *projection));
            let golden = fs::read_to_string(&golden_file)
                .with_context(|| format!("missing golden vector {}", golden_file.display()))?;
            if &golden != text {
                failures.push(format!(
                    "{name}: {} projection drifted from its golden vector",
                    projection.name()
                ));
            }
        }
    }

    // Canonical semantics: shuffled input (and a varied non-semantic
    // metadata timestamp) produces identical canonical bytes.
    let base = load_json(&fixture_dir.join("bounded_leaf_offline.v1.json"))?;
    let shuffled = load_json(&fixture_dir.join(SHUFFLED_FIXTURE))?;
    if canonical_form(&base) != canonical_form(&shuffled) {
        failures.push(
            "canonical semantics differ between the ordered and shuffled fixture documents"
                .to_string(),
        );
    }

    // Invalid fixtures fail with exactly the expected reason code.
    let expected_errors = load_json(&fixture_dir.join("invalid").join("expected_errors.json"))?;
    let Some(expected) = as_str_map(&expected_errors) else {
        bail!("expected_errors.json must be an object");
    };
    for (file, expected) in expected {
        let Some(expected_code) = expected.as_str() else {
            bail!("expected_errors.json: {file} must name a string reason code");
        };
        let doc = load_json(&fixture_dir.join("invalid").join(file))?;
        let violations = validate_document(&doc);
        let codes = violation_codes(&violations);
        if violations.is_empty() {
            failures.push(format!(
                "invalid/{file}: expected failure {expected_code}, document validated"
            ));
        } else if !codes.contains(&expected_code) {
            failures
                .push(format!("invalid/{file}: expected failure {expected_code}, got {codes:?}"));
        }
        // Fail-closed rendering: an invalid packet never renders prose.
        if let Ok(rendered) = render_to_string(&doc, PacketProjection::Compact) {
            failures.push(format!(
                "invalid/{file}: invalid packet rendered a projection ({})",
                rendered.len()
            ));
        }
    }

    if failures.is_empty() {
        if update_golden {
            println!("agent_implementation_packet.v1: golden vectors updated");
            Ok(())
        } else {
            println!(
                "agent_implementation_packet.v1: closed contract, fixtures, canonical control, and deterministic projections all valid"
            );
            Ok(())
        }
    } else {
        bail!("agent implementation packet check failed:\n{}", failures.join("\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    fn fixture(name: &str) -> TestResult<Value> {
        load_json(&project_root()?.join(FIXTURE_DIR).join(name))
    }

    fn has(doc: &Value, code: &str) -> bool {
        validate_document(doc).iter().any(|violation| violation.code == code)
    }

    fn valid(doc: &Value) -> bool {
        validate_document(doc).is_empty()
    }

    fn invalid_fixture(name: &str) -> TestResult<Value> {
        load_json(&project_root()?.join(FIXTURE_DIR).join("invalid").join(name))
    }

    #[test]
    fn valid_fixtures_validate() -> TestResult {
        for name in VALID_FIXTURES {
            let doc = fixture(name)?;
            assert!(valid(&doc), "{name} must be valid");
        }
        Ok(())
    }

    // Negative control 1: a controller cannot become direct builder work.
    #[test]
    fn controller_node_fails_without_manifest_leaf() -> TestResult {
        let doc = invalid_fixture("controller_node_as_work.json")?;
        assert!(has(&doc, "non_buildable_node"));
        // The same node with an explicit bounded-leaf declaration is valid.
        let mut repaired = doc;
        if let Some(work) = repaired
            .as_object_mut()
            .and_then(|root| root.get_mut("work"))
            .and_then(Value::as_object_mut)
        {
            work.insert(
                "bounded_leaf_manifest_ref".to_string(),
                Value::String("example_train.v1:row:N_service_marker_probe".to_string()),
            );
        }
        assert!(valid(&repaired));
        Ok(())
    }

    // Negative control 2: a blocked node names its exact blocking edges.
    #[test]
    fn blocked_node_requires_blocking_edges() -> TestResult {
        let doc = invalid_fixture("blocked_node_without_edge.json")?;
        assert!(has(&doc, "missing_blocking_edge"));
        let spurious = invalid_fixture("ready_node_with_spurious_edge.json")?;
        assert!(has(&spurious, "ambiguous_frontier_state"));
        Ok(())
    }

    // Negative control 3: conditional profile decisions are complete and
    // unambiguous.
    #[test]
    fn profile_decisions_fail_closed() -> TestResult {
        let missing = invalid_fixture("conditional_profile_without_decision.json")?;
        assert!(has(&missing, "missing_profile_decision"));
        let ambiguous = invalid_fixture("unconditional_profile_with_decision.json")?;
        assert!(has(&ambiguous, "ambiguous_profile_decision"));
        Ok(())
    }

    // Negative controls 4/5: landed authorities carry current-tree proofs;
    // historical branches never become current authorities.
    #[test]
    fn authorities_fail_closed_on_stale_and_missing_proofs() -> TestResult {
        let unproven = invalid_fixture("authority_current_without_proof.json")?;
        assert!(has(&unproven, "authority_currentness_missing"));
        let stale = invalid_fixture("stale_authority_tree.json")?;
        assert!(has(&stale, "stale_authority"));
        let historical = invalid_fixture("historical_branch_as_current_authority.json")?;
        assert!(has(&historical, "stale_authority"));
        let duplicate = invalid_fixture("duplicate_authority_ref.json")?;
        assert!(has(&duplicate, "duplicate_authority_ref"));
        Ok(())
    }

    // Negative control 6: overlapping writer/conflict keys fail closed.
    #[test]
    fn overlapping_writer_keys_fail() -> TestResult {
        let overlapping = invalid_fixture("overlapping_writer_keys.json")?;
        assert!(has(&overlapping, "overlapping_writer_keys"));
        let hidden = invalid_fixture("hidden_writer_slots.json")?;
        assert!(has(&hidden, "missing_writer_slots"));
        Ok(())
    }

    // Negative control 7: at least one discriminating falsifier exists and
    // generic statements are not falsifiers.
    #[test]
    fn falsifiers_are_discriminating() -> TestResult {
        let missing = invalid_fixture("missing_falsifier.json")?;
        assert!(has(&missing, "missing_falsifier"));
        let generic = invalid_fixture("generic_falsifier_statement.json")?;
        assert!(has(&generic, "generic_falsifier"));
        Ok(())
    }

    // Negative controls 8/9: verification routes are focused, bound, and
    // cover generated obligations.
    #[test]
    fn verification_routes_fail_closed() -> TestResult {
        let generic = invalid_fixture("generic_verification.json")?;
        assert!(has(&generic, "generic_verification"));
        assert!(has(&generic, "verification_not_focused_first"));
        let obligation = invalid_fixture("missing_generated_obligation.json")?;
        assert!(has(&obligation, "missing_generated_obligation"));
        let diff = invalid_fixture("missing_diff_check.json")?;
        assert!(has(&diff, "missing_diff_check"));
        Ok(())
    }

    // Negative controls 10/11: stop boundaries stay inside the actor
    // contract and a local diff is not delivered.
    #[test]
    fn stop_and_delivery_fail_closed() -> TestResult {
        let unauthorized = invalid_fixture("unauthorized_stop_boundary.json")?;
        assert!(has(&unauthorized, "unauthorized_stop_boundary"));
        let local = invalid_fixture("local_diff_claimed_delivered.json")?;
        assert!(has(&local, "undelivered_claim"));
        Ok(())
    }

    // Negative control 14: mutable assignment/agent state never embeds.
    #[test]
    fn mutable_state_never_embeds() -> TestResult {
        let doc = invalid_fixture("mutable_agent_state.json")?;
        assert!(has(&doc, "mutable_state_embedded"));
        // Even inside non-semantic metadata.
        let mut with_metadata = fixture("bounded_leaf_offline.v1.json")?;
        if let Some(root) = with_metadata.as_object_mut() {
            root.insert("metadata".to_string(), serde_json::json!({"assignment": "agent-7"}));
        }
        assert!(has(&with_metadata, "mutable_state_embedded"));
        Ok(())
    }

    // Negative control 15: missing required input fails generation, and
    // unknown absence is never fabricated.
    #[test]
    fn missing_input_and_fabricated_observation_fail() -> TestResult {
        let missing = invalid_fixture("missing_required_input_frontier.json")?;
        assert!(has(&missing, "missing_required_input"));
        let fabricated = invalid_fixture("fabricated_observation.json")?;
        assert!(has(&fabricated, "fabricated_observation"));
        let incomplete = invalid_fixture("incomplete_observation.json")?;
        assert!(has(&incomplete, "incomplete_observation"));
        Ok(())
    }

    // Negative control 13: input order never changes canonical bytes, and
    // metadata is excluded from canonical identity.
    #[test]
    fn canonical_bytes_ignore_input_order_and_metadata() -> TestResult {
        let base = fixture("bounded_leaf_offline.v1.json")?;
        let shuffled = fixture(SHUFFLED_FIXTURE)?;
        assert_eq!(canonical_form(&base), canonical_form(&shuffled));
        let mut re_meta = base.clone();
        if let Some(root) = re_meta.as_object_mut() {
            root.insert(
                "metadata".to_string(),
                serde_json::json!({"observed_at": "2030-01-01T00:00:00Z"}),
            );
        }
        assert_eq!(canonical_form(&base), canonical_form(&re_meta));
        Ok(())
    }

    // Negative control 12: a rendering that drops semantics fails the
    // completeness check.
    #[test]
    fn dropped_semantics_in_projection_fail() -> TestResult {
        let doc = fixture("bounded_leaf_offline.v1.json")?;
        let markdown = render_markdown(&doc);
        assert!(projection_completeness_violations(&doc, &markdown, "markdown").is_empty());
        let ceiling = doc["work"]["claim_ceiling"].as_str().unwrap_or("");
        let tampered = markdown.replace(ceiling, "");
        let violations = projection_completeness_violations(&doc, &tampered, "markdown");
        assert!(
            violations.iter().any(|violation| violation.code == "projection_dropped_semantics"),
            "a rendering without the claim ceiling must fail completeness"
        );
        Ok(())
    }

    // E06 consumer shape: the Emacs adapter supplies fields only; a fully
    // absent live observation is an honest offline packet.
    #[test]
    fn emacs_consumer_shape_offline_is_honest() -> TestResult {
        let doc = fixture("consumer_emacs_e06_shape.v1.json")?;
        assert!(doc.get("live_observation").is_none());
        assert!(valid(&doc));
        let compact = render_compact(&doc);
        assert!(compact.contains("CANDIDATE: not_observed"));
        assert!(compact.contains("#10872"));
        // The compact prompt stays executable: the literal command, its
        // scope, and the second-run obligation survive the projection.
        assert!(compact.contains("make -C emacs test PACKET_ADAPTER=1"));
        assert!(compact.contains("[focused_proof]"));
        Ok(())
    }

    // The render path is fail-closed: an invalid packet never renders
    // plausible prose.
    #[test]
    fn render_refuses_invalid_packets() -> TestResult {
        let doc = invalid_fixture("missing_claim_ceiling.json")?;
        assert!(render_to_string(&doc, PacketProjection::Machine).is_err());
        Ok(())
    }

    // All three projections of every valid fixture preserve every semantic
    // anchor.
    #[test]
    fn all_projections_preserve_semantics() -> TestResult {
        for name in VALID_FIXTURES {
            let doc = fixture(name)?;
            for (label, rendering) in [
                ("machine", render_machine(&doc)),
                ("markdown", render_markdown(&doc)),
                ("compact", render_compact(&doc)),
            ] {
                assert!(
                    projection_completeness_violations(&doc, &rendering, label).is_empty(),
                    "{name} {label} projection dropped semantics"
                );
            }
        }
        Ok(())
    }

    // Simplify-review DEAD_SCAFFOLDING repair: the full run path (closed
    // schema, valid fixtures, fail-closed controls, canonical semantics,
    // golden vectors) was reachable only via manual CLI. Check mode has no
    // side effects — `update_golden` is the only writer (mirrors
    // `train_edge_contract`'s real-tree run).
    #[test]
    fn run_passes_on_current_tree() -> TestResult {
        run(false)
    }
}
