#![expect(clippy::expect_used, reason = "test fixture setup for the panic-family denominator")]

#[path = "no_panic_debt/support.rs"]
mod support;

use std::fs;
use support::{fixture_root, write_package, write_policy, write_registry};
use xtask::no_panic_debt::{
    ClippyObservation, ClippyTargetObservation, DebtStatus, InstrumentStatus, InventoryRequest,
    OwnerState, build_inventory, canonical_json, check_inventory, semantic_delta,
};

fn inventory_at(root: &std::path::Path) -> xtask::no_panic_debt::Inventory {
    build_inventory(InventoryRequest { root, ..InventoryRequest::default() }).expect("inventory")
}

#[test]
fn new_integration_test_file_with_unwrap_cannot_be_omitted() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/new_file.rs"),
        "#[test]\nfn added() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("write");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.population.files.iter().any(|file| file.path.ends_with("tests/new_file.rs")),
        "missing file in population: {:?}",
        inventory.population.files
    );
    assert!(
        inventory
            .rows
            .iter()
            .any(|row| { row.path.ends_with("tests/new_file.rs") && row.site_family == "unwrap" }),
        "unwrap site omitted: {:?}",
        inventory.rows
    );
}

#[test]
fn new_test_function_in_registered_file_cannot_be_omitted() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/known.rs"),
        "#[test]\nfn known_panic() { panic!(\"known\"); }\n\n#[test]\nfn extra() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("write");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.population.entrypoints.iter().any(|entry| entry.name == "extra"),
        "entrypoint omitted: {:?}",
        inventory.population.entrypoints
    );
}

#[test]
fn module_wide_allowance_does_not_hide_a_direct_site() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/hidden.rs"),
        "#![allow(clippy::unwrap_used)]\n#[test]\nfn hidden() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("write");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory
            .rows
            .iter()
            .any(|row| row.site_family == "unwrap" && row.path.ends_with("hidden.rs")),
        "site hidden by allowance: {:?}",
        inventory.rows
    );
    assert!(
        inventory
            .rows
            .iter()
            .any(|row| row.kind == "declaration" && row.path.ends_with("hidden.rs")),
        "declaration omitted: {:?}",
        inventory.rows
    );
}

#[test]
fn moved_site_is_visible_when_counts_stay_equal() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/known.rs"),
        "#[test]\nfn known_panic() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("first");
    let first = inventory_at(temp.path());
    fs::write(
        temp.path().join("crates/demo/tests/moved.rs"),
        "#[test]\nfn known_panic() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("moved");
    fs::write(temp.path().join("crates/demo/tests/known.rs"), "#[test]\nfn known_panic() {}\n")
        .expect("old");
    let second = inventory_at(temp.path());
    let delta = semantic_delta(&first, &second);
    assert!(!delta.added.is_empty() || !delta.removed.is_empty());
    assert_eq!(
        first.rows.iter().filter(|row| row.site_family == "unwrap").count(),
        second.rows.iter().filter(|row| row.site_family == "unwrap").count()
    );
}

#[test]
fn landed_conversion_does_not_keep_an_active_source_row() {
    let temp = fixture_root();
    fs::write(temp.path().join("crates/demo/tests/known.rs"), "#[test]\nfn known_panic() {}\n")
        .expect("converted");
    write_registry(
        temp.path(),
        r#"{
          "schema_version": 1,
          "sites": [{
            "path": "crates/demo/tests/known.rs",
            "enclosing_test_or_function": "known_panic",
            "macro_family": "panic!",
            "normalized_snippet": "panic!",
            "selector_identity": "invocation:dead:occurrence:1",
            "accepted_reason": "Intentional test failure diagnostic.",
            "state": "active"
          }]
        }"#,
    );
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.rows.iter().any(|row| {
            row.path.ends_with("tests/known.rs") && row.status == DebtStatus::StaleRegistry
        }),
        "active registry row without source was not stale: {:?}",
        inventory.rows
    );
    assert!(!inventory.rows.iter().any(|row| {
        row.path.ends_with("tests/known.rs") && row.kind == "site" && row.site_family == "panic!"
    }));
}

