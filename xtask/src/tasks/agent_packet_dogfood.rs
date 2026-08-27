//! Validate and report the shared domain-neutral agent packet dogfood core
//! contract (`agent_packet_dogfood.core.v1`, issue #11024 family).
//!
//! One closed core consumed by every agent packet dogfood row (#11024 parser
//! P05 precedent, #11638 distribution, #11268 reload, #11715 zed, #11598
//! analysis-train, #11654 clippy packets, #11704 authority-transfer). This
//! module owns only:
//!
//! - packet/tree/spec identity digests (always recomputed, never trusted);
//! - agent/model/tool/permission subject metadata with a mandatory
//!   permission scope ceiling;
//! - bounded observable event/result records with contiguous sequences;
//! - the closed disposition vocabulary (`started`, `completed`, `refused`,
//!   `transferred`, `not_proven`);
//! - human-intervention ledger fields bound to event boundaries;
//! - mutated-packet negative controls (fail-closed tamper detection);
//! - deterministic serialization of advisory reports (sorted by content,
//!   never filesystem enumeration order);
//! - commit-hygiene guards: no chain-of-thought keys, no credentials, no
//!   machine-local paths, no unbounded logs in retained evidence.
//!
//! Domain generators extend the core only through their own versioned
//! schemas; widening this vocabulary requires an `$id`/`schema_version`
//! bump. The contract generates no packets, executes no agents, performs no
//! network access, and gates no merge on model behavior.

use crate::utils::project_root;
use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_PATH: &str = ".ci/schemas/agent-packet-dogfood.core.v1.schema.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/agent-packet-dogfood.core.v1.schema.json";
const SCHEMA_NAME: &str = "agent_packet_dogfood.core.v1";

const FIXTURE_DIR: &str = "fixtures/agent_packet_dogfood_core";
const INVALID_DIR: &str = "invalid";
const GOLDEN_DIR: &str = "golden";

/// Valid synthetic packets are stamped self-consistently by the sanctioned
/// writer action (`agent-dogfood stamp`, an explicit generator/author step)
/// and committed together with their golden report vectors. List order — not
/// filesystem enumeration order — defines the golden report projection.
const VALID_FIXTURES: &[&str] =
    &["parser_p05_synthetic.v1.json", "analysis_train_synthetic_transfer.v1.json"];

/// Closed disposition vocabulary of the shared core.
const DISPOSITIONS: &[&str] = &["started", "completed", "refused", "transferred", "not_proven"];

/// Closed bounded observable-event vocabulary.
const EVENT_KINDS: &[&str] = &["observation", "tool_call", "tool_result", "agent_message", "error"];

/// Closed bounded terminal-record vocabulary.
const RESULT_KINDS: &[&str] = &["command_result", "check_result", "artifact_record"];

/// Closed human-intervention ledger roles.
const INTERVENTION_ROLES: &[&str] = &["human_operator", "automation_supervisor"];

/// Retention bounds for retained evidence (unbounded logs fail closed).
const MAX_EVENTS: usize = 1000;
const MAX_RESULTS: usize = 1000;
const MAX_INTERVENTIONS: usize = 100;
const MAX_RECORD_BYTES: usize = 8192;

/// Chain-of-thought key family: durable dogfood evidence never carries model
/// reasoning traces under these (or any renamed) semantic equivalents.
const COT_KEYS: &[&str] = &[
    "chain_of_thought",
    "chain-of-thought",
    "thinking",
    "scratchpad",
    "reasoning_trace",
    "raw_cot",
];

/// Credential markers scanned in every retained evidence string.
const CREDENTIAL_MARKERS: &[&str] = &[
    "-----begin",
    "ghp_",
    "github_pat_",
    "akia",
    "xoxb-",
    "xoxp-",
    "sk-proj-",
    "api_key=",
    "apikey=",
    "password=",
    "authorization: bearer",
];

/// Machine-local path markers that must never survive into retained packets.
const LOCAL_PATH_MARKERS: &[&str] = &["\\users\\", "/users/", "/home/", "/root/", "%userprofile%"];

/// Mutable live-state key family banned at any depth (borrowed shape of the
/// shared packet contracts): durable packets never embed scheduler state.
const MUTABLE_STATE_KEYS: &[&str] = &[
    "lease",
    "lease_owner",
    "assignment",
    "assigned_agent",
    "wake_event",
    "liveness",
    "heartbeat",
    "task_order",
    "frontier_cursor",
    "owner_token",
];

/// Root field set. A domain extension adds its own versioned schema; it does
/// not widen this core silently.
const ROOT_KEYS: &[&str] = &[
    "schema",
    "schema_version",
    "run_id",
    "identity",
    "subject",
    "disposition",
    "events",
    "results",
    "human_intervention",
    "metadata",
];

