//! Validate the shared typed train edge and claim-profile contract
//! (`train_edge_contract.v1`, issue #10858).
//!
//! This module owns the deterministic validation of the closed shared
//! vocabulary: edge kinds, external checkpoint stages, base reason classes,
//! claim-profile invariants, canonical semantics, and the declared
//! adaptations binding landed programme train manifests to the shared kinds.
//!
//! It deliberately owns no universal train manifest, node vocabulary,
//! frontier, live observer, support evaluator, scheduler, work database, or
//! release authority. Programme manifests embed or adapt the contract and may
//! add stricter local fields without changing the shared meaning. No mutable
//! GitHub, check, run, or receipt state may be embedded in a document.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use xtask::schema_apply::validate_payload_against_schema;

const SCHEMA_PATH: &str = "schemas/train_edge_contract.v1.schema.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/train_edge_contract.v1.schema.json";
const SCHEMA_NAME: &str = "train_edge_contract.v1";

const FIXTURE_DIR: &str = "fixtures/train_edge_contract";
const ADAPTATIONS_PATH: &str = ".spec/10858-train-edge-contract/adaptations.json";

/// Closed shared edge vocabulary. Adding a kind is a contract revision.
const EDGE_KINDS: &[&str] = &[
    "requires_implementation",
    "requires_behavior_for_claim",
    "conditional_release_gate",
    "consumes_if_available",
    "fan_in",
    "external_checkpoint",
    "nonblocking_sidecar",
];

/// Closed common external stages. An internal packet or merged local PR
/// cannot satisfy any of them.
const EXTERNAL_STAGES: &[&str] = &[
    "manual_authorization",
    "external_submission",
    "external_acceptance",
    "released_public_availability",
];

const STAGE_LADDER: &[&str] = &[
    "none",
    "manual_authorization",
    "external_submission",
    "external_acceptance",
    "released_public_availability",
];

/// Closed base reason traits for non-success states. Exact domain
/// vocabularies remain programme-local.
const REASON_CLASSES: &[&str] =
    &["not_proven", "pending_external_stage", "explicit_limitation", "out_of_scope", "superseded"];

/// Closed provenance of a terminal proposition state. Only
/// `verified_proposition` may carry terminal true; closed issues, merged pull
/// requests, workflow runs, and file state never manufacture child success.
const DERIVES_FROM: &[&str] =
    &["verified_proposition", "closed_issue", "merged_pull_request", "workflow_run", "file_state"];

const FAN_IN_SATISFACTION_SOURCE: &str = "independently_terminal_child_propositions";
const EXTERNAL_SATISFACTION_SOURCES: &[&str] = &["not_satisfied", "external_observation"];

/// Closed forbidden mutable-state key family. The durable relationship
/// contract never embeds mutable GitHub/check/receipt state.
const MUTABLE_STATE_KEYS: &[&str] = &[
    "github_url",
    "github_state",
    "check_run",
    "ci_status",
    "receipt_id",
    "merge_commit",
    "review_state",
    "live_status",
];

const EDGE_KEYS: &[&str] = &[
    "source",
    "kind",
    "target",
    "claim_profile",
    "selecting_authority",
    "selected_value",
    "selection_subject",
    "active_predecessor",
    "invalidation_rule",
    "children",
    "satisfaction_source",
    "stage",
    "provenance",
];

const TRACK_KEYS: &[&str] = &["terminal", "reason_class", "derives_from"];

/// Closed root field set of a contract document.
const ROOT_KEYS: &[&str] = &[
    "schema",
    "schema_version",
    "contract_id",
    "programme",
    "authorities",
    "external_subjects",
    "propositions",
    "edges",
    "claim_profiles",
    "projection",
];

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

/// Profile eligibility evaluated from a deterministic projection snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Eligibility {
    profile: String,
    eligible: bool,
    requirements: BTreeSet<String>,
    reasons: BTreeMap<String, String>,
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

fn check_unknown_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            let code = if MUTABLE_STATE_KEYS.contains(&key.as_str()) {
                "mutable_state_embedded"
            } else {
                "unknown_field"
            };
            violations.push(Violation::new(code, format!("{where_}: unexpected field {key}")));
        }
    }
}

