//! Shared disposition ledger validation for the compatibility package contract tests.

#![allow(dead_code)]

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SCHEMA_VERSION: u64 = 2;

const INVENTORY_FIELDS: &[&str] = &[
    "schema_version",
    "owner_issue",
    "package",
    "behavior_authority",
    "canonical_owner",
    "classification_rule",
    "source_test_targets",
    "test_targets",
    "benchmarks",
    "example_targets",
    "bin_targets",
    "documents",
];

const TARGET_FIELDS: &[&str] = &[
    "path",
    "scope",
    "plane",
    "evidence_class",
    "disposition",
    "canonical_destination",
    "owner_issue",
    "claim_boundary",
    "required_features",
];

const BENCH_FIELDS: &[&str] = &[
    "path",
    "regime",
    "disposition",
    "canonical_destination",
    "owner_issue",
    "claim_boundary",
    "required_features",
];

const DOCUMENT_FIELDS: &[&str] = &["path", "disposition", "owner_issue"];

const SCOPES: &[&str] = &["all_tests_in_target"];

const PLANES: &[&str] =
    &["compatibility", "mixed_behavior", "metrics", "equivalence", "experimental_checkpoint_cache"];

const EVIDENCE_CLASSES: &[&str] = &["unique", "duplicate", "mixed"];

const TARGET_DISPOSITIONS: &[&str] = &[
    "retain_compatibility_only",
    "migrate_then_reduce",
    "rewrite_or_quarantine",
    "migrate_then_remove",
    "quarantine_experimental",
];

const BENCH_DISPOSITIONS: &[&str] = &["quarantine_and_rewrite"];

const REGIMES: &[&str] = &["historical_mixed_incremental"];

const DOCUMENT_DISPOSITIONS: &[&str] =
    &["compatibility_boundary", "historical_non_authoritative", "migration_guidance"];

const MIGRATION_DISPOSITIONS: &[&str] = &["migrate_then_reduce", "migrate_then_remove"];

const EXPERIMENTAL_MARKERS: &[&str] = &[
    "IncrementalDocument",
    "incremental_document::",
    "incremental_v2::",
    "incremental_advanced_reuse::",
    "CheckpointedIncrementalParser",
    "incremental_checkpoint::",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CargoTargetSnapshot {
    pub tests: BTreeMap<String, String>,
    pub benches: BTreeMap<String, String>,
    pub examples: BTreeMap<String, String>,
    pub bins: BTreeMap<String, String>,
}

pub fn paths(snapshot: &BTreeMap<String, String>) -> BTreeSet<String> {
    snapshot.keys().cloned().collect()
}

pub fn load_inventory_from_path(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn validate_inventory(
    inventory: &Value,
    package_root: &Path,
    package_name: &str,
    cargo: &CargoTargetSnapshot,
    source_tests: &BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    validate_top_level(inventory, package_name)?;
    validate_target_section(
        inventory,
        "test_targets",
        TARGET_FIELDS,
        &cargo.tests,
        package_root,
        TargetKind::IntegrationTest,
    )?;
    validate_source_test_section(inventory, source_tests, package_root)?;
    validate_target_section(
        inventory,
        "benchmarks",
        BENCH_FIELDS,
        &cargo.benches,
        package_root,
        TargetKind::Bench,
    )?;
    validate_path_only_section(inventory, "example_targets", &cargo.examples, package_root)?;
    validate_path_only_section(inventory, "bin_targets", &cargo.bins, package_root)?;
    validate_documents(inventory, package_root)?;
    Ok(())
}

enum TargetKind {
    IntegrationTest,
    SourceUnitTest,
    Bench,
}

fn validate_top_level(inventory: &Value, package_name: &str) -> Result<(), Box<dyn Error>> {
    let top_level = inventory.as_object().ok_or("inventory must be an object")?;
    let allowed: BTreeSet<&str> = INVENTORY_FIELDS.iter().copied().collect();
    for field in top_level.keys() {
        if !allowed.contains(field.as_str()) {
            return Err(format!("inventory carries unknown field {field:?}").into());
        }
    }
    if inventory.get("schema_version").and_then(Value::as_u64) != Some(SCHEMA_VERSION) {
        return Err(format!("inventory schema_version must be {SCHEMA_VERSION}").into());
    }
    if inventory.get("owner_issue").and_then(Value::as_u64) != Some(6971) {
        return Err("inventory owner_issue must be 6971".into());
    }
    if inventory.get("package").and_then(Value::as_str) != Some(package_name) {
        return Err("inventory package name must match the governed crate".into());
    }
    if inventory.get("behavior_authority").and_then(Value::as_bool) != Some(false) {
        return Err("compatibility package must declare behavior_authority=false".into());
    }
    if inventory.get("canonical_owner").and_then(Value::as_str) != Some("perl-parser") {
        return Err("inventory canonical_owner must be perl-parser".into());
    }
    let rule = inventory
        .get("classification_rule")
        .and_then(Value::as_str)
        .ok_or("inventory is missing classification_rule")?;
    if rule.is_empty() {
        return Err("inventory classification_rule must be non-empty".into());
    }
    Ok(())
}

fn entries<'a>(inventory: &'a Value, key: &str) -> Result<&'a [Value], Box<dyn Error>> {
    inventory
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("inventory field {key:?} must be an array").into())
}

