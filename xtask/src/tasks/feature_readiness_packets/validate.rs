//! Fail-closed deterministic validation of feature-readiness packets.
//!
//! Every violation carries a stable reason code so negative controls can
//! assert exact failures. Validation never repairs: a violating document is
//! never rendered, and a missing input fails instead of becoming prose.

use serde_json::Value;

use super::build;
use super::denominator;
use super::model::{DenominatorDisposition, NodeSpec};
use super::nodes;

/// Apply the checked-in JSON Schema to a generated instance. The handwritten
/// validator remains responsible for domain relationships; this catches wire
/// shape drift at the actual producer boundary too.
pub fn validate_schema_instance(doc: &Value) -> Vec<Violation> {
    let schema_path =
        if doc.get("schema").and_then(Value::as_str) == Some(super::build::BUILDER_SCHEMA) {
            "schemas/feature_readiness_builder_packet.v1.schema.json"
        } else {
            "schemas/feature_readiness_reviewer_packet.v1.schema.json"
        };
    let root = match crate::utils::project_root() {
        Ok(root) => root,
        Err(error) => return vec![Violation::new("schema_instrument_failure", error.to_string())],
    };
    let text = match std::fs::read_to_string(root.join(schema_path)) {
        Ok(text) => text,
        Err(error) => {
            return vec![Violation::new(
                "schema_instrument_failure",
                format!("reading {schema_path}: {error}"),
            )];
        }
    };
    let schema: Value = match serde_json::from_str(&text) {
        Ok(schema) => schema,
        Err(error) => {
            return vec![Violation::new(
                "schema_instrument_failure",
                format!("parsing {schema_path}: {error}"),
            )];
        }
    };
    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(error) => {
            return vec![Violation::new(
                "schema_instrument_failure",
                format!("compiling {schema_path}: {error}"),
            )];
        }
    };
    validator
        .iter_errors(doc)
        .map(|error| Violation::new("json_schema_violation", format!("{schema_path}: {error}")))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub code: String,
    pub detail: String,
}

/// Check the packet registry against the versioned denominator authority
/// from #11279/#11286. This catches silent fixture growth, omission, role or
/// disposition drift, duplicate identities, and accidental inclusion of the
/// controller/observational planes.
pub fn validate_registry_denominator(registry: &[NodeSpec]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let expected = denominator::ENTRIES;
    let expected_nodes: std::collections::BTreeSet<&str> =
        expected.iter().filter_map(|entry| entry.packet_node).collect();
    let actual_nodes: std::collections::BTreeSet<&str> =
        registry.iter().map(|node| node.node_id).collect();
    for missing in expected_nodes.difference(&actual_nodes) {
        violations.push(Violation::new(
            "denominator_missing",
            format!("expected packet node {missing} is absent"),
        ));
    }
    for unexpected in actual_nodes.difference(&expected_nodes) {
        violations.push(Violation::new(
            "denominator_unexpected",
            format!("packet node {unexpected} is outside the controlling denominator"),
        ));
    }
    let mut seen_issues = std::collections::BTreeSet::new();
    let mut seen_nodes = std::collections::BTreeSet::new();
    for node in registry {
        if !seen_nodes.insert(node.node_id) {
            violations.push(Violation::new(
                "duplicate_node_identity",
                format!("packet node {} appears more than once", node.node_id),
            ));
        }
        for issue in &node.issues {
            if !seen_issues.insert(*issue) {
                violations.push(Violation::new(
                    "duplicate_issue_identity",
                    format!("issue #{issue} is assigned to more than one packet node"),
                ));
            }
        }
        let Some(entry) = expected.iter().find(|entry| entry.packet_node == Some(node.node_id))
        else {
            continue;
        };
        if node.issues != vec![entry.issue] {
            violations.push(Violation::new(
                "denominator_issue_mismatch",
                format!(
                    "{} maps to issue {:?}, expected #{}",
                    node.node_id, node.issues, entry.issue
                ),
            ));
        }
        let expected_disposition = match entry.disposition {
            DenominatorDisposition::Actionable => "actionable",
            DenominatorDisposition::Deferred => "deferred",
            DenominatorDisposition::Excluded => "blocked_external_manual",
        };
        if node.disposition.as_str() != expected_disposition {
            violations.push(Violation::new(
                "denominator_disposition_mismatch",
                format!(
                    "{} is {}, expected {} ({})",
                    node.node_id,
                    node.disposition.as_str(),
                    expected_disposition,
                    entry.reason
                ),
            ));
        }
    }
    for entry in expected.iter().filter(|entry| {
        entry.disposition == DenominatorDisposition::Excluded && entry.packet_node.is_none()
    }) {
        if registry.iter().any(|node| node.issues.contains(&entry.issue)) {
            violations.push(Violation::new(
                "excluded_issue_present",
                format!("excluded issue #{} is present in packet registry", entry.issue),
            ));
        }
    }
    violations
}

impl Violation {
    pub(crate) fn new(code: &str, detail: impl Into<String>) -> Self {
        Self { code: code.to_owned(), detail: detail.into() }
    }
}

const ROOT_KEYS: &[&str] = &[
    "schema",
    "packet_id",
    "repository",
    "work",
    "claim_ceiling",
    "planes",
    "authorities",
    "operations",
    "surfaces",
    "artifacts",
    "durable_spec",
    "sequence",
    "proof",
    "delivery",
    "stop",
];

const WORK_KEYS: &[&str] = &[
    "node_id",
    "issues",
    "controller_issue",
    "domain",
    "role",
    "disposition",
    "profile",
    "objective_sentence",
    "registry_scope",
    "registry_digest",
];

const CLAIM_KEYS: &[&str] = &[
    "establishes",
    "cannot_establish",
    "prerequisite_disposition",
    "successors",
    "remaining_not_proven",
    "rollback_meaning",
];

const ARTIFACT_KEYS: &[&str] = &[
    "id",
    "kind",
    "owner",
    "mode",
    "current_disposition",
    "required_change_or_proof",
    "check_command",
    "review_lens",
    "claim_impact",
];

