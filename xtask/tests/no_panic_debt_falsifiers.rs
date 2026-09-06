#![expect(clippy::expect_used, reason = "test fixture setup for the panic-family denominator")]

#[path = "no_panic_debt/support.rs"]
mod support;

use std::fs;
use support::{fixture_root, write_empty_registry, write_package, write_policy, write_registry};
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
fn same_line_same_family_sites_keep_distinct_occurrences() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/same_line.rs"),
        "#[test]\nfn two() { let _ = (Some(1).unwrap(), Some(2).unwrap()); }\n",
    )
    .expect("same line");
    let inventory = inventory_at(temp.path());
    let sites: Vec<_> = inventory
        .rows
        .iter()
        .filter(|row| {
            row.kind == "site"
                && row.site_family == "unwrap"
                && row.path.ends_with("tests/same_line.rs")
        })
        .collect();
    assert_eq!(sites.len(), 2, "same-line unwrap sites collapsed: {:?}", inventory.rows);
    let selectors: std::collections::BTreeSet<_> =
        sites.iter().map(|row| row.selector_identity.as_str()).collect();
    assert_eq!(selectors.len(), 2, "occurrence selectors collided: {selectors:?}");
    assert!(
        sites.iter().any(|row| row.selector_identity.ends_with("occurrence:1"))
            && sites.iter().any(|row| row.selector_identity.ends_with("occurrence:2")),
        "expected occurrence 1 and 2, got {selectors:?}"
    );
}

#[test]
fn missing_registry_is_not_proven_and_fails_check() {
    let temp = fixture_root();
    fs::remove_file(temp.path().join("ci/panic_test_identities.json")).expect("remove registry");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.instruments.iter().any(|instrument| {
            instrument.kind == "panic_registry" && instrument.status == InstrumentStatus::NotProven
        }),
        "missing registry was treated as an empty join: {:?}",
        inventory.instruments
    );
    assert!(inventory.counts.instrument_not_proven > 0);
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &inventory,
        artifact: None,
        baseline: None,
    })
    .expect("check");
    assert!(!result.ok, "missing registry check passed: {:?}", result.findings);
    assert!(
        result.findings.iter().any(|finding| finding.contains("panic registry is not_proven")),
        "{:?}",
        result.findings
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
    write_empty_registry(second.path());
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
    write_empty_registry(temp.path());
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

#[test]
fn failed_source_is_not_converted_absent() {
    let temp = fixture_root();
    fs::write(
        temp.path().join("crates/demo/tests/sibling.rs"),
        "#[test]\nfn sibling() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("sibling");
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
                "accepted_reason": "retired conversion.",
                "state": "retired"
            }]
        })
        .to_string(),
    );
    let matched = inventory_at(temp.path());
    assert!(
        matched.rows.iter().any(|row| {
            row.kind == "site"
                && row.path == site.path
                && row.status == DebtStatus::StaleRegistry
                && row.registry_relation == "matched_retired"
        }),
        "retired identity on live source was not stale_registry: {:?}",
        matched.rows
    );

    fs::write(temp.path().join("crates/demo/tests/known.rs"), "fn not rust {{{").expect("broken");
    let broken = inventory_at(temp.path());
    assert!(
        broken.instruments.iter().any(|instrument| {
            instrument.kind == "source_parse"
                && instrument.status == InstrumentStatus::NotProven
                && instrument.subject.ends_with("tests/known.rs")
        }),
        "broken source was not source_parse not_proven: {:?}",
        broken.instruments
    );
    assert!(
        broken.rows.iter().any(|row| {
            row.kind == "registry"
                && row.path.ends_with("tests/known.rs")
                && row.status == DebtStatus::InstrumentNotProven
                && row.registry_relation == "source_not_proven"
        }),
        "failed source was treated as absence: {:?}",
        broken.rows
    );
    assert!(
        !broken.rows.iter().any(|row| {
            row.path.ends_with("tests/known.rs") && row.status == DebtStatus::ConvertedAbsent
        }),
        "failed source became converted_absent: {:?}",
        broken.rows
    );
    assert!(
        broken.rows.iter().any(|row| {
            row.kind == "site"
                && row.path.ends_with("tests/sibling.rs")
                && row.site_family == "unwrap"
        }),
        "unrelated parsed sibling lost its observed site: {:?}",
        broken.rows
    );
    assert!(!broken.counts.observation_complete);
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &broken,
        artifact: None,
        baseline: None,
    })
    .expect("check");
    assert!(result.ok, "failed-source observation integrity should stay ok: {:?}", result.findings);
}

