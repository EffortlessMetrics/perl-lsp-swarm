//! Discriminating class-contract tests for `cargo xtask activation
//! check/report/explain` (#9205).
//!
//! These fixtures are the claim: a wrong universal "wired" predicate, a
//! Cargo-dependency pass, or treating unknown evidence as green must fail
//! here. Live inventory evaluation is a composition check, not the
//! discriminator.

use serde_json::{Value, json};
use std::error::Error;
use std::path::{Path, PathBuf};
use xtask::activation::{self as activation, Verdict, evaluate_raw_row};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn class_authority(rule: &str) -> Value {
    json!({
        "kind": "derived",
        "authority": "features.toml",
        "rule": rule
    })
}

fn product_fixture() -> Value {
    json!({
        "surface_id": "feature:fixture.product",
        "class": "product",
        "class_authority": class_authority("features-product"),
        "semantic_authority": "features.toml#fixture.product",
        "consumers": ["crates/perl-lsp-rs"],
        "compile_profiles": [],
        "registration": {
            "state": "established",
            "authority": "features.toml",
            "detail": "capability_gate = \"hoverProvider\"; registration = \"static_capabilities\""
        },
        "proof_references": [{
            "class": "integration_test",
            "id": "crates/perl-lsp-rs/tests/lsp_hover_tests.rs"
        }],
        "publication": { "state": "not_applicable", "authority": "features.toml" },
        "owner": "crates/perl-lsp-rs",
        "promotion": { "state": "not_evaluated" }
    })
}

fn preview_fixture() -> Value {
    json!({
        "surface_id": "feature:fixture.preview",
        "class": "preview",
        "class_authority": class_authority("features-preview"),
        "semantic_authority": "features.toml#fixture.preview",
        "consumers": [],
        "compile_profiles": [],
        "registration": {
            "state": "not_established",
            "authority": "features.toml",
            "detail": "capability_gate = \"notebookDocumentSync\"; registration = \"static_capabilities\""
        },
        "proof_references": [],
        "publication": { "state": "not_applicable", "authority": "features.toml" },
        "owner": "unowned",
        "promotion": { "state": "not_evaluated" },
        "notes": "preview limitation: no implementation crate recorded"
    })
}

fn lab_fixture() -> Value {
    json!({
        "surface_id": "fuzz:fixture_lab",
        "class": "lab",
        "class_authority": {
            "kind": "derived",
            "authority": "fuzz/fuzz_targets/fixture_lab.rs",
            "rule": "fuzz-targets"
        },
        "semantic_authority": "fuzz/fuzz_targets/fixture_lab.rs",
        "consumers": ["perl-parser-fuzz"],
        "compile_profiles": ["fuzz"],
        "registration": {
            "state": "established",
            "authority": "fuzz/Cargo.toml",
            "detail": "[[bin]] name = \"fixture_lab\""
        },
        "proof_references": [],
        "publication": { "state": "not_applicable", "authority": "fuzz/Cargo.toml" },
        "owner": "perl-parser-fuzz",
        "promotion": { "state": "not_evaluated" }
    })
}

fn shim_fixture() -> Value {
    json!({
        "surface_id": "crate:fixture-shim",
        "class": "compatibility_shim",
        "class_authority": {
            "kind": "override",
            "authority": "policy/activation-overrides.toml",
            "rule": "override"
        },
        "semantic_authority": "policy/tree-sitter-compat-inventory.toml",
        "consumers": [],
        "compile_profiles": [],
        "registration": {
            "state": "not_established",
            "detail": "narrow override row"
        },
        "proof_references": [],
        "publication": {
            "state": "private_workspace_member",
            "authority": "crates/perl-tree-sitter-compat/Cargo.toml"
        },
        "owner": "parser/tree-sitter",
        "promotion": { "state": "not_evaluated" },
        "retirement": {
            "owner": "#8890",
            "boundary": "removal after migration"
        }
    })
}

fn test_api_fixture() -> Value {
    json!({
        "surface_id": "cargo-feature:fixture/test-helpers",
        "class": "test_api",
        "class_authority": {
            "kind": "derived",
            "authority": "crates/perl-dap/Cargo.toml",
            "rule": "cargo-test-features"
        },
        "semantic_authority": "crates/perl-dap/Cargo.toml#features.test-helpers",
        "consumers": ["crates/perl-dap"],
        "compile_profiles": ["test-helpers"],
        "registration": {
            "state": "established",
            "authority": "crates/perl-dap/Cargo.toml",
            "detail": "[features] test-helpers = []"
        },
        "proof_references": [],
        "publication": { "state": "not_applicable", "authority": "crates/perl-dap/Cargo.toml" },
        "owner": "perl-dap",
        "promotion": { "state": "not_evaluated" }
    })
}

