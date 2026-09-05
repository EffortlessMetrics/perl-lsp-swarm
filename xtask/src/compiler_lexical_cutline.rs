//! Validator for the compiler lexical cut-line cases manifest (#12156).
//!
//! The manifest at `.spec/12156-compiler-lexical-cutline/manifest.json` is the
//! proof authority for the first compiler-backed lexical references/rename
//! cohort. This module checks stable IDs, the admitted/excluded denominators,
//! protocol-correct preparation semantics, independently authored anchor
//! geometry (byte and UTF-16), plan/projection/application set identity,
//! mutation ownership, work assertions, and deterministic normalized bytes.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

pub const SCHEMA_VERSION: &str = "compiler_lexical_cutline_cases.v1";
pub const MANIFEST_NAME: &str = "compiler-lexical-cutline-cases";
pub const MANIFEST_PATH: &str = ".spec/12156-compiler-lexical-cutline/manifest.json";
pub const SCHEMA_PATH: &str = "schemas/compiler_lexical_cutline_cases.v1.schema.json";

const MANIFEST_TOP_LEVEL_KEYS: &[&str] = &[
    "schema_version",
    "manifest",
    "issue",
    "owner",
    "status",
    "updated",
    "claim_boundary",
    "authorities",
    "protocol_lifecycle",
    "vocabulary",
    "fixtures",
    "work_invariants",
    "cases",
    "mutations",
    "test_targets",
];

const PREPARE_RESULT_SHAPES: &[&str] = &["range", "range_placeholder", "default_behavior", "null"];
const RENAME_PARAMS_FIELDS: &[&str] = &["textDocument", "position", "newName"];
const CORRELATION_OUTCOMES: &[&str] = &[
    "no_prior_preparation",
    "matching_current_preparation",
    "prior_preparation_stale",
    "prior_preparation_different_target",
    "prior_preparation_different_family",
    "prior_preparation_different_document_instance",
    "prior_preparation_different_configuration",
    "prior_preparation_malformed_or_foreign",
    "instrument_failure",
];
const SIGILS: &[&str] = &["scalar", "array", "hash", "code"];
const OCCURRENCE_ROLES: &[&str] = &["declaration", "read", "write", "modify"];
const RESULT_CLASSES: &[&str] = &[
    "exact_nonempty",
    "exact_empty",
    "refusal",
    "not_ready",
    "unsupported",
    "old_route",
    "instrument_failure",
];
const CASE_KINDS: &[&str] = &["admitted", "excluded", "lifecycle"];
const PREPARATION_SCENARIOS: &[&str] = &[
    "no_prior_prepare",
    "matching_prepare",
    "stale_prepare_fresh_success",
    "stale_prepare_current_refusal",
    "close_reopen",
    "cache_miss_eviction",
    "malformed_foreign",
];
const FRESH_CURRENT_RESULTS: &[&str] = &["success", "refusal", "not_ready", "unsupported"];

const REQUIRED_POSITIVE_COVERAGE: &[&str] = &[
    "ordinary_exact_nonempty",
    "declaration_only_exact_empty",
    "declaration_read_write_modify",
    "for_loop_declaration_read",
    "nested_shadowing",
    "same_spelling_other_body",
    "sigil_scalar",
    "sigil_array",
    "sigil_hash",
    "sigil_code",
    "closure_capture",
    "unicode_astral_geometry",
    "crlf_geometry",
    "edit_requery_identical_generation",
    "rename_no_prior_prepare",
    "rename_matching_prepare",
    "stale_prepare_fresh_success",
    "rollback_pre_promotion",
];

const REQUIRED_EXCLUSION_COVERAGE: &[&str] = &[
    "include_declaration_true",
    "bare_destructuring_declaration",
    "package_global",
    "sub_method_import_cross_file",
    "typeglob_symbolic_dynamic",
    "partial_recovered_missing_anchor",
    "alias_localize_tied_magical",
    "stale_held_references_request",
    "wrong_root_workspace_configuration",
    "not_ready_environment",
    "cancellation_deadline_instrument_failure",
    "unprojectable_client_edit",
    "old_text_mismatch",
    "name_collision_invalid_target",
    "malformed_foreign_observation",
    "stale_prepare_current_refusal",
    "close_reopen_observation",
    "preparation_cache_miss_eviction",
];

const REQUIRED_TEST_TARGETS: &[&str] = &[
    "compiler_lexical_cutline_manifest",
    "compiler_references_stdio",
    "compiler_rename_stdio",
    "rename_preparation_correlation",
    "compiler_lexical_cutline_mutations",
];

const MUTATION_COUNT: usize = 37;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CutlineValidationError(String);

impl CutlineValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for CutlineValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CutlineValidationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationStats {
    pub fixtures: usize,
    pub cases: usize,
    pub mutations: usize,
    pub work_invariants: usize,
}

/// Validate raw manifest bytes: full value checks plus deterministic
/// canonical-byte identity (sorted keys, two-space indent, trailing LF).
pub fn validate_manifest_bytes(bytes: &[u8]) -> Result<ValidationStats, CutlineValidationError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| CutlineValidationError::new(format!("manifest: not UTF-8: {error}")))?;
    let value: Value = serde_json::from_str(text)
        .map_err(|error| CutlineValidationError::new(format!("manifest: invalid JSON: {error}")))?;
    let stats = validate_manifest_value(&value)?;
    let canonical = canonical_bytes(&value)?;
    if bytes != canonical.as_slice() {
        return Err(CutlineValidationError::new(
            "manifest: bytes are not the deterministic canonical form \
             (canonical JSON = sorted keys, two-space indent, single trailing LF)",
        ));
    }
    Ok(stats)
}

/// Validate the canonical manifest and its schema file inside a repository
/// root, including the validator test proof path named by the test targets.
pub fn validate_manifest_file(root: &Path) -> Result<ValidationStats, CutlineValidationError> {
    let schema_text = read_repo_text(root, SCHEMA_PATH)?;
    let schema: Value = serde_json::from_str(&schema_text).map_err(|error| {
        CutlineValidationError::new(format!("{SCHEMA_PATH}: invalid JSON: {error}"))
    })?;
    let bytes = read_repo_bytes(root, MANIFEST_PATH)?;
    let stats = validate_manifest_bytes(&bytes)?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| CutlineValidationError::new(format!("manifest: invalid JSON: {error}")))?;
    let mut violations = Vec::new();
    // The schema is the structural authority for this proof contract, so it
    // is actually applied: parsing it as JSON alone would let a structurally
    // invalid manifest (missing owner, claim_boundary, nested authority
    // fields, ...) pass the advertised validation command.
    let validator = jsonschema::validator_for(&schema).map_err(|error| {
        CutlineValidationError::new(format!("{SCHEMA_PATH}: invalid schema: {error}"))
    })?;
    for error in validator.iter_errors(&value) {
        violations.push(format!("manifest: schema violation: {error}"));
    }
    validate_test_target_proof_paths(root, &value, &mut violations);
    finish(violations)?;
    Ok(stats)
}