const REVIEWER_ROOT_KEYS: &[&str] = &[
    "schema",
    "review_id",
    "subject",
    "builder_ref",
    "currentness",
    "lenses",
    "stage_falsification_examples",
    "negative_control_audit",
    "old_path_audit",
    "stop",
];

/// Closed vocabulary mirrors of the pinned `$defs` enums in the two schema
/// files. A drift test keeps schema files and these slices identical.
pub const ROLES: &[&str] = &[
    "product_implementation",
    "proof_only",
    "installed_client_proof",
    "research_decision",
    "governance_support",
];
pub const DISPOSITIONS: &[&str] = &["actionable", "deferred", "blocked_external_manual"];
pub const PROFILES: &[&str] = &[
    "core_semantic",
    "optional_framework",
    "installed_client",
    "proof_only",
    "governance",
    "research",
    "deferred",
];
pub const DOMAINS: &[&str] = &[
    "imports",
    "navigation",
    "signature_help",
    "semantic_tokens",
    "critic",
    "formatting",
    "parser_research",
    "vscode_client",
    "distribution",
    "support_registry",
    "dap",
];
pub const AUTHORITY_GROUPS: &[&str] = &[
    "must_already_be_current",
    "must_be_consumed_never_reimplemented",
    "owned_by_this_node",
    "candidate_may_be_mined",
    "consumer_fan_in_after_pr",
    "external_manual_owner",
    "explicitly_not_owned",
];
pub const ARTIFACT_MODES: &[&str] =
    &["create", "update", "consume", "prove_unchanged", "not_applicable"];
pub const DURABLE_SPEC_DISPOSITIONS: &[&str] = &[
    "EXISTING_NORMATIVE_CONTRACT_SUFFICIENT",
    "COMPILE_DURABLE_DELTA_INTO_EXISTING_OWNER",
    "ISSUE_PLAN_SUFFICIENT_FOR_THIS_LEAF",
    "RETURN_TO_ISSUE_FOR_UNSETTLED_DECISION",
    "NOT_PROVEN",
];
pub const SEQUENCE_STEPS: &[&str] = &[
    "verify_packet_and_writer_state",
    "read_named_authorities",
    "materialize_first_falsifier",
    "implement_proposition",
    "execute_proof_protocol",
    "execute_research_protocol",
    "execute_registry_mapping",
    "record_disposition_no_execution",
    "retire_old_paths",
    "update_required_artifacts",
    "run_focused_proof",
    "run_negative_mutations",
    "inspect_diff_against_surfaces",
    "produce_review_forward_handoff",
    "stop_and_transfer_adjacent_findings",
];
pub const CONTROL_CLASSES: &[&str] = &[
    "wrong_subject",
    "stale",
    "false_empty",
    "unsafe_edit",
    "duplicate_authority",
    "near_miss_framework",
    "near_miss_client",
    "near_miss_platform",
    "near_miss_artifact_stage",
    "mutation",
    "fixture_as_semantic_support",
    "unexpected_duplicate_blocker",
];
pub const TERMINAL_OLD_PATH_DISPOSITIONS: &[&str] = &[
    "none",
    "removed",
    "unreachable",
    "forwarding_through_canonical_owner",
    "bounded_compatibility",
    "instrument_or_test_oracle_only",
    "historical_generated_presentation_only",
    "unexpected_duplicate_blocker",
];
pub const FORBIDDEN_ACTIONS: &[&str] = &[
    "model_invocation",
    "scheduler_or_lease_mutation",
    "merge_without_current_substantive_review",
    "release_publication_or_support_state_change",
    "product_repair_from_non_product_role",
    "generic_prompt_framework_creation",
    "spec_planning_tree_creation",
];
pub const LIVE_STATES: &[&str] = &["unknown", "observed"];
pub const LIVE_ACTIONS: &[&str] = &["none", "resume", "repair", "restack", "review"];
pub const REVIEW_LENSES: &[&str] = &[
    "coherent_subject_currentness",
    "semantic_authority",
    "fallback_refusal_legitimate_empty_truth",
    "unsafe_edit_complete_set_reauthorization",
    "old_path_retirement",
    "lifecycle_cleanup",
    "isolation_boundaries",
    "security_trust_boundary",
    "artifact_completeness",
    "stage_separation",
    "public_api_claim_consistency",
];
pub const EXAMPLE_STAGES: &[&str] = &[
    "parser_semantic_workspace_subject",
    "instrument_failure_empty_success",
    "old_path_still_live",
    "cross_root_framework_client_platform",
    "held_edit_across_movement",
    "partial_result_disappearance",
    "fixture_as_semantic_support",
    "external_tool_presence_default",
    "proof_repairs_product",
    "artifact_stage_crosstalk",
    "missing_required_artifact",
];

/// Banned mutable-state key family; durable packets never embed assignment,
/// lease, liveness, or frontier-cursor state.
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

/// Banned generic proof/verification phrasing; a falsifier names its cell.
const GENERIC_STATEMENTS: &[&str] = &[
    "add tests",
    "add more tests",
    "run the workspace",
    "run all tests",
    "run the test suite",
    "make ci green",
];

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn string_array(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn check_closed_keys(
    object: &serde_json::Map<String, Value>,
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
    for key in allowed {
        if !object.contains_key(*key) {
            violations
                .push(Violation::new("missing_field", format!("{where_}: missing field {key}")));
        }
    }
}

fn check_enum(
    object: &serde_json::Map<String, Value>,
    key: &str,
    vocabulary: &[&str],
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    let Some(raw) = object.get(key).and_then(Value::as_str) else {
        return;
    };
    if !vocabulary.contains(&raw) {
        violations.push(Violation::new(
            "vocabulary_violation",
            format!("{where_}: {key}={raw:?} is outside the closed vocabulary"),
        ));
    }
}

fn check_nonempty_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    match object.get(key).and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => {}
        _ => violations.push(Violation::new(
            "empty_field",
            format!("{where_}: {key} must be a non-empty string"),
        )),
    }
}

