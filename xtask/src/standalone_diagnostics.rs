//! Validator and projection for the standalone diagnostics registry (#11493).
//!
//! The registry at `.spec/11493-standalone-diagnostics/manifest.json` is the one
//! authority that turns a typed `standalone_install_transition.v1` outcome into a
//! bounded user consequence. This module proves that the registry is **total**
//! over the closed selector cross-product, that every reason and action is
//! first-match **reachable**, that no free text or per-attempt identifier can
//! select a reason or reach a rendered parameter, and that no reason silently
//! degrades into a generic retry.
//!
//! Reason domains whose stage results are not yet typed on `main` (transport,
//! integrity, provenance, source fallback, removal) are declared deferred with an
//! owning issue. They are outside this registry, not covered by it.

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

pub const SCHEMA_VERSION: &str = "standalone_diagnostics.v1";
pub const REGISTRY_NAME: &str = "standalone-diagnostics";
pub const MANIFEST_PATH: &str = ".spec/11493-standalone-diagnostics/manifest.json";
pub const SCHEMA_PATH: &str = "schemas/standalone_diagnostics.v1.schema.json";
pub const INPUT_SCHEMA_PATH: &str = "schemas/standalone_install_transition.v1.schema.json";
pub const INPUT_SCHEMA_VERSION: &str = "standalone_install_transition.v1";

/// The action that means "nothing is required of the user".
const NO_ACTION: &str = "no_action_required";

/// Reasons in the packet-consistency family. A packet that contradicts the
/// input contract must always reach one of these, never a product claim.
const INVARIANT_PREFIX: &str = "inv_";

const MANIFEST_TOP_LEVEL_KEYS: &[&str] = &[
    "schema_version",
    "registry",
    "issue",
    "owner",
    "status",
    "updated",
    "claim_boundary",
    "input_contract",
    "vocabulary",
    "render",
    "claim_consequences_requiring_limitations",
    "deferred_reason_domains",
    "deferred_action_ids",
    "actions",
    "summary_templates",
    "primary_reasons",
    "additional_reasons",
];

const SELECTOR_FIELDS: &[&str] = &[
    "operation",
    "disposition",
    "product_units",
    "cleanup",
    "process_startup",
    "path_persistence",
];

/// Every field `standalone_install_transition.v1` declares. The schema closes
/// the object, so admission rejects anything else.
const PACKET_TOP_LEVEL_KEYS: &[&str] = &[
    "schema_version",
    "route_mode",
    "operation",
    "transaction_id",
    "attempt_id",
    "disposition",
    "candidate_id",
    "prior_current_candidate_id",
    "outcome_dimensions",
    "bounded_reason",
];

const ROUTE_MODES: &[&str] = &["first_party_posix", "first_party_powershell"];

const OPERATIONS: &[&str] = &["install", "repair", "update", "rollback"];
const DISPOSITIONS: &[&str] = &[
    "candidate_verified",
    "candidate_published_unselected",
    "selection_committed",
    "selection_unchanged",
    "rollback_committed",
    "failed_preserved_current",
    "cancelled_preserved_current",
    "not_proven_preserved_current",
];
const PRODUCT_UNITS: &[&str] = &[
    "installed",
    "repaired",
    "updated",
    "rolled_back",
    "unchanged",
    "preserved_prior",
    "not_applicable",
];
const CLEANUP: &[&str] =
    &["completed", "deferred", "failed_preserved", "not_proven", "not_applicable"];
const PROCESS_STARTUP: &[&str] = &["verified", "unproven", "failed", "not_applicable"];
const PATH_PERSISTENCE: &[&str] = &["persisted", "unchanged", "failed", "not_applicable"];

fn domain_for(field: &str) -> Option<&'static [&'static str]> {
    match field {
        "operation" => Some(OPERATIONS),
        "disposition" => Some(DISPOSITIONS),
        "product_units" => Some(PRODUCT_UNITS),
        "cleanup" => Some(CLEANUP),
        "process_startup" => Some(PROCESS_STARTUP),
        "path_persistence" => Some(PATH_PERSISTENCE),
        _ => None,
    }
}

/// One point of the closed selector cross-product.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combination {
    pub operation: String,
    pub disposition: String,
    pub product_units: String,
    pub cleanup: String,
    pub process_startup: String,
    pub path_persistence: String,
}

impl Combination {
    fn field(&self, name: &str) -> Option<&str> {
        match name {
            "operation" => Some(&self.operation),
            "disposition" => Some(&self.disposition),
            "product_units" => Some(&self.product_units),
            "cleanup" => Some(&self.cleanup),
            "process_startup" => Some(&self.process_startup),
            "path_persistence" => Some(&self.path_persistence),
            _ => None,
        }
    }
}