const IDENTITY_KEYS: &[&str] =
    &["packet_id", "packet_digest", "tree_sha", "spec_ref", "spec_digest"];
const SUBJECT_KEYS: &[&str] = &["agent", "model", "tool", "permissions"];
const NAMED_VERSION_KEYS: &[&str] = &["name", "version"];
const MODEL_KEYS: &[&str] = &["id", "revision"];
const PERMISSIONS_KEYS: &[&str] = &["ceiling"];
const EVENT_KEYS: &[&str] = &["seq", "kind", "at_ms", "payload", "digest"];
const RESULT_KEYS: &[&str] = &["seq", "kind", "at_ms", "payload", "digest"];
const INTERVENTION_KEYS: &[&str] = &["before_seq", "role", "reason"];

/// Domain-separation prefix hashed into every record digest.
const RECORD_DIGEST_DOMAIN: &str = "agent_packet_dogfood.core.v1.record";
/// Domain-separation prefix hashed into the integrity envelope digest.
const ENVELOPE_DIGEST_DOMAIN: &str = "agent_packet_dogfood.core.v1.envelope";

/// One deterministic validation violation with a stable reason code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Violation {
    code: String,
    detail: String,
}

impl Violation {
    fn new(code: &str, detail: String) -> Self {
        Self { code: code.to_string(), detail }
    }
}

