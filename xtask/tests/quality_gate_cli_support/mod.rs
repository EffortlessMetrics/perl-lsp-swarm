//! Shared helpers for quality-gate CLI integration tests.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
};

use assert_cmd::Command;
use serde_json::{Value, json};

pub type TestResult<T = ()> = Result<T, Box<dyn Error>>;

pub fn patch_quality_gate_command(
    root: &Path,
    coverage: &Path,
    exceptions: &Path,
    receipt: &Path,
    summary: &Path,
) -> TestResult<Command> {
    let mut command = Command::cargo_bin("xtask")?;
    command.current_dir(root).args([
        "quality-gate",
        "--mode",
        "enforce-patch-coverage",
        "--ripr-receipt",
        "target/receipts/quality/missing-ripr-plus.json",
        "--ripr-pr-receipt",
        "target/ripr/pr/missing-repo-exposure.json",
        "--review-receipt",
        "target/ripr/review/missing-comments.json",
        "--coverage-receipt",
    ]);
    command.arg(coverage);
    command.args(["--codecov", "codecov.yml", "--patch-status-source", "codecov", "--exceptions"]);
    command.arg(exceptions);
    command.arg("--receipt").arg(receipt);
    command.arg("--summary").arg(summary);
    Ok(command)
}

pub fn patch_quality_gate_command_with_cli_patch(
    root: &Path,
    coverage: &Path,
    exceptions: &Path,
    receipt: &Path,
    summary: &Path,
    patch: f64,
) -> TestResult<Command> {
    let mut command = Command::cargo_bin("xtask")?;
    command.current_dir(root).args([
        "quality-gate",
        "--mode",
        "enforce-patch-coverage",
        "--ripr-receipt",
        "target/receipts/quality/missing-ripr-plus.json",
        "--ripr-pr-receipt",
        "target/ripr/pr/missing-repo-exposure.json",
        "--review-receipt",
        "target/ripr/review/missing-comments.json",
        "--coverage-receipt",
    ]);
    command.arg(coverage);
    command.args(["--codecov", "codecov.yml", "--patch-coverage"]);
    command.arg(format!("{patch:.2}"));
    command.arg("--exceptions").arg(exceptions);
    command.arg("--receipt").arg(receipt);
    command.arg("--summary").arg(summary);
    Ok(command)
}

pub fn new_ripr_quality_gate_command(
    root: &Path,
    ripr: &Path,
    ripr_pr: &Path,
    review: &Path,
    coverage: &Path,
    exceptions: &Path,
    receipt: &Path,
    summary: &Path,
) -> TestResult<Command> {
    let mut command = Command::cargo_bin("xtask")?;
    command.current_dir(root).args(["quality-gate", "--mode", "enforce-new-ripr"]);
    command.arg("--ripr-receipt").arg(ripr);
    command.arg("--ripr-pr-receipt").arg(ripr_pr);
    command.arg("--review-receipt").arg(review);
    command.arg("--coverage-receipt").arg(coverage);
    command.args(["--codecov", "codecov.yml", "--exceptions"]);
    command.arg(exceptions);
    command.arg("--receipt").arg(receipt);
    command.arg("--summary").arg(summary);
    Ok(command)
}

pub fn final_quality_gate_command(
    root: &Path,
    ripr: &Path,
    ripr_pr: &Path,
    review: &Path,
    coverage: &Path,
    codecov: &Path,
    exceptions: &Path,
    receipt: &Path,
    summary: &Path,
) -> TestResult<Command> {
    let mut command = Command::cargo_bin("xtask")?;
    command.current_dir(root).args(["quality-gate", "--mode", "enforce"]);
    command.arg("--ripr-receipt").arg(ripr);
    command.arg("--ripr-pr-receipt").arg(ripr_pr);
    command.arg("--review-receipt").arg(review);
    command.arg("--coverage-receipt").arg(coverage);
    command.arg("--codecov").arg(codecov);
    command.arg("--exceptions").arg(exceptions);
    command.arg("--receipt").arg(receipt);
    command.arg("--summary").arg(summary);
    Ok(command)
}