fn check_nonempty_array(
    object: &serde_json::Map<String, Value>,
    key: &str,
    minimum: usize,
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    let items = string_array(object.get(key));
    if items.len() < minimum || items.iter().any(|item| item.trim().is_empty()) {
        violations.push(Violation::new(
            "empty_field",
            format!("{where_}: {key} needs at least {minimum} non-empty entries"),
        ));
    }
}

fn walk_strings(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::String(text) => visit(text),
        Value::Array(items) => items.iter().for_each(|item| walk_strings(item, visit)),
        Value::Object(object) => object.values().for_each(|item| walk_strings(item, visit)),
        _ => {}
    }
}

fn check_banned_text(root: &Value, violations: &mut Vec<Violation>) {
    walk_strings(root, &mut |text| {
        let lowered = text.to_lowercase();
        for banned in GENERIC_STATEMENTS {
            if lowered.contains(banned) {
                violations.push(Violation::new(
                    "generic_verification",
                    format!("banned generic statement {banned:?}: {text}"),
                ));
            }
        }
    });
}

fn check_no_mutable_state(root: &Value, violations: &mut Vec<Violation>) {
    fn walk(value: &Value, path: &str, violations: &mut Vec<Violation>) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    if MUTABLE_STATE_KEYS.contains(&key.as_str()) {
                        violations.push(Violation::new(
                            "mutable_state_key",
                            format!("durable packet embeds mutable-state key at {path}.{key}"),
                        ));
                    }
                    walk(child, &format!("{path}.{key}"), violations);
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    walk(item, &format!("{path}[{index}]"), violations);
                }
            }
            _ => {}
        }
    }
    walk(root, "", violations);
}

fn validate_live_plane(live: &Value, violations: &mut Vec<Violation>) {
    let Some(object) = live.as_object() else {
        violations.push(Violation::new("shape_violation", "planes.live must be an object"));
        return;
    };
    check_closed_keys(
        object,
        &[
            "state",
            "snapshot_digest",
            "head_sha",
            "candidate_branch",
            "writer_active",
            "required_action",
            "preflight_required",
        ],
        "planes.live",
        violations,
    );
    check_enum(object, "state", LIVE_STATES, "planes.live", violations);
    check_enum(object, "required_action", LIVE_ACTIONS, "planes.live", violations);
    let state = object.get("state").and_then(Value::as_str);
    match state {
        Some("unknown") => {
            for key in ["snapshot_digest", "head_sha", "candidate_branch", "writer_active"] {
                if !object.get(key).map(Value::is_null).unwrap_or(false) {
                    violations.push(Violation::new(
                        "offline_live_overspecification",
                        format!("offline packet claims live knowledge in planes.live.{key}; candidate and writer state are unknown"),
                    ));
                }
            }
            if object.get("preflight_required") != Some(&Value::Bool(true)) {
                violations.push(Violation::new(
                    "offline_preflight_missing",
                    "an offline packet requires a read-only preflight before any write action",
                ));
            }
            if object.get("required_action").and_then(Value::as_str) != Some("none") {
                violations.push(Violation::new(
                    "offline_live_overspecification",
                    "an offline packet cannot demand a live action it did not observe",
                ));
            }
        }
        Some("observed") => {
            let head_ok = object
                .get("head_sha")
                .and_then(Value::as_str)
                .map(|sha| {
                    (40..=64).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit())
                })
                .unwrap_or(false);
            if !head_ok {
                violations.push(Violation::new(
                    "observed_live_underdefined",
                    "an observed packet binds a 40-64 hex head",
                ));
            }
            let digest_ok = object
                .get("snapshot_digest")
                .and_then(Value::as_str)
                .map(|digest| is_hex(digest, 64))
                .unwrap_or(false);
            if !digest_ok {
                violations.push(Violation::new(
                    "observed_live_underdefined",
                    "an observed packet binds its snapshot digest",
                ));
            }
            if !object.get("writer_active").map(Value::is_boolean).unwrap_or(false) {
                violations.push(Violation::new(
                    "observed_live_underdefined",
                    "an observed packet reports writer_active",
                ));
            }
            if object.get("preflight_required") != Some(&Value::Bool(false)) {
                violations.push(Violation::new(
                    "observed_preflight_misdeclared",
                    "a complete live observation marks the preflight satisfied",
                ));
            }
        }
        _ => {}
    }
}

fn validate_plane_honesty(
    planes: &serde_json::Map<String, Value>,
    violations: &mut Vec<Violation>,
) {
    for (key, owner) in [("current_tree", 11280), ("offline_readiness", 11281)] {
        let Some(plane) = planes.get(key).and_then(Value::as_object) else {
            violations
                .push(Violation::new("missing_field", format!("planes.{key} must be declared")));
            continue;
        };
        if plane.get("status").and_then(Value::as_str) != Some("unavailable") {
            violations.push(Violation::new(
                "plane_not_wired",
                format!("planes.{key} must declare unavailable until its owning train lands"),
            ));
        }
        if plane.get("owner_issue") != Some(&Value::from(owner)) {
            violations.push(Violation::new(
                "plane_owner_missing",
                format!("planes.{key} must name owning issue {owner}"),
            ));
        }
    }
    let stable = planes.get("stable").and_then(Value::as_object);
    let digest_ok = stable
        .and_then(|stable| stable.get("digest"))
        .and_then(Value::as_str)
        .map(|digest| is_hex(digest, 64))
        .unwrap_or(false);
    if !digest_ok {
        violations.push(Violation::new(
            "plane_digest_missing",
            "planes.stable.digest must be 64 hex characters",
        ));
    }
}