fn violation_codes(violations: &[Violation]) -> Vec<&str> {
    violations.iter().map(|violation| violation.code.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Canonical bytes, digests, and stamping (recompute path of truth).
// ---------------------------------------------------------------------------

/// Canonical serialization: serde_json maps are sorted-key BTreeMaps here, so
/// `to_string` is key-order canonical; array order stays semantic input.
fn canonical_form(value: &Value) -> String {
    value.to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// SHA-256 over the canonical form of one observable/terminal record,
/// excluding the recorded `digest` field itself (domain-separated).
fn record_digest(record: &Map<String, Value>) -> Option<String> {
    let seq = record.get("seq")?;
    let kind = record.get("kind")?;
    let payload = record.get("payload")?;
    let mut canon = Map::new();
    canon.insert("domain".to_string(), Value::String(RECORD_DIGEST_DOMAIN.to_string()));
    canon.insert("seq".to_string(), seq.clone());
    canon.insert("kind".to_string(), kind.clone());
    if let Some(at_ms) = record.get("at_ms") {
        canon.insert("at_ms".to_string(), at_ms.clone());
    }
    canon.insert("payload".to_string(), payload.clone());
    Some(sha256_hex(canonical_form(&Value::Object(canon)).as_bytes()))
}

/// SHA-256 integrity envelope binding run identity, disposition, and the
/// recomputed record digests in sequence order (domain-separated).
fn envelope_digest(
    run_id: &str,
    disposition: &str,
    event_digests: &[Option<String>],
    result_digests: &[Option<String>],
) -> String {
    let string_array = |values: &[Option<String>]| -> Value {
        Value::Array(values.iter().map(|d| Value::String(d.clone().unwrap_or_default())).collect())
    };
    let mut canon = Map::new();
    canon.insert("domain".to_string(), Value::String(ENVELOPE_DIGEST_DOMAIN.to_string()));
    canon.insert("schema".to_string(), Value::String(SCHEMA_NAME.to_string()));
    canon.insert("run_id".to_string(), Value::String(run_id.to_string()));
    canon.insert("disposition".to_string(), Value::String(disposition.to_string()));
    canon.insert("events".to_string(), string_array(event_digests));
    canon.insert("results".to_string(), string_array(result_digests));
    sha256_hex(canonical_form(&Value::Object(canon)).as_bytes())
}

/// Recompute every record digest and the envelope digest, writing them back.
/// Explicit writer action for fixture authors and future domain generators;
/// generators stamp, validators re-verify.
pub fn stamp_manifest(doc: &mut Value) -> Result<()> {
    let Some(root) = doc.as_object_mut() else {
        bail!("manifest must be a JSON object");
    };

    let stamp_records = |records: &mut Vec<Value>| -> Result<Vec<Option<String>>> {
        let mut digests = Vec::new();
        for record in records {
            let Some(object) = record.as_object_mut() else {
                bail!("record must be an object");
            };
            let digest = record_digest(object)
                .ok_or_else(|| color_eyre::eyre::eyre!("record missing seq/kind/payload"))?;
            object.insert("digest".to_string(), Value::String(digest.clone()));
            digests.push(Some(digest));
        }
        Ok(digests)
    };

    let mut event_digests = Vec::new();
    let mut result_digests = Vec::new();
    if let Some(events) = root.get_mut("events").and_then(Value::as_array_mut) {
        event_digests = stamp_records(events)?;
    }
    if let Some(results) = root.get_mut("results").and_then(Value::as_array_mut) {
        result_digests = stamp_records(results)?;
    }

    let run_id = root.get("run_id").and_then(Value::as_str).unwrap_or_default().to_string();
    // Disposition is read before mutation so envelope input mirrors what a
    // validator sees post-stamp.
    let disposition =
        root.get("disposition").and_then(Value::as_str).unwrap_or_default().to_string();

    if let Some(identity) = root.get_mut("identity").and_then(Value::as_object_mut) {
        let packet_digest = envelope_digest(&run_id, &disposition, &event_digests, &result_digests);
        identity.insert("packet_digest".to_string(), Value::String(packet_digest));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Validation (fail-closed, deterministic violation order).
// ---------------------------------------------------------------------------

fn as_str_map(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn check_unknown_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            violations.push(Violation::new(
                "unknown_field",
                format!("{where_}: unexpected field {key} (closed core vocabulary; widen via a new versioned schema, not this one)"),
            ));
        }
    }
}

fn require_non_empty(
    object: &Map<String, Value>,
    key: &str,
    code: &str,
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    if string_field(object, key).is_none() {
        violations
            .push(Violation::new(code, format!("{where_}: {key} must be a non-empty string")));
    }
}

fn require_sha256_hex(
    object: &Map<String, Value>,
    key: &str,
    where_: &str,
    violations: &mut Vec<Violation>,
) {
    match object.get(key).and_then(Value::as_str) {
        Some(digest)
            if digest.len() == 64
                && digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => {}
        Some(_) => violations.push(Violation::new(
            "malformed_digest",
            format!("{where_}: {key} must be lowercase 64-hex sha256"),
        )),
        None => violations.push(Violation::new(
            "missing_identity_field",
            format!("{where_}: {key} must be a non-empty digest"),
        )),
    }
}

fn scan_strings<F: FnMut(&str, &str)>(value: &Value, where_: &str, visit: &mut F) {
    match value {
        Value::String(text) => visit(text, where_),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                scan_strings(item, &format!("{where_}[{index}]"), visit);
            }
        }
        Value::Object(object) => {
            for (key, child) in object {
                scan_strings(child, &format!("{where_}.{key}"), visit);
            }
        }
        _ => {}
    }
}

/// Hygiene guard: credentials/machine-local paths inside one retained string.
fn scan_hygiene(text: &str, where_: &str, violations: &mut Vec<Violation>) {
    let lowered = text.to_lowercase();
    if CREDENTIAL_MARKERS.iter().any(|marker| lowered.contains(marker)) {
        violations.push(Violation::new(
            "credential_in_payload",
            format!("{where_}: retained evidence contains a credential marker"),
        ));
    }
    if LOCAL_PATH_MARKERS.iter().any(|marker| lowered.contains(marker)) {
        violations.push(Violation::new(
            "local_path_in_payload",
            format!("{where_}: retained evidence contains a machine-local path"),
        ));
    }
    for pair in lowered.chars().collect::<Vec<_>>().windows(3) {
        if pair[0].is_ascii_lowercase() && pair[1] == ':' && pair[2] == '\\' {
            violations.push(Violation::new(
                "local_path_in_payload",
                format!("{where_}: retained evidence contains a drive-letter path"),
            ));
            break;
        }
    }
}

/// Mutated-packet + hygiene recursion over one retained payload object.
fn scan_payload(payload: &Value, where_: &str, violations: &mut Vec<Violation>) {
    let mut visit = |text: &str, at: &str| scan_hygiene(text, at, violations);
    scan_strings(payload, where_, &mut visit);
    let mut check_key = |key: &str, at: &str| {
        if COT_KEYS.contains(&key) {
            violations.push(Violation::new(
                "cot_key_in_payload",
                format!("{at}: chain-of-thought key {key} must never be retained"),
            ));
        }
    };
    let mut walk_keys = |value: &Value, at: &str| {
        if let Value::Object(object) = value {
            for key in object.keys() {
                check_key(key, at);
            }
        }
    };
    walk_keys(payload, where_);
}

fn contains_forbidden_mutable_state(value: &Value, where_: &str, violations: &mut Vec<Violation>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if MUTABLE_STATE_KEYS.contains(&key.as_str()) {
                    violations.push(Violation::new(
                        "mutable_state_embedded",
                        format!("{where_}: durable packet carries live-state field {key}"),
                    ));
                }
                contains_forbidden_mutable_state(child, &format!("{where_}.{key}"), violations);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                contains_forbidden_mutable_state(item, &format!("{where_}[{index}]"), violations);
            }
        }
        _ => {}
    }
}

