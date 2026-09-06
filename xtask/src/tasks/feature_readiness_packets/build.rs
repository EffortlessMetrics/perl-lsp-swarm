//! Construction of canonical `feature_readiness_builder_packet.v1` and
//! `feature_readiness_reviewer_packet.v1` documents from one registry node.
//!
//! Stable sections derive only from the registry; observational planes are
//! supplied explicitly (currently: honest `unavailable` markers for the
//! unlanded trains) so live refresh can never rewrite stable semantics.
//! Packet identity is content-addressed: the id embeds the SHA-256 of the
//! canonical bytes of the document without its own id field.

use serde_json::{Value, json};

use super::model::{
    Disposition, FORBIDDEN_SURFACES_SHARED, LiveAction, NodeSpec, Role, registry_digest,
    sequence_strings,
};
use super::render;

pub const BUILDER_SCHEMA: &str = "feature_readiness_builder_packet.v1";
pub const REVIEWER_SCHEMA: &str = "feature_readiness_reviewer_packet.v1";

const REPOSITORY_NAME: &str = "EffortlessMetrics/perl-lsp-swarm";
const FULL_DAG_AUTHORITY_ISSUE: u32 = 11279;
const CURRENT_TREE_OWNER_ISSUE: u32 = 11280;
const OFFLINE_READINESS_OWNER_ISSUE: u32 = 11281;
const INSTRUMENT_FAILURE_BEHAVIOR: &str = "instrument failure marks the affected cell NOT_PROVEN in the receipt; it never becomes a pass, an empty success, or a skip";

/// An explicit, complete live observation consumed from `--live-snapshot`.
///
/// A snapshot is evidence, never a source of defaults: every observation key
/// must be supplied by the caller (a deliberately unknown candidate branch is
/// written as `null`), so `writer_active: false` and
/// `required_action: "none"` can only ever mean *observed*, not *unobserved*.
#[derive(Clone, Debug)]
pub struct LiveSnapshot {
    pub head_sha: String,
    pub candidate_branch: Option<String>,
    pub writer_active: bool,
    pub required_action: LiveAction,
    pub source_digest: String,
}

impl LiveSnapshot {
    /// Parse one caller-supplied snapshot document. Missing observations,
    /// mistyped values, and unknown keys all fail closed with a diagnostic
    /// naming the exact cell; the canonical observation digest becomes part of
    /// packet identity. The observed main head is deliberately excluded from
    /// that digest because conflict-free main movement is observational, while
    /// the remaining live observations remain identity-bearing.
    pub fn parse(bytes: &[u8]) -> color_eyre::eyre::Result<Self> {
        use color_eyre::eyre::{Context, bail};
        let doc: Value =
            serde_json::from_slice(bytes).with_context(|| "parsing --live-snapshot as JSON")?;
        let object = doc
            .as_object()
            .ok_or_else(|| color_eyre::eyre::eyre!("live snapshot must be an object"))?;
        let mut head_sha = None;
        let mut candidate_branch_seen = false;
        let mut candidate_branch = None;
        let mut writer_active = None;
        let mut required_action = None;
        for key in object.keys() {
            match key.as_str() {
                "head_sha" => {
                    head_sha =
                        Some(object.get("head_sha").and_then(Value::as_str).ok_or_else(|| {
                            color_eyre::eyre::eyre!("live-snapshot head_sha must be a string")
                        })?);
                }
                "candidate_branch" => {
                    candidate_branch_seen = true;
                    if let Some(value) =
                        object.get("candidate_branch").filter(|value| !value.is_null())
                    {
                        let Some(text) = value.as_str() else {
                            bail!("live-snapshot candidate_branch must be a string or null");
                        };
                        candidate_branch = Some(text.to_owned());
                    }
                }
                "writer_active" => {
                    writer_active = Some(
                        object.get("writer_active").and_then(Value::as_bool).ok_or_else(|| {
                            color_eyre::eyre::eyre!("live-snapshot writer_active must be a boolean")
                        })?,
                    );
                }
                "required_action" => {
                    let raw =
                        object.get("required_action").and_then(Value::as_str).ok_or_else(|| {
                            color_eyre::eyre::eyre!(
                                "live-snapshot required_action must be a string"
                            )
                        })?;
                    required_action = Some(LiveAction::parse(raw).ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "live-snapshot required_action {raw:?} is outside the closed vocabulary"
                        )
                    })?);
                }
                other => bail!(
                    "live-snapshot has no closed field {other:?}; expected head_sha/candidate_branch/writer_active/required_action"
                ),
            }
        }
        let head_sha: String = head_sha
            .ok_or_else(|| color_eyre::eyre::eyre!("live-snapshot requires head_sha"))?
            .to_lowercase();
        if !(40..=64).contains(&head_sha.len()) || !head_sha.chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("live-snapshot head_sha must be 40-64 hex characters");
        }
        // Fail closed on incomplete observations: an `observed` live plane
        // binds candidate/writer/action facts, so a partial snapshot may not
        // shrink into defaults that would hide an active writer or required
        // repair behind preflight_required=false.
        let missing: Vec<&str> = [
            ("candidate_branch", candidate_branch_seen),
            ("writer_active", writer_active.is_some()),
            ("required_action", required_action.is_some()),
        ]
        .into_iter()
        .filter(|(_, seen)| !seen)
        .map(|(key, _)| key)
        .collect();
        if !missing.is_empty() {
            bail!(
                "incomplete live snapshot: no explicit observation for {}; an observed plane cannot default writer/action state, supply them explicitly (null is allowed only for candidate_branch)",
                missing.join(", ")
            );
        }
        let Some(writer_active) = writer_active else {
            bail!("incomplete live snapshot: missing explicit writer_active");
        };
        let Some(required_action) = required_action else {
            bail!("incomplete live snapshot: missing explicit required_action");
        };
        let mut identity_doc = doc;
        if let Some(object) = identity_doc.as_object_mut() {
            object.remove("head_sha");
        }
        let canonical_observations = render::canonical_json(&identity_doc);
        Ok(Self {
            head_sha,
            candidate_branch,
            writer_active,
            required_action,
            source_digest: crate::tasks::emacs_train_context::digest::sha256_hex(
                canonical_observations.as_bytes(),
            ),
        })
    }
}

