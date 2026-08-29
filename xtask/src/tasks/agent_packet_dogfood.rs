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

/// Structured field names that must never be retained, even when their
/// values do not resemble a credential assignment.
const CREDENTIAL_KEY_MARKERS: &[&str] = &[
    "api_key",
    "apikey",
    "password",
    "access_token",
    "auth_token",
    "client_secret",
    "private_key",
    "credential",
    "credentials",
    "secret",
    "token",
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

/// Canonical serialization for every digest input: object keys are sorted
/// explicitly at every depth (serde_json maps are not relied on for ordering —
/// the resolved `serde_json` may back `Map` with an insertion-ordered indexmap),
/// while array element order stays semantic input. Equal documents therefore
/// hash equally regardless of key insertion order.
fn canonical_form(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let mut out = String::with_capacity(64);
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_form(&Value::String((*key).to_string())));
                out.push(':');
                out.push_str(&canonical_form(&map[*key]));
            }
            out.push('}');
            out
        }
        Value::Array(items) => {
            let mut out = String::with_capacity(16);
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_form(item));
            }
            out.push(']');
            out
        }
        other => other.to_string(),
    }
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

/// SHA-256 integrity envelope binding the semantic identity of the run
/// (every identity field except the self-reported `packet_digest`), the
/// complete subject metadata, run identity, disposition, and the recomputed
/// record digests in sequence order (domain-separated).
fn envelope_digest(
    identity: &Value,
    subject: &Value,
    run_id: &str,
    disposition: &str,
    event_digests: &[Option<String>],
    result_digests: &[Option<String>],
    human_intervention: &Value,
) -> String {
    let string_array = |values: &[Option<String>]| -> Value {
        Value::Array(values.iter().map(|d| Value::String(d.clone().unwrap_or_default())).collect())
    };
    let mut canon = Map::new();
    canon.insert("domain".to_string(), Value::String(ENVELOPE_DIGEST_DOMAIN.to_string()));
    canon.insert("schema".to_string(), Value::String(SCHEMA_NAME.to_string()));
    canon.insert("run_id".to_string(), Value::String(run_id.to_string()));
    canon.insert("identity".to_string(), identity.clone());
    canon.insert("subject".to_string(), subject.clone());
    canon.insert("disposition".to_string(), Value::String(disposition.to_string()));
    canon.insert("events".to_string(), string_array(event_digests));
    canon.insert("results".to_string(), string_array(result_digests));
    canon.insert("human_intervention".to_string(), human_intervention.clone());
    sha256_hex(canonical_form(&Value::Object(canon)).as_bytes())
}

/// Envelope input for the identity object: every identity field except the
/// self-reported `packet_digest`, which the envelope itself defines.
fn envelope_identity(identity: &Map<String, Value>) -> Value {
    let mut canon = identity.clone();
    canon.remove("packet_digest");
    Value::Object(canon)
}