/// Validate one bounded record (event or result). Returns Ok(recomputed
/// digest) when all structural rules held, even on digest mismatch, so the
/// caller can still attempt envelope comparison.
#[allow(clippy::too_many_arguments)]
fn validate_record(
    record: &Map<String, Value>,
    where_: &str,
    kinds: &[&str],
    allowed_keys: &[&str],
    expected_seq: u64,
    violations: &mut Vec<Violation>,
) -> Option<String> {
    let at = format!("{where_}[{expected_seq}]");
    check_unknown_keys(record, allowed_keys, &at, violations);
    match record.get("seq").and_then(Value::as_u64) {
        Some(seq) if seq == expected_seq => {}
        Some(_) | None => violations.push(Violation::new(
            "event_seq_not_contiguous",
            format!("{at}: seq must be the next contiguous integer (expected {expected_seq})"),
        )),
    }
    match string_field(record, "kind") {
        Some(kind) if kinds.contains(&kind) => {}
        Some(kind) => violations
            .push(Violation::new("unknown_record_kind", format!("{at}: unknown kind {kind}"))),
        None => violations
            .push(Violation::new("missing_record_kind", format!("{at}: kind is required"))),
    }
    if record.get("payload").and_then(as_str_map).is_none() {
        violations.push(Violation::new(
            "missing_record_payload",
            format!("{at}: payload must be an object"),
        ));
    }
    if record.get("at_ms").is_some_and(|at_ms| at_ms.as_u64().is_none()) {
        violations.push(Violation::new(
            "malformed_at_ms",
            format!("{at}: at_ms must be a non-negative integer"),
        ));
    }
    if canonical_form(&Value::Object(record.clone())).len() > MAX_RECORD_BYTES {
        violations.push(Violation::new(
            "retention_bound_exceeded",
            format!("{at}: record exceeds the {MAX_RECORD_BYTES}-byte retention bound (unbounded logs are not evidence)"),
        ));
    }
    if let Some(payload) = record.get("payload") {
        scan_payload(payload, &format!("{at}.payload"), violations);
    }
    let recomputed = record_digest(record);
    match (string_field(record, "digest"), &recomputed) {
        (Some(_), Some(expected)) => {
            if string_field(record, "digest") != Some(expected.as_str()) {
                violations.push(Violation::new(
                    "record_digest_mismatch",
                    format!(
                        "{at}: recorded digest does not match the recomputed content digest (tampered mid-run)"
                    ),
                ));
            }
        }
        (Some(_), None) => violations.push(Violation::new(
            "missing_record_fields",
            format!("{at}: digest could not be recomputed without seq/kind/payload"),
        )),
        (None, _) => violations
            .push(Violation::new("missing_identity_field", format!("{at}: digest is required"))),
    }
    recomputed
}