fn validate_work(node: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    check_closed_keys(node, WORK_KEYS, "work", violations);
    check_enum(node, "domain", DOMAINS, "work", violations);
    check_enum(node, "role", ROLES, "work", violations);
    check_enum(node, "disposition", DISPOSITIONS, "work", violations);
    check_enum(node, "profile", PROFILES, "work", violations);
    check_nonempty_string(node, "objective_sentence", "work", violations);
    if node.get("registry_scope").and_then(Value::as_str) != Some("representative_packet_fixtures")
    {
        violations.push(Violation::new(
            "scope_overreach",
            "work.registry_scope must stay representative_packet_fixtures; the full DAG belongs to #11279",
        ));
    }
    let digest_ok = node
        .get("registry_digest")
        .and_then(Value::as_str)
        .map(|digest| is_hex(digest, 64))
        .unwrap_or(false);
    if !digest_ok {
        violations.push(Violation::new(
            "identity_digest_invalid",
            "work.registry_digest must be 64 hex characters",
        ));
    }
    let issues = node
        .get("issues")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).collect::<Vec<_>>());
    match issues {
        Some(issues) if !issues.is_empty() => {}
        _ => violations
            .push(Violation::new("empty_field", "work.issues needs at least one issue number")),
    }
}

fn validate_claim_ceiling(claim: Option<&Value>, role: &str, violations: &mut Vec<Violation>) {
    let Some(claim) = claim.and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "claim_ceiling must be an object"));
        return;
    };
    check_closed_keys(claim, CLAIM_KEYS, "claim_ceiling", violations);
    check_nonempty_array(claim, "establishes", 1, "claim_ceiling", violations);
    check_nonempty_array(claim, "cannot_establish", 1, "claim_ceiling", violations);
    check_nonempty_string(claim, "prerequisite_disposition", "claim_ceiling", violations);
    check_nonempty_string(claim, "rollback_meaning", "claim_ceiling", violations);
    check_nonempty_array(claim, "successors", 0, "claim_ceiling", violations);
    check_nonempty_array(claim, "remaining_not_proven", 0, "claim_ceiling", violations);
    // A product role must not promise what only successor planes establish.
    if role == "product_implementation"
        && string_array(claim.get("establishes")).iter().any(|row| row.contains("installed"))
    {
        violations.push(Violation::new(
            "ceiling_overreach",
            "a product implementation cannot claim installed-client evidence in its ceiling",
        ));
    }
}

fn validate_authorities(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(items) = object.get("authorities").and_then(Value::as_array) else {
        violations.push(Violation::new("shape_violation", "authorities must be an array"));
        return;
    };
    if items.is_empty() {
        violations.push(Violation::new("empty_field", "authorities needs at least one entry"));
    }
    for (index, entry) in items.iter().enumerate() {
        let Some(entry) = entry.as_object() else {
            violations.push(Violation::new(
                "shape_violation",
                format!("authorities[{index}] must be an object"),
            ));
            continue;
        };
        check_closed_keys(
            entry,
            &["ref", "subject", "group"],
            &format!("authorities[{index}]"),
            violations,
        );
        check_enum(entry, "group", AUTHORITY_GROUPS, &format!("authorities[{index}]"), violations);
        check_nonempty_string(entry, "ref", &format!("authorities[{index}]"), violations);
        check_nonempty_string(entry, "subject", &format!("authorities[{index}]"), violations);
    }
}

fn validate_operations(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(items) = object.get("operations").and_then(Value::as_array) else {
        violations.push(Violation::new("shape_violation", "operations must be an array"));
        return;
    };
    if items.is_empty() {
        violations.push(Violation::new("empty_field", "operations needs at least one typed row"));
    }
    for (index, row) in items.iter().enumerate() {
        let Some(row) = row.as_object() else {
            violations.push(Violation::new(
                "shape_violation",
                format!("operations[{index}] must be an object"),
            ));
            continue;
        };
        check_closed_keys(
            row,
            &[
                "feature",
                "provider_or_client",
                "source_subject",
                "policy",
                "canonical_owner",
                "old_path_disposition",
                "proof_owner",
            ],
            &format!("operations[{index}]"),
            violations,
        );
        for key in ["feature", "provider_or_client", "source_subject", "canonical_owner"] {
            check_nonempty_string(row, key, &format!("operations[{index}]"), violations);
        }
        let policy = row.get("policy").and_then(Value::as_object);
        let Some(policy) = policy else {
            violations.push(Violation::new(
                "shape_violation",
                format!("operations[{index}].policy must be an object"),
            ));
            continue;
        };
        check_closed_keys(
            policy,
            &["semantic", "currentness", "fallback", "refusal", "legitimate_empty"],
            &format!("operations[{index}].policy"),
            violations,
        );
        for key in ["semantic", "currentness", "fallback", "refusal", "legitimate_empty"] {
            check_nonempty_string(policy, key, &format!("operations[{index}].policy"), violations);
        }
    }
}

fn validate_surfaces(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(surfaces) = object.get("surfaces").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "surfaces must be an object"));
        return;
    };
    check_closed_keys(surfaces, &["allowed", "forbidden"], "surfaces", violations);
    check_nonempty_array(surfaces, "allowed", 1, "surfaces", violations);
    check_nonempty_array(surfaces, "forbidden", 3, "surfaces", violations);
}

fn validate_artifacts(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(items) = object.get("artifacts").and_then(Value::as_array) else {
        violations.push(Violation::new("shape_violation", "artifacts must be an array"));
        return;
    };
    if items.is_empty() {
        violations
            .push(Violation::new("empty_field", "the artifact worklist needs at least one row"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for (index, row) in items.iter().enumerate() {
        let Some(row) = row.as_object() else {
            violations.push(Violation::new(
                "shape_violation",
                format!("artifacts[{index}] must be an object"),
            ));
            continue;
        };
        check_closed_keys(row, ARTIFACT_KEYS, &format!("artifacts[{index}]"), violations);
        check_enum(row, "mode", ARTIFACT_MODES, &format!("artifacts[{index}]"), violations);
        for key in ARTIFACT_KEYS {
            check_nonempty_string(row, key, &format!("artifacts[{index}]"), violations);
        }
        if let Some(id) = row.get("id").and_then(Value::as_str)
            && !seen.insert(id.to_owned())
        {
            violations.push(Violation::new(
                "duplicate_artifact",
                format!("artifact id {id:?} appears twice"),
            ));
        }
    }
}

fn validate_durable_spec(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(spec) = object.get("durable_spec").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "durable_spec must be an object"));
        return;
    };
    check_closed_keys(spec, &["disposition", "owner", "note"], "durable_spec", violations);
    check_enum(spec, "disposition", DURABLE_SPEC_DISPOSITIONS, "durable_spec", violations);
    check_nonempty_string(spec, "owner", "durable_spec", violations);
    check_nonempty_string(spec, "note", "durable_spec", violations);
    let owner = spec.get("owner").and_then(Value::as_str).unwrap_or_default();
    if spec.get("disposition").and_then(Value::as_str)
        == Some("COMPILE_DURABLE_DELTA_INTO_EXISTING_OWNER")
        && !owner.starts_with('#')
        && !owner.contains("contract surfaced by")
    {
        violations.push(Violation::new(
            "unowned_spec_target",
            "compiling a durable delta requires naming the existing normative owner",
        ));
    }
}

