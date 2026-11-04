use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET_FIELDS: &[&str] = &[
    "path",
    "scope",
    "plane",
    "disposition",
    "canonical_destination",
    "owner_issue",
    "claim_boundary",
];
const BENCH_FIELDS: &[&str] =
    &["path", "regime", "disposition", "canonical_destination", "owner_issue", "claim_boundary"];
const DOCUMENT_FIELDS: &[&str] = &["path", "disposition", "owner_issue"];
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
    "documents",
];

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_inventory() -> Result<Value, Box<dyn Error>> {
    let path = manifest_root().join("behavior_disposition.json");
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
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

/// Derive this package's executable test or benchmark targets from Cargo's own
/// target graph, including explicit `[[test]]`/`[[bench]]` paths that live
/// outside top-level `tests/` or `benches/` directories.
fn cargo_targets(kind: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(manifest_root())
        .output()?;
    if !output.status.success() {
        return Err(
            format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr)).into()
        );
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let manifest = manifest_root().join("Cargo.toml");
    let manifest = manifest.canonicalize()?;
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
    let root = manifest_root().canonicalize()?;
    let mut result = BTreeSet::new();
    for target in targets {
        let kinds = target
            .get("kind")
            .and_then(Value::as_array)
            .ok_or("cargo metadata target is missing kind")?;
        if !kinds.iter().any(|value| value.as_str() == Some(kind)) {
            continue;
        }
        let src_path = target
            .get("src_path")
            .and_then(Value::as_str)
            .ok_or("cargo metadata target is missing src_path")?;
        let relative = Path::new(src_path)
            .canonicalize()?
            .strip_prefix(&root)
            .map_err(|_| format!("{kind} target {src_path} lives outside the package"))?
            .to_string_lossy()
            .replace('\\', "/");
        result.insert(relative);
    }
    Ok(result)
}

/// Recursively collect source files that contain crate-local test code, so a
/// new `#[cfg(test)]` module outside `src/lib.rs` cannot escape disposition.
fn source_files_with_tests(
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
        // Whitespace-tolerant: Rust accepts `#[ test ]` and `#[ cfg(test) ]`.
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

fn require_closed_fields(
    entry: &Value,
    key: &str,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let object = entry.as_object().ok_or_else(|| format!("{key} entry must be an object"))?;
    let path = object.get("path").and_then(Value::as_str).unwrap_or("<missing path>").to_string();
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    for field in object.keys() {
        if !allowed.contains(field.as_str()) {
            return Err(format!("{key} entry {path} carries unknown field {field:?}").into());
        }
    }
    let mut values = BTreeMap::new();
    for field in &allowed {
        if *field == "owner_issue" {
            continue;
        }
        let value = object
            .get(*field)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} entry {path} is missing a string {field}"))?;
        if value.is_empty() {
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
    Ok(values)
}

fn require_target_policy(
    inventory: &Value,
    key: &str,
    allowed_fields: &[&str],
) -> Result<(), Box<dyn Error>> {
    for entry in entries(inventory, key)? {
        require_closed_fields(entry, key, allowed_fields)?;
    }
    Ok(())
}

#[test]
fn compatibility_package_is_explicitly_non_authoritative() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;
    let top_level = inventory.as_object().ok_or("inventory must be an object")?;
    let allowed: BTreeSet<&str> = INVENTORY_FIELDS.iter().copied().collect();
    for field in top_level.keys() {
        assert!(allowed.contains(field.as_str()), "inventory carries unknown field {field:?}");
    }
    assert_eq!(inventory.get("schema_version").and_then(Value::as_u64), Some(1));
    assert_eq!(inventory.get("owner_issue").and_then(Value::as_u64), Some(6971));
    assert_eq!(
        inventory.get("package").and_then(Value::as_str),
        Some(env!("CARGO_PKG_NAME")),
        "inventory must remain bound to this package"
    );
    assert!(
        inventory
            .get("classification_rule")
            .and_then(Value::as_str)
            .is_some_and(|rule| !rule.is_empty()),
        "inventory is missing a classification rule"
    );
    assert_eq!(inventory.get("behavior_authority").and_then(Value::as_bool), Some(false));
    assert_eq!(inventory.get("canonical_owner").and_then(Value::as_str), Some("perl-parser"));
    Ok(())
}

#[test]
fn every_compatibility_test_target_has_one_disposition() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;
    let listed = listed_paths(&inventory, "test_targets")?;
    let actual = cargo_targets("test")?;
    assert_eq!(
        listed, actual,
        "test target inventory drifted from Cargo's executable target graph"
    );

    for entry in entries(&inventory, "test_targets")? {
        let fields = require_closed_fields(entry, "test_targets", TARGET_FIELDS)?;
        assert_eq!(
            fields.get("scope").map(String::as_str),
            Some("all_tests_in_target"),
            "every #[test] function must inherit an explicit target disposition"
        );
        let path = fields.get("path").ok_or("test target is missing a path")?;
        let source = fs::read_to_string(manifest_root().join(path))?;
        assert!(source.contains("#[test]"), "classified target {path} contains no tests");
    }
    Ok(())
}

#[test]
fn source_tests_and_benchmarks_are_classified() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;

    let source_targets = listed_paths(&inventory, "source_test_targets")?;
    let actual_sources = source_files_with_tests(&manifest_root().join("src"), Path::new("src"))?;
    assert_eq!(
        source_targets, actual_sources,
        "crate-local test modules drifted from the source tree"
    );

    let listed_benches = listed_paths(&inventory, "benchmarks")?;
    let actual_benches = cargo_targets("bench")?;
    assert_eq!(
        listed_benches, actual_benches,
        "benchmark inventory drifted from Cargo's executable target graph"
    );

    require_target_policy(&inventory, "source_test_targets", TARGET_FIELDS)?;
    require_target_policy(&inventory, "benchmarks", BENCH_FIELDS)?;
    Ok(())
}

#[test]
fn governed_documents_exist_and_have_owners() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;
    for path in listed_paths(&inventory, "documents")? {
        assert!(manifest_root().join(&path).is_file(), "governed document is missing: {path}");
    }
    require_target_policy(&inventory, "documents", DOCUMENT_FIELDS)?;
    Ok(())
}