/// Validate one manifest against the closed core. Every rule below is
/// fail-closed and deterministic: the returned violation list is built in a
/// fixed walk order, so equal documents yield equal violation sequences.
pub(crate) fn validate_manifest(doc: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Some(root) = as_str_map(doc) else {
        return vec![Violation::new("not_an_object", "manifest must be a JSON object".to_string())];
    };

    if string_field(root, "schema") != Some(SCHEMA_NAME) {
        violations.push(Violation::new("wrong_schema", format!("schema must be {SCHEMA_NAME}")));
    }
    if root.get("schema_version").and_then(Value::as_u64) != Some(1) {
        violations
            .push(Violation::new("wrong_schema_version", "schema_version must be 1".to_string()));
    }
    check_unknown_keys(root, ROOT_KEYS, "manifest", &mut violations);
    contains_forbidden_mutable_state(doc, "manifest", &mut violations);
    require_non_empty(root, "run_id", "missing_run_id", "manifest", &mut violations);

    // Packet/tree/spec identity digests.
    match root.get("identity").and_then(as_str_map) {
        Some(identity) => {
            check_unknown_keys(identity, IDENTITY_KEYS, "identity", &mut violations);
            require_non_empty(
                identity,
                "packet_id",
                "missing_identity_field",
                "identity",
                &mut violations,
            );
            require_sha256_hex(identity, "packet_digest", "identity", &mut violations);
            require_sha256_hex(identity, "tree_sha", "identity", &mut violations);
            require_non_empty(
                identity,
                "spec_ref",
                "missing_identity_field",
                "identity",
                &mut violations,
            );
            require_sha256_hex(identity, "spec_digest", "identity", &mut violations);
        }
        None => violations.push(Violation::new(
            "missing_identity",
            "manifest: required identity was not supplied".to_string(),
        )),
    }

    // Subject metadata is required and complete: model/permission identity is
    // never optional (falsifier 2 of the family brief).
    match root.get("subject").and_then(as_str_map) {
        Some(subject) => {
            check_unknown_keys(subject, SUBJECT_KEYS, "subject", &mut violations);
            for section in ["agent", "tool"] {
                match subject.get(section).and_then(as_str_map) {
                    Some(named_version) => {
                        check_unknown_keys(
                            named_version,
                            NAMED_VERSION_KEYS,
                            &format!("subject.{section}"),
                            &mut violations,
                        );
                        for field in NAMED_VERSION_KEYS {
                            require_non_empty(
                                named_version,
                                field,
                                "missing_subject_field",
                                &format!("subject.{section}"),
                                &mut violations,
                            );
                        }
                    }
                    None => violations.push(Violation::new(
                        "missing_subject_field",
                        format!("subject: {section} must be an object with name and version"),
                    )),
                }
            }
            match subject.get("model").and_then(as_str_map) {
                Some(model) => {
                    check_unknown_keys(model, MODEL_KEYS, "subject.model", &mut violations);
                    require_non_empty(
                        model,
                        "id",
                        "missing_subject_field",
                        "subject.model",
                        &mut violations,
                    );
                }
                None => violations.push(Violation::new(
                    "missing_subject_field",
                    "subject: model must be an object with id".to_string(),
                )),
            }
            // The permission scope ceiling may never be dropped or emptied.
            let ceiling_ok = subject
                .get("permissions")
                .and_then(as_str_map)
                .map(|permissions| {
                    check_unknown_keys(
                        permissions,
                        PERMISSIONS_KEYS,
                        "subject.permissions",
                        &mut violations,
                    );
                    permissions.get("ceiling").and_then(Value::as_array)
                })
                .and_then(|ceiling| ceiling)
                .is_some_and(|ceiling| {
                    !ceiling.is_empty()
                        && ceiling.len() <= 64
                        && ceiling.iter().all(|scope| scope.as_str().is_some_and(|s| !s.is_empty()))
                });
            if !ceiling_ok {
                violations.push(Violation::new(
                    "missing_scope_ceiling",
                    "subject: permission scope ceiling is required and non-empty".to_string(),
                ));
            }
        }
        None => violations.push(Violation::new(
            "missing_subject",
            "manifest: required subject metadata was not supplied".to_string(),
        )),
    }

    // Closed disposition vocabulary.
    match string_field(root, "disposition") {
        Some(disposition) if DISPOSITIONS.contains(&disposition) => {}
        Some(disposition) => violations.push(Violation::new(
            "unknown_disposition",
            format!("manifest: unknown disposition {disposition}"),
        )),
        None => violations.push(Violation::new(
            "missing_disposition",
            "manifest: disposition is required".to_string(),
        )),
    }

    // Bounded observable event records; contiguous sequence from 0.
    let mut recompute_records = |records: Option<&Vec<Value>>,
                                 kinds: &[&str],
                                 keys: &[&str],
                                 required_min: bool,
                                 max_records: usize,
                                 label: &str|
     -> Vec<Option<String>> {
        let mut recomputed = Vec::new();
        match records {
            None => {
                if required_min {
                    violations.push(Violation::new(
                        "missing_events",
                        format!("manifest: at least one {label} record is required"),
                    ));
                }
            }
            Some(records) => {
                if records.len() > max_records {
                    violations.push(Violation::new(
                        "retention_bound_exceeded",
                        format!(
                            "{label}: {} records exceed the {max_records}-record retention bound",
                            records.len()
                        ),
                    ));
                }
                for (index, record) in records.iter().enumerate() {
                    let digest = match record.as_object() {
                        Some(object) => validate_record(
                            object,
                            label,
                            kinds,
                            keys,
                            index as u64,
                            &mut violations,
                        ),
                        None => {
                            violations.push(Violation::new(
                                "not_an_object",
                                format!("{label}[{index}] must be an object"),
                            ));
                            None
                        }
                    };
                    recomputed.push(digest);
                }
            }
        }
        recomputed
    };

    let event_digests = recompute_records(
        root.get("events").and_then(Value::as_array),
        EVENT_KINDS,
        EVENT_KEYS,
        true,
        MAX_EVENTS,
        "events",
    );
    let result_digests = recompute_records(
        root.get("results").and_then(Value::as_array),
        RESULT_KINDS,
        RESULT_KEYS,
        false,
        MAX_RESULTS,
        "results",
    );

    // Human-intervention ledger: every entry binds to an existing event
    // boundary; a packet that hides intervention while citing none is fine,
    // but every cited intervention must be complete.
    if let Some(interventions) = root.get("human_intervention").and_then(Value::as_array) {
        if interventions.len() > MAX_INTERVENTIONS {
            violations.push(Violation::new(
                "retention_bound_exceeded",
                format!(
                    "human_intervention: {} entries exceed the {MAX_INTERVENTIONS}-entry bound",
                    interventions.len()
                ),
            ));
        }
        let known_seqs: Vec<u64> = root
            .get("events")
            .and_then(Value::as_array)
            .map(|events| {
                events
                    .iter()
                    .filter_map(Value::as_object)
                    .filter_map(|event| event.get("seq").and_then(Value::as_u64))
                    .collect()
            })
            .unwrap_or_default();
        for (index, entry) in interventions.iter().enumerate() {
            let at = format!("human_intervention[{index}]");
            let Some(entry) = as_str_map(entry) else {
                violations.push(Violation::new("not_an_object", format!("{at} must be an object")));
                continue;
            };
            check_unknown_keys(entry, INTERVENTION_KEYS, &at, &mut violations);
            match entry.get("before_seq").and_then(Value::as_u64) {
                Some(seq) if known_seqs.contains(&seq) => {}
                Some(_) | None => violations.push(Violation::new(
                    "intervention_seq_unknown",
                    format!("{at}: before_seq must reference an event seq boundary"),
                )),
            }
            match string_field(entry, "role") {
                Some(role) if INTERVENTION_ROLES.contains(&role) => {}
                Some(role) => violations.push(Violation::new(
                    "unknown_intervention_role",
                    format!("{at}: unknown role {role}"),
                )),
                None => violations.push(Violation::new(
                    "missing_intervention_role",
                    format!("{at}: role is required"),
                )),
            }
            require_non_empty(entry, "reason", "missing_intervention_reason", &at, &mut violations);
            scan_hygiene(string_field(entry, "reason").unwrap_or_default(), &at, &mut violations);
        }
    } else if root.get("human_intervention").is_some() {
        violations.push(Violation::new(
            "not_an_object",
            "human_intervention must be an array".to_string(),
        ));
    }

    // Envelope integrity: the validator recomputes the packet digest from the
    // canonical envelope over the RECOMPUTED record digests, never trusting
    // the self-reported value (falsifier 6).
    let recomputed_envelope = envelope_digest(
        &string_of(root, "run_id"),
        &string_of(root, "disposition"),
        &event_digests,
        &result_digests,
    );
    let recorded = root
        .get("identity")
        .and_then(as_str_map)
        .and_then(|identity| identity.get("packet_digest"))
        .and_then(Value::as_str)
        .filter(|digest| {
            digest.len() == 64
                && digest.bytes().all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase())
        });
    if recorded.is_some_and(|recorded| recorded != recomputed_envelope) {
        violations.push(Violation::new(
            "packet_digest_mismatch",
            "identity: packet_digest does not match the recomputed integrity envelope (tampered mid-run)"
                .to_string(),
        ));
    }

    violations
}