/// Load the canonical manifest value for `list`/`explain`.
pub fn load_manifest(root: &Path) -> Result<Value, CutlineValidationError> {
    let bytes = read_repo_bytes(root, MANIFEST_PATH)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| CutlineValidationError::new(format!("manifest: invalid JSON: {error}")))
}

/// List stable case IDs in manifest order.
pub fn list_case_ids(manifest: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(cases) = manifest.get("cases").and_then(Value::as_array) {
        for case in cases {
            if let Some(id) = case.get("case_id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

/// Render one case row as pretty JSON for `explain`.
pub fn explain_case(manifest: &Value, case_id: &str) -> Option<String> {
    let cases = manifest.get("cases")?.as_array()?;
    for case in cases {
        if case.get("case_id").and_then(Value::as_str) == Some(case_id) {
            return serde_json::to_string_pretty(case).ok();
        }
    }
    None
}

/// Validate one parsed manifest value (all structural and semantic checks;
/// canonical-byte identity is checked by [`validate_manifest_bytes`]).
pub fn validate_manifest_value(
    manifest: &Value,
) -> Result<ValidationStats, CutlineValidationError> {
    let mut violations = Vec::new();
    let Some(root) = as_object(manifest, "manifest", &mut violations) else {
        finish(violations)?;
        return Err(CutlineValidationError::new("manifest: expected object"));
    };
    reject_unknown_keys(root, MANIFEST_TOP_LEVEL_KEYS, "manifest", &mut violations);
    require_const(root, "schema_version", SCHEMA_VERSION, "manifest", &mut violations);
    require_const(root, "manifest", MANIFEST_NAME, "manifest", &mut violations);
    require_const(root, "status", "proof-definition", "manifest", &mut violations);
    if manifest.get("issue").and_then(Value::as_u64) != Some(12156) {
        violations.push("manifest.issue: expected `12156`".to_string());
    }
    validate_protocol(manifest, &mut violations);
    validate_vocabulary(manifest, &mut violations);
    let fixtures = validate_fixtures(manifest, &mut violations);
    let invariants = validate_work_invariants(manifest, &mut violations);
    let case_index = validate_cases(manifest, &fixtures, &invariants, &mut violations);
    validate_mutations(manifest, &case_index, &mut violations);
    validate_test_targets(manifest, &mut violations);
    finish(violations)?;
    Ok(ValidationStats {
        fixtures: fixtures.len(),
        cases: case_index.len(),
        mutations: MUTATION_COUNT,
        work_invariants: invariants.len(),
    })
}

fn canonical_bytes(value: &Value) -> Result<Vec<u8>, CutlineValidationError> {
    let mut text = serde_json::to_string_pretty(value).map_err(|error| {
        CutlineValidationError::new(format!("manifest: cannot serialize canonical form: {error}"))
    })?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn read_repo_bytes(root: &Path, rel: &str) -> Result<Vec<u8>, CutlineValidationError> {
    fs::read(root.join(rel))
        .map_err(|error| CutlineValidationError::new(format!("{rel}: cannot read: {error}")))
}

fn read_repo_text(root: &Path, rel: &str) -> Result<String, CutlineValidationError> {
    fs::read_to_string(root.join(rel))
        .map_err(|error| CutlineValidationError::new(format!("{rel}: cannot read: {error}")))
}

fn finish(violations: Vec<String>) -> Result<(), CutlineValidationError> {
    if violations.is_empty() {
        return Ok(());
    }
    let mut message = format!(
        "compiler lexical cut-line manifest check failed with {} violation(s):",
        violations.len()
    );
    for violation in &violations {
        message.push_str("\n  - ");
        message.push_str(violation);
    }
    Err(CutlineValidationError::new(message))
}

// --- shared value helpers ----------------------------------------------------

fn as_object<'a>(
    value: &'a Value,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    match value.as_object() {
        Some(object) => Some(object),
        None => {
            violations.push(format!("{path}: expected object"));
            None
        }
    }
}

fn child_object<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    let Some(value) = parent.get(key) else {
        violations.push(format!("{path}.{key}: missing required field"));
        return None;
    };
    as_object(value, &format!("{path}.{key}"), violations)
}

fn opt_string<'a>(parent: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    parent.get(key).and_then(Value::as_str)
}

fn require_string<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<&'a str> {
    let value = opt_string(parent, key);
    match value {
        Some(text) if !text.trim().is_empty() => Some(text),
        Some(_) => {
            violations.push(format!("{path}.{key}: must not be empty"));
            None
        }
        None => {
            violations.push(format!("{path}.{key}: missing or not a string"));
            None
        }
    }
}

fn require_const(
    parent: &Map<String, Value>,
    key: &str,
    expected: &str,
    path: &str,
    violations: &mut Vec<String>,
) {
    match require_string(parent, key, path, violations) {
        Some(actual) if actual != expected => {
            violations.push(format!("{path}.{key}: expected `{expected}`, found `{actual}`"));
        }
        _ => {}
    }
}

fn reject_unknown_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
    path: &str,
    violations: &mut Vec<String>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            violations.push(format!("{path}: unknown field `{key}`"));
        }
    }
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    if let Some(array) = value.and_then(Value::as_array) {
        for item in array {
            if let Some(text) = item.as_str() {
                set.insert(text.to_string());
            }
        }
    }
    set
}

fn require_exact_string_set(
    parent: &Map<String, Value>,
    key: &str,
    expected: &[&str],
    path: &str,
    violations: &mut Vec<String>,
) {
    let actual = string_set(parent.get(key));
    let expected_set: BTreeSet<String> = expected.iter().map(|item| item.to_string()).collect();
    if actual != expected_set {
        violations
            .push(format!("{path}.{key}: expected exactly {expected_set:?}, found {actual:?}"));
    }
}

fn get_u64(parent: &Map<String, Value>, key: &str) -> Option<u64> {
    parent.get(key).and_then(Value::as_u64)
}