fn validate_sequence(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let steps = string_array(object.get("sequence"));
    if steps.is_empty() {
        violations.push(Violation::new("shape_violation", "sequence must be a non-empty array"));
        return;
    }
    for step in &steps {
        if !SEQUENCE_STEPS.contains(step) {
            violations.push(Violation::new(
                "vocabulary_violation",
                format!("sequence step {step:?} is outside the closed vocabulary"),
            ));
        }
    }
    let role =
        object.get("work").and_then(|work| work.get("role")).and_then(Value::as_str).unwrap_or("");
    let expected = match role {
        "product_implementation" => "implement_proposition",
        "proof_only" | "installed_client_proof" => "execute_proof_protocol",
        "research_decision" => "execute_research_protocol",
        "governance_support" => "execute_registry_mapping",
        _ => "",
    };
    if !expected.is_empty() && !steps.contains(&expected) {
        violations.push(Violation::new(
            "wrong_role_sequence",
            format!("role {role:?} requires sequence step {expected:?}"),
        ));
    }
    for step in [
        "implement_proposition",
        "execute_proof_protocol",
        "execute_research_protocol",
        "execute_registry_mapping",
    ] {
        if step != expected && steps.contains(&step) {
            violations.push(Violation::new(
                "wrong_role_sequence",
                format!("role {role:?} must not encode foreign core step {step:?}"),
            ));
        }
    }
    for step in [
        "implement_proposition",
        "execute_proof_protocol",
        "execute_research_protocol",
        "execute_registry_mapping",
        "record_disposition_no_execution",
    ] {
        if steps.iter().filter(|candidate| **candidate == step).count() > 1 {
            violations.push(Violation::new(
                "wrong_role_sequence",
                format!("sequence contains duplicate role-specific step {step:?}"),
            ));
        }
    }
    if role == "governance_support" && !steps.contains(&"record_disposition_no_execution") {
        violations.push(Violation::new(
            "wrong_role_sequence",
            "governance packets require record_disposition_no_execution",
        ));
    }
    if role != "governance_support" && steps.contains(&"record_disposition_no_execution") {
        violations.push(Violation::new(
            "wrong_role_sequence",
            "only governance packets may record disposition without execution",
        ));
    }
    for required in [
        "verify_packet_and_writer_state",
        "materialize_first_falsifier",
        "stop_and_transfer_adjacent_findings",
    ] {
        if !steps.contains(&required) {
            violations.push(Violation::new(
                "incomplete_sequence",
                format!("sequence is missing the required step {required:?}"),
            ));
        }
    }
}

fn validate_proof(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(proof) = object.get("proof").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "proof must be an object"));
        return;
    };
    check_closed_keys(
        proof,
        &[
            "first_falsifier",
            "positive_discriminator",
            "controls",
            "commands",
            "instrument_failure_behavior",
        ],
        "proof",
        violations,
    );
    let Some(falsifier) = proof.get("first_falsifier").and_then(Value::as_object) else {
        violations.push(Violation::new("missing_falsifier", "proof.first_falsifier is required"));
        return;
    };
    check_closed_keys(
        falsifier,
        &["description", "expected_red_reason", "canonical_owner"],
        "proof.first_falsifier",
        violations,
    );
    for key in ["description", "expected_red_reason", "canonical_owner"] {
        check_nonempty_string(falsifier, key, "proof.first_falsifier", violations);
    }
    check_nonempty_string(proof, "positive_discriminator", "proof", violations);
    check_nonempty_string(proof, "instrument_failure_behavior", "proof", violations);
    let controls = string_array_of_objects(proof.get("controls"), "controls", violations);
    for (index, control) in controls {
        check_enum(
            control,
            "class",
            CONTROL_CLASSES,
            &format!("proof.controls[{index}]"),
            violations,
        );
        check_nonempty_string(control, "subject", &format!("proof.controls[{index}]"), violations);
    }
    let commands = string_array_of_objects(proof.get("commands"), "commands", violations);
    if commands.is_empty() {
        violations.push(Violation::new(
            "empty_field",
            "proof.commands needs at least one routed command",
        ));
    }
    for (index, command) in &commands {
        check_closed_keys(
            command,
            &["id", "command", "scope"],
            &format!("proof.commands[{index}]"),
            violations,
        );
        check_nonempty_string(command, "id", &format!("proof.commands[{index}]"), violations);
        check_nonempty_string(command, "command", &format!("proof.commands[{index}]"), violations);
        check_nonempty_string(command, "scope", &format!("proof.commands[{index}]"), violations);
    }
}

fn string_array_of_objects<'a>(
    value: Option<&'a Value>,
    label: &str,
    violations: &mut Vec<Violation>,
) -> Vec<(usize, &'a serde_json::Map<String, Value>)> {
    let Some(items) = value.and_then(Value::as_array) else {
        violations.push(Violation::new("shape_violation", format!("{label} must be an array")));
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item.as_object() {
            Some(object) => out.push((index, object)),
            None => violations.push(Violation::new(
                "shape_violation",
                format!("{label}[{index}] must be an object"),
            )),
        }
    }
    out
}

