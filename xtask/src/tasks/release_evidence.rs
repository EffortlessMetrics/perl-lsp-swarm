use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::utils::project_root;

const DEFAULT_POLICY_PATH: &str = ".ci/release/evidence.toml";
const RECEIPT_REGISTRY_PATH: &str = ".ci/receipts/registry.toml";
const PARSER_RATCHET_RELEASE_NAME: &str = "parser-ratchet-release";
const PARSER_RATCHET_CHECK: &str = "parser-ratchet";
const COMMON_RECEIPT_FIELDS: &[&str] = &["check", "schema_version", "event", "verdict"];

#[derive(Debug, Deserialize)]
struct EvidencePolicy {
    receipts: Vec<ReceiptPolicy>,
}

#[derive(Debug, Deserialize, Clone)]
struct ReceiptPolicy {
    name: String,
    file: String,
    #[serde(default = "default_true")]
    required: bool,
    #[serde(default = "default_true")]
    release_blocking: bool,
}

#[derive(Debug, Deserialize)]
struct ReceiptRegistry {
    #[serde(default)]
    receipt: Vec<ReceiptRegistryEntry>,
}

#[derive(Debug, Deserialize)]
struct ReceiptRegistryEntry {
    check: String,
    schema: String,
}

#[derive(Debug, Serialize)]
struct EvidenceScaffold {
    version: String,
    bundle_dir: String,
    required_receipts: Vec<ScaffoldReceipt>,
}

#[derive(Debug, Serialize)]
struct ScaffoldReceipt {
    name: String,
    path: String,
    required: bool,
    release_blocking: bool,
}

#[derive(Debug, Serialize)]
pub struct VerifySummary {
    version: String,
    bundle_dir: String,
    status: &'static str,
    blocking_failures: Vec<String>,
    warnings: Vec<String>,
    receipts: Vec<ReceiptResult>,
}

#[derive(Debug, Serialize)]
struct ReceiptResult {
    name: String,
    path: String,
    status: String,
    required: bool,
    release_blocking: bool,
    classification: String,
}

fn default_true() -> bool {
    true
}

pub fn scaffold(version: &str, out_dir: &Path) -> Result<()> {
    let root = project_root()?;
    let policy = load_policy(&root)?;
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create output dir {}", out_dir.display()))?;

    let bundle = EvidenceScaffold {
        version: version.to_string(),
        bundle_dir: out_dir.display().to_string(),
        required_receipts: policy
            .receipts
            .iter()
            .map(|receipt| ScaffoldReceipt {
                name: receipt.name.clone(),
                path: out_dir.join(&receipt.file).display().to_string(),
                required: receipt.required,
                release_blocking: receipt.release_blocking,
            })
            .collect(),
    };

    let manifest_path = out_dir.join("required-receipts.json");
    let rendered = serde_json::to_string_pretty(&bundle)?;
    fs::write(&manifest_path, rendered)
        .with_context(|| format!("failed writing {}", manifest_path.display()))?;
    println!("release evidence scaffold written: {}", manifest_path.display());
    Ok(())
}