#[test]
fn registry_row_for_disappeared_or_changed_family_is_stale() {
    let temp = fixture_root();
    let first = inventory_at(temp.path());
    let site = first
        .rows
        .iter()
        .find(|row| row.kind == "site" && row.site_family == "panic!")
        .expect("panic site");
    write_registry(
        temp.path(),
        &serde_json::json!({
            "schema_version": 1,
            "sites": [{
                "path": site.path,
                "enclosing_test_or_function": site.entrypoint,
                "macro_family": site.site_family,
                "normalized_snippet": site.source_identity,
                "selector_identity": site.selector_identity,
                "accepted_reason": "Intentional test failure diagnostic.",
                "state": "active"
            }]
        })
        .to_string(),
    );
    fs::write(
        temp.path().join("crates/demo/tests/known.rs"),
        "#[test]\nfn known_panic() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("family change");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.rows.iter().any(|row| row.status == DebtStatus::StaleRegistry),
        "changed family did not stale the registry: {:?}",
        inventory.rows
    );
    assert!(
        !inventory.rows.iter().any(|row| {
            row.kind == "site"
                && row.site_family == "unwrap"
                && row.status == DebtStatus::IntentionalExactException
        }),
        "unwrap inherited panic! registry identity: {:?}",
        inventory.rows
    );
}

#[test]
fn source_disappearance_without_disposition_is_not_converted() {
    let temp = fixture_root();
    let first = inventory_at(temp.path());
    let site = first
        .rows
        .iter()
        .find(|row| row.kind == "site" && row.site_family == "panic!")
        .expect("panic site");
    write_registry(
        temp.path(),
        &serde_json::json!({
            "schema_version": 1,
            "sites": [{
                "path": site.path,
                "enclosing_test_or_function": site.entrypoint,
                "macro_family": site.site_family,
                "normalized_snippet": site.source_identity,
                "selector_identity": site.selector_identity,
                "accepted_reason": "Intentional test failure diagnostic.",
                "state": "active"
            }]
        })
        .to_string(),
    );
    fs::write(temp.path().join("crates/demo/tests/known.rs"), "#[test]\nfn known_panic() {}\n")
        .expect("gone");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.rows.iter().any(|row| row.status == DebtStatus::StaleRegistry),
        "active disappearance was not stale: {:?}",
        inventory.rows
    );
    assert!(
        !inventory.rows.iter().any(|row| row.status == DebtStatus::ConvertedAbsent),
        "active disappearance was treated as converted: {:?}",
        inventory.rows
    );
}

#[test]
fn closed_owner_does_not_remain_current_without_transition() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/owned.rs"),
        "#[expect(clippy::unwrap_used, reason = \"policy:#3021: leftover\")]\n#[test]\nfn leftover() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("owned");
    let mut owners = OwnerState::default();
    owners.closed_or_missing.insert("#3021".to_string());
    let inventory = build_inventory(InventoryRequest {
        root: temp.path(),
        owner_state: Some(&owners),
        ..InventoryRequest::default()
    })
    .expect("inventory");
    assert!(
        inventory.rows.iter().any(|row| row.status == DebtStatus::StaleOwner),
        "closed owner stayed current: {:?}",
        inventory.rows
    );
}

#[test]
fn aborted_clippy_target_is_not_proven_and_not_zero() {
    let temp = fixture_root();
    let observation = ClippyObservation {
        targets: vec![ClippyTargetObservation {
            package: "demo".to_string(),
            target: "demo".to_string(),
            status: xtask::no_panic_debt::ClippyTargetStatus::Aborted,
        }],
    };
    let inventory = build_inventory(InventoryRequest {
        root: temp.path(),
        clippy_observation: Some(&observation),
        ..InventoryRequest::default()
    })
    .expect("inventory");
    assert!(inventory.instruments.iter().any(|instrument| instrument.kind == "clippy"
        && instrument.status == InstrumentStatus::NotProven));
    assert!(
        !inventory.rows.is_empty() || !inventory.population.files.is_empty(),
        "aborted clippy collapsed the denominator to zero"
    );
    assert!(inventory.counts.instrument_not_proven > 0);
}

#[test]
fn feature_and_platform_tests_are_not_omitted_by_default_observation() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/gated.rs"),
        r#"