fn role_kind_word(role: Role) -> &'static str {
    match role {
        Role::ProductImplementation => "feat",
        Role::ProofOnly | Role::InstalledClientProof => "test",
        Role::ResearchDecision => "research",
        Role::GovernanceSupport => "chore",
    }
}

fn explicit_dependencies(node: &NodeSpec, nodes: &[NodeSpec]) -> Vec<u32> {
    declared_prerequisites(node)
        .into_iter()
        .filter(|issue| nodes.iter().any(|candidate| candidate.issues.contains(issue)))
        .collect()
}

pub(crate) fn declared_prerequisites(node: &NodeSpec) -> Vec<u32> {
    let mut values = Vec::new();
    let text = node.prerequisite_disposition.as_bytes();
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'#' {
            let start = index + 1;
            let mut end = start;
            while end < text.len() && text[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                if let Ok(issue) = node.prerequisite_disposition[start..end].parse::<u32>() {
                    if !values.contains(&issue) {
                        values.push(issue);
                    }
                }
            }
            index = end;
        } else {
            index += 1;
        }
    }
    // Registry prose also names prerequisites by their stable `fr_<issue>`
    // node identity. Treat both spellings as the same declaration so routing
    // cannot silently diverge from the claim ceiling.
    let text = node.prerequisite_disposition.as_bytes();
    let mut index = 0;
    while index + 3 < text.len() {
        if text[index..].starts_with(b"fr_") {
            let start = index + 3;
            let mut end = start;
            while end < text.len() && text[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                if let Ok(issue) = node.prerequisite_disposition[start..end].parse::<u32>() {
                    if !values.contains(&issue) {
                        values.push(issue);
                    }
                }
            }
            index = end;
        } else {
            index += 1;
        }
    }
    values
}

fn explicit_unblocks(node: &NodeSpec, nodes: &[NodeSpec]) -> Vec<u32> {
    node.successors
        .iter()
        .filter_map(|successor| nodes.iter().find(|candidate| candidate.node_id == *successor))
        .filter_map(|successor| successor.issues.first().copied())
        .collect()
}