pub fn verify(version: &str, bundle_dir: &Path, receipt_path: &Path) -> Result<()> {
    let root = project_root()?;
    let policy = load_policy(&root)?;
    let mut blocking_failures = Vec::new();
    let mut warnings = Vec::new();
    let mut receipts = Vec::new();

    for policy_receipt in policy.receipts {
        let receipt_file = bundle_dir.join(&policy_receipt.file);
        if !receipt_file.exists() {
            let message = format!("missing required receipt: {}", receipt_file.display());
            receipts.push(ReceiptResult {
                name: policy_receipt.name,
                path: receipt_file.display().to_string(),
                status: "missing".to_string(),
                required: policy_receipt.required,
                release_blocking: policy_receipt.release_blocking,
                classification: if policy_receipt.release_blocking {
                    "missing-blocking".to_string()
                } else {
                    "missing-warning".to_string()
                },
            });
            if policy_receipt.required && policy_receipt.release_blocking {
                blocking_failures.push(message);
            } else if policy_receipt.required {
                warnings.push(message);
            }
            continue;
        }

        let value: Value = serde_json::from_str(
            &fs::read_to_string(&receipt_file)
                .with_context(|| format!("failed reading {}", receipt_file.display()))?,
        )
        .with_context(|| format!("invalid json: {}", receipt_file.display()))?;

        let semantic_errors = if policy_receipt.name == PARSER_RATCHET_RELEASE_NAME {
            validate_parser_ratchet_release(&root, &value, version)
        } else {
            Vec::new()
        };

        let status = if policy_receipt.name == PARSER_RATCHET_RELEASE_NAME {
            value.get("verdict").and_then(Value::as_str).unwrap_or("unknown").to_string()
        } else {
            extract_status(&value).unwrap_or_else(|| "unknown".to_string())
        };
        let is_pass = status.eq_ignore_ascii_case("pass") && semantic_errors.is_empty();

        let classification = if is_pass {
            "pass".to_string()
        } else if policy_receipt.release_blocking {
            let message = if semantic_errors.is_empty() {
                format!("{} failed with status={status}", policy_receipt.name)
            } else {
                format!(
                    "{} failed semantic admission: {}",
                    policy_receipt.name,
                    semantic_errors.join("; ")
                )
            };
            blocking_failures.push(message);
            "failure-blocking".to_string()
        } else {
            let message = if semantic_errors.is_empty() {
                format!(
                    "{} failed with status={status} (classified advisory warning)",
                    policy_receipt.name
                )
            } else {
                format!(
                    "{} failed semantic admission: {} (classified advisory warning)",
                    policy_receipt.name,
                    semantic_errors.join("; ")
                )
            };
            warnings.push(message);
            "failure-advisory-warning".to_string()
        };

        receipts.push(ReceiptResult {
            name: policy_receipt.name,
            path: receipt_file.display().to_string(),
            status,
            required: policy_receipt.required,
            release_blocking: policy_receipt.release_blocking,
            classification,
        });
    }

    let overall_status = if blocking_failures.is_empty() { "pass" } else { "fail" };
    let summary = VerifySummary {
        version: version.to_string(),
        bundle_dir: bundle_dir.display().to_string(),
        status: overall_status,
        blocking_failures,
        warnings,
        receipts,
    };

    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    fs::write(receipt_path, serde_json::to_string_pretty(&summary)?)
        .with_context(|| format!("failed writing {}", receipt_path.display()))?;

    if summary.status == "fail" {
        bail!("release evidence verification failed; see {}", receipt_path.display());
    }

    println!("release evidence verification passed: {}", receipt_path.display());
    Ok(())
}