pub fn next_action<'a>(receipt: &'a Value, kind: &str) -> TestResult<&'a Value> {
    receipt
        .get("next_actions")
        .and_then(Value::as_array)
        .and_then(|actions| {
            actions.iter().find(|action| action.get("kind").and_then(Value::as_str) == Some(kind))
        })
        .ok_or_else(|| format!("missing next action `{kind}`").into())
}

pub fn next_actions_contain(receipt: &Value, kind: &str) -> bool {
    receipt.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
        actions.iter().any(|action| action.get("kind").and_then(Value::as_str) == Some(kind))
    })
}

pub fn assert_failure_stderr_points_to_receipt_and_summary(
    stderr: &str,
    receipt: &Path,
    summary: &Path,
) -> TestResult {
    let receipt = receipt.to_string_lossy();
    let summary = summary.to_string_lossy();
    for required in
        ["quality gate failed", "see receipt", receipt.as_ref(), "summary", summary.as_ref()]
    {
        assert!(
            stderr.contains(required),
            "quality-gate failure stderr missing `{required}`: {stderr}"
        );
    }
    Ok(())
}

pub fn assert_blocking_actions_have_repair_contract(receipt: &Value) -> TestResult {
    let actions =
        receipt.get("next_actions").and_then(Value::as_array).ok_or("missing next_actions")?;
    let blocking = actions
        .iter()
        .filter(|action| action.get("blocking").and_then(Value::as_bool) == Some(true))
        .collect::<Vec<_>>();
    if blocking.is_empty() {
        return Err("receipt must contain at least one blocking action".into());
    }
    for action in blocking {
        let kind = action.get("kind").and_then(Value::as_str).unwrap_or("unknown");
        let path = action
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("blocking action {kind} missing path"))?;
        if path.trim().is_empty() {
            return Err(format!("blocking action {kind} has empty path").into());
        }
        for field in ["repair", "verify", "receipt"] {
            let value = action
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("blocking action {kind} missing {field}"))?;
            if value.trim().is_empty() {
                return Err(format!("blocking action {kind} has empty {field}").into());
            }
            if matches!(field, "verify" | "receipt") && !value.starts_with("rtk ") {
                return Err(format!("blocking action {kind} {field} must use rtk: {value}").into());
            }
        }
    }
    Ok(())
}

pub fn repo_root() -> TestResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest must be nested under repo root".into())
}

pub fn current_head(root: &Path) -> TestResult<String> {
    let output = StdCommand::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!("git rev-parse HEAD failed with status {}", output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub fn write_coverage_receipt(path: &Path, head: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": head,
            "lcov": "target/lcov.info",
            "codecov_status": {
                "patch": {
                    "default": {
                        "target": "95%",
                        "threshold": "0%",
                        "if_ci_failed": "error"
                    }
                },
                "project": {
                    "default": {
                        "target": "95%",
                        "threshold": "2%",
                        "if_ci_failed": "error",
                        "informational": true
                    }
                }
            },
            "codecov_comment": {
                "layout": "reach,diff,flags,files",
                "behavior": "default",
                "require_head": true
            },
            "coverage_scope": {
                "kind": "partial",
                "source_files": 2,
                "roots": ["crates/perl-parser", "xtask"],
                "required_roots": ["crates/perl-parser", "crates/perl-lsp-rs", "xtask"],
                "missing_required_roots": ["crates/perl-lsp-rs"]
            },
            "measured": {
                "line_hit": 96,
                "line_found": 100,
                "line_coverage": 96.0
            }
        }))?,
    )?;
    Ok(())
}