fn gate_fixture() -> Value {
    json!({
        "surface_id": "gate:fixture_gate",
        "class": "gate",
        "class_authority": {
            "kind": "derived",
            "authority": ".ci/gate-policy.yaml",
            "rule": "gate-policy-gates"
        },
        "semantic_authority": ".ci/gate-policy.yaml#fixture_gate",
        "consumers": [],
        "compile_profiles": [],
        "registration": {
            "state": "established",
            "authority": ".ci/gate-policy.yaml",
            "detail": "tier = \"pr-fast\"; required = true"
        },
        "proof_references": [],
        "publication": { "state": "not_applicable", "authority": ".ci/gate-policy.yaml" },
        "owner": "release/ci",
        "promotion": { "state": "not_evaluated" }
    })
}

fn isolated_fixture() -> Value {
    json!({
        "surface_id": "fuzz:intentionally_isolated",
        "class": "lab",
        "class_authority": {
            "kind": "derived",
            "authority": "fuzz/fuzz_targets/intentionally_isolated.rs",
            "rule": "fuzz-targets"
        },
        "semantic_authority": "fuzz/fuzz_targets/intentionally_isolated.rs",
        "consumers": ["perl-parser-fuzz"],
        "compile_profiles": ["fuzz"],
        "registration": {
            "state": "established",
            "authority": "fuzz/Cargo.toml",
            "detail": "[[bin]] name = \"intentionally_isolated\""
        },
        "proof_references": [],
        "publication": { "state": "not_applicable", "authority": "fuzz/Cargo.toml" },
        "owner": "perl-parser-fuzz",
        "promotion": { "state": "not_evaluated" },
        "notes": "intentionally isolated from product routing"
    })
}

fn expect_fail(row: &Value, needle: &str) -> TestResult {
    let finding = evaluate_raw_row(row, Some(&repo_root()));
    assert_eq!(finding.verdict, Verdict::Fail, "expected fail, got {finding:?}");
    assert!(
        finding.reasons.iter().any(|reason| reason.contains(needle)),
        "expected `{needle}` in reasons: {:?}",
        finding.reasons
    );
    Ok(())
}

fn expect_pass(row: &Value) -> TestResult {
    let finding = evaluate_raw_row(row, Some(&repo_root()));
    assert_eq!(finding.verdict, Verdict::Pass, "expected pass, got fail: {:?}", finding.reasons);
    Ok(())
}

// ---------------------------------------------------------------------------
// Positive class fixtures
// ---------------------------------------------------------------------------

#[test]
fn product_fixture_with_registration_consumer_and_evidence_passes() -> TestResult {
    expect_pass(&product_fixture())
}

#[test]
fn preview_fixture_with_limitation_state_passes() -> TestResult {
    expect_pass(&preview_fixture())
}

#[test]
fn lab_fixture_without_product_consumer_passes() -> TestResult {
    let row = lab_fixture();
    assert!(
        !row["consumers"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|consumer| consumer.as_str() == Some("crates/perl-lsp-rs")),
        "lab fixture must not list a product consumer"
    );
    expect_pass(&row)
}

#[test]
fn shim_fixture_with_retirement_boundary_passes() -> TestResult {
    expect_pass(&shim_fixture())
}

#[test]
fn test_api_fixture_with_named_profile_passes() -> TestResult {
    expect_pass(&test_api_fixture())
}

#[test]
fn gate_fixture_with_policy_identity_passes() -> TestResult {
    expect_pass(&gate_fixture())
}

#[test]
fn intentionally_isolated_lab_passes_its_own_class_contract() -> TestResult {
    expect_pass(&isolated_fixture())
}

// ---------------------------------------------------------------------------
// Negative controls from #9205
// ---------------------------------------------------------------------------

#[test]
fn cargo_dependency_alone_does_not_activate_a_product_row() -> TestResult {
    let mut row = product_fixture();
    row["registration"]["state"] = json!("not_established");
    row["registration"]["authority"] = Value::Null;
    row["registration"]["detail"] = Value::Null;
    row["proof_references"] = json!([]);
    row["notes"] = json!("direct Cargo dependency of perl-lsp-rs");
    expect_fail(&row, "established registration")?;
    let finding = evaluate_raw_row(&row, None);
    assert!(
        finding.reasons.iter().any(|reason| reason.contains("evidence")
            || reason.contains("proof")
            || reason.contains("dispatch")),
        "dependency-only product must also miss evidence/dispatch: {:?}",
        finding.reasons
    );
    Ok(())
}