fn validate_parser_ratchet_release(root: &Path, value: &Value, version: &str) -> Vec<String> {
    let mut errors = Vec::new();

    if let Err(error) = validate_registered_parser_ratchet_shape(root, value) {
        errors.push(format!("registry/schema validation failed: {error}"));
    }

    require_string_eq(value, "check", PARSER_RATCHET_CHECK, &mut errors);
    require_string_eq(value, "profile", "release", &mut errors);
    require_string_eq(value, "verdict", "pass", &mut errors);
    require_string_eq(value, "release_version", version, &mut errors);
    require_bool_eq(value, "selected", true, &mut errors);
    require_bool_eq(value, "scaffold", false, &mut errors);
    require_bool_eq(value, "measurements_disabled", false, &mut errors);
    require_string_eq(value, "measurement_disposition", "complete", &mut errors);
    require_string_eq(value, "instrument_state", "complete", &mut errors);
    require_nonempty_string(value, "selection_reason", &mut errors);

    let head_sha = require_nonempty_string(value, "head_sha", &mut errors);
    let candidate_sha = require_nonempty_string(value, "candidate_sha", &mut errors);
    require_nonempty_string(value, "base_sha", &mut errors);

    if let (Some(head_sha), Some(candidate_sha)) = (head_sha, candidate_sha)
        && head_sha != candidate_sha
    {
        errors.push(format!(
            "candidate_sha must match head_sha for release evidence ({candidate_sha} != {head_sha})"
        ));
    }

    let Some(bundle) = value.get("evidence_bundle").and_then(Value::as_object) else {
        errors.push("field 'evidence_bundle' must be an object".to_string());
        return errors;
    };

    for field in [
        "bundle_id",
        "profile_policy_id",
        "metric_registry_id",
        "manifest_fingerprint",
        "semantic_digest",
    ] {
        require_nonempty_object_string(bundle, field, &mut errors);
    }

    let required_count =
        require_positive_object_integer(bundle, "required_evidence_count", &mut errors);
    let completed_count =
        require_nonnegative_object_integer(bundle, "completed_evidence_count", &mut errors);

    let required_ids =
        require_unique_nonempty_string_array(bundle, "required_producer_ids", &mut errors);
    let producer_results = bundle.get("producer_results").and_then(Value::as_array);
    let mut producer_states = BTreeMap::new();

    match producer_results {
        Some(results) if !results.is_empty() => {
            for (index, result) in results.iter().enumerate() {
                let Some(result) = result.as_object() else {
                    errors.push(format!(
                        "evidence_bundle.producer_results[{index}] must be an object"
                    ));
                    continue;
                };
                let producer_id =
                    require_nonempty_object_string(result, "producer_id", &mut errors);
                require_nonempty_object_string(result, "receipt_id", &mut errors);
                let state = require_nonempty_object_string(result, "state", &mut errors);
                let producer_candidate =
                    require_nonempty_object_string(result, "candidate_sha", &mut errors);

                if let Some(state) = state
                    && state != "complete"
                {
                    let producer_label = producer_id.unwrap_or("unknown");
                    errors.push(format!(
                        "producer '{producer_label}' must be complete, got '{state}'"
                    ));
                }
                if let (Some(expected), Some(actual)) = (candidate_sha, producer_candidate)
                    && expected != actual
                {
                    errors.push(format!(
                        "producer candidate_sha must match release candidate ({actual} != {expected})"
                    ));
                }
                if let Some(producer_id) = producer_id
                    && producer_states.insert(producer_id.to_string(), state).is_some()
                {
                    errors.push(format!("duplicate producer result '{producer_id}'"));
                }
            }
        }
        _ => errors.push(
            "evidence_bundle.producer_results must contain at least one producer result"
                .to_string(),
        ),
    }

    if let Some(required_ids) = required_ids {
        if let Some(required_count) = required_count
            && required_count as usize != required_ids.len()
        {
            errors.push(format!(
                "required_evidence_count must match required_producer_ids length ({} != {})",
                required_count,
                required_ids.len()
            ));
        }
        for producer_id in required_ids {
            if !producer_states.contains_key(&producer_id) {
                errors.push(format!("required producer '{producer_id}' has no terminal result"));
            }
        }
    }

    if let (Some(required_count), Some(completed_count)) = (required_count, completed_count)
        && required_count != completed_count
    {
        errors.push(format!(
            "completed_evidence_count must equal required_evidence_count ({completed_count} != {required_count})"
        ));
    }

    match value.get("metrics") {
        Some(Value::Object(metrics))
            if metrics.get("base").and_then(Value::as_object).is_some()
                && metrics.get("head").and_then(Value::as_object).is_some() => {}
        _ => errors.push("metrics must contain base and head objects".to_string()),
    }

    match value.get("violations").and_then(Value::as_array) {
        Some(violations) => {
            for violation in violations {
                if violation.get("severity").and_then(Value::as_str) == Some("error") {
                    errors.push(
                        "pass verdict cannot contain an error-severity violation".to_string(),
                    );
                    break;
                }
            }
        }
        None => errors.push("field 'violations' must be an array".to_string()),
    }

    errors
}

fn validate_registered_parser_ratchet_shape(root: &Path, value: &Value) -> Result<()> {
    let registry_path = root.join(RECEIPT_REGISTRY_PATH);
    let registry_contents = fs::read_to_string(&registry_path)
        .with_context(|| format!("failed reading registry {}", registry_path.display()))?;
    let registry: ReceiptRegistry = toml::from_str(&registry_contents)
        .with_context(|| format!("invalid registry {}", registry_path.display()))?;
    let entry = registry
        .receipt
        .iter()
        .find(|entry| entry.check == PARSER_RATCHET_CHECK)
        .ok_or_else(|| color_eyre::eyre::eyre!("parser-ratchet is not registered"))?;

    let schema_path = root.join(&entry.schema);
    let schema_contents = fs::read_to_string(&schema_path)
        .with_context(|| format!("failed reading schema {}", schema_path.display()))?;
    let schema: Value = serde_json::from_str(&schema_contents)
        .with_context(|| format!("invalid schema JSON {}", schema_path.display()))?;

    let mut required = BTreeSet::new();
    for field in COMMON_RECEIPT_FIELDS {
        required.insert((*field).to_string());
    }
    collect_required_fields(&schema, &mut required);

    let missing =
        required.into_iter().filter(|field| value.get(field).is_none()).collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!("missing registered schema fields: {}", missing.join(", "));
    }

    Ok(())
}