pub fn write_stale_coverage_receipt(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": "quality-gate-cli-stale-coverage-head",
            "lcov": "target/lcov.info",
            "codecov_status": {
                "patch": {
                    "default": {
                        "target": "95%",
                        "threshold": "0%",
                        "if_ci_failed": "error"
                    }
                },
                "project": {
                    "default": {
                        "target": "95%",
                        "threshold": "2%",
                        "if_ci_failed": "error",
                        "informational": true
                    }
                }
            },
            "codecov_comment": actionable_codecov_comment(),
            "coverage_scope": {
                "kind": "partial",
                "source_files": 2,
                "roots": ["crates/perl-parser", "xtask"],
                "required_roots": ["crates/perl-parser", "crates/perl-lsp-rs", "xtask"],
                "missing_required_roots": ["crates/perl-lsp-rs"]
            },
            "measured": {
                "line_hit": 96,
                "line_found": 100,
                "line_coverage": 96.0
            }
        }))?,
    )?;
    Ok(())
}

pub fn write_patch_gap_coverage_receipt(path: &Path, head: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": head,
            "lcov": "target/lcov.info",
            "coverage": {
                "patch": 94.9
            },
            "codecov_status": {
                "patch": {
                    "default": {
                        "target": "95%",
                        "threshold": "0%",
                        "if_ci_failed": "error"
                    }
                },
                "project": {
                    "default": {
                        "target": "95%",
                        "threshold": "2%",
                        "if_ci_failed": "error",
                        "informational": true
                    }
                }
            },
            "codecov_comment": actionable_codecov_comment(),
            "coverage_scope": {
                "kind": "partial",
                "source_files": 2,
                "roots": ["crates/perl-parser", "xtask"],
                "required_roots": ["crates/perl-parser", "crates/perl-lsp-rs", "xtask"],
                "missing_required_roots": ["crates/perl-lsp-rs"]
            },
            "measured": {
                "line_hit": 96,
                "line_found": 100,
                "line_coverage": 96.0
            },
            "files_below_target": [
                {
                    "path": "crates/perl-parser/src/lib.rs",
                    "line_hit": 4,
                    "line_found": 10,
                    "line_coverage": 40.0,
                    "sample_uncovered_lines": [12, 13, 17]
                }
            ]
        }))?,
    )?;
    Ok(())
}

pub fn write_workspace_coverage_receipt(path: &Path, root: &Path, head: &str) -> TestResult {
    write_workspace_coverage_receipt_with_values(path, root, head, 96, 96.0, 96.0, json!([]))
}

pub fn write_project_gap_workspace_coverage_receipt(
    path: &Path,
    root: &Path,
    head: &str,
) -> TestResult {
    write_workspace_coverage_receipt_with_values(
        path,
        root,
        head,
        94,
        94.0,
        96.0,
        coverage_gap_files(),
    )
}

pub fn write_patch_gap_workspace_coverage_receipt(
    path: &Path,
    root: &Path,
    head: &str,
) -> TestResult {
    write_workspace_coverage_receipt_with_values(
        path,
        root,
        head,
        96,
        96.0,
        94.0,
        coverage_gap_files(),
    )
}

pub fn write_workspace_coverage_receipt_with_values(
    path: &Path,
    root: &Path,
    head: &str,
    line_hit: u64,
    line_coverage: f64,
    patch_coverage: f64,
    files_below_target: Value,
) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "coverage_baseline",
            "head": head,
            "lcov": "target/lcov.info",
            "coverage": {
                "patch": patch_coverage
            },
            "codecov_status": {
                "patch": {
                    "default": {
                        "target": "95%",
                        "threshold": "0%",
                        "if_ci_failed": "error"
                    }
                },
                "project": {
                    "default": final_codecov_project_status()
                }
            },
            "codecov_comment": actionable_codecov_comment(),
            "coverage_scope": workspace_coverage_scope(root)?,
            "measured": {
                "line_hit": line_hit,
                "line_found": 100,
                "line_coverage": line_coverage
            },
            "files_below_target": files_below_target
        }))?,
    )?;
    Ok(())
}

pub fn coverage_gap_files() -> Value {
    json!([
        {
            "path": "crates/perl-parser/src/lib.rs",
            "line_hit": 4,
            "line_found": 10,
            "line_coverage": 40.0,
            "sample_uncovered_lines": [12, 13, 17]
        }
    ])
}

