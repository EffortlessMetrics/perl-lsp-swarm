//! Contract tests for PR-body quality proof guidance.

#[path = "quality_gate_cli_support/mod.rs"]
mod quality_gate_cli_support;

use std::{error::Error, fs, path::Path};

use quality_gate_cli_support::*;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[test]
fn pr_templates_require_quality_proof_block() -> TestResult {
    let root = repo_root()?;
    let template_paths = existing_pr_templates(&root)?;
    let mut blocks = Vec::new();
    for path in &template_paths {
        let content = fs::read_to_string(path)?;
        blocks.push((path.display().to_string(), quality_block(&content)?.to_string()));
    }

    let first_block = blocks.first().ok_or("no PR template blocks found")?.1.as_str();
    for (path, block) in blocks.iter().skip(1) {
        assert_eq!(
            first_block, block,
            "duplicate PR template {path} must keep the quality proof block in sync"
        );
    }
    for required in [
        "## Quality Proof",
        "coverage/proof/enforcement lane",
        "- Objective:",
        "- Claim boundary:",
        "- Non-goals:",
        "- RIPR/coverage effect:",
        "- Local proof commands and pass/fail results:",
        "- Cleanup performed (`rtk git status --short --branch`, `rtk git diff --check`, `rtk bash scripts/storage-doctor`):",
        "- What remains (advisory burn-down debt or `N/A`):",
    ] {
        assert!(first_block.contains(required), "quality proof block must include `{required}`");
    }
    Ok(())
}

#[test]
fn pr_templates_use_rtk_for_local_verification_commands() -> TestResult {
    let root = repo_root()?;
    for path in existing_pr_templates(&root)? {
        let content = fs::read_to_string(&path)?;
        assert!(
            content.contains("`rtk cargo xtask fmt`"),
            "PR template {} must show rtk-prefixed local verification commands",
            path.display()
        );
        assert!(
            content.contains("`rtk git status --short --branch`")
                && content.contains("`rtk git diff --check`")
                && content.contains("`rtk bash scripts/storage-doctor`"),
            "PR template {} must include cleanup evidence commands",
            path.display()
        );
        assert!(
            content.contains("`rtk cargo clippy -p <crate> --tests`")
                && content.contains("`rtk cargo test -p <crate>`"),
            "PR template {} must rtk-prefix the standard clippy/test verification commands",
            path.display()
        );
        for command in inline_command_snippets(&content) {
            let first = command.split_whitespace().next().unwrap_or_default();
            assert!(
                !matches!(first, "cargo" | "just" | "git" | "gh"),
                "PR template {} must not use bare local command `{command}`",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn pr_templates_offer_coverage_proof_lane() -> TestResult {
    let root = repo_root()?;
    for path in existing_pr_templates(&root)? {
        let content = fs::read_to_string(&path)?;
        let block = lane_block(&content)?;
        assert!(
            block.contains("- [ ] coverage / proof / enforcement"),
            "PR template {} must let proof-lane PRs declare their actual lane",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn quality_gate_summary_includes_pr_body_guidance_and_local_commands() -> TestResult {
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

    let markdown = fs::read_to_string(summary)?;
    for required in [
        "## Quality Gates",
        "## PR Summary Guidance",
        "Objective: one sentence naming the proof target",
        "Claim boundary: state what this proves",
        "Non-goals: explicitly note no LSP 3.18 behavior work",
        "RIPR/coverage effect: new-gap count",
        "Local proof commands: paste the commands run and their pass/fail result",
        "Cleanup performed: state `rtk git status --short --branch`, `rtk git diff --check`, and `rtk bash scripts/storage-doctor` results",
        "What remains: name any advisory burn-down debt",
        "Suggested local proof commands for this gate:",
        "rtk cargo xtask quality-gate --mode enforce-patch-coverage",
        "rtk git status --short --branch",
        "rtk git diff --check",
        "rtk bash scripts/storage-doctor",
    ] {
        assert!(markdown.contains(required), "quality-gate summary missing `{required}`");
    }

    Ok(())
}

fn existing_pr_templates(root: &Path) -> TestResult<Vec<std::path::PathBuf>> {
    let candidates = [".github/PULL_REQUEST_TEMPLATE.md", ".github/pull_request_template.md"];
    let mut paths = Vec::new();
    for candidate in candidates {
        let path = root.join(candidate);
        if path.exists()
            && !paths.iter().any(|existing: &std::path::PathBuf| same_path(existing, &path))
        {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err("repository must contain a PR template".into());
    }
    Ok(paths)
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.canonicalize().ok() == right.canonicalize().ok()
}

fn repo_root() -> TestResult<std::path::PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest must be nested under repo root".into())
}

fn quality_block(template: &str) -> TestResult<&str> {
    let start =
        template.find("## Quality Proof").ok_or("PR template is missing ## Quality Proof")?;
    let rest = &template[start..];
    let end = rest
        .find("\n## Promotion Discipline")
        .ok_or("quality proof block must appear before promotion discipline")?;
    Ok(&rest[..end])
}

fn lane_block(template: &str) -> TestResult<&str> {
    let start = template.find("## Lane").ok_or("PR template is missing ## Lane")?;
    let rest = &template[start..];
    let end =
        rest.find("\n## Claim Boundary").ok_or("lane block must appear before claim boundary")?;
    Ok(&rest[..end])
}

fn inline_command_snippets(template: &str) -> Vec<&str> {
    template
        .split('`')
        .enumerate()
        .filter_map(|(index, snippet)| if index % 2 == 1 { Some(snippet.trim()) } else { None })
        .filter(|snippet| !snippet.is_empty())
        .filter(|snippet| {
            snippet.split_whitespace().next().is_some_and(|first| {
                matches!(first, "rtk" | "cargo" | "just" | "git" | "gh" | "bash")
            })
        })
        .collect()
}