#[cfg(feature = "extra")]
#[test]
fn feature_site() { let _ = Some(1).unwrap(); }

#[cfg(windows)]
#[test]
fn platform_site() { panic!("win"); }
"#,
    )
    .expect("gated");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.population.entrypoints.iter().any(|entry| entry.name == "feature_site"),
        "feature test omitted: {:?}",
        inventory.population.entrypoints
    );
    assert!(
        inventory.population.entrypoints.iter().any(|entry| entry.name == "platform_site"),
        "platform test omitted: {:?}",
        inventory.population.entrypoints
    );
}

#[test]
fn output_is_stable_across_host_path_and_file_write_order() {
    let first = fixture_root();
    let second = tempfile::tempdir().expect("second");
    write_policy(second.path());
    write_package(
        second.path(),
        "demo",
        &fs::read_to_string(first.path().join("crates/demo/src/lib.rs")).expect("lib"),
        &[(
            "known.rs",
            &fs::read_to_string(first.path().join("crates/demo/tests/known.rs")).expect("known"),
        )],
    );
    let left = build_inventory(InventoryRequest {
        root: first.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("left");
    let right = build_inventory(InventoryRequest {
        root: second.path(),
        repository_commit: Some("fixture".to_string()),
        ..InventoryRequest::default()
    })
    .expect("right");
    assert_eq!(
        canonical_json(&left).expect("left json"),
        canonical_json(&right).expect("right json")
    );
}

#[test]
fn hand_edited_counts_cannot_make_check_pass() {
    let temp = fixture_root();
    let inventory = inventory_at(temp.path());
    let mut tampered = inventory.clone();
    tampered.counts.rows = 0;
    let path = temp.path().join("tampered.json");
    fs::write(&path, canonical_json(&tampered).expect("json")).expect("write");
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &inventory,
        artifact: Some(&path),
        baseline: None,
    })
    .expect("check");
    assert!(!result.ok, "tampered counts passed: {:?}", result.findings);
}

#[test]
fn regeneration_does_not_absorb_new_unowned_into_baseline() {
    let temp = fixture_root();
    let baseline_inv = inventory_at(temp.path());
    let baseline_path = temp.path().join("baseline.json");
    fs::write(&baseline_path, canonical_json(&baseline_inv).expect("json")).expect("baseline");
    fs::write(
        temp.path().join("crates/demo/tests/new_unowned.rs"),
        "#[test]\nfn fresh() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("new");
    let current = inventory_at(temp.path());
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &current,
        artifact: None,
        baseline: Some(&baseline_path),
    })
    .expect("check");
    assert!(!result.ok, "new unowned site was absorbed: {:?}", result.findings);
}

#[test]
fn issue_closure_does_not_convert_current_source_to_converted_absent() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/owned.rs"),
        "#[expect(clippy::unwrap_used, reason = \"#14020 leftover\")]\n#[test]\nfn leftover() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("owned");
    let first = inventory_at(temp.path());
    let site = first
        .rows
        .iter()
        .find(|row| {
            row.kind == "site" && row.path.ends_with("owned.rs") && row.site_family == "unwrap"
        })
        .expect("owned unwrap");
    write_registry(
        temp.path(),
        &serde_json::json!({
            "schema_version": 1,
            "sites": [{
                "path": site.path,
                "enclosing_test_or_function": site.entrypoint,
                "macro_family": "panic!",
                "normalized_snippet": site.source_identity,
                "selector_identity": site.selector_identity,
                "accepted_reason": "retired conversion.",
                "state": "retired"
            }]
        })
        .to_string(),
    );
    let mut owners = OwnerState::default();
    owners.closed_or_missing.insert("#14020".to_string());
    let inventory = build_inventory(InventoryRequest {
        root: temp.path(),
        owner_state: Some(&owners),
        ..InventoryRequest::default()
    })
    .expect("inventory");
    assert!(
        inventory.rows.iter().any(|row| {
            row.path.ends_with("owned.rs")
                && row.kind == "site"
                && row.status == DebtStatus::StaleOwner
        }),
        "closed owner on live source was not stale_owner: {:?}",
        inventory.rows
    );
    assert!(!inventory.rows.iter().any(|row| {
        row.kind == "site"
            && row.path.ends_with("owned.rs")
            && row.status == DebtStatus::ConvertedAbsent
    }));
}

