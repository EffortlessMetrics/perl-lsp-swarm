//! Validate and project the stable `perl-corpus` authority train manifest
//! (`perl_corpus_train.v1`, issue #10980 under controller #8826 / epic #6696).
//!
//! The manifest is stable reviewed programme data only: node identities,
//! roles, release horizons, typed dependency classes, exclusive conflict keys,
//! authority transitions, legacy exits, candidate lineages, proof/spec
//! obligations, and stop/claim boundaries. This module enforces the
//! manifest's own shift-left rejection law with named diagnostics, renders
//! deterministic reviewer projections, and explains one static node packet.
//!
//! It never derives a current-tree state or ready frontier (#10992), observes
//! GitHub or any live candidate (#11001), compiles a spec packet (#11010),
//! emits an agent work packet (#11017), changes corpus behavior, or performs
//! an external action. Mutable GitHub/task/agent state is rejected from the
//! stable bytes.
//!
//! Shared mechanics are consumed, not duplicated: the order-invariant
//! canonical digest comes from `module_train::canonical_digest` and the
//! canonical serialization form from `native_neovim_train::canonical_form`.
//! The mechanical overlap that remains (title fingerprint, banned-key walk,
//! hard-cycle search) is recorded for the #10554 extraction gate in the
//! bundle's `context.md`.

use crate::tasks::module_train::canonical_digest;
use crate::tasks::native_neovim_train::canonical_form;
use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[cfg(test)]
#[path = "perl_corpus_train_tests.rs"]
mod tests;

/// Repository-relative bundle directory of the stable train.
pub const BUNDLE_DIR: &str = ".spec/10980-perl-corpus-stable-dag";
/// Canonical manifest bytes.
pub const MANIFEST_PATH: &str = ".spec/10980-perl-corpus-stable-dag/train.manifest.json";
/// Order-shuffled determinism control; must canonize identically and validate.
pub const SHUFFLED_PATH: &str = ".spec/10980-perl-corpus-stable-dag/shuffled/train.manifest.json";
/// Discriminating invalid fixtures, each bound to one named reason code.
pub const INVALID_DIR: &str = ".spec/10980-perl-corpus-stable-dag/invalid";
/// Generated reviewer projections (`graph`), freshness-checked by `check`.
pub const PROJECTIONS_DIR: &str = ".spec/10980-perl-corpus-stable-dag/projections";
const EXPECTED_ERRORS_FILENAME: &str = "expected_errors.json";
const SCHEMA_PATH: &str = "schemas/perl_corpus_train.v1.schema.json";

pub const SCHEMA_NAME: &str = "perl_corpus_train.v1";

/// Roles that may become ordinary one-PR coding work.
const SELECTABLE_ROLES: &[&str] = &["implementation", "proof", "cutover"];
/// Roles that are grouping, decision, authorization, or history surfaces and
/// can never be emitted as builder work.
const NEVER_SELECTABLE_ROLES: &[&str] =
    &["controller", "decision", "external_action", "historical"];
/// Roles whose authority move must name a legacy path exit.
const EXIT_BEARING_ROLES: &[&str] = &["implementation", "cutover"];
/// Closed dependency classes; the manifest's `dependency_classes` must declare
/// exactly this set once each.
const DEPENDENCY_CLASSES: &[&str] = &["hard", "evidence", "authorization"];
/// The only authority an authorization edge may target; no other declared
/// symbol can stand in for explicit approval.
const EXPLICIT_AUTHORIZATION: &str = "#EXPLICIT-AUTHORIZATION";
/// Fields that retire a node into history; a node carries at most one.
const SUPERSESSION_FIELDS: &[&str] = &["superseded_by", "duplicate_of", "transferred_to"];

/// Closed horizon order; the manifest's `release_horizons.rank` must agree.
const HORIZON_ORDER: &[&str] = &[
    "foundation_safety",
    "topology_registration",
    "expectation_gold_generator",
    "workspace_command",
    "audit_evidence",
    "consumer_ci_closeout",
    "package_externalization",
    "publication_manual",
    "optional_breadth",
];
/// Horizons a foundation/topology/etc. node may never hard-depend on.
const EXTERNAL_HORIZONS: &[&str] = &["package_externalization", "publication_manual"];

/// Object keys banned anywhere in stable bytes: no mutable GitHub, task,
/// agent, run, writer, or frontier state.
const BANNED_KEYS: &[&str] = &[
    "live_frontier",
    "live_state",
    "github_state",
    "task_state",
    "agent_state",
    "head_sha",
    "base_sha",
    "current_sha",
    "merge_commit",
    "check_run",
    "workflow_run",
    "latest_check",
    "issue_status",
    "pr_state",
    "review_state",
    "frontier_cache",
    "active_writer",
    "assignee",
];
/// String fragments banned in stable values: branch/pull/commit coordinates.
const BANNED_VALUE_FRAGMENTS: &[&str] = &["refs/heads/", "/pull/", "/commit/", "/tree/"];

#[derive(Debug)]
pub struct Violation {
    pub code: String,
    pub detail: String,
}

impl Violation {
    /// One named rejection with its bounded detail.
    fn new(code: &str, detail: impl Into<String>) -> Self {
        Self { code: code.to_string(), detail: detail.into() }
    }
}

/// Reason codes of a violation list, in report order.
fn violation_codes(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(|violation| violation.code.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Small accessors over the untyped document.
// ---------------------------------------------------------------------------

/// A string field, or `""` when absent or not a string.
fn str_field<'a>(node: &'a Map<String, Value>, key: &str) -> &'a str {
    node.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The string items of an array field, in manifest order.
fn strings<'a>(node: &'a Map<String, Value>, key: &str) -> Vec<&'a str> {
    node.get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// The object items of a top-level array field, in manifest order.
fn objects<'a>(doc: &'a Value, key: &str) -> Vec<&'a Map<String, Value>> {
    doc.get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

/// Whether a boolean field is exactly `true`.
fn is_true(node: &Map<String, Value>, key: &str) -> bool {
    node.get(key).and_then(Value::as_bool) == Some(true)
}

/// A non-blank string field, or `None`.
fn optional_str<'a>(node: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    node.get(key).and_then(Value::as_str).filter(|value| !value.trim().is_empty())
}

/// Title fingerprint law shared with the landed trains: first 16 uppercase
/// hex characters of the SHA-256 of the exact title bytes.
pub fn title_fingerprint(title: &str) -> String {
    let digest = Sha256::digest(title.as_bytes());
    let mut hex = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        let _ = write!(hex, "{byte:02X}");
    }
    hex
}

/// A symbolic authority such as `#EXPLICIT-AUTHORIZATION`, as opposed to a
/// numeric issue reference.
fn is_symbolic_authority(target: &str) -> bool {
    target.starts_with('#') && target.chars().nth(1).is_some_and(|ch| ch.is_ascii_uppercase())
}

// ---------------------------------------------------------------------------
// Stable-byte hygiene.
// ---------------------------------------------------------------------------

