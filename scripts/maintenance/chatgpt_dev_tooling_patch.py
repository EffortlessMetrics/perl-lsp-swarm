#!/usr/bin/env python3
"""Apply the bounded #12956 developer-tooling repair in a checked-out branch."""

from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one replacement site, found {count}")
    target.write_text(text.replace(old, new), encoding="utf-8")


def find_matching_paren(text: str, opening: int) -> int:
    depth = 0
    quote: str | None = None
    escaped = False
    line_comment = False
    block_comment_depth = 0
    index = opening
    while index < len(text):
        char = text[index]
        next_char = text[index + 1] if index + 1 < len(text) else ""
        if line_comment:
            if char == "\n":
                line_comment = False
            index += 1
            continue
        if block_comment_depth:
            if char == "/" and next_char == "*":
                block_comment_depth += 1
                index += 2
                continue
            if char == "*" and next_char == "/":
                block_comment_depth -= 1
                index += 2
                continue
            index += 1
            continue
        if quote is not None:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            index += 1
            continue
        if char == "/" and next_char == "/":
            line_comment = True
            index += 2
            continue
        if char == "/" and next_char == "*":
            block_comment_depth = 1
            index += 2
            continue
        if char in {'"', "'"}:
            quote = char
            index += 1
            continue
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    raise RuntimeError("unterminated pr_evidence_packet_with_count call")


def split_top_level_arguments(text: str) -> list[str]:
    arguments: list[str] = []
    start = 0
    depths = {"(": 0, "[": 0, "{": 0}
    closing = {")": "(", "]": "[", "}": "{"}
    quote: str | None = None
    escaped = False
    line_comment = False
    block_comment_depth = 0
    index = 0
    while index < len(text):
        char = text[index]
        next_char = text[index + 1] if index + 1 < len(text) else ""
        if line_comment:
            if char == "\n":
                line_comment = False
            index += 1
            continue
        if block_comment_depth:
            if char == "/" and next_char == "*":
                block_comment_depth += 1
                index += 2
                continue
            if char == "*" and next_char == "/":
                block_comment_depth -= 1
                index += 2
                continue
            index += 1
            continue
        if quote is not None:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            index += 1
            continue
        if char == "/" and next_char == "/":
            line_comment = True
            index += 2
            continue
        if char == "/" and next_char == "*":
            block_comment_depth = 1
            index += 2
            continue
        if char in {'"', "'"}:
            quote = char
            index += 1
            continue
        if char in depths:
            depths[char] += 1
        elif char in closing:
            depths[closing[char]] -= 1
        elif char == "," and all(depth == 0 for depth in depths.values()):
            argument = text[start:index].strip()
            if argument:
                arguments.append(argument)
            start = index + 1
        index += 1
    final = text[start:].strip()
    if final:
        arguments.append(final)
    return arguments


def refactor_pr_evidence_packet_calls() -> None:
    path = ROOT / "xtask/src/tasks/ripr_evidence.rs"
    text = path.read_text(encoding="utf-8")
    needle = "pr_evidence_packet_with_count("
    fields = [
        "options",
        "check_value",
        "base_sha",
        "head_sha",
        "suppressions",
        "changed_file_count",
        "head_extents",
        "attribution_scope",
        "production_surface",
    ]
    replacements: list[tuple[int, int, str]] = []
    cursor = 0
    while True:
        start = text.find(needle, cursor)
        if start < 0:
            break
        line_start = text.rfind("\n", 0, start) + 1
        if text[line_start:start].strip() == "fn":
            cursor = start + len(needle)
            continue
        opening = start + len(needle) - 1
        end = find_matching_paren(text, opening)
        arguments = split_top_level_arguments(text[opening + 1 : end])
        if len(arguments) != len(fields):
            raise RuntimeError(
                "pr_evidence_packet_with_count call has "
                f"{len(arguments)} arguments, expected {len(fields)}"
            )
        leading = text[line_start:start]
        indent = leading[: len(leading) - len(leading.lstrip())]
        rows = []
        for field, argument in zip(fields, arguments, strict=True):
            value = field if argument == field else f"{field}: {argument}"
            rows.append(f"{indent}    {value},")
        replacement = (
            "pr_evidence_packet_with_count(PrEvidencePacketInput {\n"
            + "\n".join(rows)
            + f"\n{indent}}})"
        )
        replacements.append((start, end + 1, replacement))
        cursor = end + 1
    if len(replacements) != 7:
        raise RuntimeError(
            f"expected seven pr_evidence_packet_with_count calls, found {len(replacements)}"
        )
    for start, end, replacement in reversed(replacements):
        text = text[:start] + replacement + text[end:]

    old_signature = """fn pr_evidence_packet_with_count(
    options: &PrEvidenceOptions,
    check_value: &Value,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
    changed_file_count: usize,
    head_extents: Option<&HeadLineExtents>,
    attribution_scope: Option<&AttributionScope>,
    production_surface: Option<&ProductionSurface>,
) -> Value {
"""
    new_signature = """struct PrEvidencePacketInput<'a> {
    options: &'a PrEvidenceOptions,
    check_value: &'a Value,
    base_sha: &'a str,
    head_sha: &'a str,
    suppressions: &'a RiprSuppressionRules,
    changed_file_count: usize,
    head_extents: Option<&'a HeadLineExtents>,
    attribution_scope: Option<&'a AttributionScope>,
    production_surface: Option<&'a ProductionSurface>,
}

fn pr_evidence_packet_with_count(input: PrEvidencePacketInput<'_>) -> Value {
    let PrEvidencePacketInput {
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        changed_file_count,
        head_extents,
        attribution_scope,
        production_surface,
    } = input;
"""
    count = text.count(old_signature)
    if count != 1:
        raise RuntimeError(f"ripr_evidence signature: expected one site, found {count}")
    path.write_text(text.replace(old_signature, new_signature), encoding="utf-8")


