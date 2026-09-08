//! Validate the stable native Neovim implementation train manifest
//! (`native_neovim_train.v1`, issue #11392).
//!
//! The manifest is stable reviewed programme data only: node/proposition
//! identities, roles, claim-profile membership, typed relationships,
//! writer/conflict metadata, proof/spec references, and stop/transfer
//! contracts. This module enforces the manifest's own shift-left rejection
//! law with named diagnostics and never infers current implementation state,
//! derives a frontier, inspects GitHub, launches Neovim, evaluates receipts,
//! promotes support, or embeds mutable GitHub/task/agent state.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const MANIFEST_PATH: &str = ".spec/11392-native-neovim-train-graph/train.manifest.json";
const SHUFFLED_PATH: &str = ".spec/11392-native-neovim-train-graph/shuffled/train.manifest.json";
const INVALID_DIR: &str = ".spec/11392-native-neovim-train-graph/invalid";
const EXPECTED_ERRORS_FILENAME: &str = "expected_errors.json";
const SCHEMA_PATH: &str = "schemas/native_neovim_train.v1.schema.json";

/// Shared host execution authority every actual-host row must bind (#10894;
/// recurrence #10899, durable spec #11766 fly separately).
const SHARED_HOST_EXECUTION_NODE: &str = "nv_shared_host_execution_adoption";

/// Thin native host adapter binding the shared execution authority for its
/// consumers (#10503).
const THIN_NATIVE_HOST_ADAPTER_NODE: &str = "nv_thin_native_host_adapter";

/// The universal closeout composition covers every row (including instruments
/// and the DAP sidecar reaching explicit terminal dispositions), so its fan-in
/// is the one lawful place those families appear as children.
const CLOSEOUT_COMPOSITION_NODE: &str = "nv_closeout_terminal_composition";

/// Generic actual-editor evidence anchors (#7777 receipt contract,
/// #10527 hardening) that actual-host rows must cite.
const ACTUAL_HOST_REQUIRED_REFS: &[&str] = &["#7777", "#10527"];

/// Dependency classes that constitute hard/cross-subject satisfaction between
/// sibling subjects.
const SATISFACTION_CLASSES: &[&str] = &["requires_implementation", "requires_behavior_for_claim"];

/// Behavioral profile -> allowed proposition families. Instrument rows
/// (host_toolchain_action_receipt_roles) and DAP sidecar rows are structurally
/// excluded everywhere; closeout is universal and exempt.
const PROFILE_ALLOWED_FAMILIES: &[(&str, &[&str])] = &[
    ("native_neovim_core", &["core_baseline"]),
    ("release_v0_18_bounded", &["release_envelope", "core_baseline"]),
    ("native_neovim_configuration", &["core_baseline"]),
    (
        "native_neovim_deep_lifecycle",
        &["atomic_deep", "parser_artifact_deep", "lifecycle_race_deep", "control_plane"],
    ),
    (
        "native_neovim_first_class",
        &["version_platform", "install_channels", "upstream_external", "support_docs"],
    ),
];

const UNIVERSAL_PROFILE_ID: &str = "native_neovim_programme_closeout";

/// Banned object keys anywhere in the document: no mutable GitHub, task,
/// agent, run, or frontier state may be embedded in stable bytes.
const BANNED_KEYS: &[&str] = &[
    "live_frontier",
    "live_state",
    "github_state",
    "task_state",
    "agent_state",
    "head_sha",
    "current_sha",
    "check_run",
    "workflow_run",
    "latest_check",
    "issue_status",
    "frontier_cache",
];

#[derive(Debug)]
pub struct Violation {
    pub code: String,
    pub detail: String,
}

impl Violation {
    fn new(code: &str, detail: impl Into<String>) -> Self {
        Self { code: code.to_string(), detail: detail.into() }
    }
}

fn violation_codes(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(|violation| violation.code.as_str()).collect()
}

fn strings<'a>(node: &'a Map<String, Value>, key: &str) -> Vec<&'a str> {
    node.get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn str_field<'a>(node: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    node.get(key).and_then(Value::as_str)
}