/// A run of 32 or more hex digits in either case: a commit coordinate, never
/// a stable fact. (Title fingerprints are 16 digits and never reach the run.)
fn looks_like_commit_hash(text: &str) -> bool {
    let mut run = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 32 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Reject banned state keys and live coordinates anywhere in the document.
fn walk_stable_bytes(value: &Value, path: &str, violations: &mut Vec<Violation>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if BANNED_KEYS.contains(&key.as_str()) {
                    violations.push(Violation::new(
                        "MUTABLE_STATE_EMBEDDED",
                        format!("{path}.{key}: live GitHub/task/agent/writer state key embedded in stable manifest bytes"),
                    ));
                }
                walk_stable_bytes(child, &format!("{path}.{key}"), violations);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                walk_stable_bytes(child, &format!("{path}[{index}]"), violations);
            }
        }
        Value::String(text) => {
            if looks_like_commit_hash(text)
                || BANNED_VALUE_FRAGMENTS.iter().any(|fragment| text.contains(fragment))
            {
                violations.push(Violation::new(
                    "MUTABLE_STATE_EMBEDDED",
                    format!("{path}: value carries a commit/branch/pull coordinate; stable bytes never bind live state"),
                ));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Vocabulary laws.
// ---------------------------------------------------------------------------

struct Vocabulary<'a> {
    selectable_by_role: BTreeMap<&'a str, bool>,
    horizon_rank: BTreeMap<&'a str, i64>,
    conflict_keys: BTreeSet<&'a str>,
    external_authorities: BTreeSet<&'a str>,
    lineages: BTreeSet<&'a str>,
}

/// Check the closed vocabularies and collect them for the node laws.
fn vocabulary_problems<'a>(doc: &'a Value, violations: &mut Vec<Violation>) -> Vocabulary<'a> {
    let mut selectable_by_role = BTreeMap::new();
    for entry in objects(doc, "role_vocabulary") {
        let role = str_field(entry, "role");
        let selectable = is_true(entry, "selectable");
        let expected = SELECTABLE_ROLES.contains(&role);
        if selectable != expected || NEVER_SELECTABLE_ROLES.contains(&role) == selectable {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("role_vocabulary {role}: selectable={selectable} contradicts the closed role law"),
            ));
        }
        if selectable_by_role.insert(role, selectable).is_some() {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("role_vocabulary {role}: duplicate role entry"),
            ));
        }
    }
    for role in SELECTABLE_ROLES.iter().chain(NEVER_SELECTABLE_ROLES) {
        if !selectable_by_role.contains_key(role) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("role_vocabulary: {role} is missing"),
            ));
        }
    }

    // The schema fixes the count and the enum; only this law rejects a
    // duplicated class standing in for an omitted one.
    let mut declared_classes = BTreeSet::new();
    for entry in objects(doc, "dependency_classes") {
        let class = str_field(entry, "class");
        if !DEPENDENCY_CLASSES.contains(&class) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("dependency_classes {class}: not a closed dependency class"),
            ));
        }
        if !declared_classes.insert(class) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("dependency_classes {class}: duplicate class entry"),
            ));
        }
    }
    for class in DEPENDENCY_CLASSES {
        if !declared_classes.contains(class) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("dependency_classes: {class} is missing"),
            ));
        }
    }

    let mut horizon_rank = BTreeMap::new();
    for entry in objects(doc, "release_horizons") {
        let horizon = str_field(entry, "horizon");
        let rank = entry.get("rank").and_then(Value::as_i64).unwrap_or(-1);
        let expected = HORIZON_ORDER.iter().position(|&item| item == horizon).map(|p| p as i64);
        if expected != Some(rank) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!(
                    "release_horizons {horizon}: rank {rank} drifts from the closed horizon order"
                ),
            ));
        }
        if horizon_rank.insert(horizon, rank).is_some() {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("release_horizons {horizon}: duplicate horizon entry"),
            ));
        }
    }
    for horizon in HORIZON_ORDER {
        if !horizon_rank.contains_key(horizon) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("release_horizons: {horizon} is missing"),
            ));
        }
    }

    let mut conflict_keys = BTreeSet::new();
    for entry in objects(doc, "conflict_keys") {
        let key = str_field(entry, "key");
        if !conflict_keys.insert(key) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("conflict_keys {key}: duplicate registry entry"),
            ));
        }
    }

    let mut external_authorities = BTreeSet::new();
    for entry in objects(doc, "external_authorities") {
        let id = str_field(entry, "id");
        if !external_authorities.insert(id) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("external_authorities {id}: duplicate authority"),
            ));
        }
    }

    let mut lineages = BTreeSet::new();
    for entry in objects(doc, "candidate_lineages") {
        let reference = str_field(entry, "ref");
        if !lineages.insert(reference) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("candidate_lineages {reference}: duplicate lineage"),
            ));
        }
        for field in ["subject", "reuse_policy"] {
            let text = str_field(entry, field).to_ascii_lowercase();
            if ["merged", "closed", "open", "draft", "landed"]
                .iter()
                .any(|&word| text.split(|c: char| !c.is_ascii_alphabetic()).any(|w| w == word))
            {
                violations.push(Violation::new(
                    "MUTABLE_STATE_EMBEDDED",
                    format!("candidate_lineages {reference}.{field}: lineage rows never carry current candidate status"),
                ));
            }
        }
    }

    Vocabulary { selectable_by_role, horizon_rank, conflict_keys, external_authorities, lineages }
}

// ---------------------------------------------------------------------------
// Node laws.
// ---------------------------------------------------------------------------

