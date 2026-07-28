#!/usr/bin/env python3
"""One-shot repair for the residual RTK cleanup branch."""

from pathlib import Path
import subprocess


def replace(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"expected text not found in {path}: {old!r}")
    file.write_text(text.replace(old, new), encoding="utf-8")


subprocess.run(["git", "fetch", "origin", "main"], check=True)
subprocess.run(
    [
        "git",
        "checkout",
        "origin/main",
        "--",
        "docs/project/TREE_SITTER_INCREMENTAL_CROSSOVER.md",
        "docs/reference/CI_ARCHITECTURE.md",
        "docs/reference/FAILURE_MODES.md",
    ],
    check=True,
)

for path in [
    "xtask/tests/quality_gate_cli_policy.rs",
    "xtask/tests/quality_gate_patch_coverage_cli_policy.rs",
]:
    replace(
        path,
        'assert!(value.starts_with(""), "action {field} must use rtk: {value}");',
        'assert!(\n'
        '                value.starts_with("cargo xtask "),\n'
        '                "action {field} must use a direct cargo xtask command: {value}"\n'
        '            );',
    )

replace(
    "xtask/tests/quality_gate_ripr_new_gap_cli_policy.rs",
    'if matches!(field, "verify" | "receipt") && !value.starts_with("") {\n'
    '                return Err(format!("blocking action {kind} {field} must use rtk: {value}").into());\n'
    '            }',
    'if matches!(field, "verify" | "receipt")\n'
    '                && !value.starts_with("cargo xtask ")\n'
    '            {\n'
    '                return Err(format!(\n'
    '                    "blocking action {kind} {field} must use a direct cargo xtask command: {value}"\n'
    '                )\n'
    '                .into());\n'
    '            }',
)

replace(
    "xtask/tests/quality_gate_exception_policy.rs",
    'assert!(value.starts_with("rtk "), "action {field} must use rtk: {value}");',
    'assert!(\n'
    '                value.starts_with("cargo xtask "),\n'
    '                "action {field} must use a direct cargo xtask command: {value}"\n'
    '            );',
)

path = Path("xtask/tests/codecov_patch_gate_policy.rs")
text = path.read_text(encoding="utf-8")
for old, new in [
    ('coverage_readme.contains("rtk just coverage-summary")',
     'coverage_readme.contains("just coverage-summary")'),
    ('coverage_readme.contains("rtk just coverage-branch-gate")',
     'coverage_readme.contains("just coverage-branch-gate")'),
    ('coverage_readme.contains("rtk just coverage-baseline-refresh")',
     'coverage_readme.contains("just coverage-baseline-refresh")'),
    ('"coverage README must show rtk-prefixed local coverage policy commands"',
     '"coverage README must show direct local coverage policy commands"'),
]:
    if old not in text:
        raise SystemExit(f"expected coverage policy text not found: {old}")
    text = text.replace(old, new)
path.write_text(text, encoding="utf-8")

path = Path("xtask/tests/ripr_new_gap_gate_workflow.rs")
text = path.read_text(encoding="utf-8")
old = '''#[test]
fn ripr_docs_use_rtk_for_local_proof_commands() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let block = fenced_block_after(&docs, "## Running locally")
        .ok_or("docs/ci/ripr.md is missing the Running locally command block")?;
    let commands = block.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>();
    assert!(!commands.is_empty(), "RIPR local proof block must list commands");
    for command in &commands {
        assert!(command.starts_with("rtk "), "RIPR local proof command must use rtk: {command}");
        assert!(
            !command.contains("quality-gate --mode enforce "),
            "RIPR local proof commands must not run final enforcement before burn-down: {command}"
        );
    }
'''
new = '''#[test]
fn ripr_docs_use_direct_local_proof_commands() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let block = fenced_block_after(&docs, "## Running locally")
        .ok_or("docs/ci/ripr.md is missing the Running locally command block")?;
    let commands = block.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>();
    assert!(!commands.is_empty(), "RIPR local proof block must list commands");
    for command in &commands {
        let direct = command.starts_with("cargo install ripr ")
            || command.starts_with("cargo xtask ")
            || *command == "ripr doctor";
        assert!(direct, "RIPR local proof command must be directly executable: {command}");
        assert_ne!(
            command.split_whitespace().next(),
            Some("rtk"),
            "RIPR local proof command must not use the retired RTK wrapper: {command}"
        );
        assert!(
            !command.contains("quality-gate --mode enforce "),
            "RIPR local proof commands must not run final enforcement before burn-down: {command}"
        );
    }
'''
if old not in text:
    raise SystemExit("expected RIPR local-command policy block was not found")
path.write_text(text.replace(old, new), encoding="utf-8")