pub fn write_ripr_plus_receipt(path: &Path, head: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "ripr_plus_baseline",
            "head": head,
            "unresolved": 0,
            "top_files": []
        }))?,
    )?;
    Ok(())
}

pub fn write_actionable_ripr_plus_receipt(path: &Path, head: &str, unresolved: u64) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "ripr_plus_baseline",
            "head": head,
            "unresolved": unresolved,
            "top_files": [
                {
                    "name": "crates/perl-lexer/src/lib.rs",
                    "count": unresolved,
                    "sample_seams": [
                        {
                            "gap_id": "RIPR-SPEC-CLI-TOTAL",
                            "kind": "predicate_boundary",
                            "line": 42,
                            "seam": "lex_segment",
                            "reason": "lexer boundary branch is unobserved",
                            "suggested_test": "prove lexer boundary branch"
                        }
                    ]
                }
            ]
        }))?,
    )?;
    Ok(())
}

pub fn write_stale_ripr_plus_receipt(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "kind": "ripr_plus_baseline",
            "head": "quality-gate-cli-stale-head",
            "unresolved": 0,
            "top_files": []
        }))?,
    )?;
    Ok(())
}

pub fn write_ripr_pr_receipt(path: &Path, head: &str, severe_gaps: u64) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "0.1",
            "tool": "ripr",
            "kind": "pr_evidence",
            "scope": "diff",
            "base": "quality-gate-cli-test-base",
            "base_sha": "quality-gate-cli-test-base-sha",
            "head": "HEAD",
            "head_sha": head,
            "summary": {
                "changed_files": 1,
                "weakly_exposed": severe_gaps,
                "reachable_unrevealed": 0,
                "no_static_path": 0,
                "severe_gaps": severe_gaps
            }
        }))?,
    )?;
    Ok(())
}

pub fn write_stale_ripr_pr_receipt(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "0.1",
            "tool": "ripr",
            "kind": "pr_evidence",
            "scope": "diff",
            "base": "quality-gate-cli-test-base",
            "base_sha": "quality-gate-cli-test-base-sha",
            "head": "HEAD",
            "head_sha": "quality-gate-cli-stale-pr-head",
            "summary": {
                "changed_files": 1,
                "weakly_exposed": 0,
                "reachable_unrevealed": 0,
                "no_static_path": 0,
                "severe_gaps": 0
            }
        }))?,
    )?;
    Ok(())
}

pub fn write_review_guidance_receipt(path: &Path, head: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "0.1",
            "tool": "ripr",
            "status": "advisory",
            "base": "quality-gate-cli-test-base",
            "base_sha": "quality-gate-cli-test-base-sha",
            "head": "HEAD",
            "head_sha": head,
            "summary": {
                "comments": 1,
                "summary_only": 0,
                "suppressed": 0
            },
            "comments": [
                {
                    "canonical_gap_id": "RIPR-SPEC-CLI",
                    "kind": "focused_test",
                    "severity": "severe",
                    "reason": "changed parser branch has only weak proof",
                    "placement": {
                        "path": "crates/perl-parser/src/lib.rs",
                        "line": 42,
                        "mode": "exact_seam_line"
                    },
                    "suggested_test": {
                        "intent": "prove parser branch recovery"
                    }
                }
            ],
            "summary_only": [],
            "suppressed": [],
            "warnings": []
        }))?,
    )?;
    Ok(())
}

pub fn write_empty_review_guidance_receipt(path: &Path, head: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "0.1",
            "tool": "ripr",
            "status": "advisory",
            "base": "quality-gate-cli-test-base",
            "base_sha": "quality-gate-cli-test-base-sha",
            "head": "HEAD",
            "head_sha": head,
            "summary": {
                "comments": 0,
                "summary_only": 0,
                "suppressed": 0
            },
            "comments": [],
            "summary_only": [],
            "suppressed": [],
            "warnings": []
        }))?,
    )?;
    Ok(())
}

