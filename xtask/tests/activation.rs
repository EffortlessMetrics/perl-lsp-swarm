//! Discriminating tests for the activation inventory schema, deterministic
//! generator, and fail-closed validator (#9204).

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::activation::{
    self as activation, ActivationClass, INVENTORY_PATH, load_overrides, validate_inventory_value,
    validate_overrides,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn canonical_inventory() -> TestResult<Value> {
    let bytes = std::fs::read(repo_root().join(INVENTORY_PATH))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn expect_violation(value: &Value, needle: &str) -> TestResult {
    let error = match validate_inventory_value(&repo_root(), value) {
        Ok(_) => "inventory unexpectedly validated".to_string(),
        Err(error) => error.to_string(),
    };
    assert!(error.contains(needle), "expected `{needle}` in:\n{error}");
    Ok(())
}

fn row_mut<'a>(inventory: &'a mut Value, surface_id: &str) -> Option<&'a mut Value> {
    inventory
        .get_mut("rows")?
        .as_array_mut()?
        .iter_mut()
        .find(|row| row.get("surface_id").and_then(Value::as_str) == Some(surface_id))
}

fn overrides_and_index()
-> TestResult<(activation::OverridesFile, BTreeMap<String, ActivationClass>)> {
    let file = load_overrides(&repo_root()).map_err(|error| error.to_string())?;
    let index = activation::derived_class_index(&repo_root()).map_err(|error| error.to_string())?;
    Ok((file, index))
}

fn expect_override_violation(
    file: &activation::OverridesFile,
    index: &BTreeMap<String, ActivationClass>,
    needle: &str,
) -> TestResult {
    let violations = validate_overrides(&repo_root(), file, index);
    assert!(
        violations.iter().any(|violation| violation.contains(needle)),
        "expected `{needle}` in: {violations:?}"
    );
    Ok(())
}

const EXPECTED_CLASS_COUNTS: &[(&str, usize)] = &[
    ("product", 16),
    ("preview", 2),
    ("compatibility_shim", 1),
    ("test_api", 10),
    ("lab", 21),
    ("oracle", 1),
    ("benchmark", 14),
    ("gate", 80),
];

// ---------------------------------------------------------------------------
// Positive proof
// ---------------------------------------------------------------------------

#[test]
fn committed_inventory_validates() -> TestResult {
    let inventory = activation::validate(&repo_root()).map_err(|error| error.to_string())?;
    let expected_total: usize = EXPECTED_CLASS_COUNTS.iter().map(|(_, count)| count).sum();
    assert_eq!(inventory.rows.len(), expected_total);
    Ok(())
}

#[test]
fn committed_inventory_matches_fresh_generation() -> TestResult {
    // Drift proof: `check_drift` regenerates in memory and byte-compares
    // against the committed `policy/activation-inventory.v1.json`.
    let inventory = activation::check_drift(&repo_root()).map_err(|error| error.to_string())?;
    assert!(!inventory.rows.is_empty());
    Ok(())
}

#[test]
fn generation_is_deterministic_across_runs_and_process_cwd() -> TestResult {
    let root = repo_root();
    let first = activation::generate(&root).map_err(|error| error.to_string())?;
    let second = activation::generate(&root).map_err(|error| error.to_string())?;
    assert_eq!(first, second, "two in-process generations must be structurally identical");
    assert_eq!(first.to_bytes()?, second.to_bytes()?);

    // Negative control: an implementation that (incorrectly) reads through a
    // relative path or `env::current_dir()` would produce different bytes,
    // or fail outright, once the process CWD differs from the repo root.
    let original_cwd = std::env::current_dir()?;
    let scratch = std::env::temp_dir().join(format!(
        "activation-cwd-independence-{}-{}",
        std::process::id(),
        first.rows.len()
    ));
    std::fs::create_dir_all(&scratch)?;
    std::env::set_current_dir(&scratch)?;
    let third = activation::generate(&root);
    let restore_result = std::env::set_current_dir(&original_cwd);
    let _ = std::fs::remove_dir_all(&scratch);
    restore_result?;
    let third = third.map_err(|error| error.to_string())?;

    assert_eq!(first, third, "generation must not depend on process CWD");
    assert_eq!(first.to_bytes()?, third.to_bytes()?);
    Ok(())
}

#[test]
fn exact_per_class_row_counts() -> TestResult {
    let inventory = activation::validate(&repo_root()).map_err(|error| error.to_string())?;
    let counts: BTreeMap<&str, usize> = activation::class_counts(&inventory)
        .into_iter()
        .map(|(class, count)| (class.as_str(), count))
        .collect();
    for (class, expected) in EXPECTED_CLASS_COUNTS {
        assert_eq!(
            counts.get(class).copied().unwrap_or(0),
            *expected,
            "class `{class}` row count drifted; a silent derivation change must fail this test"
        );
    }
    Ok(())
}

