use serde_json::Value;
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

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

fn rust_targets(directory: &Path, prefix: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut result = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("Rust target has a non-UTF-8 file name")?;
        result.insert(format!("{prefix}/{file_name}"));
    }
    Ok(result)
}

fn require_target_policy(inventory: &Value, key: &str) -> Result<(), Box<dyn Error>> {
    for entry in entries(inventory, key)? {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} entry is missing a path"))?;
        let disposition = entry
            .get("disposition")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{path} is missing a disposition"))?;
        if disposition.is_empty() {
            return Err(format!("{path} has an empty disposition").into());
        }
        let owner_issue = entry
            .get("owner_issue")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{path} is missing an owner issue"))?;
        if owner_issue == 0 {
            return Err(format!("{path} has an invalid owner issue").into());
        }
    }
    Ok(())
}

#[test]
fn compatibility_package_is_explicitly_non_authoritative() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;
    assert_eq!(inventory.get("schema_version").and_then(Value::as_u64), Some(1));
    assert_eq!(inventory.get("owner_issue").and_then(Value::as_u64), Some(6971));
    assert_eq!(inventory.get("behavior_authority").and_then(Value::as_bool), Some(false));
    assert_eq!(
        inventory.get("canonical_owner").and_then(Value::as_str),
        Some("perl-parser")
    );
    Ok(())
}

#[test]
fn every_compatibility_test_target_has_one_disposition() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;
    let listed = listed_paths(&inventory, "test_targets")?;
    let actual = rust_targets(&manifest_root().join("tests"), "tests")?;
    assert_eq!(listed, actual, "test target inventory drifted");

    for entry in entries(&inventory, "test_targets")? {
        assert_eq!(
            entry.get("scope").and_then(Value::as_str),
            Some("all_tests_in_target"),
            "every #[test] function must inherit an explicit target disposition"
        );
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or("test target is missing a path")?;
        let source = fs::read_to_string(manifest_root().join(path))?;
        assert!(source.contains("#[test]"), "classified target {path} contains no tests");
    }

    require_target_policy(&inventory, "test_targets")?;
    Ok(())
}

#[test]
fn source_tests_and_benchmarks_are_classified() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;

    let source_targets = listed_paths(&inventory, "source_test_targets")?;
    assert_eq!(source_targets, BTreeSet::from(["src/lib.rs".to_string()]));
    let source = fs::read_to_string(manifest_root().join("src/lib.rs"))?;
    assert!(source.contains("#[cfg(test)]"));
    assert!(source.contains("#[test]"));

    let listed_benches = listed_paths(&inventory, "benchmarks")?;
    let actual_benches = rust_targets(&manifest_root().join("benches"), "benches")?;
    assert_eq!(listed_benches, actual_benches, "benchmark inventory drifted");

    require_target_policy(&inventory, "source_test_targets")?;
    require_target_policy(&inventory, "benchmarks")?;
    Ok(())
}

#[test]
fn governed_documents_exist_and_have_owners() -> Result<(), Box<dyn Error>> {
    let inventory = load_inventory()?;
    for path in listed_paths(&inventory, "documents")? {
        assert!(manifest_root().join(&path).is_file(), "governed document is missing: {path}");
    }
    require_target_policy(&inventory, "documents")?;
    Ok(())
}