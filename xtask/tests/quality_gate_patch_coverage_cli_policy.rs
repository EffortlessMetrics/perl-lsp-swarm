#[path = "quality_gate_cli_support/mod.rs"]
mod quality_gate_cli_support;

use std::fs;

use serde_json::Value;
use tempfile::tempdir;

use quality_gate_cli_support::*;

#[test]
fn coverage_how_to_documents_patch_gate_cli_guidance() -> TestResult {
    let root = repo_root()?;
    let coverage_doc = fs::read_to_string(root.join("docs/how-to/COVERAGE.md"))?;

    assert!(
        coverage_doc.contains(
            "rtk cargo xtask quality-gate --mode enforce-patch-coverage --codecov codecov.yml"
        ),
        "coverage how-to must show the rtk-prefixed local quality-gate command"
    );
    assert!(
        coverage_doc.contains("representative positive")
            && coverage_doc.contains("1-based uncovered line")
            && coverage_doc.contains("rejects LCOV `DA` entries whose line number is `0`")
            && coverage_doc.contains("samples")
            && coverage_doc.contains("only renders positive uncovered line samples")
            && coverage_doc.contains("Non-actionable file rows are filtered"),
        "coverage how-to must describe actionable below-target file/line guidance"
    );

    Ok(())
}

#[test]
fn quality_gate_cli_writes_and_checks_patch_gate_receipts() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");

    write_coverage_receipt(&coverage, &current_head(&root)?)?;
    write_exception_policy(&exceptions)?;

    patch_quality_gate_command(&root, &coverage, &exceptions, &receipt, &summary)?
        .assert()
        .success();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/kind").and_then(Value::as_str), Some("quality_gate"));
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-patch-coverage"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("pass"));
    assert_eq!(payload.pointer("/coverage/status").and_then(Value::as_str), Some("present"));
    assert_eq!(
        payload.pointer("/coverage/patch_source").and_then(Value::as_str),
        Some("codecov_status")
    );
    assert_eq!(
        payload.pointer("/coverage/codecov_config_status").and_then(Value::as_str),
        Some("present")
    );
    assert_eq!(payload.pointer("/exceptions/status").and_then(Value::as_str), Some("present"));

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("## Quality Gates"), "{markdown}");
    assert!(markdown.contains("Codecov patch source: `codecov_status`"), "{markdown}");
    assert!(markdown.contains("rtk cargo xtask quality-gate --mode enforce-patch-coverage"));

    patch_quality_gate_command(&root, &coverage, &exceptions, &receipt, &summary)?
        .arg("--check")
        .assert()
        .success();

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_patch_gate_when_coverage_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_stale_coverage_receipt(&coverage)?;
    write_exception_policy(&exceptions)?;

    let output =
        patch_quality_gate_command(&root, &coverage, &exceptions, &receipt, &summary)?.output()?;
    assert!(
        !output.status.success(),
        "patch coverage enforcement must fail when the coverage baseline receipt is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert_failure_stderr_points_to_receipt_and_summary(&stderr, &receipt, &summary)?;

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-patch-coverage"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/coverage/status").and_then(Value::as_str), Some("stale"));

    let action = next_action(&payload, "coverage_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        action.get("receipt_head").and_then(Value::as_str),
        Some("quality-gate-cli-stale-coverage-head")
    );
    assert_eq!(action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair.contains("Refresh the LCOV coverage receipt")),
        "stale coverage receipt failure must explain the repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
            && verify.contains("--codecov codecov.yml")
            && verify.contains("--check")),
        "stale coverage receipt failure must carry the focused verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
                && receipt_command.contains("--codecov codecov.yml")
                && !receipt_command.contains("--check")
        }),
        "stale coverage receipt failure must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("coverage_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-coverage-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_patch_gate_when_coverage_receipt_is_missing() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let coverage = dir.path().join("missing-coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_exception_policy(&exceptions)?;

    let output =
        patch_quality_gate_command(&root, &coverage, &exceptions, &receipt, &summary)?.output()?;
    assert!(
        !output.status.success(),
        "patch coverage enforcement must fail when the coverage baseline receipt is missing"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert_failure_stderr_points_to_receipt_and_summary(&stderr, &receipt, &summary)?;

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-patch-coverage"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/coverage/status").and_then(Value::as_str), Some("missing"));

    let action = next_action(&payload, "coverage_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair.contains("Refresh the LCOV coverage receipt")),
        "missing coverage receipt failure must explain the repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
            && verify.contains("--codecov codecov.yml")
            && verify.contains("--check")),
        "missing coverage receipt failure must carry the focused verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
                && receipt_command.contains("--codecov codecov.yml")
                && !receipt_command.contains("--check")
        }),
        "missing coverage receipt failure must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("coverage_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `missing`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_patch_coverage_below_target_with_file_guidance() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");

    write_patch_gap_coverage_receipt(&coverage, &current_head(&root)?)?;
    write_exception_policy(&exceptions)?;

    let output = patch_quality_gate_command_with_cli_patch(
        &root,
        &coverage,
        &exceptions,
        &receipt,
        &summary,
        94.9,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "patch coverage enforcement must fail when the measured patch is below 95%"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/kind").and_then(Value::as_str), Some("quality_gate"));
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-patch-coverage"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(94.9));
    assert_eq!(payload.pointer("/coverage/patch_source").and_then(Value::as_str), Some("cli"));
    assert_blocking_actions_have_repair_contract(&payload)?;

    let action = next_action(&payload, "patch_coverage_below_target")?;
    assert_eq!(action.get("current").and_then(Value::as_f64), Some(94.9));
    assert_eq!(action.get("target").and_then(Value::as_f64), Some(95.0));
    assert_eq!(action.get("source").and_then(Value::as_str), Some("cli"));
    assert_eq!(
        action.pointer("/top_files/0/path").and_then(Value::as_str),
        Some("crates/perl-parser/src/lib.rs")
    );
    assert_eq!(action.pointer("/top_files/0/line_coverage").and_then(Value::as_f64), Some(40.0));
    assert_eq!(
        action.pointer("/top_files/0/sample_uncovered_lines/0").and_then(Value::as_u64),
        Some(12)
    );
    assert!(
        action.get("suggested_test").and_then(Value::as_str).is_some_and(|suggested| suggested
            .contains("focused tests for error paths, boundary conditions, config parsing")),
        "patch coverage failure must suggest behavior-oriented tests: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
            && verify.contains("--patch-coverage 94.90")
            && verify.contains("--check")),
        "patch coverage failure must carry the aggregate verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask quality-gate --mode enforce-patch-coverage")
                && receipt_command.contains("--patch-coverage 94.90")
                && !receipt_command.contains("--check")
        }),
        "patch coverage failure must carry the aggregate receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("patch_coverage_below_target"), "{markdown}");
    assert!(markdown.contains("crates/perl-parser/src/lib.rs"), "{markdown}");
    assert!(markdown.contains("sample uncovered lines: 12, 13, 17"), "{markdown}");
    assert!(
        markdown.contains("suggested test: Prefer focused tests for error paths"),
        "{markdown}"
    );

    Ok(())
}