/// Per-node identity, role, supersession, authority, and contract laws.
fn node_problems(doc: &Value, vocab: &Vocabulary<'_>, violations: &mut Vec<Violation>) {
    let nodes = objects(doc, "nodes");
    let ids: BTreeSet<&str> = nodes.iter().map(|node| str_field(node, "node_id")).collect();
    let subjects: BTreeSet<&str> = nodes.iter().map(|node| str_field(node, "issue_ref")).collect();
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    let mut seen_subjects: BTreeSet<&str> = BTreeSet::new();
    let mut active_authorities: BTreeMap<&str, &str> = BTreeMap::new();

    for node in &nodes {
        let id = str_field(node, "node_id");
        let role = str_field(node, "role");
        let subject = str_field(node, "issue_ref");
        let selectable = is_true(node, "selectable");

        if !seen_ids.insert(id) {
            violations.push(Violation::new(
                "DUPLICATE_NODE_IDENTITY",
                format!("node {id}: duplicate node identity"),
            ));
        }
        if !seen_subjects.insert(subject) {
            violations.push(Violation::new(
                "DUPLICATE_SUBJECT_IDENTITY",
                format!("node {id}: subject {subject} already owns another node"),
            ));
        }
        if subject.starts_with("PR ") && role != "historical" {
            violations.push(Violation::new(
                "CANDIDATE_AS_ACTIVE_NODE",
                format!("node {id}: a pull request ({subject}) is a candidate lineage, never an active stable node"),
            ));
        }
        if str_field(node, "title_fingerprint") != title_fingerprint(str_field(node, "title")) {
            violations.push(Violation::new(
                "TITLE_FINGERPRINT_MISMATCH",
                format!("node {id}: title fingerprint does not match the exact title bytes"),
            ));
        }

        match vocab.selectable_by_role.get(role) {
            None => violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("node {id}: role {role} is not in the role vocabulary"),
            )),
            Some(true) if !selectable => violations.push(Violation::new(
                "ROLE_SELECTABILITY_MISMATCH",
                format!("node {id}: role {role} is a coding leaf but the node is marked non-selectable; supersede or transfer it explicitly instead"),
            )),
            Some(false) if selectable => violations.push(Violation::new(
                "NON_LEAF_SELECTABLE",
                format!("node {id}: {role} nodes are never coding-selectable"),
            )),
            _ => {}
        }

        let superseded = SUPERSESSION_FIELDS
            .iter()
            .filter_map(|field| optional_str(node, field))
            .collect::<Vec<_>>();
        for target in &superseded {
            if !ids.contains(target) {
                violations.push(Violation::new(
                    "UNKNOWN_EDGE_TARGET",
                    format!("node {id}: supersession target {target} resolves to no node"),
                ));
            }
        }
        if !superseded.is_empty() && role != "historical" {
            violations.push(Violation::new(
                "SUPERSEDED_REACTIVATED",
                format!("node {id}: carries superseded_by/duplicate_of/transferred_to but keeps role {role}; a superseded node is historical"),
            ));
        }
        for target in strings(node, "supersedes") {
            if !ids.contains(target) {
                violations.push(Violation::new(
                    "UNKNOWN_EDGE_TARGET",
                    format!("node {id}: supersedes {target} which resolves to no node"),
                ));
            }
        }

        if role != "historical" {
            let authority = str_field(node, "authority_after");
            if let Some(previous) = active_authorities.insert(authority, id) {
                violations.push(Violation::new(
                    "DUPLICATE_ACTIVE_AUTHORITY",
                    format!(
                        "node {id}: authority_after already established by {previous}: {authority}"
                    ),
                ));
            }
        }

        let horizon = str_field(node, "release_horizon");
        if !vocab.horizon_rank.contains_key(horizon) {
            violations.push(Violation::new(
                "VOCABULARY_DRIFT",
                format!("node {id}: release horizon {horizon} is not declared"),
            ));
        }
        for key in strings(node, "exclusive_conflict_keys") {
            if !vocab.conflict_keys.contains(key) {
                violations.push(Violation::new(
                    "CONFLICT_KEY_UNKNOWN",
                    format!("node {id}: conflict key {key} is not in the conflict-key registry"),
                ));
            }
        }
        for lineage in strings(node, "known_candidate_lineage_refs") {
            if !vocab.lineages.contains(lineage) {
                violations.push(Violation::new(
                    "UNKNOWN_CANDIDATE_LINEAGE",
                    format!("node {id}: candidate lineage {lineage} is not declared in candidate_lineages"),
                ));
            }
        }
        // Numeric and symbolic alike: an authority is either a declared
        // external authority or the subject of a node in this manifest.
        for authority in strings(node, "semantic_authority_refs") {
            if !vocab.external_authorities.contains(authority) && !subjects.contains(authority) {
                violations.push(Violation::new(
                    "UNKNOWN_EDGE_TARGET",
                    format!("node {id}: semantic authority {authority} is neither a declared external authority nor a node subject"),
                ));
            }
        }

        if selectable {
            let short = |field: &str, min: usize| str_field(node, field).trim().len() < min;
            let empty = |field: &str| strings(node, field).is_empty();
            let mut missing: Vec<&str> = Vec::new();
            if short("one_pr_proposition", 20) {
                missing.push("one_pr_proposition");
            }
            if short("first_falsifier", 20) {
                missing.push("first_falsifier");
            }
            if short("claim_ceiling", 20) {
                missing.push("claim_ceiling");
            }
            if short("authority_before", 5) {
                missing.push("authority_before");
            }
            if short("authority_after", 10) {
                missing.push("authority_after");
            }
            if short("candidate_reuse_policy", 10) {
                missing.push("candidate_reuse_policy");
            }
            for field in [
                "stop_conditions",
                "positive_proof_obligations",
                "negative_controls",
                "repository_verification",
                "exclusive_conflict_keys",
                "likely_owned_surfaces",
                "forbidden_surfaces",
            ] {
                if empty(field) {
                    missing.push(field);
                }
            }
            if !missing.is_empty() {
                violations.push(Violation::new(
                    "INCOMPLETE_ONE_PR_CONTRACT",
                    format!("node {id}: selectable leaf lacks {}", missing.join(", ")),
                ));
            }
        }

        if EXIT_BEARING_ROLES.contains(&role) && selectable {
            let exit = node.get("legacy_exit").and_then(Value::as_object);
            let complete = exit.is_some_and(|exit| {
                optional_str(exit, "owner").is_some() && optional_str(exit, "condition").is_some()
            });
            if !complete {
                violations.push(Violation::new(
                    "MISSING_LEGACY_EXIT",
                    format!("node {id}: {role} moves an authority but names no legacy path exit owner and removal condition"),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Edge laws.
// ---------------------------------------------------------------------------

struct Graph<'a> {
    /// hard + evidence node edges, source -> targets (acyclicity law).
    ordering: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// hard node edges only, source -> targets. Only a hard path proves two
    /// writers are serialized: an evidence dependency lets its source land
    /// while the target is still `not_proven`, so it orders nothing.
    hard_ordering: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

/// Per-edge target, class, horizon, and authorization laws; returns the
/// ordering graphs for the cycle and conflict laws.
fn edge_problems<'a>(
    doc: &'a Value,
    vocab: &Vocabulary<'_>,
    violations: &mut Vec<Violation>,
) -> Graph<'a> {
    let nodes = objects(doc, "nodes");
    let by_id: BTreeMap<&str, &Map<String, Value>> =
        nodes.iter().map(|node| (str_field(node, "node_id"), *node)).collect();
    let package_rank = vocab.horizon_rank.get("package_externalization").copied().unwrap_or(6);

    let mut ordering: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut hard_ordering: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut derived_consumers: BTreeMap<&str, BTreeSet<&str>> =
        by_id.keys().map(|id| (*id, BTreeSet::new())).collect();

    for node in &nodes {
        let source = str_field(node, "node_id");
        let role = str_field(node, "role");
        let source_rank =
            vocab.horizon_rank.get(str_field(node, "release_horizon")).copied().unwrap_or(0);
        let mut targets: BTreeSet<&str> = BTreeSet::new();
        let mut authorization_edges = 0usize;

        let deps: Vec<&'a Map<String, Value>> = node
            .get("dependencies")
            .and_then(Value::as_array)
            .map(|deps| deps.iter().filter_map(Value::as_object).collect())
            .unwrap_or_default();
        for dep in deps {
            let target = str_field(dep, "target");
            let class = str_field(dep, "class");
            if target == source {
                violations.push(Violation::new(
                    "SELF_DEPENDENCY",
                    format!("node {source}: depends on itself"),
                ));
                continue;
            }
            if !targets.insert(target) {
                violations.push(Violation::new(
                    "DUPLICATE_EDGE_TARGET",
                    format!("node {source}: more than one dependency edge to {target}"),
                ));
            }
            let target_is_node = by_id.contains_key(target);
            let target_is_external = vocab.external_authorities.contains(target);
            if !target_is_node && !target_is_external {
                violations.push(Violation::new(
                    "UNKNOWN_EDGE_TARGET",
                    format!("node {source}: dependency target {target} resolves to no node or declared authority"),
                ));
                continue;
            }
            match class {
                "authorization" => {
                    authorization_edges += 1;
                    if target != EXPLICIT_AUTHORIZATION || !target_is_external {
                        violations.push(Violation::new(
                            "DEPENDENCY_CLASS_COLLAPSED",
                            format!("node {source}: authorization dependencies target the declared {EXPLICIT_AUTHORIZATION} authority only, not {target}"),
                        ));
                    }
                    if role != "external_action" {
                        violations.push(Violation::new(
                            "AUTHORIZATION_ON_CODING_NODE",
                            format!("node {source}: only external_action nodes carry authorization dependencies; role {role} would turn authorization into ordinary coding work"),
                        ));
                    }
                }
                "hard" | "evidence" => {
                    if is_symbolic_authority(target) {
                        violations.push(Violation::new(
                            "DEPENDENCY_CLASS_COLLAPSED",
                            format!("node {source}: {class} dependency on symbolic authority {target} collapses authorization into a coding dependency"),
                        ));
                    }
                    if let Some(target_node) = by_id.get(target) {
                        let target_role = str_field(target_node, "role");
                        if target_role == "controller" {
                            violations.push(Violation::new(
                                "DEPENDENCY_ON_CONTROLLER",
                                format!("node {source}: {class} dependency on controller {target}; depend on the leaf that owns the result"),
                            ));
                        }
                        let target_horizon = str_field(target_node, "release_horizon");
                        if source_rank < package_rank && EXTERNAL_HORIZONS.contains(&target_horizon)
                        {
                            violations.push(Violation::new(
                                "PUBLICATION_PROMOTED_INTO_FOUNDATION",
                                format!("node {source}: {class} dependency on {target_horizon} node {target} makes package/publication an ordinary prerequisite of earlier-horizon work"),
                            ));
                        }
                        ordering.entry(source).or_default().insert(target);
                        if class == "hard" {
                            hard_ordering.entry(source).or_default().insert(target);
                        }
                        if let Some(consumers) = derived_consumers.get_mut(target) {
                            consumers.insert(source);
                        }
                    }
                }
                other => violations.push(Violation::new(
                    "DEPENDENCY_CLASS_COLLAPSED",
                    format!("node {source}: unknown dependency class {other}"),
                )),
            }
        }

        if role == "external_action" && authorization_edges == 0 {
            violations.push(Violation::new(
                "AUTHORIZATION_MISSING",
                format!("node {source}: external_action carries no authorization dependency; authorization is never inferred"),
            ));
        }
    }

    for node in &nodes {
        let id = str_field(node, "node_id");
        let declared: BTreeSet<&str> = strings(node, "consumed_by").into_iter().collect();
        let derived = derived_consumers.get(id).cloned().unwrap_or_default();
        if declared != derived {
            violations.push(Violation::new(
                "CONSUMED_BY_MISMATCH",
                format!(
                    "node {id}: consumed_by [{}] is not the derived reverse edge set [{}]",
                    declared.iter().copied().collect::<Vec<_>>().join(","),
                    derived.iter().copied().collect::<Vec<_>>().join(",")
                ),
            ));
        }
    }

    Graph { ordering, hard_ordering }
}

/// First hard/evidence cycle, as the node path that closes it.
fn find_cycle<'a>(graph: &Graph<'a>) -> Option<Vec<&'a str>> {
    fn visit<'a>(
        node: &'a str,
        graph: &BTreeMap<&'a str, BTreeSet<&'a str>>,
        colour: &mut BTreeMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<&'a str>> {
        match colour.get(node) {
            Some(1) => {
                let start = path.iter().position(|&item| item == node).unwrap_or(0);
                return Some(path[start..].to_vec());
            }
            Some(_) => return None,
            None => {}
        }
        colour.insert(node, 1);
        path.push(node);
        if let Some(targets) = graph.get(node) {
            for target in targets {
                if let Some(cycle) = visit(target, graph, colour, path) {
                    return Some(cycle);
                }
            }
        }
        path.pop();
        colour.insert(node, 2);
        None
    }
    let mut colour = BTreeMap::new();
    let mut path = Vec::new();
    for source in graph.ordering.keys() {
        if let Some(cycle) = visit(source, &graph.ordering, &mut colour, &mut path) {
            return Some(cycle);
        }
    }
    None
}