/// Every point of the closed cross-product, in a stable order.
pub fn all_combinations() -> Vec<Combination> {
    let mut out = Vec::with_capacity(
        OPERATIONS.len()
            * DISPOSITIONS.len()
            * PRODUCT_UNITS.len()
            * CLEANUP.len()
            * PROCESS_STARTUP.len()
            * PATH_PERSISTENCE.len(),
    );
    for operation in OPERATIONS {
        for disposition in DISPOSITIONS {
            for product_units in PRODUCT_UNITS {
                for cleanup in CLEANUP {
                    for process_startup in PROCESS_STARTUP {
                        for path_persistence in PATH_PERSISTENCE {
                            out.push(Combination {
                                operation: (*operation).to_string(),
                                disposition: (*disposition).to_string(),
                                product_units: (*product_units).to_string(),
                                cleanup: (*cleanup).to_string(),
                                process_startup: (*process_startup).to_string(),
                                path_persistence: (*path_persistence).to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

#[derive(Debug)]
pub struct StandaloneDiagnosticsError(String);

impl Display for StandaloneDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl Error for StandaloneDiagnosticsError {}

impl StandaloneDiagnosticsError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

type Res<T> = Result<T, StandaloneDiagnosticsError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationStats {
    pub actions: usize,
    pub summary_templates: usize,
    pub primary_reasons: usize,
    pub additional_reasons: usize,
    pub combinations: usize,
    pub deferred_reason_domains: usize,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

fn read_repo_bytes(root: &Path, rel: &str) -> Res<Vec<u8>> {
    fs::read(root.join(rel))
        .map_err(|error| StandaloneDiagnosticsError::new(format!("cannot read `{rel}`: {error}")))
}

fn parse_json(bytes: &[u8], label: &str) -> Res<Value> {
    serde_json::from_slice(bytes).map_err(|error| {
        StandaloneDiagnosticsError::new(format!("`{label}` is not valid JSON: {error}"))
    })
}

pub fn load_manifest(root: &Path) -> Res<Value> {
    let bytes = read_repo_bytes(root, MANIFEST_PATH)?;
    parse_json(&bytes, MANIFEST_PATH)
}

// ---------------------------------------------------------------------------
// Small typed accessors
// ---------------------------------------------------------------------------

fn as_object<'a>(
    value: &'a Value,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    match value.as_object() {
        Some(object) => Some(object),
        None => {
            violations.push(format!("{path} must be an object"));
            None
        }
    }
}

fn as_array<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<&'a Vec<Value>> {
    match parent.get(key).and_then(Value::as_array) {
        Some(array) => Some(array),
        None => {
            violations.push(format!("{path}.{key} must be an array"));
            None
        }
    }
}

fn opt_str<'a>(parent: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    parent.get(key).and_then(Value::as_str)
}

fn require_str<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
    violations: &mut Vec<String>,
) -> Option<&'a str> {
    match opt_str(parent, key) {
        Some(text) if !text.is_empty() => Some(text),
        _ => {
            violations.push(format!("{path}.{key} must be a non-empty string"));
            None
        }
    }
}

fn string_list(parent: &Map<String, Value>, key: &str) -> Vec<String> {
    parent
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn finish(violations: Vec<String>) -> Res<()> {
    if violations.is_empty() {
        return Ok(());
    }
    let mut message = format!("{} registry violation(s):", violations.len());
    for violation in &violations {
        message.push_str("\n  - ");
        message.push_str(violation);
    }
    Err(StandaloneDiagnosticsError::new(message))
}

// ---------------------------------------------------------------------------
// Selector matching
// ---------------------------------------------------------------------------

fn selector_matches(selector: &Map<String, Value>, combination: &Combination) -> bool {
    for (field, allowed) in selector {
        let Some(actual) = combination.field(field) else {
            return false;
        };
        let Some(values) = allowed.as_array() else {
            return false;
        };
        if !values.iter().filter_map(Value::as_str).any(|value| value == actual) {
            return false;
        }
    }
    true
}

/// The first primary reason whose selector matches, if any.
pub fn primary_reason_for<'a>(manifest: &'a Value, combination: &Combination) -> Option<&'a Value> {
    manifest.get("primary_reasons")?.as_array()?.iter().find(|reason| {
        reason
            .get("selector")
            .and_then(Value::as_object)
            .is_some_and(|selector| selector_matches(selector, combination))
    })
}

/// Every additional reason whose selector matches, in registry order.
pub fn additional_reasons_for<'a>(
    manifest: &'a Value,
    combination: &Combination,
) -> Vec<&'a Value> {
    manifest
        .get("additional_reasons")
        .and_then(Value::as_array)
        .map(|reasons| {
            reasons
                .iter()
                .filter(|reason| {
                    reason
                        .get("selector")
                        .and_then(Value::as_object)
                        .is_some_and(|selector| selector_matches(selector, combination))
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Listing and explanation
// ---------------------------------------------------------------------------

pub fn list_reason_ids(manifest: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for key in ["primary_reasons", "additional_reasons"] {
        if let Some(reasons) = manifest.get(key).and_then(Value::as_array) {
            for reason in reasons {
                if let Some(id) = reason.get("reason_id").and_then(Value::as_str) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids
}

pub fn explain_reason(manifest: &Value, reason_id: &str) -> Option<String> {
    for key in ["primary_reasons", "additional_reasons"] {
        let Some(reasons) = manifest.get(key).and_then(Value::as_array) else {
            continue;
        };
        for reason in reasons {
            if reason.get("reason_id").and_then(Value::as_str) != Some(reason_id) {
                continue;
            }
            let mut row = reason.clone();
            if let Some(object) = row.as_object_mut() {
                object.insert("reason_role".to_string(), Value::from(key));
            }
            return serde_json::to_string_pretty(&row).ok();
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Manifest validation
// ---------------------------------------------------------------------------

pub fn validate_manifest_file(root: &Path) -> Res<ValidationStats> {
    let bytes = read_repo_bytes(root, MANIFEST_PATH)?;
    let manifest = parse_json(&bytes, MANIFEST_PATH)?;

    let mut violations = Vec::new();
    validate_canonical_bytes(&bytes, &manifest, &mut violations);
    validate_against_registry_schema(root, &manifest, &mut violations)?;
    validate_input_schema_agreement(root, &mut violations)?;
    validate_registry_schema_agreement(root, &mut violations)?;
    finish(violations)?;

    validate_manifest_value(&manifest)
}

/// Apply the registry schema as the structural authority.
///
/// The semantic checks below know the rules this registry cares about; they do
/// not know every field the durable contract declares. Parsing the schema
/// without applying it would let a structurally invalid manifest - an action
/// missing `applicability`, an unknown nested key, a malformed issue reference -
/// pass the advertised validation command.
fn validate_against_registry_schema(
    root: &Path,
    manifest: &Value,
    violations: &mut Vec<String>,
) -> Res<()> {
    let bytes = read_repo_bytes(root, SCHEMA_PATH)?;
    let schema = parse_json(&bytes, SCHEMA_PATH)?;
    let validator = jsonschema::validator_for(&schema).map_err(|error| {
        StandaloneDiagnosticsError::new(format!("`{SCHEMA_PATH}` is not a valid schema: {error}"))
    })?;
    for error in validator.iter_errors(manifest) {
        violations.push(format!("registry schema violation: {error}"));
    }
    Ok(())
}

/// The committed manifest must already be in canonical pretty form so that a
/// regenerated registry cannot drift from the reviewed bytes.
fn validate_canonical_bytes(bytes: &[u8], manifest: &Value, violations: &mut Vec<String>) {
    let Ok(canonical) = serde_json::to_string_pretty(manifest) else {
        violations.push("manifest cannot be re-serialized canonically".to_string());
        return;
    };
    let canonical = format!("{canonical}\n");
    if bytes == canonical.as_bytes() {
        return;
    }
    let offset = bytes
        .iter()
        .zip(canonical.as_bytes())
        .position(|(actual, expected)| actual != expected)
        .unwrap_or_else(|| bytes.len().min(canonical.len()));
    let window = |source: &[u8]| {
        let start = offset.saturating_sub(40);
        let end = source.len().min(offset + 40);
        String::from_utf8_lossy(source.get(start..end).unwrap_or_default()).to_string()
    };
    violations.push(format!(
        "`{MANIFEST_PATH}` is not canonical: expected two-space pretty JSON with a trailing newline; \
         first difference at byte {offset}\n      committed: {:?}\n      canonical: {:?}",
        window(bytes),
        window(canonical.as_bytes())
    ));
}

/// The selector domains this module enumerates must equal the input schema's
/// enums. A widened installer contract must fail here rather than silently
/// leaving new outcomes unmapped.
fn validate_input_schema_agreement(root: &Path, violations: &mut Vec<String>) -> Res<()> {
    let bytes = read_repo_bytes(root, INPUT_SCHEMA_PATH)?;
    let schema = parse_json(&bytes, INPUT_SCHEMA_PATH)?;

    let properties = schema.get("properties");
    compare_enum(properties, &["operation"], OPERATIONS, "operation", violations);
    compare_enum(properties, &["disposition"], DISPOSITIONS, "disposition", violations);

    let dimensions = schema
        .get("$defs")
        .and_then(|defs| defs.get("outcome_dimensions"))
        .and_then(|dimensions| dimensions.get("properties"));
    compare_enum(dimensions, &["product_units"], PRODUCT_UNITS, "product_units", violations);
    compare_enum(dimensions, &["cleanup"], CLEANUP, "cleanup", violations);
    compare_enum(dimensions, &["process_startup"], PROCESS_STARTUP, "process_startup", violations);
    compare_enum(
        dimensions,
        &["path_persistence"],
        PATH_PERSISTENCE,
        "path_persistence",
        violations,
    );
    Ok(())
}

fn compare_enum(
    parent: Option<&Value>,
    path: &[&str],
    expected: &[&str],
    label: &str,
    violations: &mut Vec<String>,
) {
    let mut cursor = parent;
    for segment in path {
        cursor = cursor.and_then(|value| value.get(segment));
    }
    let Some(actual) = cursor.and_then(|value| value.get("enum")).and_then(Value::as_array) else {
        violations.push(format!("`{INPUT_SCHEMA_PATH}` does not declare an enum for `{label}`"));
        return;
    };
    let actual: Vec<&str> = actual.iter().filter_map(Value::as_str).collect();
    if actual != expected {
        violations.push(format!(
            "`{label}` domain drifted from `{INPUT_SCHEMA_PATH}`: registry knows {expected:?}, schema declares {actual:?}"
        ));
    }
}

/// The registry schema's selector field list must equal this module's.
fn validate_registry_schema_agreement(root: &Path, violations: &mut Vec<String>) -> Res<()> {
    let bytes = read_repo_bytes(root, SCHEMA_PATH)?;
    let schema = parse_json(&bytes, SCHEMA_PATH)?;
    let Some(fields) = schema
        .get("$defs")
        .and_then(|defs| defs.get("selector_field"))
        .and_then(|field| field.get("enum"))
        .and_then(Value::as_array)
    else {
        violations.push(format!("`{SCHEMA_PATH}` does not declare `$defs.selector_field.enum`"));
        return Ok(());
    };
    let fields: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
    if fields != SELECTOR_FIELDS {
        violations.push(format!(
            "selector fields drifted from `{SCHEMA_PATH}`: registry knows {SELECTOR_FIELDS:?}, schema declares {fields:?}"
        ));
    }
    Ok(())
}

/// Validate the registry's *semantics* for an already-parsed manifest.
///
/// This is the narrower of the two entry points and is deliberately not the
/// advertised gate. `validate_manifest_file` is the authority `cargo xtask
/// standalone-diagnostics check` runs: it additionally applies the JSON Schema,
/// the canonical-bytes check, and the input-contract agreement check, none of
/// which can be evaluated from a `Value` alone because they need the committed
/// bytes and the sibling schema files. Callers holding only an in-memory
/// manifest therefore get the semantic layer, not the structural one.
pub fn validate_manifest_value(manifest: &Value) -> Res<ValidationStats> {
    let mut violations = Vec::new();
    let Some(root) = as_object(manifest, "manifest", &mut violations) else {
        finish(violations)?;
        return Err(StandaloneDiagnosticsError::new("manifest is not an object"));
    };

    validate_top_level(root, &mut violations);
    let vocabulary = validate_vocabulary(root, &mut violations);
    let render = validate_render(root, &mut violations);
    validate_input_contract(root, &mut violations);
    validate_deferred(root, &mut violations);

    let actions = validate_actions(root, &vocabulary, &mut violations);
    let templates = validate_templates(root, &render, &mut violations);
    validate_reasons(root, &vocabulary, &actions, &templates, &mut violations);
    validate_totality_and_reachability(root, &mut violations);

    finish(violations)?;

    Ok(ValidationStats {
        actions: actions.len(),
        summary_templates: templates.len(),
        primary_reasons: root
            .get("primary_reasons")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        additional_reasons: root
            .get("additional_reasons")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        combinations: all_combinations().len(),
        deferred_reason_domains: root
            .get("deferred_reason_domains")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
    })
}

fn validate_top_level(root: &Map<String, Value>, violations: &mut Vec<String>) {
    let expected: BTreeSet<&str> = MANIFEST_TOP_LEVEL_KEYS.iter().copied().collect();
    let actual: BTreeSet<&str> = root.keys().map(String::as_str).collect();
    for unknown in actual.difference(&expected) {
        violations.push(format!("unknown top-level key `{unknown}`"));
    }
    for missing in expected.difference(&actual) {
        violations.push(format!("missing top-level key `{missing}`"));
    }
    if opt_str(root, "schema_version") != Some(SCHEMA_VERSION) {
        violations.push(format!("schema_version must be `{SCHEMA_VERSION}`"));
    }
    if opt_str(root, "registry") != Some(REGISTRY_NAME) {
        violations.push(format!("registry must be `{REGISTRY_NAME}`"));
    }
}

#[derive(Default)]
struct Vocabulary {
    classification: BTreeSet<String>,
    terminality: BTreeSet<String>,
    retryability: BTreeSet<String>,
    side_effect_disposition: BTreeSet<String>,
    claim_consequence: BTreeSet<String>,
    action_kind: BTreeSet<String>,
    public_rendering_ceiling: BTreeSet<String>,
    platform_scope: BTreeSet<String>,
    claims_requiring_limitations: BTreeSet<String>,
}

fn required_token_set(
    object: &Map<String, Value>,
    key: &str,
    violations: &mut Vec<String>,
) -> BTreeSet<String> {
    let values = string_list(object, key);
    if values.is_empty() {
        violations.push(format!("vocabulary.{key} must be a non-empty token list"));
    }
    values.into_iter().collect()
}

fn validate_vocabulary(root: &Map<String, Value>, violations: &mut Vec<String>) -> Vocabulary {
    let mut vocabulary = Vocabulary::default();
    let Some(object) = root.get("vocabulary").and_then(Value::as_object) else {
        violations.push("vocabulary must be an object".to_string());
        return vocabulary;
    };
    vocabulary.classification = required_token_set(object, "classification", violations);
    vocabulary.terminality = required_token_set(object, "terminality", violations);
    vocabulary.retryability = required_token_set(object, "retryability", violations);
    vocabulary.side_effect_disposition =
        required_token_set(object, "side_effect_disposition", violations);
    vocabulary.claim_consequence = required_token_set(object, "claim_consequence", violations);
    vocabulary.action_kind = required_token_set(object, "action_kind", violations);
    vocabulary.public_rendering_ceiling =
        required_token_set(object, "public_rendering_ceiling", violations);
    vocabulary.platform_scope = required_token_set(object, "platform_scope", violations);

    vocabulary.claims_requiring_limitations = root
        .get("claim_consequences_requiring_limitations")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    for claim in &vocabulary.claims_requiring_limitations {
        if !vocabulary.claim_consequence.contains(claim) {
            violations.push(format!(
                "claim_consequences_requiring_limitations lists unknown claim `{claim}`"
            ));
        }
    }
    vocabulary
}

struct RenderPolicy {
    allowed: BTreeSet<String>,
    forbidden: BTreeSet<String>,
}

fn validate_render(root: &Map<String, Value>, violations: &mut Vec<String>) -> RenderPolicy {
    let mut policy = RenderPolicy { allowed: BTreeSet::new(), forbidden: BTreeSet::new() };
    let Some(object) = root.get("render").and_then(Value::as_object) else {
        violations.push("render must be an object".to_string());
        return policy;
    };
    policy.allowed = string_list(object, "allowed_parameters").into_iter().collect();
    policy.forbidden = string_list(object, "forbidden_parameters").into_iter().collect();
    if policy.allowed.is_empty() {
        violations.push("render.allowed_parameters must be a non-empty token list".to_string());
    }
    for overlap in policy.allowed.intersection(&policy.forbidden) {
        violations.push(format!("render parameter `{overlap}` is both allowed and forbidden"));
    }
    // A rendered parameter may only come from a typed selector field, never from
    // free text or per-attempt identity.
    for parameter in &policy.allowed {
        if parameter != "route_mode" && domain_for(parameter).is_none() {
            violations.push(format!(
                "render.allowed_parameters includes `{parameter}`, which is not a typed selector field or route_mode"
            ));
        }
    }
    policy
}

fn validate_input_contract(root: &Map<String, Value>, violations: &mut Vec<String>) {
    let Some(object) = root.get("input_contract").and_then(Value::as_object) else {
        violations.push("input_contract must be an object".to_string());
        return;
    };
    if opt_str(object, "schema_version") != Some(INPUT_SCHEMA_VERSION) {
        violations.push(format!("input_contract.schema_version must be `{INPUT_SCHEMA_VERSION}`"));
    }
    if opt_str(object, "schema_path") != Some(INPUT_SCHEMA_PATH) {
        violations.push(format!("input_contract.schema_path must be `{INPUT_SCHEMA_PATH}`"));
    }
    let declared = string_list(object, "selector_fields");
    if declared != SELECTOR_FIELDS {
        violations.push(format!(
            "input_contract.selector_fields must be {SELECTOR_FIELDS:?}, found {declared:?}"
        ));
    }
    let forbidden = string_list(object, "forbidden_selector_fields");
    if !forbidden.iter().any(|field| field == "bounded_reason") {
        violations.push(
            "input_contract.forbidden_selector_fields must forbid `bounded_reason`: typed results, not tool prose, select a reason".to_string(),
        );
    }
    for field in &forbidden {
        if SELECTOR_FIELDS.contains(&field.as_str()) {
            violations.push(format!(
                "input_contract lists `{field}` as both a selector field and a forbidden selector field"
            ));
        }
    }
}

fn validate_deferred(root: &Map<String, Value>, violations: &mut Vec<String>) {
    let Some(domains) = as_array(root, "deferred_reason_domains", "manifest", violations) else {
        return;
    };
    let mut seen = BTreeSet::new();
    for (index, domain) in domains.iter().enumerate() {
        let path = format!("deferred_reason_domains[{index}]");
        let Some(object) = as_object(domain, &path, violations) else {
            continue;
        };
        let Some(id) = require_str(object, "domain_id", &path, violations) else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            violations.push(format!("duplicate deferred domain `{id}`"));
        }
        // A deferred domain must transfer to a real owner, never back to this issue.
        if opt_str(object, "owner_issue") == Some("#11493") {
            violations.push(format!(
                "deferred domain `{id}` names #11493 as its owner; a deferred domain must transfer to the issue that types its stage result"
            ));
        }
    }
}

fn validate_actions(
    root: &Map<String, Value>,
    vocabulary: &Vocabulary,
    violations: &mut Vec<String>,
) -> BTreeMap<String, Value> {
    let mut actions = BTreeMap::new();
    let Some(items) = as_array(root, "actions", "manifest", violations) else {
        return actions;
    };
    for (index, action) in items.iter().enumerate() {
        let path = format!("actions[{index}]");
        let Some(object) = as_object(action, &path, violations) else {
            continue;
        };
        let Some(id) = require_str(object, "action_id", &path, violations) else {
            continue;
        };
        if actions.contains_key(id) {
            violations.push(format!("duplicate action id `{id}`"));
        }
        if let Some(kind) = require_str(object, "action_kind", &path, violations)
            && !vocabulary.action_kind.contains(kind)
        {
            violations.push(format!("action `{id}` has unknown action_kind `{kind}`"));
        }
        if let Some(scope) = require_str(object, "platform_scope", &path, violations)
            && !vocabulary.platform_scope.contains(scope)
        {
            violations.push(format!("action `{id}` has unknown platform_scope `{scope}`"));
        }
        if let Some(ceiling) = require_str(object, "public_rendering_ceiling", &path, violations)
            && !vocabulary.public_rendering_ceiling.contains(ceiling)
        {
            violations
                .push(format!("action `{id}` has unknown public_rendering_ceiling `{ceiling}`"));
        }
        for flag in ["destructive", "external", "elevated", "manual"] {
            if !object.get(flag).is_some_and(Value::is_boolean) {
                violations.push(format!("action `{id}` must declare boolean `{flag}`"));
            }
        }
        // This registry describes consequences; it never authorizes an external
        // or privileged mutation on the user's behalf.
        if object.get("external").and_then(Value::as_bool) == Some(true) {
            violations.push(format!(
                "action `{id}` is marked external: the diagnostics registry may not authorize release, publication, or upstream mutation"
            ));
        }
        if object.get("elevated").and_then(Value::as_bool) == Some(true) {
            violations.push(format!(
                "action `{id}` is marked elevated: privileged operations are not owned by this registry"
            ));
        }
        actions.insert(id.to_string(), action.clone());
    }
    actions
}

fn validate_templates(
    root: &Map<String, Value>,
    render: &RenderPolicy,
    violations: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut templates = BTreeMap::new();
    let Some(items) = as_array(root, "summary_templates", "manifest", violations) else {
        return templates;
    };
    for (index, template) in items.iter().enumerate() {
        let path = format!("summary_templates[{index}]");
        let Some(object) = as_object(template, &path, violations) else {
            continue;
        };
        let Some(id) = require_str(object, "template_id", &path, violations) else {
            continue;
        };
        let Some(text) = require_str(object, "text", &path, violations) else {
            continue;
        };
        if templates.contains_key(id) {
            violations.push(format!("duplicate template id `{id}`"));
        }
        for parameter in template_parameters(text) {
            if render.forbidden.contains(&parameter) {
                violations
                    .push(format!("template `{id}` renders forbidden parameter `{parameter}`"));
            } else if !render.allowed.contains(&parameter) {
                violations.push(format!(
                    "template `{id}` renders parameter `{parameter}`, which is not in render.allowed_parameters"
                ));
            }
        }
        templates.insert(id.to_string(), text.to_string());
    }
    templates
}

/// Placeholders are `{name}` spans. An unterminated brace is a violation
/// surfaced by the caller as an unknown parameter.
pub fn template_parameters(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => {
                out.push(after.to_string());
                break;
            }
        }
    }
    out
}

fn check_reason_token(
    object: &Map<String, Value>,
    id: &str,
    path: &str,
    field: &str,
    allowed: &BTreeSet<String>,
    violations: &mut Vec<String>,
) -> Option<String> {
    let value = require_str(object, field, path, violations)?.to_string();
    if !allowed.contains(&value) {
        violations.push(format!("reason `{id}` has unknown {field} `{value}`"));
    }
    Some(value)
}

fn validate_reasons(
    root: &Map<String, Value>,
    vocabulary: &Vocabulary,
    actions: &BTreeMap<String, Value>,
    templates: &BTreeMap<String, String>,
    violations: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut referenced_actions = BTreeSet::new();
    let mut referenced_templates = BTreeSet::new();

    for key in ["primary_reasons", "additional_reasons"] {
        let Some(items) = as_array(root, key, "manifest", violations) else {
            continue;
        };
        for (index, reason) in items.iter().enumerate() {
            let path = format!("{key}[{index}]");
            let Some(object) = as_object(reason, &path, violations) else {
                continue;
            };
            let Some(id) = require_str(object, "reason_id", &path, violations) else {
                continue;
            };
            if !ids.insert(id.to_string()) {
                violations.push(format!("duplicate reason id `{id}`"));
            }

            validate_selector(object, id, violations);

            check_reason_token(
                object,
                id,
                &path,
                "classification",
                &vocabulary.classification,
                violations,
            );
            check_reason_token(
                object,
                id,
                &path,
                "terminality",
                &vocabulary.terminality,
                violations,
            );
            check_reason_token(
                object,
                id,
                &path,
                "retryability",
                &vocabulary.retryability,
                violations,
            );
            check_reason_token(
                object,
                id,
                &path,
                "side_effect_disposition",
                &vocabulary.side_effect_disposition,
                violations,
            );
            let claim = check_reason_token(
                object,
                id,
                &path,
                "claim_consequence",
                &vocabulary.claim_consequence,
                violations,
            );

            let limitations = string_list(object, "required_limitations");
            if let Some(claim) = claim.as_deref()
                && vocabulary.claims_requiring_limitations.contains(claim)
                && limitations.is_empty()
            {
                violations.push(format!(
                    "reason `{id}` claims `{claim}` but retains no limitation; an unproven or withheld claim must stay load-bearing"
                ));
            }

            if let Some(template_id) = require_str(object, "summary_template_id", &path, violations)
            {
                if !templates.contains_key(template_id) {
                    violations
                        .push(format!("reason `{id}` names unknown template `{template_id}`"));
                }
                referenced_templates.insert(template_id.to_string());
            }

            let action_ids = string_list(object, "action_ids");
            if action_ids.is_empty() {
                violations.push(format!("reason `{id}` must name at least one action"));
            }
            for action_id in &action_ids {
                if !actions.contains_key(action_id) {
                    violations.push(format!("reason `{id}` names unknown action `{action_id}`"));
                }
                referenced_actions.insert(action_id.clone());
            }
            if action_ids.iter().any(|action| action == NO_ACTION) {
                if action_ids.len() > 1 {
                    violations
                        .push(format!("reason `{id}` combines `{NO_ACTION}` with another action"));
                }
                if let Some(claim) = claim.as_deref()
                    && vocabulary.claims_requiring_limitations.contains(claim)
                {
                    violations.push(format!(
                        "reason `{id}` claims `{claim}` yet requires no action; an unproven claim may not render as nothing to do"
                    ));
                }
            }
            if opt_str(object, "redaction_policy") != Some("bounded_typed_fields_only") {
                violations.push(format!(
                    "reason `{id}` must declare redaction_policy `bounded_typed_fields_only`"
                ));
            }
        }
    }

    for action_id in actions.keys() {
        if !referenced_actions.contains(action_id) {
            violations.push(format!("action `{action_id}` is never referenced by a reason"));
        }
    }
    for template_id in templates.keys() {
        if !referenced_templates.contains(template_id) {
            violations.push(format!("template `{template_id}` is never referenced by a reason"));
        }
    }
    ids
}

fn validate_selector(reason: &Map<String, Value>, id: &str, violations: &mut Vec<String>) {
    let Some(selector) = reason.get("selector").and_then(Value::as_object) else {
        violations.push(format!("reason `{id}` must declare a selector object"));
        return;
    };
    if selector.is_empty() {
        violations.push(format!(
            "reason `{id}` has an empty selector: an unconditional catch-all would turn an unmapped outcome into generic advice"
        ));
    }
    for (field, values) in selector {
        let Some(domain) = domain_for(field) else {
            violations.push(format!(
                "reason `{id}` selects on `{field}`, which is not a typed selector field"
            ));
            continue;
        };
        let Some(items) = values.as_array().filter(|items| !items.is_empty()) else {
            violations.push(format!(
                "reason `{id}` selector field `{field}` must be a non-empty value list"
            ));
            continue;
        };
        let mut seen = BTreeSet::new();
        for value in items {
            let Some(text) = value.as_str() else {
                violations.push(format!(
                    "reason `{id}` selector field `{field}` contains a non-string value"
                ));
                continue;
            };
            if !domain.contains(&text) {
                violations.push(format!(
                    "reason `{id}` selector field `{field}` contains `{text}`, which is outside the typed domain"
                ));
            }
            if !seen.insert(text) {
                violations.push(format!("reason `{id}` selector field `{field}` repeats `{text}`"));
            }
        }
        if items.len() == domain.len() && seen.len() == domain.len() {
            violations.push(format!(
                "reason `{id}` selector field `{field}` lists the whole domain; omit the field instead of restating it"
            ));
        }
    }
}

fn validate_totality_and_reachability(root: &Map<String, Value>, violations: &mut Vec<String>) {
    let manifest = Value::Object(root.clone());
    let mut first_match: BTreeMap<String, usize> = BTreeMap::new();
    let mut additional_match: BTreeMap<String, usize> = BTreeMap::new();

    for key in ["primary_reasons", "additional_reasons"] {
        let target =
            if key == "primary_reasons" { &mut first_match } else { &mut additional_match };
        if let Some(items) = root.get(key).and_then(Value::as_array) {
            for reason in items {
                if let Some(id) = reason.get("reason_id").and_then(Value::as_str) {
                    target.insert(id.to_string(), 0);
                }
            }
        }
    }

    let mut gaps: Vec<String> = Vec::new();
    let mut gap_total: usize = 0;
    for combination in all_combinations() {
        match primary_reason_for(&manifest, &combination) {
            Some(reason) => {
                if let Some(id) = reason.get("reason_id").and_then(Value::as_str) {
                    *first_match.entry(id.to_string()).or_insert(0) += 1;
                }
            }
            None => {
                gap_total += 1;
                if gaps.len() < 5 {
                    gaps.push(format!(
                        "{}/{}/{}/{}/{}/{}",
                        combination.operation,
                        combination.disposition,
                        combination.product_units,
                        combination.cleanup,
                        combination.process_startup,
                        combination.path_persistence
                    ));
                }
            }
        }
        for reason in additional_reasons_for(&manifest, &combination) {
            if let Some(id) = reason.get("reason_id").and_then(Value::as_str) {
                *additional_match.entry(id.to_string()).or_insert(0) += 1;
            }
        }
    }

    if !gaps.is_empty() {
        violations.push(format!(
            "registry gap: no primary reason matches {} typed combination(s), for example {}",
            gap_total,
            gaps.join(", ")
        ));
    }
    validate_invariants_are_never_shadowed(&manifest, violations);
    for (id, hits) in &first_match {
        if *hits == 0 {
            violations.push(format!(
                "primary reason `{id}` is never the first match; it is shadowed and cannot be reached"
            ));
        }
    }
    for (id, hits) in &additional_match {
        if *hits == 0 {
            violations.push(format!("additional reason `{id}` never matches a typed combination"));
        }
    }
}

/// A packet-consistency invariant must never be shadowed by an ordinary reason.
///
/// First-match reachability alone proves only that each reason fires *somewhere*.
/// It cannot see an `inv_` reason whose intended space has been eroded by a
/// narrower reason placed ahead of it, which would report a self-contradictory
/// packet as a product success. This asserts the stronger property: for every
/// combination an invariant claims, the winning reason is itself an invariant.
fn validate_invariants_are_never_shadowed(manifest: &Value, violations: &mut Vec<String>) {
    let Some(reasons) = manifest.get("primary_reasons").and_then(Value::as_array) else {
        return;
    };
    let mut reported: BTreeSet<String> = BTreeSet::new();
    for reason in reasons {
        let Some(id) = reason.get("reason_id").and_then(Value::as_str) else {
            continue;
        };
        if !id.starts_with(INVARIANT_PREFIX) {
            continue;
        }
        let Some(selector) = reason.get("selector").and_then(Value::as_object) else {
            continue;
        };
        for combination in all_combinations() {
            if !selector_matches(selector, &combination) {
                continue;
            }
            let winner = primary_reason_for(manifest, &combination)
                .and_then(|winner| winner.get("reason_id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if winner.starts_with(INVARIANT_PREFIX) {
                continue;
            }
            if reported.insert(id.to_string()) {
                violations.push(format!(
                    "invariant `{id}` is shadowed by `{winner}` for {}/{}/{}/{}/{}/{}: a self-contradictory packet would be reported as a product outcome",
                    combination.operation,
                    combination.disposition,
                    combination.product_units,
                    combination.cleanup,
                    combination.process_startup,
                    combination.path_persistence
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Packet projection
// ---------------------------------------------------------------------------

/// Read a transition packet and reduce it to the typed selector fields.
///
/// `bounded_reason` is required by the input schema and deliberately dropped
/// here: it may never influence reason selection or reach a rendered parameter.
fn typed_field(parent: &Map<String, Value>, key: &str, violations: &mut Vec<String>) -> String {
    match opt_str(parent, key) {
        Some(value) if domain_for(key).is_some_and(|domain| domain.contains(&value)) => {
            value.to_string()
        }
        Some(value) => {
            violations.push(format!("`{key}` value `{value}` is outside the typed domain"));
            String::new()
        }
        None => {
            violations.push(format!("transition packet is missing `{key}`"));
            String::new()
        }
    }
}

/// One admitted `standalone_install_transition.v1` packet, reduced to the parts
/// the projection is allowed to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionPacket {
    pub combination: Combination,
    /// Rendering context only. Bounded to the closed route set so an unvalidated
    /// document can never echo arbitrary text into a projection.
    pub route_mode: String,
}

pub fn combination_from_packet(packet: &Value) -> Res<Combination> {
    read_packet(packet).map(|admitted| admitted.combination)
}

/// Admit a transition packet, or fail.
///
/// Admission is total over the input contract: every declared field must be
/// present and well-shaped, unknown fields are rejected, and `route_mode` is
/// bounded to the closed route set. A document that is not a valid
/// `standalone_install_transition.v1` packet is an admission failure, never a
/// bounded diagnostic.
pub fn read_packet(packet: &Value) -> Res<TransitionPacket> {
    let mut violations = Vec::new();
    let Some(object) = packet.as_object() else {
        return Err(StandaloneDiagnosticsError::new("transition packet must be an object"));
    };

    let expected: BTreeSet<&str> = PACKET_TOP_LEVEL_KEYS.iter().copied().collect();
    let actual: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    for unknown in actual.difference(&expected) {
        violations.push(format!("transition packet has unknown field `{unknown}`"));
    }
    for missing in expected.difference(&actual) {
        violations.push(format!("transition packet is missing `{missing}`"));
    }

    if object.contains_key("schema_version")
        && opt_str(object, "schema_version") != Some(INPUT_SCHEMA_VERSION)
    {
        violations
            .push(format!("transition packet schema_version must be `{INPUT_SCHEMA_VERSION}`"));
    }
    let route_mode = match object.get("route_mode") {
        Some(Value::String(value)) if ROUTE_MODES.contains(&value.as_str()) => value.clone(),
        Some(value) => {
            violations.push(format!(
                "`route_mode` value `{value}` is outside the typed domain; a rendered parameter may not carry arbitrary text"
            ));
            String::new()
        }
        None => String::new(),
    };
    // An absent key is already reported once by the top-level key check above;
    // these arms speak only to a key that is present and ill-shaped.
    match object.get("bounded_reason") {
        None => {}
        Some(Value::String(text)) if !text.is_empty() && text.chars().count() <= 512 => {}
        Some(_) => {
            violations.push("`bounded_reason` must be a string of 1 to 512 characters".to_string());
        }
    }
    for key in ["transaction_id", "attempt_id"] {
        match object.get(key) {
            None => {}
            Some(Value::String(text)) if is_bounded_id(text) => {}
            Some(_) => violations.push(format!("`{key}` is not a bounded identifier")),
        }
    }
    for key in ["candidate_id", "prior_current_candidate_id"] {
        match object.get(key) {
            Some(Value::Null) => {}
            Some(Value::String(text)) if is_sha256(text) => {}
            Some(_) => violations.push(format!("`{key}` must be a sha256 digest or null")),
            None => {}
        }
    }

    let operation = typed_field(object, "operation", &mut violations);
    let disposition = typed_field(object, "disposition", &mut violations);

    let empty = Map::new();
    let dimensions = match object.get("outcome_dimensions").and_then(Value::as_object) {
        Some(dimensions) => dimensions,
        None => {
            violations.push("transition packet is missing `outcome_dimensions`".to_string());
            &empty
        }
    };
    // `outcome_dimensions` is closed by the input schema; an undeclared member
    // means the emitter and this registry disagree about the contract.
    let declared: BTreeSet<&str> =
        ["product_units", "cleanup", "process_startup", "path_persistence"].into_iter().collect();
    for unknown in dimensions.keys().map(String::as_str).filter(|key| !declared.contains(key)) {
        violations.push(format!("`outcome_dimensions` has unknown field `{unknown}`"));
    }

    let product_units = typed_field(dimensions, "product_units", &mut violations);
    let cleanup = typed_field(dimensions, "cleanup", &mut violations);
    let process_startup = typed_field(dimensions, "process_startup", &mut violations);
    let path_persistence = typed_field(dimensions, "path_persistence", &mut violations);

    let mut seen = BTreeSet::new();
    violations.retain(|violation| seen.insert(violation.clone()));
    finish(violations)?;
    Ok(TransitionPacket {
        combination: Combination {
            operation,
            disposition,
            product_units,
            cleanup,
            process_startup,
            path_persistence,
        },
        route_mode,
    })
}

fn is_bounded_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text.starts_with(|first: char| first.is_ascii_alphanumeric())
        && text.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|character| character.is_ascii_hexdigit())
}

fn stage_state(field: &str, value: &str) -> &'static str {
    match (field, value) {
        (_, "not_applicable") => "not_applicable",
        ("cleanup", "completed") => "settled",
        ("cleanup", "deferred") => "pending",
        ("cleanup", "failed_preserved") => "failed",
        ("cleanup", "not_proven") => "not_proven",
        ("process_startup", "verified") => "settled",
        ("process_startup", "unproven") => "not_proven",
        ("process_startup", "failed") => "failed",
        ("path_persistence", "failed") => "failed",
        ("path_persistence", _) => "settled",
        ("product_units", _) => "settled",
        _ => "not_proven",
    }
}

fn current_consequence(side_effect: &str) -> &'static str {
    match side_effect {
        "current_advanced" => "advanced_to_new_candidate",
        "current_restored" => "restored_to_prior_candidate",
        "current_preserved_known_good" => "preserved",
        "current_preserved_but_unproven" => "preserved_but_unproven",
        _ => "unchanged",
    }
}

fn known_good_consequence(side_effect: &str) -> &'static str {
    match side_effect {
        "current_advanced" => "superseded_by_new_current",
        "current_restored" => "restored_as_current",
        "current_preserved_but_unproven" => "retained_but_startup_unproven",
        _ => "retained",
    }
}

fn rollback_consequence(disposition: &str) -> &'static str {
    match disposition {
        "rollback_committed" => "committed",
        "failed_preserved_current"
        | "cancelled_preserved_current"
        | "not_proven_preserved_current" => "not_required_current_preserved",
        _ => "not_applicable",
    }
}

fn path_consequence(path_persistence: &str, process_startup: &str) -> &'static str {
    match (path_persistence, process_startup) {
        ("failed", _) => "not_persisted",
        ("not_applicable", _) => "not_applicable",
        ("persisted", "verified") => "persisted_and_visible",
        // A fresh process that was observed and failed is not waiting on a new
        // session; saying so would name the wrong remaining step.
        ("persisted", "failed") => "persisted_but_startup_failed",
        ("persisted", _) => "persisted_new_session_required",
        ("unchanged", "verified") => "already_visible",
        ("unchanged", "failed") => "unchanged_but_startup_failed",
        ("unchanged", _) => "unchanged_visibility_unproven",
        _ => "not_applicable",
    }
}

/// Project one typed transition packet into its bounded user consequence.
pub fn project_packet(manifest: &Value, packet: &Value) -> Res<Value> {
    let admitted = read_packet(packet)?;
    project_combination(manifest, &admitted.combination, Some(admitted.route_mode.as_str()))
}

/// Project one selector combination. `route_mode` is rendering context only and
/// never participates in reason selection.
pub fn project_combination(
    manifest: &Value,
    combination: &Combination,
    route_mode: Option<&str>,
) -> Res<Value> {
    if let Some(route_mode) = route_mode
        && !ROUTE_MODES.contains(&route_mode)
    {
        return Err(StandaloneDiagnosticsError::new(format!(
            "`route_mode` value `{route_mode}` is outside the typed domain; a rendered parameter may not carry arbitrary text"
        )));
    }
    let primary = primary_reason_for(manifest, combination).ok_or_else(|| {
        StandaloneDiagnosticsError::new(format!(
            "registry gap: no reason covers {}/{}/{}/{}/{}/{}",
            combination.operation,
            combination.disposition,
            combination.product_units,
            combination.cleanup,
            combination.process_startup,
            combination.path_persistence
        ))
    })?;
    let additional = additional_reasons_for(manifest, combination);

    let side_effect =
        primary.get("side_effect_disposition").and_then(Value::as_str).unwrap_or("no_side_effect");

    // Actions are ordered by the registry, deduplicated, and `no_action_required`
    // is dropped whenever any real action applies.
    let mut action_ids: Vec<String> = Vec::new();
    for reason in std::iter::once(primary).chain(additional.iter().copied()) {
        for action in reason
            .get("action_ids")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if let Some(id) = action.as_str()
                && !action_ids.iter().any(|existing| existing == id)
            {
                action_ids.push(id.to_string());
            }
        }
    }
    if action_ids.len() > 1 {
        action_ids.retain(|id| id != NO_ACTION);
    }

    let action_rows: Vec<Value> = manifest
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter(|action| {
                    action
                        .get("action_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| action_ids.iter().any(|selected| selected == id))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let mut limitations: BTreeSet<String> = BTreeSet::new();
    for reason in std::iter::once(primary).chain(additional.iter().copied()) {
        if let Some(items) = reason.get("required_limitations").and_then(Value::as_array) {
            for item in items {
                if let Some(text) = item.as_str() {
                    limitations.insert(text.to_string());
                }
            }
        }
    }

    let template_id =
        primary.get("summary_template_id").and_then(Value::as_str).unwrap_or_default();
    let template_text = manifest
        .get("summary_templates")
        .and_then(Value::as_array)
        .and_then(|templates| {
            templates.iter().find(|template| {
                template.get("template_id").and_then(Value::as_str) == Some(template_id)
            })
        })
        .and_then(|template| template.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut parameters = Map::new();
    for name in template_parameters(template_text) {
        let value = match name.as_str() {
            "operation" => Some(combination.operation.clone()),
            "disposition" => Some(combination.disposition.clone()),
            "product_units" => Some(combination.product_units.clone()),
            "cleanup" => Some(combination.cleanup.clone()),
            "process_startup" => Some(combination.process_startup.clone()),
            "path_persistence" => Some(combination.path_persistence.clone()),
            "route_mode" => route_mode.map(str::to_string),
            _ => None,
        };
        if let Some(value) = value {
            parameters.insert(name, Value::from(value));
        }
    }

    let mut selector = Map::new();
    selector.insert("operation".to_string(), Value::from(combination.operation.clone()));
    selector.insert("disposition".to_string(), Value::from(combination.disposition.clone()));
    selector.insert("product_units".to_string(), Value::from(combination.product_units.clone()));
    selector.insert("cleanup".to_string(), Value::from(combination.cleanup.clone()));
    selector
        .insert("process_startup".to_string(), Value::from(combination.process_startup.clone()));
    selector
        .insert("path_persistence".to_string(), Value::from(combination.path_persistence.clone()));

    let mut stage_states = Map::new();
    stage_states.insert(
        "product_units".to_string(),
        Value::from(stage_state("product_units", &combination.product_units)),
    );
    stage_states
        .insert("cleanup".to_string(), Value::from(stage_state("cleanup", &combination.cleanup)));
    stage_states.insert(
        "process_startup".to_string(),
        Value::from(stage_state("process_startup", &combination.process_startup)),
    );
    stage_states.insert(
        "path_persistence".to_string(),
        Value::from(stage_state("path_persistence", &combination.path_persistence)),
    );

    let mut consequences = Map::new();
    consequences.insert("current".to_string(), Value::from(current_consequence(side_effect)));
    consequences.insert("known_good".to_string(), Value::from(known_good_consequence(side_effect)));
    consequences.insert(
        "rollback".to_string(),
        Value::from(rollback_consequence(&combination.disposition)),
    );
    consequences.insert(
        "path".to_string(),
        Value::from(path_consequence(&combination.path_persistence, &combination.process_startup)),
    );

    let mut render = Map::new();
    render.insert("template_id".to_string(), Value::from(template_id));
    render.insert("text".to_string(), Value::from(template_text));
    render.insert("parameters".to_string(), Value::Object(parameters));

    let mut projection = Map::new();
    projection.insert("schema_version".to_string(), Value::from(SCHEMA_VERSION));
    projection.insert("projection_of".to_string(), Value::from(INPUT_SCHEMA_VERSION));
    if let Some(route_mode) = route_mode {
        projection.insert("route_mode".to_string(), Value::from(route_mode));
    }
    projection.insert("selector".to_string(), Value::Object(selector));
    projection.insert(
        "primary_reason".to_string(),
        primary.get("reason_id").cloned().unwrap_or(Value::Null),
    );
    projection.insert(
        "additional_reasons".to_string(),
        Value::from(
            additional
                .iter()
                .filter_map(|reason| reason.get("reason_id").and_then(Value::as_str))
                .map(Value::from)
                .collect::<Vec<Value>>(),
        ),
    );
    projection.insert(
        "classification".to_string(),
        primary.get("classification").cloned().unwrap_or(Value::Null),
    );
    projection.insert(
        "terminality".to_string(),
        primary.get("terminality").cloned().unwrap_or(Value::Null),
    );
    projection.insert(
        "retryability".to_string(),
        primary.get("retryability").cloned().unwrap_or(Value::Null),
    );
    projection.insert("stage_states".to_string(), Value::Object(stage_states));
    projection.insert("consequences".to_string(), Value::Object(consequences));
    projection.insert("allowed_actions".to_string(), Value::from(action_rows));
    projection.insert(
        "limitations".to_string(),
        Value::from(limitations.into_iter().map(Value::from).collect::<Vec<Value>>()),
    );
    projection.insert(
        "claim_ceiling".to_string(),
        primary.get("claim_consequence").cloned().unwrap_or(Value::Null),
    );
    projection.insert("render".to_string(), Value::Object(render));

    Ok(Value::Object(projection))
}