pub(crate) fn delivery_issue_ids(node: &NodeSpec, nodes: &[NodeSpec]) -> (u32, Vec<u32>, Vec<u32>) {
    (node.controller_issue, explicit_dependencies(node, nodes), explicit_unblocks(node, nodes))
}

/// The builder document for `node` under the optional live observation.
/// Returns the document (including its content-addressed `packet_id`) plus
/// the hex digest of its canonical bytes.
pub fn builder_document(node: &NodeSpec, live: Option<&LiveSnapshot>) -> (Value, String) {
    let nodes = super::nodes::all_nodes();
    let doc = json!({
        "schema": BUILDER_SCHEMA,
        "repository": { "name": REPOSITORY_NAME },
        "work": {
            "node_id": node.node_id,
            "issues": node.issues,
            "controller_issue": node.controller_issue,
            "domain": node.domain.as_str(),
            "role": node.role.as_str(),
            "disposition": node.disposition.as_str(),
            "profile": node.profile.as_str(),
            "objective_sentence": node.objective_sentence,
            "registry_scope": "representative_packet_fixtures",
            "registry_digest": registry_digest(&nodes),
        },
        "claim_ceiling": {
            "establishes": node.establishes,
            "cannot_establish": node.cannot_establish,
            "prerequisite_disposition": node.prerequisite_disposition,
            "successors": node.successors,
            "remaining_not_proven": node.remaining_not_proven,
            "rollback_meaning": node.rollback_meaning,
        },
        "planes": {
            "stable": {
                "status": "embedded_fixture_registry",
                "digest": registry_digest(&nodes),
                "full_dag_authority_issue": FULL_DAG_AUTHORITY_ISSUE,
            },
            "current_tree": {
                "status": "unavailable",
                "owner_issue": CURRENT_TREE_OWNER_ISSUE,
            },
            "offline_readiness": {
                "status": "unavailable",
                "owner_issue": OFFLINE_READINESS_OWNER_ISSUE,
            },
            "live": live_plane_value(live),
        },
        "authorities": node.authorities.iter().map(|entry| json!({
            "ref": entry.reference,
            "subject": entry.subject,
            "group": entry.group.as_str(),
        })).collect::<Vec<_>>(),
        "operations": node.operations.iter().map(|row| json!({
            "feature": row.feature,
            "provider_or_client": row.provider_or_client,
            "source_subject": row.source_subject,
            "policy": {
                "semantic": row.policy_semantic,
                "currentness": row.policy_currentness,
                "fallback": row.policy_fallback,
                "refusal": row.policy_refusal,
                "legitimate_empty": row.policy_legitimate_empty,
            },
            "canonical_owner": row.canonical_owner,
            "old_path_disposition": row.old_path,
            "proof_owner": row.proof_owner,
        })).collect::<Vec<_>>(),
        "surfaces": {
            "allowed": node.allowed_surfaces,
            "forbidden": forbidden_surface_values(node),
        },
        "artifacts": node.artifacts.iter().map(|row| json!({
            "id": row.id,
            "kind": row.kind,
            "owner": row.owner,
            "mode": row.mode.as_str(),
            "current_disposition": row.current_disposition,
            "required_change_or_proof": row.required_change_or_proof,
            "check_command": row.check_command,
            "review_lens": row.review_lens,
            "claim_impact": row.claim_impact,
        })).collect::<Vec<_>>(),
        "durable_spec": {
            "disposition": node.durable_spec.0.as_str(),
            "owner": node.durable_spec.1,
            "note": node.durable_spec.2,
        },
        "sequence": sequence_strings(node.role),
        "proof": {
            "first_falsifier": {
                "description": node.first_falsifier.0,
                "expected_red_reason": node.first_falsifier.1,
                "canonical_owner": node.first_falsifier.2,
            },
            "positive_discriminator": node.positive_discriminator,
            "controls": node.controls.iter().map(|row| json!({
                "class": row.class,
                "subject": row.subject,
            })).collect::<Vec<_>>(),
            "commands": node.commands.iter().map(|(id, command, scope)| json!({
                "id": id,
                "command": command,
                "scope": scope,
            })).collect::<Vec<_>>(),
            "instrument_failure_behavior": INSTRUMENT_FAILURE_BEHAVIOR,
        },
        "delivery": {
            "branch_suggestion": format!("agent/{}", node.node_id),
            "pr_title_suggestion": format!(
                "{}({}): {}",
                role_kind_word(node.role),
                node.domain.as_str(),
                node.objective_sentence
            ),
            "base_head": base_head_value(live),
            "issues": {
                "controller": node.controller_issue,
                "dependencies": explicit_dependencies(node, &nodes),
                "unblocks": explicit_unblocks(node, &nodes),
            },
            "changed_surfaces": node.allowed_surfaces,
            "old_path_dispositions": old_path_values(node),
            "limitations": limitation_values(node),
            "review_map": review_map_values(node),
            "stop_before": stop_condition_values(node),
        },
        "stop": {
            "conditions": stop_condition_values(node),
            "forbidden_actions": forbidden_actions(node),
            "handoff": handoff_text(node),
        },
    });
    let packet_id = format!("frbld_{}", &content_digest(&doc)[..16]);
    let doc = with_root_id(doc, "packet_id", packet_id);
    let digest = content_digest(&doc);
    (doc, digest)
}