fn string_of(root: &Map<String, Value>, key: &str) -> String {
    string_field(root, key).unwrap_or_default().to_string()
}

/// Render one deterministic advisory report line-group for validated runs.
/// Ordering is derived from manifest content (run identity) and the caller's
/// explicit manifest list order — never from directory enumeration.
struct RunRow {
    source: String,
    run_id: String,
    disposition: String,
    validity: String,
    violations: Vec<Violation>,
}

fn load_manifest(path: &Path) -> Result<(String, Value)> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let doc: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let source = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    Ok((source, doc))
}

fn collect_rows(entries: &[(String, Value)]) -> Vec<RunRow> {
    entries
        .iter()
        .map(|(source, doc)| {
            let run_id =
                doc.get("run_id").and_then(Value::as_str).unwrap_or("<missing-run-id>").to_string();
            let disposition = doc
                .get("disposition")
                .and_then(Value::as_str)
                .unwrap_or("<missing-disposition>")
                .to_string();
            let violations = validate_manifest(doc);
            RunRow {
                source: source.clone(),
                run_id,
                disposition,
                validity: if violations.is_empty() { "valid" } else { "invalid" }.to_string(),
                violations,
            }
        })
        .collect()
}

/// Advisory report across runs, sorted by (run_id, source) for determinism.
fn render_report(rows: &[RunRow], format: DogfoodReportFormat) -> String {
    let mut ordered: Vec<&RunRow> = rows.iter().collect();
    ordered.sort_by(|a, b| (&a.run_id, &a.source).cmp(&(&b.run_id, &b.source)));
    match format {
        DogfoodReportFormat::Markdown => {
            let mut out = String::new();
            out.push_str("# agent-packet-dogfood core advisory report\n\n");
            out.push_str("| run_id | disposition | validity | findings |\n");
            out.push_str("| --- | --- | --- | --- |\n");
            for row in &ordered {
                out.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    row.run_id,
                    row.disposition,
                    row.validity,
                    row.violations.len()
                ));
            }
            out
        }
        DogfoodReportFormat::Json => {
            let runs: Vec<Value> = ordered
                .iter()
                .map(|row| {
                    let mut entry = Map::new();
                    entry.insert("run_id".to_string(), Value::String(row.run_id.clone()));
                    entry.insert("disposition".to_string(), Value::String(row.disposition.clone()));
                    entry.insert("validity".to_string(), Value::String(row.validity.clone()));
                    let codes: Vec<Value> = violation_codes(&row.violations)
                        .into_iter()
                        .map(|code| Value::String(code.to_string()))
                        .collect();
                    entry.insert("findings".to_string(), Value::Array(codes));
                    Value::Object(entry)
                })
                .collect();
            let mut report = Map::new();
            report.insert(
                "report".to_string(),
                Value::String("agent-packet-dogfood.core.report.v1".to_string()),
            );
            report.insert("runs".to_string(), Value::Array(runs));
            format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(report)).unwrap_or_default()
            )
        }
    }
}

