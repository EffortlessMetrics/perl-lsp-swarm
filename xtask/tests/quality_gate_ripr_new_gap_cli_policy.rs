#[path = "quality_gate_cli_support/mod.rs"]
mod quality_gate_cli_support;

use std::fs;

use serde_json::Value;
use tempfile::tempdir;

use quality_gate_cli_support::*;

#[test]
fn quality_gate_cli_blocks_new_ripr_gaps_with_actionable_receipt() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 1)?;
    write_review_guidance_receipt(&review, &head)?;
    write_coverage_receipt(&coverage, &head)?;
    write_exception_policy(&exceptions)?;

    let output = new_ripr_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "new RIPR gap enforcement must fail when diff-scoped severe_gaps is nonzero"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/kind").and_then(Value::as_str), Some("quality_gate"));
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-new-ripr"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("present"));
    assert_eq!(payload.pointer("/ripr_pr/status").and_then(Value::as_str), Some("present"));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(1));
    assert_eq!(payload.pointer("/review_guidance/status").and_then(Value::as_str), Some("present"));

    let action = next_action(&payload, "new_ripr_gap")?;
    assert_eq!(action.pointer("/top_gaps/0/gap_id").and_then(Value::as_str), Some("RIPR-SPEC-CLI"));
    assert_eq!(
        action.pointer("/top_gaps/0/path").and_then(Value::as_str),
        Some("crates/perl-parser/src/lib.rs")
    );
    assert_eq!(action.pointer("/top_gaps/0/line").and_then(Value::as_u64), Some(42));
    assert_eq!(
        action.pointer("/top_gaps/0/suggested_test").and_then(Value::as_str),
        Some("prove parser branch recovery")
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .contains("cargo xtask quality-gate --mode enforce-new-ripr")
            && verify.contains("--check")),
        "new RIPR gap action must carry the aggregate verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(
            |receipt| receipt.contains("cargo xtask quality-gate --mode enforce-new-ripr")
        ),
        "new RIPR gap action must carry the aggregate receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("new_ripr_gap"), "{markdown}");
    assert!(markdown.contains("RIPR-SPEC-CLI"), "{markdown}");
    assert!(markdown.contains("crates/perl-parser/src/lib.rs"), "{markdown}");
    assert!(markdown.contains("prove parser branch recovery"), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_new_ripr_when_required_receipts_are_missing() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("missing-ripr-plus.json");
    let ripr_pr = dir.path().join("missing-repo-exposure.json");
    let review = dir.path().join("missing-comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_coverage_receipt(&coverage, &head)?;
    write_exception_policy(&exceptions)?;

    let output = new_ripr_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "new RIPR enforcement must fail when required RIPR proof receipts are missing"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-new-ripr"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("missing"));
    assert_eq!(payload.pointer("/ripr_pr/status").and_then(Value::as_str), Some("missing"));
    assert_eq!(payload.pointer("/review_guidance/status").and_then(Value::as_str), Some("missing"));
    assert_blocking_actions_have_repair_contract(&payload)?;

    let ripr_action = next_action(&payload, "ripr_receipt_not_current")?;
    assert_eq!(ripr_action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(ripr_action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        ripr_action
            .get("verify")
            .and_then(Value::as_str)
            .is_some_and(|verify| verify.starts_with("rtk cargo xtask ripr-plus --receipt")
                && verify.contains("--check")),
        "missing repo-wide RIPR proof must carry the focused verify command: {ripr_action}"
    );
    assert!(
        ripr_action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-plus --receipt")
                && !receipt_command.contains("--check")
        }),
        "missing repo-wide RIPR proof must carry the focused receipt command: {ripr_action}"
    );

    let pr_action = next_action(&payload, "ripr_pr_receipt_not_current")?;
    assert_eq!(pr_action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(pr_action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        pr_action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD")
            && verify.contains("--check")),
        "missing diff-scoped RIPR proof must carry the focused verify command: {pr_action}"
    );
    assert!(
        pr_action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD")
                && !receipt_command.contains("--check")
        }),
        "missing diff-scoped RIPR proof must carry the focused receipt command: {pr_action}"
    );

    let review_action = next_action(&payload, "ripr_review_receipt_not_current")?;
    assert_eq!(review_action.get("reason").and_then(Value::as_str), Some("missing"));
    assert_eq!(review_action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        review_action.get("repair").and_then(Value::as_str).is_some_and(|repair| {
            repair.contains("exact file, line, seam, and suggested proof")
        }),
        "missing review guidance must explain why the receipt is required: {review_action}"
    );
    assert!(
        review_action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD")
            && verify.contains("--check")),
        "missing review guidance must carry the focused verify command: {review_action}"
    );
    assert!(
        review_action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command
                .starts_with("rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD")
                && !receipt_command.contains("--check")
        }),
        "missing review guidance must carry the focused receipt command: {review_action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("ripr_pr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("ripr_review_receipt_not_current"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_new_ripr_when_repo_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_stale_ripr_plus_receipt(&ripr)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_coverage_receipt(&coverage, &head)?;
    write_exception_policy(&exceptions)?;

    let output = new_ripr_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "new RIPR enforcement must fail when repo-wide RIPR+ proof is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-new-ripr"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("stale"));
    assert_eq!(payload.pointer("/ripr_pr/new_unresolved").and_then(Value::as_u64), Some(0));

    let action = next_action(&payload, "ripr_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        action.get("receipt_head").and_then(Value::as_str),
        Some("quality-gate-cli-stale-head")
    );
    assert_eq!(action.get("expected_head").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action
            .get("repair")
            .and_then(Value::as_str)
            .is_some_and(|repair| repair.contains("does not require total RIPR+ zero yet")
                && repair.contains("current total-debt proof")),
        "stale RIPR receipt failure must explain the transition contract: {action}"
    );
    assert!(
        action
            .get("verify")
            .and_then(Value::as_str)
            .is_some_and(|verify| verify.starts_with("rtk cargo xtask ripr-plus --receipt")
                && verify.contains("--check")),
        "stale RIPR receipt failure must carry the focused verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with("rtk cargo xtask ripr-plus --receipt")
                && !receipt_command.contains("--check")
        }),
        "stale RIPR receipt failure must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_new_ripr_when_pr_receipt_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_stale_ripr_pr_receipt(&ripr_pr)?;
    write_empty_review_guidance_receipt(&review, &head)?;
    write_coverage_receipt(&coverage, &head)?;
    write_exception_policy(&exceptions)?;

    let output = new_ripr_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "new RIPR enforcement must fail when diff-scoped PR evidence is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-new-ripr"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("present"));
    assert_eq!(payload.pointer("/ripr_pr/status").and_then(Value::as_str), Some("stale"));

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
        "stale PR receipt failure must explain the missing proof: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify
            .starts_with("rtk cargo xtask ripr-pr --base quality-gate-cli-test-base --head HEAD")
            && verify.contains("--check")),
        "stale PR receipt failure must carry the focused verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with(
                "rtk cargo xtask ripr-pr --base quality-gate-cli-test-base --head HEAD",
            ) && !receipt_command.contains("--check")
        }),
        "stale PR receipt failure must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_pr_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-pr-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}