/// The independent adversarial reviewer document for the same subject.
pub fn reviewer_document(node: &NodeSpec, live: Option<&LiveSnapshot>) -> (Value, String) {
    let (builder, builder_digest) = builder_document(node, live);
    let doc = json!({
        "schema": REVIEWER_SCHEMA,
        "subject": {
            "node_id": node.node_id,
            "issues": node.issues,
            "role": node.role.as_str(),
            "profile": node.profile.as_str(),
            "claim_ceiling_sentence": node.objective_sentence,
        },
        "builder_ref": {
            "packet_id": builder["packet_id"],
            "digest": builder_digest,
        },
        "currentness": {
            "base_head": base_head_value(live),
            "live_state": live_state_value(live),
            "invalidators": [
                "stable train, typed edge, profile, conflict, or artifact contract change",
                "current-tree observation change once #11280 lands",
                "accepted issue decision change",
                "canonical API/schema/provider/framework/config route change",
                "falsifier, fixture, receipt, or generator identity change",
                "candidate base/head/diff change",
                "live writer/review state change",
                "claim or external/manual boundary change",
            ],
            "stale_rule": "a review of another head/base/diff/packet/artifact contract is stale for the affected dimensions; unrelated comments, timestamps, or declared-equivalent main movement do not churn the packet",
        },
        "lenses": lens_values(node),
        "stage_falsification_examples": node
            .stage_examples
            .iter()
            .map(|index| {
                let example = &super::nodes::STAGE_EXAMPLES[*index];
                json!({
                    "stage": example.stage,
                    "question": example.question,
                })
            })
            .collect::<Vec<_>>(),
        "negative_control_audit": negative_control_audit(node),
        "old_path_audit": old_path_audit(node),
        "stop": {
            "reviewer_must_not": [
                "repeat the builder summary without applying the stage-specific falsifiers",
                "accept a test name, snapshot, or claimed mutation as sufficient evidence",
                "treat construction-context output as the only detection surface for a substantive merge",
                "approve while any required lens lacks an attempted angle and outcome",
                "submit findings that change no review criterion",
            ],
        },
    });
    let digest_without_id = content_digest(&doc);
    let review_id = format!("frrvw_{}", &digest_without_id[..16]);
    let doc = with_root_id(doc, "review_id", review_id);
    let digest = content_digest(&doc);
    (doc, digest)
}

/// Insert a root id field; a non-object root (impossible by construction)
/// simply drops the id and fails downstream validation instead of panicking.
fn with_root_id(mut doc: Value, key: &str, id: String) -> Value {
    if let Value::Object(map) = &mut doc {
        map.insert(key.to_owned(), Value::String(id));
    }
    doc
}

fn forbidden_surface_values(node: &NodeSpec) -> Vec<String> {
    // Order-stable dedupe: nodes may restate shared surfaces verbatim.
    let mut seen = std::collections::BTreeSet::new();
    FORBIDDEN_SURFACES_SHARED
        .iter()
        .chain(node.forbidden_surfaces.iter())
        .filter(|value| seen.insert(**value))
        .map(|value| (*value).to_owned())
        .collect()
}