def write_contract_test() -> None:
    path = ROOT / "xtask/tests/clippy_command_contract.rs"
    if path.exists():
        raise RuntimeError(f"{path.relative_to(ROOT)} already exists")
    path.write_text(
        r'''//! Contract for issue #12956: full-workspace Clippy callers consume the
//! canonical `clippy_full` gate rather than carrying private command copies.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GatePolicy {
    gates: Vec<Gate>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    name: String,
    command: String,
}

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn clippy_full_command() -> Result<String, Box<dyn std::error::Error>> {
    let policy = fs::read_to_string(project_root().join(".ci/gate-policy.yaml"))?;
    let parsed: GatePolicy = serde_yaml_ng::from_str(&policy)?;
    parsed
        .gates
        .into_iter()
        .find(|gate| gate.name == "clippy_full")
        .map(|gate| gate.command)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "clippy_full gate is missing").into()
        })
}

fn just_recipe<'a>(justfile: &'a str, name: &str) -> Result<&'a str, io::Error> {
    let marker = format!("\n{name}:");
    let start = justfile.find(&marker).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("just recipe {name} is missing"))
    })? + 1;
    let rest = justfile.get(start..).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "just recipe start is invalid")
    })?;
    let end = rest.find("\n\n").unwrap_or(rest.len());
    rest.get(..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "just recipe end is invalid"))
}

#[test]
fn canonical_gate_covers_workspace_libraries_and_binaries(
) -> Result<(), Box<dyn std::error::Error>> {
    let command = clippy_full_command()?.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        command.contains("cargo clippy --workspace --locked -- -D warnings -A missing_docs"),
        "clippy_full must lint workspace default targets: {command}"
    );
    assert!(
        !command.contains("--workspace --lib"),
        "clippy_full must not narrow the warning pass to libraries: {command}"
    );
    assert!(
        command.contains(
            "cargo clippy --workspace --bins --locked --no-deps -- -D clippy::unwrap_used -D clippy::expect_used"
        ),
        "clippy_full must retain the explicit bin panic-family backstop: {command}"
    );
    assert_eq!(
        command.matches("cargo clippy").count(),
        2,
        "clippy_full must carry exactly the warning pass and bin backstop"
    );
    Ok(())
}

#[test]
fn local_recipe_delegates_to_the_canonical_gate(
) -> Result<(), Box<dyn std::error::Error>> {
    let justfile = fs::read_to_string(project_root().join("justfile"))?;
    let recipe = just_recipe(&justfile, "clippy-full")?;
    assert!(
        recipe.contains("cargo xtask gates --gate clippy_full"),
        "clippy-full must delegate to the policy-owned gate: {recipe}"
    );
    assert!(
        !recipe.contains("cargo clippy"),
        "clippy-full must not carry a private cargo-clippy command: {recipe}"
    );
    Ok(())
}

#[test]
fn nightly_strict_job_delegates_to_the_canonical_gate(
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow =
        fs::read_to_string(project_root().join(".github/workflows/ci-nightly.yml"))?;
    assert!(
        workflow.contains(
            "- name: Run clippy (strict mode)\n        run: cargo xtask gates --gate clippy_full"
        ),
        "nightly strict Clippy must delegate to clippy_full"
    );
    assert!(
        !workflow.contains(
            "cargo clippy --workspace --locked -- -D warnings -D clippy::all -A missing_docs"
        ),
        "nightly must not carry its retired private Clippy command"
    );
    Ok(())
}

#[test]
fn policy_documentation_names_the_default_target_contract(
) -> Result<(), Box<dyn std::error::Error>> {
    let documentation = fs::read_to_string(project_root().join("docs/CLIPPY_POLICY.md"))?;
    assert!(
        documentation.contains(
            "required workspace default-target gate (libraries and binaries)"
        ),
        "Clippy policy must describe the maintained lib+bin warning surface"
    );
    assert!(
        !documentation.contains("required workspace `--lib` gate"),
        "Clippy policy must not retain the old lib-only claim"
    );
    Ok(())
}
''',
        encoding="utf-8",
    )


