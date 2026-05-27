//! Contract tests for Codecov patch-coverage enforcement.

use std::fs;
use std::path::PathBuf;

use serde_yaml_ng::Value;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

#[test]
fn codecov_patch_status_requires_95_with_no_threshold() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let raw_config = fs::read_to_string(root.join("codecov.yml"))?;
    let config: Value = serde_yaml_ng::from_str(&raw_config)?;

    assert_eq!(
        yaml_path(&config, &["coverage", "status", "patch", "default", "target"]),
        Some("95%"),
        "codecov patch status must require 95% coverage"
    );
    assert_eq!(
        yaml_path(&config, &["coverage", "status", "patch", "default", "threshold"]),
        Some("0%"),
        "codecov patch status must have no threshold allowance"
    );
    assert_ne!(
        yaml_path(&config, &["coverage", "status", "patch", "default", "informational"]),
        Some("true"),
        "codecov patch status must be blocking, not informational"
    );
    assert_eq!(
        yaml_path(&config, &["coverage", "status", "project", "default", "target"]),
        Some("95%"),
        "codecov project status should advertise the final 95% target"
    );
    assert_eq!(
        yaml_path(&config, &["coverage", "status", "project", "default", "informational"]),
        Some("true"),
        "project coverage remains informational while burn-down is open"
    );
    assert!(
        yaml_path(&config, &["comment", "layout"])
            .is_some_and(|layout| layout.split(',').any(|part| part.trim() == "diff")
                && layout.split(',').any(|part| part.trim() == "files")),
        "Codecov comments must include diff and files guidance so coverage failures are actionable"
    );
    assert_eq!(
        yaml_path(&config, &["comment", "require_head"]),
        Some("true"),
        "Codecov comments must require head coverage before reporting patch guidance"
    );
    assert!(
        !raw_config.contains("- \"xtask/**\""),
        "proof-rail xtask code must not be ignored by Codecov"
    );
    assert_eq!(
        yaml_path(&config, &["flags", "xtask", "paths", "0"]),
        Some("xtask/src/"),
        "Codecov must expose an xtask flag so proof-rail coverage is inspectable"
    );

    Ok(())
}

#[test]
fn codecov_flags_do_not_carry_status_targets() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let config: Value = serde_yaml_ng::from_str(&fs::read_to_string(root.join("codecov.yml"))?)?;
    let flags =
        config.get("flags").and_then(Value::as_mapping).ok_or("codecov.yml is missing flags")?;

    for (name, config) in flags {
        assert!(
            config.get("target").is_none(),
            "codecov flag {name:?} must not use unsupported target fields"
        );
    }

    Ok(())
}

#[test]
fn coverage_docs_use_rtk_for_local_proof_commands() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let coverage_doc = fs::read_to_string(root.join("docs/how-to/COVERAGE.md"))?;
    let rollout_doc = fs::read_to_string(root.join("docs/ci/codecov-rollout.md"))?;
    let coverage_readme = fs::read_to_string(root.join(".ci/README-coverage.md"))?;

    assert!(
        coverage_doc.contains("| **Patch** | 95% | Blocking PR gate with `0%` threshold |")
            && coverage_doc.contains("| **Project** | 95% | Overall coverage target"),
        "coverage how-to must describe the Codecov patch/project policy targets"
    );

    let acceptance = fenced_block_after(&rollout_doc, "## Acceptance gates (every PR)")
        .ok_or("codecov rollout doc is missing acceptance gate commands")?;
    assert_rtk_commands(acceptance, "Codecov acceptance gates")?;

    let receipts = fenced_block_after(&rollout_doc, "### Receipts")
        .ok_or("codecov rollout doc is missing receipt commands")?;
    assert_rtk_commands(receipts, "Codecov receipt commands")?;
    assert!(
        coverage_readme.contains("rtk just coverage-summary")
            && coverage_readme.contains("rtk just coverage-branch-gate")
            && coverage_readme.contains("rtk just coverage-baseline-refresh"),
        "coverage README must show rtk-prefixed local coverage policy commands"
    );

    Ok(())
}

#[test]
fn codecov_rollout_docs_match_current_blocking_patch_posture()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let rollout = fs::read_to_string(root.join("docs/ci/codecov-rollout.md"))?;

    assert!(
        rollout.contains("patch `95%` / `0%` blocking"),
        "rollout doc must describe the active blocking patch policy"
    );
    assert!(
        rollout.contains("patch coverage is a front-door PR gate, not a label-gated lane"),
        "rollout doc must describe patch coverage as a default PR gate"
    );
    assert!(
        !rollout.contains("PR labels (`ci:coverage`")
            && !rollout.contains("target/coverage/coverage-receipt.json"),
        "rollout doc must not preserve the older label-gated coverage receipt posture"
    );
    assert!(
        rollout.contains("target/receipts/quality/coverage-baseline.json"),
        "rollout doc must name the current coverage baseline receipt"
    );
    assert!(
        rollout.contains("single `parser-branch` upload as the active plan"),
        "historical parser-branch ladder must be explicitly marked superseded"
    );
    assert!(
        rollout.contains("| Patch status          | Codecov patch result")
            && rollout.contains("Yes, `95%` / `0%`"),
        "rollout doc must classify Codecov patch status as blocking"
    );
    assert!(
        !rollout.contains("| Project/patch status  | Codecov `parser-branch` flag"),
        "rollout doc must not classify the active patch status as informational parser-branch telemetry"
    );
    assert!(
        rollout.contains("Codecov comments include actionable diff/files guidance")
            && !rollout.contains("Codecov comments remain disabled"),
        "rollout doc must describe active Codecov comment guidance"
    );

    Ok(())
}

fn yaml_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = match current {
            Value::Mapping(mapping) => mapping.get(Value::String((*key).to_string()))?,
            Value::Sequence(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    match current {
        Value::String(value) => Some(value.as_str()),
        Value::Bool(true) => Some("true"),
        Value::Bool(false) => Some("false"),
        _ => None,
    }
}

fn fenced_block_after<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let heading_start = content.find(heading)?;
    let after_heading = &content[heading_start..];
    let fence_start = after_heading.find("```bash")? + "```bash".len();
    let after_fence = &after_heading[fence_start..];
    let content_start = after_fence.strip_prefix('\n').unwrap_or(after_fence);
    let fence_end = content_start.find("```")?;
    Some(&content_start[..fence_end])
}

fn assert_rtk_commands(block: &str, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut previous_continues = false;

    for line in block.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || previous_continues {
            previous_continues = trimmed.ends_with('\\');
            continue;
        }

        assert!(trimmed.starts_with("rtk "), "{label} command must be rtk-prefixed: {trimmed}");
        previous_continues = trimmed.ends_with('\\');
    }

    Ok(())
}
