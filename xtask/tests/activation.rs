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
    ("test_api", 13),
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
fn fuzz_surface_identity_follows_the_registered_target_name() -> TestResult {
    // `fuzz/fuzz_targets/fuzz_target_1.rs` is registered in fuzz/Cargo.toml as
    // `[[bin]] name = "parser_integration"`. The runnable target is what a
    // consumer names, so the source stem must not become the surface id.
    let inventory = activation::validate(&repo_root()).map_err(|error| error.to_string())?;
    let ids: Vec<&str> = inventory.rows.iter().map(|row| row.surface_id.as_str()).collect();
    assert!(ids.contains(&"fuzz:parser_integration"), "registered target name missing");
    assert!(!ids.contains(&"fuzz:fuzz_target_1"), "source file stem must not be the surface id");
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
fn override_review_after_must_be_an_iso_date() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].review_after = Some("soon".to_string());
    expect_override_violation(&file, &index, "is not an ISO `YYYY-MM-DD` date")
}

#[test]
fn override_review_after_rejects_an_impossible_month() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].review_after = Some("2026-13-01".to_string());
    expect_override_violation(&file, &index, "is not an ISO `YYYY-MM-DD` date")
}

#[test]
fn override_review_after_rejects_an_impossible_calendar_day() -> TestResult {
    // Bounding month and day independently accepts 2026-02-31; a real
    // calendar check is what makes the expiry a date rather than a shape.
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].review_after = Some("2026-02-31".to_string());
    expect_override_violation(&file, &index, "is not an ISO `YYYY-MM-DD` date")
}

#[test]
fn override_review_after_rejects_a_non_leap_february_29() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].review_after = Some("2027-02-29".to_string());
    expect_override_violation(&file, &index, "is not an ISO `YYYY-MM-DD` date")
}

#[test]
fn override_review_after_accepts_a_real_leap_day() -> TestResult {
    // The control that keeps the calendar check from being over-strict.
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].review_after = Some("2028-02-29".to_string());
    let violations = validate_overrides(&repo_root(), &file, &index);
    assert!(
        !violations.iter().any(|violation| violation.contains("ISO `YYYY-MM-DD`")),
        "a real leap day must be accepted: {violations:?}"
    );
    Ok(())
}

#[test]
fn override_authority_path_must_exist() -> TestResult {
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].semantic_authority = "policy/not-a-real-ledger.toml".to_string();
    expect_override_violation(&file, &index, "missing authority path")
}

#[test]
fn override_toml_authority_fragment_must_resolve() -> TestResult {
    // The path exists, so a path-only check would pass this. The fragment is
    // what a human gets wrong: `#publish` looks right but the real Cargo key
    // is `package.publish`.
    let (mut file, index) = overrides_and_index()?;
    file.overrides[0].publication_authority =
        "crates/perl-tree-sitter-compat/Cargo.toml#publish".to_string();
    expect_override_violation(&file, &index, "has no key `publish`")
}

#[test]
fn override_ledger_rejects_an_unknown_field() -> TestResult {
    // A misspelled optional key must not deserialize into a silent default.
    let text = std::fs::read_to_string(repo_root().join("policy/activation-overrides.toml"))?;
    let typo = text.replace("compile_profiles = ", "compile_profile = ");
    assert_ne!(typo, text, "fixture assumption: the ledger declares compile_profiles");
    let parsed: Result<activation::OverridesFile, _> = toml::from_str(&typo);
    assert!(parsed.is_err(), "a misspelled override key must fail to parse");
    Ok(())
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

#[test]
fn override_cannot_reclassify_a_derived_surface() -> TestResult {
    // The dangerous case is not the no-op above but the silent demotion: an
    // override quietly replacing a derived `product` row with its own answer.
    let (mut file, index) = overrides_and_index()?;
    assert_eq!(
        index.get("feature:lsp.hover").copied(),
        Some(ActivationClass::Product),
        "fixture assumption: feature:lsp.hover must derive to `product`"
    );
    file.overrides[0].surface_id = "feature:lsp.hover".to_string();
    file.overrides[0].class = "lab".to_string();
    expect_override_violation(&file, &index, "would reclassify a derived surface")
}

#[test]
fn product_row_cannot_be_unowned() -> TestResult {
    // The `unowned` token is admissible on a preview row whose authority
    // records no implementation crate. On a product row it is a contradiction.
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "feature:lsp.completion").ok_or("product row not found")?["owner"] =
        json!(activation::UNOWNED);
    expect_violation(&inventory, "product row requires a real owner")
}

#[test]
fn unowned_preview_rows_say_so_instead_of_inventing_an_owner() -> TestResult {
    // features.toml records `implementation_owner = "missing"` for these two.
    // The inventory must not launder that into a plausible-looking owner.
    let inventory = activation::validate(&repo_root()).map_err(|error| error.to_string())?;
    let unowned: Vec<&str> = inventory
        .rows
        .iter()
        .filter(|row| row.owner == activation::UNOWNED)
        .map(|row| row.surface_id.as_str())
        .collect();
    assert_eq!(
        unowned,
        vec!["feature:lsp.notebook_cell_execution", "feature:lsp.notebook_document_sync"]
    );
    for row in inventory.rows.iter().filter(|row| row.owner == activation::UNOWNED) {
        let note = row.notes.as_deref().unwrap_or_default();
        assert!(note.contains("no implementation crate recorded"), "{note}");
    }
    assert!(
        !inventory.rows.iter().any(|row| row.owner == "missing"),
        "features.toml's `missing` sentinel must never be passed through as an owner"
    );
    Ok(())
}

#[test]
fn test_api_rule_covers_the_repository_s_other_test_feature_spellings() -> TestResult {
    // `slow_tests` and `integration-test` are real test-only features that a
    // `test-*`/`expose_*` prefix rule alone would miss.
    let inventory = activation::validate(&repo_root()).map_err(|error| error.to_string())?;
    let ids: Vec<&str> = inventory.rows.iter().map(|row| row.surface_id.as_str()).collect();
    for expected in [
        "cargo-feature:perl-lexer/slow_tests",
        "cargo-feature:perl-parser/slow_tests",
        "cargo-feature:perl-lsp-ux-tests/integration-test",
    ] {
        assert!(ids.contains(&expected), "missing test_api row `{expected}`");
    }
    Ok(())
}

#[test]
fn row_requires_a_non_blank_owner() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "gate:whitespace_check").ok_or("gate row not found")?["owner"] =
        json!("   ");
    expect_violation(&inventory, "requires a non-blank owner")
}

#[test]
fn dangling_consumer_path_fails_validation() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "feature:lsp.completion").ok_or("product row not found")?["consumers"] =
        json!(["crates/does-not-exist"]);
    expect_violation(&inventory, "missing authority path `crates/does-not-exist`")
}

#[test]
fn dangling_proof_reference_fails_validation() -> TestResult {
    let mut inventory = canonical_inventory()?;
    row_mut(&mut inventory, "feature:lsp.completion").ok_or("product row not found")?["proof_references"] =
        json!([{ "class": "integration_test", "id": "crates/nope/tests/gone.rs" }]);
    expect_violation(&inventory, "missing authority path `crates/nope/tests/gone.rs`")
}