#[test]
fn legitimate_lab_does_not_fail_for_missing_product_consumer() -> TestResult {
    let finding = evaluate_raw_row(&lab_fixture(), None);
    assert_eq!(finding.verdict, Verdict::Pass, "{:?}", finding.reasons);
    assert!(
        finding.reasons.iter().all(|reason| !reason.contains("perl-lsp-rs")),
        "lab evaluation must not demand a product consumer: {:?}",
        finding.reasons
    );
    Ok(())
}

#[test]
fn advertised_product_with_tests_but_no_executable_route_fails() -> TestResult {
    let mut row = product_fixture();
    row["registration"]["state"] = json!("not_established");
    row["registration"]["detail"] = Value::Null;
    expect_fail(&row, "established registration")
}

#[test]
fn compatibility_shim_without_retirement_owner_fails() -> TestResult {
    let mut row = shim_fixture();
    row.as_object_mut().ok_or("shim fixture is an object")?.remove("retirement");
    expect_fail(&row, "retirement")
}

#[test]
fn unknown_activation_class_fails_closed() -> TestResult {
    let mut row = product_fixture();
    row["class"] = json!("mystery");
    expect_fail(&row, "unknown activation class")
}

#[test]
fn unknown_or_blank_product_evidence_fails_closed() -> TestResult {
    let mut row = product_fixture();
    row["proof_references"] =
        json!([{ "class": " ", "id": "crates/perl-lsp-rs/tests/lsp_hover_tests.rs" }]);
    expect_fail(&row, "unknown or blank proof evidence")
}

#[test]
fn slashless_unknown_product_evidence_fails_closed() -> TestResult {
    let mut row = product_fixture();
    row["proof_references"] = json!([{ "class": "integration_test", "id": "not-a-proof" }]);
    expect_fail(&row, "unknown proof evidence")
}

#[test]
fn stale_product_proof_path_fails_closed() -> TestResult {
    let mut row = product_fixture();
    row["proof_references"] = json!([{
        "class": "integration_test",
        "id": "crates/does-not-exist-9205.rs"
    }]);
    expect_fail(&row, "stale proof evidence")
}

#[test]
fn missing_product_evidence_fails_closed() -> TestResult {
    let mut row = product_fixture();
    row["proof_references"] = json!([]);
    expect_fail(&row, "requires evidence")
}

#[test]
fn preview_without_limitation_is_accidental_ga() -> TestResult {
    let mut row = preview_fixture();
    row["registration"]["state"] = json!("established");
    row["owner"] = json!("crates/perl-lsp-rs");
    row.as_object_mut().ok_or("preview fixture is an object")?.remove("notes");
    expect_fail(&row, "accidental GA")
}

#[test]
fn preview_notes_are_not_a_limitation() -> TestResult {
    let mut row = preview_fixture();
    row["registration"]["state"] = json!("established");
    row["owner"] = json!("crates/perl-lsp-rs");
    row["notes"] = json!("see also related work");
    expect_fail(&row, "accidental GA")
}

#[test]
fn lab_without_runnable_profile_or_registration_fails() -> TestResult {
    let mut row = lab_fixture();
    row["compile_profiles"] = json!([]);
    row["registration"]["state"] = json!("not_established");
    expect_fail(&row, "runnable compile profile")
}

#[test]
fn unregistered_fuzz_target_is_not_runnable() -> TestResult {
    let mut row = lab_fixture();
    row["compile_profiles"] = json!(["fuzz"]);
    row["registration"]["state"] = json!("not_established");
    expect_fail(&row, "established [[bin]] registration")
}

#[test]
fn lab_without_decision_consumer_fails() -> TestResult {
    let mut row = lab_fixture();
    row["consumers"] = json!([]);
    expect_fail(&row, "decision consumer")
}

#[test]
fn lab_without_receipt_identity_fails() -> TestResult {
    let mut row = lab_fixture();
    row["registration"]["detail"] = Value::Null;
    expect_fail(&row, "receipt identity")
}

#[test]
fn test_api_without_named_profile_fails() -> TestResult {
    let mut row = test_api_fixture();
    row["compile_profiles"] = json!([]);
    expect_fail(&row, "named harness/compile profile")
}

#[test]
fn whitespace_only_harness_names_are_not_named() -> TestResult {
    let mut row = test_api_fixture();
    row["compile_profiles"] = json!([" "]);
    row["consumers"] = json!(["\t"]);
    expect_fail(&row, "named harness/compile profile")?;
    expect_fail(&row, "named harness consumer")
}

#[test]
fn whitespace_only_product_consumers_are_not_named() -> TestResult {
    let mut row = product_fixture();
    row["consumers"] = json!([" "]);
    expect_fail(&row, "at least one consumer")
}

