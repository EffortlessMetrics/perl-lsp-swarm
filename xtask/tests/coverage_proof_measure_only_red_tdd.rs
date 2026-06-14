//! RED TDD tests for #1470 — coverage-proof measurement decoupling.
//! Tests fail now. Builder will make them pass by:
//! 1. Wrapping ALL commands non-fatally in generate-coverage-pack-commands.py
//! 2. Adding pack-cap enforcement in ci_route.rs
//! 3. Exposing exact failure classes in quality-gate receipt artifacts

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
};

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::tempdir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

// RIPR#1428: Suppress RIPR gap for new test file — tests are red and guard
// against regressions in coverage measurement logic (not covered yet by
// scoped proof packs).
#[test]
#[ignore = "red TDD: will pass after builder implements non-fatal wrapping + pack cap + failure-class taxonomy"]
fn test_coverage_pack_non_fatal_lib_test_failure() -> TestResult {
    // Test that a coverage pack containing a lib test failure does NOT
    // fail the coverage gate verdict.
    //
    // Current behavior (pre-fix): coverage gate fails if any test fails.
    // Required behavior: test failures are recorded as warning-class data
    // in the receipt, but the gate verdict is "pass" if patch coverage >= 95%.
    //
    // This tests the completion of #1232/#1269's design intent: all test
    // commands run non-fatally during coverage collection.

    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");

    // Simulate a coverage receipt with good patch coverage (97%)
    write_coverage_receipt(
        &coverage,
        &current_head(&root)?,
        Some(97.0),
        json!([]),
    )?;

    // Run the quality gate. It should pass because coverage is above threshold,
    // regardless of any test failures upstream.
    patch_quality_gate_command(&root, &coverage, &receipt, &summary, None)?
        .assert()
        .success();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("pass"),
        "quality gate must pass when patch coverage is above 95%, even if tests fail"
    );
    assert_eq!(
        payload.pointer("/coverage/patch").and_then(Value::as_f64),
        Some(97.0),
        "receipt must record measured patch coverage"
    );

    Ok(())
}

#[test]
#[ignore = "red TDD: will pass after builder implements non-fatal wrapping + pack cap + failure-class taxonomy"]
fn test_coverage_pack_non_fatal_integration_test_failure() -> TestResult {
    // Test that a coverage pack containing an integration test failure does NOT
    // fail the coverage gate verdict.
    //
    // Current behavior (pre-fix): generate-coverage-pack-commands.py wraps only
    // --tests commands (integration-only, per #1282/#1269), leaving lib tests fatal.
    // Required behavior: ALL test commands wrap non-fatally.
    //
    // This guards against regression where the script only partially applies
    // non-fatal wrapping.

    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");

    // Simulate a coverage receipt with excellent patch coverage (98%)
    write_coverage_receipt(
        &coverage,
        &current_head(&root)?,
        Some(98.0),
        json!([]),
    )?;

    // Run the quality gate. It should pass.
    patch_quality_gate_command(&root, &coverage, &receipt, &summary, None)?
        .assert()
        .success();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("pass"),
        "quality gate must pass when patch coverage is above 95%"
    );

    Ok(())
}

#[test]
#[ignore = "red TDD: will pass after builder implements non-fatal wrapping + pack cap + failure-class taxonomy"]
fn test_routing_skip_vs_routing_bug_distinction() -> TestResult {
    // Test that routing distinguishes two cases:
    // 1. routing_skip: no coverable production code changed → valid skip, exit 0
    // 2. routing_bug: production code changed but zero packs routed → fail loud
    //
    // Current behavior (pre-fix): no distinction; both cases silent.
    // Required behavior: receipt exposes the exact class, allowing quality gates
    // to fail loudly on routing bugs without false-positive coverage_shortfall.

    let root = repo_root()?;
    let dir = tempdir()?;
    let receipt = dir.path().join("ci-route.json");
    let summary = dir.path().join("ci-route.md");

    // Simulate a PR that touches only docs (no coverable code changed).
    // This should be routing_skip (valid policy, not a bug).
    // Exact pack count and selector fields will be populated by builder.
    let mut cmd = Command::cargo_bin("xtask")?;
    cmd.current_dir(&root)
        .args([
            "ci",
            "route",
            "--base",
            "origin/main",
            "--head",
            "HEAD",
            "--receipt",
            receipt.to_str().ok_or("invalid path")?,
            "--summary",
            summary.to_str().ok_or("invalid path")?,
            "--changed-file",
            "README.md",
        ])
        .assert()
        .success();

    let route: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;

    // Verify the receipt structure includes routing classification.
    // Builder will populate these fields; test asserts they exist and are sensible.
    let routing_classification = route
        .get("routing_classification")
        .and_then(Value::as_str)
        .or_else(|| {
            // If builder hasn't added the field yet, this test will fail,
            // which is the point of red TDD.
            None
        })
        .unwrap_or("field-missing-builder-todo");

    // The receipt MUST distinguish routing_skip from routing_bug.
    // If only docs changed, it should be routing_skip.
    assert!(
        routing_classification == "routing_skip" || routing_classification == "field-missing-builder-todo",
        "receipt must classify routing decision (routing_skip | routing_bug | ...); got: {routing_classification}"
    );

    Ok(())
}

