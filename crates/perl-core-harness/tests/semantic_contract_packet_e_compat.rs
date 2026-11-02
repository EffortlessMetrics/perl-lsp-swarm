//! Harness compatibility pins for semantic packet E (#6599).
//!
//! Bootstrap and platform-sensitive upstream populations must stay explicitly
//! indexed in checked-in harness fixtures rather than inferred from Linux-only
//! semantic tests.

use perl_core_harness::target_contracts::model::{
    TARGET_MATRIX_PART_SCHEMA_VERSION, TargetDisposition, TargetMatrixPart,
};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

#[test]
fn platform_only_upstream_targets_are_platform_unavailable_on_linux_programme() -> TestResult {
    let raw = fs::read_to_string(repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1/06-optional-platform.json",
    ))?;
    let matrix: TargetMatrixPart = serde_json::from_str(&raw)?;
    assert_eq!(matrix.schema_version, TARGET_MATRIX_PART_SCHEMA_VERSION);

    let mut saw_os2 = false;
    let mut saw_win32 = false;
    for entry in &matrix.targets {
        let id = entry.contract.target_id.as_str();
        if id == "platform_os2" {
            saw_os2 = true;
            assert_eq!(entry.disposition, TargetDisposition::PlatformUnavailable);
            assert!(
                entry.contract.capability_predicates.iter().any(|p| p == "os2"),
                "os2 target must declare os2 capability predicate"
            );
        }
        if id == "platform_win32" {
            saw_win32 = true;
            assert_eq!(entry.disposition, TargetDisposition::PlatformUnavailable);
            assert!(
                entry.contract.capability_predicates.iter().any(|p| p == "windows"),
                "win32 target must declare windows capability predicate"
            );
        }
    }
    assert!(saw_os2 && saw_win32, "platform matrix must index os2 and win32 rows");
    Ok(())
}

#[test]
fn optional_resource_populations_stay_out_of_default_denominator() -> TestResult {
    let raw = fs::read_to_string(repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1/06-optional-platform.json",
    ))?;
    let matrix: TargetMatrixPart = serde_json::from_str(&raw)?;
    for entry in &matrix.targets {
        let id = entry.contract.target_id.as_str();
        if matches!(id, "optional_benchmark" | "optional_bigmem" | "optional_japh") {
            assert!(!entry.claim_boundary.is_empty(), "{id} must document a claim boundary");
            assert!(
                entry.contract.change_reason.as_deref().is_some_and(|r| !r.is_empty()),
                "{id} must record why it is optional"
            );
        }
    }
    Ok(())
}

#[test]
fn invocation_trace_model_documents_testinit_bootstrap_classes() -> TestResult {
    let source =
        fs::read_to_string(repo_file("crates/perl-core-harness/src/invocation_trace/model.rs"))?;
    assert!(
        source.contains("TestInit/bootstrap"),
        "invocation trace must document bootstrap classes"
    );
    assert!(
        source.contains("Standard bootstrap"),
        "invocation trace must name the standard bootstrap class"
    );
    Ok(())
}

#[test]
fn semantic_boundary_registry_fixture_is_valid_empty_registry() -> TestResult {
    let raw =
        fs::read_to_string(repo_file(".ci/perl-core-harness/semantic-boundary-registry.v1.json"))?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    assert_eq!(
        value.get("schema_version").and_then(|v| v.as_str()),
        Some("perl_core_harness.semantic_boundary_registry.v1")
    );
    assert!(
        value.get("entries").and_then(|v| v.as_array()).is_some_and(|a| a.is_empty()),
        "registry scaffold must start empty until #4737 population lands"
    );
    Ok(())
}

#[test]
fn packet_e_routes_bootstrap_and_platform_issues_to_compatibility_ledger() -> TestResult {
    let policy = fs::read_to_string(repo_file("policy/semantic/packet-e-issue-routing.v1.toml"))?;
    for issue in [3651_u32, 3700, 3708, 4737] {
        let needle = format!("issue = {issue}");
        assert!(policy.contains(&needle), "routing policy must mention issue {issue}");
        let section = policy.split(&needle).nth(1).ok_or("issue section missing")?;
        assert!(
            section.contains("follow_up = \"compatibility_ledger\""),
            "issue {issue} must route to compatibility_ledger"
        );
    }
    Ok(())
}
