//! Validate the transport-neutral compiler performance receipt schema.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const SCHEMA_PATH: &str = "schemas/compiler_performance_receipt.v1.schema.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/compiler_performance_receipt.v1.schema.json";
const VERSION: &str = "compiler_performance_receipt.v1";
const REQUIRED: &[&str] =
    &["schema_version", "receipt_id", "subject", "workload", "stages", "provider", "limitations"];
const STAGES: &[&str] = &[
    "upstream",
    "lex_parse",
    "hir",
    "pir",
    "effects",
    "module_graph",
    "world",
    "interface_invalidation",
    "fact_projection",
    "provider_request",
    "serialization",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerPerformanceReceipt {
    pub schema_version: String,
    pub receipt_id: String,
    pub subject: Subject,
    pub workload: Workload,
    pub stages: Vec<Stage>,
    pub provider: Provider,
    pub limitations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub repository: String,
    pub candidate: String,
    pub tree: String,
    pub dirty_tree: bool,
    pub toolchain: String,
    pub runner: String,
    pub identities: BTreeMap<String, Identity>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub schema: String,
    pub profile: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    pub id: String,
    pub class: String,
    pub profile: String,
    pub fixture: String,
    pub series: String,
    pub cache_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub name: String,
    pub applicability: String,
    pub result: String,
    pub work: Work,
    pub timing: Timing,
    pub instrumentation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub status: String,
    pub units: u64,
    pub objects: u64,
    pub bytes: u64,
    pub reused: u64,
    pub recomputed: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub status: String,
    pub wall_ns: Option<u64>,
    pub cpu_ns: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub requests: u64,
    pub exact: u64,
    pub partial: u64,
    pub fallback: u64,
    pub refusal: u64,
    pub correctness: Correctness,
    pub timing: Timing,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Correctness {
    pub false_exact: u64,
    pub stale_exact: u64,
    pub unsafe_edit: u64,
    pub unexplained_empty: u64,
}

pub fn run() -> Result<()> {
    let stats = validate(&project_root()?)?;
    println!(
        "compiler performance receipt schema check passed: {} required fields, {} stages",
        stats.0, stats.1
    );
    Ok(())
}

fn validate(root: &Path) -> Result<(usize, usize)> {
    let path = root.join(SCHEMA_PATH);
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?,
    )?;
    let mut errors = Vec::new();
    require_string(&schema, &["$id"], SCHEMA_ID, &mut errors);
    require_string(&schema, &["properties", "schema_version", "const"], VERSION, &mut errors);
    require_set(&schema, &["required"], REQUIRED, &mut errors);
    require_set(&schema, &["$defs", "stage", "properties", "name", "enum"], STAGES, &mut errors);
    require_set(
        &schema,
        &["$defs", "work", "required"],
        &["status", "units", "objects", "bytes", "reused", "recomputed"],
        &mut errors,
    );
    require_set(
        &schema,
        &["$defs", "provider", "required"],
        &["requests", "exact", "partial", "fallback", "refusal", "correctness", "timing"],
        &mut errors,
    );
    require_set(
        &schema,
        &["$defs", "correctness", "required"],
        &["false_exact", "stale_exact", "unsafe_edit", "unexplained_empty"],
        &mut errors,
    );
    if !errors.is_empty() {
        bail!("compiler performance receipt schema violations: {}", errors.join("; "));
    }
    Ok((REQUIRED.len(), STAGES.len()))
}

fn lookup<'a>(mut value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    for part in path {
        value = value.get(*part)?;
    }
    Some(value)
}

fn require_string(schema: &Value, path: &[&str], expected: &str, errors: &mut Vec<String>) {
    if lookup(schema, path).and_then(Value::as_str) != Some(expected) {
        errors.push(format!("{} must be {expected:?}", path.join(".")));
    }
}

fn require_set(schema: &Value, path: &[&str], expected: &[&str], errors: &mut Vec<String>) {
    let Some(values) = lookup(schema, path).and_then(Value::as_array) else {
        errors.push(format!("{} must be a string array", path.join(".")));
        return;
    };
    let actual = values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let wanted = expected.iter().copied().collect::<BTreeSet<_>>();
    for missing in wanted.difference(&actual) {
        errors.push(format!("{} missing {missing:?}", path.join(".")));
    }
    for extra in actual.difference(&wanted) {
        errors.push(format!("{} contains unsupported {extra:?}", path.join(".")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    fn workspace(schema: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("schemas")).unwrap();
        fs::write(dir.path().join(SCHEMA_PATH), schema).unwrap();
        dir
    }

    fn schema() -> String {
        include_str!("../../../schemas/compiler_performance_receipt.v1.schema.json").to_owned()
    }

    #[test]
    fn accepts_current_schema() {
        let dir = workspace(&schema());
        assert_eq!(validate(dir.path()).unwrap(), (REQUIRED.len(), STAGES.len()));
    }

    #[test]
    fn fixture_deserializes_into_typed_receipt() {
        let receipt: CompilerPerformanceReceipt = serde_json::from_str(include_str!(
            "../../fixtures/compiler_performance_receipt.v1.json"
        ))
        .unwrap();
        assert_eq!(receipt.schema_version, VERSION);
        assert_eq!(receipt.stages.len(), 1);
        assert_eq!(receipt.provider.correctness.false_exact, 0);
    }

    #[test]
    fn rejects_missing_stage_from_vocabulary() {
        let dir = workspace(&schema().replace("\"serialization\"", "\"serialization_removed\""));
        assert!(validate(dir.path()).is_err());
    }

    #[test]
    fn rejects_missing_work_status() {
        let dir = workspace(&schema().replace("\"status\", \"units\"", "\"units\""));
        assert!(validate(dir.path()).is_err());
    }

    #[test]
    fn rejects_provider_without_correctness() {
        let dir = workspace(&schema().replace("\"correctness\", \"timing\"", "\"timing\""));
        assert!(validate(dir.path()).is_err());
    }
}