fn stop_condition_values(node: &NodeSpec) -> Vec<String> {
    let mut conditions: Vec<String> =
        node.extra_stop_conditions.iter().map(|value| (*value).to_owned()).collect();
    conditions.extend(derived_stop_conditions(node));
    conditions
}

fn live_plane_value(live: Option<&LiveSnapshot>) -> Value {
    match live {
        None => json!({
            "state": "unknown",
            "snapshot_digest": Value::Null,
            "head_sha": Value::Null,
            "candidate_branch": Value::Null,
            "writer_active": Value::Null,
            "required_action": "none",
            "preflight_required": true,
        }),
        // Reachable only with a snapshot that parse() accepted as a complete
        // observation: every writer/candidate/action cell was supplied
        // explicitly, so `observed` never hides an unobserved state and
        // preflight_required=false is earned rather than assumed.
        Some(snapshot) => json!({
            "state": "observed",
            "snapshot_digest": snapshot.source_digest,
            "head_sha": snapshot.head_sha,
            "candidate_branch": snapshot.candidate_branch,
            "writer_active": snapshot.writer_active,
            "required_action": snapshot.required_action.as_str(),
            "preflight_required": false,
        }),
    }
}

fn live_state_value(live: Option<&LiveSnapshot>) -> &'static str {
    if live.is_some() { "observed" } else { "unknown" }
}

fn base_head_value(live: Option<&LiveSnapshot>) -> Value {
    match live {
        None => json!("unknown-until-read-only-writer-preflight"),
        Some(snapshot) => json!(format!("main@{}", snapshot.head_sha)),
    }
}

fn old_path_values(node: &NodeSpec) -> Vec<Value> {
    if node.old_paths.is_empty() {
        return vec![
            json!({ "seam": "none declared for this node", "terminal_disposition": "none" }),
        ];
    }
    node.old_paths
        .iter()
        .map(|row| json!({ "seam": row.seam, "terminal_disposition": row.terminal_disposition }))
        .collect()
}

fn old_path_audit(node: &NodeSpec) -> Vec<Value> {
    old_path_values(node)
}

fn limitation_values(node: &NodeSpec) -> Vec<String> {
    node.cannot_establish
        .iter()
        .chain(node.remaining_not_proven.iter())
        .map(|value| (*value).to_owned())
        .collect()
}

fn review_map_values(node: &NodeSpec) -> Vec<String> {
    let mut entries: Vec<String> = node
        .lenses
        .iter()
        .filter(|lens| lens.applicable)
        .map(|lens| format!("independent lens {}: challenge with the packet questions", lens.name))
        .collect();
    entries.push("adversarial reviewer packet (feature_readiness_reviewer_packet.v1) generated from the same subject digests".to_owned());
    entries
}

fn derived_stop_conditions(node: &NodeSpec) -> Vec<String> {
    let mut conditions = vec![
        "stop before merge, release, publication, or any external action".to_owned(),
        "stop and return the exact gap when a sufficient packet would require inventing semantics, a second authority, mutable status, or model-specific folklore".to_owned(),
    ];
    if !node.role.allows_product_implementation() {
        conditions.push(
            "stop before any product repair; this role owns evidence or rulings only".to_owned(),
        );
    }
    conditions
}

fn forbidden_actions(node: &NodeSpec) -> Vec<&'static str> {
    let mut actions = vec![
        "model_invocation",
        "scheduler_or_lease_mutation",
        "merge_without_current_substantive_review",
        "release_publication_or_support_state_change",
    ];
    if !node.role.allows_product_implementation() {
        actions.push("product_repair_from_non_product_role");
    }
    if matches!(node.disposition, Disposition::Deferred | Disposition::BlockedExternalManual) {
        actions.push("generic_prompt_framework_creation");
        actions.push("spec_planning_tree_creation");
    }
    actions
}

fn handoff_text(node: &NodeSpec) -> String {
    format!(
        "deliver through the current $deliver-pr route; the packet authorizes only the listed surfaces and stops before {}",
        if node.role.allows_product_implementation() {
            "merge/release/external actions"
        } else {
            "any execution beyond this role"
        }
    )
}