#[test]
fn retired_parsed_removal_is_converted_absent() {
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
                "accepted_reason": "retired conversion.",
                "state": "retired"
            }]
        })
        .to_string(),
    );
    fs::write(temp.path().join("crates/demo/tests/known.rs"), "#[test]\nfn known_panic() {}\n")
        .expect("removed site");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.rows.iter().any(|row| {
            row.kind == "registry"
                && row.path.ends_with("tests/known.rs")
                && row.status == DebtStatus::ConvertedAbsent
                && row.registry_relation == "retired_absent_from_source"
        }),
        "parsed retired removal was not converted_absent: {:?}",
        inventory.rows
    );
}

#[test]
fn workspace_root_package_is_in_population() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_empty_registry(temp.path());
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[workspace]
members = ["crates/demo"]
resolver = "2"

[package]
name = "root-tool"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("workspace+package");
    let crate_root = temp.path().join("crates/demo");
    fs::create_dir_all(crate_root.join("src")).expect("demo src");
    fs::write(
        crate_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("demo manifest");
    fs::write(crate_root.join("src/lib.rs"), "pub fn ready() {}\n").expect("demo lib");
    fs::create_dir_all(temp.path().join("src")).expect("root src");
    fs::write(
        temp.path().join("src/lib.rs"),
        "#[cfg(test)]\nmod tests { #[test] fn root() { let _ = Some(1).unwrap(); } }\n",
    )
    .expect("root lib");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.population.packages.iter().any(|package| package.name == "root-tool"),
        "root [package] omitted: {:?}",
        inventory.population.packages
    );
    assert!(
        inventory.rows.iter().any(|row| {
            row.path.ends_with("src/lib.rs")
                && row.site_family == "unwrap"
                && row.package == "root-tool"
        }),
        "root-package cfg(test) unwrap omitted: {:?}",
        inventory.rows
    );
}

#[test]
fn excluded_member_is_not_population() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_empty_registry(temp.path());
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]
exclude = ["crates/skipped"]
resolver = "2"
"#,
    )
    .expect("workspace");
    for name in ["demo", "skipped"] {
        let crate_root = temp.path().join("crates").join(name);
        fs::create_dir_all(crate_root.join("src")).expect("src");
        fs::create_dir_all(crate_root.join("tests")).expect("tests");
        fs::write(
            crate_root.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .expect("manifest");
        fs::write(crate_root.join("src/lib.rs"), "pub fn ready() {}\n").expect("lib");
        fs::write(
            crate_root.join("tests/debt.rs"),
            "#[test]\nfn debt() { let _ = Some(1).unwrap(); }\n",
        )
        .expect("test");
    }
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.population.packages.iter().any(|package| package.name == "demo"),
        "included member missing: {:?}",
        inventory.population.packages
    );
    assert!(
        !inventory.population.packages.iter().any(|package| package.name == "skipped"),
        "excluded member leaked into population: {:?}",
        inventory.population.packages
    );
    assert!(
        !inventory.rows.iter().any(|row| row.path.contains("crates/skipped")),
        "excluded member sites leaked: {:?}",
        inventory.rows
    );
}

#[test]
fn missing_member_manifest_is_not_proven() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_package(
        temp.path(),
        "demo",
        "pub fn ready() {}\n",
        &[("known.rs", "#[test]\nfn known() {}\n")],
    );
    fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/demo\", \"crates/ghost\"]\nresolver = \"2\"\n",
    )
    .expect("members");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.instruments.iter().any(|instrument| {
            instrument.kind == "test_topology"
                && instrument.status == InstrumentStatus::NotProven
                && instrument.subject.ends_with("crates/ghost/Cargo.toml")
        }),
        "missing member was a silent skip: {:?}",
        inventory.instruments
    );
    assert!(!inventory.counts.observation_complete);
}