/// Transitive closure of the hard ordering, used to prove two same-key
/// writers are serialized by a dependency path.
fn reachability<'a>(graph: &Graph<'a>) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
    let mut closure: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
    for source in graph.hard_ordering.keys() {
        let mut seen: BTreeSet<&'a str> = BTreeSet::new();
        let mut stack: Vec<&'a str> = vec![source];
        while let Some(current) = stack.pop() {
            if let Some(targets) = graph.hard_ordering.get(current) {
                for target in targets {
                    if seen.insert(target) {
                        stack.push(target);
                    }
                }
            }
        }
        closure.insert(source, seen);
    }
    closure
}

/// Supersession links form a consistent history: a node carries at most one
/// retirement disposition, never points at itself, `superseded_by` and
/// `supersedes` mirror each other exactly, and following dispositions never
/// cycles.
fn supersession_problems(doc: &Value, violations: &mut Vec<Violation>) {
    let nodes = objects(doc, "nodes");
    let by_id: BTreeMap<&str, &Map<String, Value>> =
        nodes.iter().map(|node| (str_field(node, "node_id"), *node)).collect();
    let mut successor: BTreeMap<&str, &str> = BTreeMap::new();

    for node in &nodes {
        let id = str_field(node, "node_id");
        let dispositions: Vec<(&str, &str)> = SUPERSESSION_FIELDS
            .iter()
            .filter_map(|field| optional_str(node, field).map(|target| (*field, target)))
            .collect();
        if dispositions.len() > 1 {
            violations.push(Violation::new(
                "SUPERSESSION_INCONSISTENT",
                format!("node {id}: carries more than one retirement disposition ({})", {
                    dispositions.iter().map(|(field, _)| *field).collect::<Vec<_>>().join(", ")
                }),
            ));
        }
        for (field, target) in &dispositions {
            if *target == id {
                violations.push(Violation::new(
                    "SUPERSESSION_INCONSISTENT",
                    format!("node {id}: {field} points at itself"),
                ));
            } else {
                successor.insert(id, target);
            }
        }
        if let Some(target) = optional_str(node, "superseded_by") {
            let mirrored = by_id
                .get(target)
                .is_some_and(|successor| strings(successor, "supersedes").contains(&id));
            if target != id && by_id.contains_key(target) && !mirrored {
                violations.push(Violation::new(
                    "SUPERSESSION_INCONSISTENT",
                    format!("node {id}: superseded_by {target} but {target} does not list it under supersedes"),
                ));
            }
        }
        for target in strings(node, "supersedes") {
            if target == id {
                violations.push(Violation::new(
                    "SUPERSESSION_INCONSISTENT",
                    format!("node {id}: supersedes itself"),
                ));
                continue;
            }
            let mirrored =
                by_id.get(target).is_some_and(|old| optional_str(old, "superseded_by") == Some(id));
            if by_id.contains_key(target) && !mirrored {
                violations.push(Violation::new(
                    "SUPERSESSION_INCONSISTENT",
                    format!(
                        "node {id}: supersedes {target} but {target} is not superseded_by {id}"
                    ),
                ));
            }
        }
    }

    // Each node has at most one successor, so a walk either terminates or
    // returns to a node already on the current path.
    let mut reported: BTreeSet<&str> = BTreeSet::new();
    for start in successor.keys() {
        let mut path: Vec<&str> = vec![start];
        let mut current = *start;
        while let Some(next) = successor.get(current) {
            if let Some(position) = path.iter().position(|item| item == next) {
                let cycle = &path[position..];
                if cycle.iter().any(|item| reported.insert(item)) {
                    violations.push(Violation::new(
                        "SUPERSESSION_INCONSISTENT",
                        format!("supersession cycle: {} -> {next}", cycle.join(" -> ")),
                    ));
                }
                break;
            }
            path.push(next);
            current = next;
        }
    }
}