fn is_true(node: &Map<String, Value>, key: &str) -> bool {
    node.get(key).and_then(Value::as_bool) == Some(true)
}

/// Canonical deterministic form: objects sort by key and arrays sort by their
/// canonical serialization, so semantically identical documents produce
/// identical bytes regardless of authoring order.
pub fn canonical_form(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            let mut out = Map::new();
            for (key, item) in sorted {
                out.insert(key.clone(), canonical_form(item));
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            let mut canonical_items: Vec<Value> = items.iter().map(canonical_form).collect();
            canonical_items.sort_by(|left, right| {
                serde_json::to_string(left)
                    .unwrap_or_default()
                    .cmp(&serde_json::to_string(right).unwrap_or_default())
            });
            Value::Array(canonical_items)
        }
        other => other.clone(),
    }
}

fn walk_banned_keys(value: &Value, path: &str, violations: &mut Vec<Violation>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if BANNED_KEYS.contains(&key.as_str()) {
                    violations.push(Violation::new(
                        "MUTABLE_STATE_EMBEDDED",
                        format!("{path}.{key}: live GitHub/task/agent state key embedded in stable manifest bytes"),
                    ));
                }
                walk_banned_keys(child, &format!("{path}.{key}"), violations);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                walk_banned_keys(child, &format!("{path}[{index}]"), violations);
            }
        }
        _ => {}
    }
}