#[test]
fn custom_test_target_helper_unwrap_is_debt() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_empty_registry(temp.path());
    write_package(temp.path(), "demo", "pub fn ready() {}\n", &[]);
    fs::write(
        temp.path().join("crates/demo/Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"

[[test]]
name = "custom"
path = "checks/custom.rs"
"#,
    )
    .expect("custom target");
    fs::create_dir_all(temp.path().join("crates/demo/checks")).expect("checks");
    fs::write(
        temp.path().join("crates/demo/checks/custom.rs"),
        "fn helper() { let _ = Some(1).unwrap(); }\n#[test]\nfn uses_helper() { helper(); }\n",
    )
    .expect("custom.rs");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory.population.files.iter().any(|file| {
            file.path.ends_with("checks/custom.rs")
                && file.target_kind == xtask::no_panic_debt::TargetKind::IntegrationTest
        }),
        "custom [[test]] path omitted or not a test target: {:?}",
        inventory.population.files
    );
    assert!(
        inventory
            .rows
            .iter()
            .any(|row| { row.path.ends_with("checks/custom.rs") && row.site_family == "unwrap" }),
        "helper unwrap outside #[test] in a custom test target was omitted: {:?}",
        inventory.rows
    );
}

#[test]
fn named_test_target_without_path_is_still_a_test_crate() {
    let temp = tempfile::tempdir().expect("temp");
    write_policy(temp.path());
    write_empty_registry(temp.path());
    write_package(temp.path(), "demo", "pub fn ready() {}\n", &[]);
    fs::write(
        temp.path().join("crates/demo/Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
autotests = false

[[test]]
name = "named"
"#,
    )
    .expect("named target");
    fs::write(
        temp.path().join("crates/demo/tests/named.rs"),
        "fn helper() { let _ = Some(1).unwrap(); }\n#[test]\nfn uses_helper() { helper(); }\n",
    )
    .expect("named.rs");
    fs::write(
        temp.path().join("crates/demo/tests/ignored.rs"),
        "#[test]\nfn skipped() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("ignored autodiscovery");
    let inventory = inventory_at(temp.path());
    assert!(
        inventory
            .rows
            .iter()
            .any(|row| { row.path.ends_with("tests/named.rs") && row.site_family == "unwrap" }),
        "name-only [[test]] crate omitted: {:?}",
        inventory.rows
    );
    assert!(
        !inventory.population.files.iter().any(|file| file.path.ends_with("tests/ignored.rs")),
        "autotests=false still absorbed tests/*.rs: {:?}",
        inventory.population.files
    );
}

#[test]
fn detached_tests_fixture_is_not_executable_population() {
    let temp = fixture_root();
    fs::create_dir_all(temp.path().join("crates/demo/tests/fixtures")).expect("fixtures");
    fs::write(
        temp.path().join("crates/demo/tests/fixtures/orphan.rs"),
        "fn not_a_target() { let _ = Some(1).unwrap(); }\n",
    )
    .expect("orphan");
    let inventory = inventory_at(temp.path());
    assert!(
        !inventory
            .population
            .files
            .iter()
            .any(|file| file.path.ends_with("tests/fixtures/orphan.rs")),
        "detached fixture counted as executable test population: {:?}",
        inventory.population.files
    );
    assert!(
        !inventory.rows.iter().any(|row| row.path.ends_with("tests/fixtures/orphan.rs")),
        "detached fixture became test debt: {:?}",
        inventory.rows
    );
    let result = check_inventory(xtask::no_panic_debt::CheckRequest {
        root: temp.path(),
        current: &inventory,
        artifact: None,
        baseline: None,
    })
    .expect("check");
    assert!(result.ok, "detached fixture failed integrity: {:?}", result.findings);
}