/// Recompute every record digest and the envelope digest, writing them back.
/// Explicit writer action for fixture authors and future domain generators;
/// generators stamp, validators re-verify. Stamping never reports success
/// without writing a digest: a document missing any semantic envelope input
/// is a typed error, not a silent no-op.
fn stamp_validation_document(doc: &Value) -> Value {
    let mut candidate = doc.clone();
    let Some(root) = candidate.as_object_mut() else {
        return candidate;
    };

    // Stamping is allowed to create or repair digest fields, so validate the
    // semantic document with those recomputable fields normalized first. The
    // clone keeps the caller's document untouched if any contract violation is
    // found, while all non-digest rules still come from validate_manifest.
    let digest_values = |slot: Option<&mut Value>| -> Vec<Option<String>> {
        match slot {
            Some(Value::Array(records)) => records
                .iter_mut()
                .map(|record| {
                    record.as_object_mut().and_then(|object| {
                        let digest = record_digest(object);
                        if let Some(digest) = &digest {
                            object.insert("digest".to_string(), Value::String(digest.clone()));
                        }
                        digest
                    })
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    let event_digests = digest_values(root.get_mut("events"));
    let result_digests = digest_values(root.get_mut("results"));

    let identity_input = root.get("identity").and_then(as_str_map).map(envelope_identity);
    let subject =
        root.get("subject").and_then(as_str_map).map(|subject| Value::Object(subject.clone()));
    let run_id = root.get("run_id").and_then(Value::as_str);
    let disposition = root.get("disposition").and_then(Value::as_str);
    let human_intervention =
        root.get("human_intervention").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    if let (Some(identity_input), Some(subject), Some(run_id), Some(disposition)) =
        (identity_input, subject, run_id, disposition)
    {
        let packet_digest = envelope_digest(
            &identity_input,
            &subject,
            run_id,
            disposition,
            &event_digests,
            &result_digests,
            &human_intervention,
        );
        if let Some(identity) = root.get_mut("identity").and_then(Value::as_object_mut) {
            identity.insert("packet_digest".to_string(), Value::String(packet_digest));
        }
    }

    candidate
}

pub fn stamp_manifest(doc: &mut Value) -> Result<()> {
    let validation_doc = stamp_validation_document(doc);
    let violations = validate_manifest(&validation_doc);
    if !violations.is_empty() {
        bail!(
            "manifest failed closed validation before stamping: {:?}",
            violation_codes(&violations)
        );
    }

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

    let stamp_slot = |slot: Option<&mut Value>, label: &str| -> Result<Vec<Option<String>>> {
        match slot {
            None => Ok(Vec::new()),
            Some(Value::Array(records)) => stamp_records(records),
            Some(_) => bail!("{label} must be an array when present"),
        }
    };

    let event_digests = stamp_slot(root.get_mut("events"), "events")?;
    let result_digests = stamp_slot(root.get_mut("results"), "results")?;

    let Some(run_id) = root.get("run_id").and_then(Value::as_str).map(str::to_string) else {
        bail!("manifest must carry a run_id string before stamping");
    };
    // Disposition is read before mutation so envelope input mirrors what a
    // validator sees post-stamp.
    let Some(disposition) = root.get("disposition").and_then(Value::as_str).map(str::to_string)
    else {
        bail!("manifest must carry a disposition string before stamping");
    };
    let identity_input =
        root.get("identity").and_then(as_str_map).map(envelope_identity).ok_or_else(|| {
            color_eyre::eyre::eyre!("manifest must carry an identity object before stamping")
        })?;
    let subject = root.get("subject").and_then(as_str_map).ok_or_else(|| {
        color_eyre::eyre::eyre!("manifest must carry a subject object before stamping")
    })?;
    let human_intervention =
        root.get("human_intervention").cloned().unwrap_or_else(|| Value::Array(Vec::new()));

    let packet_digest = envelope_digest(
        &identity_input,
        &Value::Object(subject.clone()),
        &run_id,
        &disposition,
        &event_digests,
        &result_digests,
        &human_intervention,
    );
    let Some(identity) = root.get_mut("identity").and_then(Value::as_object_mut) else {
        bail!("manifest must carry an identity object before stamping");
    };
    identity.insert("packet_digest".to_string(), Value::String(packet_digest));
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
        Some(digest) if is_lowercase_sha256_hex(digest) => {}
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

/// Strict lowercase 64-hex sha256 charset: `0-9a-f` only (never `g-z`).
fn is_lowercase_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

fn is_credential_key(key: &str) -> bool {
    let normalized = normalize_credential_key(key);
    let bounded = format!("_{normalized}_");
    CREDENTIAL_KEY_MARKERS.iter().any(|marker| bounded.contains(&format!("_{marker}_")))
}

fn is_named_key(key: &str, names: &[&str]) -> bool {
    let normalized = normalize_key(key);
    names.iter().any(|name| normalized == *name)
}

/// Normalize structured field names before comparing them with the closed
/// credential vocabulary. JSON producers use both snake_case and camelCase
/// (including acronym-bearing names such as `APIKey`), so lowercasing alone
/// would let `accessToken`, `tokenValue`, or `clientSecretValue` bypass the
/// fail-closed check. The caller bounds marker matches to underscore-delimited
/// segments so comparable prefix, suffix, and nested variants are rejected too.
fn normalize_credential_key(key: &str) -> String {
    normalize_key(key)
}

fn normalize_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let mut normalized = String::with_capacity(key.len());
    for (index, character) in chars.iter().copied().enumerate() {
        if character == '-' {
            if !normalized.ends_with('_') {
                normalized.push('_');
            }
            continue;
        }
        if character.is_ascii_uppercase() {
            let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(index + 1).copied();
            let starts_word = previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
            if starts_word && !normalized.ends_with('_') {
                normalized.push('_');
            }
            normalized.push(character.to_ascii_lowercase());
        } else {
            normalized.push(character);
        }
    }
    normalized
}

/// Hygiene guard over the complete retained document: every string and
/// prohibited key anywhere in the packet (payloads, metadata, subject,
/// identity, ledger, run id) is scanned for credentials, machine-local paths,
/// and chain-of-thought keys — a marker outside the payload can no longer
/// survive stamping or validation.
fn scan_document_hygiene(value: &Value, where_: &str, violations: &mut Vec<Violation>) {
    match value {
        Value::String(text) => scan_hygiene(text, where_, violations),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                scan_document_hygiene(item, &format!("{where_}[{index}]"), violations);
            }
        }
        Value::Object(object) => {
            for (key, child) in object {
                let child_where = format!("{where_}.{key}");
                if is_named_key(key, COT_KEYS) {
                    violations.push(Violation::new(
                        "cot_key_in_payload",
                        format!("{child_where}: chain-of-thought key {key} must never be retained"),
                    ));
                }
                if is_credential_key(key) {
                    violations.push(Violation::new(
                        "credential_in_payload",
                        format!(
                            "{child_where}: retained evidence contains a credential field name"
                        ),
                    ));
                }
                scan_document_hygiene(child, &child_where, violations);
            }
        }
        _ => {}
    }
}

fn contains_forbidden_mutable_state(value: &Value, where_: &str, violations: &mut Vec<Violation>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if is_named_key(key, MUTABLE_STATE_KEYS) {
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
    seq_code: &'static str,
    expected_seq: u64,
    violations: &mut Vec<Violation>,
) -> Option<String> {
    let at = format!("{where_}[{expected_seq}]");
    check_unknown_keys(record, allowed_keys, &at, violations);
    match record.get("seq").and_then(Value::as_u64) {
        Some(seq) if seq == expected_seq => {}
        Some(_) | None => violations.push(Violation::new(
            seq_code,
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
    scan_document_hygiene(doc, "manifest", &mut violations);
    if root.get("metadata").is_some_and(|metadata| !metadata.is_object()) {
        violations.push(Violation::new(
            "not_an_object",
            "manifest: metadata must be an object when present".to_string(),
        ));
    }
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
                    if model.contains_key("revision") {
                        require_non_empty(
                            model,
                            "revision",
                            "malformed_subject_field",
                            "subject.model",
                            &mut violations,
                        );
                    } else {
                        violations.push(Violation::new(
                            "missing_subject_field",
                            "subject.model.revision: required model revision was not supplied"
                                .to_string(),
                        ));
                    }
                }
                None => violations.push(Violation::new(
                    "missing_subject_field",
                    "subject: model must be an object with id and revision".to_string(),
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

    // Bounded observable event records; contiguous sequence from 0. An
    // absent required collection, a present-but-empty required collection,
    // and a present-but-mistyped collection all fail closed: the committed
    // schema's minItems/`type` constraints are enforced here, not assumed.
    let mut recompute_records = |records: Option<&Value>,
                                 kinds: &[&str],
                                 keys: &[&str],
                                 required_min: bool,
                                 max_records: usize,
                                 seq_code: &'static str,
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
            Some(Value::Array(records)) => {
                if required_min && records.is_empty() {
                    violations.push(Violation::new(
                        "missing_events",
                        format!("manifest: at least one {label} record is required"),
                    ));
                }
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
                            seq_code,
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
            Some(_) => violations.push(Violation::new(
                "not_an_object",
                format!("manifest: {label} must be an array"),
            )),
        }
        recomputed
    };

    let event_digests = recompute_records(
        root.get("events"),
        EVENT_KINDS,
        EVENT_KEYS,
        true,
        MAX_EVENTS,
        "event_seq_not_contiguous",
        "events",
    );
    let result_digests = recompute_records(
        root.get("results"),
        RESULT_KINDS,
        RESULT_KEYS,
        false,
        MAX_RESULTS,
        "result_seq_not_contiguous",
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
        }
    } else if root.get("human_intervention").is_some() {
        violations.push(Violation::new(
            "not_an_object",
            "human_intervention must be an array".to_string(),
        ));
    }

    // Envelope integrity: the validator recomputes the packet digest from the
    // canonical envelope over the full semantic identity (every identity field
    // except the self-reported digest), the complete subject metadata, and the
    // RECOMPUTED record digests — never trusting the self-reported value.
    let empty_object = Value::Object(Map::new());
    let identity_input = root
        .get("identity")
        .and_then(as_str_map)
        .map(envelope_identity)
        .unwrap_or_else(|| empty_object.clone());
    let subject_input = root.get("subject").cloned().unwrap_or(empty_object);
    let human_intervention_input =
        root.get("human_intervention").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    let recomputed_envelope = envelope_digest(
        &identity_input,
        &subject_input,
        &string_of(root, "run_id"),
        &string_of(root, "disposition"),
        &event_digests,
        &result_digests,
        &human_intervention_input,
    );
    let recorded = root
        .get("identity")
        .and_then(as_str_map)
        .and_then(|identity| identity.get("packet_digest"))
        .and_then(Value::as_str)
        .filter(|digest| is_lowercase_sha256_hex(digest));
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
            let disposition = match doc.get("disposition").and_then(Value::as_str) {
                Some(disposition) if DISPOSITIONS.contains(&disposition) => disposition.to_string(),
                Some(_) => "<redacted-invalid-disposition>".to_string(),
                None => "<missing-disposition>".to_string(),
            };
            let violations = validate_manifest(doc);
            // Invalid packets are still reportable for advisory classification,
            // but none of their untrusted run identity may be copied to stdout.
            let run_id = if violations.is_empty() {
                doc.get("run_id").and_then(Value::as_str).unwrap_or("<missing-run-id>").to_string()
            } else {
                "<redacted-invalid-run-id>".to_string()
            };
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

/// Schema self-check against the committed JSON schema. Pinned here: the
/// `$id`, the draft 2020-12 `$schema`, the four closed enum vocabularies, the
/// property-name set of every Rust key family (root, identity, subject,
/// named_version, model, permissions, event, result, intervention), the
/// required-root set, and the numeric retention bounds (event/result/
/// intervention counts, the events and ceiling `minItems: 1`, and the
/// 64-scope ceiling cap). Property sets are compared order-insensitively.
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
    let at = |path_segments: &[&str]| -> Value {
        let mut value = &schema;
        for segment in path_segments {
            value = value.get(*segment).unwrap_or(&Value::Null);
        }
        value.clone()
    };
    let enum_of = |path_segments: &[&str]| -> Option<Vec<String>> {
        at(path_segments)
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
    };
    let expected_enums: &[(&[&str], &[&str], &str)] = &[
        (&["$defs", "disposition", "enum"], DISPOSITIONS, "dispositions"),
        (&["$defs", "event_kind", "enum"], EVENT_KINDS, "event kinds"),
        (&["$defs", "result_kind", "enum"], RESULT_KINDS, "result kinds"),
        (&["$defs", "intervention_role", "enum"], INTERVENTION_ROLES, "intervention roles"),
    ];
    for (path_segments, expected_values, label) in expected_enums {
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

    // Property-name sets: order-insensitive equality with each Rust key
    // family, so silent vocabulary drift fails the self-check.
    let properties_of = |path_segments: &[&str]| -> Option<Vec<String>> {
        let mut segments = path_segments.to_vec();
        segments.push("properties");
        at(&segments).as_object().map(|properties| {
            let mut names: Vec<String> = properties.keys().cloned().collect();
            names.sort();
            names
        })
    };
    let expected_properties: &[(&[&str], &[&str], &str)] = &[
        (&[], ROOT_KEYS, "root"),
        (&["$defs", "identity"], IDENTITY_KEYS, "identity"),
        (&["$defs", "subject"], SUBJECT_KEYS, "subject"),
        (&["$defs", "named_version"], NAMED_VERSION_KEYS, "named_version"),
        (&["$defs", "subject", "properties", "model"], MODEL_KEYS, "subject.model"),
        (
            &["$defs", "subject", "properties", "permissions"],
            PERMISSIONS_KEYS,
            "subject.permissions",
        ),
        (&["$defs", "event_record"], EVENT_KEYS, "event_record"),
        (&["$defs", "result_record"], RESULT_KEYS, "result_record"),
        (&["$defs", "intervention_entry"], INTERVENTION_KEYS, "intervention_entry"),
    ];
    for (path_segments, expected_values, label) in expected_properties {
        match properties_of(path_segments) {
            Some(mut names) => {
                let mut expected_names: Vec<&str> = expected_values.to_vec();
                expected_names.sort_unstable();
                names.sort();
                if names.iter().map(String::as_str).ne(expected_names.iter().copied()) {
                    violations.push(format!(
                        "schema {label} property set drifted from the pinned vocabulary: {names:?}"
                    ));
                }
            }
            None => violations.push(format!(
                "schema is missing the {label} properties at {}",
                segments_label(path_segments)
            )),
        }
    }

    // Required sets: the closed core's mandatory fields are pinned, including
    // the model revision that makes the executing model identity complete.
    let expected_required_sets: &[(&[&str], &[&str], &str)] = &[
        (
            &["required"],
            &["schema", "schema_version", "run_id", "identity", "subject", "disposition", "events"],
            "root",
        ),
        (
            &["$defs", "subject", "properties", "model", "required"],
            &["id", "revision"],
            "subject.model",
        ),
    ];
    for (path_segments, expected_values, label) in expected_required_sets {
        match at(path_segments).as_array() {
            Some(required) => {
                let mut names: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
                names.sort_unstable();
                let mut expected_names = expected_values.to_vec();
                expected_names.sort_unstable();
                if names.ne(&expected_names) {
                    violations.push(format!(
                        "schema {label} required set drifted from the pinned vocabulary: {names:?}"
                    ));
                }
            }
            None => violations.push(format!(
                "schema is missing the {label} required set at {}",
                segments_label(path_segments)
            )),
        }
    }

    // Numeric bounds: retention caps and non-empty collection minimums.
    let number_of = |path_segments: &[&str]| -> Option<u64> { at(path_segments).as_u64() };
    let expected_bounds: &[(&[&str], u64, &str)] = &[
        (&["properties", "events", "maxItems"], MAX_EVENTS as u64, "events.maxItems"),
        (&["properties", "events", "minItems"], 1, "events.minItems"),
        (&["properties", "results", "maxItems"], MAX_RESULTS as u64, "results.maxItems"),
        (
            &["properties", "human_intervention", "maxItems"],
            MAX_INTERVENTIONS as u64,
            "human_intervention.maxItems",
        ),
        (
            &["$defs", "subject", "properties", "permissions", "properties", "ceiling", "maxItems"],
            64,
            "ceiling.maxItems",
        ),
        (
            &["$defs", "subject", "properties", "permissions", "properties", "ceiling", "minItems"],
            1,
            "ceiling.minItems",
        ),
    ];
    for (path_segments, expected_value, label) in expected_bounds {
        match number_of(path_segments) {
            Some(value) if value == *expected_value => {}
            Some(value) => violations.push(format!(
                "schema bound {label} drifted from the pinned {expected_value}: found {value}"
            )),
            None => violations
                .push(format!("schema is missing the bound {}", segments_label(path_segments))),
        }
    }
    Ok(violations)
}

/// Stable display label for a schema path segment list.
fn segments_label(path_segments: &[&str]) -> String {
    if path_segments.is_empty() {
        "<root>".to_string()
    } else {
        path_segments.join(".")
    }
}

#[cfg(test)]
mod tests;