// ---------------------------------------------------------------------------
// CLI surface: `cargo xtask agent-dogfood validate|report|stamp`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum DogfoodReportFormat {
    Markdown,
    Json,
}

impl DogfoodReportFormat {
    fn name(self) -> &'static str {
        match self {
            DogfoodReportFormat::Markdown => "markdown",
            DogfoodReportFormat::Json => "json",
        }
    }

    fn golden_extension(self) -> &'static str {
        match self {
            DogfoodReportFormat::Markdown => "md",
            DogfoodReportFormat::Json => "json",
        }
    }
}

#[derive(Subcommand)]
pub enum AgentDogfoodCommand {
    /// Validate shared-core schemas and synthetic packets (fail-closed).
    ///
    /// With no `--manifest`, runs the repository contract proof: schema
    /// self-check against the pinned Rust vocabulary, every fail-closed
    /// negative-control fixture, every stamped valid fixture, and the golden
    /// report determinism vectors. With `--manifest FILE` (repeatable),
    /// validates exactly those caller-supplied packets.
    Validate {
        /// Caller-supplied packet documents to validate (repeatable).
        #[arg(long = "manifest", value_name = "FILE")]
        manifests: Vec<PathBuf>,

        /// Rewrite golden report vectors (explicit writer action; contract
        /// mode only, never live packet state).
        #[arg(long)]
        update_golden: bool,
    },

    /// Emit the deterministic advisory report for the given packets
    /// (defaults to the committed synthetic fixture set). Never gates on
    /// model behavior; reports classification and validity only.
    Report {
        /// Caller-supplied packet documents (repeatable).
        #[arg(long = "manifest", value_name = "FILE")]
        manifests: Vec<PathBuf>,

        /// Output projection.
        #[arg(long, value_enum, default_value = "markdown")]
        format: DogfoodReportFormat,
    },

    /// Recompute record and envelope digests for one packet document and
    /// rewrite the file in place. Explicit writer action for fixture authors
    /// and domain generators; validators always re-verify instead of trusting.
    Stamp {
        /// Packet document to stamp in place.
        #[arg(long = "manifest", value_name = "FILE")]
        manifest: PathBuf,
    },
}

/// Entry point dispatched from main.rs.
pub fn run(command: AgentDogfoodCommand) -> Result<()> {
    match command {
        AgentDogfoodCommand::Validate { manifests, update_golden } => {
            run_validate(&manifests, update_golden)
        }
        AgentDogfoodCommand::Report { manifests, format } => run_report(&manifests, format),
        AgentDogfoodCommand::Stamp { manifest } => run_stamp(&manifest),
    }
}