#[test]
fn open_candidate_tree_is_not_landed_source() {
    let landed = fixture_root();
    let candidate = tempfile::tempdir().expect("candidate");
    write_policy(candidate.path());
    write_package(
        candidate.path(),
        "demo",
        "pub fn ready() -> Option<u8> { Some(1) }\n",
        &[("pr_only.rs", "#[test]\nfn only_on_pr() { let _ = Some(1).unwrap(); }\n")],
    );
    let landed_inv = inventory_at(landed.path());
    assert!(
        !landed_inv.population.files.iter().any(|file| file.path.ends_with("pr_only.rs")),
        "open PR file leaked into landed tree"
    );
    let candidate_inv = inventory_at(candidate.path());
    assert!(candidate_inv.population.files.iter().any(|file| file.path.ends_with("pr_only.rs")));
}

#[test]
fn matching_registry_panic_is_intentional_exception() {
    let temp = fixture_root();
    let first = inventory_at(temp.path());
    let site = first
        .rows
        .iter()
        .find(|row| row.kind == "site" && row.site_family == "panic!")
        .expect("panic site");
    write_registry(
        temp.path(),
        &serde_json::json!({
            "schema_version": 1,
            "sites": [{
                "path": site.path,
                "enclosing_test_or_function": site.entrypoint,
                "macro_family": site.site_family,
                "normalized_snippet": site.source_identity,
                "selector_identity": site.selector_identity,
                "accepted_reason": "Intentional test failure diagnostic.",
                "state": "active"
            }]
        })
        .to_string(),
    );
    let joined = inventory_at(temp.path());
    assert!(
        joined.rows.iter().any(|row| {
            row.kind == "site"
                && row.site_family == "panic!"
                && row.status == DebtStatus::IntentionalExactException
                && row.registry_relation == "matched_active"
        }),
        "exact registry identity did not join: {:?}",
        joined.rows
    );
}

#[test]
fn nested_path_attr_inside_inline_module_is_not_resolved_against_src() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_package(
        temp.path(),
        "demo",
        r#"
pub mod nested {
    #[cfg(test)]
    #[path = "tests.rs"]
    mod tests;

    #[cfg(test)]
    #[path = "test_support.rs"]
    mod test_support;
}
"#,
        &[],
    );
    let nested = temp.path().join("crates/demo/src/nested");
    fs::create_dir_all(&nested).expect("nested");
    fs::write(nested.join("tests.rs"), "#[test]\nfn nested_test() { let _ = Some(1).unwrap(); }\n")
        .expect("tests");
    fs::write(nested.join("test_support.rs"), "pub fn helper() { let _ = Some(1).unwrap(); }\n")
        .expect("support");

    let inventory = inventory_at(temp.path());
    assert!(
        !inventory.instruments.iter().any(|instrument| {
            instrument.status == InstrumentStatus::NotProven
                && (instrument.subject.ends_with("src/tests.rs")
                    || instrument.subject.ends_with("src/test_support.rs"))
        }),
        "phantom src-relative #[path] was not_proven: {:?}",
        inventory.instruments
    );
    assert!(
        inventory.rows.iter().any(|row| {
            row.path.ends_with("src/nested/tests.rs") && row.site_family == "unwrap"
        }),
        "nested #[path] test site omitted: {:?}",
        inventory.rows
    );
    assert!(
        inventory.rows.iter().any(|row| {
            row.path.ends_with("src/nested/test_support.rs") && row.site_family == "unwrap"
        }),
        "cfg(test) helper unwrap omitted: {:?}",
        inventory.rows
    );
}

#[test]
fn missing_cfg_test_path_module_is_not_proven() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_package(
        temp.path(),
        "demo",
        r#"
pub mod nested {
    #[cfg(test)]
    #[path = "missing_tests.rs"]
    mod tests;
}
"#,
        &[],
    );
    fs::create_dir_all(temp.path().join("crates/demo/src/nested")).expect("nested");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.instruments.iter().any(|instrument| {
            instrument.kind == "source_parse"
                && instrument.status == InstrumentStatus::NotProven
                && instrument.subject.ends_with("src/nested/missing_tests.rs")
        }),
        "missing #[path] module was not not_proven: {:?}",
        inventory.instruments
    );
    assert!(inventory.counts.instrument_not_proven > 0);
}