// --- protocol and vocabulary -------------------------------------------------

fn validate_protocol(manifest: &Value, violations: &mut Vec<String>) {
    let Some(root) = manifest.as_object() else { return };
    let Some(protocol) = child_object(root, "protocol_lifecycle", "manifest", violations) else {
        return;
    };
    let path = "manifest.protocol_lifecycle";
    reject_unknown_keys(
        protocol,
        &[
            "prepare_rename_result_shapes",
            "rename_params_fields",
            "client_carried_continuation_token",
            "stale_prior_observation_rule",
            "correlation_outcomes",
        ],
        path,
        violations,
    );
    require_exact_string_set(
        protocol,
        "prepare_rename_result_shapes",
        PREPARE_RESULT_SHAPES,
        path,
        violations,
    );
    require_exact_string_set(
        protocol,
        "rename_params_fields",
        RENAME_PARAMS_FIELDS,
        path,
        violations,
    );
    require_const(protocol, "client_carried_continuation_token", "forbidden", path, violations);
    require_exact_string_set(
        protocol,
        "correlation_outcomes",
        CORRELATION_OUTCOMES,
        path,
        violations,
    );
    require_string(protocol, "stale_prior_observation_rule", path, violations);
}

fn validate_vocabulary(manifest: &Value, violations: &mut Vec<String>) {
    let Some(root) = manifest.as_object() else { return };
    let Some(vocabulary) = child_object(root, "vocabulary", "manifest", violations) else {
        return;
    };
    let path = "manifest.vocabulary";
    reject_unknown_keys(
        vocabulary,
        &["sigils", "occurrence_roles", "result_classes", "case_kinds", "position_encoding"],
        path,
        violations,
    );
    require_exact_string_set(vocabulary, "sigils", SIGILS, path, violations);
    require_exact_string_set(vocabulary, "occurrence_roles", OCCURRENCE_ROLES, path, violations);
    require_exact_string_set(vocabulary, "result_classes", RESULT_CLASSES, path, violations);
    require_exact_string_set(vocabulary, "case_kinds", CASE_KINDS, path, violations);
    require_const(vocabulary, "position_encoding", "utf16", path, violations);
}

// --- fixtures ----------------------------------------------------------------

#[derive(Clone, Debug)]
struct FixtureInfo {
    source: String,
}

fn validate_fixtures(
    manifest: &Value,
    violations: &mut Vec<String>,
) -> BTreeMap<String, FixtureInfo> {
    let mut fixtures = BTreeMap::new();
    let Some(root) = manifest.as_object() else { return fixtures };
    let Some(array) = root.get("fixtures").and_then(Value::as_array) else {
        violations.push("manifest.fixtures: missing or not an array".to_string());
        return fixtures;
    };
    if array.is_empty() {
        violations.push("manifest.fixtures: must not be empty".to_string());
    }
    for (index, value) in array.iter().enumerate() {
        let path = format!("manifest.fixtures[{index}]");
        let Some(fixture) = as_object(value, &path, violations) else { continue };
        reject_unknown_keys(
            fixture,
            &["id", "line_ending", "source", "source_sha256"],
            &path,
            violations,
        );
        let Some(id) = require_string(fixture, "id", &path, violations) else { continue };
        let id = id.to_string();
        if fixtures.contains_key(&id) {
            violations.push(format!("{path}: duplicate fixture id `{id}`"));
            continue;
        }
        match opt_string(fixture, "line_ending") {
            Some("LF") | Some("CRLF") => {}
            _ => violations.push(format!("{path}.line_ending: expected `LF` or `CRLF`")),
        }
        let Some(source) = require_string(fixture, "source", &path, violations) else {
            continue;
        };
        let source = source.to_string();
        if let Some(recorded) = require_string(fixture, "source_sha256", &path, violations) {
            let digest = Sha256::digest(source.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if recorded != digest {
                violations.push(format!(
                    "{path}.source_sha256: digest mismatch; source bytes do not match the recorded digest"
                ));
            }
        }
        fixtures.insert(id, FixtureInfo { source });
    }
    fixtures
}

// --- geometry ------------------------------------------------------------------

/// Compute the zero-based line and UTF-16 column of a byte offset, matching
/// the LSP UTF-16 position encoding over both LF and CRLF text.
fn line_char(source: &str, byte_offset: usize) -> Option<(u64, u64)> {
    if byte_offset > source.len() || !source.is_char_boundary(byte_offset) {
        return None;
    }
    let prefix = &source[..byte_offset];
    let line = prefix.matches('\n').count() as u64;
    let column_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = source[column_start..byte_offset].chars().map(|ch| ch.len_utf16() as u64).sum();
    Some((line, column))
}

/// Resolve an LSP UTF-16 position back to a byte offset.
fn byte_offset_for_position(source: &str, line: u64, character: u64) -> Option<usize> {
    let mut current_line = 0_u64;
    let mut line_start = 0_usize;
    for (index, ch) in source.char_indices() {
        if current_line == line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = index + 1;
        }
    }
    if current_line != line {
        return None;
    }
    let mut column = 0_u64;
    for (relative, ch) in source[line_start..].char_indices() {
        if ch == '\n' {
            return None;
        }
        if column == character {
            return Some(line_start + relative);
        }
        column += ch.len_utf16() as u64;
    }
    if column == character { Some(source.len()) } else { None }
}

fn sigil_prefix(sigil: &str) -> &'static str {
    match sigil {
        "scalar" => "$",
        "array" => "@",
        "hash" => "%",
        _ => "",
    }
}

struct AnchorData {
    byte_start: u64,
    byte_end: u64,
    role: String,
}

fn read_anchor(value: &Value, path: &str, violations: &mut Vec<String>) -> Option<AnchorData> {
    let anchor = as_object(value, path, violations)?;
    reject_unknown_keys(
        anchor,
        &["byte_start", "byte_end", "line", "character_start", "character_end", "role"],
        path,
        violations,
    );
    let byte_start = require_u64(anchor, "byte_start", path, violations);
    let byte_end = require_u64(anchor, "byte_end", path, violations);
    require_u64(anchor, "line", path, violations);
    require_u64(anchor, "character_start", path, violations);
    require_u64(anchor, "character_end", path, violations);
    let role = opt_string(anchor, "role").unwrap_or_default().to_string();
    let (byte_start, byte_end) = match (byte_start, byte_end) {
        (Some(start), Some(end)) => (start, end),
        _ => return None,
    };
    if byte_end <= byte_start {
        violations.push(format!("{path}: byte_end must exceed byte_start"));
    }
    if !OCCURRENCE_ROLES.contains(&role.as_str()) {
        violations.push(format!("{path}.role: unsupported value `{role}`"));
    }
    Some(AnchorData { byte_start, byte_end, role })
}