fn lens_values(node: &NodeSpec) -> Vec<Value> {
    const ALL_LENSES: &[&str] = &[
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
    ALL_LENSES
        .iter()
        .map(|name| match node.lenses.iter().find(|lens| lens.name == *name) {
            Some(lens) => json!({
                "name": lens.name,
                "applicable": lens.applicable,
                "reason": lens.reason,
                "questions": lens.questions,
            }),
            None => json!({
                "name": name,
                "applicable": false,
                "reason": "not load-bearing for this node's role and domain",
                "questions": [],
            }),
        })
        .collect()
}

fn control_requirement(class: &str) -> &'static str {
    match class {
        "mutation" => {
            "the named mutation produces the smallest expected divergence in the intended cell"
        }
        "stale" => "the stale input fails the currentness assertion instead of passing silently",
        "false_empty" => {
            "instrument failure or missing data never renders as [], null, no-change, or clean success"
        }
        "wrong_subject" => {
            "the oracle cannot be satisfied by a different binary, provider, root, client, or stage"
        }
        "duplicate_authority" => {
            "no second authority beside the canonical owner can satisfy the oracle"
        }
        "unsafe_edit" => "an unsafe edit cannot survive movement between prepare and apply",
        "near_miss_framework"
        | "near_miss_client"
        | "near_miss_platform"
        | "near_miss_artifact_stage" => {
            "the adjacent-stage lookalike fails while the intended subject passes"
        }
        "unexpected_duplicate_blocker" => {
            "an unexpected duplicate path surfaces as a blocker, never as convergence"
        }
        "fixture_as_semantic_support" => {
            "fixtures or token types cannot stand in for semantic support"
        }
        _ => "the control fails for its intended reason and smallest cell",
    }
}

fn negative_control_audit(node: &NodeSpec) -> Vec<Value> {
    let mut rows: Vec<Value> = node
        .controls
        .iter()
        .map(|row| {
            json!({
                "subject": format!("{}: {}", row.class, row.subject),
                "requirement": control_requirement(row.class),
            })
        })
        .collect();
    for artifact in &node.artifacts {
        rows.push(json!({
            "subject": format!("artifact completeness: {}", artifact.id),
            "requirement": "a missing generator or check command for this artifact invalidates prose completeness claims",
        }));
    }
    rows.push(json!({
        "subject": format!("first falsifier: {}", node.first_falsifier.0),
        "requirement": "it materializes red for the expected reason before positive code exists and passes only for the intended implementation",
    }));
    rows
}

/// Canonical bytes of a document with its own id field removed, hashed to
/// the lowercase hex SHA-256 that content-addresses the packet.
pub fn content_digest(doc: &Value) -> String {
    let mut stripped = doc.clone();
    if let Some(object) = stripped.as_object_mut() {
        object.remove("packet_id");
        object.remove("review_id");
    }
    // A conflict-free movement of main is observational, not a change to the
    // candidate subject. Keep the observed semantic live cells and exact source
    // snapshot digest in the identity. The digest binds the relevant source/
    // head contents, while the displayed main head remains observational:
    // equivalent conflict-free main movement does not churn the packet.
    if let Some(delivery) = stripped.pointer_mut("/delivery").and_then(Value::as_object_mut) {
        delivery.insert("base_head".to_owned(), Value::String("main@equivalent".to_owned()));
    }
    if let Some(currentness) = stripped.pointer_mut("/currentness").and_then(Value::as_object_mut) {
        currentness.insert("base_head".to_owned(), Value::String("main@equivalent".to_owned()));
    }
    if let Some(live) = stripped.pointer_mut("/planes/live").and_then(Value::as_object_mut) {
        live.insert("head_sha".to_owned(), Value::Null);
    }
    let canonical = render::canonical_json(&stripped);
    crate::tasks::emacs_train_context::digest::sha256_hex(canonical.as_bytes())
}

/// Recompute whether a document's embedded id matches its content digest.
pub fn id_matches_content(doc: &Value) -> bool {
    let id_key = if doc.get("packet_id").is_some() { "packet_id" } else { "review_id" };
    let Some(id) = doc.get(id_key).and_then(Value::as_str) else {
        return false;
    };
    let digest = content_digest(doc);
    let expected_suffix = &digest[..16];
    match id.strip_prefix(if id_key == "packet_id" { "frbld_" } else { "frrvw_" }) {
        Some(suffix) => suffix == expected_suffix,
        None => false,
    }
}