/// Two selectable owners of one exclusive key must be serialized by a hard path.
fn conflict_problems(doc: &Value, graph: &Graph<'_>, violations: &mut Vec<Violation>) {
    let nodes = objects(doc, "nodes");
    let closure = reachability(graph);
    let mut by_key: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in &nodes {
        if !is_true(node, "selectable") {
            continue;
        }
        let id = str_field(node, "node_id");
        for key in strings(node, "exclusive_conflict_keys") {
            by_key.entry(key).or_default().push(id);
        }
    }
    for (key, owners) in &by_key {
        for (index, left) in owners.iter().enumerate() {
            for right in owners.iter().skip(index + 1) {
                let ordered = closure.get(left).is_some_and(|set| set.contains(right))
                    || closure.get(right).is_some_and(|set| set.contains(left));
                if !ordered {
                    violations.push(Violation::new(
                        "CONFLICT_KEY_PARALLEL_COLLISION",
                        format!("nodes {left} and {right} both own exclusive conflict key {key} without a hard dependency path between them; two writers would mutate one authority"),
                    ));
                }
            }
        }
    }
}

/// Every named shift-left diagnostic over one parsed document.
pub fn validate_document(doc: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    walk_stable_bytes(doc, "$", &mut violations);
    if doc.get("schema").and_then(Value::as_str) != Some(SCHEMA_NAME) {
        violations
            .push(Violation::new("VOCABULARY_DRIFT", format!("schema must be {SCHEMA_NAME}")));
    }
    let vocab = vocabulary_problems(doc, &mut violations);
    node_problems(doc, &vocab, &mut violations);
    supersession_problems(doc, &mut violations);
    let graph = edge_problems(doc, &vocab, &mut violations);
    if let Some(cycle) = find_cycle(&graph) {
        violations.push(Violation::new(
            "HARD_DEPENDENCY_CYCLE",
            format!("hard/evidence dependency cycle: {}", cycle.join(" -> ")),
        ));
    }
    conflict_problems(doc, &graph, &mut violations);
    violations
}

// ---------------------------------------------------------------------------
// Loading.
// ---------------------------------------------------------------------------

/// Names of every `*.json` fixture in the invalid directory except the
/// expectation map itself, so coverage can be compared as an exact set.
pub fn invalid_fixture_names(root: &Path) -> Result<BTreeSet<String>> {
    let dir = root.join(INVALID_DIR);
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry =
            entry.with_context(|| format!("failed to read an entry of {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") && name != EXPECTED_ERRORS_FILENAME {
            names.insert(name);
        }
    }
    Ok(names)
}

/// Read one JSON document with the path in every error.
fn load_json(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON in {}", path.display()))
}

/// JSON Schema violations of a document against the closed schema.
fn schema_failures(root: &Path, manifest: &Value) -> Result<Vec<String>> {
    let schema = load_json(&root.join(SCHEMA_PATH))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| color_eyre::eyre::eyre!("{SCHEMA_PATH}: invalid schema: {error}"))?;
    Ok(validator.iter_errors(manifest).map(|error| format!("schema violation: {error}")).collect())
}

/// Load the canonical manifest and reject it unless schema and every named
/// law pass. Consumers never see an unvalidated document.
pub fn load_validated_manifest(root: &Path) -> Result<Value> {
    let manifest = load_json(&root.join(MANIFEST_PATH))?;
    let mut failures = schema_failures(root, &manifest)?;
    for violation in validate_document(&manifest) {
        failures.push(format!("{}: {}", violation.code, violation.detail));
    }
    if !failures.is_empty() {
        bail!("{MANIFEST_PATH} is not a valid {SCHEMA_NAME} manifest:\n{}", failures.join("\n"));
    }
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// Deterministic projections.
// ---------------------------------------------------------------------------

struct ProjectedNode<'a> {
    node: &'a Map<String, Value>,
    id: &'a str,
}

/// Nodes in canonical (`node_id`) order for every projection.
fn projected_nodes(doc: &Value) -> Vec<ProjectedNode<'_>> {
    let mut nodes: Vec<ProjectedNode<'_>> = objects(doc, "nodes")
        .into_iter()
        .map(|node| ProjectedNode { node, id: str_field(node, "node_id") })
        .collect();
    nodes.sort_by(|left, right| left.id.cmp(right.id));
    nodes
}

/// Typed dependencies as `(target, class)` pairs in canonical (sorted) order
/// so every projection is invariant under authoring order.
fn dependency_triples<'a>(node: &'a Map<String, Value>) -> Vec<(&'a str, &'a str)> {
    let mut deps: Vec<(&'a str, &'a str)> = node
        .get("dependencies")
        .and_then(Value::as_array)
        .map(|deps| {
            deps.iter()
                .filter_map(Value::as_object)
                .map(|dep| (str_field(dep, "target"), str_field(dep, "class")))
                .collect()
        })
        .unwrap_or_default();
    deps.sort_unstable();
    deps
}

/// A string list from the node in canonical (sorted) order.
fn sorted_strings<'a>(node: &'a Map<String, Value>, key: &str) -> Vec<&'a str> {
    let mut items = strings(node, key);
    items.sort_unstable();
    items
}

/// A string list field as a sorted JSON array.
fn sorted_string_value(node: &Map<String, Value>, key: &str) -> Value {
    Value::Array(
        sorted_strings(node, key).into_iter().map(|s| Value::String(s.to_string())).collect(),
    )
}

/// Conflict classes per phase: selectable nodes joined by a shared exclusive
/// conflict key form one writer class; distinct classes in one phase are
/// conflict-safe siblings (the dependency ordering still applies).
fn conflict_classes(doc: &Value) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut by_phase: BTreeMap<String, Vec<(&str, BTreeSet<&str>)>> = BTreeMap::new();
    for entry in projected_nodes(doc) {
        if !is_true(entry.node, "selectable") {
            continue;
        }
        let keys: BTreeSet<&str> =
            strings(entry.node, "exclusive_conflict_keys").into_iter().collect();
        by_phase
            .entry(str_field(entry.node, "phase").to_string())
            .or_default()
            .push((entry.id, keys));
    }
    let mut result = BTreeMap::new();
    for (phase, members) in by_phase {
        let mut classes: Vec<(BTreeSet<&str>, Vec<&str>)> = Vec::new();
        for (id, keys) in members {
            let mut merged_keys = keys.clone();
            let mut merged_ids = vec![id];
            let mut remaining = Vec::new();
            for (class_keys, class_ids) in classes {
                if class_keys.intersection(&keys).next().is_some() {
                    merged_keys.extend(class_keys);
                    merged_ids.extend(class_ids);
                } else {
                    remaining.push((class_keys, class_ids));
                }
            }
            merged_ids.sort_unstable();
            remaining.push((merged_keys, merged_ids));
            classes = remaining;
        }
        let mut rendered: Vec<Vec<String>> = classes
            .into_iter()
            .map(|(_, ids)| ids.into_iter().map(str::to_string).collect())
            .collect();
        rendered.sort();
        result.insert(phase, rendered);
    }
    result
}