/// Validate one contract document. Returns every violation, deterministically
/// ordered. An empty result means the document satisfies the shared closed
/// contract.
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
    if string_field(root, "contract_id").is_none() {
        violations.push(Violation::new(
            "missing_contract_id",
            "contract_id must be a non-empty string".to_string(),
        ));
    }
    // Root-level unknown fields fail closed too: the durable contract never
    // embeds mutable GitHub/check/receipt state at any depth (#10858 P2
    // review finding).
    check_unknown_keys(root, ROOT_KEYS, "document", &mut violations);
    let Some(programme) = as_str_map(root.get("programme").unwrap_or(&Value::Null)) else {
        violations
            .push(Violation::new("missing_programme", "programme object is required".to_string()));
        return violations;
    };
    check_unknown_keys(programme, &["name", "local_extension"], "programme", &mut violations);
    if string_field(programme, "name").is_none() {
        violations.push(Violation::new(
            "missing_programme_name",
            "programme.name must be a non-empty string".to_string(),
        ));
    }

    let propositions: Vec<&Map<String, Value>> = root
        .get("propositions")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    if propositions.is_empty() {
        violations.push(Violation::new(
            "missing_propositions",
            "at least one proposition is required".to_string(),
        ));
    }

    let mut proposition_ids: BTreeSet<String> = BTreeSet::new();
    for proposition in &propositions {
        check_unknown_keys(
            proposition,
            &["id", "claim_ceiling", "local_role", "external_subject"],
            "proposition",
            &mut violations,
        );
        let Some(id) = string_field(proposition, "id") else {
            violations.push(Violation::new(
                "missing_proposition_id",
                "proposition.id must be a non-empty string".to_string(),
            ));
            continue;
        };
        if !proposition_ids.insert(id.to_string()) {
            violations.push(Violation::new(
                "duplicate_proposition",
                format!("duplicate proposition id {id}"),
            ));
        }
        if string_field(proposition, "claim_ceiling").is_none() {
            violations.push(Violation::new(
                "missing_claim_ceiling",
                format!("proposition {id} must carry a claim ceiling"),
            ));
        }
    }

    let authorities: Vec<&Map<String, Value>> = root
        .get("authorities")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    let mut authority_ids: BTreeSet<String> = BTreeSet::new();
    for authority in &authorities {
        check_unknown_keys(authority, &["id", "subject"], "authority", &mut violations);
        if let Some(id) = string_field(authority, "id") {
            if !authority_ids.insert(id.to_string()) {
                violations.push(Violation::new(
                    "duplicate_authority",
                    format!("duplicate authority id {id}"),
                ));
            }
        } else {
            violations.push(Violation::new(
                "missing_authority_id",
                "authority.id must be a non-empty string".to_string(),
            ));
        }
        if string_field(authority, "subject").is_none() {
            violations.push(Violation::new(
                "missing_authority_subject",
                "authority.subject must be a non-empty string".to_string(),
            ));
        }
    }

    let external_subjects: Vec<&Map<String, Value>> = root
        .get("external_subjects")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    let mut external_subject_ids: BTreeSet<String> = BTreeSet::new();
    for subject in &external_subjects {
        check_unknown_keys(subject, &["id", "kind"], "external_subject", &mut violations);
        if let Some(id) = string_field(subject, "id") {
            if !external_subject_ids.insert(id.to_string()) {
                violations.push(Violation::new(
                    "duplicate_external_subject",
                    format!("duplicate external subject id {id}"),
                ));
            }
        } else {
            violations.push(Violation::new(
                "missing_external_subject_id",
                "external_subject.id must be a non-empty string".to_string(),
            ));
        }
        if string_field(subject, "kind").is_none() {
            violations.push(Violation::new(
                "missing_external_subject_kind",
                "external_subject.kind must be a non-empty string".to_string(),
            ));
        }
    }

    let external_subject_ref = |proposition: &Map<String, Value>| -> Option<String> {
        string_field(proposition, "external_subject").map(str::to_string)
    };

    // Validate edges against the closed vocabulary and declared registries.
    let edges: Vec<&Map<String, Value>> = root
        .get("edges")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    let mut seen_edges: BTreeSet<String> = BTreeSet::new();
    // (selecting_authority, selection_subject) -> set of selected values.
    let mut conditional_selections: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut sidecar_targets: BTreeSet<String> = BTreeSet::new();
    let mut optional_targets: BTreeSet<String> = BTreeSet::new();

    for edge in &edges {
        let where_ = "edge";
        check_unknown_keys(edge, EDGE_KEYS, where_, &mut violations);
        let source = string_field(edge, "source");
        let kind = string_field(edge, "kind");
        let target = string_field(edge, "target");
        let Some(source) = source else {
            violations.push(Violation::new(
                "missing_edge_source",
                "edge.source must be a non-empty string".to_string(),
            ));
            continue;
        };
        let Some(kind) = kind else {
            violations.push(Violation::new(
                "unknown_edge_kind",
                format!("edge {source}: kind must be a non-empty string"),
            ));
            continue;
        };
        if !EDGE_KINDS.contains(&kind) {
            violations.push(Violation::new(
                "unknown_edge_kind",
                format!("edge {source}: unknown edge kind {kind}"),
            ));
            continue;
        }
        let Some(target) = target else {
            violations.push(Violation::new(
                "missing_edge_target",
                format!("edge {source} ({kind}): target must be a non-empty string"),
            ));
            continue;
        };
        if !proposition_ids.contains(source) {
            violations.push(Violation::new(
                "unknown_proposition_reference",
                format!("edge {source} ({kind}): unknown source proposition"),
            ));
        }
        let target_resolves =
            proposition_ids.contains(target) || external_subject_ids.contains(target);
        if !target_resolves {
            violations.push(Violation::new(
                "unknown_proposition_reference",
                format!("edge {source} ({kind}): unknown target {target}"),
            ));
        }
        match kind {
            "conditional_release_gate" => {
                for field in [
                    "selecting_authority",
                    "selected_value",
                    "selection_subject",
                    "active_predecessor",
                    "invalidation_rule",
                ] {
                    if string_field(edge, field).is_none() {
                        violations.push(Violation::new(
                            "conditional_gate_missing_fields",
                            format!("edge {source}: conditional_release_gate missing {field}"),
                        ));
                    }
                }
                if let (Some(authority), Some(subject), Some(value)) = (
                    string_field(edge, "selecting_authority"),
                    string_field(edge, "selection_subject"),
                    string_field(edge, "selected_value"),
                ) {
                    if !authority_ids.contains(authority) {
                        violations.push(Violation::new(
                            "unknown_selecting_authority",
                            format!("edge {source}: undeclared selecting authority {authority}"),
                        ));
                    }
                    conditional_selections
                        .entry((authority.to_string(), subject.to_string()))
                        .or_default()
                        .insert(value.to_string());
                }
                if let Some(predecessor) = string_field(edge, "active_predecessor")
                    && !proposition_ids.contains(predecessor)
                {
                    violations.push(Violation::new(
                        "unknown_proposition_reference",
                        format!("edge {source}: unknown active_predecessor {predecessor}"),
                    ));
                }
            }
            "consumes_if_available" => {
                optional_targets.insert(target.to_string());
            }
            "fan_in" => {
                let children = string_array(edge, "children");
                if children.is_empty() {
                    violations.push(Violation::new(
                        "fan_in_missing_children",
                        format!("edge {source}: fan_in requires children"),
                    ));
                }
                for child in &children {
                    if !proposition_ids.contains(child) {
                        violations.push(Violation::new(
                            "unknown_proposition_reference",
                            format!("edge {source}: unknown fan_in child {child}"),
                        ));
                    }
                }
                match string_field(edge, "satisfaction_source") {
                    Some(source_) if source_ == FAN_IN_SATISFACTION_SOURCE => {}
                    Some(other) => violations.push(Violation::new(
                        "fan_in_invalid_satisfaction_source",
                        format!("edge {source}: fan_in satisfaction_source must be {FAN_IN_SATISFACTION_SOURCE}, found {other}"),
                    )),
                    None => violations.push(Violation::new(
                        "fan_in_invalid_satisfaction_source",
                        format!("edge {source}: fan_in requires satisfaction_source {FAN_IN_SATISFACTION_SOURCE}"),
                    )),
                }
            }
            "external_checkpoint" => {
                if !external_subject_ids.contains(target) {
                    violations.push(Violation::new(
                        "unknown_external_subject_reference",
                        format!("edge {source}: external_checkpoint target {target} is not a declared external subject"),
                    ));
                }
                match string_field(edge, "stage") {
                    Some(stage) if EXTERNAL_STAGES.contains(&stage) => {}
                    Some(other) => violations.push(Violation::new(
                        "unknown_external_stage",
                        format!("edge {source}: unknown external stage {other}"),
                    )),
                    None => violations.push(Violation::new(
                        "unknown_external_stage",
                        format!("edge {source}: external_checkpoint requires a stage"),
                    )),
                }
            }
            "nonblocking_sidecar" => {
                // The sidecar proposition is the edge source (the adjacent
                // work); the target is the core it cannot block or satisfy.
                // Core requirement sets must exclude the sidecar source, not
                // the core target.
                sidecar_targets.insert(source.to_string());
            }
            _ => {}
        }
        let dedupe_key = format!(
            "{source}|{kind}|{target}|{}|{}|{}",
            string_field(edge, "claim_profile").unwrap_or(""),
            string_field(edge, "selected_value").unwrap_or(""),
            string_field(edge, "stage").unwrap_or(""),
        );
        if !seen_edges.insert(dedupe_key) {
            violations.push(Violation::new(
                "duplicate_edge",
                format!("edge {source} ({kind} -> {target}) is declared twice"),
            ));
        }
    }

    for ((authority, subject), values) in &conditional_selections {
        if values.len() > 1 {
            violations.push(Violation::new(
                "contradictory_conditional_selection",
                format!(
                    "authority {authority} on subject {subject} has {} active selected values; exactly one may be current",
                    values.len()
                ),
            ));
        }
    }

    // Claim profiles.
    let profiles: Vec<&Map<String, Value>> = root
        .get("claim_profiles")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    let mut profile_ids: BTreeSet<String> = BTreeSet::new();
    for profile in &profiles {
        check_unknown_keys(
            profile,
            &[
                "id",
                "version",
                "selecting_authority",
                "selection_subject",
                "required",
                "allowed_terminal_limitation_states",
                "claim_ceiling",
            ],
            "claim_profile",
            &mut violations,
        );
        let Some(id) = string_field(profile, "id") else {
            violations.push(Violation::new(
                "missing_profile_id",
                "claim_profile.id must be a non-empty string".to_string(),
            ));
            continue;
        };
        if !profile_ids.insert(id.to_string()) {
            violations
                .push(Violation::new("duplicate_profile", format!("duplicate claim profile {id}")));
        }
        if profile.get("version").and_then(Value::as_i64).is_none() {
            violations.push(Violation::new(
                "missing_profile_version",
                format!("claim profile {id}: version must be an integer"),
            ));
        }
        let authority = string_field(profile, "selecting_authority");
        let subject = string_field(profile, "selection_subject");
        match (authority, subject) {
            (Some(authority), Some(_)) => {
                if !authority_ids.contains(authority) {
                    violations.push(Violation::new(
                        "unknown_selecting_authority",
                        format!("claim profile {id}: undeclared selecting authority {authority}"),
                    ));
                }
            }
            (Some(_), None) | (None, Some(_)) => violations.push(Violation::new(
                "conditional_profile_pairing",
                format!(
                    "claim profile {id}: selecting_authority and selection_subject must be declared together"
                ),
            )),
            (None, None) => {}
        }
        let required = string_array(profile, "required");
        if required.is_empty() {
            violations.push(Violation::new(
                "missing_profile_requirements",
                format!("claim profile {id}: at least one required proposition"),
            ));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for proposition in &required {
            if !seen.insert(proposition) {
                violations.push(Violation::new(
                    "duplicate_profile_requirement",
                    format!("claim profile {id}: duplicate requirement {proposition}"),
                ));
            }
            if !proposition_ids.contains(proposition) {
                violations.push(Violation::new(
                    "unknown_proposition_reference",
                    format!("claim profile {id}: unknown required proposition {proposition}"),
                ));
            }
            if sidecar_targets.contains(proposition) {
                violations.push(Violation::new(
                    "sidecar_in_core_requirement_set",
                    format!(
                        "claim profile {id}: nonblocking_sidecar target {proposition} entered a core requirement set"
                    ),
                ));
            }
            if optional_targets.contains(proposition) {
                violations.push(Violation::new(
                    "optional_edge_in_required_set",
                    format!(
                        "claim profile {id}: consumes_if_available target {proposition} cannot become a requirement"
                    ),
                ));
            }
        }
        for state in string_array(profile, "allowed_terminal_limitation_states") {
            if !REASON_CLASSES.contains(&state.as_str()) {
                violations.push(Violation::new(
                    "unknown_reason_class",
                    format!("claim profile {id}: unknown reason class {state}"),
                ));
            }
        }
        if string_field(profile, "claim_ceiling").is_none() {
            violations.push(Violation::new(
                "missing_claim_ceiling",
                format!("claim profile {id} must carry a claim ceiling"),
            ));
        }
    }

    // requires_behavior_for_claim profile references.
    for edge in &edges {
        if string_field(edge, "kind") == Some("requires_behavior_for_claim")
            && let Some(profile) = string_field(edge, "claim_profile")
            && !profile_ids.contains(profile)
        {
            violations.push(Violation::new(
                "unknown_claim_profile",
                format!(
                    "edge {}: requires_behavior_for_claim names undeclared claim profile {profile}",
                    string_field(edge, "source").unwrap_or("?")
                ),
            ));
        }
    }

    // Projection snapshot: four independent tracks; reason classes closed;
    // terminal success never manufactured from GitHub/workflow/file state.
    if let Some(projection) = root.get("projection").and_then(as_str_map) {
        check_unknown_keys(
            projection,
            &["proposition_states", "external_stage_states"],
            "projection",
            &mut violations,
        );
        let states: Vec<&Map<String, Value>> = projection
            .get("proposition_states")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(as_str_map).collect())
            .unwrap_or_default();
        // Duplicate projection identities would make first-match lookups
        // order-significant and could flip eligibility on a reorder; they
        // fail validation instead (#10858 P2 review finding).
        let mut seen_state_ids: BTreeSet<String> = BTreeSet::new();
        for state in &states {
            check_unknown_keys(
                state,
                &["id", "implementation", "evidence", "proposition", "external"],
                "proposition_state",
                &mut violations,
            );
            let id = string_field(state, "id").unwrap_or("?");
            if let Some(id) = string_field(state, "id")
                && !seen_state_ids.insert(id.to_string())
            {
                violations.push(Violation::new(
                    "duplicate_projection_state",
                    format!("projection: duplicate proposition state id {id}"),
                ));
            }
            if let Some(id) = string_field(state, "id")
                && !proposition_ids.contains(id)
            {
                violations.push(Violation::new(
                    "unknown_proposition_reference",
                    format!("projection: unknown proposition state id {id}"),
                ));
            }
            for track_name in ["implementation", "evidence", "proposition", "external"] {
                let Some(track) = state.get(track_name).and_then(as_str_map) else {
                    continue;
                };
                check_unknown_keys(track, TRACK_KEYS, "track_state", &mut violations);
                let terminal = track.get("terminal").and_then(Value::as_bool);
                let Some(terminal) = terminal else {
                    violations.push(Violation::new(
                        "missing_track_terminal",
                        format!("projection {id}.{track_name}: terminal flag required"),
                    ));
                    continue;
                };
                if !terminal && string_field(track, "reason_class").is_none() {
                    violations.push(Violation::new(
                        "missing_reason_class",
                        format!("projection {id}.{track_name}: non-terminal state requires a reason class"),
                    ));
                }
                if let Some(reason) = string_field(track, "reason_class")
                    && !REASON_CLASSES.contains(&reason)
                {
                    violations.push(Violation::new(
                        "unknown_reason_class",
                        format!("projection {id}.{track_name}: unknown reason class {reason}"),
                    ));
                }
                match string_field(track, "derives_from") {
                    Some(derives) => {
                        if !DERIVES_FROM.contains(&derives) {
                            violations.push(Violation::new(
                                "unknown_derives_from",
                                format!(
                                    "projection {id}.{track_name}: unknown derives_from {derives}"
                                ),
                            ));
                        } else if terminal && derives != "verified_proposition" {
                            violations.push(Violation::new(
                                "manufactured_child_success",
                                format!(
                                    "projection {id}.{track_name}: terminal state derived from {derives} instead of an independently verified proposition"
                                ),
                            ));
                        }
                    }
                    None => {
                        // Terminal proposition success without declared
                        // provenance evades the manufactured-success control:
                        // unverifiable success is not proven (#10858 P1
                        // review finding). Currentness tracks (implementation,
                        // evidence, external) stay independent and do not
                        // declare fan-in provenance.
                        if track_name == "proposition" && terminal {
                            violations.push(Violation::new(
                                "missing_derives_from",
                                format!(
                                    "projection {id}.{track_name}: terminal proposition success requires derives_from provenance"
                                ),
                            ));
                        }
                    }
                }
            }
        }
        let stage_states: Vec<&Map<String, Value>> = projection
            .get("external_stage_states")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(as_str_map).collect())
            .unwrap_or_default();
        let mut seen_stage_subjects: BTreeSet<String> = BTreeSet::new();
        for stage_state in &stage_states {
            check_unknown_keys(
                stage_state,
                &["subject", "stage_reached", "satisfied_by"],
                "external_stage_state",
                &mut violations,
            );
            let subject = string_field(stage_state, "subject").unwrap_or("?");
            if string_field(stage_state, "subject").is_some()
                && !seen_stage_subjects.insert(subject.to_string())
            {
                violations.push(Violation::new(
                    "duplicate_external_stage_state",
                    format!("projection: duplicate external stage subject {subject}"),
                ));
            }
            if string_field(stage_state, "subject").is_some()
                && !external_subject_ids.contains(subject)
            {
                violations.push(Violation::new(
                    "unknown_external_subject_reference",
                    format!("projection: unknown external stage subject {subject}"),
                ));
            }
            match string_field(stage_state, "stage_reached") {
                Some(stage) if STAGE_LADDER.contains(&stage) => {}
                Some(other) => violations.push(Violation::new(
                    "unknown_external_stage",
                    format!("projection {subject}: unknown stage {other}"),
                )),
                None => violations.push(Violation::new(
                    "unknown_external_stage",
                    format!("projection {subject}: stage_reached required"),
                )),
            }
            match string_field(stage_state, "satisfied_by") {
                Some(source) if EXTERNAL_SATISFACTION_SOURCES.contains(&source) => {}
                _ => violations.push(Violation::new(
                    "internal_state_cannot_satisfy_external_stage",
                    format!(
                        "projection {subject}: an external stage can only be satisfied by external_observation, never by internal packet or merged local PR state"
                    ),
                )),
            }
        }
        // An external track that claims terminal success must be backed by an
        // externally observed stage.
        for state in &states {
            let Some(id) = string_field(state, "id") else {
                continue;
            };
            let Some(track) = state.get("external").and_then(as_str_map) else {
                continue;
            };
            if track.get("terminal").and_then(Value::as_bool) == Some(true) {
                let subject_ref = propositions
                    .iter()
                    .find(|proposition| string_field(proposition, "id") == Some(id))
                    .and_then(|proposition| external_subject_ref(proposition));
                let externally_observed = subject_ref
                    .and_then(|subject| {
                        stage_states
                            .iter()
                            .find(|entry| string_field(entry, "subject") == Some(subject.as_str()))
                    })
                    .is_some_and(|entry| {
                        string_field(entry, "satisfied_by") == Some("external_observation")
                            && string_field(entry, "stage_reached") != Some("none")
                    });
                if !externally_observed {
                    violations.push(Violation::new(
                        "internal_state_cannot_satisfy_external_stage",
                        format!("projection {id}: external track terminal without an externally observed stage"),
                    ));
                }
            }
        }
    }

    violations.sort();
    violations.dedup();
    violations
}