fn validate_delivery(object: &serde_json::Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(delivery) = object.get("delivery").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "delivery must be an object"));
        return;
    };
    check_closed_keys(
        delivery,
        &[
            "branch_suggestion",
            "pr_title_suggestion",
            "base_head",
            "issues",
            "changed_surfaces",
            "old_path_dispositions",
            "limitations",
            "review_map",
            "stop_before",
        ],
        "delivery",
        violations,
    );
    for key in ["branch_suggestion", "pr_title_suggestion", "base_head"] {
        check_nonempty_string(delivery, key, "delivery", violations);
    }
    for key in ["changed_surfaces", "limitations", "review_map", "stop_before"] {
        if !delivery.get(key).is_some_and(Value::is_array) {
            violations.push(Violation::new(
                "shape_violation",
                format!("delivery.{key} must be an array"),
            ));
        } else if let Some(items) = delivery.get(key).and_then(Value::as_array) {
            for (index, item) in items.iter().enumerate() {
                if item.as_str().is_none_or(|value| value.trim().is_empty()) {
                    violations.push(Violation::new(
                        "shape_violation",
                        format!("delivery.{key}[{index}] must be a non-empty string"),
                    ));
                }
            }
        }
    }
    let Some(issues) = delivery.get("issues").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "delivery.issues must be an object"));
        return;
    };
    check_closed_keys(
        issues,
        &["controller", "dependencies", "unblocks"],
        "delivery.issues",
        violations,
    );
    if !issues.get("controller").is_some_and(Value::is_u64) {
        violations.push(Violation::new(
            "shape_violation",
            "delivery.issues.controller must be an integer",
        ));
    }
    for key in ["dependencies", "unblocks"] {
        if !issues.get(key).is_some_and(Value::is_array) {
            violations.push(Violation::new(
                "shape_violation",
                format!("delivery.issues.{key} must be an array"),
            ));
        } else if let Some(items) = issues.get(key).and_then(Value::as_array) {
            for (index, item) in items.iter().enumerate() {
                if !item.is_u64() {
                    violations.push(Violation::new(
                        "shape_violation",
                        format!("delivery.issues.{key}[{index}] must be an integer"),
                    ));
                }
            }
        }
    }
    let declared = object
        .get("claim_ceiling")
        .and_then(|claim| claim.get("prerequisite_disposition"))
        .and_then(Value::as_str)
        .map(|text| {
            text.replace("fr_", "#")
                .split('#')
                .skip(1)
                .filter_map(|part| {
                    part.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse::<u32>()
                        .ok()
                })
                .fold(Vec::new(), |mut issues, issue| {
                    if !issues.contains(&issue) {
                        issues.push(issue);
                    }
                    issues
                })
        })
        .unwrap_or_default();
    let emitted = issues.get("dependencies").and_then(Value::as_array).cloned().unwrap_or_default();
    let expected: Vec<Value> = declared.into_iter().map(Value::from).collect();
    if emitted != expected {
        violations.push(Violation::new(
            "dependency_mismatch",
            "delivery.issues.dependencies must equal issue references declared by claim_ceiling.prerequisite_disposition",
        ));
    }
    check_nonempty_array(delivery, "review_map", 1, "delivery", violations);
    check_nonempty_array(delivery, "stop_before", 1, "delivery", violations);
    let dispositions = delivery.get("old_path_dispositions").and_then(Value::as_array);
    match dispositions {
        Some(items) if !items.is_empty() => {
            for (index, item) in items.iter().enumerate() {
                let row = item.as_object();
                if let Some(row) = row {
                    check_closed_keys(
                        row,
                        &["seam", "terminal_disposition"],
                        &format!("delivery.old_path_dispositions[{index}]"),
                        violations,
                    );
                    check_nonempty_string(
                        row,
                        "seam",
                        &format!("delivery.old_path_dispositions[{index}]"),
                        violations,
                    );
                }
                let disposition =
                    row.and_then(|row| row.get("terminal_disposition")).and_then(Value::as_str);
                match disposition {
                    Some(value) if TERMINAL_OLD_PATH_DISPOSITIONS.contains(&value) => {}
                    _ => violations.push(Violation::new(
                        "old_path_unterminated",
                        format!(
                            "delivery.old_path_dispositions[{index}] lacks a terminal disposition"
                        ),
                    )),
                }
            }
        }
        _ => violations.push(Violation::new(
            "old_path_unterminated",
            "delivery.old_path_dispositions must record every replaced seam terminally",
        )),
    }
}

fn validate_delivery_authority(
    object: &serde_json::Map<String, Value>,
    violations: &mut Vec<Violation>,
) {
    let Some(work) = object.get("work").and_then(Value::as_object) else { return };
    let Some(node_id) = work.get("node_id").and_then(Value::as_str) else { return };
    let Some(node) = nodes::all_nodes().into_iter().find(|node| node.node_id == node_id) else {
        return;
    };
    let Some(delivery) = object.get("delivery").and_then(Value::as_object) else { return };
    let Some(issues) = delivery.get("issues").and_then(Value::as_object) else { return };
    let expected = build::delivery_issue_ids(&node, &nodes::all_nodes());
    let actual_controller = issues.get("controller").and_then(Value::as_u64);
    if actual_controller != Some(u64::from(expected.0)) {
        violations.push(Violation::new(
            "controller_mismatch",
            "delivery.issues.controller must equal the registry controller issue",
        ));
    }
    for (key, expected) in [("dependencies", expected.1), ("unblocks", expected.2)] {
        let actual: Vec<u32> = issues
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .filter_map(|value| u32::try_from(value).ok())
            .collect();
        if actual != expected {
            violations.push(Violation::new(
                "routing_mismatch",
                format!(
                    "delivery.issues.{key} must equal the registry-derived authoritative routing"
                ),
            ));
        }
    }
}

