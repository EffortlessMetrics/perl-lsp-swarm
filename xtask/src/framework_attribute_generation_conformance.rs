//! Validator for the framework attribute-generation conformance manifest (#2373).

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "framework_attribute_generation_conformance.v1";
pub const MANIFEST_NAME: &str = "framework-attribute-generation-conformance";
pub const MANIFEST_PATH: &str = ".spec/2373-framework-attribute-generation/manifest.json";
pub const SCHEMA_PATH: &str = "schemas/framework_attribute_generation_conformance.v1.schema.json";
pub const FIXTURE_ROOT: &str = ".spec/2373-framework-attribute-generation/fixtures";

const REQUIRED_PROBE_IDS: [u32; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

#[derive(Debug, Clone, Copy)]
pub struct ValidationStats {
    pub fixtures: usize,
    pub cases: usize,
    pub failure_probes: usize,
}

#[derive(Debug)]
pub enum ConformanceValidationError {
    Io(String),
    Json(String),
    Violation(String),
}

impl Display for ConformanceValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::Json(message) | Self::Violation(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl Error for ConformanceValidationError {}

pub fn load_manifest(root: &Path) -> Result<Value, ConformanceValidationError> {
    let bytes = fs::read(root.join(MANIFEST_PATH))
        .map_err(|error| ConformanceValidationError::Io(format!("read manifest: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| ConformanceValidationError::Json(format!("parse manifest: {error}")))
}

pub fn validate_manifest_file(root: &Path) -> Result<ValidationStats, ConformanceValidationError> {
    let manifest = load_manifest(root)?;
    validate_manifest_value(root, &manifest)
}

pub fn validate_manifest_value(
    root: &Path,
    manifest: &Value,
) -> Result<ValidationStats, ConformanceValidationError> {
    if manifest.get("schema_version").and_then(Value::as_str) != Some(SCHEMA_VERSION) {
        return Err(violation(format!("schema_version mismatch: expected `{SCHEMA_VERSION}`")));
    }
    if manifest.get("manifest").and_then(Value::as_str) != Some(MANIFEST_NAME) {
        return Err(violation(format!("manifest mismatch: expected `{MANIFEST_NAME}`")));
    }
    if manifest.get("issue").and_then(Value::as_i64) != Some(2373) {
        return Err(violation("issue mismatch: expected `2373`".to_string()));
    }

    let fixtures = manifest
        .get("fixtures")
        .and_then(Value::as_object)
        .ok_or_else(|| violation("fixtures must be an object"))?;
    verify_fixture_digests(root, fixtures)?;

    let cases = manifest
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| violation("cases must be an array"))?;
    let case_ids = verify_cases(fixtures, cases)?;

    let probes = manifest
        .get("failure_probes")
        .and_then(Value::as_array)
        .ok_or_else(|| violation("failure_probes must be an array"))?;
    verify_failure_probes(probes, &case_ids)?;

    verify_matrices(manifest.get("matrices"), &case_ids)?;

    let classes = manifest
        .get("expectation_classes")
        .and_then(Value::as_array)
        .ok_or_else(|| violation("expectation_classes must be an array"))?;
    if classes.len() < 6 {
        return Err(violation("expectation_classes must enumerate distinct classes"));
    }

    Ok(ValidationStats {
        fixtures: fixtures.len(),
        cases: cases.len(),
        failure_probes: probes.len(),
    })
}

fn verify_fixture_digests(
    root: &Path,
    fixtures: &serde_json::Map<String, Value>,
) -> Result<(), ConformanceValidationError> {
    for (relative, record) in fixtures {
        let expected = record
            .get("digest")
            .and_then(Value::as_str)
            .ok_or_else(|| violation(format!("fixture `{relative}` missing digest")))?;
        let path = root.join(FIXTURE_ROOT).join(relative);
        let bytes = fs::read(&path).map_err(|error| {
            ConformanceValidationError::Io(format!("read fixture `{relative}`: {error}"))
        })?;
        let actual = format!(
            "sha256:{}",
            Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        );
        if actual != expected {
            return Err(violation(format!(
                "fixture `{relative}` digest mismatch: expected {expected}, got {actual}"
            )));
        }
    }
    Ok(())
}

fn verify_cases(
    fixtures: &serde_json::Map<String, Value>,
    cases: &[Value],
) -> Result<BTreeSet<String>, ConformanceValidationError> {
    let mut ids = BTreeSet::new();
    for case in cases {
        let case_id = case
            .get("case_id")
            .and_then(Value::as_str)
            .ok_or_else(|| violation("case missing case_id"))?;
        if !ids.insert(case_id.to_string()) {
            return Err(violation(format!("duplicate case_id `{case_id}`")));
        }
        let fixture = case
            .get("fixture")
            .and_then(Value::as_str)
            .ok_or_else(|| violation(format!("case `{case_id}` missing fixture")))?;
        if !fixtures.contains_key(fixture) {
            return Err(violation(format!(
                "case `{case_id}` references unknown fixture `{fixture}`"
            )));
        }
        let expectation = case
            .get("expectation")
            .and_then(Value::as_object)
            .ok_or_else(|| violation(format!("case `{case_id}` missing expectation object")))?;
        for member in
            expectation.get("provided_members").and_then(Value::as_array).into_iter().flatten()
        {
            if member.get("generator_anchor").and_then(Value::as_str) != Some("has_declaration") {
                return Err(violation(format!(
                    "case `{case_id}` provided member must use has_declaration anchor"
                )));
            }
            if member.get("method_body_range").and_then(Value::as_str) != Some("none") {
                return Err(violation(format!(
                    "case `{case_id}` must not invent a generated method-body range"
                )));
            }
        }
        for hook in
            expectation.get("referenced_hooks").and_then(Value::as_array).into_iter().flatten()
        {
            if hook.get("provided_by_attribute").and_then(Value::as_bool) != Some(false) {
                return Err(violation(format!(
                    "case `{case_id}` referenced hook must not be marked provided"
                )));
            }
        }
        if expectation
            .get("constraint")
            .and_then(|value| value.get("object_return_proof"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return Err(violation(format!(
                "case `{case_id}` must not claim object_return_proof from this corpus"
            )));
        }
    }
    Ok(ids)
}

fn verify_failure_probes(
    probes: &[Value],
    case_ids: &BTreeSet<String>,
) -> Result<(), ConformanceValidationError> {
    if probes.len() != 10 {
        return Err(violation(format!(
            "failure_probes must contain exactly 10 probes, found {}",
            probes.len()
        )));
    }
    let mut seen = BTreeSet::new();
    for probe in probes {
        let probe_id = probe
            .get("probe_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| violation("failure probe missing probe_id"))?
            as u32;
        if !seen.insert(probe_id) {
            return Err(violation(format!("duplicate failure probe_id {probe_id}")));
        }
        let case_id = probe
            .get("case_id")
            .and_then(Value::as_str)
            .ok_or_else(|| violation("failure probe missing case_id"))?;
        if !case_ids.contains(case_id) {
            return Err(violation(format!(
                "failure probe {probe_id} references unknown case `{case_id}`"
            )));
        }
    }
    for required in REQUIRED_PROBE_IDS {
        if !seen.contains(&required) {
            return Err(violation(format!("missing failure probe_id {required}")));
        }
    }
    Ok(())
}

fn verify_matrices(
    matrices: Option<&Value>,
    case_ids: &BTreeSet<String>,
) -> Result<(), ConformanceValidationError> {
    let matrices = matrices
        .and_then(Value::as_object)
        .ok_or_else(|| violation("matrices must be an object"))?;
    for (name, matrix) in matrices {
        let ids = matrix
            .get("case_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| violation(format!("matrix `{name}` missing case_ids")))?;
        for id in ids {
            let id = id
                .as_str()
                .ok_or_else(|| violation(format!("matrix `{name}` case_ids must be strings")))?;
            if !case_ids.contains(id) {
                return Err(violation(format!("matrix `{name}` references unknown case `{id}`")));
            }
        }
    }
    Ok(())
}

fn violation(message: impl Into<String>) -> ConformanceValidationError {
    ConformanceValidationError::Violation(message.into())
}

#[must_use]
pub fn list_case_ids(manifest: &Value) -> Vec<String> {
    manifest
        .get("cases")
        .and_then(Value::as_array)
        .map(|cases| {
            cases
                .iter()
                .filter_map(|case| case.get("case_id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[must_use]
pub fn fixture_path(root: &Path, relative: &str) -> PathBuf {
    root.join(FIXTURE_ROOT).join(relative)
}