#[test]
fn gate_without_established_policy_registration_fails() -> TestResult {
    let mut row = gate_fixture();
    row["registration"]["state"] = json!("not_established");
    expect_fail(&row, "established registration")
}

#[test]
fn checker_source_does_not_consult_cargo_dependency_membership() {
    let src = include_str!("../src/activation/check.rs");
    assert!(
        !src.contains("parse_crate_deps") && !src.contains("is_direct_dep"),
        "activation check must not treat Cargo dependency membership as wiring"
    );
}

// ---------------------------------------------------------------------------
// Live inventory composition
// ---------------------------------------------------------------------------

#[test]
fn committed_inventory_satisfies_class_contracts() -> TestResult {
    let report = activation::check(&repo_root()).map_err(|error| error.to_string())?;
    assert!(
        report.is_clean(),
        "committed inventory failed class contracts:\n{}",
        activation::render_report(&report)
    );
    assert_eq!(report.schema, activation::CHECK_SCHEMA);
    assert_eq!(report.controlling_issue, "#9205");
    Ok(())
}

#[test]
fn explain_known_product_surface_and_reject_unknown_id() -> TestResult {
    let finding = activation::explain(&repo_root(), "feature:lsp.hover")
        .map_err(|error| error.to_string())?;
    assert_eq!(finding.surface_id, "feature:lsp.hover");
    assert_eq!(finding.class, "product");
    assert_eq!(finding.verdict, Verdict::Pass);

    let error = match activation::explain(&repo_root(), "feature:does-not-exist") {
        Ok(finding) => format!("unexpected finding {finding:?}"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("unknown activation surface"), "unknown id must fail closed: {error}");
    Ok(())
}

#[test]
fn report_render_is_deterministic_and_names_failures() -> TestResult {
    let report = activation::check(&repo_root()).map_err(|error| error.to_string())?;
    let first = activation::render_report(&report);
    let second = activation::render_report(&report);
    assert_eq!(first, second);
    assert!(first.contains("activation check (activation_check.v1, #9205)"));
    assert!(
        first.contains("pass  feature:lsp.hover (product)"),
        "human report must name passing rows: {first}"
    );
    let summary = activation::render_summary(&report);
    assert!(summary.contains("all class contracts satisfied"), "{summary}");
    assert!(
        !summary.contains("feature:lsp.hover"),
        "compact check output must not dump every passing row: {summary}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn run_activation(args: &[&str]) -> TestResult<std::process::Output> {
    use assert_cmd::cargo::cargo_bin;
    let mut cmd = std::process::Command::new(cargo_bin("xtask"));
    cmd.arg("activation");
    for arg in args {
        cmd.arg(arg);
    }
    Ok(cmd.output()?)
}

#[test]
fn cli_check_report_and_explain_operate_on_the_committed_inventory() -> TestResult {
    let check = run_activation(&["check"])?;
    assert!(
        check.status.success(),
        "activation check must pass the committed inventory; stderr={}",
        String::from_utf8_lossy(&check.stderr)
    );
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(stdout.contains("activation_check.v1"), "{stdout}");
    assert!(stdout.contains("#9205"), "{stdout}");
    assert!(stdout.contains("all class contracts satisfied"), "{stdout}");

    let human_report = run_activation(&["report"])?;
    assert!(
        human_report.status.success(),
        "activation report must succeed; stderr={}",
        String::from_utf8_lossy(&human_report.stderr)
    );
    let human = String::from_utf8_lossy(&human_report.stdout);
    assert!(
        human.contains("pass  feature:lsp.hover (product)"),
        "human report must list passing rows: {human}"
    );

    let report = run_activation(&["report", "--json"])?;
    assert!(
        report.status.success(),
        "activation report --json must succeed; stderr={}",
        String::from_utf8_lossy(&report.stderr)
    );
    let parsed: Value = serde_json::from_slice(&report.stdout)?;
    assert_eq!(parsed["schema"].as_str(), Some("activation_check.v1"));
    assert_eq!(parsed["controlling_issue"].as_str(), Some("#9205"));
    assert_eq!(parsed["failed"].as_u64(), Some(0));

    let explain = run_activation(&["explain", "feature:lsp.hover"])?;
    assert!(
        explain.status.success(),
        "explain known surface; stderr={}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let explained: Value = serde_json::from_slice(&explain.stdout)?;
    assert_eq!(explained["surface_id"].as_str(), Some("feature:lsp.hover"));
    assert_eq!(explained["verdict"].as_str(), Some("pass"));

    let unknown = run_activation(&["explain", "feature:does-not-exist"])?;
    assert!(!unknown.status.success(), "unknown surface must fail closed");
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(stderr.contains("unknown activation surface"), "unknown explain stderr: {stderr}");
    Ok(())
}