fn require_u64(
    parent: &Map<String, Value>,
    key: &str,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<u64> {
    match get_u64(parent, key) {
        Some(value) => Some(value),
        None => {
            violations.push(format!("{path}.{key}: missing or not an unsigned integer"));
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_anchor_geometry(
    anchor: &AnchorData,
    anchor_value: &Map<String, Value>,
    source: &str,
    expected_text: &str,
    path: &str,
    violations: &mut Vec<String>,
) {
    let start = anchor.byte_start as usize;
    let end = anchor.byte_end as usize;
    if end > source.len() {
        violations.push(format!("{path}: byte range {start}..{end} exceeds fixture source"));
        return;
    }
    let Some(slice) = source.get(start..end) else {
        violations.push(format!("{path}: byte range {start}..{end} splits a UTF-8 boundary"));
        return;
    };
    if slice != expected_text {
        violations.push(format!(
            "{path}: byte range {start}..{end} selects `{slice}`, expected `{expected_text}`"
        ));
    }
    let Some((line, character_start)) = line_char(source, start) else {
        violations.push(format!("{path}: cannot derive position for byte {start}"));
        return;
    };
    let Some((end_line, character_end)) = line_char(source, end) else {
        violations.push(format!("{path}: cannot derive position for byte {end}"));
        return;
    };
    if line != end_line {
        violations.push(format!("{path}: anchor spans lines {line}..{end_line}"));
    }
    if get_u64(anchor_value, "line") != Some(line)
        || get_u64(anchor_value, "character_start") != Some(character_start)
        || get_u64(anchor_value, "character_end") != Some(character_end)
    {
        violations.push(format!(
            "{path}: recorded UTF-16 position does not match the byte-derived position \
             (line {line}, characters {character_start}..{character_end})"
        ));
    }
}

// --- work invariants -----------------------------------------------------------

fn validate_work_invariants(manifest: &Value, violations: &mut Vec<String>) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let Some(root) = manifest.as_object() else { return ids };
    let Some(array) = root.get("work_invariants").and_then(Value::as_array) else {
        violations.push("manifest.work_invariants: missing or not an array".to_string());
        return ids;
    };
    if array.is_empty() {
        violations.push("manifest.work_invariants: must not be empty".to_string());
    }
    for (index, value) in array.iter().enumerate() {
        let path = format!("manifest.work_invariants[{index}]");
        let Some(invariant) = as_object(value, &path, violations) else { continue };
        reject_unknown_keys(
            invariant,
            &["id", "stage", "authority", "subject", "assertion", "instrument", "status", "note"],
            &path,
            violations,
        );
        let Some(id) = require_string(invariant, "id", &path, violations) else { continue };
        if !ids.insert(id.to_string()) {
            violations.push(format!("{path}: duplicate work-invariant id `{id}`"));
        }
        require_string(invariant, "stage", &path, violations);
        require_string(invariant, "authority", &path, violations);
        require_string(invariant, "subject", &path, violations);
        let assertion = opt_string(invariant, "assertion").unwrap_or_default();
        if !["zero", "false", "identity"].contains(&assertion) {
            violations.push(format!("{path}.assertion: unsupported value `{assertion}`"));
        }
        let status = opt_string(invariant, "status").unwrap_or_default();
        if !["active", "pending"].contains(&status) {
            violations.push(format!("{path}.status: unsupported value `{status}`"));
        }
        // Unknown or uninstrumented work is never numeric zero: a zero/false/
        // identity claim must name its instrument.
        if matches!(assertion, "zero" | "false" | "identity") {
            require_string(invariant, "instrument", &path, violations);
        }
        // Pending is reserved for final #4306 old-work zeroes and must say so;
        // a pending claim is not a numeric zero.
        if status == "pending" {
            if assertion != "zero" {
                violations.push(format!(
                    "{path}: only zero assertions may be pending; pending is reserved \
                     for final #4306 old-work zeroes"
                ));
            }
            match opt_string(invariant, "note") {
                Some(note) if note.contains("#4306") => {}
                _ => violations.push(format!(
                    "{path}: pending assertion requires a note naming #4306; \
                     unknown/uninstrumented work is not numeric zero"
                )),
            }
        }
    }
    ids
}

// --- cases ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CaseInfo {
    mutations: BTreeSet<String>,
}

#[allow(clippy::too_many_lines)]
fn validate_cases(
    manifest: &Value,
    fixtures: &BTreeMap<String, FixtureInfo>,
    invariants: &BTreeSet<String>,
    violations: &mut Vec<String>,
) -> BTreeMap<String, CaseInfo> {
    let mut index = BTreeMap::new();
    let Some(root) = manifest.as_object() else { return index };
    let Some(array) = root.get("cases").and_then(Value::as_array) else {
        violations.push("manifest.cases: missing or not an array".to_string());
        return index;
    };
    if array.is_empty() {
        violations.push("manifest.cases: must not be empty".to_string());
    }
    // Coverage is polarity-separated, not kind-exclusive: admitted rows feed
    // the admitted denominator, excluded rows feed the exclusion denominator,
    // and lifecycle rows feed both (their scenarios demonstrate positive
    // preparation outcomes and boundary/refusal outcomes by design). A
    // positive tag on an excluded row or an exclusion tag on an admitted row
    // is a polarity violation — one shared set would let a tag move across
    // polarities without failing validation.
    let mut admitted_coverage = BTreeSet::new();
    let mut exclusion_coverage = BTreeSet::new();
    let mut scenarios_seen = BTreeSet::new();
    for (i, value) in array.iter().enumerate() {
        let path = format!("manifest.cases[{i}]");
        let Some(case) = as_object(value, &path, violations) else { continue };
        reject_unknown_keys(
            case,
            &[
                "case_id",
                "kind",
                "coverage",
                "summary",
                "fixture",
                "request",
                "binding",
                "expected",
                "preparation",
                "rename",
                "lifecycle",
                "generation",
                "work_invariants",
                "adjacent_boundary",
                "mutations",
            ],
            &path,
            violations,
        );
        let Some(case_id) = require_string(case, "case_id", &path, violations) else { continue };
        let case_id = case_id.to_string();
        let path = format!("manifest.cases[{i}]:{case_id}");
        if index.contains_key(&case_id) {
            violations.push(format!("{path}: duplicate case id"));
        }
        let kind = opt_string(case, "kind").unwrap_or_default().to_string();
        if !CASE_KINDS.contains(&kind.as_str()) {
            violations.push(format!("{path}.kind: unsupported value `{kind}`"));
        }
        let prefix_ok = match case_id.split('-').nth(1) {
            Some("POS") | Some("RN") => kind == "admitted",
            Some("EXC") => kind == "excluded",
            Some("LC") => kind == "lifecycle",
            _ => false,
        };
        if !prefix_ok {
            violations.push(format!("{path}: case id prefix does not match kind `{kind}`"));
        }
        for tag in string_set(case.get("coverage")) {
            if kind == "excluded" && REQUIRED_POSITIVE_COVERAGE.contains(&tag.as_str()) {
                violations.push(format!(
                    "{path}.coverage: admitted-denominator tag `{tag}` sits on an excluded row"
                ));
            }
            if kind == "admitted" && REQUIRED_EXCLUSION_COVERAGE.contains(&tag.as_str()) {
                violations.push(format!(
                    "{path}.coverage: exclusion-denominator tag `{tag}` sits on an admitted row"
                ));
            }
            match kind.as_str() {
                "admitted" => {
                    admitted_coverage.insert(tag);
                }
                "excluded" => {
                    exclusion_coverage.insert(tag);
                }
                _ => {
                    admitted_coverage.insert(tag.clone());
                    exclusion_coverage.insert(tag);
                }
            }
        }
        require_string(case, "summary", &path, violations);
        require_string(case, "adjacent_boundary", &path, violations);

        let fixture_id = opt_string(case, "fixture").unwrap_or_default().to_string();
        let fixture = fixtures.get(&fixture_id);
        if fixture.is_none() {
            violations.push(format!("{path}.fixture: unknown fixture `{fixture_id}`"));
        }

        for invariant in string_set(case.get("work_invariants")) {
            if !invariants.contains(&invariant) {
                violations.push(format!("{path}.work_invariants: unknown invariant `{invariant}`"));
            }
        }

        let mutations = string_set(case.get("mutations"));
        index.insert(case_id.clone(), CaseInfo { mutations });

        let binding = validate_binding(case, &path, violations);
        if let Some(fixture_info) = fixture {
            validate_request(case, &kind, &binding, fixture_info, &path, violations);
            validate_expected(case, &kind, &binding, fixture_info, &path, violations);
            validate_rename(case, &binding, fixture_info, &path, violations);
        }
        if let Some(scenario) = validate_preparation(case, fixtures, &path, violations) {
            scenarios_seen.insert(scenario);
        }
    }
    for tag in REQUIRED_POSITIVE_COVERAGE {
        if !admitted_coverage.contains(*tag) {
            violations
                .push(format!("manifest.cases: admitted denominator missing coverage `{tag}`"));
        }
    }
    for tag in REQUIRED_EXCLUSION_COVERAGE {
        if !exclusion_coverage.contains(*tag) {
            violations
                .push(format!("manifest.cases: exclusion denominator missing coverage `{tag}`"));
        }
    }
    for scenario in PREPARATION_SCENARIOS {
        if !scenarios_seen.contains(*scenario) {
            violations.push(format!(
                "manifest.cases: preparation scenario `{scenario}` has no row; no-prepare, \
                 matching-prepare, stale/fresh-success, stale/current-refusal, close-reopen, \
                 cache-miss, and malformed-foreign rows must stay distinct"
            ));
        }
    }
    index
}

#[derive(Clone, Debug)]
struct BindingData {
    sigil: String,
    name: String,
}

fn validate_binding(
    case: &Map<String, Value>,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<BindingData> {
    let value = case.get("binding")?;
    if value.is_null() {
        return None;
    }
    let binding = as_object(value, &format!("{path}.binding"), violations)?;
    reject_unknown_keys(
        binding,
        &["sigil", "name", "body", "scope_path"],
        &format!("{path}.binding"),
        violations,
    );
    let sigil = require_string(binding, "sigil", &format!("{path}.binding"), violations)?;
    if !SIGILS.contains(&sigil) {
        violations.push(format!("{path}.binding.sigil: unsupported value `{sigil}`"));
    }
    let name = require_string(binding, "name", &format!("{path}.binding"), violations)?;
    require_string(binding, "body", &format!("{path}.binding"), violations);
    Some(BindingData { sigil: sigil.to_string(), name: name.to_string() })
}

fn validate_request(
    case: &Map<String, Value>,
    kind: &str,
    binding: &Option<BindingData>,
    fixture: &FixtureInfo,
    path: &str,
    violations: &mut Vec<String>,
) {
    let Some(request) = child_object(case, "request", path, violations) else { return };
    let request_path = format!("{path}.request");
    reject_unknown_keys(
        request,
        &["method", "subject", "include_declaration", "new_name"],
        &request_path,
        violations,
    );
    let method = opt_string(request, "method").unwrap_or_default();
    if !["textDocument/references", "textDocument/rename"].contains(&method) {
        violations.push(format!("{request_path}.method: unsupported value `{method}`"));
    }
    let include_declaration = request.get("include_declaration").and_then(Value::as_bool);
    if include_declaration.is_none() {
        violations.push(format!("{request_path}.include_declaration: expected boolean"));
    }
    // The admitted denominator is exactly includeDeclaration=false for
    // references; includeDeclaration=true stays an excluded old-route row.
    if method == "textDocument/references"
        && kind == "admitted"
        && include_declaration != Some(false)
    {
        violations.push(format!(
            "{request_path}.include_declaration: admitted references rows must use false"
        ));
    }
    if method == "textDocument/rename" && opt_string(request, "new_name").is_none() {
        violations.push(format!("{request_path}.new_name: rename requests must name the target"));
    }
    if binding.is_some() {
        if let Some(subject) = request.get("subject").and_then(Value::as_object) {
            let line = get_u64(subject, "line");
            let character = get_u64(subject, "character");
            match (line, character) {
                (Some(line), Some(character)) => {
                    if byte_offset_for_position(&fixture.source, line, character).is_none() {
                        violations.push(format!(
                            "{request_path}.subject: position does not resolve inside the fixture source"
                        ));
                    }
                }
                _ => violations
                    .push(format!("{request_path}.subject: expected line/character integers")),
            }
        } else {
            violations.push(format!("{request_path}.subject: missing or not an object"));
        }
    }
}

#[allow(clippy::too_many_lines)]
fn validate_expected(
    case: &Map<String, Value>,
    kind: &str,
    binding: &Option<BindingData>,
    fixture: &FixtureInfo,
    path: &str,
    violations: &mut Vec<String>,
) {
    let Some(expected) = child_object(case, "expected", path, violations) else { return };
    let expected_path = format!("{path}.expected");
    reject_unknown_keys(
        expected,
        &[
            "result_class",
            "declaration_anchor",
            "reference_locations",
            "completeness",
            "fallback_invoked",
        ],
        &expected_path,
        violations,
    );
    let result_class = opt_string(expected, "result_class").unwrap_or_default();
    if !RESULT_CLASSES.contains(&result_class) {
        violations
            .push(format!("{expected_path}.result_class: unsupported value `{result_class}`"));
    }
    let completeness = opt_string(expected, "completeness").unwrap_or_default();
    if !["exact", "not-applicable"].contains(&completeness) {
        violations
            .push(format!("{expected_path}.completeness: unsupported value `{completeness}`"));
    }
    // Exact answers require exact completeness; exact empty is only ever an
    // exact answer, never a partial-facts answer.
    if matches!(result_class, "exact_nonempty" | "exact_empty") && completeness != "exact" {
        violations.push(format!(
            "{expected_path}.completeness: `{result_class}` requires completeness `exact`"
        ));
    }
    let fallback_invoked = expected.get("fallback_invoked").and_then(Value::as_bool);
    if fallback_invoked.is_none() {
        violations.push(format!("{expected_path}.fallback_invoked: expected boolean"));
    }
    if kind == "admitted" && fallback_invoked == Some(true) {
        violations
            .push(format!("{expected_path}.fallback_invoked: admitted rows never invoke fallback"));
    }

    let expected_text =
        binding.as_ref().map(|data| format!("{}{}", sigil_prefix(&data.sigil), data.name));

    let mut declaration_range = None;
    match expected.get("declaration_anchor") {
        Some(Value::Null) | None => {
            if kind == "admitted" {
                violations.push(format!(
                    "{expected_path}.declaration_anchor: admitted rows record the compiler-owned anchor"
                ));
            }
        }
        Some(anchor_value) => {
            if let (Some(anchor), Some(anchor_object)) = (
                read_anchor(
                    anchor_value,
                    &format!("{expected_path}.declaration_anchor"),
                    violations,
                ),
                anchor_value.as_object(),
            ) {
                if anchor.role != "declaration" {
                    violations.push(format!(
                        "{expected_path}.declaration_anchor: role must be `declaration`"
                    ));
                }
                if let Some(text) = &expected_text {
                    validate_anchor_geometry(
                        &anchor,
                        anchor_object,
                        &fixture.source,
                        text,
                        &format!("{expected_path}.declaration_anchor"),
                        violations,
                    );
                }
                declaration_range = Some((anchor.byte_start, anchor.byte_end));
            }
        }
    }

    let mut reference_ranges = Vec::new();
    match expected.get("reference_locations").and_then(Value::as_array) {
        None => {
            violations.push(format!("{expected_path}.reference_locations: missing or not an array"))
        }
        Some(locations) => {
            if result_class == "exact_empty" && !locations.is_empty() {
                violations.push(format!(
                    "{expected_path}.reference_locations: exact_empty rows must be empty"
                ));
            }
            if result_class == "exact_nonempty" && locations.is_empty() {
                violations.push(format!(
                    "{expected_path}.reference_locations: exact_nonempty rows must not be empty"
                ));
            }
            for (index, location) in locations.iter().enumerate() {
                let location_path = format!("{expected_path}.reference_locations[{index}]");
                let Some(anchor) = read_anchor(location, &location_path, violations) else {
                    continue;
                };
                if anchor.role == "declaration" {
                    violations.push(format!(
                        "{location_path}: the declaration never appears in reference_locations"
                    ));
                }
                if declaration_range == Some((anchor.byte_start, anchor.byte_end)) {
                    violations.push(format!(
                        "{location_path}: duplicates the declaration anchor while includeDeclaration=false"
                    ));
                }
                if reference_ranges.contains(&(anchor.byte_start, anchor.byte_end)) {
                    violations.push(format!(
                        "{location_path}: duplicate occurrence range inflates the exact denominator"
                    ));
                }
                if let (Some(text), Some(anchor_object)) = (&expected_text, location.as_object()) {
                    validate_anchor_geometry(
                        &anchor,
                        anchor_object,
                        &fixture.source,
                        text,
                        &location_path,
                        violations,
                    );
                }
                reference_ranges.push((anchor.byte_start, anchor.byte_end));
            }
        }
    }

    // The request subject must resolve inside the declaration or one of the
    // reference occurrences for admitted rows with a binding.
    if kind != "admitted" || binding.is_none() {
        return;
    }
    if let Some(subject) = case
        .get("request")
        .and_then(Value::as_object)
        .and_then(|request| request.get("subject"))
        .and_then(Value::as_object)
        && let (Some(line), Some(character)) =
            (get_u64(subject, "line"), get_u64(subject, "character"))
        && let Some(byte) = byte_offset_for_position(&fixture.source, line, character)
    {
        let byte = byte as u64;
        let mut ranges = reference_ranges;
        if let Some(range) = declaration_range {
            ranges.push(range);
        }
        if !ranges.iter().any(|(start, end)| byte >= *start && byte < *end) {
            violations.push(format!(
                "{path}.request.subject: position does not land on the binding's \
                 declaration or occurrences"
            ));
        }
    }
}

fn validate_preparation(
    case: &Map<String, Value>,
    fixtures: &BTreeMap<String, FixtureInfo>,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<String> {
    let value = case.get("preparation")?;
    if value.is_null() {
        return None;
    }
    let prep_path = format!("{path}.preparation");
    let preparation = as_object(value, &prep_path, violations)?;
    reject_unknown_keys(
        preparation,
        &[
            "scenario",
            "prior_observation",
            "correlation_outcome",
            "old_plan_reuse",
            "fresh_current_result",
            "prior_fixture",
        ],
        &prep_path,
        violations,
    );
    let scenario = opt_string(preparation, "scenario").unwrap_or_default();
    if !PREPARATION_SCENARIOS.contains(&scenario) {
        violations.push(format!("{prep_path}.scenario: unsupported value `{scenario}`"));
    }
    let outcome = opt_string(preparation, "correlation_outcome").unwrap_or_default();
    if !CORRELATION_OUTCOMES.contains(&outcome) {
        violations.push(format!("{prep_path}.correlation_outcome: unsupported value `{outcome}`"));
    }
    // Old plan reuse is always forbidden; staleness never authorizes reuse.
    require_const(preparation, "old_plan_reuse", "forbidden", &prep_path, violations);
    let fresh = opt_string(preparation, "fresh_current_result").unwrap_or_default();
    if !FRESH_CURRENT_RESULTS.contains(&fresh) {
        violations.push(format!("{prep_path}.fresh_current_result: unsupported value `{fresh}`"));
    }
    require_string(preparation, "prior_observation", &prep_path, violations);
    if let Some(prior_fixture) = opt_string(preparation, "prior_fixture")
        && !fixtures.contains_key(prior_fixture)
    {
        violations.push(format!("{prep_path}.prior_fixture: unknown fixture `{prior_fixture}`"));
    }
    Some(scenario.to_string())
}

#[allow(clippy::too_many_lines)]
fn validate_rename(
    case: &Map<String, Value>,
    binding: &Option<BindingData>,
    fixture: &FixtureInfo,
    path: &str,
    violations: &mut Vec<String>,
) {
    let Some(value) = case.get("rename") else { return };
    if value.is_null() {
        return;
    }
    let rename_path = format!("{path}.rename");
    let Some(rename) = as_object(value, &rename_path, violations) else { return };
    reject_unknown_keys(
        rename,
        &[
            "new_name",
            "authorization",
            "plan_result",
            "rename_outcome",
            "authorized_occurrence_ids",
            "plan_edit_ids",
            "projected_edit_ids",
            "applied_edit_ids",
            "edits",
            "postcondition_source",
            "client_application",
        ],
        &rename_path,
        violations,
    );
    // Rename authorization is always the fresh current subject, never a prior
    // preparation observation or a returned range/placeholder.
    require_const(rename, "authorization", "current-subject-#10650", &rename_path, violations);
    let new_name = require_string(rename, "new_name", &rename_path, violations)
        .unwrap_or_default()
        .to_string();
    require_string(rename, "plan_result", &rename_path, violations);
    let outcome = opt_string(rename, "rename_outcome").unwrap_or_default();
    if !["success", "refusal", "instrument_failure"].contains(&outcome) {
        violations.push(format!("{rename_path}.rename_outcome: unsupported value `{outcome}`"));
    }
    let application = opt_string(rename, "client_application").unwrap_or_default();
    if !["applied-verified", "rejected", "not-applicable"].contains(&application) {
        violations
            .push(format!("{rename_path}.client_application: unsupported value `{application}`"));
    }

    let authorized = string_set(rename.get("authorized_occurrence_ids"));
    let planned = string_set(rename.get("plan_edit_ids"));
    let projected = string_set(rename.get("projected_edit_ids"));
    let applied = string_set(rename.get("applied_edit_ids"));
    let mut edit_ids = BTreeSet::new();
    let mut edits = Vec::new();
    if let Some(array) = rename.get("edits").and_then(Value::as_array) {
        for (index, edit) in array.iter().enumerate() {
            let edit_path = format!("{rename_path}.edits[{index}]");
            let Some(edit_object) = as_object(edit, &edit_path, violations) else { continue };
            reject_unknown_keys(
                edit_object,
                &["id", "byte_start", "byte_end"],
                &edit_path,
                violations,
            );
            if let Some(id) = require_string(edit_object, "id", &edit_path, violations) {
                edit_ids.insert(id.to_string());
            }
            match (get_u64(edit_object, "byte_start"), get_u64(edit_object, "byte_end")) {
                (Some(start), Some(end)) if end > start => edits.push((start, end)),
                _ => violations.push(format!("{edit_path}: invalid byte range")),
            }
        }
    } else {
        violations.push(format!("{rename_path}.edits: missing or not an array"));
    }

    if outcome == "success" {
        // Set identity: authorized occurrences, plan edits, and projected wire
        // edits are one set; a subset or superset is a falsifier.
        if authorized != planned || planned != projected || projected != edit_ids {
            violations.push(format!(
                "{rename_path}: authorized occurrence IDs, plan edit IDs, projected edit IDs, \
                 and edit IDs must be identical on success"
            ));
        }
        if edits.is_empty() {
            violations.push(format!("{rename_path}.edits: success rows must plan edits"));
        }
        if application == "applied-verified" && applied != projected {
            violations.push(format!(
                "{rename_path}.applied_edit_ids: verified application applies exactly the projected set"
            ));
        }
        if application != "applied-verified" && !applied.is_empty() {
            violations.push(format!(
                "{rename_path}.applied_edit_ids: only applied-verified rows record applied edits"
            ));
        }
        // Every edit selects the binding text and the applied edits reproduce
        // the declared postcondition source.
        if let Some(data) = binding {
            let expected_text = format!("{}{}", sigil_prefix(&data.sigil), data.name);
            for (start, end) in &edits {
                let slice = fixture.source.get(*start as usize..*end as usize);
                if slice != Some(expected_text.as_str()) {
                    violations.push(format!(
                        "{rename_path}.edits: range {start}..{end} selects `{:?}`, expected `{expected_text}`",
                        slice.unwrap_or("<invalid>")
                    ));
                }
            }
            if application == "applied-verified" {
                let replacement = format!("{}{}", sigil_prefix(&data.sigil), new_name);
                match apply_edits(&fixture.source, &edits, &replacement) {
                    Some(result) => {
                        if rename.get("postcondition_source").and_then(Value::as_str)
                            != Some(result.as_str())
                        {
                            violations.push(format!(
                                "{rename_path}.postcondition_source: applying the declared edits to \
                                 the fixture source does not reproduce the recorded postcondition"
                            ));
                        }
                    }
                    None => violations.push(format!(
                        "{rename_path}.edits: ranges overlap or split UTF-8 boundaries"
                    )),
                }
            } else if rename.get("postcondition_source").and_then(Value::as_str)
                != Some(fixture.source.as_str())
            {
                violations.push(format!(
                    "{rename_path}.postcondition_source: an unapplied rename leaves the source unchanged"
                ));
            }
        }
    } else {
        if !edits.is_empty()
            || !authorized.is_empty()
            || !planned.is_empty()
            || !projected.is_empty()
            || !applied.is_empty()
        {
            violations.push(format!(
                "{rename_path}: refusal/instrument-failure rows emit no edits and no partial ID sets"
            ));
        }
        if rename.get("postcondition_source").and_then(Value::as_str)
            != Some(fixture.source.as_str())
        {
            violations.push(format!(
                "{rename_path}.postcondition_source: a refused rename leaves the source unchanged"
            ));
        }
    }
}

fn apply_edits(source: &str, edits: &[(u64, u64)], replacement: &str) -> Option<String> {
    let mut ordered: Vec<(u64, u64)> = edits.to_vec();
    ordered.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    for window in ordered.windows(2) {
        if window[0].0 < window[1].1 {
            return None;
        }
    }
    let mut result = source.to_string();
    for (start, end) in ordered {
        let range = result.get(start as usize..end as usize)?;
        let _ = range;
        result.replace_range(start as usize..end as usize, replacement);
    }
    Some(result)
}

// --- mutations -----------------------------------------------------------------

fn validate_mutations(
    manifest: &Value,
    case_index: &BTreeMap<String, CaseInfo>,
    violations: &mut Vec<String>,
) {
    let Some(root) = manifest.as_object() else { return };
    let Some(array) = root.get("mutations").and_then(Value::as_array) else {
        violations.push("manifest.mutations: missing or not an array".to_string());
        return;
    };
    if array.len() != MUTATION_COUNT {
        violations.push(format!(
            "manifest.mutations: expected exactly {MUTATION_COUNT} controlled mutations, found {}",
            array.len()
        ));
    }
    let mut expected_ids = BTreeSet::new();
    for number in 1..=MUTATION_COUNT {
        expected_ids.insert(format!("LX-MUT-{number:02}"));
    }
    let mut seen = BTreeSet::new();
    let mut fails_rows_by_mutation: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (index, value) in array.iter().enumerate() {
        let path = format!("manifest.mutations[{index}]");
        let Some(mutation) = as_object(value, &path, violations) else { continue };
        reject_unknown_keys(
            mutation,
            &["mutation_id", "summary", "wrong_behavior", "expected_detection", "fails_rows"],
            &path,
            violations,
        );
        let Some(id) = require_string(mutation, "mutation_id", &path, violations) else { continue };
        if !seen.insert(id.to_string()) {
            violations.push(format!("{path}: duplicate mutation id `{id}`"));
        }
        if !expected_ids.contains(id) {
            violations.push(format!("{path}: unsupported mutation id `{id}`"));
        }
        require_string(mutation, "summary", &path, violations);
        require_string(mutation, "wrong_behavior", &path, violations);
        require_string(mutation, "expected_detection", &path, violations);
        // Every mutation maps to at least one stable row.
        let fails_rows = string_set(mutation.get("fails_rows"));
        if fails_rows.is_empty() {
            violations.push(format!("{path}.fails_rows: must name at least one stable row"));
        }
        fails_rows_by_mutation.insert(id.to_string(), fails_rows.clone());
        for row in &fails_rows {
            match case_index.get(row) {
                None => violations.push(format!("{path}.fails_rows: unknown case `{row}`")),
                Some(case) => {
                    // Bidirectional ownership: the case must list the mutation.
                    if !case.mutations.contains(id) {
                        violations.push(format!(
                            "{path}: mutation `{id}` fails row `{row}` but the row does not list it"
                        ));
                    }
                }
            }
        }
    }
    // Bidirectional ownership, other direction: every mutation a case lists
    // must exist and must actually fail that row — listing an existing but
    // unrelated mutation falsely claims it discriminates this row.
    for (case_id, case) in case_index {
        for mutation in &case.mutations {
            match fails_rows_by_mutation.get(mutation) {
                None => violations.push(format!(
                    "manifest.cases:{case_id}: lists unknown mutation `{mutation}`"
                )),
                Some(rows) if !rows.contains(case_id) => violations.push(format!(
                    "manifest.cases:{case_id}: lists mutation `{mutation}` but its fails_rows does not name this row"
                )),
                Some(_) => {}
            }
        }
    }
}

// --- test targets ----------------------------------------------------------------

fn validate_test_targets(manifest: &Value, violations: &mut Vec<String>) {
    let Some(root) = manifest.as_object() else { return };
    let Some(array) = root.get("test_targets").and_then(Value::as_array) else {
        violations.push("manifest.test_targets: missing or not an array".to_string());
        return;
    };
    let mut seen = BTreeSet::new();
    for (index, value) in array.iter().enumerate() {
        let path = format!("manifest.test_targets[{index}]");
        let Some(target) = as_object(value, &path, violations) else { continue };
        reject_unknown_keys(
            target,
            &["target", "kind", "status", "proof", "owner", "registration"],
            &path,
            violations,
        );
        let Some(name) = require_string(target, "target", &path, violations) else { continue };
        if !seen.insert(name.to_string()) {
            violations.push(format!("{path}: duplicate test target `{name}`"));
        }
        let kind = opt_string(target, "kind").unwrap_or_default();
        if !["manifest", "external-stdio", "mutation"].contains(&kind) {
            violations.push(format!("{path}.kind: unsupported value `{kind}`"));
        }
        let status = opt_string(target, "status").unwrap_or_default();
        if !["active", "named-pending"].contains(&status) {
            violations.push(format!("{path}.status: unsupported value `{status}`"));
        }
        require_string(target, "registration", &path, violations);
        match status {
            "active" => {
                require_string(target, "proof", &path, violations);
            }
            "named-pending" => {
                require_string(target, "owner", &path, violations);
            }
            _ => {}
        }
    }
    for required in REQUIRED_TEST_TARGETS {
        if !seen.contains(*required) {
            violations.push(format!("manifest.test_targets: missing required target `{required}`"));
        }
    }
    let required_set: BTreeSet<String> =
        REQUIRED_TEST_TARGETS.iter().map(|item| item.to_string()).collect();
    for extra in seen.difference(&required_set) {
        violations.push(format!("manifest.test_targets: unsupported target `{extra}`"));
    }
}

fn validate_test_target_proof_paths(root: &Path, manifest: &Value, violations: &mut Vec<String>) {
    let Some(array) = manifest.get("test_targets").and_then(Value::as_array) else { return };
    for target in array {
        let Some(object) = target.as_object() else { continue };
        if opt_string(object, "status") != Some("active") {
            continue;
        }
        if let Some(proof) = opt_string(object, "proof")
            && !root.join(proof).is_file()
        {
            violations
                .push(format!("manifest.test_targets: active proof path `{proof}` does not exist"));
        }
    }
}
