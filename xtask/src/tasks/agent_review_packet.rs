//! Validate and render the shared adversarial review-packet, review-finding,
//! and advisory stage closure-projection contracts
//! (`agent_review_packet.v1`, `agent_review_finding.v1`,
//! `stage_closure_projection.v1`, issue #10881).
//!
//! This module owns the closed shared review vocabulary (lenses, roles,
//! outcomes, dispositions, check criteria), the deterministic validation
//! invariants, and the canonical machine, Markdown, and compact projections
//! including the closed seed falsification questions no rendering may drop.
//! It converts caller-supplied facts into model-neutral challenge documents.
//!
//! It deliberately owns no GitHub or network observation, no model
//! invocation, no review submission, no review-thread retention, no
//! mergeability decision, no issue closure, and no live review state. Every
//! required input is supplied by the caller; missing input fails validation
//! instead of producing plausible prose. Document instances are
//! runtime-local outputs: the render path writes projections to stdout only
//! and never writes tracked repository state.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Fetch a string nested under a chain of object sections.
fn nested_str<'a>(root: &'a Map<String, Value>, sections: &[&str], key: &str) -> &'a str {
    let mut current = root;
    for section in sections {
        match current.get(*section).and_then(as_str_map) {
            Some(child) => current = child,
            None => return "",
        }
    }
    string_field(current, key).unwrap_or("")
}

const PACKET_SCHEMA_PATH: &str = "schemas/agent_review_packet.v1.schema.json";
const PACKET_SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/agent_review_packet.v1.schema.json";
const PACKET_SCHEMA_NAME: &str = "agent_review_packet.v1";

const FINDING_SCHEMA_PATH: &str = "schemas/agent_review_finding.v1.schema.json";
const FINDING_SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/agent_review_finding.v1.schema.json";
const FINDING_SCHEMA_NAME: &str = "agent_review_finding.v1";

const CLOSURE_SCHEMA_PATH: &str = "schemas/stage_closure_projection.v1.schema.json";
const CLOSURE_SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/stage_closure_projection.v1.schema.json";
const CLOSURE_SCHEMA_NAME: &str = "stage_closure_projection.v1";

const FIXTURE_DIR: &str = "fixtures/agent_review_packet";
const GOLDEN_DIR: &str = "fixtures/agent_review_packet/golden";

const VALID_FIXTURES: &[&str] = &[
    "challenger_service_marker.v1.json",
    "consumer_issue_controller_t07_shape.v1.json",
    "finding_stale_marker_resolved.v1.json",
    "closure_service_marker_eligible.v1.json",
    "closure_open_finding.v1.json",
];
const SHUFFLED_FIXTURE: &str = "shuffled/challenger_service_marker_shuffled.v1.json";

/// Closed base review-lens vocabulary. A base lens may never be silently
/// omitted; programme refinements attach inside a lens entry.
const REVIEW_LENSES: &[&str] = &[
    "semantic_correctness",
    "architecture_authority_duplication",
    "subject_evidence_identity",
    "lifecycle_currentness_concurrency",
    "security_trust_boundary",
    "resource_retention_cleanup",
    "platform_runtime_portability",
    "spec_test_docs_consistency",
    "release_external_boundary",
];

const LENS_APPLICABILITIES: &[&str] = &["required", "not_applicable"];

/// Closed differentiated review-role vocabulary. The closure projection is
/// derived over individual reviews and findings; it is not itself a role.
const REVIEW_ROLES: &[&str] =
    &["builder_self_review", "adversarial_challenger", "specialist", "evidence_worker"];

/// Closed typed disposition per migrated/replaced seam.
const OLD_PATH_DISPOSITIONS: &[&str] = &[
    "removed",
    "unreachable",
    "compatibility_projection",
    "historical_salvage_only",
    "still_live_independent",
    "unexpected_duplicate",
];

/// Closed programme-supplied obligation families.
const OBLIGATION_KINDS: &[&str] = &[
    "spec_ledger_ids",
    "fixture_expectation_manifests",
    "tests_mutations",
    "generated_artifacts",
    "docs_projections",
    "change_fragments",
];

/// Closed negative-control audit criteria. A test name or fixture file alone
/// is not proof that a falsifier is load-bearing.
const CHECK_CRITERIA: &[&str] = &[
    "exists",
    "red_before_or_mutation_evidence",
    "passes_only_intended_implementation",
    "correct_subject_and_generation",
    "independent_expectation_source",
    "alternate_subject_exclusion",
];

/// Reason code emitted when a criterion is honestly `not_established`.
/// Gaps are findings, never passes.
const CRITERION_FAILURE_CODES: &[(&str, &str)] = &[
    ("exists", "missing_negative_control"),
    ("red_before_or_mutation_evidence", "control_not_load_bearing"),
    ("passes_only_intended_implementation", "weak_discriminator"),
    ("correct_subject_and_generation", "wrong_subject_control"),
    ("independent_expectation_source", "circular_expectation"),
    ("alternate_subject_exclusion", "weak_discriminator"),
];

const CHECK_STATUSES: &[&str] = &["established", "not_established"];

const CANDIDATE_STATES: &[&str] = &["not_observed", "observed"];

/// Closed seed falsification questions. The shared renderer appends them to
/// every packet projection; they cannot disappear from a challenge.
const SEED_QUESTIONS: &[(&str, &str)] = &[
    ("Q_seed_proposition", "What exact proposition would this PR establish?"),
    ("Q_seed_wrong_impl", "What realistic wrong implementation could pass a weak test?"),
    ("Q_seed_substrate", "What distinguishes substrate/mechanism from external behavior?"),
    ("Q_seed_currentness", "Which subject/currentness mismatch could create a false green?"),
    ("Q_seed_duplicate", "Which existing authority might have been duplicated?"),
    ("Q_seed_cleanup", "Which failure, cleanup, or retention path is easiest to omit?"),
    ("Q_seed_widening", "Which claim could be accidentally widened?"),
];

/// Closed typed review outcomes.
const FINDING_OUTCOMES: &[&str] = &[
    "material_blocker",
    "bounded_follow_up",
    "question_requires_evidence",
    "claim_narrowing_required",
    "stale_or_wrong_subject",
    "instrument_failure",
    "no_finding",
    "resolved_current_head",
];

/// Closed severity, deterministically derived from the outcome.
const FINDING_SEVERITIES: &[&str] = &["material", "bounded", "advisory"];

fn derived_severity(outcome: &str) -> &'static str {
    match outcome {
        "material_blocker" | "stale_or_wrong_subject" => "material",
        "bounded_follow_up" | "question_requires_evidence" | "claim_narrowing_required" => {
            "bounded"
        }
        _ => "advisory",
    }
}

const BUILDER_RESPONSE_DISPOSITIONS: &[&str] = &["accepted", "rejected", "clarified"];

/// Closed terminal states of a finding within the owning review.
/// `instrument_failure` may only stay open or be superseded by a re-run.
const FINAL_DISPOSITIONS: &[&str] = &[
    "open",
    "resolved_on_current_head",
    "claim_narrowed",
    "withdrawn_stale_subject",
    "superseded_by_rerun",
    "closed_no_finding",
];

/// Closed terminality states of one review role inside a closure projection.
const ROLE_STATES: &[&str] = &["terminal", "pending", "not_applicable"];

/// Closed state of one finding inside a closure projection.
const CLOSURE_FINDING_STATES: &[&str] =
    &["resolved_on_current_head", "open", "narrowed", "withdrawn"];

const CLOSURE_ELIGIBILITIES: &[&str] = &["closure_eligible", "not_eligible"];

/// Closed advisory authorization of a closure projection.
const CLOSURE_AUTHORIZATION: &str = "advisory_only";

/// Closed forbidden mutable-state key family. A durable review document
/// never embeds assignment, lease, reviewer, liveness, or queue state.
const MUTABLE_STATE_KEYS: &[&str] = &[
    "lease",
    "lease_owner",
    "assignment",
    "assigned_agent",
    "agent_id",
    "reviewer_lease",
    "review_queue",
    "wake_event",
    "liveness",
    "heartbeat",
    "task_order",
    "active_goal",
    "frontier_cursor",
    "next_wake",
    "owner_token",
];

/// Closed banned generic statements. "Add tests" or "run the workspace" is
/// not a falsification question or a falsifier.
const GENERIC_STATEMENTS: &[&str] = &[
    "add tests",
    "add more tests",
    "run the workspace",
    "run all tests",
    "run the test suite",
    "make ci green",
];

const PACKET_ROOT_KEYS: &[&str] = &[
    "schema",
    "schema_version",
    "packet_id",
    "subject",
    "challenge",
    "lenses",
    "negative_controls",
    "old_paths",
    "obligations",
    "roles",
    "lifecycle",
    "metadata",
];

const PACKET_REQUIRED_SECTIONS: &[&str] = &[
    "packet_id",
    "subject",
    "challenge",
    "lenses",
    "negative_controls",
    "old_paths",
    "obligations",
    "roles",
    "lifecycle",
];

const SUBJECT_KEYS: &[&str] =
    &["repository", "programme", "owning_issue", "builder_packet", "live_observation", "changed"];
const REPOSITORY_KEYS: &[&str] = &["name", "base", "head", "tree", "diff"];
const PROGRAMME_KEYS: &[&str] = &["name", "stage", "proposition", "profile"];
const BUILDER_PACKET_KEYS: &[&str] = &["contract", "digest"];
const LIVE_OBSERVATION_KEYS: &[&str] =
    &["candidate_state", "digest", "candidate_identity", "collision_state"];
const CHANGED_KEYS: &[&str] = &["authorities", "evidence", "migrated_seams"];
const AUTHORITY_ENTRY_KEYS: &[&str] = &["ref", "subject"];
const EVIDENCE_ENTRY_KEYS: &[&str] = &["kind", "identity"];
const CHALLENGE_KEYS: &[&str] = &["primary_proposition", "falsifiers", "stage_questions"];
const FALSIFIER_KEYS: &[&str] = &["id", "stage", "statement"];
const QUESTION_KEYS: &[&str] = &["id", "question"];
const LENS_KEYS: &[&str] = &["lens", "applicability", "reason", "refinements"];
const REFINEMENT_KEYS: &[&str] = &["id", "statement"];
const NEGATIVE_CONTROL_KEYS: &[&str] = &["falsifier_id", "checks"];
const CHECK_RESULT_KEYS: &[&str] = &["status", "evidence"];
const OLD_PATH_KEYS: &[&str] = &["seam", "disposition", "owner", "exit"];
const OBLIGATION_ENTRY_KEYS: &[&str] = &["ref", "identity"];
const ROLE_KEYS: &[&str] = &["role", "required", "obligation"];
const LIFECYCLE_KEYS: &[&str] = &["graceful_cleanup_claimed", "force_path_excluded", "evidence"];

const FINDING_ROOT_KEYS: &[&str] = &[
    "schema",
    "schema_version",
    "finding_id",
    "packet_ref",
    "lens",
    "outcome",
    "severity",
    "evidence",
    "suggested_action",
    "builder_response",
    "resolution",
    "final_disposition",
    "metadata",
];
const PACKET_REF_KEYS: &[&str] = &["packet_id", "reviewed_head"];
const FINDING_EVIDENCE_KEYS: &[&str] = &["kind", "identity", "observation"];
const BUILDER_RESPONSE_KEYS: &[&str] = &["disposition", "statement"];
const RESOLUTION_KEYS: &[&str] = &["resolution_head", "evidence"];
const RESOLUTION_EVIDENCE_KEYS: &[&str] = &["kind", "identity"];

const CLOSURE_ROOT_KEYS: &[&str] = &[
    "schema",
    "schema_version",
    "projection_id",
    "reviewed",
    "currentness",
    "review_state",
    "controls",
    "external_stage",
    "lifecycle",
    "eligibility",
    "authorization",
    "merge_authorized",
    "metadata",
];
const CLOSURE_REVIEWED_KEYS: &[&str] = &[
    "packet_id",
    "packet_digest",
    "builder_packet_digest",
    "reviewed_head",
    "reviewed_base",
    "claim_ceiling",
];
const CLOSURE_CURRENTNESS_KEYS: &[&str] =
    &["head", "base", "tree", "packet_digest", "builder_packet_digest", "claim_ceiling"];
const CLOSURE_ROLE_KEYS: &[&str] = &["role", "required", "state", "reference", "reason"];
const CLOSURE_FINDING_KEYS: &[&str] = &["finding_id", "material", "outcome", "state"];
const CLOSURE_CONTROLS_KEYS: &[&str] = &[
    "negative_controls_load_bearing",
    "old_paths_dispositioned",
    "generated_outputs_current",
    "generated_identities",
];
const EXTERNAL_STAGE_KEYS: &[&str] = &["claimed", "observed", "evidence"];
const CLOSURE_LIFECYCLE_KEYS: &[&str] = &["graceful_cleanup_claimed", "force_path_excluded"];

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
/// preserves the full semantic document; wrappers change syntax only.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ReviewProjection {
    /// Canonical machine document (canonical JSON bytes).
    Machine,
    /// Phone-readable Markdown.
    Markdown,
    /// Compact agent prompt.
    Compact,
}

impl ReviewProjection {
    fn name(self) -> &'static str {
        match self {
            ReviewProjection::Machine => "machine",
            ReviewProjection::Markdown => "markdown",
            ReviewProjection::Compact => "compact",
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

fn echoes_proposition(question: &str, proposition: &str) -> bool {
    question.trim().eq_ignore_ascii_case(proposition.trim())
}

/// Recursively reject the closed mutable-state key family at any depth: a
/// durable review document never carries assignment, lease, reviewer-queue,
/// or liveness state.
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

fn require_bool(
    object: &Map<String, Value>,
    key: &str,
    code: &str,
    where_: &str,
    violations: &mut Vec<Violation>,
) -> Option<bool> {
    match object.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        _ => {
            violations.push(Violation::new(code, format!("{where_}: {key} must be a boolean")));
            None
        }
    }
}

fn criterion_failure_code(criterion: &str) -> Option<&'static str> {
    CRITERION_FAILURE_CODES.iter().find(|(name, _)| *name == criterion).map(|(_, code)| *code)
}

/// Validate one review document (packet, finding, or closure projection).
/// Returns every violation, deterministically ordered. An empty result means
/// the document satisfies the shared closed contract and may be rendered.
fn validate_document(doc: &Value) -> Vec<Violation> {
    let Some(root) = as_str_map(doc) else {
        return vec![Violation::new("not_an_object", "document must be a JSON object".to_string())];
    };
    let mut violations = Vec::new();
    match string_field(root, "schema") {
        Some(PACKET_SCHEMA_NAME) => validate_packet(root, doc, &mut violations),
        Some(FINDING_SCHEMA_NAME) => validate_finding(root, doc, &mut violations),
        Some(CLOSURE_SCHEMA_NAME) => validate_closure(root, doc, &mut violations),
        Some(other) => violations.push(Violation::new(
            "wrong_schema",
            format!(
                "schema must be one of {PACKET_SCHEMA_NAME}, {FINDING_SCHEMA_NAME}, {CLOSURE_SCHEMA_NAME}; got {other}"
            ),
        )),
        None => violations.push(Violation::new(
            "wrong_schema",
            format!(
                "schema must be one of {PACKET_SCHEMA_NAME}, {FINDING_SCHEMA_NAME}, {CLOSURE_SCHEMA_NAME}"
            ),
        )),
    }
    violations
}