#[test]
fn production_unwrap_is_not_test_debt() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_package(
        temp.path(),
        "demo",
        "pub fn ready() -> u8 { Some(1).unwrap() }\n",
        &[("known.rs", "#[test]\nfn known() {}\n")],
    );
    let inventory = inventory_at(temp.path());
    assert!(
        !inventory.rows.iter().any(|row| {
            row.kind == "site" && row.path.ends_with("src/lib.rs") && row.site_family == "unwrap"
        }),
        "production unwrap classified as test debt: {:?}",
        inventory.rows
    );
}

#[test]
fn non_member_nested_workspace_is_not_proven_not_a_population_hole() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_package(
        temp.path(),
        "demo",
        "pub fn ready() {}\n",
        &[("known.rs", "#[test]\nfn known() {}\n")],
    );
    let fuzz = temp.path().join("tests/fuzz");
    fs::create_dir_all(&fuzz).expect("fuzz");
    fs::write(
        fuzz.join("Cargo.toml"),
        r#"[package]
name = "fuzz-parser-robustness"
version = "0.1.0"
edition = "2021"

[workspace]
"#,
    )
    .expect("fuzz manifest");
    fs::write(fuzz.join("quick_lsp_test.rs"), "fn main() { let _ = Some(1).unwrap(); }\n")
        .expect("fuzz bin");

    let inventory = inventory_at(temp.path());
    assert!(
        inventory.instruments.iter().any(|instrument| {
            instrument.kind == "test_topology"
                && instrument.status == InstrumentStatus::NotProven
                && instrument.subject.ends_with("tests/fuzz/Cargo.toml")
        }),
        "unreachable nested workspace was a silent zero: {:?}",
        inventory.instruments
    );
    assert!(
        !inventory.rows.iter().any(|row| row.path.ends_with("quick_lsp_test.rs")),
        "non-member fuzz bin was absorbed as workspace test debt: {:?}",
        inventory.rows
    );
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &inventory,
        artifact: None,
        baseline: None,
    })
    .expect("check");
    assert!(
        result.ok,
        "non-member tests/fuzz was treated as a missing population hole: {:?}",
        result.findings
    );
}

#[test]
fn registry_is_not_the_discovery_denominator() {
    let temp = fixture_root();
    write_registry(temp.path(), r#"{"schema_version":1,"sites":[]}"#);
    fs::write(
        temp.path().join("crates/demo/tests/unregistered.rs"),
        "#[test]\nfn fresh() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("unregistered");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory
            .rows
            .iter()
            .any(|row| { row.path.ends_with("unregistered.rs") && row.site_family == "unwrap" }),
        "unregistered site escaped because registry was empty: {:?}",
        inventory.rows
    );
}

#[test]
fn unreadable_source_is_not_proven_not_empty_success() {
    let temp = fixture_root();
    fs::write(temp.path().join("crates/demo/tests/broken.rs"), "fn not rust {{{").expect("broken");
    let inventory = inventory_at(temp.path());
    assert!(inventory.instruments.iter().any(|instrument| instrument.kind == "source_parse"
        && instrument.status == InstrumentStatus::NotProven));
    assert!(inventory.counts.instrument_not_proven > 0);
}

#[test]
fn missing_vocabulary_is_not_a_clean_zero() {
    let temp = tempfile::tempdir().expect("temp");
    write_package(
        temp.path(),
        "demo",
        "pub fn ready() {}\n",
        &[("known.rs", "#[test]\nfn known() { let _ = Some(1).unwrap(); }\n")],
    );
    let inventory = inventory_at(temp.path());
    assert!(
        inventory
            .instruments
            .iter()
            .any(|instrument| instrument.status == InstrumentStatus::NotProven)
    );
    let findings = xtask::no_panic_debt::check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &inventory,
        artifact: None,
        baseline: None,
    })
    .expect("check");
    assert!(!findings.ok, "missing vocabulary became a clean zero: {:?}", findings.findings);
}
