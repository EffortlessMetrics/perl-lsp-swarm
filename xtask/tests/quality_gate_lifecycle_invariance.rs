use assert_cmd::Command;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
};
use tempfile::tempdir;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[test]
fn candidate_verdict_is_invariant_across_review_and_expiry_dates() -> TestResult {
    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage.json");
    write_json(
        &coverage,
        json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": current_head(&root)?,
            "lcov": "target/lcov.info",
            "coverage": { "patch": 97.1 },
            "files_below_target": []
        }),
    )?;

    let boundary = Utc::now().date_naive();
    let elapsed = boundary - Duration::days(1);
    let future = boundary + Duration::days(1);
    let cases = [
        ("elapsed", elapsed.to_string(), elapsed.to_string()),
        ("boundary", boundary.to_string(), boundary.to_string()),
        ("future", future.to_string(), future.to_string()),
    ];

    for (case, review_after, expires) in &cases {
        let policy = dir.path().join(format!("policy-{case}.toml"));
        let receipt = dir.path().join(format!("receipt-{case}.json"));
        let summary = dir.path().join(format!("summary-{case}.md"));
        fs::write(&policy, policy_text(review_after, expires))?;

        patch_gate(&root, &coverage, &policy, &receipt, &summary)?.assert().success();

        let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
        assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("pass"));
        assert_eq!(
            payload.pointer("/temporary_exceptions/active_count").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            payload.pointer("/temporary_exceptions/active/0/review_after").and_then(Value::as_str),
            Some(review_after.as_str())
        );
        assert_eq!(
            payload.pointer("/temporary_exceptions/active/0/expires").and_then(Value::as_str),
            Some(expires.as_str())
        );
        assert_eq!(
            payload.pointer("/temporary_exceptions/lifecycle_authority").and_then(Value::as_str),
            Some("policy_cadence")
        );
        assert_eq!(
            payload
                .pointer("/temporary_exceptions/missing_required")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0)
        );

        let lifecycle_actions = payload
            .get("next_actions")
            .and_then(Value::as_array)
            .map(|actions| {
                actions
                    .iter()
                    .filter_map(|action| action.get("kind").and_then(Value::as_str))
                    .filter(|&kind| {
                        matches!(
                            kind,
                            "quality_exception_review_due"
                                | "quality_exception_expired"
                                | "quality_exception_required_missing"
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert!(
            lifecycle_actions.is_empty(),
            "{case} candidate inherited lifecycle action(s): {lifecycle_actions:?}"
        );

        let markdown = fs::read_to_string(summary)?;
        assert!(markdown.contains("authority: `cargo xtask policy cadence`"));
        assert!(markdown.contains("candidate impact: `advisory_only`"));
    }

    Ok(())
}

#[test]
fn missing_policy_still_emits_fail_closed_artifacts() -> TestResult {
    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage.json");
    let policy = dir.path().join("missing-policy.toml");
    let receipt = dir.path().join("receipt.json");
    let summary = dir.path().join("summary.md");
    write_json(
        &coverage,
        json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": current_head(&root)?,
            "lcov": "target/lcov.info",
            "coverage": { "patch": 97.1 },
            "files_below_target": []
        }),
    )?;

    patch_gate(&root, &coverage, &policy, &receipt, &summary)?.assert().failure();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(
        payload.pointer("/temporary_exceptions/status").and_then(Value::as_str),
        Some("missing")
    );
    assert!(payload.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
        actions.iter().any(|action| {
            action.get("kind").and_then(Value::as_str)
                == Some("quality_exception_policy_not_current")
        })
    }));
    assert!(fs::read_to_string(summary)?.contains("quality_exception_policy_not_current"));
    Ok(())
}

#[test]
fn unsupported_due_review_still_emits_fail_closed_artifacts() -> TestResult {
    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage.json");
    let policy = dir.path().join("policy.toml");
    let receipt = dir.path().join("receipt.json");
    let summary = dir.path().join("summary.md");
    write_json(
        &coverage,
        json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": current_head(&root)?,
            "lcov": "target/lcov.info",
            "coverage": { "patch": 97.1 },
            "files_below_target": []
        }),
    )?;
    fs::write(
        &policy,
        policy_text("2099-01-01", "2099-12-31")
            .replace("due_review = \"fail\"", "due_review = \"error\""),
    )?;

    patch_gate(&root, &coverage, &policy, &receipt, &summary)?.assert().failure();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(
        payload.pointer("/temporary_exceptions/validation_error").and_then(Value::as_str),
        Some("quality exception due_review must be warn or fail, found error")
    );
    let action = payload
        .get("next_actions")
        .and_then(Value::as_array)
        .and_then(|actions| {
            actions.iter().find(|action| {
                action.get("kind").and_then(Value::as_str)
                    == Some("quality_exception_policy_not_current")
            })
        })
        .ok_or("missing quality_exception_policy_not_current action")?;
    assert_eq!(action.get("reason").and_then(Value::as_str), Some("invalid_due_review"));
    assert!(
        fs::read_to_string(summary)?
            .contains("quality exception due_review must be warn or fail, found error")
    );
    Ok(())
}