def main() -> None:
    replace_once(
        ".ci/gate-policy.yaml",
        '''  - name: clippy_full
    tier: pr_fast
    description: "Clippy lints on full workspace with strict settings"
    required: true
    command: >-
      cargo clippy
      --workspace --lib --locked
      -- -D warnings -A missing_docs &&
      cargo clippy
      --workspace --bins --locked --no-deps
      -- -D clippy::unwrap_used -D clippy::expect_used
''',
        '''  - name: clippy_full
    tier: pr_fast
    description: "Clippy lints on workspace libraries and binaries with strict settings"
    required: true
    command: >-
      cargo clippy
      --workspace --locked
      -- -D warnings -A missing_docs &&
      cargo clippy
      --workspace --bins --locked --no-deps
      -- -D clippy::unwrap_used -D clippy::expect_used
''',
    )
    replace_once(
        "justfile",
        '''# Clippy full workspace (thorough, for merge gate)
clippy-full:
    @echo "Running clippy (full workspace)..."
    cargo clippy --workspace --locked -- -D warnings -A missing_docs
    cargo clippy --workspace --bins --locked --no-deps -- -D clippy::unwrap_used -D clippy::expect_used
    @echo "Clippy (full) passed"
''',
        '''# Clippy full workspace (thorough, for merge gate)
clippy-full:
    @echo "Running clippy (full workspace)..."
    cargo xtask gates --gate clippy_full
    @echo "Clippy (full) passed"
''',
    )
    replace_once(
        ".github/workflows/ci-nightly.yml",
        "cargo clippy --workspace --locked -- -D warnings -D clippy::all -A missing_docs",
        "cargo xtask gates --gate clippy_full",
    )
    replace_once(
        "docs/CLIPPY_POLICY.md",
        "Its maintained enforcement surface is the required workspace `--lib` gate, the production `--bins` gate, and the explicitly listed all-targets kernel cohort; that cohort is intentionally non-exhaustive.",
        "Its maintained enforcement surface is the required workspace default-target gate (libraries and binaries), the explicit production `--bins` panic-family backstop, and the explicitly listed all-targets kernel cohort; that cohort is intentionally non-exhaustive.",
    )
    replace_once(
        "xtask/src/tasks/gates/planning_types.rs",
        '''use xtask::ci_route_plan::{
    CompileRoutePlanInput, ExpansionStatus, GateSelectorInput, LifecycleDisposition,
''',
        '''use xtask::ci_route_plan::{
    ExpansionStatus, GateSelectorInput, LifecycleDisposition,
''',
    )
    replace_once(
        "xtask/src/tasks/gates/planning_types.rs",
        "use xtask::ci_route_plan::{Applicability, CiRoutePlanV1, PlannedOutcome, RouteSubjectRef};",
        '''use xtask::ci_route_plan::{
        Applicability, CiRoutePlanV1, CompileRoutePlanInput, PlannedOutcome, RouteSubjectRef,
    };''',
    )
    replace_once(
        "xtask/src/tasks/ripr_evidence.rs",
        '''fn normalize_repo_relative_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    normalized.strip_prefix("./").unwrap_or(&normalized).to_string()
}
''',
        '''fn normalize_repo_relative_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    normalized.strip_prefix("./").unwrap_or(normalized.as_str()).to_string()
}
''',
    )
    replace_once(
        "xtask/src/tasks/ripr_evidence.rs",
        '''fn normalize_suppression_match_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
''',
        '''fn normalize_suppression_match_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    let normalized = normalized.strip_prefix("./").unwrap_or(normalized.as_str());
''',
    )
    replace_once(
        "xtask/src/tasks/ripr_evidence.rs",
        '''    let normalized = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
''',
        '''    let normalized = normalized.strip_prefix("//?/").unwrap_or(normalized.as_str());
    let normalized = normalized.strip_prefix("./").unwrap_or(normalized);
''',
    )
    replace_once(
        "xtask/src/tasks/ripr_evidence.rs",
        '''            ".." => {
                if components.pop().is_none() {
                    return None;
                }
            }
''',
        '''            ".." => {
                components.pop()?;
            }
''',
    )
    refactor_pr_evidence_packet_calls()
    write_contract_test()

    # This script is bootstrap-only. Its own deletion keeps the final PR surface
    # limited to the governed tooling and regression proof.
    Path(__file__).unlink()


if __name__ == "__main__":
    main()