/// Deterministic canonical semantics: array order never changes meaning.
/// Set-valued fields (fan-in children, profile requirements, allowed
/// limitation states) are unordered by contract, so they are sorted too.
fn canonical_form(doc: &Value) -> String {
    let Some(root) = as_str_map(doc) else {
        return Value::Null.to_string();
    };
    let mut canonical = Map::new();
    for (key, value) in root {
        let ordered = match value {
            Value::Array(items) if key == "propositions" || key == "authorities" => {
                sort_by_field(items, "id")
            }
            Value::Array(items) if key == "external_subjects" => sort_by_field(items, "id"),
            Value::Array(items) if key == "edges" => {
                let canonical_edges: Vec<Value> = items.iter().map(canonical_edge).collect();
                sort_by_edge_key(&canonical_edges)
            }
            Value::Array(items) if key == "claim_profiles" => {
                let canonical_profiles: Vec<Value> = items.iter().map(canonical_profile).collect();
                sort_by_field(&canonical_profiles, "id")
            }
            Value::Array(items) if key == "proposition_states" => sort_by_field(items, "id"),
            Value::Array(items) if key == "external_stage_states" => {
                sort_by_field(items, "subject")
            }
            Value::Object(projection) if key == "projection" => canonical_projection(projection),
            other => other.clone(),
        };
        canonical.insert(key.clone(), ordered);
    }
    Value::Object(canonical).to_string()
}