fn node_role_family_problems(doc: &Value, violations: &mut Vec<Violation>) {
    let empty = Vec::new();
    let nodes = doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let profiles = doc.get("claim_profiles").and_then(Value::as_array).unwrap_or(&empty);

    let mut family_by_id: BTreeMap<&str, &str> = BTreeMap::new();
    let mut instrument_by_id: BTreeMap<&str, bool> = BTreeMap::new();
    for node in nodes.iter().filter_map(Value::as_object) {
        if let (Some(id), Some(family)) =
            (str_field(node, "node_id"), str_field(node, "proposition_family"))
        {
            family_by_id.insert(id, family);
            instrument_by_id.insert(id, node.contains_key("host_toolchain_action_receipt_roles"));
        }
    }

    // Conditional claim profiles pair their selecting authority exactly; an
    // undeclared or unpaired gate fails closed.
    let authority_ids: Vec<&str> = doc
        .get("selecting_authorities")
        .and_then(Value::as_array)
        .map(|authorities| {
            authorities
                .iter()
                .filter_map(|authority| authority.get("id"))
                .filter_map(Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    for profile in profiles.iter().filter_map(Value::as_object) {
        let profile_id = str_field(profile, "id").unwrap_or("");
        if let Some(authority) = str_field(profile, "selecting_authority") {
            if !authority_ids.contains(&authority) {
                violations.push(Violation::new(
                    "UNQUALIFIED_RELEASE_GATE",
                    format!(
                        "profile {profile_id}: selecting authority {authority} is not declared"
                    ),
                ));
            }
            if str_field(profile, "selection_subject").is_none() {
                violations.push(Violation::new(
                    "UNQUALIFIED_RELEASE_GATE",
                    format!("profile {profile_id}: declaring a selecting authority requires selection_subject"),
                ));
            }
        }
    }

    let mut seen_profile_ids: BTreeSet<&str> = BTreeSet::new();
    for profile in profiles.iter().filter_map(Value::as_object) {
        let Some(profile_id) = str_field(profile, "id") else { continue };
        if !seen_profile_ids.insert(profile_id) {
            violations.push(Violation::new(
                "DUPLICATE_PROFILE_IDENTITY",
                format!("claim profile {profile_id}: duplicate profile identity"),
            ));
        }
        // Only the closeout profile is universal; no other profile may claim
        // exemption from behavioral family membership even though the closed
        // schema permits the policy field syntactically.
        if profile_id == UNIVERSAL_PROFILE_ID || profile.get("universal_member_policy").is_some() {
            if profile_id != UNIVERSAL_PROFILE_ID {
                violations.push(Violation::new(
                    "PROFILE_FAMILY_MISMATCH",
                    format!("claim profile {profile_id}: universal_member_policy is reserved for {UNIVERSAL_PROFILE_ID}"),
                ));
            }
            continue;
        }
        let allowed = PROFILE_ALLOWED_FAMILIES
            .iter()
            .find(|(id, _)| *id == profile_id)
            .map(|(_, families)| *families);
        let member_ids = strings(profile, "members");
        for member in &member_ids {
            let Some(family) = family_by_id.get(*member) else {
                violations.push(Violation::new(
                    "UNKNOWN_PROFILE_MEMBER",
                    format!("profile {profile_id}: member {member} does not resolve to a node"),
                ));
                continue;
            };
            let Some(allowed) = allowed else {
                violations.push(Violation::new(
                    "PROFILE_FAMILY_UNDECLARED",
                    format!(
                        "profile {profile_id}: no declared allowed proposition-family set; membership cannot be judged closed"
                    ),
                ));
                continue;
            };
            if !allowed.contains(family) {
                let code = if instrument_by_id.get(*member) == Some(&true) {
                    "HOST_PROVISIONING_AS_BEHAVIOR"
                } else if *family == "dap_sidecar" && profile_id == "native_neovim_core" {
                    "SIDECAR_IN_CORE_PROFILE"
                } else if profile_id == "native_neovim_core" && family.ends_with("_deep") {
                    "DEEP_CELL_IN_CORE_PROFILE"
                } else {
                    "PROFILE_FAMILY_MISMATCH"
                };
                violations.push(Violation::new(
                    code,
                    format!("profile {profile_id}: member {member} has family {family}, outside the declared behavioral family set"),
                ));
            }
        }
        // DAP sidecar nodes may never enter any profile at all.
        for member in &member_ids {
            if family_by_id.get(*member) == Some(&"dap_sidecar")
                && profile_id != "native_neovim_core"
            {
                violations.push(Violation::new(
                    "SIDECAR_IN_CORE_PROFILE",
                    format!(
                        "profile {profile_id}: DAP sidecar member {member} entered a claim profile"
                    ),
                ));
            }
        }
    }
}

fn identity_and_row_contract_problems(doc: &Value, violations: &mut Vec<Violation>) {
    let empty = Vec::new();
    let nodes = doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    let mut seen_issues: BTreeSet<i64> = BTreeSet::new();
    let mut conflict_keys: BTreeMap<&str, &str> = BTreeMap::new();

    for node in nodes.iter().filter_map(Value::as_object) {
        let id = str_field(node, "node_id").unwrap_or("");
        let role = str_field(node, "role").unwrap_or("");

        if !seen_ids.insert(id) {
            violations.push(Violation::new(
                "DUPLICATE_NODE_IDENTITY",
                format!("node {id}: duplicate node/proposition identity"),
            ));
        }
        if let Some(issue) = node.get("issue").and_then(Value::as_i64)
            && !seen_issues.insert(issue)
        {
            violations.push(Violation::new(
                "DUPLICATE_PRIMARY_ISSUE",
                format!("node {id}: issue #{issue} already owns another primary row"),
            ));
        }

        let buildable = is_true(node, "buildable");
        if buildable && role == "controller" {
            violations.push(Violation::new(
                "CONTROLLER_EMIT_AS_WORK",
                format!("node {id}: controller emitted as direct builder work"),
            ));
        }

        if let Some(key) = node
            .get("writer_slot")
            .and_then(Value::as_object)
            .and_then(|writer| str_field(writer, "conflict_key"))
        {
            if buildable {
                if let Some(previous) = conflict_keys.insert(key, id) {
                    violations.push(Violation::new(
                        "OVERLAPPING_CONFLICT_KEYS",
                        format!(
                            "node {id}: conflict key {key} already assigned to node {previous}"
                        ),
                    ));
                }
            } else if role == "controller" {
                violations.push(Violation::new(
                    "CONTROLLER_WRITER_SLOT",
                    format!("node {id}: controller row carries a writer slot; controllers are never emitted as direct builder work"),
                ));
            }
        }

        if buildable {
            let missing_falsifier =
                str_field(node, "cheapest_discriminating_falsifier").map(str::len).unwrap_or(0)
                    < 20;
            let missing_proof_owner = str_field(node, "focused_proof_generation_owner").is_none();
            let missing_negatives = strings(node, "negative_controls").is_empty();
            let missing_review = strings(node, "review_map_requirements").is_empty();
            let missing_rollback =
                str_field(node, "rollback_shape").map(str::len).unwrap_or(0) < 15;
            let missing_stop =
                str_field(node, "codex_stop_transfer_condition").map(str::len).unwrap_or(0) < 15;
            let has_writer_slot = node
                .get("writer_slot")
                .and_then(Value::as_object)
                .is_some_and(|writer| str_field(writer, "conflict_key").is_some());
            if missing_falsifier
                || missing_proof_owner
                || missing_negatives
                || missing_review
                || missing_rollback
                || missing_stop
                || !has_writer_slot
            {
                violations.push(Violation::new(
                    "INCOMPLETE_ROW_CONTRACT",
                    format!(
                        "node {id}: builder row must record falsifier, proof owner, negative controls, review-map requirements, rollback, stop condition, and writer slot"
                    ),
                ));
            }
        }

        if is_true(node, "requires_actual_host") {
            let deps: Vec<&str> = node
                .get("dependencies")
                .and_then(Value::as_array)
                .map(|deps| {
                    deps.iter().filter_map(|d| d.get("target")).filter_map(Value::as_str).collect()
                })
                .unwrap_or_default();
            let refs = strings(node, "proof_spec_references");
            // The shared host execution authority binds either directly or
            // through the thin native host adapter, which itself requires it.
            let binds_shared_host_execution = deps.iter().any(|target| {
                *target == SHARED_HOST_EXECUTION_NODE || *target == THIN_NATIVE_HOST_ADAPTER_NODE
            });
            if !binds_shared_host_execution {
                violations.push(Violation::new(
                    "ACTUAL_HOST_UNBOUND",
                    format!("node {id}: actual-host row does not bind the shared host execution authority ({SHARED_HOST_EXECUTION_NODE})"),
                ));
            }
            for anchor in ACTUAL_HOST_REQUIRED_REFS {
                if !refs.iter().any(|reference| reference.starts_with(anchor)) {
                    violations.push(Violation::new(
                        "ACTUAL_HOST_UNBOUND",
                        format!("node {id}: actual-host row does not cite generic actual-editor evidence anchor {anchor}"),
                    ));
                }
            }
        }
    }
}

fn edge_problems(doc: &Value, violations: &mut Vec<Violation>) {
    let empty = Vec::new();
    let nodes = doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let authorities: Vec<Value> = doc
        .get("selecting_authorities")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()))
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut ids: BTreeSet<&str> = BTreeSet::new();
    let mut externals: BTreeSet<&str> = BTreeSet::new();
    for node in nodes.iter().filter_map(Value::as_object) {
        if let Some(id) = str_field(node, "node_id") {
            ids.insert(id);
        }
    }
    for authority in doc.get("external_authorities").and_then(Value::as_array).unwrap_or(&empty) {
        if let Some(id) = authority.get("id").and_then(Value::as_str) {
            externals.insert(id);
        }
    }
    // Namespace law: external-checkpoint classes target declared external
    // authorities; every other class targets manifest nodes.
    let external_classes: &[&str] =
        &["external_submission", "external_acceptance", "released_public"];
    let external_stage_by_class: &[(&str, &str)] = &[
        ("external_submission", "external_submission"),
        ("external_acceptance", "external_acceptance"),
        ("released_public", "released_public_availability"),
    ];

    for node in nodes.iter().filter_map(Value::as_object) {
        let source = str_field(node, "node_id").unwrap_or("").to_string();
        let source_family = str_field(node, "proposition_family").unwrap_or("");
        let source_group = node
            .get("writer_slot")
            .and_then(|slot| slot.get("parallel_group"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        for dep in node.get("dependencies").and_then(Value::as_array).unwrap_or(&empty) {
            let Some(dep) = dep.as_object() else { continue };
            let target = str_field(dep, "target").unwrap_or("");
            let class = str_field(dep, "class").unwrap_or("");

            if !ids.contains(target) && !externals.contains(target) {
                violations.push(Violation::new(
                    "UNKNOWN_EDGE_TARGET",
                    format!("node {source}: dependency target {target} resolves to no node or external authority"),
                ));
                continue;
            }
            let class_is_external = external_classes.contains(&class);
            if class_is_external && !externals.contains(target) {
                violations.push(Violation::new(
                    "EXTERNAL_TARGET_NAMESPACE",
                    format!("node {source}: external checkpoint class {class} must target a declared external authority, not node {target}"),
                ));
            }
            if !class_is_external && !ids.contains(target) {
                violations.push(Violation::new(
                    "INTERNAL_TARGET_NAMESPACE",
                    format!("node {source}: dependency class {class} must target a manifest node, not external authority {target}"),
                ));
            }

            if SATISFACTION_CLASSES.contains(&class) {
                let target_node = nodes
                    .iter()
                    .filter_map(Value::as_object)
                    .find(|object| str_field(object, "node_id") == Some(target));
                if let Some(target_node) = target_node {
                    let target_family = str_field(target_node, "proposition_family").unwrap_or("");
                    if target_family == "dap_sidecar" && source_family != "dap_sidecar" {
                        violations.push(Violation::new(
                            "SIDECAR_IN_SPINE_CLOSURE",
                            format!("node {source}: {class} edge enters the DAP sidecar node {target} from outside the sidecar lane"),
                        ));
                    }
                    if target_family == source_family && !source_group.is_empty() {
                        let target_group = target_node
                            .get("writer_slot")
                            .and_then(|slot| slot.get("parallel_group"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if source_group == target_group {
                            violations.push(Violation::new(
                                "SIBLING_SATISFACTION",
                                format!(
                                    "node {source}: {class} edge targets sibling {target} inside the same independence cohort ({source_family}/{source_group}); one subject can never satisfy another"
                                ),
                            ));
                        }
                    }
                }
            }

            if let Some((class_name, stage)) =
                external_stage_by_class.iter().find(|(name, _)| *name == class)
            {
                let declared = str_field(dep, "stage");
                if declared.is_none() || declared != Some(*stage) {
                    violations.push(Violation::new(
                        "EXTERNAL_STAGE_MISMATCH",
                        format!("node {source}: dependency class {class_name} requires exact stage {stage}"),
                    ));
                }
            }
        }

        // Release gates: full qualification plus branch contradiction law.
        let mut selection_values: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
        for gate in node.get("release_gates").and_then(Value::as_array).unwrap_or(&empty) {
            let Some(gate) = gate.as_object() else { continue };
            let predecessor = str_field(gate, "active_predecessor").unwrap_or("");
            let authority = str_field(gate, "selecting_authority").unwrap_or("");
            let subject = str_field(gate, "selection_subject").unwrap_or("");
            let selected = str_field(gate, "selected_value").unwrap_or("");

            if !ids.contains(predecessor) {
                violations.push(Violation::new(
                    "UNQUALIFIED_RELEASE_GATE",
                    format!("node {source}: gate predecessor {predecessor} does not resolve"),
                ));
            }
            let declared = authorities.iter().find_map(|candidate| {
                let object = candidate.as_object()?;
                (str_field(object, "id") == Some(authority)).then(|| object.clone())
            });
            match declared {
                None => violations.push(Violation::new(
                    "UNQUALIFIED_RELEASE_GATE",
                    format!("node {source}: selecting authority {authority} is not declared"),
                )),
                Some(object) => {
                    let allowed: Vec<&str> = strings(&object, "allowed_values");
                    if !allowed.contains(&selected) {
                        violations.push(Violation::new(
                            "UNQUALIFIED_RELEASE_GATE",
                            format!("node {source}: selected value {selected} is not an allowed value of {authority}"),
                        ));
                    }
                }
            }
            selection_values
                .entry((authority.to_string(), subject.to_string()))
                .or_default()
                .insert(selected.to_string());
        }
        for ((authority, subject), values) in &selection_values {
            if values.len() > 1 {
                violations.push(Violation::new(
                    "CONTRADICTORY_RELEASE_BRANCHES",
                    format!(
                        "node {source}: gates under authority {authority} subject {subject} select contradictory branches {values:?}; the #8129 decision consumes exactly one branch"
                    ),
                ));
            }
        }

        if let Some(fan_in) = node.get("fan_in").and_then(Value::as_object) {
            if fan_in.get("satisfaction_source").and_then(Value::as_str)
                != Some("independently_terminal_child_propositions")
            {
                violations.push(Violation::new(
                    "FAN_IN_INVALID_COMPOSITION",
                    format!("node {source}: fan-in satisfaction_source must be independently_terminal_child_propositions"),
                ));
            }
            for child in fan_in.get("children").and_then(Value::as_array).unwrap_or(&empty) {
                let Some(child_id) = child.as_str() else { continue };
                if !ids.contains(child_id) {
                    violations.push(Violation::new(
                        "UNKNOWN_FAN_IN_CHILD",
                        format!("node {source}: fan-in child {child_id} does not resolve"),
                    ));
                    continue;
                }
                let child_family = nodes
                    .iter()
                    .filter_map(Value::as_object)
                    .find(|object| str_field(object, "node_id") == Some(child_id))
                    .and_then(|object| str_field(object, "proposition_family"))
                    .unwrap_or("");
                if (child_family == "host_instrument" || child_family == "dap_sidecar")
                    && source != CLOSEOUT_COMPOSITION_NODE
                {
                    violations.push(Violation::new(
                        "FAN_IN_INVALID_COMPOSITION",
                        format!("node {source}: fan-in composes instrument/sidecar child {child_id}; instruments never count as product behavior evidence"),
                    ));
                }
            }
        }
    }
}

fn hard_cycle_exists(nodes: &[Value]) -> Option<Vec<String>> {
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut buildable: BTreeSet<&str> = BTreeSet::new();
    for node in nodes.iter().filter_map(Value::as_object) {
        let Some(id) = str_field(node, "node_id") else { continue };
        if is_true(node, "buildable") {
            buildable.insert(id);
        }
    }
    let empty_deps: Vec<Value> = Vec::new();
    for node in nodes.iter().filter_map(Value::as_object) {
        let Some(id) = str_field(node, "node_id") else { continue };
        for dep in node.get("dependencies").and_then(Value::as_array).unwrap_or(&empty_deps) {
            let class = dep.get("class").and_then(Value::as_str).unwrap_or("");
            let target = dep.get("target").and_then(Value::as_str).unwrap_or("");
            if class == "requires_implementation" && buildable.contains(target) {
                graph.entry(id).or_default().push(target);
            }
        }
    }
    // Iterative DFS with colors: 0 unvisited, 1 in stack, 2 done.
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    let mut path: Vec<&str> = Vec::new();

    fn visit<'a>(
        node: &'a str,
        graph: &BTreeMap<&'a str, Vec<&'a str>>,
        color: &mut BTreeMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match color.get(node) {
            Some(1) => {
                let start = path.iter().position(|item| *item == node).unwrap_or(0);
                return Some(path[start..].iter().map(|item| item.to_string()).collect());
            }
            Some(_) => return None,
            None => {}
        }
        color.insert(node, 1);
        path.push(node);
        if let Some(targets) = graph.get(node) {
            for target in targets.clone() {
                if let Some(cycle) = visit(target, graph, color, path) {
                    return Some(cycle);
                }
            }
        }
        path.pop();
        color.insert(node, 2);
        None
    }

    for id in buildable.clone() {
        if let Some(cycle) = visit(id, &graph, &mut color, &mut path) {
            return Some(cycle);
        }
    }
    None
}

pub fn validate_document(doc: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    walk_banned_keys(doc, "$", &mut violations);
    identity_and_row_contract_problems(doc, &mut violations);
    node_role_family_problems(doc, &mut violations);
    edge_problems(doc, &mut violations);
    let empty = Vec::new();
    let nodes = doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    if let Some(cycle) = hard_cycle_exists(nodes) {
        violations.push(Violation::new(
            "HARD_IMPLEMENTATION_CYCLE",
            format!("requires_implementation cycle through builder rows: {}", cycle.join(" -> ")),
        ));
    }
    violations
}

fn load_json(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON in {}", path.display()))
}

/// Entry point: validate the closed schema, the canonical manifest, the
/// shuffled determinism control, and every discriminating invalid fixture.
pub fn run() -> Result<()> {
    let root = project_root()?;
    let mut failures: Vec<String> = Vec::new();

    // Structural authority: the closed JSON Schema is actually applied.
    let schema = load_json(&root.join(SCHEMA_PATH))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| color_eyre::eyre::eyre!("{SCHEMA_PATH}: invalid schema: {error}"))?;
    let manifest_path = root.join(MANIFEST_PATH);
    let manifest = load_json(&manifest_path)?;

    for error in validator.iter_errors(&manifest) {
        failures.push(format!("{MANIFEST_PATH}: schema violation: {error}"));
    }

    // Graph semantics: the named shift-left diagnostics actually run.
    let violations = validate_document(&manifest);
    for violation in &violations {
        failures.push(format!("{MANIFEST_PATH}: {}: {}", violation.code, violation.detail));
    }

    // Deterministic serialization control: the shuffled control must canonize
    // identically and also pass semantic validation.
    let shuffled = load_json(&root.join(SHUFFLED_PATH))?;
    if canonical_form(&manifest) != canonical_form(&shuffled) {
        failures.push(format!(
            "{SHUFFLED_PATH}: canonical form differs from {} under array/key reordering",
            MANIFEST_PATH
        ));
    }
    let shuffled_violations = validate_document(&shuffled);
    if !shuffled_violations.is_empty() {
        failures.push(format!(
            "{SHUFFLED_PATH}: shuffled control violated the manifest contract: {:?}",
            violation_codes(&shuffled_violations)
        ));
    }

    // Invalid fixtures fail closed with exactly the named reason code.
    let expected_errors = load_json(&root.join(INVALID_DIR).join(EXPECTED_ERRORS_FILENAME))?;
    let expected = expected_errors
        .as_object()
        .ok_or_else(|| color_eyre::eyre::eyre!("{EXPECTED_ERRORS_FILENAME} must be an object"))?;
    if expected.is_empty() {
        bail!("{EXPECTED_ERRORS_FILENAME}: at least one expectation required");
    }
    for (filename, expected_code) in expected {
        let Some(expected_code) = expected_code.as_str() else {
            bail!("{EXPECTED_ERRORS_FILENAME}: {filename} must name a string reason code");
        };
        let doc = load_json(&root.join(INVALID_DIR).join(filename))?;
        let violations = validate_document(&doc);
        let codes = violation_codes(&violations);
        if codes.is_empty() {
            failures.push(format!(
                "invalid/{filename}: expected failure {expected_code}, manifest validated"
            ));
        } else if !codes.contains(&expected_code) {
            failures.push(format!(
                "invalid/{filename}: expected failure {expected_code}, got {codes:?}"
            ));
        }
    }

    if failures.is_empty() {
        println!(
            "native_neovim_train.v1: schema, canonical manifest, shuffled determinism control, and all discriminating invalid fixtures valid"
        );
        Ok(())
    } else {
        bail!("native neovim train manifest check failed:\n{}", failures.join("\n"));
    }
}

#[cfg(test)]
mod tests;