/// Machine projection: per-node typed edges, keys, counts, writer classes.
fn render_json(doc: &Value, digest: &str) -> Result<String> {
    let mut nodes = Vec::new();
    for entry in projected_nodes(doc) {
        let node = entry.node;
        let deps = dependency_triples(node);
        let by_class = |class: &str| -> Vec<Value> {
            deps.iter()
                .filter(|(_, c)| *c == class)
                .map(|(target, _)| Value::String((*target).to_string()))
                .collect()
        };
        let mut projected = Map::new();
        for field in [
            "node_id",
            "issue_ref",
            "role",
            "selectable",
            "lane",
            "phase",
            "release_horizon",
            "authority_after",
            "superseded_by",
            "transferred_to",
        ] {
            projected.insert(field.to_string(), node.get(field).cloned().unwrap_or(Value::Null));
        }
        projected.insert("hard_dependencies".to_string(), Value::Array(by_class("hard")));
        projected.insert("evidence_dependencies".to_string(), Value::Array(by_class("evidence")));
        projected.insert(
            "authorization_dependencies".to_string(),
            Value::Array(by_class("authorization")),
        );
        projected.insert("consumed_by".to_string(), sorted_string_value(node, "consumed_by"));
        projected.insert(
            "exclusive_conflict_keys".to_string(),
            sorted_string_value(node, "exclusive_conflict_keys"),
        );
        projected.insert(
            "legacy_exit_owner".to_string(),
            node.get("legacy_exit")
                .and_then(|exit| exit.get("owner"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        nodes.push(Value::Object(projected));
    }

    let mut role_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut horizon_counts: BTreeMap<String, u64> = BTreeMap::new();
    for entry in projected_nodes(doc) {
        *role_counts.entry(str_field(entry.node, "role").to_string()).or_default() += 1;
        *horizon_counts.entry(str_field(entry.node, "release_horizon").to_string()).or_default() +=
            1;
    }
    let classes: Map<String, Value> = conflict_classes(doc)
        .into_iter()
        .map(|(phase, classes)| {
            (
                phase,
                Value::Array(
                    classes
                        .into_iter()
                        .map(|class| Value::Array(class.into_iter().map(Value::String).collect()))
                        .collect(),
                ),
            )
        })
        .collect();

    let mut root = Map::new();
    root.insert("schema".to_string(), Value::String(format!("{SCHEMA_NAME}.graph")));
    root.insert("manifest_schema".to_string(), Value::String(SCHEMA_NAME.to_string()));
    root.insert("canonical_digest".to_string(), Value::String(digest.to_string()));
    root.insert("node_count".to_string(), Value::from(nodes.len() as u64));
    root.insert(
        "role_counts".to_string(),
        Value::Object(role_counts.into_iter().map(|(k, v)| (k, Value::from(v))).collect()),
    );
    root.insert(
        "horizon_counts".to_string(),
        Value::Object(horizon_counts.into_iter().map(|(k, v)| (k, Value::from(v))).collect()),
    );
    root.insert("conflict_classes_by_phase".to_string(), Value::Object(classes));
    root.insert("nodes".to_string(), Value::Array(nodes));
    root.insert(
        "law".to_string(),
        Value::String(
            "stable topology projection only: no current-tree state, frontier, candidate, or readiness claim (#10992/#11001)".to_string(),
        ),
    );
    let mut text = serde_json::to_string_pretty(&Value::Object(root))?;
    text.push('\n');
    Ok(text)
}

/// Reviewer projection: per-phase tables, writer classes, lineages.
fn render_markdown(doc: &Value, digest: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# perl_corpus_train.v1 — stable authority graph");
    out.push('\n');
    let _ =
        writeln!(out, "Generated by `cargo xtask perl-corpus-train graph` from `{MANIFEST_PATH}`.");
    let _ = writeln!(out, "Canonical digest: `{digest}`.");
    out.push('\n');
    out.push_str(
        "This projection is stable topology only. It states no current-tree implementation \
         state, no ready frontier, no candidate state, and no readiness; those belong to \
         #10992 and #11001.\n\n",
    );

    let nodes = projected_nodes(doc);
    let mut by_phase: BTreeMap<&str, Vec<&ProjectedNode<'_>>> = BTreeMap::new();
    for entry in &nodes {
        by_phase.entry(str_field(entry.node, "phase")).or_default().push(entry);
    }
    let _ = writeln!(out, "## Nodes by train phase ({} nodes)", nodes.len());
    for (phase, members) in &by_phase {
        out.push('\n');
        let _ = writeln!(out, "### Phase {phase}");
        out.push('\n');
        out.push_str("| Node | Subject | Role | Horizon | Hard deps | Evidence deps | Authorization | Conflict keys | Legacy exit owner |\n");
        out.push_str("|---|---|---|---|---|---|---|---|---|\n");
        for entry in members {
            let deps = dependency_triples(entry.node);
            let list = |class: &str| -> String {
                let items: Vec<&str> =
                    deps.iter().filter(|(_, c)| *c == class).map(|(t, _)| *t).collect();
                if items.is_empty() { "—".to_string() } else { items.join(", ") }
            };
            let keys = sorted_strings(entry.node, "exclusive_conflict_keys");
            let exit_owner = entry
                .node
                .get("legacy_exit")
                .and_then(|exit| exit.get("owner"))
                .and_then(Value::as_str)
                .unwrap_or("—");
            let _ = writeln!(
                out,
                "| `{}` | {} | {}{} | {} | {} | {} | {} | {} | {} |",
                entry.id,
                str_field(entry.node, "issue_ref"),
                str_field(entry.node, "role"),
                if is_true(entry.node, "selectable") { "" } else { " (non-selectable)" },
                str_field(entry.node, "release_horizon"),
                list("hard"),
                list("evidence"),
                list("authorization"),
                if keys.is_empty() { "—".to_string() } else { keys.join(", ") },
                exit_owner,
            );
        }
    }

    out.push('\n');
    out.push_str("## Conflict-safe writer classes by phase\n\n");
    out.push_str(
        "Selectable nodes sharing an exclusive conflict key form one writer class and are \
         serialized by their dependency path; distinct classes within a phase may proceed in \
         parallel where their dependencies permit. Classes are identities, not reservations.\n\n",
    );
    for (phase, classes) in conflict_classes(doc) {
        let rendered: Vec<String> = classes.iter().map(|class| class.join(" + ")).collect();
        let _ = writeln!(out, "- Phase {phase}: {}", rendered.join(" | "));
    }

    out.push('\n');
    out.push_str("## Supersessions, transfers, and candidate lineages\n\n");
    let mut any = false;
    for entry in &nodes {
        let facts: Vec<String> = ["superseded_by", "duplicate_of", "transferred_to"]
            .iter()
            .filter_map(|field| optional_str(entry.node, field).map(|v| format!("{field}={v}")))
            .collect();
        let lineages = sorted_strings(entry.node, "known_candidate_lineage_refs");
        if facts.is_empty() && lineages.is_empty() {
            continue;
        }
        any = true;
        let _ = writeln!(
            out,
            "- `{}`: {}{}",
            entry.id,
            facts.join(", "),
            if lineages.is_empty() {
                String::new()
            } else {
                format!(
                    "{}lineage {}",
                    if facts.is_empty() { "" } else { "; " },
                    lineages.join(", ")
                )
            }
        );
    }
    if !any {
        out.push_str("- none\n");
    }
    out
}

/// Graphviz projection with edge style by dependency class.
fn render_dot(doc: &Value) -> String {
    let mut out = String::new();
    out.push_str("digraph perl_corpus_train {\n  rankdir=LR;\n  node [shape=box, fontsize=10];\n");
    for entry in projected_nodes(doc) {
        let role = str_field(entry.node, "role");
        let style = if is_true(entry.node, "selectable") { "solid" } else { "dashed" };
        let _ = writeln!(
            out,
            "  \"{}\" [label=\"{}\\n{} {}\\n{}\", style={style}];",
            entry.id,
            entry.id,
            str_field(entry.node, "issue_ref"),
            role,
            str_field(entry.node, "release_horizon")
        );
    }
    for entry in projected_nodes(doc) {
        for (target, class) in dependency_triples(entry.node) {
            let style = match class {
                "hard" => "solid",
                "evidence" => "dotted",
                _ => "bold",
            };
            let _ = writeln!(
                out,
                "  \"{}\" -> \"{}\" [label=\"{}\", style={style}];",
                entry.id, target, class
            );
        }
    }
    out.push_str("}\n");
    out
}

/// Mermaid-safe identifier for a node id or authority reference.
fn mermaid_id(id: &str) -> String {
    id.replace(['#', '-', ' '], "_")
}

/// Mermaid flowchart projection. Every edge endpoint is declared with its
/// label first, including external authorities, so an undeclared identifier
/// never renders as its mangled id.
fn render_mermaid(doc: &Value) -> String {
    let mut out = String::new();
    out.push_str("flowchart LR\n");
    for entry in projected_nodes(doc) {
        let _ = writeln!(
            out,
            "  {}[\"{} {} {}\"]",
            mermaid_id(entry.id),
            entry.id,
            str_field(entry.node, "issue_ref"),
            str_field(entry.node, "role")
        );
    }
    let mut externals: BTreeSet<&str> = BTreeSet::new();
    for entry in projected_nodes(doc) {
        for (target, _) in dependency_triples(entry.node) {
            if target.starts_with('#') {
                externals.insert(target);
            }
        }
    }
    for target in externals {
        let _ = writeln!(out, "  {}([\"{} external authority\"])", mermaid_id(target), target);
    }
    for entry in projected_nodes(doc) {
        for (target, class) in dependency_triples(entry.node) {
            let arrow = match class {
                "hard" => "-->",
                "evidence" => "-.->",
                _ => "==>",
            };
            let _ = writeln!(
                out,
                "  {} {arrow}|{}| {}",
                mermaid_id(entry.id),
                class,
                mermaid_id(target)
            );
        }
    }
    out
}

/// All projections as `(file name, bytes)` in a fixed order.
pub fn render_projections(doc: &Value) -> Result<Vec<(&'static str, String)>> {
    let digest = canonical_digest(doc)?;
    Ok(vec![
        ("train.graph.json", render_json(doc, &digest)?),
        ("train.graph.md", render_markdown(doc, &digest)),
        ("train.graph.dot", render_dot(doc)),
        ("train.graph.mmd", render_mermaid(doc)),
    ])
}

/// Committed projections that differ from a fresh render, or are missing.
fn projection_drift(root: &Path, doc: &Value) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    for (name, expected) in render_projections(doc)? {
        let path = root.join(PROJECTIONS_DIR).join(name);
        match fs::read_to_string(&path) {
            Ok(actual) if actual == expected => {}
            Ok(_) => failures.push(format!(
                "{PROJECTIONS_DIR}/{name}: generated projection drifted from the manifest; rerun `cargo xtask perl-corpus-train graph`"
            )),
            Err(error) => failures.push(format!("{PROJECTIONS_DIR}/{name}: missing or unreadable ({error})")),
        }
    }
    Ok(failures)
}

// ---------------------------------------------------------------------------
// explain-static.
// ---------------------------------------------------------------------------

/// One labelled bullet list in the static packet.
fn render_list(out: &mut String, label: &str, items: &[&str]) {
    let _ = writeln!(out, "{label}:");
    if items.is_empty() {
        out.push_str("  - none\n");
    }
    for item in items {
        let _ = writeln!(out, "  - {item}");
    }
}

/// Render one bounded static node packet. Makes no readiness claim.
pub fn render_explain_static(doc: &Value, node_id: &str) -> Result<String> {
    let nodes = objects(doc, "nodes");
    let Some(node) = nodes.iter().find(|node| str_field(node, "node_id") == node_id) else {
        bail!("node {node_id} is not in {MANIFEST_PATH}");
    };
    let role_of = |id: &str| -> String {
        nodes
            .iter()
            .find(|candidate| str_field(candidate, "node_id") == id)
            .map(|candidate| {
                format!("{} {}", str_field(candidate, "issue_ref"), str_field(candidate, "role"))
            })
            .unwrap_or_else(|| "external authority".to_string())
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "perl-corpus-train explain-static {node_id} (stable packet; no readiness claim)"
    );
    for field in ["issue_ref", "title", "role", "lane", "phase", "release_horizon", "selectable"] {
        let _ = writeln!(
            out,
            "{field}: {}",
            node.get(field)
                .map(|v| v.to_string().trim_matches('"').to_string())
                .unwrap_or_default()
        );
    }
    let _ = writeln!(out, "one_pr_proposition: {}", str_field(node, "one_pr_proposition"));
    let _ = writeln!(out, "authority_before: {}", str_field(node, "authority_before"));
    let _ = writeln!(out, "authority_after: {}", str_field(node, "authority_after"));
    let _ = writeln!(out, "claim_ceiling: {}", str_field(node, "claim_ceiling"));
    out.push_str("dependencies:\n");
    let deps = dependency_triples(node);
    if deps.is_empty() {
        out.push_str("  - none\n");
    }
    for (target, class) in &deps {
        let _ = writeln!(out, "  - {class} -> {target} ({})", role_of(target));
    }
    let consumers: Vec<String> =
        strings(node, "consumed_by").iter().map(|id| format!("{id} ({})", role_of(id))).collect();
    render_list(&mut out, "consumed_by", &consumers.iter().map(String::as_str).collect::<Vec<_>>());
    render_list(&mut out, "exclusive_conflict_keys", &strings(node, "exclusive_conflict_keys"));
    render_list(&mut out, "semantic_authority_refs", &strings(node, "semantic_authority_refs"));
    render_list(&mut out, "likely_owned_surfaces", &strings(node, "likely_owned_surfaces"));
    render_list(&mut out, "forbidden_surfaces", &strings(node, "forbidden_surfaces"));
    render_list(
        &mut out,
        "positive_proof_obligations",
        &strings(node, "positive_proof_obligations"),
    );
    let _ = writeln!(out, "first_falsifier: {}", str_field(node, "first_falsifier"));
    render_list(&mut out, "negative_controls", &strings(node, "negative_controls"));
    render_list(&mut out, "repository_verification", &strings(node, "repository_verification"));
    render_list(&mut out, "owned_generated_artifacts", &strings(node, "owned_generated_artifacts"));
    if let Some(spec) = node.get("spec").and_then(Value::as_object) {
        let _ = writeln!(
            out,
            "spec: requirement={} owner={}",
            str_field(spec, "requirement"),
            str_field(spec, "owner")
        );
    }
    render_list(&mut out, "stop_conditions", &strings(node, "stop_conditions"));
    if let Some(exit) = node.get("legacy_exit").and_then(Value::as_object) {
        let _ = writeln!(
            out,
            "legacy_exit: owner={} condition={}",
            optional_str(exit, "owner").unwrap_or("none"),
            optional_str(exit, "condition").unwrap_or("none")
        );
    }
    for field in ["superseded_by", "duplicate_of", "transferred_to"] {
        if let Some(value) = optional_str(node, field) {
            let _ = writeln!(out, "{field}: {value}");
        }
    }
    render_list(
        &mut out,
        "known_candidate_lineage_refs",
        &strings(node, "known_candidate_lineage_refs"),
    );
    let _ = writeln!(out, "candidate_reuse_policy: {}", str_field(node, "candidate_reuse_policy"));
    out.push_str(
        "readiness: not evaluated here; current-tree state and frontier belong to #10992, live candidates to #11001\n",
    );
    Ok(out)
}

// ---------------------------------------------------------------------------
// CLI entry points.
// ---------------------------------------------------------------------------

/// `check`: schema, named laws, shuffled determinism control, every
/// discriminating invalid fixture, and projection freshness.
pub fn run_check() -> Result<()> {
    let root = project_root()?;
    let mut failures: Vec<String> = Vec::new();

    let manifest = load_json(&root.join(MANIFEST_PATH))?;
    for failure in schema_failures(&root, &manifest)? {
        failures.push(format!("{MANIFEST_PATH}: {failure}"));
    }
    for violation in validate_document(&manifest) {
        failures.push(format!("{MANIFEST_PATH}: {}: {}", violation.code, violation.detail));
    }

    let shuffled = load_json(&root.join(SHUFFLED_PATH))?;
    if canonical_form(&manifest) != canonical_form(&shuffled)
        || canonical_digest(&manifest)? != canonical_digest(&shuffled)?
    {
        failures.push(format!(
            "{SHUFFLED_PATH}: canonical form or digest differs from {MANIFEST_PATH} under reordering"
        ));
    }
    let shuffled_violations = validate_document(&shuffled);
    if !shuffled_violations.is_empty() {
        failures.push(format!(
            "{SHUFFLED_PATH}: shuffled control violated the manifest contract: {:?}",
            violation_codes(&shuffled_violations)
        ));
    }
    if render_projections(&manifest)? != render_projections(&shuffled)? {
        failures.push(format!("{SHUFFLED_PATH}: projections differ under reordering"));
    }

    let expected_errors = load_json(&root.join(INVALID_DIR).join(EXPECTED_ERRORS_FILENAME))?;
    let Some(expected) = expected_errors.as_object() else {
        bail!("{EXPECTED_ERRORS_FILENAME} must be an object");
    };
    if expected.is_empty() {
        bail!("{EXPECTED_ERRORS_FILENAME}: at least one expectation required");
    }
    // The fixture directory and the expectation map must name exactly the
    // same files: an unlisted fixture is dead proof, a missing one is a lie.
    let present = invalid_fixture_names(&root)?;
    let listed: BTreeSet<String> = expected.keys().cloned().collect();
    for unlisted in present.difference(&listed) {
        failures.push(format!(
            "invalid/{unlisted}: fixture present but not listed in {EXPECTED_ERRORS_FILENAME}"
        ));
    }
    for missing in listed.difference(&present) {
        failures
            .push(format!("invalid/{missing}: listed in {EXPECTED_ERRORS_FILENAME} but absent"));
    }
    for (filename, expected_code) in expected {
        let Some(expected_code) = expected_code.as_str() else {
            bail!("{EXPECTED_ERRORS_FILENAME}: {filename} must name a string reason code");
        };
        if !present.contains(filename) {
            continue;
        }
        let doc = load_json(&root.join(INVALID_DIR).join(filename))?;
        let codes: BTreeSet<String> = if expected_code == "SCHEMA_VIOLATION" {
            // The schema must be the sole discriminator: the semantic layer
            // still runs so a schema fixture that also trips a graph law is
            // reported as a second code rather than concealed.
            let mut codes: BTreeSet<String> = BTreeSet::new();
            if !schema_failures(&root, &doc)?.is_empty() {
                codes.insert("SCHEMA_VIOLATION".to_string());
            }
            codes.extend(violation_codes(&validate_document(&doc)).iter().map(|c| c.to_string()));
            codes
        } else {
            let schema = schema_failures(&root, &doc)?;
            if !schema.is_empty() {
                failures.push(format!(
                    "invalid/{filename}: fixture must be schema-valid so the semantic law is what discriminates; {}",
                    schema.join("; ")
                ));
            }
            violation_codes(&validate_document(&doc)).iter().map(|c| c.to_string()).collect()
        };
        // Exactly one law discriminates each fixture: a second code means the
        // fixture no longer isolates the law it is named for.
        let want: BTreeSet<String> = BTreeSet::from([expected_code.to_string()]);
        if codes.is_empty() {
            failures.push(format!(
                "invalid/{filename}: expected failure {expected_code}, document validated"
            ));
        } else if codes != want {
            failures.push(format!(
                "invalid/{filename}: expected exactly {{{expected_code}}}, got {codes:?}"
            ));
        }
    }

    failures.extend(projection_drift(&root, &manifest)?);

    if failures.is_empty() {
        println!(
            "{SCHEMA_NAME}: schema, named laws, shuffled determinism control, every discriminating invalid fixture, and generated projections valid"
        );
        Ok(())
    } else {
        bail!("perl-corpus train manifest check failed:\n{}", failures.join("\n"));
    }
}

/// `graph`: regenerate the projections, or with `check` verify they are
/// byte-identical to the committed ones.
pub fn run_graph(check: bool) -> Result<()> {
    let root = project_root()?;
    let manifest = load_validated_manifest(&root)?;
    if check {
        let failures = projection_drift(&root, &manifest)?;
        if !failures.is_empty() {
            bail!("perl-corpus train projections are stale:\n{}", failures.join("\n"));
        }
        println!("{SCHEMA_NAME}: generated projections are current");
        return Ok(());
    }
    let dir = root.join(PROJECTIONS_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    for (name, text) in render_projections(&manifest)? {
        let path = dir.join(name);
        fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))?;
        println!("wrote {PROJECTIONS_DIR}/{name}");
    }
    Ok(())
}

/// `explain-static <node>`: one bounded static node packet.
pub fn run_explain_static(node_id: &str) -> Result<()> {
    let root = project_root()?;
    let manifest = load_validated_manifest(&root)?;
    print!("{}", render_explain_static(&manifest, node_id)?);
    Ok(())
}