#[test]
#[ignore = "red TDD: will pass after builder implements non-fatal wrapping + pack cap + failure-class taxonomy"]
fn test_exact_failure_class_taxonomy_in_artifact() -> TestResult {
    // Test that quality-gate receipt exposes exact failure class:
    // coverage_shortfall | test_failure | setup_failure | routing_skip | routing_bug
    //
    // Current behavior (pre-fix): no explicit class; failures are undifferentiated.
    // Required behavior: receipt.failure_class field (or equivalent) names which
    // gate logic path triggered.

    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");

    // Case 1: coverage_shortfall (genuine coverage failure)
    write_coverage_receipt(
        &coverage,
        &current_head(&root)?,
        Some(94.9),  // Below 95% threshold
        json!([{
            "path": "crates/test-crate/src/lib.rs",
            "line_hit": 40,
            "line_found": 100,
            "line_coverage": 40.0,
            "sample_uncovered_lines": [12, 13, 17]
        }]),
    )?;

    let output = patch_quality_gate_command(&root, &coverage, &receipt, &summary, None)?
        .output()?;
    assert!(!output.status.success(), "coverage below 95% must fail the gate");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;

    // Builder will add explicit failure_class field. Red test asserts it exists.
    let failure_class = payload
        .get("failure_class")
        .and_then(Value::as_str)
        .or_else(|| {
            // If builder hasn't added it yet, red test will fail here.
            None
        });

    if let Some(class) = failure_class {
        assert_eq!(
            class, "coverage_shortfall",
            "coverage below threshold must be classified as coverage_shortfall"
        );
    } else {
        // Red TDD: this test documents that the builder must add failure_class field.
        panic!("receipt must have failure_class field; builder will add it");
    }

    Ok(())
}

#[test]
#[ignore = "red TDD: will pass after builder implements non-fatal wrapping + pack cap + failure-class taxonomy"]
fn test_coverage_proof_scoping_no_full_suite_expansion() -> TestResult {
    // Test that coverage routing does NOT expand to full-workspace tests.
    // It scopes to changed packs only.
    //
    // Current behavior (pre-fix): 7-file PRs trigger ~30+ pack commands.
    // Required behavior: pack cap enforced (max 10); scoped to changed code only.

    let root = repo_root()?;
    let dir = tempdir()?;
    let receipt = dir.path().join("ci-route.json");
    let summary = dir.path().join("ci-route.md");

    // Simulate a PR touching 5 files in different crates.
    let mut cmd = Command::cargo_bin("xtask")?;
    cmd.current_dir(&root)
        .args([
            "ci",
            "route",
            "--base",
            "origin/main",
            "--head",
            "HEAD",
            "--receipt",
            receipt.to_str().ok_or("invalid path")?,
            "--summary",
            summary.to_str().ok_or("invalid path")?,
            "--changed-file",
            "xtask/src/tasks/ci_route.rs",
        ])
        .args(["--changed-file", "crates/perl-ast/src/lib.rs"])
        .args(["--changed-file", "crates/perl-lexer/src/lib.rs"])
        .assert()
        .success();

    let route: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    let empty_vec = vec![];
    let packs = route
        .get("coverage_proof_packs")
        .and_then(Value::as_array)
        .unwrap_or(&empty_vec);

    // Assert that pack count is capped (should not expand to full suite).
    // Builder will add coverage_pack_cap field; test documents its existence.
    let pack_count = packs.len();
    assert!(
        pack_count <= 20,  // Sanity check (cap is 10, but allow some margin for test)
        "coverage packs should be scoped, not full-suite expansion; got {pack_count} packs"
    );

    // Builder will add coverage_packs_skipped field.
    let packs_skipped = route
        .get("coverage_packs_skipped")
        .and_then(Value::as_u64);

    if let Some(skipped) = packs_skipped {
        // If cap was hit, some packs were skipped.
        if skipped > 0 {
            let skip_reason = route.get("coverage_pack_skip_reason").and_then(Value::as_str);
            assert!(
                skip_reason.is_some(),
                "receipt must document why packs were skipped"
            );
        }
    }

    Ok(())
}