fn listed_paths(inventory: &Value, key: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut result = BTreeSet::new();
    for entry in entries(inventory, key)? {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} entry is missing a string path"))?;
        if !result.insert(path.to_string()) {
            return Err(format!("duplicate {key} path: {path}").into());
        }
    }
    Ok(result)
}

fn validate_path_only_section(
    inventory: &Value,
    key: &str,
    actual: &BTreeMap<String, String>,
    package_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let listed = listed_paths(inventory, key)?;
    if listed != paths(actual) {
        return Err(format!("{key} inventory drifted from Cargo's executable target graph").into());
    }
    for entry in entries(inventory, key)? {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} entry is missing a string path"))?;
        let object = entry.as_object().ok_or_else(|| format!("{key} entry must be an object"))?;
        let ledger_features = object
            .get("required_features")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} entry {path} is missing required_features"))?;
        let cargo_features =
            actual.get(path).ok_or_else(|| format!("{key} entry {path} is not a Cargo target"))?;
        if ledger_features != cargo_features {
            return Err(format!(
                "{key} entry {path} required_features={ledger_features:?} does not match Cargo invocation identity {cargo_features:?}"
            )
            .into());
        }
    }
    for path in &listed {
        let file = package_root.join(path);
        if !file.is_file() {
            return Err(format!("{key} entry references missing file: {path}").into());
        }
    }
    Ok(())
}

fn validate_target_section(
    inventory: &Value,
    key: &str,
    allowed_fields: &[&str],
    actual: &BTreeMap<String, String>,
    package_root: &Path,
    kind: TargetKind,
) -> Result<(), Box<dyn Error>> {
    let listed = listed_paths(inventory, key)?;
    if listed != paths(actual) {
        return Err(format!("{key} inventory drifted from the governed target inventory").into());
    }
    for entry in entries(inventory, key)? {
        let fields = require_closed_fields(entry, key, allowed_fields)?;
        let path = fields.get("path").ok_or("entry missing path")?;
        let cargo_features =
            actual.get(path).ok_or_else(|| format!("{key} entry {path} is not a Cargo target"))?;
        let ledger_features =
            fields.get("required_features").ok_or("entry missing required_features")?;
        if ledger_features != cargo_features {
            return Err(format!(
                "{key} entry {path} required_features={ledger_features:?} does not match Cargo invocation identity {cargo_features:?}"
            )
            .into());
        }
        validate_target_semantics(key, &fields, &kind, package_root)?;
    }
    Ok(())
}

fn validate_source_test_section(
    inventory: &Value,
    actual: &BTreeSet<String>,
    package_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let listed = listed_paths(inventory, "source_test_targets")?;
    if listed != *actual {
        return Err("source_test_targets inventory drifted from the source tree".into());
    }
    for entry in entries(inventory, "source_test_targets")? {
        let fields = require_closed_fields(entry, "source_test_targets", TARGET_FIELDS)?;
        let required_features =
            fields.get("required_features").ok_or("source test row missing required_features")?;
        if required_features != "default" {
            return Err(format!(
                "source_test_targets entry {} must use required_features=default (feature-gated unit tests need explicit function-level rows)",
                fields.get("path").unwrap_or(&"<missing>".to_string())
            )
            .into());
        }
        validate_target_semantics(
            "source_test_targets",
            &fields,
            &TargetKind::SourceUnitTest,
            package_root,
        )?;
    }
    Ok(())
}