#[test]
fn every_activation_class_has_at_least_one_row() -> TestResult {
    let inventory = activation::validate(&repo_root()).map_err(|error| error.to_string())?;
    for (class, count) in activation::class_counts(&inventory) {
        assert!(count > 0, "class `{}` has zero rows", class.as_str());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: one test per named validation rule
// ---------------------------------------------------------------------------

#[test]
fn duplicate_surface_id_fails_validation() -> TestResult {
    let mut inventory = canonical_inventory()?;
    let duplicate =
        row_mut(&mut inventory, "gate:whitespace_check").ok_or("gate row not found")?.clone();
    inventory["rows"].as_array_mut().ok_or("rows is not an array")?.push(duplicate);
    expect_violation(&inventory, "duplicate surface id")
}

#[test]
fn unknown_activation_class_fails_validation() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "gate:whitespace_check").ok_or("gate row not found")?["class"] =
        json!("not_a_real_class");
    expect_violation(&inventory, "unknown activation class")
}

#[test]
fn artifact_must_validate_against_json_schema() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "gate:whitespace_check").ok_or("gate row not found")?["surface_id"] =
        json!(12345);
    expect_violation(&inventory, "schema:")
}

#[test]
fn product_row_requires_a_semantic_authority() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "feature:lsp.completion").ok_or("product row not found")?["semantic_authority"] =
        json!("");
    expect_violation(&inventory, "product row requires a semantic authority")
}

#[test]
fn product_row_requires_at_least_one_consumer() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "feature:lsp.completion").ok_or("product row not found")?["consumers"] =
        json!([]);
    expect_violation(&inventory, "product row requires at least one consumer")
}

#[test]
fn missing_authority_path_fails_validation() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "gate:whitespace_check").ok_or("gate row not found")?["semantic_authority"] =
        json!("not/a/real/authority/path.yaml");
    expect_violation(&inventory, "missing authority path")
}

#[test]
fn compatibility_shim_requires_retirement_owner_and_boundary() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "crate:perl-tree-sitter-compat")
        .ok_or("compat shim row not found")?
        .as_object_mut()
        .ok_or("compat shim row is not an object")?
        .remove("retirement");
    expect_violation(&inventory, "compatibility shim requires a retirement owner and boundary")
}

#[test]
fn rows_not_sorted_by_surface_id_fails_validation() -> TestResult {
    let mut inventory = canonical_inventory()?;
    let rows = inventory["rows"].as_array_mut().ok_or("rows is not an array")?;
    assert!(rows.len() > 1, "need at least two rows to prove a sort violation");
    rows.swap(0, 1);
    expect_violation(&inventory, "rows are not sorted by surface id")
}

#[test]
fn override_requires_an_owner() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].owner = None;
    expect_override_violation(&file, &index, "requires an owner")
}

#[test]
fn override_requires_a_review_after_date() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].review_after = None;
    expect_override_violation(&file, &index, "requires a review_after date")
}

#[test]
fn override_requires_a_reason() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].reason = None;
    expect_override_violation(&file, &index, "requires a reason")
}

#[test]
fn override_targets_an_unknown_surface() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].surface_id = "feature:not_a_real_feature".to_string();
    expect_override_violation(&file, &index, "targets an unknown surface")
}

#[test]
fn override_cannot_invent_a_crate_that_does_not_exist() -> TestResult {
    // `crate:` is the one surface-id kind an override may introduce rather
    // than reclassify. Without an existence check that escape hatch would let
    // the ledger name a surface no authority — and no directory — backs.
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].surface_id = "crate:not-a-real-crate".to_string();
    expect_override_violation(&file, &index, "no crates/not-a-real-crate/Cargo.toml")
}

#[test]
fn override_with_an_unknown_publication_state_fails_validation() -> TestResult {
    // Fail-open guard: an unparseable publication_state must be reported, not
    // silently drop the row out of the generated inventory.
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].publication_state = "not_a_real_state".to_string();
    expect_override_violation(&file, &index, "unknown publication_state")
}

#[test]
fn override_ledger_rejects_an_unsupported_schema_version() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.schema_version = 2;
    expect_override_violation(&file, &index, "unsupported schema_version")
}

#[test]
fn override_ledger_rejects_an_unexpected_policy_name() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.policy = "some-other-ledger".to_string();
    expect_override_violation(&file, &index, "unexpected policy")
}

#[test]
fn override_does_not_change_the_derived_class() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    assert_eq!(
        index.get("gate:whitespace_check").copied(),
        Some(ActivationClass::Gate),
        "fixture assumption: gate:whitespace_check must derive to `gate`"
    );
    file.overrides[0].surface_id = "gate:whitespace_check".to_string();
    file.overrides[0].class = "gate".to_string();
    expect_override_violation(&file, &index, "does not change the derived class")
}