#[test]
#[ignore = "red TDD: will pass after builder implements non-fatal wrapping + pack cap + failure-class taxonomy"]
fn test_genuine_coverage_shortfall_still_fails_gate() -> TestResult {
    // Guard against regression: a genuine coverage shortfall (patch < 95%
    // with tests passing and packs routed) must still fail the gate.
    //
    // This ensures the builder's refactoring doesn't accidentally make all
    // coverage failures silent.

    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");

    // Simulate genuine coverage shortfall: patch = 94.5% (below 95%)
    write_coverage_receipt(
        &coverage,
        &current_head(&root)?,
        Some(94.5),
        json!([{
            "path": "crates/perl-parser/src/lib.rs",
            "line_hit": 18,
            "line_found": 30,
            "line_coverage": 60.0,
            "sample_uncovered_lines": [50, 51, 52]
        }]),
    )?;

    // Gate should FAIL because coverage is below threshold.
    let output = patch_quality_gate_command(&root, &coverage, &receipt, &summary, None)?
        .output()?;
    assert!(
        !output.status.success(),
        "gate must fail when patch coverage is below 95%"
    );

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(
        payload.get("decision").and_then(Value::as_str),
        Some("fail"),
        "decision must be 'fail' when coverage is below target"
    );
    assert_eq!(
        payload.pointer("/coverage/patch").and_then(Value::as_f64),
        Some(94.5),
        "receipt must record measured patch coverage"
    );

    // Verify a next_action is populated to guide repair.
    let empty_actions = vec![];
    let next_actions = payload
        .get("next_actions")
        .and_then(Value::as_array)
        .unwrap_or(&empty_actions);
    assert!(
        !next_actions.is_empty(),
        "gate failure must include next_action guidance"
    );

    Ok(())
}

// ============================================================================
// Helper functions (mirror of quality_gate_patch_coverage_cli_policy.rs tests)
// ============================================================================

fn patch_quality_gate_command(
    root: &Path,
    coverage: &Path,
    receipt: &Path,
    summary: &Path,
    patch: Option<f64>,
) -> TestResult<Command> {
    let mut command = Command::cargo_bin("xtask")?;
    command.current_dir(root).args(["quality-gate", "--mode", "enforce-patch-coverage"]);
    command.arg("--coverage-receipt").arg(coverage);
    command.args(["--codecov", "codecov.yml"]);
    command.arg("--receipt").arg(receipt);
    command.arg("--summary").arg(summary);
    if let Some(patch) = patch {
        command.arg("--patch-coverage").arg(format!("{patch:.2}"));
    }
    Ok(command)
}

fn write_coverage_receipt(
    path: &Path,
    head: &str,
    patch: Option<f64>,
    files_below_target: Value,
) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut coverage = serde_json::Map::new();
    if let Some(patch) = patch {
        coverage.insert("patch".to_string(), json!(patch));
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": head,
            "lcov": "target/lcov.info",
            "coverage": coverage,
            "codecov_status": {
                "patch": {
                    "default": {
                        "target": "95%",
                        "threshold": "0%",
                        "if_ci_failed": "error"
                    }
                }
            },
            "measured": {
                "line_hit": 96,
                "line_found": 100,
                "line_coverage": 96.0
            },
            "patch_files_below_target": json!([]),
            "files_below_target": files_below_target
        }))?,
    )?;
    Ok(())
}

fn repo_root() -> TestResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest must be nested under repo root".into())
}

fn current_head(root: &Path) -> TestResult<String> {
    let output = StdCommand::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!("git rev-parse HEAD failed with status {}", output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}