fn validate_stop(
    object: &serde_json::Map<String, Value>,
    role: &str,
    violations: &mut Vec<Violation>,
) {
    let Some(stop) = object.get("stop").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "stop must be an object"));
        return;
    };
    check_closed_keys(stop, &["conditions", "forbidden_actions", "handoff"], "stop", violations);
    check_nonempty_array(stop, "conditions", 1, "stop", violations);
    check_nonempty_string(stop, "handoff", "stop", violations);
    let actions = string_array(stop.get("forbidden_actions"));
    for action in &actions {
        if !FORBIDDEN_ACTIONS.contains(action) {
            violations.push(Violation::new(
                "vocabulary_violation",
                format!("forbidden action {action:?} is outside the closed vocabulary"),
            ));
        }
    }
    for required in
        ["merge_without_current_substantive_review", "release_publication_or_support_state_change"]
    {
        if !actions.contains(&required) {
            violations.push(Violation::new(
                "missing_forbidden_action",
                format!("stop.forbidden_actions must include {required:?}"),
            ));
        }
    }
    if role != "product_implementation"
        && !actions.contains(&"product_repair_from_non_product_role")
    {
        violations.push(Violation::new(
            "missing_forbidden_action",
            "non-product roles must forbid product repair explicitly",
        ));
    }
}

/// Validate one builder document completely.
pub fn validate_builder(doc: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Some(object) = doc.as_object() else {
        return vec![Violation::new("shape_violation", "builder packet root must be an object")];
    };
    check_closed_keys(object, ROOT_KEYS, "document", &mut violations);
    if object.get("schema").and_then(Value::as_str) != Some(super::build::BUILDER_SCHEMA) {
        violations.push(Violation::new(
            "schema_mismatch",
            "document.schema must be the builder contract id",
        ));
    }
    let packet_id_ok = object
        .get("packet_id")
        .and_then(Value::as_str)
        .map(|id| id.starts_with("frbld_") && is_hex(id.trim_start_matches("frbld_"), 16))
        .unwrap_or(false);
    if !packet_id_ok {
        violations.push(Violation::new("packet_id_invalid", "packet_id must be frbld_<16 hex>"));
    }
    if packet_id_ok && !build::id_matches_content(doc) {
        violations.push(Violation::new(
            "content_address_mismatch",
            "packet_id does not match the SHA-256 of the canonical content; regenerate the packet",
        ));
    }
    let role =
        object.get("work").and_then(|work| work.get("role")).and_then(Value::as_str).unwrap_or("");
    if let Some(work) = object.get("work").and_then(Value::as_object) {
        validate_work(work, &mut violations);
    } else {
        violations.push(Violation::new("shape_violation", "work must be an object"));
    }
    validate_claim_ceiling(object.get("claim_ceiling"), role, &mut violations);
    match object.get("planes").and_then(Value::as_object) {
        Some(planes) => {
            validate_plane_honesty(planes, &mut violations);
            if let Some(live) = planes.get("live") {
                validate_live_plane(live, &mut violations);
            } else {
                violations.push(Violation::new("missing_field", "planes.live must be declared"));
            }
        }
        None => violations.push(Violation::new("shape_violation", "planes must be an object")),
    }
    validate_authorities(object, &mut violations);
    validate_operations(object, &mut violations);
    validate_surfaces(object, &mut violations);
    validate_artifacts(object, &mut violations);
    validate_durable_spec(object, &mut violations);
    validate_sequence(object, &mut violations);
    validate_proof(object, &mut violations);
    validate_delivery(object, &mut violations);
    validate_delivery_authority(object, &mut violations);
    validate_stop(object, role, &mut violations);
    check_banned_text(doc, &mut violations);
    check_no_mutable_state(doc, &mut violations);
    violations
}

/// Validate one reviewer document completely (without builder cross-checks).
pub fn validate_reviewer(doc: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Some(object) = doc.as_object() else {
        return vec![Violation::new("shape_violation", "reviewer packet root must be an object")];
    };
    check_closed_keys(object, REVIEWER_ROOT_KEYS, "document", &mut violations);
    if object.get("schema").and_then(Value::as_str) != Some(super::build::REVIEWER_SCHEMA) {
        violations.push(Violation::new(
            "schema_mismatch",
            "document.schema must be the reviewer contract id",
        ));
    }
    let review_id_ok = object
        .get("review_id")
        .and_then(Value::as_str)
        .map(|id| id.starts_with("frrvw_") && is_hex(id.trim_start_matches("frrvw_"), 16))
        .unwrap_or(false);
    if !review_id_ok {
        violations.push(Violation::new("packet_id_invalid", "review_id must be frrvw_<16 hex>"));
    }
    if review_id_ok && !build::id_matches_content(doc) {
        violations.push(Violation::new(
            "content_address_mismatch",
            "review_id does not match the SHA-256 of the canonical content; regenerate the packet",
        ));
    }
    validate_reviewer_body(object, &mut violations);
    check_banned_text(doc, &mut violations);
    check_no_mutable_state(doc, &mut violations);
    violations
}