fn validate_documents(inventory: &Value, package_root: &Path) -> Result<(), Box<dyn Error>> {
    for path in listed_paths(inventory, "documents")? {
        if !package_root.join(&path).is_file() {
            return Err(format!("governed document is missing: {path}").into());
        }
    }
    for entry in entries(inventory, "documents")? {
        let fields = require_closed_fields(entry, "documents", DOCUMENT_FIELDS)?;
        let path = fields.get("path").ok_or("document entry missing path")?;
        let disposition = fields.get("disposition").ok_or("document missing disposition")?;
        require_enum("documents", path, "disposition", disposition, DOCUMENT_DISPOSITIONS)?;
    }
    Ok(())
}

fn require_closed_fields(
    entry: &Value,
    key: &str,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let object = entry.as_object().ok_or_else(|| format!("{key} entry must be an object"))?;
    let path = object.get("path").and_then(Value::as_str).unwrap_or("<missing path>").to_string();
    let allowed_set: BTreeSet<&str> = allowed.iter().copied().collect();
    for field in object.keys() {
        if !allowed_set.contains(field.as_str()) {
            return Err(format!("{key} entry {path} carries unknown field {field:?}").into());
        }
    }
    let mut values = BTreeMap::new();
    for field in allowed {
        if *field == "owner_issue" {
            continue;
        }
        let value = object
            .get(*field)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} entry {path} is missing a string {field}"))?;
        if value.is_empty() || value.trim().is_empty() {
            return Err(format!("{key} entry {path} has an empty {field}").into());
        }
        values.insert((*field).to_string(), value.to_string());
    }
    let owner_issue = object
        .get("owner_issue")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{key} entry {path} is missing an owner issue"))?;
    if owner_issue == 0 {
        return Err(format!("{key} entry {path} has an invalid owner issue").into());
    }
    values.insert("owner_issue".to_string(), owner_issue.to_string());
    Ok(values)
}

fn require_enum(
    key: &str,
    path: &str,
    field: &str,
    value: &str,
    allowed: &[&str],
) -> Result<(), Box<dyn Error>> {
    if !allowed.contains(&value) {
        return Err(format!(
            "{key} entry {path} has invalid {field} {value:?}; allowed: {allowed:?}"
        )
        .into());
    }
    Ok(())
}

fn validate_target_semantics(
    key: &str,
    fields: &BTreeMap<String, String>,
    kind: &TargetKind,
    package_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let path = fields.get("path").ok_or("entry missing path")?;
    match kind {
        TargetKind::Bench => {
            let regime = fields.get("regime").ok_or("bench missing regime")?;
            require_enum(key, path, "regime", regime, REGIMES)?;
            let disposition = fields.get("disposition").ok_or("bench missing disposition")?;
            require_enum(key, path, "disposition", disposition, BENCH_DISPOSITIONS)?;
            if disposition.contains("quarantine") && fields.get("owner_issue").is_none() {
                return Err(format!("bench {path} quarantine requires owner_issue").into());
            }
        }
        TargetKind::IntegrationTest | TargetKind::SourceUnitTest => {
            let scope = fields.get("scope").ok_or("target missing scope")?;
            require_enum(key, path, "scope", scope, SCOPES)?;
            let plane = fields.get("plane").ok_or("target missing plane")?;
            require_enum(key, path, "plane", plane, PLANES)?;
            let evidence_class =
                fields.get("evidence_class").ok_or("target missing evidence_class")?;
            require_enum(key, path, "evidence_class", evidence_class, EVIDENCE_CLASSES)?;
            let disposition = fields.get("disposition").ok_or("target missing disposition")?;
            require_enum(key, path, "disposition", disposition, TARGET_DISPOSITIONS)?;
            let source = fs::read_to_string(package_root.join(path))?;
            if matches!(kind, TargetKind::IntegrationTest) && !source.contains("#[test]") {
                return Err(
                    format!("classified integration target {path} contains no tests").into()
                );
            }
            if matches!(kind, TargetKind::SourceUnitTest) {
                let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
                if !compact.contains("#[test]") && !compact.contains("#[cfg(test)]") {
                    return Err(
                        format!("classified source target {path} contains no unit tests").into()
                    );
                }
            }
        }
    }

    let disposition = fields.get("disposition").ok_or("entry missing disposition")?;
    if MIGRATION_DISPOSITIONS.contains(&disposition.as_str()) {
        let destination = fields.get("canonical_destination").map(String::as_str).unwrap_or("");
        if destination.is_empty() {
            return Err(format!(
                "{key} entry {path} migration disposition requires canonical_destination"
            )
            .into());
        }
    }

    if disposition == "retain_compatibility_only" {
        let plane = fields.get("plane").ok_or("retain row missing plane")?;
        if plane != "compatibility" {
            return Err(format!(
                "{key} entry {path} retain_compatibility_only requires plane=compatibility"
            )
            .into());
        }
        let source = fs::read_to_string(package_root.join(path))?;
        for marker in EXPERIMENTAL_MARKERS {
            if source.contains(marker) {
                return Err(format!(
                    "{key} entry {path} retain_compatibility_only must not exercise experimental generation ({marker})"
                )
                .into());
            }
        }
    }

    if disposition.contains("quarantine") {
        let owner = fields.get("owner_issue").ok_or("quarantine row missing owner")?;
        if owner == "0" {
            return Err(
                format!("{key} entry {path} quarantine disposition requires owner_issue").into()
            );
        }
    }

    Ok(())
}