fn check_common_shape(
    root: &Map<String, Value>,
    doc: &Value,
    root_keys: &[&str],
    id_field: &str,
    required_sections: &[&str],
    violations: &mut Vec<Violation>,
) {
    if root.get("schema_version").and_then(Value::as_i64) != Some(1) {
        violations
            .push(Violation::new("wrong_schema_version", "schema_version must be 1".to_string()));
    }
    require_non_empty(root, id_field, "missing_document_id", "document", violations);
    check_unknown_keys(root, root_keys, "document", violations);
    scan_mutable_state(doc, "document", violations);
    for section in required_sections {
        match root.get(*section) {
            None => violations.push(Violation::new(
                "missing_required_input",
                format!("document: required input {section} was not supplied"),
            )),
            // Required sections are objects or arrays; a scalar or null must
            // fail closed instead of silently skipping validation, except the
            // document id itself which is the one scalar required input.
            Some(value)
                if *section != id_field
                    && as_str_map(value).is_none()
                    && value.as_array().is_none() =>
            {
                violations.push(Violation::new(
                    "malformed_section",
                    format!("document: required input {section} must be an object or array"),
                ))
            }
            Some(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// agent_review_packet.v1
// ---------------------------------------------------------------------------

fn validate_packet(root: &Map<String, Value>, doc: &Value, violations: &mut Vec<Violation>) {
    check_common_shape(
        root,
        doc,
        PACKET_ROOT_KEYS,
        "packet_id",
        PACKET_REQUIRED_SECTIONS,
        violations,
    );
    validate_packet_subject(root, violations);
    validate_packet_challenge(root, violations);
    validate_packet_lenses(root, violations);
    validate_packet_negative_controls(root, violations);
    validate_packet_old_paths(root, violations);
    validate_packet_obligations(root, violations);
    validate_packet_roles(root, violations);
    validate_packet_lifecycle(root, violations);
}

fn validate_packet_subject(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(subject) = root.get("subject").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(subject, SUBJECT_KEYS, "subject", violations);
    require_non_empty(subject, "owning_issue", "missing_owning_issue", "subject", violations);
    // Every exact-subject binding is required: an omitted child section
    // fails closed instead of silently skipping its validation (PR #12220
    // review finding).
    for child in ["repository", "programme", "builder_packet", "changed"] {
        if subject.get(child).is_none() {
            violations.push(Violation::new(
                "missing_subject_binding",
                format!("subject: required binding {child} was not supplied"),
            ));
        }
    }

    if let Some(repository) = subject.get("repository").and_then(as_str_map) {
        check_unknown_keys(repository, REPOSITORY_KEYS, "subject.repository", violations);
        for (key, code) in [
            ("name", "missing_repository_identity"),
            ("base", "missing_repository_identity"),
            ("head", "missing_repository_identity"),
            ("tree", "missing_repository_identity"),
        ] {
            require_non_empty(repository, key, code, "subject.repository", violations);
        }
        // The exact diff identity is load-bearing: a packet without one
        // cannot bind the reviewed change (#10881 negative control 1).
        require_non_empty(
            repository,
            "diff",
            "missing_diff_identity",
            "subject.repository",
            violations,
        );
    }
    if let Some(programme) = subject.get("programme").and_then(as_str_map) {
        check_unknown_keys(programme, PROGRAMME_KEYS, "subject.programme", violations);
        for (key, code) in [
            ("name", "missing_programme_identity"),
            ("stage", "missing_programme_identity"),
            ("proposition", "missing_programme_identity"),
            ("profile", "missing_programme_identity"),
        ] {
            require_non_empty(programme, key, code, "subject.programme", violations);
        }
    }
    if let Some(builder) = subject.get("builder_packet").and_then(as_str_map) {
        check_unknown_keys(builder, BUILDER_PACKET_KEYS, "subject.builder_packet", violations);
        require_non_empty(
            builder,
            "contract",
            "missing_builder_binding",
            "subject.builder_packet",
            violations,
        );
        require_non_empty(
            builder,
            "digest",
            "missing_builder_binding",
            "subject.builder_packet",
            violations,
        );
    }
    // Optional live observation: absence of knowledge is never fabricated as
    // vacancy and observation is never invented.
    if let Some(live) = subject.get("live_observation").and_then(as_str_map) {
        check_unknown_keys(live, LIVE_OBSERVATION_KEYS, "subject.live_observation", violations);
        require_non_empty(
            live,
            "digest",
            "missing_observation_digest",
            "subject.live_observation",
            violations,
        );
        match string_field(live, "candidate_state") {
            Some(state) if CANDIDATE_STATES.contains(&state) => {
                if state == "observed" {
                    require_non_empty(
                        live,
                        "candidate_identity",
                        "incomplete_observation",
                        "subject.live_observation",
                        violations,
                    );
                } else if live.contains_key("candidate_identity")
                    || live.contains_key("collision_state")
                {
                    violations.push(Violation::new(
                        "fabricated_observation",
                        "subject.live_observation: not_observed must not fabricate candidate or collision facts"
                            .to_string(),
                    ));
                }
            }
            Some(state) => violations.push(Violation::new(
                "unknown_candidate_state",
                format!("subject.live_observation: unknown candidate state {state}"),
            )),
            None => violations.push(Violation::new(
                "missing_candidate_state",
                "subject.live_observation: candidate_state is required".to_string(),
            )),
        }
    }
    if let Some(changed) = subject.get("changed").and_then(as_str_map) {
        check_unknown_keys(changed, CHANGED_KEYS, "subject.changed", violations);
        for entry in object_array(changed, "authorities") {
            let where_ = "subject.changed.authorities";
            check_unknown_keys(entry, AUTHORITY_ENTRY_KEYS, where_, violations);
            require_non_empty(entry, "ref", "malformed_changed_authority", where_, violations);
            require_non_empty(entry, "subject", "malformed_changed_authority", where_, violations);
        }
        let evidence = object_array(changed, "evidence");
        if evidence.is_empty() {
            violations.push(Violation::new(
                "missing_evidence_identity",
                "subject.changed: at least one current proof/receipt identity is required"
                    .to_string(),
            ));
        }
        for entry in &evidence {
            let where_ = "subject.changed.evidence";
            check_unknown_keys(entry, EVIDENCE_ENTRY_KEYS, where_, violations);
            require_non_empty(entry, "kind", "malformed_evidence_identity", where_, violations);
            require_non_empty(entry, "identity", "malformed_evidence_identity", where_, violations);
        }
    }
}

fn validate_packet_challenge(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(challenge) = root.get("challenge").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(challenge, CHALLENGE_KEYS, "challenge", violations);
    let proposition = require_non_empty(
        challenge,
        "primary_proposition",
        "missing_primary_proposition",
        "challenge",
        violations,
    );

    let falsifiers = object_array(challenge, "falsifiers");
    if falsifiers.is_empty() {
        violations.push(Violation::new(
            "missing_falsifier",
            "challenge: at least one stage-specific falsifier is required".to_string(),
        ));
    }
    let mut falsifier_ids: BTreeSet<String> = BTreeSet::new();
    for falsifier in &falsifiers {
        check_unknown_keys(falsifier, FALSIFIER_KEYS, "challenge.falsifiers", violations);
        if let Some(id) = require_non_empty(
            falsifier,
            "id",
            "missing_falsifier_id",
            "challenge.falsifiers",
            violations,
        ) && !falsifier_ids.insert(id.clone())
        {
            violations.push(Violation::new(
                "duplicate_falsifier_id",
                format!("challenge.falsifiers: duplicate id {id}"),
            ));
        }
        require_non_empty(
            falsifier,
            "stage",
            "missing_falsifier_stage",
            "challenge.falsifiers",
            violations,
        );
        match string_field(falsifier, "statement") {
            Some(statement) if is_generic_statement(statement) => violations.push(Violation::new(
                "generic_falsifier",
                format!("challenge.falsifiers: generic statement {statement:?} is not a falsifier"),
            )),
            Some(_) => {}
            None => violations.push(Violation::new(
                "missing_falsifier_statement",
                "challenge.falsifiers: statement is required".to_string(),
            )),
        }
    }

    let questions = object_array(challenge, "stage_questions");
    if questions.is_empty() {
        violations.push(Violation::new(
            "missing_stage_question",
            "challenge: at least one stage-specific falsification question is required".to_string(),
        ));
    }
    let mut question_ids: BTreeSet<String> = BTreeSet::new();
    let proposition_text = proposition.unwrap_or_default();
    for question in &questions {
        check_unknown_keys(question, QUESTION_KEYS, "challenge.stage_questions", violations);
        if let Some(id) = require_non_empty(
            question,
            "id",
            "missing_question_id",
            "challenge.stage_questions",
            violations,
        ) && !question_ids.insert(id.clone())
        {
            violations.push(Violation::new(
                "duplicate_question_id",
                format!("challenge.stage_questions: duplicate id {id}"),
            ));
        }
        match string_field(question, "question") {
            Some(text) => {
                // A question that repeats the builder claim, or a generic
                // statement, is not a challenge (#10881 negative control 3).
                if echoes_proposition(text, &proposition_text) || is_generic_statement(text) {
                    violations.push(Violation::new(
                        "generic_challenge",
                        format!(
                            "challenge.stage_questions: {text:?} repeats the builder claim or a generic statement instead of falsifying it"
                        ),
                    ));
                }
            }
            None => violations.push(Violation::new(
                "missing_question_text",
                "challenge.stage_questions: question is required".to_string(),
            )),
        }
    }
}

fn validate_packet_lenses(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(lenses) = root.get("lenses").and_then(Value::as_array) else {
        return;
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for entry in lenses.iter().filter_map(as_str_map) {
        check_unknown_keys(entry, LENS_KEYS, "lenses", violations);
        let lens = string_field(entry, "lens");
        match lens {
            Some(name) if REVIEW_LENSES.contains(&name) => {
                if !seen.insert(name.to_string()) {
                    violations.push(Violation::new(
                        "duplicate_lens",
                        format!("lenses: base lens {name} declared twice"),
                    ));
                }
            }
            Some(name) => violations
                .push(Violation::new("unknown_lens", format!("lenses: unknown base lens {name}"))),
            None => violations
                .push(Violation::new("missing_lens", "lenses: lens is required".to_string())),
        }
        match string_field(entry, "applicability") {
            Some(applicability) if LENS_APPLICABILITIES.contains(&applicability) => {
                match applicability {
                    "not_applicable" => {
                        if string_field(entry, "reason").is_none() {
                            violations.push(Violation::new(
                                "lens_applicability_unjustified",
                                format!(
                                    "lenses: {lens:?} skipped as not_applicable without a reason"
                                ),
                            ));
                        }
                    }
                    _ => {
                        if entry.contains_key("reason") {
                            violations.push(Violation::new(
                                "ambiguous_lens_applicability",
                                format!("lenses: {lens:?} is required but carries a skip reason"),
                            ));
                        }
                    }
                }
            }
            Some(applicability) => violations.push(Violation::new(
                "unknown_lens_applicability",
                format!("lenses: unknown applicability {applicability}"),
            )),
            None => violations.push(Violation::new(
                "missing_lens_applicability",
                "lenses: applicability is required".to_string(),
            )),
        }
        let mut refinement_ids: BTreeSet<String> = BTreeSet::new();
        for refinement in object_array(entry, "refinements") {
            check_unknown_keys(refinement, REFINEMENT_KEYS, "lenses.refinements", violations);
            if let Some(id) = require_non_empty(
                refinement,
                "id",
                "missing_refinement_id",
                "lenses.refinements",
                violations,
            ) && !refinement_ids.insert(id.clone())
            {
                violations.push(Violation::new(
                    "duplicate_refinement_id",
                    format!("lenses.refinements: duplicate id {id}"),
                ));
            }
            require_non_empty(
                refinement,
                "statement",
                "missing_refinement_statement",
                "lenses.refinements",
                violations,
            );
        }
    }
    // A base lens may never be silently skipped (#10881 negative control 4).
    for base in REVIEW_LENSES {
        if !seen.contains(*base) {
            violations.push(Violation::new(
                "missing_required_lens",
                format!("lenses: base lens {base} is absent; applicability is explicit"),
            ));
        }
    }
}

fn validate_packet_negative_controls(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(controls) = root.get("negative_controls").and_then(Value::as_array) else {
        return;
    };
    let declared: BTreeSet<String> = root
        .get("challenge")
        .and_then(as_str_map)
        .map(|challenge| {
            object_array(challenge, "falsifiers")
                .iter()
                .filter_map(|falsifier| string_field(falsifier, "id"))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut audited: BTreeSet<String> = BTreeSet::new();
    for entry in controls.iter().filter_map(as_str_map) {
        check_unknown_keys(entry, NEGATIVE_CONTROL_KEYS, "negative_controls", violations);
        let falsifier_id = require_non_empty(
            entry,
            "falsifier_id",
            "missing_control_falsifier",
            "negative_controls",
            violations,
        );
        if let Some(id) = &falsifier_id {
            if !declared.contains(id) {
                violations.push(Violation::new(
                    "unknown_falsifier_reference",
                    format!("negative_controls: audit row names undeclared falsifier {id}"),
                ));
            }
            if !audited.insert(id.clone()) {
                violations.push(Violation::new(
                    "duplicate_control_row",
                    format!("negative_controls: falsifier {id} audited twice"),
                ));
            }
        }
        let Some(checks) = entry.get("checks").and_then(as_str_map) else {
            violations.push(Violation::new(
                "malformed_control_checks",
                "negative_controls: checks must be an object".to_string(),
            ));
            continue;
        };
        check_unknown_keys(checks, CHECK_CRITERIA, "negative_controls.checks", violations);
        for criterion in CHECK_CRITERIA {
            let Some(result) = checks.get(*criterion).and_then(as_str_map) else {
                violations.push(Violation::new(
                    "missing_check_criterion",
                    format!("negative_controls.checks: criterion {criterion} is required"),
                ));
                continue;
            };
            let where_ = format!("negative_controls.checks.{criterion}");
            check_unknown_keys(result, CHECK_RESULT_KEYS, &where_, violations);
            match string_field(result, "status") {
                Some(status) if CHECK_STATUSES.contains(&status) => {
                    if status == "established" {
                        require_non_empty(
                            result,
                            "evidence",
                            "control_evidence_missing",
                            &where_,
                            violations,
                        );
                    } else if let Some(code) = criterion_failure_code(criterion) {
                        violations.push(Violation::new(
                            code,
                            format!("{where_}: criterion honestly not established; record it as a finding, never a pass"),
                        ));
                    }
                }
                Some(status) => violations.push(Violation::new(
                    "unknown_check_status",
                    format!("{where_}: unknown status {status}"),
                )),
                None => violations.push(Violation::new(
                    "missing_check_status",
                    format!("{where_}: status is required"),
                )),
            }
        }
    }
    // Every declared falsifier retains a load-bearing audit row; a missing
    // row must fail closed instead of becoming a silent skip (#10881
    // negative control 10).
    for id in &declared {
        if !audited.contains(id) {
            violations.push(Violation::new(
                "missing_negative_control",
                format!("negative_controls: declared falsifier {id} has no audit row"),
            ));
        }
    }
}

fn validate_packet_old_paths(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(old_paths) = root.get("old_paths").and_then(Value::as_array) else {
        return;
    };
    let migrated: BTreeSet<String> = root
        .get("subject")
        .and_then(as_str_map)
        .and_then(|subject| subject.get("changed").and_then(as_str_map))
        .map(|changed| string_array(changed, "migrated_seams").into_iter().collect())
        .unwrap_or_default();
    let mut dispositioned: BTreeSet<String> = BTreeSet::new();
    for entry in old_paths.iter().filter_map(as_str_map) {
        check_unknown_keys(entry, OLD_PATH_KEYS, "old_paths", violations);
        let seam = require_non_empty(entry, "seam", "malformed_old_path", "old_paths", violations);
        let disposition = string_field(entry, "disposition");
        match disposition {
            Some(value) if OLD_PATH_DISPOSITIONS.contains(&value) => match value {
                "compatibility_projection"
                    if string_field(entry, "owner").is_none()
                        || string_field(entry, "exit").is_none() =>
                {
                    violations.push(Violation::new(
                        "compatibility_projection_unowned",
                        format!("old_paths: compatibility projection for {seam:?} needs an owner and an exit"),
                    ));
                }
                "still_live_independent" if string_field(entry, "owner").is_none() => {
                    violations.push(Violation::new(
                        "still_live_unowned",
                        format!("old_paths: still-live independent seam {seam:?} needs an owner"),
                    ));
                }
                _ => {}
            },
            Some(value) => violations.push(Violation::new(
                "unknown_old_path_disposition",
                format!("old_paths: unknown disposition {value}"),
            )),
            None => violations.push(Violation::new(
                "malformed_old_path",
                "old_paths: disposition is required".to_string(),
            )),
        }
        if let Some(seam) = &seam
            && !dispositioned.insert(seam.clone())
        {
            violations.push(Violation::new(
                "duplicate_old_path_seam",
                format!("old_paths: seam {seam} dispositioned twice"),
            ));
        }
    }
    // A migrated seam without a typed disposition is not convergence
    // (#10881 negative control 8).
    for seam in &migrated {
        if !dispositioned.contains(seam) {
            violations.push(Violation::new(
                "undispositioned_old_path",
                format!("old_paths: migrated seam {seam} has no typed disposition"),
            ));
        }
    }
}

fn validate_packet_obligations(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(obligations) = root.get("obligations").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(obligations, OBLIGATION_KINDS, "obligations", violations);
    for kind in OBLIGATION_KINDS {
        for entry in object_array(obligations, kind) {
            let where_ = format!("obligations.{kind}");
            check_unknown_keys(entry, OBLIGATION_ENTRY_KEYS, &where_, violations);
            require_non_empty(entry, "ref", "malformed_obligation", &where_, violations);
            // A declared-current obligation without a verifying identity is
            // an unverified claim (#10881 negative control 9).
            require_non_empty(entry, "identity", "unverified_obligation", &where_, violations);
        }
    }
    if object_array(obligations, "tests_mutations").is_empty() {
        violations.push(Violation::new(
            "missing_test_obligation",
            "obligations: at least one test/mutation obligation is required".to_string(),
        ));
    }
}

fn validate_packet_roles(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(roles) = root.get("roles").and_then(Value::as_array) else {
        return;
    };
    let mut required_roles: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for entry in roles.iter().filter_map(as_str_map) {
        check_unknown_keys(entry, ROLE_KEYS, "roles", violations);
        let role = string_field(entry, "role");
        match role {
            Some(name) if REVIEW_ROLES.contains(&name) => {
                if !seen.insert(name.to_string()) {
                    violations.push(Violation::new(
                        "duplicate_role",
                        format!("roles: role {name} declared twice"),
                    ));
                }
            }
            Some(name) => violations
                .push(Violation::new("unknown_role", format!("roles: unknown role {name}"))),
            None => violations
                .push(Violation::new("missing_role", "roles: role is required".to_string())),
        }
        let required =
            require_bool(entry, "required", "malformed_role_required", "roles", violations);
        require_non_empty(entry, "obligation", "missing_role_obligation", "roles", violations);
        if required == Some(true)
            && let Some(role) = role
        {
            required_roles.push(role.to_string());
        }
    }
    if required_roles.is_empty() {
        violations.push(Violation::new(
            "missing_required_role",
            "roles: the programme profile must require at least one review role".to_string(),
        ));
    } else if required_roles.iter().all(|role| role == "builder_self_review") {
        // Correlated builder/reviewer reads create false convergence
        // (#10881 negative control 3).
        violations.push(Violation::new(
            "sole_builder_review",
            "roles: builder self-review alone cannot be the only required review role".to_string(),
        ));
    }
}

fn validate_packet_lifecycle(root: &Map<String, Value>, violations: &mut Vec<Violation>) {
    let Some(lifecycle) = root.get("lifecycle").and_then(as_str_map) else {
        return;
    };
    check_unknown_keys(lifecycle, LIFECYCLE_KEYS, "lifecycle", violations);
    let claimed = require_bool(
        lifecycle,
        "graceful_cleanup_claimed",
        "missing_lifecycle_claim",
        "lifecycle",
        violations,
    );
    if claimed == Some(true) {
        match lifecycle.get("force_path_excluded").and_then(Value::as_bool) {
            Some(true) => {}
            _ => violations.push(Violation::new(
                // Force cleanup is never graceful lifecycle success
                // (#10881 negative control 14).
                "force_cleanup_as_success",
                "lifecycle: graceful cleanup claimed without discriminating the force path"
                    .to_string(),
            )),
        }
        require_non_empty(
            lifecycle,
            "evidence",
            "lifecycle_evidence_missing",
            "lifecycle",
            violations,
        );
    }
}

// ---------------------------------------------------------------------------
// agent_review_finding.v1
// ---------------------------------------------------------------------------

fn validate_finding(root: &Map<String, Value>, doc: &Value, violations: &mut Vec<Violation>) {
    check_common_shape(
        root,
        doc,
        FINDING_ROOT_KEYS,
        "finding_id",
        &["packet_ref", "evidence"],
        violations,
    );

    if let Some(reference) = root.get("packet_ref").and_then(as_str_map) {
        check_unknown_keys(reference, PACKET_REF_KEYS, "packet_ref", violations);
        require_non_empty(
            reference,
            "packet_id",
            "missing_packet_reference",
            "packet_ref",
            violations,
        );
        let reviewed_head = require_non_empty(
            reference,
            "reviewed_head",
            "missing_packet_reference",
            "packet_ref",
            violations,
        );
        // Resolution evidence must anchor to the same head the finding was
        // raised against (#10881 negative control 11).
        if let Some(resolution) = root.get("resolution").and_then(as_str_map) {
            check_unknown_keys(resolution, RESOLUTION_KEYS, "resolution", violations);
            if let (Some(head), Some(reviewed)) =
                (string_field(resolution, "resolution_head"), reviewed_head.as_deref())
                && head != reviewed
            {
                violations.push(Violation::new(
                    "stale_resolution_head",
                    format!(
                        "resolution: resolution head {head} is not the reviewed head {reviewed}"
                    ),
                ));
            }
            let evidence = object_array(resolution, "evidence");
            if evidence.is_empty() {
                violations.push(Violation::new(
                    "prose_resolution",
                    "resolution: closing a finding requires current-head evidence, not prose"
                        .to_string(),
                ));
            }
            for entry in &evidence {
                check_unknown_keys(
                    entry,
                    RESOLUTION_EVIDENCE_KEYS,
                    "resolution.evidence",
                    violations,
                );
                require_non_empty(
                    entry,
                    "kind",
                    "malformed_resolution_evidence",
                    "resolution.evidence",
                    violations,
                );
                require_non_empty(
                    entry,
                    "identity",
                    "malformed_resolution_evidence",
                    "resolution.evidence",
                    violations,
                );
            }
        }
    }

    match string_field(root, "lens") {
        Some(lens) if REVIEW_LENSES.contains(&lens) => {}
        Some(lens) => violations
            .push(Violation::new("unknown_lens", format!("finding: unknown review lens {lens}"))),
        None => {
            violations.push(Violation::new("missing_lens", "finding: lens is required".to_string()))
        }
    }

    let outcome = string_field(root, "outcome");
    match outcome {
        Some(value) if FINDING_OUTCOMES.contains(&value) => {
            // Severity is derived from the outcome; the composer's assertion
            // must match the derivation.
            let expected = derived_severity(value);
            match string_field(root, "severity") {
                Some(severity) if FINDING_SEVERITIES.contains(&severity) => {
                    if severity != expected {
                        violations.push(Violation::new(
                            "severity_outcome_mismatch",
                            format!(
                                "finding: outcome {value} derives severity {expected}, not {severity}"
                            ),
                        ));
                    }
                }
                Some(severity) => violations.push(Violation::new(
                    "unknown_severity",
                    format!("finding: unknown severity {severity}"),
                )),
                None => violations.push(Violation::new(
                    "missing_severity",
                    "finding: severity is required".to_string(),
                )),
            }
        }
        Some(value) => violations
            .push(Violation::new("unknown_outcome", format!("finding: unknown outcome {value}"))),
        None => violations
            .push(Violation::new("missing_outcome", "finding: outcome is required".to_string())),
    }

    let evidence = object_array(root, "evidence");
    if evidence.is_empty() {
        violations.push(Violation::new(
            "missing_finding_evidence",
            "finding: at least one supporting evidence identity is required".to_string(),
        ));
    }
    for entry in &evidence {
        check_unknown_keys(entry, FINDING_EVIDENCE_KEYS, "evidence", violations);
        require_non_empty(entry, "kind", "malformed_finding_evidence", "evidence", violations);
        require_non_empty(entry, "identity", "malformed_finding_evidence", "evidence", violations);
        require_non_empty(
            entry,
            "observation",
            "malformed_finding_evidence",
            "evidence",
            violations,
        );
    }
    require_non_empty(root, "suggested_action", "missing_suggested_action", "finding", violations);

    if let Some(response) = root.get("builder_response").and_then(as_str_map) {
        check_unknown_keys(response, BUILDER_RESPONSE_KEYS, "builder_response", violations);
        match string_field(response, "disposition") {
            Some(disposition) if BUILDER_RESPONSE_DISPOSITIONS.contains(&disposition) => {}
            Some(disposition) => violations.push(Violation::new(
                "unknown_response_disposition",
                format!("builder_response: unknown disposition {disposition}"),
            )),
            None => violations.push(Violation::new(
                "missing_response_disposition",
                "builder_response: disposition is required".to_string(),
            )),
        }
        require_non_empty(
            response,
            "statement",
            "missing_response_statement",
            "builder_response",
            violations,
        );
    }

    let final_disposition = string_field(root, "final_disposition");
    match final_disposition {
        Some(value) if FINAL_DISPOSITIONS.contains(&value) => {
            validate_finding_consistency(outcome, value, root, violations);
        }
        Some(value) => violations.push(Violation::new(
            "unknown_final_disposition",
            format!("finding: unknown final disposition {value}"),
        )),
        None => violations.push(Violation::new(
            "missing_final_disposition",
            "finding: final_disposition is required".to_string(),
        )),
    }
}

fn validate_finding_consistency(
    outcome: Option<&str>,
    final_disposition: &str,
    root: &Map<String, Value>,
    violations: &mut Vec<Violation>,
) {
    let has_resolution = root.get("resolution").and_then(as_str_map).is_some();
    let resolution_evidence_empty = root
        .get("resolution")
        .and_then(as_str_map)
        .map(|resolution| object_array(resolution, "evidence").is_empty())
        .unwrap_or(true);
    match (outcome, final_disposition) {
        // An outcome claiming current-head resolution must be terminally
        // resolved with evidence.
        (Some("resolved_current_head"), "resolved_on_current_head") => {
            if !has_resolution || resolution_evidence_empty {
                violations.push(Violation::new(
                    "prose_resolution",
                    "finding: outcome resolved_current_head requires current-head resolution evidence"
                        .to_string(),
                ));
            }
        }
        (Some("resolved_current_head"), other) => violations.push(Violation::new(
            "disposition_mismatch",
            format!(
                "finding: outcome resolved_current_head cannot carry final disposition {other}"
            ),
        )),
        // No finding closes nothing and never resolves.
        (Some("no_finding"), "closed_no_finding") => {
            if has_resolution {
                violations.push(Violation::new(
                    "fabricated_resolution",
                    "finding: a no_finding record must not fabricate resolution evidence"
                        .to_string(),
                ));
            }
        }
        (Some("no_finding"), other) => violations.push(Violation::new(
            "disposition_mismatch",
            format!("finding: outcome no_finding cannot carry final disposition {other}"),
        )),
        // Missing or failed instrument evidence never becomes a pass; only a
        // recorded re-run may supersede it (#10881 negative control 10).
        (Some("instrument_failure"), "open") => {}
        (Some("instrument_failure"), "superseded_by_rerun") => {
            if !has_resolution || resolution_evidence_empty {
                violations.push(Violation::new(
                    "instrument_failure_resolved",
                    "finding: an instrument failure needs re-run evidence before supersession"
                        .to_string(),
                ));
            }
        }
        (Some("instrument_failure"), other) => violations.push(Violation::new(
            "instrument_failure_resolved",
            format!(
                "finding: instrument failure cannot become {other}; only a re-run may supersede it"
            ),
        )),
        (Some("claim_narrowing_required"), "open")
        | (Some("claim_narrowing_required"), "claim_narrowed") => {}
        (Some("stale_or_wrong_subject"), "open")
        | (Some("stale_or_wrong_subject"), "withdrawn_stale_subject") => {}
        (Some("material_blocker"), "open")
        | (Some("material_blocker"), "resolved_on_current_head")
        | (Some("bounded_follow_up"), "open")
        | (Some("bounded_follow_up"), "resolved_on_current_head")
        | (Some("question_requires_evidence"), "open")
        | (Some("question_requires_evidence"), "resolved_on_current_head") => {}
        (Some(_), _) => violations.push(Violation::new(
            "disposition_mismatch",
            format!(
                "finding: outcome {outcome:?} cannot carry final disposition {final_disposition}"
            ),
        )),
        (None, _) => {}
    }
    // Any final disposition claiming current-head resolution — defect
    // classes included — must carry current-head evidence, not prose (PR
    // #12220 review finding).
    if final_disposition == "resolved_on_current_head"
        && (!has_resolution || resolution_evidence_empty)
    {
        violations.push(Violation::new(
            "prose_resolution",
            "finding: resolved_on_current_head requires current-head resolution evidence"
                .to_string(),
        ));
    }
}

// ---------------------------------------------------------------------------
// stage_closure_projection.v1
// ---------------------------------------------------------------------------

fn validate_closure(root: &Map<String, Value>, doc: &Value, violations: &mut Vec<Violation>) {
    check_common_shape(
        root,
        doc,
        CLOSURE_ROOT_KEYS,
        "projection_id",
        &["reviewed", "currentness", "review_state", "controls", "external_stage", "lifecycle"],
        violations,
    );

    let reviewed = root.get("reviewed").and_then(as_str_map);
    if let Some(reviewed) = reviewed {
        check_unknown_keys(reviewed, CLOSURE_REVIEWED_KEYS, "reviewed", violations);
        for (key, code) in [
            ("packet_id", "missing_reviewed_binding"),
            ("packet_digest", "missing_reviewed_binding"),
            ("builder_packet_digest", "missing_reviewed_binding"),
            ("reviewed_head", "missing_reviewed_binding"),
            ("reviewed_base", "missing_reviewed_binding"),
            ("claim_ceiling", "missing_reviewed_binding"),
        ] {
            require_non_empty(reviewed, key, code, "reviewed", violations);
        }
    }

    if let Some(currentness) = root.get("currentness").and_then(as_str_map) {
        check_unknown_keys(currentness, CLOSURE_CURRENTNESS_KEYS, "currentness", violations);
        for (key, code) in [
            ("head", "missing_current_observation"),
            ("base", "missing_current_observation"),
            ("tree", "missing_current_observation"),
            ("packet_digest", "missing_current_observation"),
            ("builder_packet_digest", "missing_current_observation"),
            ("claim_ceiling", "missing_current_observation"),
        ] {
            require_non_empty(currentness, key, code, "currentness", violations);
        }
        if let Some(reviewed) = reviewed {
            // A review of another head cannot satisfy a current closure
            // projection (#10881 negative control 1).
            if let (Some(head), Some(reviewed_head)) =
                (string_field(currentness, "head"), string_field(reviewed, "reviewed_head"))
                && head != reviewed_head
            {
                violations.push(Violation::new(
                    "stale_head_binding",
                    format!(
                        "currentness: current head {head} is not the reviewed head {reviewed_head}"
                    ),
                ));
            }
            // A changed builder contract without invalidation voids the
            // review (#10881 negative control 2).
            if let (Some(digest), Some(reviewed_digest)) = (
                string_field(currentness, "builder_packet_digest"),
                string_field(reviewed, "builder_packet_digest"),
            ) && digest != reviewed_digest
            {
                violations.push(Violation::new(
                    "builder_contract_changed",
                    format!(
                        "currentness: builder packet digest {digest} differs from the reviewed {reviewed_digest}"
                    ),
                ));
            }
            if let (Some(digest), Some(reviewed_digest)) = (
                string_field(currentness, "packet_digest"),
                string_field(reviewed, "packet_digest"),
            ) && digest != reviewed_digest
            {
                violations.push(Violation::new(
                    "review_packet_changed",
                    format!(
                        "currentness: review packet digest {digest} differs from the reviewed {reviewed_digest}"
                    ),
                ));
            }
            // A widened claim ceiling after review is claim widening
            // (#10881 negative control 12).
            if let (Some(ceiling), Some(reviewed_ceiling)) = (
                string_field(currentness, "claim_ceiling"),
                string_field(reviewed, "claim_ceiling"),
            ) && ceiling != reviewed_ceiling
            {
                violations.push(Violation::new(
                    "claim_widening",
                    format!(
                        "currentness: current claim ceiling {ceiling:?} differs from the reviewed {reviewed_ceiling:?}"
                    ),
                ));
            }
        }
    }

    let mut role_blockers = 0usize;
    let mut finding_blockers = 0usize;
    if let Some(review_state) = root.get("review_state").and_then(as_str_map) {
        check_unknown_keys(review_state, &["roles", "findings"], "review_state", violations);
        let roles = object_array(review_state, "roles");
        if roles.is_empty() {
            violations.push(Violation::new(
                "missing_review_roles",
                "review_state.roles: at least one review role is required".to_string(),
            ));
        }
        let mut seen_roles: BTreeSet<String> = BTreeSet::new();
        for entry in &roles {
            check_unknown_keys(entry, CLOSURE_ROLE_KEYS, "review_state.roles", violations);
            let role = string_field(entry, "role");
            match role {
                Some(name) if REVIEW_ROLES.contains(&name) => {
                    if !seen_roles.insert(name.to_string()) {
                        violations.push(Violation::new(
                            "duplicate_role",
                            format!("review_state.roles: role {name} appears twice"),
                        ));
                    }
                }
                Some(name) => violations.push(Violation::new(
                    "unknown_role",
                    format!("review_state.roles: unknown role {name}"),
                )),
                None => violations.push(Violation::new(
                    "missing_role",
                    "review_state.roles: role is required".to_string(),
                )),
            }
            let required = require_bool(
                entry,
                "required",
                "malformed_role_required",
                "review_state.roles",
                violations,
            );
            match string_field(entry, "state") {
                Some(state) if ROLE_STATES.contains(&state) => match state {
                    "terminal" if string_field(entry, "reference").is_none() => {
                        violations.push(Violation::new(
                            "missing_role_reference",
                            format!(
                                "review_state.roles: terminal role {role:?} must reference its individual review"
                            ),
                        ));
                    }
                    "not_applicable" if string_field(entry, "reason").is_none() => {
                        violations.push(Violation::new(
                            "role_not_applicable_unjustified",
                            format!("review_state.roles: role {role:?} skipped without a reason"),
                        ));
                    }
                    _ => {}
                },
                Some(state) => violations.push(Violation::new(
                    "unknown_role_state",
                    format!("review_state.roles: unknown state {state}"),
                )),
                None => violations.push(Violation::new(
                    "missing_role_state",
                    "review_state.roles: state is required".to_string(),
                )),
            }
            if required == Some(true) {
                match string_field(entry, "state") {
                    Some("terminal") => {}
                    // A programme-required role cannot be skipped with a
                    // reason: the profile that requires it owns that
                    // decision (PR #12220 review finding).
                    Some("not_applicable") => violations.push(Violation::new(
                        "required_role_not_applicable",
                        format!(
                            "review_state.roles: required role {role:?} cannot be not_applicable; fix the profile"
                        ),
                    )),
                    _ => role_blockers += 1,
                }
            }
        }
        let mut seen_findings: BTreeSet<String> = BTreeSet::new();
        for entry in object_array(review_state, "findings") {
            check_unknown_keys(entry, CLOSURE_FINDING_KEYS, "review_state.findings", violations);
            let finding_id = require_non_empty(
                entry,
                "finding_id",
                "malformed_finding_reference",
                "review_state.findings",
                violations,
            );
            if let Some(id) = &finding_id
                && !seen_findings.insert(id.clone())
            {
                violations.push(Violation::new(
                    "duplicate_finding_reference",
                    format!("review_state.findings: finding {id} referenced twice"),
                ));
            }
            require_bool(
                entry,
                "material",
                "malformed_finding_reference",
                "review_state.findings",
                violations,
            );
            let outcome = string_field(entry, "outcome");
            match outcome {
                Some(value) if FINDING_OUTCOMES.contains(&value) => {}
                Some(value) => violations.push(Violation::new(
                    "unknown_outcome",
                    format!("review_state.findings: unknown outcome {value}"),
                )),
                None => violations.push(Violation::new(
                    "missing_outcome",
                    "review_state.findings: outcome is required".to_string(),
                )),
            }
            match string_field(entry, "state") {
                Some(state) if CLOSURE_FINDING_STATES.contains(&state) => {
                    // An instrument failure never becomes a resolution inside
                    // a closure projection either (#10881 negative control 10).
                    if outcome == Some("instrument_failure") && state == "resolved_on_current_head"
                    {
                        violations.push(Violation::new(
                            "instrument_failure_resolved",
                            format!(
                                "review_state.findings: instrument failure {finding_id:?} cannot resolve; only a re-run may supersede it"
                            ),
                        ));
                    }
                    if state == "open"
                        && (entry.get("material").and_then(Value::as_bool) == Some(true)
                            || outcome == Some("instrument_failure"))
                    {
                        // Only open material or instrument-failure findings
                        // block eligibility; a nonmaterial bounded follow-up
                        // does not (PR #12220 review finding).
                        finding_blockers += 1;
                    }
                }
                Some(state) => violations.push(Violation::new(
                    "unknown_closure_finding_state",
                    format!("review_state.findings: unknown state {state}"),
                )),
                None => violations.push(Violation::new(
                    "missing_closure_finding_state",
                    "review_state.findings: state is required".to_string(),
                )),
            }
        }
    }

    if let Some(controls) = root.get("controls").and_then(as_str_map) {
        check_unknown_keys(controls, CLOSURE_CONTROLS_KEYS, "controls", violations);
        for (key, code) in [
            ("negative_controls_load_bearing", "control_not_load_bearing"),
            ("old_paths_dispositioned", "undispositioned_old_path"),
            ("generated_outputs_current", "stale_generated_output"),
        ] {
            match controls.get(key).and_then(Value::as_bool) {
                Some(true) => {}
                Some(false) => violations.push(Violation::new(
                    code,
                    format!(
                        "controls: {key} is false; the projection must be re-derived after repair"
                    ),
                )),
                None => violations.push(Violation::new(
                    "missing_control_fact",
                    format!("controls: {key} is required"),
                )),
            }
        }
        if controls.get("generated_outputs_current").and_then(Value::as_bool) == Some(true)
            && string_array(controls, "generated_identities").is_empty()
        {
            violations.push(Violation::new(
                "unverified_obligation",
                "controls: generated_outputs_current requires current identities".to_string(),
            ));
        }
    }

    if let Some(external) = root.get("external_stage").and_then(as_str_map) {
        check_unknown_keys(external, EXTERNAL_STAGE_KEYS, "external_stage", violations);
        let claimed = external.get("claimed").and_then(Value::as_bool);
        let observed = external.get("observed").and_then(Value::as_bool);
        if claimed.is_none() || observed.is_none() {
            violations.push(Violation::new(
                "missing_external_stage_facts",
                "external_stage: claimed and observed are required".to_string(),
            ));
        }
        // An external/public stage is never inferred from an internal merge
        // (#10881 negative control 13).
        if claimed == Some(true)
            && (observed != Some(true) || string_field(external, "evidence").is_none())
        {
            violations.push(Violation::new(
                "external_stage_inferred",
                "external_stage: an external stage is claimed without actual observation evidence"
                    .to_string(),
            ));
        }
    }

    if let Some(lifecycle) = root.get("lifecycle").and_then(as_str_map) {
        check_unknown_keys(lifecycle, CLOSURE_LIFECYCLE_KEYS, "lifecycle", violations);
        let claimed = lifecycle.get("graceful_cleanup_claimed").and_then(Value::as_bool);
        let excluded = lifecycle.get("force_path_excluded").and_then(Value::as_bool);
        if claimed.is_none() || excluded.is_none() {
            violations.push(Violation::new(
                "missing_lifecycle_claim",
                "lifecycle: graceful_cleanup_claimed and force_path_excluded are required"
                    .to_string(),
            ));
        } else if claimed == Some(true) && excluded != Some(true) {
            violations.push(Violation::new(
                "force_cleanup_as_success",
                "lifecycle: force cleanup is not graceful lifecycle success".to_string(),
            ));
        }
    }

    // Eligibility is deterministically derived from the supplied facts; the
    // composer's assertion must match the derivation.
    let derived = if role_blockers == 0 && finding_blockers == 0 {
        "closure_eligible"
    } else {
        "not_eligible"
    };
    match string_field(root, "eligibility") {
        Some(eligibility) if CLOSURE_ELIGIBILITIES.contains(&eligibility) => {
            if eligibility != derived {
                violations.push(Violation::new(
                    "eligibility_mismatch",
                    format!("eligibility: supplied facts derive {derived}, not {eligibility}"),
                ));
            }
        }
        Some(eligibility) => violations.push(Violation::new(
            "unknown_eligibility",
            format!("eligibility: unknown value {eligibility}"),
        )),
        None => violations.push(Violation::new(
            "missing_eligibility",
            "eligibility: advisory eligibility is required".to_string(),
        )),
    }

    // An advisory closure projection never authorizes merge (#10881
    // negative control 15).
    if string_field(root, "authorization") != Some(CLOSURE_AUTHORIZATION) {
        violations.push(Violation::new(
            "authorization_violation",
            format!("authorization must be {CLOSURE_AUTHORIZATION}"),
        ));
    }
    if root.get("merge_authorized").and_then(Value::as_bool) != Some(false) {
        violations.push(Violation::new(
            "merge_authority_claimed",
            "merge_authorized must be false: an advisory closure never authorizes merge"
                .to_string(),
        ));
    }
}

// ---------------------------------------------------------------------------
// Canonical semantics and projections
// ---------------------------------------------------------------------------

/// Canonical semantic value: strips non-semantic metadata and sorts every
/// order-insensitive array. With serde_json's default BTreeMap-backed
/// objects, key order is already canonical; only array order is normalized
/// here.
fn canonical_value(doc: &Value) -> Value {
    match doc {
        Value::Object(object) => {
            let mut canonical = Map::new();
            for (key, value) in object {
                if key == "metadata" {
                    continue;
                }
                canonical.insert(key.clone(), canonical_value(value));
            }
            Value::Object(canonical)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        other => other.clone(),
    }
}

/// Sort an array of objects by one of its string fields, preserving all
/// entries.
fn sort_by_field(items: &[Value], field: &str) -> Vec<Value> {
    let mut sorted: Vec<&Value> = items.iter().collect();
    sorted.sort_by_key(|item| {
        item.as_object()
            .and_then(|object| object.get(field))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    sorted.into_iter().cloned().collect()
}

fn sorted_string_value(items: &[Value]) -> Vec<Value> {
    let mut sorted: Vec<String> =
        items.iter().filter_map(Value::as_str).map(str::to_string).collect();
    sorted.sort();
    sorted.dedup();
    sorted.into_iter().map(Value::String).collect()
}

fn canonical_packet(doc: &Value) -> Value {
    let mut root = canonical_value(doc);
    let Some(object) = root.as_object_mut() else {
        return root;
    };
    if let Some(changed) = object
        .get_mut("subject")
        .and_then(Value::as_object_mut)
        .and_then(|subject| subject.get_mut("changed"))
        .and_then(Value::as_object_mut)
    {
        sort_object_array(changed, "authorities", "ref");
        sort_object_array(changed, "evidence", "identity");
        if let Some(seams) = changed.get_mut("migrated_seams").and_then(Value::as_array_mut) {
            *seams = sorted_string_value(seams);
        }
    }
    if let Some(challenge) = object.get_mut("challenge").and_then(Value::as_object_mut) {
        sort_object_array(challenge, "falsifiers", "id");
        sort_object_array(challenge, "stage_questions", "id");
    }
    if let Some(lenses) = object.get_mut("lenses").and_then(Value::as_array_mut) {
        *lenses = sort_by_field(lenses, "lens");
        for lens in lenses.iter_mut() {
            if let Some(refinements) = lens.get_mut("refinements").and_then(Value::as_array_mut) {
                *refinements = sort_by_field(refinements, "id");
            }
        }
    }
    if let Some(controls) = object.get_mut("negative_controls").and_then(Value::as_array_mut) {
        *controls = sort_by_field(controls, "falsifier_id");
    }
    if let Some(old_paths) = object.get_mut("old_paths").and_then(Value::as_array_mut) {
        *old_paths = sort_by_field(old_paths, "seam");
    }
    if let Some(obligations) = object.get_mut("obligations").and_then(Value::as_object_mut) {
        for kind in OBLIGATION_KINDS {
            if let Some(entries) = obligations.get_mut(*kind).and_then(Value::as_array_mut) {
                *entries = sort_by_field(entries, "ref");
            }
        }
    }
    if let Some(roles) = object.get_mut("roles").and_then(Value::as_array_mut) {
        *roles = sort_by_field(roles, "role");
    }
    root
}

fn canonical_finding(doc: &Value) -> Value {
    let mut root = canonical_value(doc);
    let Some(object) = root.as_object_mut() else {
        return root;
    };
    sort_object_array(object, "evidence", "identity");
    if let Some(resolution) = object.get_mut("resolution").and_then(Value::as_object_mut) {
        sort_object_array(resolution, "evidence", "identity");
    }
    root
}

fn canonical_closure(doc: &Value) -> Value {
    let mut root = canonical_value(doc);
    let Some(object) = root.as_object_mut() else {
        return root;
    };
    if let Some(review_state) = object.get_mut("review_state").and_then(Value::as_object_mut) {
        sort_object_array(review_state, "roles", "role");
        sort_object_array(review_state, "findings", "finding_id");
    }
    if let Some(identities) = object
        .get_mut("controls")
        .and_then(Value::as_object_mut)
        .and_then(|controls| controls.get_mut("generated_identities"))
        .and_then(Value::as_array_mut)
    {
        *identities = sorted_string_value(identities);
    }
    root
}

fn sort_object_array(object: &mut Map<String, Value>, key: &str, field: &str) {
    if let Some(items) = object.get_mut(key).and_then(Value::as_array_mut) {
        *items = sort_by_field(items, field);
    }
}

fn canonical_form(doc: &Value) -> String {
    let schema = doc.get("schema").and_then(Value::as_str).unwrap_or("");
    let canonical = match schema {
        PACKET_SCHEMA_NAME => canonical_packet(doc),
        FINDING_SCHEMA_NAME => canonical_finding(doc),
        CLOSURE_SCHEMA_NAME => canonical_closure(doc),
        _ => canonical_value(doc),
    };
    canonical.to_string()
}

/// Anchors that every projection must preserve verbatim. A rendering that
/// drops any of them has dropped document semantics, not just syntax.
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
    match string_field(root, "schema") {
        Some(PACKET_SCHEMA_NAME) => {
            push(string_field(root, "packet_id"));
            if let Some(subject) = root.get("subject").and_then(as_str_map) {
                if let Some(repository) = subject.get("repository").and_then(as_str_map) {
                    for key in ["base", "head", "diff"] {
                        push(string_field(repository, key));
                    }
                }
                if let Some(programme) = subject.get("programme").and_then(as_str_map) {
                    for key in ["stage", "proposition", "profile"] {
                        push(string_field(programme, key));
                    }
                }
                push(string_field(subject, "owning_issue"));
                if let Some(builder) = subject.get("builder_packet").and_then(as_str_map) {
                    push(string_field(builder, "digest"));
                }
                if let Some(changed) = subject.get("changed").and_then(as_str_map) {
                    for entry in object_array(changed, "evidence") {
                        push(string_field(entry, "identity"));
                    }
                }
            }
            if let Some(challenge) = root.get("challenge").and_then(as_str_map) {
                push(string_field(challenge, "primary_proposition"));
                for falsifier in object_array(challenge, "falsifiers") {
                    push(string_field(falsifier, "id"));
                    push(string_field(falsifier, "statement"));
                }
                for question in object_array(challenge, "stage_questions") {
                    push(string_field(question, "id"));
                    push(string_field(question, "question"));
                }
            }
            for lens in root.get("lenses").and_then(Value::as_array).unwrap_or(&Vec::new()) {
                if let Some(lens) = as_str_map(lens) {
                    push(string_field(lens, "lens"));
                }
            }
            for entry in
                root.get("negative_controls").and_then(Value::as_array).unwrap_or(&Vec::new())
            {
                if let Some(entry) = as_str_map(entry) {
                    push(string_field(entry, "falsifier_id"));
                    // The audit evidence itself is semantic: a projection
                    // that asserts load-bearing controls without their
                    // evidence has dropped the challenge surface (PR #12220
                    // review finding).
                    if let Some(checks) = entry.get("checks").and_then(as_str_map) {
                        for criterion in CHECK_CRITERIA {
                            if let Some(result) = checks.get(*criterion).and_then(as_str_map) {
                                push(string_field(result, "evidence"));
                            }
                        }
                    }
                }
            }
            for entry in root.get("old_paths").and_then(Value::as_array).unwrap_or(&Vec::new()) {
                if let Some(entry) = as_str_map(entry) {
                    push(string_field(entry, "seam"));
                    push(string_field(entry, "disposition"));
                }
            }
            if let Some(obligations) = root.get("obligations").and_then(as_str_map) {
                for kind in OBLIGATION_KINDS {
                    for entry in object_array(obligations, kind) {
                        push(string_field(entry, "ref"));
                    }
                }
            }
            for entry in root.get("roles").and_then(Value::as_array).unwrap_or(&Vec::new()) {
                if let Some(entry) = as_str_map(entry) {
                    push(string_field(entry, "role"));
                    push(string_field(entry, "obligation"));
                }
            }
            // The seed questions are shared semantics: no rendering may
            // drop them.
            for (_, text) in SEED_QUESTIONS {
                anchors.push((*text).to_string());
            }
        }
        Some(FINDING_SCHEMA_NAME) => {
            push(string_field(root, "finding_id"));
            if let Some(reference) = root.get("packet_ref").and_then(as_str_map) {
                push(string_field(reference, "packet_id"));
                push(string_field(reference, "reviewed_head"));
            }
            push(string_field(root, "lens"));
            push(string_field(root, "outcome"));
            push(string_field(root, "suggested_action"));
            push(string_field(root, "final_disposition"));
            for entry in object_array(root, "evidence") {
                push(string_field(entry, "identity"));
            }
            if let Some(resolution) = root.get("resolution").and_then(as_str_map) {
                push(string_field(resolution, "resolution_head"));
                for entry in object_array(resolution, "evidence") {
                    push(string_field(entry, "identity"));
                }
            }
        }
        Some(CLOSURE_SCHEMA_NAME) => {
            push(string_field(root, "projection_id"));
            if let Some(reviewed) = root.get("reviewed").and_then(as_str_map) {
                push(string_field(reviewed, "packet_id"));
                push(string_field(reviewed, "reviewed_head"));
            }
            if let Some(currentness) = root.get("currentness").and_then(as_str_map) {
                push(string_field(currentness, "head"));
            }
            if let Some(review_state) = root.get("review_state").and_then(as_str_map) {
                for entry in object_array(review_state, "roles") {
                    push(string_field(entry, "role"));
                    push(string_field(entry, "state"));
                }
                for entry in object_array(review_state, "findings") {
                    push(string_field(entry, "finding_id"));
                }
            }
            push(string_field(root, "eligibility"));
            anchors.push(CLOSURE_AUTHORIZATION.to_string());
        }
        _ => {
            push(string_field(root, "schema"));
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
/// must validate first; rendering an invalid document fails closed.
fn render_machine(doc: &Value) -> String {
    canonical_form(doc)
}

fn section_str<'a>(root: &'a Map<String, Value>, section: &str, key: &str) -> &'a str {
    root.get(section)
        .and_then(as_str_map)
        .and_then(|object| string_field(object, key))
        .unwrap_or("")
}

fn render_packet_markdown(root: &Map<String, Value>) -> String {
    let mut out = String::new();
    out.push_str("# Review packet: ");
    out.push_str(nested_str(root, &["subject", "programme"], "stage"));
    out.push_str("\n\n## Review subject\n\n");
    out.push_str(&format!("- packet: {}\n", string_field(root, "packet_id").unwrap_or("")));
    out.push_str(&format!(
        "- repository: {} base {} head {} (diff {})\n",
        nested_str(root, &["subject", "repository"], "name"),
        nested_str(root, &["subject", "repository"], "base"),
        nested_str(root, &["subject", "repository"], "head"),
        nested_str(root, &["subject", "repository"], "diff")
    ));
    out.push_str(&format!(
        "- programme: {} / stage {} / proposition {} / profile {}\n",
        nested_str(root, &["subject", "programme"], "name"),
        nested_str(root, &["subject", "programme"], "stage"),
        nested_str(root, &["subject", "programme"], "proposition"),
        nested_str(root, &["subject", "programme"], "profile")
    ));
    out.push_str(&format!("- owning issue: {}\n", nested_str(root, &["subject"], "owning_issue")));
    out.push_str(&format!(
        "- builder packet: {} ({})\n",
        nested_str(root, &["subject", "builder_packet"], "digest"),
        nested_str(root, &["subject", "builder_packet"], "contract")
    ));
    match root
        .get("subject")
        .and_then(as_str_map)
        .and_then(|subject| subject.get("live_observation").and_then(as_str_map))
    {
        Some(live) => {
            out.push_str(&format!(
                "- candidate state: {} ({})\n",
                string_field(live, "candidate_state").unwrap_or(""),
                string_field(live, "digest").unwrap_or("")
            ));
            if let Some(identity) = string_field(live, "candidate_identity") {
                out.push_str(&format!("  - candidate: {identity}\n"));
            }
            if let Some(collision) = string_field(live, "collision_state") {
                out.push_str(&format!("  - collision: {collision}\n"));
            }
        }
        None => out.push_str("- candidate state: not_observed (no live observation supplied)\n"),
    }
    if let Some(changed) = root
        .get("subject")
        .and_then(as_str_map)
        .and_then(|subject| subject.get("changed").and_then(as_str_map))
    {
        for entry in object_array(changed, "authorities") {
            out.push_str(&format!(
                "- changed authority: {} — {}\n",
                string_field(entry, "ref").unwrap_or(""),
                string_field(entry, "subject").unwrap_or("")
            ));
        }
        for entry in object_array(changed, "evidence") {
            out.push_str(&format!(
                "- evidence: {} ({})\n",
                string_field(entry, "identity").unwrap_or(""),
                string_field(entry, "kind").unwrap_or("")
            ));
        }
    }

    out.push_str("\n## Stage falsifiers under audit\n\n");
    if let Some(challenge) = root.get("challenge").and_then(as_str_map) {
        for falsifier in object_array(challenge, "falsifiers") {
            out.push_str(&format!(
                "- falsifier {} [{}]: {}\n",
                string_field(falsifier, "id").unwrap_or(""),
                string_field(falsifier, "stage").unwrap_or(""),
                string_field(falsifier, "statement").unwrap_or("")
            ));
        }
    }
    out.push_str("\n## Primary proposition and falsification questions\n\n");
    if let Some(challenge) = root.get("challenge").and_then(as_str_map) {
        out.push_str(&format!(
            "Primary proposition: {}\n\n",
            string_field(challenge, "primary_proposition").unwrap_or("")
        ));
        for question in object_array(challenge, "stage_questions") {
            out.push_str(&format!(
                "- stage question {}: {}\n",
                string_field(question, "id").unwrap_or(""),
                string_field(question, "question").unwrap_or("")
            ));
        }
    }
    out.push_str("\nShared seed questions (immutable):\n\n");
    for (id, text) in SEED_QUESTIONS {
        out.push_str(&format!("- {id}: {text}\n"));
    }

    out.push_str("\n## Review lenses\n\n");
    for lens in root.get("lenses").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(lens) = as_str_map(lens) else { continue };
        match string_field(lens, "applicability") {
            Some("not_applicable") => out.push_str(&format!(
                "- [not_applicable] {} — {}\n",
                string_field(lens, "lens").unwrap_or(""),
                string_field(lens, "reason").unwrap_or("")
            )),
            _ => {
                out.push_str(&format!(
                    "- [required] {}\n",
                    string_field(lens, "lens").unwrap_or("")
                ));
                for refinement in object_array(lens, "refinements") {
                    out.push_str(&format!(
                        "  - refinement {}: {}\n",
                        string_field(refinement, "id").unwrap_or(""),
                        string_field(refinement, "statement").unwrap_or("")
                    ));
                }
            }
        }
    }

    out.push_str("\n## Negative-control audit\n\n");
    for entry in root.get("negative_controls").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(entry) = as_str_map(entry) else { continue };
        out.push_str(&format!(
            "- falsifier {}:\n",
            string_field(entry, "falsifier_id").unwrap_or("")
        ));
        if let Some(checks) = entry.get("checks").and_then(as_str_map) {
            for criterion in CHECK_CRITERIA {
                if let Some(result) = checks.get(*criterion).and_then(as_str_map) {
                    out.push_str(&format!(
                        "  - {}: {} ({})\n",
                        criterion,
                        string_field(result, "status").unwrap_or(""),
                        string_field(result, "evidence").unwrap_or("")
                    ));
                }
            }
        }
    }

    out.push_str("\n## Old-path disposition\n\n");
    let old_paths =
        root.get("old_paths").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    if old_paths.is_empty() {
        out.push_str("- no migrated or replaced seams declared\n");
    }
    for entry in old_paths {
        let Some(entry) = as_str_map(entry) else { continue };
        out.push_str(&format!(
            "- seam {}: {}\n",
            string_field(entry, "seam").unwrap_or(""),
            string_field(entry, "disposition").unwrap_or("")
        ));
        if let Some(owner) = string_field(entry, "owner") {
            out.push_str(&format!("  - owner: {owner}\n"));
        }
        if let Some(exit) = string_field(entry, "exit") {
            out.push_str(&format!("  - exit: {exit}\n"));
        }
    }

    out.push_str("\n## Spec/test/docs/generated obligations\n\n");
    if let Some(obligations) = root.get("obligations").and_then(as_str_map) {
        for kind in OBLIGATION_KINDS {
            for entry in object_array(obligations, kind) {
                out.push_str(&format!(
                    "- {}: {} ({})\n",
                    kind,
                    string_field(entry, "ref").unwrap_or(""),
                    string_field(entry, "identity").unwrap_or("")
                ));
            }
        }
    }

    out.push_str("\n## Review roles\n\n");
    for entry in root.get("roles").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(entry) = as_str_map(entry) else { continue };
        let required = if entry.get("required").and_then(Value::as_bool) == Some(true) {
            "required"
        } else {
            "optional"
        };
        out.push_str(&format!(
            "- [{required}] {}: {}\n",
            string_field(entry, "role").unwrap_or(""),
            string_field(entry, "obligation").unwrap_or("")
        ));
    }

    out.push_str("\n## Lifecycle discrimination\n\n");
    if let Some(lifecycle) = root.get("lifecycle").and_then(as_str_map) {
        let claimed = lifecycle.get("graceful_cleanup_claimed").and_then(Value::as_bool);
        out.push_str(&format!(
            "- graceful cleanup claimed: {}\n",
            claimed.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_string())
        ));
        if claimed == Some(true) {
            out.push_str(&format!(
                "- force path excluded: {}\n",
                lifecycle.get("force_path_excluded").and_then(Value::as_bool).unwrap_or(false)
            ));
            out.push_str(&format!(
                "- evidence: {}\n",
                string_field(lifecycle, "evidence").unwrap_or("")
            ));
        }
    }
    out
}

fn render_finding_markdown(root: &Map<String, Value>) -> String {
    let mut out = String::new();
    out.push_str("# Review finding: ");
    out.push_str(string_field(root, "finding_id").unwrap_or(""));
    out.push_str("\n\n## Finding identity\n\n");
    out.push_str(&format!(
        "- packet: {} @ head {}\n",
        section_str(root, "packet_ref", "packet_id"),
        section_str(root, "packet_ref", "reviewed_head")
    ));
    out.push_str(&format!(
        "- lens: {}\n- outcome: {} (severity {})\n- final disposition: {}\n",
        string_field(root, "lens").unwrap_or(""),
        string_field(root, "outcome").unwrap_or(""),
        string_field(root, "severity").unwrap_or(""),
        string_field(root, "final_disposition").unwrap_or("")
    ));
    out.push_str("\n## Supporting evidence\n\n");
    for entry in object_array(root, "evidence") {
        out.push_str(&format!(
            "- {} ({}): {}\n",
            string_field(entry, "identity").unwrap_or(""),
            string_field(entry, "kind").unwrap_or(""),
            string_field(entry, "observation").unwrap_or("")
        ));
    }
    out.push_str("\n## Suggested action\n\n");
    out.push_str(string_field(root, "suggested_action").unwrap_or(""));
    out.push('\n');
    if let Some(response) = root.get("builder_response").and_then(as_str_map) {
        out.push_str("\n## Builder response\n\n");
        out.push_str(&format!(
            "- {}: {}\n",
            string_field(response, "disposition").unwrap_or(""),
            string_field(response, "statement").unwrap_or("")
        ));
    }
    if let Some(resolution) = root.get("resolution").and_then(as_str_map) {
        out.push_str("\n## Current-head resolution\n\n");
        out.push_str(&format!(
            "- resolution head: {}\n",
            string_field(resolution, "resolution_head").unwrap_or("")
        ));
        for entry in object_array(resolution, "evidence") {
            out.push_str(&format!(
                "- evidence: {} ({})\n",
                string_field(entry, "identity").unwrap_or(""),
                string_field(entry, "kind").unwrap_or("")
            ));
        }
    }
    out
}

fn render_closure_markdown(root: &Map<String, Value>) -> String {
    let mut out = String::new();
    out.push_str("# Stage closure projection: ");
    out.push_str(string_field(root, "projection_id").unwrap_or(""));
    out.push_str("\n\n## Reviewed versus current\n\n");
    out.push_str(&format!(
        "- reviewed: packet {} head {} (builder digest {})\n",
        section_str(root, "reviewed", "packet_id"),
        section_str(root, "reviewed", "reviewed_head"),
        section_str(root, "reviewed", "builder_packet_digest")
    ));
    out.push_str(&format!(
        "- current: head {} (builder digest {})\n",
        section_str(root, "currentness", "head"),
        section_str(root, "currentness", "builder_packet_digest")
    ));
    out.push_str(&format!(
        "- reviewed claim ceiling: {}\n- current claim ceiling: {}\n",
        section_str(root, "reviewed", "claim_ceiling"),
        section_str(root, "currentness", "claim_ceiling")
    ));
    out.push_str("\n## Review roles\n\n");
    if let Some(review_state) = root.get("review_state").and_then(as_str_map) {
        for entry in object_array(review_state, "roles") {
            let required = if entry.get("required").and_then(Value::as_bool) == Some(true) {
                "required"
            } else {
                "optional"
            };
            out.push_str(&format!(
                "- [{required}] {}: {}",
                string_field(entry, "role").unwrap_or(""),
                string_field(entry, "state").unwrap_or("")
            ));
            if let Some(reference) = string_field(entry, "reference") {
                out.push_str(&format!(" ({reference})"));
            }
            if let Some(reason) = string_field(entry, "reason") {
                out.push_str(&format!(" — {reason}"));
            }
            out.push('\n');
        }
        out.push_str("\n## Findings\n\n");
        let findings = object_array(review_state, "findings");
        if findings.is_empty() {
            out.push_str("- no findings recorded\n");
        }
        for entry in findings {
            out.push_str(&format!(
                "- {}: outcome {} (material {}): {}\n",
                string_field(entry, "finding_id").unwrap_or(""),
                string_field(entry, "outcome").unwrap_or(""),
                entry.get("material").and_then(Value::as_bool).unwrap_or(false),
                string_field(entry, "state").unwrap_or("")
            ));
        }
    }
    out.push_str("\n## Closure facts\n\n");
    if let Some(controls) = root.get("controls").and_then(as_str_map) {
        for key in [
            "negative_controls_load_bearing",
            "old_paths_dispositioned",
            "generated_outputs_current",
        ] {
            out.push_str(&format!(
                "- {}: {}\n",
                key,
                controls
                    .get(key)
                    .and_then(Value::as_bool)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "missing".to_string())
            ));
        }
        for identity in string_array(controls, "generated_identities") {
            out.push_str(&format!("- generated identity: {identity}\n"));
        }
    }
    if let Some(external) = root.get("external_stage").and_then(as_str_map) {
        out.push_str(&format!(
            "- external stage: claimed {} / observed {}\n",
            external.get("claimed").and_then(Value::as_bool).unwrap_or(false),
            external.get("observed").and_then(Value::as_bool).unwrap_or(false)
        ));
    }
    if let Some(lifecycle) = root.get("lifecycle").and_then(as_str_map) {
        out.push_str(&format!(
            "- lifecycle: graceful claimed {} / force path excluded {}\n",
            lifecycle.get("graceful_cleanup_claimed").and_then(Value::as_bool).unwrap_or(false),
            lifecycle.get("force_path_excluded").and_then(Value::as_bool).unwrap_or(false)
        ));
    }
    out.push_str(&format!(
        "\n## Derived eligibility\n\n{} (authorization: advisory_only — never merge authorization)\n",
        string_field(root, "eligibility").unwrap_or("")
    ));
    out
}

fn render_markdown(doc: &Value) -> String {
    let Some(root) = as_str_map(doc) else {
        return String::new();
    };
    match string_field(root, "schema") {
        Some(PACKET_SCHEMA_NAME) => render_packet_markdown(root),
        Some(FINDING_SCHEMA_NAME) => render_finding_markdown(root),
        Some(CLOSURE_SCHEMA_NAME) => render_closure_markdown(root),
        _ => String::new(),
    }
}

/// Render the compact agent prompt. Compact means fewer decorations, never
/// fewer semantics: every anchor of the Markdown projection is preserved.
fn render_packet_compact(root: &Map<String, Value>) -> String {
    let mut out = String::new();
    out.push_str("REVIEW PACKET ");
    out.push_str(nested_str(root, &["subject", "programme"], "stage"));
    out.push_str(&format!(
        " | {}/{}@{} diff {}\n",
        nested_str(root, &["subject", "repository"], "name"),
        nested_str(root, &["subject", "repository"], "base"),
        nested_str(root, &["subject", "repository"], "head"),
        nested_str(root, &["subject", "repository"], "diff")
    ));
    out.push_str(&format!(
        "PROP: {}\n",
        root.get("challenge")
            .and_then(as_str_map)
            .and_then(|challenge| string_field(challenge, "primary_proposition"))
            .unwrap_or("")
    ));
    out.push_str(&format!("PACKET-ID: {}\n", string_field(root, "packet_id").unwrap_or("")));
    out.push_str(&format!(
        "ISSUE: {} | PROPOSITION: {} | PROFILE: {} | BUILDER: {} ({})\n",
        nested_str(root, &["subject"], "owning_issue"),
        nested_str(root, &["subject", "programme"], "proposition"),
        nested_str(root, &["subject", "programme"], "profile"),
        nested_str(root, &["subject", "builder_packet"], "digest"),
        nested_str(root, &["subject", "builder_packet"], "contract")
    ));
    match root
        .get("subject")
        .and_then(as_str_map)
        .and_then(|subject| subject.get("live_observation").and_then(as_str_map))
    {
        Some(live) => {
            out.push_str(&format!(
                "CANDIDATE: {} ({})\n",
                string_field(live, "candidate_state").unwrap_or(""),
                string_field(live, "digest").unwrap_or("")
            ));
            if let Some(identity) = string_field(live, "candidate_identity") {
                out.push_str(&format!("CANDIDATE-ID: {identity}\n"));
            }
        }
        None => out.push_str("CANDIDATE: not_observed\n"),
    }
    if let Some(changed) = root
        .get("subject")
        .and_then(as_str_map)
        .and_then(|subject| subject.get("changed").and_then(as_str_map))
    {
        out.push_str("CHANGED-AUTHORITY:\n");
        for entry in object_array(changed, "authorities") {
            out.push_str(&format!(
                "  {}: {}\n",
                string_field(entry, "ref").unwrap_or(""),
                string_field(entry, "subject").unwrap_or("")
            ));
        }
        out.push_str("EVIDENCE:\n");
        for entry in object_array(changed, "evidence") {
            out.push_str(&format!(
                "  {} [{}]\n",
                string_field(entry, "identity").unwrap_or(""),
                string_field(entry, "kind").unwrap_or("")
            ));
        }
    }
    if let Some(challenge) = root.get("challenge").and_then(as_str_map) {
        for falsifier in object_array(challenge, "falsifiers") {
            out.push_str(&format!(
                "FALSIFIER {}: {}\n",
                string_field(falsifier, "id").unwrap_or(""),
                string_field(falsifier, "statement").unwrap_or("")
            ));
        }
    }
    out.push_str("STAGE-QUESTIONS:\n");
    if let Some(challenge) = root.get("challenge").and_then(as_str_map) {
        for question in object_array(challenge, "stage_questions") {
            out.push_str(&format!(
                "  {}: {}\n",
                string_field(question, "id").unwrap_or(""),
                string_field(question, "question").unwrap_or("")
            ));
        }
    }
    out.push_str("SEED-QUESTIONS (answer every one):\n");
    for (id, text) in SEED_QUESTIONS {
        out.push_str(&format!("  {id}: {text}\n"));
    }
    out.push_str("LENSES:\n");
    for lens in root.get("lenses").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(lens) = as_str_map(lens) else { continue };
        match string_field(lens, "applicability") {
            Some("not_applicable") => out.push_str(&format!(
                "  {} [not_applicable]: {}\n",
                string_field(lens, "lens").unwrap_or(""),
                string_field(lens, "reason").unwrap_or("")
            )),
            _ => {
                out.push_str(&format!(
                    "  {} [required]\n",
                    string_field(lens, "lens").unwrap_or("")
                ));
                for refinement in object_array(lens, "refinements") {
                    out.push_str(&format!(
                        "    refinement {}: {}\n",
                        string_field(refinement, "id").unwrap_or(""),
                        string_field(refinement, "statement").unwrap_or("")
                    ));
                }
            }
        }
    }
    out.push_str("OBLIGATIONS:\n");
    if let Some(obligations) = root.get("obligations").and_then(as_str_map) {
        for kind in OBLIGATION_KINDS {
            for entry in object_array(obligations, kind) {
                out.push_str(&format!(
                    "  {} [{}]: {}\n",
                    kind,
                    string_field(entry, "ref").unwrap_or(""),
                    string_field(entry, "identity").unwrap_or("")
                ));
            }
        }
    }
    out.push_str("OLD-PATHS:\n");
    let old_paths =
        root.get("old_paths").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    if old_paths.is_empty() {
        out.push_str("  none\n");
    }
    for entry in old_paths {
        let Some(entry) = as_str_map(entry) else { continue };
        out.push_str(&format!(
            "  {}: {}\n",
            string_field(entry, "seam").unwrap_or(""),
            string_field(entry, "disposition").unwrap_or("")
        ));
    }
    out.push_str("ROLES:\n");
    for entry in root.get("roles").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(entry) = as_str_map(entry) else { continue };
        let required = if entry.get("required").and_then(Value::as_bool) == Some(true) {
            "required"
        } else {
            "optional"
        };
        out.push_str(&format!(
            "  [{required}] {}: {}\n",
            string_field(entry, "role").unwrap_or(""),
            string_field(entry, "obligation").unwrap_or("")
        ));
    }
    out.push_str("NEGATIVE-CONTROLS (challenge any evidence you can break):\n");
    for entry in root.get("negative_controls").and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let Some(entry) = as_str_map(entry) else { continue };
        out.push_str(&format!("  {}:\n", string_field(entry, "falsifier_id").unwrap_or("")));
        if let Some(checks) = entry.get("checks").and_then(as_str_map) {
            for criterion in CHECK_CRITERIA {
                if let Some(result) = checks.get(*criterion).and_then(as_str_map) {
                    out.push_str(&format!(
                        "    {}: {} ({})\n",
                        criterion,
                        string_field(result, "status").unwrap_or(""),
                        string_field(result, "evidence").unwrap_or("")
                    ));
                }
            }
        }
    }
    out
}

fn render_finding_compact(root: &Map<String, Value>) -> String {
    let mut out = String::new();
    out.push_str("REVIEW FINDING ");
    out.push_str(string_field(root, "finding_id").unwrap_or(""));
    out.push_str(&format!(
        " | packet {}@{}\n",
        section_str(root, "packet_ref", "packet_id"),
        section_str(root, "packet_ref", "reviewed_head")
    ));
    out.push_str(&format!(
        "LENS: {} | OUTCOME: {} [{}] | FINAL: {}\n",
        string_field(root, "lens").unwrap_or(""),
        string_field(root, "outcome").unwrap_or(""),
        string_field(root, "severity").unwrap_or(""),
        string_field(root, "final_disposition").unwrap_or("")
    ));
    out.push_str("EVIDENCE:\n");
    for entry in object_array(root, "evidence") {
        out.push_str(&format!(
            "  {} [{}]: {}\n",
            string_field(entry, "identity").unwrap_or(""),
            string_field(entry, "kind").unwrap_or(""),
            string_field(entry, "observation").unwrap_or("")
        ));
    }
    out.push_str(&format!("ACTION: {}\n", string_field(root, "suggested_action").unwrap_or("")));
    if let Some(resolution) = root.get("resolution").and_then(as_str_map) {
        out.push_str(&format!(
            "RESOLVED-AT: {}\n",
            string_field(resolution, "resolution_head").unwrap_or("")
        ));
        for entry in object_array(resolution, "evidence") {
            out.push_str(&format!(
                "RESOLUTION-EVIDENCE: {} [{}]\n",
                string_field(entry, "identity").unwrap_or(""),
                string_field(entry, "kind").unwrap_or("")
            ));
        }
    }
    out
}

fn render_closure_compact(root: &Map<String, Value>) -> String {
    let mut out = String::new();
    out.push_str("STAGE CLOSURE PROJECTION ");
    out.push_str(string_field(root, "projection_id").unwrap_or(""));
    out.push_str(&format!(
        " | packet {} reviewed {} current {}\n",
        section_str(root, "reviewed", "packet_id"),
        section_str(root, "reviewed", "reviewed_head"),
        section_str(root, "currentness", "head")
    ));
    out.push_str(&format!(
        "BUILDER-DIGEST: reviewed {} current {}\n",
        section_str(root, "reviewed", "builder_packet_digest"),
        section_str(root, "currentness", "builder_packet_digest")
    ));
    out.push_str("ROLES:\n");
    if let Some(review_state) = root.get("review_state").and_then(as_str_map) {
        for entry in object_array(review_state, "roles") {
            out.push_str(&format!(
                "  {}: {}\n",
                string_field(entry, "role").unwrap_or(""),
                string_field(entry, "state").unwrap_or("")
            ));
        }
        out.push_str("FINDINGS:\n");
        let findings = object_array(review_state, "findings");
        if findings.is_empty() {
            out.push_str("  none\n");
        }
        for entry in findings {
            out.push_str(&format!(
                "  {}: {} [{}]\n",
                string_field(entry, "finding_id").unwrap_or(""),
                string_field(entry, "state").unwrap_or(""),
                string_field(entry, "outcome").unwrap_or("")
            ));
        }
    }
    if let Some(controls) = root.get("controls").and_then(as_str_map) {
        out.push_str(&format!(
            "CONTROLS: load_bearing={} old_paths={} generated_current={}\n",
            controls
                .get("negative_controls_load_bearing")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            controls.get("old_paths_dispositioned").and_then(Value::as_bool).unwrap_or(false),
            controls.get("generated_outputs_current").and_then(Value::as_bool).unwrap_or(false)
        ));
    }
    out.push_str(&format!(
        "ELIGIBILITY: {} (advisory_only; merge_authorized=false)\n",
        string_field(root, "eligibility").unwrap_or("")
    ));
    out
}

fn render_compact(doc: &Value) -> String {
    let Some(root) = as_str_map(doc) else {
        return String::new();
    };
    match string_field(root, "schema") {
        Some(PACKET_SCHEMA_NAME) => render_packet_compact(root),
        Some(FINDING_SCHEMA_NAME) => render_finding_compact(root),
        Some(CLOSURE_SCHEMA_NAME) => render_closure_compact(root),
        _ => String::new(),
    }
}

/// Render one projection of a caller-supplied review document, fail-closed:
/// any validation violation aborts rendering instead of producing plausible
/// prose. Projections go to stdout only; no repository file is written.
pub fn render_to_string(doc: &Value, projection: ReviewProjection) -> Result<String> {
    let violations = validate_document(doc);
    if !violations.is_empty() {
        let codes: Vec<&str> = violations.iter().map(|violation| violation.code.as_str()).collect();
        bail!(
            "review document failed validation; refusing to render plausible prose ({}): {:?}",
            codes.len(),
            codes
        );
    }
    Ok(match projection {
        ReviewProjection::Machine => render_machine(doc),
        ReviewProjection::Markdown => render_markdown(doc),
        ReviewProjection::Compact => render_compact(doc),
    })
}

// ---------------------------------------------------------------------------
// Schema pinning
// ---------------------------------------------------------------------------

fn validate_schema_enums(root: &Path) -> Result<Vec<String>> {
    let mut violations = Vec::new();
    let expected: &[(&str, &[&str], &[&str])] = &[
        (PACKET_SCHEMA_PATH, &["$defs", "review_lens", "enum"], REVIEW_LENSES),
        (PACKET_SCHEMA_PATH, &["$defs", "lens_applicability", "enum"], LENS_APPLICABILITIES),
        (PACKET_SCHEMA_PATH, &["$defs", "review_role", "enum"], REVIEW_ROLES),
        (PACKET_SCHEMA_PATH, &["$defs", "old_path_disposition", "enum"], OLD_PATH_DISPOSITIONS),
        (PACKET_SCHEMA_PATH, &["$defs", "obligation_kind", "enum"], OBLIGATION_KINDS),
        (PACKET_SCHEMA_PATH, &["$defs", "check_criterion", "enum"], CHECK_CRITERIA),
        (PACKET_SCHEMA_PATH, &["$defs", "check_status", "enum"], CHECK_STATUSES),
        (FINDING_SCHEMA_PATH, &["$defs", "review_lens", "enum"], REVIEW_LENSES),
        (FINDING_SCHEMA_PATH, &["$defs", "finding_outcome", "enum"], FINDING_OUTCOMES),
        (FINDING_SCHEMA_PATH, &["$defs", "finding_severity", "enum"], FINDING_SEVERITIES),
        (
            FINDING_SCHEMA_PATH,
            &["$defs", "builder_response_disposition", "enum"],
            BUILDER_RESPONSE_DISPOSITIONS,
        ),
        (FINDING_SCHEMA_PATH, &["$defs", "final_disposition", "enum"], FINAL_DISPOSITIONS),
        (CLOSURE_SCHEMA_PATH, &["$defs", "review_role", "enum"], REVIEW_ROLES),
        (CLOSURE_SCHEMA_PATH, &["$defs", "role_state", "enum"], ROLE_STATES),
        (CLOSURE_SCHEMA_PATH, &["$defs", "finding_outcome", "enum"], FINDING_OUTCOMES),
        (CLOSURE_SCHEMA_PATH, &["$defs", "closure_finding_state", "enum"], CLOSURE_FINDING_STATES),
        (CLOSURE_SCHEMA_PATH, &["$defs", "closure_eligibility", "enum"], CLOSURE_ELIGIBILITIES),
    ];
    for (schema_path, defs_path, expected_values) in expected {
        let schema = load_schema(root, schema_path)?;
        let label = format!("{schema_path}:{}", defs_path.join("."));
        let mut value = &schema;
        let mut ok = true;
        for segment in defs_path.iter() {
            match value.get(*segment) {
                Some(child) => value = child,
                None => {
                    violations.push(format!(
                        "{label}: schema is missing the enum at {}",
                        defs_path.join(".")
                    ));
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        match value.as_array() {
            Some(items) => {
                let values: Vec<&str> = items.iter().filter_map(Value::as_str).collect::<Vec<_>>();
                if !values.iter().copied().eq(expected_values.iter().copied()) {
                    violations.push(format!(
                        "{label}: enum drifted from the pinned closed vocabulary: {values:?}"
                    ));
                }
            }
            None => {
                violations.push(format!("{label}: expected an enum at {}", defs_path.join(".")))
            }
        }
    }
    for (path, id) in [
        (PACKET_SCHEMA_PATH, PACKET_SCHEMA_ID),
        (FINDING_SCHEMA_PATH, FINDING_SCHEMA_ID),
        (CLOSURE_SCHEMA_PATH, CLOSURE_SCHEMA_ID),
    ] {
        let schema = load_schema(root, path)?;
        if schema.get("$id").and_then(Value::as_str) != Some(id) {
            violations.push(format!("{path}: $id must be {id}"));
        }
    }
    // The packet schema must structurally enforce the one obligation the
    // validator derives: at least one test/mutation entry. A description
    // alone is not enforcement (PR #12220 review finding).
    let packet_schema = load_schema(root, PACKET_SCHEMA_PATH)?;
    if packet_schema
        .pointer("/$defs/obligations/properties/tests_mutations/minItems")
        .and_then(Value::as_i64)
        != Some(1)
    {
        violations.push(format!(
            "{PACKET_SCHEMA_PATH}: obligations.tests_mutations must carry minItems 1"
        ));
    }
    Ok(violations)
}

fn load_schema(root: &Path, path: &str) -> Result<Value> {
    let file = root.join(path);
    let text =
        fs::read_to_string(&file).with_context(|| format!("failed to read {}", file.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", file.display()))
}

fn load_json(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn violation_codes(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(|violation| violation.code.as_str()).collect()
}

fn golden_path(stem: &str, projection: ReviewProjection) -> String {
    match projection {
        ReviewProjection::Machine => format!("{GOLDEN_DIR}/{stem}.machine.json"),
        ReviewProjection::Markdown => format!("{GOLDEN_DIR}/{stem}.markdown.md"),
        ReviewProjection::Compact => format!("{GOLDEN_DIR}/{stem}.compact.txt"),
    }
}

/// Entry point: validate the closed contract schemas, the valid fixtures,
/// the fail-closed negative controls, the canonical-semantics control, and
/// the deterministic golden projections. `update_golden` rewrites the golden
/// vectors (an explicit writer action, never live review state).
pub fn run(update_golden: bool) -> Result<()> {
    let root = project_root()?;
    let mut failures: Vec<String> = Vec::new();

    for violation in validate_schema_enums(&root)? {
        failures.push(format!("schema: {violation}"));
    }

    let fixture_dir = root.join(FIXTURE_DIR);
    for name in VALID_FIXTURES {
        let doc = load_json(&fixture_dir.join(name))?;
        let violations = validate_document(&doc);
        if !violations.is_empty() {
            failures.push(format!(
                "{name}: expected a valid document, got {:?}",
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
        // Projection completeness: no rendering drops document semantics.
        for (label, rendering) in [("markdown", &markdown), ("compact", &compact)] {
            for violation in projection_completeness_violations(&doc, rendering, label) {
                failures.push(format!("{name}: {violation:?}"));
            }
        }
        let rendered = [
            (ReviewProjection::Machine, machine),
            (ReviewProjection::Markdown, markdown),
            (ReviewProjection::Compact, compact),
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

    // Canonical semantics: shuffled input (and varied non-semantic metadata)
    // produces identical canonical bytes.
    let base = load_json(&fixture_dir.join("challenger_service_marker.v1.json"))?;
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
        // Fail-closed rendering: an invalid document never renders prose.
        if let Ok(rendered) = render_to_string(&doc, ReviewProjection::Compact) {
            failures.push(format!(
                "invalid/{file}: invalid document rendered a projection ({})",
                rendered.len()
            ));
        }
    }

    if failures.is_empty() {
        if update_golden {
            println!("agent_review_packet.v1 family: golden vectors updated");
            Ok(())
        } else {
            println!(
                "agent_review_packet.v1 family: closed contracts, fixtures, canonical control, and deterministic projections all valid"
            );
            Ok(())
        }
    } else {
        bail!("agent review packet check failed:\n{}", failures.join("\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    fn fixture(name: &str) -> TestResult<Value> {
        load_json(&project_root()?.join(FIXTURE_DIR).join(name))
    }

    fn invalid_fixture(name: &str) -> TestResult<Value> {
        load_json(&project_root()?.join(FIXTURE_DIR).join("invalid").join(name))
    }

    fn has(doc: &Value, code: &str) -> bool {
        validate_document(doc).iter().any(|violation| violation.code == code)
    }

    fn valid(doc: &Value) -> bool {
        validate_document(doc).is_empty()
    }

    #[test]
    fn valid_fixtures_validate() -> TestResult {
        for name in VALID_FIXTURES {
            let doc = fixture(name)?;
            assert!(valid(&doc), "{name} must be valid");
        }
        Ok(())
    }

    // Negative control 1: a closure binds another head.
    #[test]
    fn closure_rejects_stale_head_binding() -> TestResult {
        let doc = invalid_fixture("stale_head_binding.json")?;
        assert!(has(&doc, "stale_head_binding"));
        Ok(())
    }

    // Negative control 2: builder contract changed without invalidation.
    #[test]
    fn closure_rejects_changed_builder_contract() -> TestResult {
        let doc = invalid_fixture("builder_contract_changed.json")?;
        assert!(has(&doc, "builder_contract_changed"));
        Ok(())
    }

    // Negative control 3: questions repeating the builder claim are not a
    // challenge, and builder self-review alone is not independent review.
    #[test]
    fn challenge_must_falsify_rather_than_echo() -> TestResult {
        let echo = invalid_fixture("generic_challenge.json")?;
        assert!(has(&echo, "generic_challenge"));
        let sole = invalid_fixture("sole_builder_review.json")?;
        assert!(has(&sole, "sole_builder_review"));
        // Repair: one independent required role restores validity.
        let mut repaired = sole;
        if let Some(roles) = repaired
            .as_object_mut()
            .and_then(|root| root.get_mut("roles"))
            .and_then(Value::as_array_mut)
        {
            roles.push(serde_json::json!({
                "role": "adversarial_challenger",
                "required": true,
                "obligation": "An independent challenger attempts the named escapes."
            }));
        }
        assert!(!has(&repaired, "sole_builder_review"));
        Ok(())
    }

    // Negative control 4: a required lens cannot be silently skipped.
    #[test]
    fn required_lenses_cannot_disappear() -> TestResult {
        let missing = invalid_fixture("missing_required_lens.json")?;
        assert!(has(&missing, "missing_required_lens"));
        let unjustified = invalid_fixture("unjustified_lens_skip.json")?;
        assert!(has(&unjustified, "lens_applicability_unjustified"));
        Ok(())
    }

    // Negative controls 5/6/7: negative controls must be load-bearing.
    #[test]
    fn negative_controls_must_be_load_bearing() -> TestResult {
        let circular = invalid_fixture("circular_expectation.json")?;
        assert!(has(&circular, "circular_expectation"));
        let mutation = invalid_fixture("control_not_load_bearing.json")?;
        assert!(has(&mutation, "control_not_load_bearing"));
        let alternate = invalid_fixture("weak_discriminator.json")?;
        assert!(has(&alternate, "weak_discriminator"));
        Ok(())
    }

    // Negative controls 8/9: old paths and generated obligations fail closed.
    #[test]
    fn old_paths_and_obligations_fail_closed() -> TestResult {
        let undispositioned = invalid_fixture("undispositioned_old_path.json")?;
        assert!(has(&undispositioned, "undispositioned_old_path"));
        let stale = invalid_fixture("stale_generated_output.json")?;
        assert!(has(&stale, "stale_generated_output"));
        let unverified = invalid_fixture("unverified_obligation.json")?;
        assert!(has(&unverified, "unverified_obligation"));
        let unowned = invalid_fixture("compatibility_projection_unowned.json")?;
        assert!(has(&unowned, "compatibility_projection_unowned"));
        Ok(())
    }

    #[test]
    fn old_path_disposition_fields_are_independently_required() -> TestResult {
        let cases = [
            ("compatibility_projection", true, true, None),
            ("compatibility_projection", false, true, Some("compatibility_projection_unowned")),
            ("compatibility_projection", true, false, Some("compatibility_projection_unowned")),
            ("compatibility_projection", false, false, Some("compatibility_projection_unowned")),
            ("still_live_independent", true, false, None),
            ("still_live_independent", false, false, Some("still_live_unowned")),
        ];

        for (disposition, keep_owner, keep_exit, expected_violation) in cases {
            let mut doc = fixture("consumer_issue_controller_t07_shape.v1.json")?;
            let old_paths = doc
                .as_object_mut()
                .and_then(|root| root.get_mut("old_paths"))
                .and_then(Value::as_array_mut)
                .ok_or_else(|| color_eyre::eyre::eyre!("fixture old_paths must be an array"))?;
            let row = old_paths
                .iter_mut()
                .find(|row| {
                    row.get("disposition").and_then(Value::as_str)
                        == Some("compatibility_projection")
                })
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!(
                        "fixture must contain the compatibility_projection old-path row"
                    )
                })?;
            row.insert("disposition".to_string(), Value::String(disposition.to_string()));
            if !keep_owner {
                row.remove("owner");
            }
            if !keep_exit {
                row.remove("exit");
            }

            match expected_violation {
                Some(code) => color_eyre::eyre::ensure!(
                    has(&doc, code),
                    "{disposition} owner={keep_owner} exit={keep_exit} must report {code}"
                ),
                None => color_eyre::eyre::ensure!(
                    valid(&doc),
                    "{disposition} owner={keep_owner} exit={keep_exit} must remain valid: {:?}",
                    validate_document(&doc)
                ),
            }
        }
        Ok(())
    }

    // Negative control 10: missing audit rows and swallowed instrument
    // failures never become a pass.
    #[test]
    fn missing_and_failed_instruments_never_pass() -> TestResult {
        let missing = invalid_fixture("missing_negative_control.json")?;
        assert!(has(&missing, "missing_negative_control"));
        let swallowed = invalid_fixture("instrument_failure_resolved.json")?;
        assert!(has(&swallowed, "instrument_failure_resolved"));
        // The honest path: an instrument failure stays open.
        let mut honest = swallowed;
        if let Some(root) = honest.as_object_mut() {
            root.insert("final_disposition".to_string(), Value::String("open".to_string()));
        }
        assert!(!has(&honest, "instrument_failure_resolved"));
        Ok(())
    }

    // Negative control 11: findings close on current-head evidence only.
    #[test]
    fn findings_resolve_on_evidence_not_prose() -> TestResult {
        let prose = invalid_fixture("prose_resolution.json")?;
        assert!(has(&prose, "prose_resolution"));
        Ok(())
    }

    // Negative controls 12/13/14/15: closure boundaries fail closed.
    #[test]
    fn closure_boundaries_fail_closed() -> TestResult {
        let widened = invalid_fixture("claim_widening.json")?;
        assert!(has(&widened, "claim_widening"));
        let external = invalid_fixture("external_stage_inferred.json")?;
        assert!(has(&external, "external_stage_inferred"));
        let force = invalid_fixture("force_cleanup_as_success.json")?;
        assert!(has(&force, "force_cleanup_as_success"));
        let merge = invalid_fixture("merge_authority_claimed.json")?;
        assert!(has(&merge, "merge_authority_claimed"));
        Ok(())
    }

    // Eligibility is derived, never asserted.
    #[test]
    fn eligibility_is_derived() -> TestResult {
        let mismatch = invalid_fixture("eligible_with_open_finding.json")?;
        assert!(has(&mismatch, "eligibility_mismatch"));
        // The honest not-eligible projection over the same facts is valid.
        let honest = fixture("closure_open_finding.v1.json")?;
        assert!(valid(&honest));
        Ok(())
    }

    // Negative control 17: mutable live-state never embeds.
    #[test]
    fn mutable_state_never_embeds() -> TestResult {
        let doc = invalid_fixture("mutable_review_state.json")?;
        assert!(has(&doc, "mutable_state_embedded"));
        // Even inside non-semantic metadata.
        let mut with_metadata = fixture("challenger_service_marker.v1.json")?;
        if let Some(root) = with_metadata.as_object_mut() {
            root.insert("metadata".to_string(), serde_json::json!({"assignment": "agent-7"}));
        }
        assert!(has(&with_metadata, "mutable_state_embedded"));
        Ok(())
    }

    // Missing required input fails generation fail-closed.
    #[test]
    fn missing_input_fails_closed() -> TestResult {
        let missing = invalid_fixture("missing_required_input.json")?;
        assert!(has(&missing, "missing_required_input"));
        let evidence = invalid_fixture("missing_evidence_identity.json")?;
        assert!(has(&evidence, "missing_evidence_identity"));
        let diff = invalid_fixture("missing_diff_identity.json")?;
        assert!(has(&diff, "missing_diff_identity"));
        Ok(())
    }

    // Input order never changes canonical bytes, and metadata is excluded
    // from canonical identity.
    #[test]
    fn canonical_bytes_ignore_input_order_and_metadata() -> TestResult {
        let base = fixture("challenger_service_marker.v1.json")?;
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

    // The seed falsification questions cannot disappear from a rendering.
    #[test]
    fn seed_questions_survive_every_projection() -> TestResult {
        let doc = fixture("challenger_service_marker.v1.json")?;
        for (label, rendering) in
            [("markdown", render_markdown(&doc)), ("compact", render_compact(&doc))]
        {
            for (id, text) in SEED_QUESTIONS {
                assert!(rendering.contains(id), "{label} dropped seed question id {id}");
                assert!(rendering.contains(text), "{label} dropped seed question {text}");
            }
        }
        Ok(())
    }

    // A rendering that drops semantics fails the completeness check.
    #[test]
    fn dropped_semantics_in_projection_fail() -> TestResult {
        let doc = fixture("challenger_service_marker.v1.json")?;
        let markdown = render_markdown(&doc);
        assert!(projection_completeness_violations(&doc, &markdown, "markdown").is_empty());
        let proposition = doc["challenge"]["primary_proposition"].as_str().unwrap_or("");
        let tampered = markdown.replace(proposition, "");
        let violations = projection_completeness_violations(&doc, &tampered, "markdown");
        assert!(
            violations.iter().any(|violation| violation.code == "projection_dropped_semantics"),
            "a rendering without the primary proposition must fail completeness"
        );
        Ok(())
    }

    // The T07 consumer shape: programme refinements only, no second schema.
    #[test]
    fn issue_controller_consumer_shape_validates() -> TestResult {
        let doc = fixture("consumer_issue_controller_t07_shape.v1.json")?;
        assert!(valid(&doc));
        let compact = render_compact(&doc);
        assert!(compact.contains("#11775"));
        assert!(compact.contains("CANDIDATE: not_observed"));
        // Programme refinements survive the projection.
        assert!(compact.contains("no_second_packet_schema"));
        Ok(())
    }

    // The render path is fail-closed: an invalid document never renders
    // plausible prose.
    #[test]
    fn render_refuses_invalid_documents() -> TestResult {
        let doc = invalid_fixture("merge_authority_claimed.json")?;
        assert!(render_to_string(&doc, ReviewProjection::Machine).is_err());
        Ok(())
    }

    // All three projections of every valid fixture preserve every semantic
    // anchor.
    #[test]
    fn all_projections_preserve_semantics() -> TestResult {
        for name in VALID_FIXTURES {
            let doc = fixture(name)?;
            // The machine projection is the canonical document itself; the
            // renderer-added seed questions are checked on the prose
            // projections, which must carry them.
            for (label, rendering) in
                [("markdown", render_markdown(&doc)), ("compact", render_compact(&doc))]
            {
                assert!(
                    projection_completeness_violations(&doc, &rendering, label).is_empty(),
                    "{name} {label} projection dropped semantics"
                );
            }
            let anchors = semantic_anchors(&doc);
            for anchor in &anchors {
                if !matches!(doc.get("schema").and_then(Value::as_str), Some(PACKET_SCHEMA_NAME))
                    || !SEED_QUESTIONS.iter().any(|(_, text)| text == anchor)
                {
                    assert!(
                        render_machine(&doc).contains(anchor.as_str()),
                        "{name} machine projection dropped document anchor {anchor:?}"
                    );
                }
            }
        }
        Ok(())
    }

    // Severity is a deterministic function of the outcome.
    #[test]
    fn severity_is_derived_from_outcome() -> TestResult {
        for (outcome, severity) in [
            ("material_blocker", "material"),
            ("stale_or_wrong_subject", "material"),
            ("bounded_follow_up", "bounded"),
            ("question_requires_evidence", "bounded"),
            ("claim_narrowing_required", "bounded"),
            ("instrument_failure", "advisory"),
            ("no_finding", "advisory"),
            ("resolved_current_head", "advisory"),
        ] {
            assert_eq!(derived_severity(outcome), severity);
        }
        let mut doc = fixture("finding_stale_marker_resolved.v1.json")?;
        if let Some(root) = doc.as_object_mut() {
            root.insert("severity".to_string(), Value::String("material".to_string()));
        }
        assert!(has(&doc, "severity_outcome_mismatch"));
        Ok(())
    }

    // PR #12220 review repairs: omitted subject bindings fail closed.
    #[test]
    fn omitted_subject_bindings_fail_closed() -> TestResult {
        let doc = invalid_fixture("missing_subject_binding.json")?;
        assert!(has(&doc, "missing_subject_binding"));
        Ok(())
    }

    // PR #12220 review repairs: defect-class resolutions need evidence too.
    #[test]
    fn defect_resolutions_need_current_head_evidence() -> TestResult {
        let doc = invalid_fixture("defect_resolution_prose_only.json")?;
        assert!(has(&doc, "prose_resolution"));
        Ok(())
    }

    // PR #12220 review repairs: a programme-required role cannot be skipped.
    #[test]
    fn required_roles_cannot_be_skipped() -> TestResult {
        let doc = invalid_fixture("required_role_skipped.json")?;
        assert!(has(&doc, "required_role_not_applicable"));
        Ok(())
    }

    #[test]
    fn closure_role_state_fields_are_required_and_sufficient() -> TestResult {
        for (state, field, expected_violation) in [
            ("terminal", "reference", "missing_role_reference"),
            ("not_applicable", "reason", "role_not_applicable_unjustified"),
        ] {
            let mut doc = fixture("closure_service_marker_eligible.v1.json")?;
            color_eyre::eyre::ensure!(
                valid(&doc),
                "fixture with {state} {field} must establish sufficiency"
            );
            let roles = doc
                .as_object_mut()
                .and_then(|root| root.get_mut("review_state"))
                .and_then(Value::as_object_mut)
                .and_then(|review_state| review_state.get_mut("roles"))
                .and_then(Value::as_array_mut)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("fixture review_state.roles must be an array")
                })?;
            let role = roles
                .iter_mut()
                .find(|role| role.get("state").and_then(Value::as_str) == Some(state))
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("fixture must contain a role in state {state}")
                })?;
            role.remove(field);

            color_eyre::eyre::ensure!(
                has(&doc, expected_violation),
                "{state} without {field} must report {expected_violation}"
            );
        }
        Ok(())
    }

    // PR #12220 review repairs: nonmaterial open follow-ups do not block
    // closure eligibility; material and instrument failures do.
    #[test]
    fn nonmaterial_open_followups_do_not_block_eligibility() -> TestResult {
        let mut doc = fixture("closure_open_finding.v1.json")?;
        // Resolve the material finding and terminalize the pending role; the
        // open nonmaterial bounded follow-up alone must leave the honest
        // projection eligible.
        if let Some(review_state) = doc
            .as_object_mut()
            .and_then(|root| root.get_mut("review_state"))
            .and_then(Value::as_object_mut)
        {
            for role in review_state
                .get_mut("roles")
                .and_then(Value::as_array_mut)
                .unwrap_or(&mut Vec::new())
            {
                if role.get("role").and_then(Value::as_str) == Some("specialist")
                    && let Some(role) = role.as_object_mut()
                {
                    role.insert("state".to_string(), Value::String("terminal".to_string()));
                    role.insert(
                        "reference".to_string(),
                        Value::String("specialist review@c44e0d1b7".to_string()),
                    );
                }
            }
            for finding in review_state
                .get_mut("findings")
                .and_then(Value::as_array_mut)
                .unwrap_or(&mut Vec::new())
            {
                if finding.get("material").and_then(Value::as_bool) == Some(true)
                    && let Some(finding) = finding.as_object_mut()
                {
                    finding.insert(
                        "state".to_string(),
                        Value::String("resolved_on_current_head".to_string()),
                    );
                }
            }
        }
        if let Some(root) = doc.as_object_mut() {
            root.insert("eligibility".to_string(), Value::String("closure_eligible".to_string()));
        }
        assert!(valid(&doc), "an open nonmaterial follow-up must not block eligibility");
        Ok(())
    }

    // PR #12220 review repairs: the compact challenger prompt carries the
    // negative-control audit evidence, and completeness verifies it.
    #[test]
    fn compact_prompt_carries_control_evidence() -> TestResult {
        let doc = fixture("challenger_service_marker.v1.json")?;
        let compact = render_compact(&doc);
        assert!(compact.contains("NEGATIVE-CONTROLS"));
        assert!(compact.contains("independent_expectation_source: established"));
        let tampered = compact.replace(
            "Expected mismatch values are literal fixture expectations authored before the implementation.",
            "",
        );
        let violations = projection_completeness_violations(&doc, &tampered, "compact");
        assert!(
            violations.iter().any(|violation| violation.code == "projection_dropped_semantics"),
            "a compact prompt without control evidence must fail completeness"
        );
        Ok(())
    }

    // Simplify-review DEAD_SCAFFOLDING repair: the full run path (schema-enum
    // pins, valid fixtures, fail-closed controls, canonical semantics,
    // golden vectors) was reachable only via manual CLI. Check mode has no
    // side effects — `update_golden` is the only writer (mirrors
    // `train_edge_contract`'s real-tree run).
    #[test]
    fn run_passes_on_current_tree() -> TestResult {
        run(false)
    }
}
