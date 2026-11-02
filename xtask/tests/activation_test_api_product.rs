//! Discriminating tests for test-only protocol product exclusion (#9207).

use std::error::Error;
use std::path::{Path, PathBuf};

use xtask::activation::{
    TEST_API_CFG_GATE, check_product_capability_snapshots, check_routing_completeness,
    evaluate_test_api_product, load_test_api_protocol_inventory,
    production_routing_contains_method,
};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

#[test]
fn clean_repository_passes_test_api_product_exclusion() -> TestResult {
    let root = repo_root();
    let inventory = load_test_api_protocol_inventory(&root)?;
    let report = evaluate_test_api_product(&root, &inventory);
    assert!(
        report.is_clean(),
        "expected clean tree, got violations:\n{}",
        report.violations.join("\n")
    );
    Ok(())
}

#[test]
fn inventory_requires_named_harness_profile_and_consumer() -> TestResult {
    let root = repo_root();
    let inventory = load_test_api_protocol_inventory(&root)?;
    let route = inventory
        .routes
        .iter()
        .find(|row| row.method == "$/test/slowOperation")
        .ok_or("missing slowOperation inventory row")?;
    assert!(route.harness_profiles.iter().any(|profile| profile.contains("expose_lsp_test_api")));
    assert!(!route.harness_consumers.is_empty());
    Ok(())
}

#[test]
fn ungated_test_route_fails_production_scan() -> TestResult {
    let routing = "\"$/test/slowOperation\" => self.handle(),";
    assert!(production_routing_contains_method(routing, "$/test/slowOperation"));
    Ok(())
}

#[test]
fn cfg_gated_test_route_passes_production_scan() -> TestResult {
    let routing =
        format!("{TEST_API_CFG_GATE}\n            \"$/test/slowOperation\" => self.handle(),");
    assert!(!production_routing_contains_method(&routing, "$/test/slowOperation"));
    Ok(())
}

#[test]
fn leaked_capability_snapshot_fails_check() -> TestResult {
    let root = repo_root();
    let inventory = load_test_api_protocol_inventory(&root)?;
    let mut violations = check_product_capability_snapshots(&root, &inventory);
    assert!(violations.is_empty(), "baseline snapshots must stay clean: {violations:?}");

    let temp = tempfile::tempdir()?;
    let snapshot_dir = temp.path().join("crates/perl-lsp-rs/tests/snapshots");
    std::fs::create_dir_all(&snapshot_dir)?;
    std::fs::write(
        snapshot_dir.join("production_capabilities.json"),
        r#"{"executeCommandProvider":{"commands":["$/test/slowOperation"]}}"#,
    )?;
    violations = check_product_capability_snapshots(temp.path(), &inventory);
    assert!(
        violations.iter().any(|v| v.contains("production_capabilities.json")),
        "expected capability leak to fail closed, got: {violations:?}"
    );
    Ok(())
}

#[test]
fn unlisted_routing_discovery_fails_check() -> TestResult {
    let inventory = load_test_api_protocol_inventory(&repo_root())?;
    let routing = format!(
        "{TEST_API_CFG_GATE}\n            \"$/test/slowOperation\" => self.handle(),\n\
         {TEST_API_CFG_GATE}\n            \"$/test/extraRoute\" => self.handle(),"
    );
    let violations = check_routing_completeness(&routing, &inventory);
    assert!(
        violations.iter().any(|v| v.contains("extraRoute")),
        "expected unlisted route violation, got: {violations:?}"
    );
    Ok(())
}