fn cargo_invocation_identity(target: &Value) -> Result<String, Box<dyn Error>> {
    let required = target.get("required_features");
    let Some(required) = required else {
        return Ok("default".to_string());
    };
    let Some(features) = required.as_array() else {
        return Err("cargo metadata target required_features must be an array when present".into());
    };
    let mut names = BTreeSet::new();
    for feature in features {
        let name = feature
            .as_str()
            .ok_or("cargo metadata target required_features entries must be strings")?;
        if name.is_empty() {
            return Err(
                "cargo metadata target required_features must not contain empty names".into()
            );
        }
        names.insert(name.to_string());
    }
    if names.is_empty() {
        Ok("default".to_string())
    } else {
        Ok(names.into_iter().collect::<Vec<_>>().join(","))
    }
}

fn insert_target_path(
    map: &mut BTreeMap<String, String>,
    relative: &str,
    identity: String,
) -> Result<(), Box<dyn Error>> {
    if map.insert(relative.to_string(), identity).is_some() {
        return Err(format!("duplicate Cargo target path in snapshot: {relative}").into());
    }
    Ok(())
}

pub fn cargo_snapshot_from_metadata(
    package_root: &Path,
) -> Result<CargoTargetSnapshot, Box<dyn Error>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(package_root)
        .output()?;
    if !output.status.success() {
        return Err(
            format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)).into()
        );
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let manifest = package_root.join("Cargo.toml").canonicalize()?;
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("cargo metadata reports no packages")?;
    let package = packages
        .iter()
        .find(|package| {
            package
                .get("manifest_path")
                .and_then(Value::as_str)
                .and_then(|path| Path::new(path).canonicalize().ok())
                .is_some_and(|path| path == manifest)
        })
        .ok_or("cargo metadata does not describe this package")?;
    let targets = package
        .get("targets")
        .and_then(Value::as_array)
        .ok_or("cargo metadata reports no targets for this package")?;
    let root = package_root.canonicalize()?;
    let mut snapshot = CargoTargetSnapshot::default();
    for target in targets {
        let kinds = target
            .get("kind")
            .and_then(Value::as_array)
            .ok_or("cargo metadata target is missing kind")?;
        let src_path = target
            .get("src_path")
            .and_then(Value::as_str)
            .ok_or("cargo metadata target is missing src_path")?;
        let relative = Path::new(src_path)
            .canonicalize()?
            .strip_prefix(&root)
            .map_err(|_| format!("target {src_path} lives outside the package"))?
            .to_string_lossy()
            .replace('\\', "/");
        for kind in kinds {
            let identity = cargo_invocation_identity(target)?;
            match kind.as_str() {
                Some("test") => {
                    insert_target_path(&mut snapshot.tests, &relative, identity.clone())?
                }
                Some("bench") => {
                    insert_target_path(&mut snapshot.benches, &relative, identity.clone())?
                }
                Some("example") => {
                    insert_target_path(&mut snapshot.examples, &relative, identity.clone())?
                }
                Some("bin") => insert_target_path(&mut snapshot.bins, &relative, identity.clone())?,
                _ => {}
            }
        }
    }
    Ok(snapshot)
}

pub fn source_files_with_tests(
    directory: &Path,
    prefix: &Path,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut result = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            result.extend(source_files_with_tests(&path, &prefix.join(entry_file_name(&path)?))?);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains("#[test]") || compact.contains("#[cfg(test)]") {
            let relative = prefix.join(entry_file_name(&path)?);
            result.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(result)
}

fn entry_file_name(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("source target has a non-UTF-8 file name")?
        .to_string())
}

pub fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