fn validate_reviewer_body(
    object: &serde_json::Map<String, Value>,
    violations: &mut Vec<Violation>,
) {
    let Some(subject) = object.get("subject").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "subject must be an object"));
        return;
    };
    check_closed_keys(
        subject,
        &["node_id", "issues", "role", "profile", "claim_ceiling_sentence"],
        "subject",
        violations,
    );
    check_enum(subject, "role", ROLES, "subject", violations);
    check_nonempty_string(subject, "node_id", "subject", violations);
    check_nonempty_string(subject, "claim_ceiling_sentence", "subject", violations);
    let Some(builder_ref) = object.get("builder_ref").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "builder_ref must be an object"));
        return;
    };
    check_closed_keys(builder_ref, &["packet_id", "digest"], "builder_ref", violations);
    check_nonempty_string(builder_ref, "packet_id", "builder_ref", violations);
    let currentness = object.get("currentness").and_then(Value::as_object);
    let Some(currentness) = currentness else {
        violations.push(Violation::new("shape_violation", "currentness must be an object"));
        return;
    };
    check_closed_keys(
        currentness,
        &["base_head", "live_state", "invalidators", "stale_rule"],
        "currentness",
        violations,
    );
    check_enum(currentness, "live_state", LIVE_STATES, "currentness", violations);
    check_nonempty_array(currentness, "invalidators", 1, "currentness", violations);
    check_nonempty_string(currentness, "stale_rule", "currentness", violations);
    let lenses = string_array_of_objects(object.get("lenses"), "lenses", violations);
    let mut seen_names = std::collections::BTreeSet::new();
    for (index, lens) in &lenses {
        check_closed_keys(
            lens,
            &["name", "applicable", "reason", "questions"],
            &format!("lenses[{index}]"),
            violations,
        );
        let name = lens.get("name").and_then(Value::as_str).unwrap_or_default();
        if !REVIEW_LENSES.contains(&name) {
            violations.push(Violation::new(
                "vocabulary_violation",
                format!("lenses[{index}] name {name:?} is outside the closed lens vocabulary"),
            ));
        }
        if !seen_names.insert(name.to_owned()) {
            violations.push(Violation::new(
                "lens_duplicate",
                format!("lens {name:?} appears more than once"),
            ));
        }
        let applicable = lens.get("applicable").and_then(Value::as_bool).unwrap_or(false);
        let questions = string_array(lens.get("questions"));
        if applicable && questions.is_empty() {
            violations.push(Violation::new(
                "lens_without_questions",
                format!("applicable lens {name:?} must carry typed questions"),
            ));
        }
        check_nonempty_string(lens, "reason", &format!("lenses[{index}]"), violations);
    }
    for lens in REVIEW_LENSES {
        if !seen_names.contains(*lens) {
            violations.push(Violation::new(
                "lens_missing",
                format!("required lens {lens:?} disappeared; non-applicable lenses retain reasons"),
            ));
        }
    }
    let examples = string_array_of_objects(
        object.get("stage_falsification_examples"),
        "stage_falsification_examples",
        violations,
    );
    if examples.is_empty() {
        violations.push(Violation::new(
            "weak_review",
            "a reviewer repeating the builder summary without stage-specific falsifiers is invalid",
        ));
    }
    for (index, example) in &examples {
        check_enum(
            example,
            "stage",
            EXAMPLE_STAGES,
            &format!("stage_falsification_examples[{index}]"),
            violations,
        );
        check_nonempty_string(
            example,
            "question",
            &format!("stage_falsification_examples[{index}]"),
            violations,
        );
    }
    let audit = string_array_of_objects(
        object.get("negative_control_audit"),
        "negative_control_audit",
        violations,
    );
    if audit.is_empty() {
        violations
            .push(Violation::new("empty_field", "negative_control_audit needs at least one row"));
    }
    for (index, row) in &audit {
        check_closed_keys(
            row,
            &["subject", "requirement"],
            &format!("negative_control_audit[{index}]"),
            violations,
        );
        check_nonempty_string(
            row,
            "subject",
            &format!("negative_control_audit[{index}]"),
            violations,
        );
        check_nonempty_string(
            row,
            "requirement",
            &format!("negative_control_audit[{index}]"),
            violations,
        );
    }
    let old_paths =
        string_array_of_objects(object.get("old_path_audit"), "old_path_audit", violations);
    for (index, row) in &old_paths {
        check_enum(
            row,
            "terminal_disposition",
            TERMINAL_OLD_PATH_DISPOSITIONS,
            &format!("old_path_audit[{index}]"),
            violations,
        );
    }
    let Some(stop) = object.get("stop").and_then(Value::as_object) else {
        violations.push(Violation::new("shape_violation", "stop must be an object"));
        return;
    };
    check_closed_keys(stop, &["reviewer_must_not"], "stop", violations);
    check_nonempty_array(stop, "reviewer_must_not", 1, "stop", violations);
}

/// Cross-document currentness: the reviewer challenges exactly the emitted
/// builder packet at the same base/head/live state, or the pair is stale.
pub fn validate_pair(builder: &Value, reviewer: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let builder_node = builder.pointer("/work/node_id").and_then(Value::as_str).unwrap_or_default();
    let reviewer_node =
        reviewer.pointer("/subject/node_id").and_then(Value::as_str).unwrap_or_default();
    if builder_node != reviewer_node {
        violations.push(Violation::new(
            "subject_mismatch",
            format!(
                "reviewer subject {reviewer_node:?} does not match builder node {builder_node:?}"
            ),
        ));
    }
    for (path, builder_value, reviewer_value) in [
        ("issues", builder.pointer("/work/issues"), reviewer.pointer("/subject/issues")),
        ("role", builder.pointer("/work/role"), reviewer.pointer("/subject/role")),
        ("profile", builder.pointer("/work/profile"), reviewer.pointer("/subject/profile")),
        (
            "claim ceiling",
            builder.pointer("/work/objective_sentence"),
            reviewer.pointer("/subject/claim_ceiling_sentence"),
        ),
    ] {
        if builder_value != reviewer_value {
            violations.push(Violation::new(
                "subject_mismatch",
                format!("reviewer {path} does not match the builder subject"),
            ));
        }
    }
    let expected_id = builder.get("packet_id").cloned().unwrap_or(Value::Null);
    let expected_digest = build::content_digest(builder);
    let actual_id = reviewer.pointer("/builder_ref/packet_id").cloned().unwrap_or(Value::Null);
    let actual_digest = reviewer
        .pointer("/builder_ref/digest")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if actual_id != expected_id || actual_digest != expected_digest {
        violations.push(Violation::new(
            "stale_builder_ref",
            "reviewer references another builder packet than the one generated here",
        ));
    }
    let builder_base = builder.pointer("/delivery/base_head").cloned().unwrap_or(Value::Null);
    let reviewer_base = reviewer.pointer("/currentness/base_head").cloned().unwrap_or(Value::Null);
    let equivalent_main =
        |value: &Value| value.as_str().and_then(|text| text.strip_prefix("main@")).is_some();
    if builder_base != reviewer_base
        && !(equivalent_main(&builder_base) && equivalent_main(&reviewer_base))
    {
        violations.push(Violation::new(
            "stale_head",
            "reviewer base/head differs from the builder packet; the review is stale for affected dimensions",
        ));
    }
    let builder_live = builder.pointer("/planes/live/state").cloned().unwrap_or(Value::Null);
    let reviewer_live = reviewer.pointer("/currentness/live_state").cloned().unwrap_or(Value::Null);
    if builder_live != reviewer_live {
        violations.push(Violation::new(
            "live_state_mismatch",
            "reviewer live state differs from the builder packet",
        ));
    }
    violations
}
