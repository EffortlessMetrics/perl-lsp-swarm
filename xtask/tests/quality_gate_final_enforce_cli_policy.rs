#[path = "quality_gate_cli_support/mod.rs"]
mod quality_gate_cli_support;

use std::fs;

use serde_json::Value;
use tempfile::tempdir;

use quality_gate_cli_support::*;

#[test]
fn quality_gate_cli_passes_final_enforce_after_burndown_contract_is_met() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .assert()
    .success();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/kind").and_then(Value::as_str), Some("quality_gate"));
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("pass"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));
    assert_eq!(
        payload.pointer("/coverage/coverage_scope/kind").and_then(Value::as_str),
        Some("workspace")
    );
    assert_eq!(payload.pointer("/exceptions/status").and_then(Value::as_str), Some("missing"));
    assert!(
        !next_actions_contain(&payload, "temporary_exceptions_still_active"),
        "final enforce should pass only after temporary exceptions are gone: {payload}"
    );
    assert!(
        !next_actions_contain(&payload, "coverage_scope_not_workspace"),
        "workspace coverage scope should satisfy final enforce: {payload}"
    );
    assert!(
        !next_actions_contain(&payload, "project_coverage_policy_not_enforcing"),
        "final Codecov project policy should satisfy final enforce: {payload}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("## Quality Gates"), "{markdown}");
    assert!(markdown.contains("rtk cargo xtask quality-gate --mode enforce"), "{markdown}");
    assert!(markdown.contains("| Codecov project coverage | pass | 96.00% |"), "{markdown}");
    assert!(markdown.contains("| Coverage scope | pass | workspace"), "{markdown}");

    final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .arg("--check")
    .assert()
    .success();

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_ripr_total_is_nonzero() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_actionable_ripr_plus_receipt(&ripr, &head, 2)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail while repo-wide RIPR+ unresolved gaps remain"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("present"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(2));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_total_not_zero")?;
    assert_eq!(action.get("unresolved").and_then(Value::as_u64), Some(2));
    assert_eq!(
        action.pointer("/top_files/0/name").and_then(Value::as_str),
        Some("crates/perl-lexer/src/lib.rs")
    );
    assert_eq!(
        action.pointer("/top_files/0/sample_seams/0/gap_id").and_then(Value::as_str),
        Some("RIPR-SPEC-CLI-TOTAL")
    );
    assert_eq!(
        action.pointer("/top_files/0/sample_seams/0/line").and_then(Value::as_u64),
        Some(42)
    );
    assert_eq!(
        action.pointer("/top_files/0/sample_seams/0/seam").and_then(Value::as_str),
        Some("lex_segment")
    );
    assert!(
        action
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair
                .contains("Burn down the named RIPR seam clusters with focused tests")),
        "final RIPR blocker must explain the burn-down repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--check")),
        "final RIPR blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-plus --receipt")
                && !receipt_command.contains("--check")
        }),
        "final RIPR blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_total_not_zero"), "{markdown}");
    assert!(markdown.contains("crates/perl-lexer/src/lib.rs"), "{markdown}");
    assert!(markdown.contains("unresolved 2"), "{markdown}");
    assert!(markdown.contains("RIPR-SPEC-CLI-TOTAL"), "{markdown}");
    assert!(markdown.contains("seam `lex_segment`"), "{markdown}");
    assert!(markdown.contains("suggested test: prove lexer boundary branch"), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_ripr_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_stale_ripr_plus_receipt(&ripr)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the repo-wide RIPR+ receipt is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        payload.pointer("/ripr_plus/head").and_then(Value::as_str),
        Some("quality-gate-cli-stale-head")
    );
    assert_eq!(
        payload.pointer("/ripr_plus/expected_head").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        action.get("receipt_head").and_then(Value::as_str),
        Some("quality-gate-cli-stale-head")
    );
    assert_eq!(action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Regenerate and check the repo-wide RIPR+ receipt")
        ),
        "final stale RIPR blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--ripr-receipt")
            && verify.contains("--check")),
        "final stale RIPR blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-plus --receipt")
                && !receipt_command.contains("--check")
        }),
        "final stale RIPR blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `stale`"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_ripr_receipt_is_missing() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("missing-ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the repo-wide RIPR+ receipt is missing"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("missing"));
    assert_eq!(
        payload.pointer("/ripr_plus/expected_head").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), None);
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Regenerate and check the repo-wide RIPR+ receipt")
        ),
        "final missing RIPR blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--ripr-receipt")
            && verify.contains("--check")),
        "final missing RIPR blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-plus --receipt")
                && !receipt_command.contains("--check")
        }),
        "final missing RIPR blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `missing`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_coverage_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_stale_coverage_receipt(&coverage)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the coverage baseline receipt is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/status").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        payload.pointer("/coverage/head").and_then(Value::as_str),
        Some("quality-gate-cli-stale-coverage-head")
    );
    assert_eq!(
        payload.pointer("/coverage/expected_head").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));

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
        "final stale coverage blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--coverage-receipt")
            && verify.contains("--check")),
        "final stale coverage blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
                && receipt_command.contains("--codecov")
                && !receipt_command.contains("--check")
        }),
        "final stale coverage blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("coverage_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `stale`"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-coverage-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_coverage_receipt_is_missing() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("missing-coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the coverage baseline receipt is missing"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/status").and_then(Value::as_str), Some("missing"));
    assert_eq!(
        payload.pointer("/coverage/expected_head").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), None);
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), None);

    let action = next_action(&payload, "coverage_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair.contains("Refresh the LCOV coverage receipt")),
        "final missing coverage blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--coverage-receipt")
            && verify.contains("--check")),
        "final missing coverage blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
                && receipt_command.contains("--codecov")
                && !receipt_command.contains("--check")
        }),
        "final missing coverage blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("coverage_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `missing`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_ripr_pr_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_stale_ripr_pr_receipt(&ripr_pr)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the diff-scoped RIPR PR receipt is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/status").and_then(Value::as_str), Some("stale"));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload.pointer("/ripr_pr/head_sha").and_then(Value::as_str),
        Some("quality-gate-cli-stale-pr-head")
    );
    assert_eq!(
        payload.pointer("/ripr_pr/expected_head_sha").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_pr_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        action.get("receipt_head_sha").and_then(Value::as_str),
        Some("quality-gate-cli-stale-pr-head")
    );
    assert_eq!(action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Regenerate and check the diff-scoped RIPR PR receipt")
        ),
        "final stale diff-RIPR blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--ripr-pr-receipt")
            && verify.contains("--check")),
        "final stale diff-RIPR blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with(
                "rtk cargo xtask ripr-pr --base quality-gate-cli-test-base --head HEAD",
            ) && !receipt_command.contains("--check")
        }),
        "final stale diff-RIPR blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_pr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `stale`"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-pr-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_ripr_pr_receipt_is_missing() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("missing-repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the diff-scoped RIPR PR receipt is missing"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/status").and_then(Value::as_str), Some("missing"));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), None);
    assert_eq!(
        payload.pointer("/ripr_pr/expected_head_sha").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_pr_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Regenerate and check the diff-scoped RIPR PR receipt")
        ),
        "final missing diff-RIPR blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--ripr-pr-receipt")
            && verify.contains("--check")),
        "final missing diff-RIPR blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD")
                && !receipt_command.contains("--check")
        }),
        "final missing diff-RIPR blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_pr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `missing`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_review_guidance_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_stale_review_guidance_receipt(&review)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the RIPR review-guidance receipt is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/review_guidance/status").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        payload.pointer("/review_guidance/head_sha").and_then(Value::as_str),
        Some("quality-gate-cli-stale-review-head")
    );
    assert_eq!(
        payload.pointer("/review_guidance/expected_head_sha").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_review_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        action.get("receipt_head_sha").and_then(Value::as_str),
        Some("quality-gate-cli-stale-review-head")
    );
    assert_eq!(action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Regenerate and check the RIPR review-guidance receipt")
        ),
        "final stale review-guidance blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--review-receipt")
            && verify.contains("--check")),
        "final stale review-guidance blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with(
                "rtk cargo xtask ripr-review-comments --base quality-gate-cli-test-base --head HEAD",
            ) && !receipt_command.contains("--check")
        }),
        "final stale review-guidance blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_review_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `stale`"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-review-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_review_guidance_receipt_is_missing() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("missing-comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail when the RIPR review-guidance receipt is missing"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/review_guidance/status").and_then(Value::as_str), Some("missing"));
    assert_eq!(
        payload.pointer("/review_guidance/expected_head_sha").and_then(Value::as_str),
        Some(head.as_str())
    );
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let action = next_action(&payload, "ripr_review_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Regenerate and check the RIPR review-guidance receipt")
        ),
        "final missing review-guidance blocker must explain the receipt repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--review-receipt")
            && verify.contains("--check")),
        "final missing review-guidance blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with(
                "rtk cargo xtask ripr-review-comments --base quality-gate-cli-test-base --head HEAD",
            ) && !receipt_command.contains("--check")
        }),
        "final missing review-guidance blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_review_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("reason `missing`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_project_coverage_is_below_target() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_project_gap_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail while project coverage is below 95%"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(94.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));
    assert_eq!(
        payload.pointer("/coverage/coverage_scope/kind").and_then(Value::as_str),
        Some("workspace")
    );
    assert!(
        !next_actions_contain(&payload, "ripr_total_not_zero"),
        "project coverage failure must not imply unresolved RIPR debt: {payload}"
    );

    let action = next_action(&payload, "project_coverage_below_target")?;
    assert_eq!(action.get("current").and_then(Value::as_f64), Some(94.0));
    assert_eq!(action.get("target").and_then(Value::as_f64), Some(95.0));
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
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Add behavior tests for high-risk uncovered code")
        ),
        "final project coverage blocker must explain the burn-down repair: {action}"
    );
    assert!(
        action.get("suggested_test").and_then(Value::as_str).is_some_and(|suggested| suggested
            .contains("focused tests for error paths, boundary conditions, config parsing")),
        "final project coverage blocker must suggest behavior-oriented proof: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--check")),
        "final project coverage blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask coverage-baseline --lcov target/lcov.info --receipt")
                && receipt_command.contains("--codecov")
                && !receipt_command.contains("--check")
        }),
        "final project coverage blocker must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("project_coverage_below_target"), "{markdown}");
    assert!(markdown.contains("crates/perl-parser/src/lib.rs"), "{markdown}");
    assert!(markdown.contains("line coverage 40.00%"), "{markdown}");
    assert!(markdown.contains("sample uncovered lines: 12, 13, 17"), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_patch_coverage_is_below_target() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_patch_gap_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(!output.status.success(), "final enforce must fail while patch coverage is below 95%");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(94.0));
    assert!(
        !next_actions_contain(&payload, "project_coverage_below_target"),
        "patch coverage failure must not imply project coverage burn-down debt: {payload}"
    );

    let action = next_action(&payload, "patch_coverage_below_target")?;
    assert_eq!(action.get("current").and_then(Value::as_f64), Some(94.0));
    assert_eq!(action.get("target").and_then(Value::as_f64), Some(95.0));
    assert_eq!(action.get("source").and_then(Value::as_str), Some("coverage_receipt"));
    assert_eq!(
        action.pointer("/top_files/0/path").and_then(Value::as_str),
        Some("crates/perl-parser/src/lib.rs")
    );
    assert_eq!(action.pointer("/top_files/0/line_coverage").and_then(Value::as_f64), Some(40.0));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(|repair| repair.contains(
            "Add behavior tests for the changed code until patch coverage is at least 95%"
        )),
        "final patch coverage blocker must explain the repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--check")),
        "final patch coverage blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask quality-gate --mode enforce")
                && !receipt_command.contains("--check")
        }),
        "final patch coverage blocker must carry the aggregate receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("patch_coverage_below_target"), "{markdown}");
    assert!(markdown.contains("current `94.00` target `95.00`"), "{markdown}");
    assert!(markdown.contains("source `coverage_receipt`"), "{markdown}");
    assert!(markdown.contains("crates/perl-parser/src/lib.rs"), "{markdown}");
    assert!(markdown.contains("sample uncovered lines: 12, 13, 17"), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_when_project_policy_is_still_advisory() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("missing-quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_advisory_project_codecov_config(&codecov)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail while Codecov project coverage policy is advisory"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));
    assert_eq!(
        payload.pointer("/coverage/project_policy/threshold").and_then(Value::as_str),
        Some("2%")
    );
    assert_eq!(
        payload.pointer("/coverage/project_policy/informational").and_then(Value::as_bool),
        Some(true)
    );

    let action = next_action(&payload, "project_coverage_policy_not_enforcing")?;
    assert_eq!(action.get("target").and_then(Value::as_str), Some("95%"));
    assert_eq!(action.get("threshold").and_then(Value::as_str), Some("2%"));
    assert_eq!(action.get("informational").and_then(Value::as_bool), Some(true));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(
            |repair| repair.contains("Promote codecov.yml coverage.status.project.default")
        ),
        "final project policy blocker must explain the promotion repair: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--codecov")
            && verify.contains("--check")),
        "final project policy blocker must carry the aggregate final verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask quality-gate --mode enforce")
                && receipt_command.contains("--codecov")
                && !receipt_command.contains("--check")
        }),
        "final project policy blocker must carry the aggregate receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("project_coverage_policy_not_enforcing"), "{markdown}");
    assert!(
        markdown.contains("| Codecov project policy | fail | target 95%, threshold 2% |"),
        "{markdown}"
    );
    assert!(markdown.contains("remove informational mode"), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_final_enforce_while_temporary_exceptions_are_active() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let codecov = dir.path().join("codecov.yml");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_workspace_coverage_receipt(&coverage, &root, &head)?;
    write_final_codecov_config(&codecov)?;
    write_exception_policy(&exceptions)?;

    let output = final_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &codecov,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "final enforce must fail while transition exceptions remain active"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.pointer("/coverage/project").and_then(Value::as_f64), Some(96.0));
    assert_eq!(payload.pointer("/coverage/patch").and_then(Value::as_f64), Some(96.0));

    let blocker = next_action(&payload, "temporary_exceptions_still_active")?;
    assert_eq!(blocker.pointer("/active/0").and_then(Value::as_str), Some("ripr-total-burndown"));
    assert_eq!(
        blocker.pointer("/active/1").and_then(Value::as_str),
        Some("project-coverage-burndown")
    );
    assert!(
        blocker
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair.contains("Remove temporary burn-down exceptions")),
        "exception blocker must tell agents how to close the transition: {blocker}"
    );
    assert!(
        blocker.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask quality-gate --mode enforce")
            && verify.contains("--check")),
        "exception blocker must carry the aggregate final verify command: {blocker}"
    );
    assert!(
        blocker.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask quality-gate --mode enforce")
                && !receipt_command.contains("--check")
        }),
        "exception blocker must carry the aggregate final receipt command: {blocker}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("## Temporary Exceptions"), "{markdown}");
    assert!(markdown.contains("ripr-total-burndown"), "{markdown}");
    assert!(markdown.contains("project-coverage-burndown"), "{markdown}");
    assert!(
        markdown.contains(
            "These entries document transition debt only; they do not waive `quality-gate --mode enforce` blockers."
        ),
        "{markdown}"
    );

    Ok(())
}