fn collect_required_fields(schema: &Value, required: &mut BTreeSet<String>) {
    if let Some(fields) = schema.get("required").and_then(Value::as_array) {
        for field in fields.iter().filter_map(Value::as_str) {
            required.insert(field.to_string());
        }
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        for child in all_of {
            collect_required_fields(child, required);
        }
    }
}

fn require_string_eq(value: &Value, field: &str, expected: &str, errors: &mut Vec<String>) {
    match value.get(field).and_then(Value::as_str) {
        Some(actual) if actual == expected => {}
        Some(actual) => {
            errors.push(format!("field '{field}' must be '{expected}', got '{actual}'"))
        }
        None => errors.push(format!("field '{field}' must be a string")),
    }
}

fn require_bool_eq(value: &Value, field: &str, expected: bool, errors: &mut Vec<String>) {
    match value.get(field).and_then(Value::as_bool) {
        Some(actual) if actual == expected => {}
        Some(actual) => errors.push(format!("field '{field}' must be {expected}, got {actual}")),
        None => errors.push(format!("field '{field}' must be a boolean")),
    }
}

fn require_nonempty_string<'a>(
    value: &'a Value,
    field: &str,
    errors: &mut Vec<String>,
) -> Option<&'a str> {
    match value.get(field).and_then(Value::as_str) {
        Some(actual) if !actual.is_empty() => Some(actual),
        Some(_) => {
            errors.push(format!("field '{field}' must not be empty"));
            None
        }
        None => {
            errors.push(format!("field '{field}' must be a string"));
            None
        }
    }
}

fn require_nonempty_object_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
    errors: &mut Vec<String>,
) -> Option<&'a str> {
    match object.get(field).and_then(Value::as_str) {
        Some(actual) if !actual.is_empty() => Some(actual),
        Some(_) => {
            errors.push(format!("field '{field}' must not be empty"));
            None
        }
        None => {
            errors.push(format!("field '{field}' must be a string"));
            None
        }
    }
}

fn require_positive_object_integer(
    object: &serde_json::Map<String, Value>,
    field: &str,
    errors: &mut Vec<String>,
) -> Option<u64> {
    match object.get(field).and_then(Value::as_u64) {
        Some(value) if value > 0 => Some(value),
        Some(value) => {
            errors.push(format!("field '{field}' must be > 0, got {value}"));
            None
        }
        None => {
            errors.push(format!("field '{field}' must be a non-negative integer"));
            None
        }
    }
}

fn require_nonnegative_object_integer(
    object: &serde_json::Map<String, Value>,
    field: &str,
    errors: &mut Vec<String>,
) -> Option<u64> {
    match object.get(field).and_then(Value::as_u64) {
        Some(value) => Some(value),
        None => {
            errors.push(format!("field '{field}' must be a non-negative integer"));
            None
        }
    }
}

fn require_unique_nonempty_string_array(
    object: &serde_json::Map<String, Value>,
    field: &str,
    errors: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    let Some(values) = object.get(field).and_then(Value::as_array) else {
        errors.push(format!("field '{field}' must be an array"));
        return None;
    };
    if values.is_empty() {
        errors.push(format!("field '{field}' must not be empty"));
        return None;
    }

    let mut result = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        match value.as_str() {
            Some(value) if !value.is_empty() => {
                if !result.insert(value.to_string()) {
                    errors.push(format!("field '{field}' contains duplicate value '{value}'"));
                }
            }
            _ => errors.push(format!("field '{field}[{index}]' must be a non-empty string")),
        }
    }
    Some(result)
}

fn load_policy(root: &Path) -> Result<EvidencePolicy> {
    let path = root.join(DEFAULT_POLICY_PATH);
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed reading policy {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("invalid policy {}", path.display()))
}

fn extract_status(value: &Value) -> Option<String> {
    value
        .get("status")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| value.get("outcome").and_then(Value::as_str).map(ToString::to_string))
        .or_else(|| {
            value
                .get("success")
                .and_then(Value::as_bool)
                .map(|v| if v { "pass" } else { "fail" }.to_string())
        })
}