/// Canonicalize the nested projection snapshot arrays.
fn canonical_projection(projection: &Map<String, Value>) -> Value {
    let mut canonical = Map::new();
    for (key, value) in projection {
        let ordered = match value {
            Value::Array(items) if key == "proposition_states" => sort_by_field(items, "id"),
            Value::Array(items) if key == "external_stage_states" => {
                sort_by_field(items, "subject")
            }
            other => other.clone(),
        };
        canonical.insert(key.clone(), ordered);
    }
    Value::Object(canonical)
}

/// Sort a set-valued string array field inside an object.
fn sorted_string_set(object: &Map<String, Value>, field: &str) -> Option<Value> {
    let values = object.get(field)?.as_array()?;
    let mut sorted: Vec<String> =
        values.iter().filter_map(Value::as_str).map(str::to_string).collect();
    sorted.sort();
    sorted.dedup();
    Some(Value::Array(sorted.into_iter().map(Value::String).collect()))
}

fn canonical_edge(edge: &Value) -> Value {
    let Some(object) = as_str_map(edge) else {
        return edge.clone();
    };
    let mut canonical = object.clone();
    if let Some(children) = sorted_string_set(object, "children") {
        canonical.insert("children".to_string(), children);
    }
    Value::Object(canonical)
}

fn canonical_profile(profile: &Value) -> Value {
    let Some(object) = as_str_map(profile) else {
        return profile.clone();
    };
    let mut canonical = object.clone();
    for field in ["required", "allowed_terminal_limitation_states"] {
        if let Some(sorted) = sorted_string_set(object, field) {
            canonical.insert(field.to_string(), sorted);
        }
    }
    Value::Object(canonical)
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

fn sort_by_edge_key(items: &[Value]) -> Value {
    let mut sorted: Vec<&Value> = items.iter().collect();
    sorted.sort_by_key(|item| {
        let object = item.as_object();
        let field = |name: &str| {
            object
                .and_then(|object| object.get(name))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        (
            field("source"),
            field("kind"),
            field("target"),
            field("claim_profile"),
            field("selected_value"),
            field("stage"),
        )
    });
    Value::Array(sorted.into_iter().cloned().collect())
}

/// Expand the exact requirement set of one claim profile: profile members
/// plus active implementation predecessors, active conditional gates,
/// behavior-for-claim targets, and fan-in children. `consumes_if_available`
/// and `nonblocking_sidecar` edges never enter the set.
fn profile_requirements(doc: &Value, profile_id: &str) -> Option<BTreeSet<String>> {
    let root = doc.as_object()?;
    let profiles = root.get("claim_profiles")?.as_array()?;
    let profile = profiles
        .iter()
        .find(|profile| {
            profile.as_object().and_then(|object| string_field(object, "id")) == Some(profile_id)
        })?
        .as_object()?;
    let edges: Vec<&Map<String, Value>> = root
        .get("edges")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();

    let mut requirements: BTreeSet<String> =
        string_array(profile, "required").into_iter().collect();
    // Fixpoint expansion: requirements may themselves carry requirement
    // edges. The set is bounded by the declared propositions and subjects.
    loop {
        let mut added = false;
        for edge in &edges {
            let source = string_field(edge, "source").unwrap_or("");
            let kind = string_field(edge, "kind").unwrap_or("");
            let target = string_field(edge, "target").unwrap_or("");
            if !requirements.contains(source) {
                continue;
            }
            let introduces: Option<Vec<String>> = match kind {
                "requires_implementation" => Some(vec![target.to_string()]),
                "conditional_release_gate" => string_field(edge, "active_predecessor")
                    .map(|predecessor| vec![predecessor.to_string()]),
                "fan_in" => Some(string_array(edge, "children")),
                _ => None,
            };
            for requirement in introduces.unwrap_or_default() {
                added |= requirements.insert(requirement);
            }
        }
        // requires_behavior_for_claim binds the named profile regardless of
        // the source's membership.
        for edge in &edges {
            if string_field(edge, "kind") == Some("requires_behavior_for_claim")
                && string_field(edge, "claim_profile") == Some(profile_id)
                && let Some(target) = string_field(edge, "target")
            {
                added |= requirements.insert(target.to_string());
            }
        }
        if !added {
            break;
        }
    }
    Some(requirements)
}

/// Evaluate one claim profile against the deterministic projection snapshot.
/// A profile is eligible only when every requirement is an independently
/// verified terminal proposition and every external checkpoint it reaches is
/// externally observed at the required stage or later.
fn profile_eligibility(doc: &Value, profile_id: &str) -> Option<Eligibility> {
    let requirements = profile_requirements(doc, profile_id)?;
    let root = doc.as_object()?;
    let states: Vec<&Map<String, Value>> = root
        .get("projection")
        .and_then(as_str_map)
        .and_then(|projection| projection.get("proposition_states"))
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    let stage_states: Vec<&Map<String, Value>> = root
        .get("projection")
        .and_then(as_str_map)
        .and_then(|projection| projection.get("external_stage_states"))
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();
    let edges: Vec<&Map<String, Value>> = root
        .get("edges")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(as_str_map).collect())
        .unwrap_or_default();

    let state_of =
        |id: &str| states.iter().find(|state| string_field(state, "id") == Some(id)).cloned();
    let proposition_track = |id: &str| -> Option<&Map<String, Value>> {
        state_of(id).and_then(|state| state.get("proposition").and_then(as_str_map))
    };

    let mut reasons: BTreeMap<String, String> = BTreeMap::new();
    for requirement in &requirements {
        match proposition_track(requirement) {
            Some(track) => {
                let terminal = track.get("terminal").and_then(Value::as_bool);
                let verified = string_field(track, "derives_from")
                    .map(|derives| derives == "verified_proposition");
                // A requirement passes only when its proposition track is
                // terminally satisfied by an independently verified
                // proposition; undeclared provenance cannot pass (#10858 P1
                // review finding: missing evidence stays not_proven).
                let passes = terminal == Some(true) && verified == Some(true);
                if !passes {
                    let reason = string_field(track, "reason_class").unwrap_or("not_proven");
                    reasons.insert(requirement.clone(), reason.to_string());
                }
            }
            None => {
                reasons.insert(requirement.clone(), "not_proven".to_string());
            }
        }
    }

    // External checkpoints reachable from the requirement set.
    for edge in &edges {
        if string_field(edge, "kind") != Some("external_checkpoint") {
            continue;
        }
        let source = string_field(edge, "source").unwrap_or("");
        if !requirements.contains(source) {
            continue;
        }
        let subject = string_field(edge, "target").unwrap_or("");
        let required_stage = string_field(edge, "stage").unwrap_or("");
        let entry =
            stage_states.iter().find(|entry| string_field(entry, "subject") == Some(subject));
        let satisfied = entry.is_some_and(|entry| {
            let reached = string_field(entry, "stage_reached").unwrap_or("none");
            let externally = string_field(entry, "satisfied_by") == Some("external_observation");
            externally
                && stage_rank(reached) > 0
                && stage_rank(reached) >= stage_rank(required_stage)
        });
        if !satisfied {
            reasons.insert(format!("external:{subject}"), "pending_external_stage".to_string());
        }
    }

    let eligible = reasons.is_empty();
    Some(Eligibility { profile: profile_id.to_string(), eligible, requirements, reasons })
}

fn stage_rank(stage: &str) -> usize {
    STAGE_LADDER.iter().position(|candidate| *candidate == stage).unwrap_or(0)
}

/// Adapt one landed programme train manifest into a contract document using
/// the declared class-to-kind adaptation rows. Unknown manifest classes fail
/// closed; manifest bytes are never rewritten.
struct ManifestAdaptations {
    rows: BTreeMap<(String, String), (String, Option<String>)>,
    manifests: Vec<(String, String)>,
}

fn load_adaptations(root: &Path) -> Result<ManifestAdaptations> {
    let path = root.join(ADAPTATIONS_PATH);
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut rows = BTreeMap::new();
    let mut manifests = Vec::new();
    let Some(root_map) = as_str_map(&value) else {
        bail!("adaptations document must be an object");
    };
    if string_field(root_map, "schema") != Some("train_edge_contract.adaptations.v1") {
        bail!("unexpected adaptations schema");
    }
    for row in root_map.get("adaptations").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(row) = as_str_map(row) else {
            bail!("adaptation row must be an object");
        };
        let (Some(schema), Some(class), Some(kind)) = (
            string_field(row, "programme_schema"),
            string_field(row, "class"),
            string_field(row, "kind"),
        ) else {
            bail!("adaptation row missing programme_schema/class/kind");
        };
        if !EDGE_KINDS.contains(&kind) {
            bail!("adaptation row references unknown shared kind {kind}");
        }
        let stage = string_field(row, "stage");
        if (kind == "external_checkpoint") != stage.is_some() {
            bail!(
                "adaptation row for {kind} must declare a stage exactly when the kind is external_checkpoint"
            );
        }
        rows.insert(
            (schema.to_string(), class.to_string()),
            (kind.to_string(), stage.map(str::to_string)),
        );
    }
    for manifest in root_map.get("manifests").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(manifest) = as_str_map(manifest) else {
            bail!("manifest entry must be an object");
        };
        let (Some(bundle), Some(schema)) =
            (string_field(manifest, "bundle"), string_field(manifest, "programme_schema"))
        else {
            bail!("manifest entry missing bundle/programme_schema");
        };
        manifests.push((bundle.to_string(), schema.to_string()));
    }
    if manifests.is_empty() {
        bail!("at least one landed manifest must be declared");
    }
    Ok(ManifestAdaptations { rows, manifests })
}