pub fn write_stale_review_guidance_receipt(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&json!({
            "schema_version": "0.1",
            "tool": "ripr",
            "status": "advisory",
            "base": "quality-gate-cli-test-base",
            "base_sha": "quality-gate-cli-test-base-sha",
            "head": "HEAD",
            "head_sha": "quality-gate-cli-stale-review-head",
            "summary": {
                "comments": 0,
                "summary_only": 0,
                "suppressed": 0
            },
            "comments": [],
            "summary_only": [],
            "suppressed": [],
            "warnings": []
        }))?,
    )?;
    Ok(())
}

pub fn write_final_codecov_config(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        r#"coverage:
  status:
    patch:
      default:
        target: 95%
        threshold: 0%
        if_ci_failed: error
    project:
      default:
        target: 95%
        threshold: 0.25%
        if_ci_failed: error
comment:
  layout: "reach,diff,flags,files"
  behavior: default
  require_head: true
"#,
    )?;
    Ok(())
}

pub fn write_advisory_project_codecov_config(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        r#"coverage:
  status:
    patch:
      default:
        target: 95%
        threshold: 0%
        if_ci_failed: error
    project:
      default:
        target: 95%
        threshold: 2%
        informational: true
        if_ci_failed: error
comment:
  layout: "reach,diff,flags,files"
  behavior: default
  require_head: true
"#,
    )?;
    Ok(())
}

pub fn workspace_coverage_scope(root: &Path) -> TestResult<Value> {
    let roots = required_coverage_roots(root)?;
    let source_files = u64::try_from(roots.len())?;
    Ok(json!({
        "kind": "workspace",
        "source_files": source_files,
        "roots": roots.clone(),
        "required_roots": roots,
        "missing_required_roots": []
    }))
}

pub fn required_coverage_roots(root: &Path) -> TestResult<Vec<String>> {
    let raw = fs::read_to_string(root.join("Cargo.toml"))?;
    let parsed: toml::Value = toml::from_str(&raw)?;
    let members = parsed
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or("workspace manifest is missing workspace.members")?;
    let mut roots = members
        .iter()
        .filter_map(toml::Value::as_str)
        .map(normalize_member_root)
        .filter(|member| !member.is_empty())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    Ok(roots)
}

pub fn normalize_member_root(member: &str) -> String {
    let mut normalized = member.trim().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    normalized.trim_end_matches('/').to_string()
}

pub fn final_codecov_project_status() -> Value {
    json!({
        "target": "95%",
        "threshold": "0.25%",
        "if_ci_failed": "error"
    })
}

pub fn actionable_codecov_comment() -> Value {
    json!({
        "layout": "reach,diff,flags,files",
        "behavior": "default",
        "require_head": true
    })
}

pub fn write_exception_policy(path: &Path) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "EffortlessMetrics"
status = "active"
updated = "2026-05-26"

[[exception]]
id = "ripr-total-burndown"
applies_to = "ripr_total_not_zero"
owner = "coverage-proof-lane"
reason = "Existing RIPR+ debt is being burned down while new gaps are blocking."
final_target = "ripr_plus.unresolved == 0"
current_evidence = [
  "target/receipts/quality/ripr-plus.json",
  "target/receipts/quality/quality-gate.json",
  "target/receipts/quality/quality-gate.md",
]
removal_criteria = "Remove after total RIPR+ is zero on main."
review_after = "2026-06-07"
expires = "2026-08-07"

[[exception]]
id = "project-coverage-burndown"
applies_to = "project_coverage_below_target"
owner = "coverage-proof-lane"
reason = "Project coverage is below target during the burn-down."
final_target = "coverage.project >= 95.0"
current_evidence = [
  "target/receipts/quality/coverage-baseline.json",
  "target/receipts/quality/coverage-quality-gate.json",
  "target/receipts/quality/coverage-quality-gate.md",
  "codecov.yml",
]
removal_criteria = "Remove after project coverage reaches 95% and is blocking."
review_after = "2026-06-07"
expires = "2026-08-07"
"#,
    )?;
    Ok(())
}