fn run_validate(manifests: &[PathBuf], update_golden: bool) -> Result<()> {
    if !manifests.is_empty() {
        return validate_supplied(manifests);
    }
    let root = project_root()?;
    let mut failures: Vec<String> = Vec::new();

    for failure in validate_schema_file(&root)? {
        failures.push(format!("{SCHEMA_PATH}: {failure}"));
    }

    // Fail-closed negative controls: each synthetic mutant must be rejected
    // with exactly its pinned reason code.
    let invalid_dir = root.join(FIXTURE_DIR).join(INVALID_DIR);
    let expected_path = invalid_dir.join("expected_errors.json");
    if expected_path.exists() {
        let expected: Value = serde_json::from_str(
            &fs::read_to_string(&expected_path)
                .with_context(|| format!("failed to read {}", expected_path.display()))?,
        )
        .with_context(|| format!("failed to parse {}", expected_path.display()))?;
        let mut names: Vec<String> =
            expected.as_object().map(|object| object.keys().cloned().collect()).unwrap_or_default();
        names.sort();
        for name in names {
            let expected_code =
                expected.get(&name).and_then(Value::as_str).unwrap_or_default().to_string();
            let path = invalid_dir.join(&name);
            let doc = load_manifest(&path)?.1;
            let violations = validate_manifest(&doc);
            let codes = violation_codes(&violations);
            if codes.is_empty() {
                failures.push(format!(
                    "{INVALID_DIR}/{name}: expected failure {expected_code}, document validated"
                ));
            } else if !codes.contains(&expected_code.as_str()) {
                failures.push(format!(
                    "{INVALID_DIR}/{name}: expected failure {expected_code}, got {codes:?}"
                ));
            }
        }
    } else {
        failures.push(format!("missing negative-control matrix {}", expected_path.display()));
    }

    // Stamped positive fixtures and their golden advisory reports.
    let fixture_dir = root.join(FIXTURE_DIR);
    let mut valid_entries: Vec<(String, Value)> = Vec::new();
    for name in VALID_FIXTURES {
        let path = fixture_dir.join(name);
        let (source, doc) = load_manifest(&path)?;
        let violations = validate_manifest(&doc);
        if !violations.is_empty() {
            failures.push(format!(
                "{name}: expected a valid packet, got {:?}",
                violation_codes(&violations)
            ));
        }
        valid_entries.push((source, doc));
    }
    if !valid_entries.is_empty() || update_golden {
        let markdown = render_report(&collect_rows(&valid_entries), DogfoodReportFormat::Markdown);
        let json = render_report(&collect_rows(&valid_entries), DogfoodReportFormat::Json);
        let projections =
            [(DogfoodReportFormat::Markdown, markdown), (DogfoodReportFormat::Json, json)];
        if update_golden {
            for (format, text) in &projections {
                let path = fixture_dir.join(GOLDEN_DIR).join(format!(
                    "agent_packet_dogfood_core.advisory.{}",
                    format.golden_extension()
                ));
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::write(&path, text)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                println!("wrote golden vector {}", path.display());
            }
        } else {
            for (format, text) in &projections {
                let path = fixture_dir.join(GOLDEN_DIR).join(format!(
                    "agent_packet_dogfood_core.advisory.{}",
                    format.golden_extension()
                ));
                let golden = fs::read_to_string(&path).with_context(|| {
                    format!("missing golden vector {}; rerun with --update-golden", path.display())
                })?;
                if &golden != text {
                    failures.push(format!(
                        "advisory report {} projection drifted from its golden vector",
                        format.name()
                    ));
                }
            }
        }
    }

    if failures.is_empty() {
        println!("{SCHEMA_NAME}: all contract proofs pass");
        return Ok(());
    }
    for failure in &failures {
        eprintln!("FAIL: {failure}");
    }
    bail!("{} contract failure(s)", failures.len());
}

fn validate_supplied(manifests: &[PathBuf]) -> Result<()> {
    let mut had_failure = false;
    for path in manifests {
        let (_, doc) = load_manifest(path)?;
        let violations = validate_manifest(&doc);
        if violations.is_empty() {
            println!("PASS {}: valid {} packet", path.display(), SCHEMA_NAME);
        } else {
            had_failure = true;
            for violation in &violations {
                eprintln!("FAIL {}: {} ({})", path.display(), violation.code, violation.detail);
            }
        }
    }
    if had_failure {
        bail!("caller-supplied packets failed the closed contract");
    }
    Ok(())
}

fn run_report(manifests: &[PathBuf], format: DogfoodReportFormat) -> Result<()> {
    let entries = load_entries(manifests)?;
    print!("{}", render_report(&collect_rows(&entries), format));
    Ok(())
}

fn load_entries(manifests: &[PathBuf]) -> Result<Vec<(String, Value)>> {
    if manifests.is_empty() {
        let root = project_root()?;
        let fixture_dir = root.join(FIXTURE_DIR);
        let mut entries = Vec::new();
        for name in VALID_FIXTURES {
            let (source, doc) = load_manifest(&fixture_dir.join(name))?;
            entries.push((source, doc));
        }
        return Ok(entries);
    }
    manifests.iter().map(|path| load_manifest(path)).collect()
}

fn run_stamp(path: &Path) -> Result<()> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut doc: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    stamp_manifest(&mut doc)?;
    let stamped = serde_json::to_string_pretty(&doc)?;
    fs::write(path, stamped + "\n")
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("stamped {}", path.display());
    Ok(())
}

/// Schema self-check: the committed JSON schema pins exactly the Rust
/// vocabulary of this module, the `$id`, and the closed enum surface.
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
    if string_field(object, "$schema") != Some("https://json-schema.org/draft/2020-12/schema") {
        violations.push("$schema must pin draft 2020-12".to_string());
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
        (&["$defs", "disposition", "enum"], DISPOSITIONS, "dispositions"),
        (&["$defs", "event_kind", "enum"], EVENT_KINDS, "event kinds"),
        (&["$defs", "result_kind", "enum"], RESULT_KINDS, "result kinds"),
        (&["$defs", "intervention_role", "enum"], INTERVENTION_ROLES, "intervention roles"),
    ];
    for (path_segments, expected_values, label) in expected {
        match enum_of(path_segments) {
            Some(values)
                if values.iter().map(String::as_str).eq(expected_values.iter().copied()) => {}
            Some(values) => violations.push(format!(
                "schema {label} enum drifted from the pinned closed vocabulary: {values:?}"
            )),
            None => violations
                .push(format!("schema is missing the {label} enum at {}", path_segments.join("."))),
        }
    }
    Ok(violations)
}

#[cfg(test)]
mod tests;