/// Adapt a landed train manifest's per-node dependency edges into the shared
/// vocabulary, preserving every target and provenance byte. Returns the
/// adapted document plus per-shared-kind edge counts for reporting.
fn adapt_manifest(
    manifest: &Value,
    programme_schema: &str,
    adaptations: &ManifestAdaptations,
) -> Result<(Value, BTreeMap<String, usize>)> {
    let Some(root) = as_str_map(manifest) else {
        bail!("train manifest must be an object");
    };
    if string_field(root, "schema") != Some(programme_schema) {
        bail!(
            "manifest schema {} does not match declared programme schema {programme_schema}",
            string_field(root, "schema").unwrap_or("?")
        );
    }
    let empty_values: Vec<Value> = Vec::new();
    let nodes = root
        .get("nodes")
        .and_then(Value::as_array)
        .unwrap_or(&empty_values)
        .iter()
        .filter_map(as_str_map)
        .collect::<Vec<_>>();
    let external_authorities: BTreeMap<String, String> = root
        .get("external_authorities")
        .and_then(Value::as_array)
        .unwrap_or(&empty_values)
        .iter()
        .filter_map(as_str_map)
        .filter_map(|authority| {
            let id = string_field(authority, "id")?;
            let subject = string_field(authority, "subject").unwrap_or("");
            Some((id.to_string(), subject.to_string()))
        })
        .collect();

    let mut referenced_external: BTreeSet<String> = BTreeSet::new();
    let mut edges: Vec<Value> = Vec::new();
    let mut kind_counts: BTreeMap<String, usize> = BTreeMap::new();
    for node in &nodes {
        let source = string_field(node, "node_id").unwrap_or("");
        for dependency in
            node.get("dependencies").and_then(Value::as_array).unwrap_or(&empty_values)
        {
            let Some(dependency) = as_str_map(dependency) else {
                bail!("node {source}: dependency must be an object");
            };
            let class = string_field(dependency, "class").unwrap_or("");
            let target = string_field(dependency, "target").unwrap_or("");
            let Some((kind, stage)) =
                adaptations.rows.get(&(programme_schema.to_string(), class.to_string()))
            else {
                bail!(
                    "manifest {programme_schema} uses dependency class {class} with no declared adaptation row; the contract fails closed"
                );
            };
            if external_authorities.contains_key(target) {
                referenced_external.insert(target.to_string());
            }
            let mut edge = Map::new();
            edge.insert("source".to_string(), Value::String(source.to_string()));
            edge.insert("kind".to_string(), Value::String(kind.clone()));
            edge.insert("target".to_string(), Value::String(target.to_string()));
            if let Some(stage) = stage {
                edge.insert("stage".to_string(), Value::String(stage.clone()));
            }
            if let Some(provenance) = string_field(dependency, "provenance") {
                edge.insert("provenance".to_string(), Value::String(provenance.to_string()));
            }
            *kind_counts.entry(kind.clone()).or_default() += 1;
            edges.push(Value::Object(edge));
        }
    }

    let propositions: Vec<Value> = nodes
        .iter()
        .map(|node| {
            let mut proposition = Map::new();
            proposition.insert(
                "id".to_string(),
                Value::String(string_field(node, "node_id").unwrap_or("").to_string()),
            );
            proposition.insert(
                "claim_ceiling".to_string(),
                Value::String(string_field(node, "claim_ceiling").unwrap_or("").to_string()),
            );
            if let Some(role) = string_field(node, "train_role") {
                proposition.insert("local_role".to_string(), Value::String(role.to_string()));
            }
            Value::Object(proposition)
        })
        .collect();

    let external_subjects: Vec<Value> = referenced_external
        .iter()
        .map(|id| {
            let mut subject = Map::new();
            subject.insert("id".to_string(), Value::String(id.clone()));
            subject.insert("kind".to_string(), Value::String("external_authority".to_string()));
            Value::Object(subject)
        })
        .collect();

    // Programme-owned claim profiles (module_train.v1 embeds them as data)
    // adapt into the shared shape; the programme keeps ownership.
    let mut claim_profiles: Vec<Value> = Vec::new();
    for profile in root
        .get("claim_profiles")
        .and_then(Value::as_array)
        .unwrap_or(&empty_values)
        .iter()
        .filter_map(as_str_map)
    {
        let mut adapted = Map::new();
        adapted.insert(
            "id".to_string(),
            Value::String(string_field(profile, "id").unwrap_or("").to_string()),
        );
        adapted.insert("version".to_string(), Value::Number(serde_json::Number::from(1)));
        let members = string_array(profile, "members");
        adapted.insert(
            "required".to_string(),
            Value::Array(members.iter().map(|member| Value::String(member.clone())).collect()),
        );
        adapted.insert("allowed_terminal_limitation_states".to_string(), Value::Array(Vec::new()));
        adapted.insert(
            "claim_ceiling".to_string(),
            Value::String(string_field(profile, "definition").unwrap_or("").to_string()),
        );
        claim_profiles.push(Value::Object(adapted));
    }

    let mut document = Map::new();
    document.insert("schema".to_string(), Value::String("train_edge_contract.v1".to_string()));
    document.insert("schema_version".to_string(), Value::Number(serde_json::Number::from(1)));
    document
        .insert("contract_id".to_string(), Value::String(format!("{programme_schema}.adapted")));
    let mut programme = Map::new();
    programme.insert("name".to_string(), Value::String(programme_schema.to_string()));
    programme.insert(
        "local_extension".to_string(),
        Value::String(
            "adapted from the landed train manifest; manifest bytes unchanged".to_string(),
        ),
    );
    document.insert("programme".to_string(), Value::Object(programme));
    if !external_subjects.is_empty() {
        document.insert("external_subjects".to_string(), Value::Array(external_subjects));
    }
    document.insert("propositions".to_string(), Value::Array(propositions));
    document.insert("edges".to_string(), Value::Array(edges));
    document.insert("claim_profiles".to_string(), Value::Array(claim_profiles));
    Ok((Value::Object(document), kind_counts))
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
        (&["$defs", "edge_kind", "enum"], EDGE_KINDS, "edge kinds"),
        (&["$defs", "external_checkpoint_stage", "enum"], EXTERNAL_STAGES, "external stages"),
        (&["$defs", "reason_class", "enum"], REASON_CLASSES, "reason classes"),
        (&["$defs", "derives_from", "enum"], DERIVES_FROM, "derives_from values"),
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

/// Entry point: validate the closed contract, the programme-neutral
/// fixtures, the canonical-semantics control, and the declared adaptations
/// of the landed programme train manifests.
pub fn run() -> Result<()> {
    let root = project_root()?;
    let mut failures: Vec<String> = Vec::new();

    for violation in validate_schema_file(&root)? {
        failures.push(format!("{SCHEMA_PATH}: {violation}"));
    }

    let schema = load_json(&root.join(SCHEMA_PATH))?;
    let fixture_dir = root.join(FIXTURE_DIR);
    let valid_documents = ["neovim_examples.v1.json", "external_stages.v1.json"];
    for name in valid_documents {
        let doc = load_json(&fixture_dir.join(name))?;
        let violations = validate_document(&doc);
        if !violations.is_empty() {
            failures
                .push(format!("{name}: expected a valid contract document, got {violations:?}"));
        }
        // `validate_document` is the Rust-side reader of the contract, and
        // `validate_schema_file` only proves the schema still declares the
        // pinned vocabulary. Applying the compiled schema to the document is
        // what binds a schema-only consumer to `additionalProperties`, the
        // `required` lists, and every nested `$defs` constraint (#14268).
        failures.extend(validate_payload_against_schema(
            &schema,
            SCHEMA_PATH,
            &doc,
            &format!("{FIXTURE_DIR}/{name}"),
        )?);
    }

    // Deterministic profile eligibility for the reviewed examples: the run
    // path itself proves the fixture semantics (bounded core, selected
    // branch, absent optional evidence, independent siblings, external
    // stages), not only the unit-test falsifiers.
    let expected_eligibility: &[(&str, &str, bool)] = &[
        ("neovim_examples.v1.json", "neovim_bounded_core", true),
        ("neovim_examples.v1.json", "neovim_atomic_incremental", false),
        ("neovim_examples.v1.json", "neovim_nightly_channel", true),
        ("neovim_examples.v1.json", "neovim_stable_channel", false),
        ("neovim_examples.v1.json", "neovim_platform_linux", true),
        ("neovim_examples.v1.json", "neovim_platform_macos", false),
        ("external_stages.v1.json", "coc_public_artifact_core", false),
    ];
    for (file, profile_id, expected_eligible) in expected_eligibility {
        let doc = load_json(&fixture_dir.join(file))?;
        match profile_eligibility(&doc, profile_id) {
            Some(eligibility) => {
                if eligibility.eligible != *expected_eligible {
                    failures.push(format!(
                        "{file}: profile {profile_id} eligibility {} but expected {expected_eligible} (reasons: {:?})",
                        eligibility.eligible, eligibility.reasons
                    ));
                }
            }
            None => failures.push(format!("{file}: profile {profile_id} did not resolve")),
        }
    }
    // Fixture 2 discrimination: the ineligible atomic profile is blocked
    // exactly by the unproven selected branch.
    let neovim = load_json(&fixture_dir.join("neovim_examples.v1.json"))?;
    if let Some(atomic) = profile_eligibility(&neovim, "neovim_atomic_incremental")
        && atomic.reasons.get("P_atomic_ranged_actual_host_proof")
            != Some(&"not_proven".to_string())
    {
        failures.push(
            "neovim_examples.v1.json: the atomic profile must be blocked by the unproven selected branch"
                .to_string(),
        );
    }
    // Fixture 5 discrimination: the public-artifact profile is blocked
    // exactly by the pending external stage.
    let stages = load_json(&fixture_dir.join("external_stages.v1.json"))?;
    if let Some(public_artifact) = profile_eligibility(&stages, "coc_public_artifact_core")
        && public_artifact.reasons.get("external:ext_marketplace")
            != Some(&"pending_external_stage".to_string())
    {
        failures.push(
            "external_stages.v1.json: the public-artifact profile must be blocked by the pending external stage"
                .to_string(),
        );
    }

    // Canonical semantics: shuffled input must produce identical output.
    let base = load_json(&fixture_dir.join("neovim_examples.v1.json"))?;
    let shuffled =
        load_json(&fixture_dir.join("shuffled").join("neovim_examples_shuffled.v1.json"))?;
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
    }

    // Landed manifests adapt into the shared vocabulary without rewriting
    // their bytes.
    let adaptations = load_adaptations(&root)?;
    for (bundle, programme_schema) in &adaptations.manifests {
        let manifest = load_json(&root.join(bundle).join("train.manifest.json"))?;
        let (adapted, kind_counts) = match adapt_manifest(&manifest, programme_schema, &adaptations)
        {
            Ok(adapted) => adapted,
            Err(error) => {
                failures.push(format!("{bundle}: adaptation failed: {error}"));
                continue;
            }
        };
        let violations = validate_document(&adapted);
        if !violations.is_empty() {
            failures.push(format!(
                "{bundle}: adapted document violates the shared contract: {:?}",
                violation_codes(&violations)
            ));
        } else {
            let total: usize = kind_counts.values().sum();
            let summary: Vec<String> =
                kind_counts.iter().map(|(kind, count)| format!("{kind}={count}")).collect();
            println!("{bundle}: adapted {total} edges ({})", summary.join(", "));
        }
    }

    if failures.is_empty() {
        println!(
            "train_edge_contract.v1: closed contract, fixtures, canonical control, and landed-manifest adaptations all valid"
        );
        Ok(())
    } else {
        bail!("train edge contract check failed:\n{}", failures.join("\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    fn fixture(name: &str) -> TestResult<Value> {
        load_json(&project_root()?.join(FIXTURE_DIR).join(name))
    }

    fn eligibility(doc: &Value, profile: &str) -> Eligibility {
        match profile_eligibility(doc, profile) {
            Some(eligibility) => eligibility,
            None => panic!("profile {profile} must resolve"),
        }
    }

    fn schema() -> TestResult<Value> {
        load_json(&project_root()?.join(SCHEMA_PATH))
    }

    /// The committed documents must satisfy the published schema, not only
    /// the Rust reader. Without this the schema is documentation.
    #[test]
    fn committed_documents_satisfy_the_published_schema() -> TestResult {
        let schema = schema()?;
        for name in ["neovim_examples.v1.json", "external_stages.v1.json"] {
            let violations =
                validate_payload_against_schema(&schema, SCHEMA_PATH, &fixture(name)?, name)?;
            assert!(violations.is_empty(), "{name}: {violations:?}");
        }
        Ok(())
    }

    /// Discriminating controls for #14268.
    ///
    /// The Rust reader is deliberately thorough about unknown keys, missing
    /// fields, empty strings, and duplicate ids -- it already catches those,
    /// and this test does not claim otherwise. The gap the apply step closes
    /// is the narrower set of *value* constraints the reader never looks at:
    /// `claim_profile.version` is only checked for being an integer, never
    /// against `minimum: 1`; and `allowed_terminal_limitation_states` is only
    /// checked for reason-class membership, never against `uniqueItems`.
    /// Both documents passed the task before the apply step while violating
    /// the published contract.
    #[test]
    fn schema_rejects_values_the_rust_reader_never_constrains() -> TestResult {
        let cases: &[(&str, fn(&mut Value))] = &[
            ("version below the schema minimum", |doc| {
                doc["claim_profiles"][0]["version"] = Value::from(0);
            }),
            ("duplicate terminal limitation state", |doc| {
                let first =
                    doc["claim_profiles"][0]["allowed_terminal_limitation_states"][0].clone();
                if let Some(states) =
                    doc["claim_profiles"][0]["allowed_terminal_limitation_states"].as_array_mut()
                {
                    states.push(first);
                }
            }),
        ];

        for (label, mutate) in cases {
            let mut doc = fixture("neovim_examples.v1.json")?;
            mutate(&mut doc);

            // Negative control: the Rust reader accepts every one of these.
            let reader_violations = validate_document(&doc);
            assert!(
                reader_violations.is_empty(),
                "{label}: the Rust reader is not expected to catch this: {:?}",
                violation_codes(&reader_violations)
            );

            let violations = validate_payload_against_schema(
                &schema()?,
                SCHEMA_PATH,
                &doc,
                "neovim_examples.v1.json",
            )?;
            assert!(
                !violations.is_empty(),
                "{label}: the applied schema must reject this document"
            );
        }
        Ok(())
    }

    // Fixture 1: full-document v0.18 does not require atomic-ranged
    // actual-host proof; fixture 3: parser/race evidence may remain absent
    // for the bounded core profile.
    #[test]
    fn bounded_core_excludes_atomic_proof_and_optional_evidence() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let bounded = eligibility(&doc, "neovim_bounded_core");
        assert!(bounded.eligible, "reasons: {:?}", bounded.reasons);
        assert!(!bounded.requirements.contains("P_atomic_ranged_actual_host_proof"));
        assert!(!bounded.requirements.contains("P_parser_race_evidence"));
        Ok(())
    }

    // Fixture 2: atomic-incremental does require its selected branch.
    #[test]
    fn atomic_incremental_requires_selected_branch() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let atomic = eligibility(&doc, "neovim_atomic_incremental");
        assert!(atomic.requirements.contains("P_atomic_ranged_actual_host_proof"));
        assert!(!atomic.eligible);
        assert_eq!(
            atomic.reasons.get("P_atomic_ranged_actual_host_proof"),
            Some(&"not_proven".to_string())
        );
        // The full-document proposition is not gated for the atomic profile.
        assert!(!atomic.requirements.contains("P_full_document_v0_18"));
        Ok(())
    }

    // Fixture 4: one platform or install channel can pass while siblings
    // remain not_proven.
    #[test]
    fn sibling_channels_and_platforms_stay_independent() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        assert!(eligibility(&doc, "neovim_nightly_channel").eligible);
        assert!(!eligibility(&doc, "neovim_stable_channel").eligible);
        assert!(eligibility(&doc, "neovim_platform_linux").eligible);
        let macos = eligibility(&doc, "neovim_platform_macos");
        assert!(!macos.eligible);
        assert_eq!(macos.reasons.get("P_platform_macos"), Some(&"not_proven".to_string()));
        Ok(())
    }

    // Fixture 5: a prepared packet is not submitted, accepted, released, or
    // publicly installed.
    #[test]
    fn prepared_packet_stops_before_every_external_stage() -> TestResult {
        let doc = fixture("external_stages.v1.json")?;
        let profile = eligibility(&doc, "coc_public_artifact_core");
        assert!(!profile.eligible);
        assert_eq!(
            profile.reasons.get("external:ext_marketplace"),
            Some(&"pending_external_stage".to_string())
        );
        let stage_state =
            doc.pointer("/projection/external_stage_states/0").and_then(as_str_map).unwrap();
        assert_eq!(
            string_field(stage_state, "stage_reached"),
            Some("none"),
            "a prepared internal packet reaches no external stage"
        );
        assert_eq!(string_field(stage_state, "satisfied_by"), Some("not_satisfied"));
        Ok(())
    }

    // Fixture 6: a DAP sidecar cannot block or satisfy LSP core.
    #[test]
    fn dap_sidecar_never_blocks_or_satisfies_core() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        // Core-adjacent profiles stay eligible while the sidecar is not proven.
        assert!(eligibility(&doc, "neovim_bounded_core").eligible);
        // No profile requirement set contains the sidecar.
        let profiles = doc.get("claim_profiles").and_then(Value::as_array).unwrap();
        for profile in profiles {
            let required = string_array(profile.as_object().unwrap(), "required");
            assert!(
                !required.contains(&"P_dap_sidecar".to_string()),
                "sidecar must never enter a core requirement set"
            );
        }
        // The sidecar edge itself validates as nonblocking.
        assert!(validate_document(&doc).is_empty());
        Ok(())
    }

    // Fixture 7: fan-in cannot pass from closed issues or merged helpers.
    #[test]
    fn fan_in_rejects_manufactured_child_success() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let composition = eligibility(&doc, "neovim_bounded_core");
        // The fan-in composition's children are independently terminal, so
        // the bounded profile that includes the composition stays eligible.
        let _ = composition;
        let invalid = load_json(
            &project_root()?
                .join(FIXTURE_DIR)
                .join("invalid")
                .join("fan_in_from_github_state.json"),
        )?;
        let violations = validate_document(&invalid);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"manufactured_child_success"), "{codes:?}");
        Ok(())
    }

    // Fixture 8: unknown edge kind or selecting authority fails.
    #[test]
    fn unknown_edge_kind_and_authority_fail_closed() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        assert!(validate_document(&doc).is_empty());
        let invalid_kind = load_json(
            &project_root()?.join(FIXTURE_DIR).join("invalid").join("unknown_edge_kind.json"),
        )?;
        let violations = validate_document(&invalid_kind);
        assert!(violation_codes(&violations).contains(&"unknown_edge_kind"));
        let invalid_authority = load_json(
            &project_root()?
                .join(FIXTURE_DIR)
                .join("invalid")
                .join("unknown_selecting_authority.json"),
        )?;
        let violations = validate_document(&invalid_authority);
        assert!(violation_codes(&violations).contains(&"unknown_selecting_authority"));
        Ok(())
    }

    // Fixture 9: two active conditional alternatives are invalid.
    #[test]
    fn two_active_conditional_alternatives_are_invalid() -> TestResult {
        let invalid = load_json(
            &project_root()?
                .join(FIXTURE_DIR)
                .join("invalid")
                .join("two_active_conditional_alternatives.json"),
        )?;
        let violations = validate_document(&invalid);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"contradictory_conditional_selection"), "{codes:?}");
        Ok(())
    }

    // Fixture 10: shuffled input produces identical canonical semantics.
    #[test]
    fn shuffled_input_produces_identical_canonical_semantics() -> TestResult {
        let base = fixture("neovim_examples.v1.json")?;
        let shuffled = load_json(
            &project_root()?
                .join(FIXTURE_DIR)
                .join("shuffled")
                .join("neovim_examples_shuffled.v1.json"),
        )?;
        assert_eq!(canonical_form(&base), canonical_form(&shuffled));
        // And eligibility judgments are identical per profile.
        for profile in ["neovim_bounded_core", "neovim_atomic_incremental"] {
            assert_eq!(
                eligibility(&base, profile).eligible,
                eligibility(&shuffled, profile).eligible
            );
        }
        Ok(())
    }

    // Negative control: every edge becoming an unconditional implementation
    // dependency changes the requirement set (the distinction is real).
    #[test]
    fn unqualified_normalization_would_change_requirements() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let bounded = profile_requirements(&doc, "neovim_bounded_core").unwrap();
        let mut normalized = doc.clone();
        let edges = normalized.get_mut("edges").and_then(Value::as_array_mut).unwrap();
        for edge in edges.iter_mut() {
            if let Some(object) = edge.as_object_mut()
                && object.get("kind").and_then(Value::as_str) == Some("consumes_if_available")
            {
                object.insert(
                    "kind".to_string(),
                    Value::String("requires_implementation".to_string()),
                );
            }
        }
        let inflated = profile_requirements(&normalized, "neovim_bounded_core").unwrap();
        assert!(inflated.len() > bounded.len());
        assert!(inflated.contains("P_parser_race_evidence"));
        Ok(())
    }

    // Negative control: a programme-specific state is never normalized away.
    #[test]
    fn local_extension_fields_survive_validation() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let mut localized = doc.clone();
        let proposition = localized
            .get_mut("propositions")
            .and_then(Value::as_array_mut)
            .unwrap()
            .first_mut()
            .unwrap()
            .as_object_mut()
            .unwrap();
        proposition.insert(
            "local_role".to_string(),
            Value::String("programme-local editor role".to_string()),
        );
        assert!(validate_document(&localized).is_empty());
        Ok(())
    }

    // The full run path must pass against the real tree.
    #[test]
    fn run_passes_on_current_tree() -> TestResult {
        run()
    }

    // Adaptation is lossless: every dependency target and provenance byte
    // survives the class-to-kind mapping (a mutation dropping or rewriting
    // provenance must fail this equality).
    #[test]
    fn adaptation_preserves_targets_and_provenance() -> TestResult {
        let root = project_root()?;
        let adaptations = load_adaptations(&root)?;
        let manifest =
            load_json(&root.join(".spec/11764-controller-train-graph/train.manifest.json"))?;
        let mut expected: Vec<(String, String)> = Vec::new();
        for node in manifest.get("nodes").and_then(Value::as_array).unwrap() {
            let source = node.get("node_id").and_then(Value::as_str).unwrap();
            for dependency in node.get("dependencies").and_then(Value::as_array).unwrap() {
                expected.push((
                    format!(
                        "{source}|{}",
                        dependency.get("target").and_then(Value::as_str).unwrap()
                    ),
                    dependency.get("provenance").and_then(Value::as_str).unwrap_or("").to_string(),
                ));
            }
        }
        let (adapted, _) = adapt_manifest(&manifest, "issue_controller_train.v1", &adaptations)?;
        let mut actual: Vec<(String, String)> = Vec::new();
        for edge in adapted.get("edges").and_then(Value::as_array).unwrap() {
            actual.push((
                format!(
                    "{}|{}",
                    edge.get("source").and_then(Value::as_str).unwrap(),
                    edge.get("target").and_then(Value::as_str).unwrap()
                ),
                edge.get("provenance").and_then(Value::as_str).unwrap_or("").to_string(),
            ));
        }
        expected.sort();
        actual.sort();
        assert_eq!(expected.len(), actual.len());
        assert_eq!(expected, actual);
        Ok(())
    }

    // Adaptability falsifiers: the landed emacs/controller/module manifests
    // adapt losslessly, and an unknown manifest class fails closed.
    #[test]
    fn landed_manifests_adapt_into_the_shared_vocabulary() -> TestResult {
        let root = project_root()?;
        let adaptations = load_adaptations(&root)?;
        assert!(adaptations.manifests.len() >= 3);
        let mut edge_counts = BTreeMap::new();
        for (bundle, programme_schema) in &adaptations.manifests {
            let manifest = load_json(&root.join(bundle).join("train.manifest.json"))?;
            let (adapted, kind_counts) = adapt_manifest(&manifest, programme_schema, &adaptations)?;
            let violations = validate_document(&adapted);
            assert!(violations.is_empty(), "{bundle}: {violations:?}");
            let count: usize = kind_counts.values().sum();
            edge_counts.insert(bundle.clone(), count);
        }
        // The two required adaptability anchors carry their landed edges.
        assert!(edge_counts.get(".spec/10918-emacs-train-graph").unwrap_or(&0) > &0);
        assert!(edge_counts.get(".spec/11764-controller-train-graph").unwrap_or(&0) > &0);
        Ok(())
    }

    #[test]
    fn unknown_manifest_class_fails_closed() -> TestResult {
        let root = project_root()?;
        let adaptations = load_adaptations(&root)?;
        let manifest = load_json(&root.join(".spec/10918-emacs-train-graph/train.manifest.json"))?;
        let mut mutated = manifest.clone();
        // Mutate the first dependency that exists on any node.
        let nodes = mutated.get_mut("nodes").and_then(Value::as_array_mut).unwrap();
        let mut mutated_any = false;
        for node in nodes.iter_mut() {
            let Some(dependencies) = node
                .as_object_mut()
                .and_then(|node| node.get_mut("dependencies"))
                .and_then(Value::as_array_mut)
                .filter(|dependencies| !dependencies.is_empty())
            else {
                continue;
            };
            if let Some(first) = dependencies.first_mut().and_then(Value::as_object_mut) {
                first.insert("class".to_string(), Value::String("soft".to_string()));
                mutated_any = true;
                break;
            }
        }
        assert!(mutated_any, "the emacs manifest must carry dependencies");
        assert!(
            adapt_manifest(&mutated, "emacs_train.v1", &adaptations).is_err(),
            "unknown class must fail closed"
        );
        Ok(())
    }

    // Negative control: mutable GitHub/check/receipt state cannot be embedded.
    #[test]
    fn mutable_state_is_rejected() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let mut mutated = doc.clone();
        let edge = mutated
            .get_mut("edges")
            .and_then(Value::as_array_mut)
            .unwrap()
            .first_mut()
            .unwrap()
            .as_object_mut()
            .unwrap();
        edge.insert("ci_status".to_string(), Value::String("success".to_string()));
        let violations = validate_document(&mutated);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"mutable_state_embedded"), "{codes:?}");
        Ok(())
    }

    // Sidecar entering a core requirement set is rejected (negative control).
    #[test]
    fn sidecar_in_core_spine_is_rejected() -> TestResult {
        let invalid = load_json(
            &project_root()?.join(FIXTURE_DIR).join("invalid").join("sidecar_in_core_spine.json"),
        )?;
        let violations = validate_document(&invalid);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"sidecar_in_core_requirement_set"), "{codes:?}");
        Ok(())
    }

    // The sidecar control binds the sidecar SOURCE (the adjacent-work
    // proposition), never the core target it points at: a profile requiring
    // only the core validates cleanly.
    #[test]
    fn sidecar_control_binds_the_source_not_the_core_target() -> TestResult {
        let doc = load_json(
            &project_root()?.join(FIXTURE_DIR).join("invalid").join("sidecar_in_core_spine.json"),
        )?;
        let mut core_only = doc.clone();
        let profile = core_only
            .get_mut("claim_profiles")
            .and_then(Value::as_array_mut)
            .unwrap()
            .first_mut()
            .unwrap()
            .as_object_mut()
            .unwrap();
        profile.insert(
            "required".to_string(),
            Value::Array(vec![Value::String("P_lsp_core".to_string())]),
        );
        assert!(
            validate_document(&core_only).is_empty(),
            "a core-only profile must not be rejected for pointing at a sidecar's core target: {:?}",
            violation_codes(&validate_document(&core_only))
        );
        // And the sidecar source alone is still rejected.
        let mut sidecar_only = doc.clone();
        let profile = sidecar_only
            .get_mut("claim_profiles")
            .and_then(Value::as_array_mut)
            .unwrap()
            .first_mut()
            .unwrap()
            .as_object_mut()
            .unwrap();
        profile.insert(
            "required".to_string(),
            Value::Array(vec![Value::String("P_dap_sidecar".to_string())]),
        );
        let violations = validate_document(&sidecar_only);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"sidecar_in_core_requirement_set"), "{codes:?}");
        Ok(())
    }

    // Terminal proposition success without declared provenance is not
    // proven: validation fails and eligibility never passes.
    #[test]
    fn terminal_proposition_without_provenance_is_not_proven() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let mut mutated = doc.clone();
        let states = mutated
            .get_mut("projection")
            .and_then(Value::as_object_mut)
            .and_then(|projection| projection.get_mut("proposition_states"))
            .and_then(Value::as_array_mut)
            .unwrap();
        let full_document = states
            .iter_mut()
            .find(|state| state.get("id").and_then(Value::as_str) == Some("P_full_document_v0_18"))
            .unwrap()
            .as_object_mut()
            .unwrap();
        full_document
            .get_mut("proposition")
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove("derives_from");
        let violations = validate_document(&mutated);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"missing_derives_from"), "{codes:?}");
        // The bounded core profile loses eligibility: missing evidence stays
        // not_proven instead of passing on undeclared provenance.
        let eligibility = eligibility(&mutated, "neovim_bounded_core");
        assert!(!eligibility.eligible);
        assert_eq!(
            eligibility.reasons.get("P_full_document_v0_18"),
            Some(&"not_proven".to_string())
        );
        Ok(())
    }

    // Duplicate projection identities fail validation instead of making
    // first-match lookups order-significant.
    #[test]
    fn duplicate_projection_states_fail() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let mut duplicated = doc.clone();
        let states = duplicated
            .get_mut("projection")
            .and_then(Value::as_object_mut)
            .and_then(|projection| projection.get_mut("proposition_states"))
            .and_then(Value::as_array_mut)
            .unwrap();
        let first = states.first().cloned().unwrap();
        states.push(first);
        let violations = validate_document(&duplicated);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"duplicate_projection_state"), "{codes:?}");
        // Eligibility is unaffected by the duplicate row's position.
        let eligibility_before = eligibility(&doc, "neovim_bounded_core");
        let eligibility_after = eligibility(&duplicated, "neovim_bounded_core");
        assert_eq!(eligibility_before.eligible, eligibility_after.eligible);
        assert_eq!(eligibility_before.reasons, eligibility_after.reasons);
        Ok(())
    }

    // Root-level mutable state is rejected just like nested embedding.
    #[test]
    fn root_level_mutable_state_is_rejected() -> TestResult {
        let doc = fixture("neovim_examples.v1.json")?;
        let mut mutated = doc.clone();
        let root = mutated.as_object_mut().unwrap();
        root.insert("ci_status".to_string(), Value::String("success".to_string()));
        let violations = validate_document(&mutated);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"mutable_state_embedded"), "{codes:?}");
        Ok(())
    }

    // An external checkpoint cannot be satisfied by internal state.
    #[test]
    fn external_checkpoint_rejects_internal_satisfaction() -> TestResult {
        let invalid = load_json(
            &project_root()?
                .join(FIXTURE_DIR)
                .join("invalid")
                .join("external_stage_satisfied_internally.json"),
        )?;
        let violations = validate_document(&invalid);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"internal_state_cannot_satisfy_external_stage"), "{codes:?}");
        Ok(())
    }

    // consumes_if_available must never block unrelated work (negative
    // control fixture plus the eligibility view).
    #[test]
    fn optional_edge_cannot_block_unrelated_work() -> TestResult {
        let invalid = load_json(
            &project_root()?
                .join(FIXTURE_DIR)
                .join("invalid")
                .join("optional_edge_in_required_set.json"),
        )?;
        let violations = validate_document(&invalid);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"optional_edge_in_required_set"), "{codes:?}");
        Ok(())
    }

    // A non-terminal track without a reason class fails (fail-closed
    // semantics for stage/proposition separation).
    #[test]
    fn non_terminal_state_requires_reason_class() -> TestResult {
        let invalid = load_json(
            &project_root()?.join(FIXTURE_DIR).join("invalid").join("missing_reason_class.json"),
        )?;
        let violations = validate_document(&invalid);
        let codes = violation_codes(&violations);
        assert!(codes.contains(&"missing_reason_class"), "{codes:?}");
        Ok(())
    }
}