#[test]
fn malformed_lifecycle_date_still_fails_structural_validation() -> TestResult {
    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage.json");
    let policy = dir.path().join("policy.toml");
    let receipt = dir.path().join("receipt.json");
    let summary = dir.path().join("summary.md");
    write_json(
        &coverage,
        json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": current_head(&root)?,
            "lcov": "target/lcov.info",
            "coverage": { "patch": 97.1 },
            "files_below_target": []
        }),
    )?;
    fs::write(&policy, policy_text("2026-09-10", "not-a-date"))?;

    patch_gate(&root, &coverage, &policy, &receipt, &summary)?.assert().failure();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(payload.pointer("/decision").and_then(Value::as_str), Some("fail"));
    assert!(payload.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
        actions.iter().any(|action| {
            action.get("kind").and_then(Value::as_str) == Some("quality_exception_invalid")
        })
    }));
    Ok(())
}

#[test]
fn rejected_duplicate_id_row_cannot_corrupt_active_lifecycle_dates() -> TestResult {
    let root = repo_root()?;
    let dir = tempdir()?;
    let coverage = dir.path().join("coverage.json");
    let policy = dir.path().join("policy.toml");
    let receipt = dir.path().join("receipt.json");
    let summary = dir.path().join("summary.md");
    write_json(
        &coverage,
        json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": current_head(&root)?,
            "lcov": "target/lcov.info",
            "coverage": { "patch": 97.1 },
            "files_below_target": []
        }),
    )?;
    fs::write(&policy, malformed_created_duplicate_policy())?;

    patch_gate(&root, &coverage, &policy, &receipt, &summary)?.assert().failure();

    let payload: Value = serde_json::from_str(&fs::read_to_string(&receipt)?)?;
    assert_eq!(
        payload.pointer("/temporary_exceptions/active/0/review_after").and_then(Value::as_str),
        Some("2026-10-16")
    );
    assert_eq!(
        payload.pointer("/temporary_exceptions/active/0/expires").and_then(Value::as_str),
        Some("2026-10-30")
    );
    assert!(payload.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
        actions.iter().any(|action| {
            action.get("kind").and_then(Value::as_str) == Some("quality_exception_invalid")
        })
    }));
    Ok(())
}

fn patch_gate(
    root: &Path,
    coverage: &Path,
    policy: &Path,
    receipt: &Path,
    summary: &Path,
) -> TestResult<Command> {
    let mut command = Command::cargo_bin("xtask")?;
    command
        .current_dir(root)
        .args(["quality-gate", "--mode", "enforce-patch-coverage"])
        .arg("--coverage-receipt")
        .arg(coverage)
        .arg("--exception-policy")
        .arg(policy)
        .args(["--codecov", "codecov.yml"])
        .arg("--receipt")
        .arg(receipt)
        .arg("--summary")
        .arg(summary);
    Ok(command)
}

fn policy_text(review_after: &str, expires: &str) -> String {
    format!(
        r##"schema_version = 1
policy = "quality-gate-exceptions"
owner = "test"
status = "active"
updated = "2026-09-10"
due_review = "fail"

[requirements]
required_active = ["fixture"]

[[exception]]
id = "fixture"
kind = "temporary_burndown"
scope = "project_coverage"
owner = "proof"
issue = "#15267"
reason = "fixture inherited debt"
final_target = "fixture target"
evidence = "fixture evidence"
removal_criteria = "fixture removal"
created = "2026-01-01"
review_after = "{review_after}"
expires = "{expires}"
"##
    )
}

fn malformed_created_duplicate_policy() -> &'static str {
    r##"schema_version = 1
policy = "quality-gate-exceptions"
owner = "test"
status = "active"
updated = "2026-09-10"
due_review = "fail"

[requirements]
required_active = ["fixture"]

[[exception]]
id = "fixture"
kind = "temporary_burndown"
scope = "project_coverage"
owner = "proof"
issue = "#15267"
reason = "rejected"
final_target = "fixture target"
evidence = "fixture evidence"
removal_criteria = "fixture removal"
created = "not-a-date"
review_after = "2026-09-16"
expires = "2026-09-30"

[[exception]]
id = "fixture"
kind = "temporary_burndown"
scope = "project_coverage"
owner = "proof"
issue = "#15267"
reason = "accepted"
final_target = "fixture target"
evidence = "fixture evidence"
removal_criteria = "fixture removal"
created = "2026-01-01"
review_after = "2026-10-16"
expires = "2026-10-30"
"##
}

fn current_head(root: &Path) -> TestResult<String> {
    let output = StdCommand::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!("git rev-parse HEAD failed with {}", output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn repo_root() -> TestResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest must have a repository parent".into())
}

fn write_json(path: &Path, value: Value) -> TestResult {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&value)?))?;
    Ok(())
}