#[test]
fn quality_gate_cli_blocks_new_ripr_when_review_guidance_is_stale() -> TestResult {
    let root = repo_root()?;
    let dir = tempfile::tempdir()?;
    let ripr = dir.path().join("ripr-plus.json");
    let ripr_pr = dir.path().join("repo-exposure.json");
    let review = dir.path().join("comments.json");
    let coverage = dir.path().join("coverage-baseline.json");
    let exceptions = dir.path().join("quality-gate-exceptions.toml");
    let receipt = dir.path().join("quality-gate.json");
    let summary = dir.path().join("quality-gate.md");
    let head = current_head(&root)?;

    write_ripr_plus_receipt(&ripr, &head)?;
    write_ripr_pr_receipt(&ripr_pr, &head, 0)?;
    write_stale_review_guidance_receipt(&review)?;
    write_coverage_receipt(&coverage, &head)?;
    write_exception_policy(&exceptions)?;

    let output = new_ripr_quality_gate_command(
        &root,
        &ripr,
        &ripr_pr,
        &review,
        &coverage,
        &exceptions,
        &receipt,
        &summary,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "new RIPR enforcement must fail when review guidance is stale"
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("quality gate failed"), "{stderr}");

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/mode").and_then(Value::as_str), Some("enforce-new-ripr"));
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert_eq!(payload.pointer("/ripr_plus/status").and_then(Value::as_str), Some("present"));
    assert_eq!(payload.pointer("/ripr_pr/status").and_then(Value::as_str), Some("present"));
    assert_eq!(payload.pointer("/review_guidance/status").and_then(Value::as_str), Some("stale"));

    let action = next_action(&payload, "ripr_review_receipt_not_current")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("stale"));
    assert_eq!(
        action.get("receipt_head_sha").and_then(Value::as_str),
        Some("quality-gate-cli-stale-review-head")
    );
    assert_eq!(action.get("expected_head_sha").and_then(Value::as_str), Some(head.as_str()));
    assert!(
        action.get("repair").and_then(Value::as_str).is_some_and(|repair| {
            repair.contains("RIPR review-guidance receipt")
                && repair.contains("exact file, line, seam, and suggested proof")
        }),
        "stale review guidance failure must explain why the proof is required: {action}"
    );
    assert!(
        action.get("verify").and_then(Value::as_str).is_some_and(|verify| verify.starts_with(
            "rtk cargo xtask ripr-review-comments --base quality-gate-cli-test-base --head HEAD",
        ) && verify
            .contains("--check")),
        "stale review guidance failure must carry the focused verify command: {action}"
    );
    assert!(
        action.get("receipt").and_then(Value::as_str).is_some_and(|receipt_command| {
            receipt_command.starts_with(
                "rtk cargo xtask ripr-review-comments --base quality-gate-cli-test-base --head HEAD",
            ) && !receipt_command.contains("--check")
        }),
        "stale review guidance failure must carry the focused receipt command: {action}"
    );

    let markdown = fs::read_to_string(&summary)?;
    assert!(markdown.contains("ripr_review_receipt_not_current"), "{markdown}");
    assert!(markdown.contains("receipt-head `quality-gate-cli-stale-review-head`"), "{markdown}");
    assert!(markdown.contains(&format!("expected-head `{head}`")), "{markdown}");

    Ok(())
}
