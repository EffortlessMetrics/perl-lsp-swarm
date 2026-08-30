#!/usr/bin/env python3
"""Apply the bounded #10060 PR-telemetry retirement on one exact branch.

This is a temporary migration runner. It fails when the expected current-source
steps or route entries have drifted instead of guessing at a partial edit.
"""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def write(relative: str, text: str) -> None:
    (ROOT / relative).write_text(text, encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def remove_yaml_step(relative: str, name: str) -> None:
    path = ROOT / relative
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    needle = f"- name: {name}"
    matches = [index for index, line in enumerate(lines) if line.strip() == needle]
    require(
        len(matches) == 1,
        f"{relative}: expected exactly one step {name!r}, found {len(matches)}",
    )
    start = matches[0]
    indent = len(lines[start]) - len(lines[start].lstrip(" "))
    end = len(lines)
    for index in range(start + 1, len(lines)):
        stripped = lines[index].lstrip(" ")
        current_indent = len(lines[index]) - len(stripped)
        if current_indent == indent and stripped.startswith("- "):
            end = index
            break
    del lines[start:end]
    path.write_text("".join(lines), encoding="utf-8")


def replace_exact(relative: str, old: str, new: str, expected: int = 1) -> None:
    text = read(relative)
    count = text.count(old)
    require(
        count == expected,
        f"{relative}: expected {expected} occurrence(s) of {old!r}, found {count}",
    )
    write(relative, text.replace(old, new))


def remove_regex(relative: str, pattern: str, expected: int = 1) -> None:
    text = read(relative)
    updated, count = re.subn(pattern, "", text, flags=re.MULTILINE | re.DOTALL)
    require(
        count == expected,
        f"{relative}: expected {expected} regex removal(s), found {count}: {pattern}",
    )
    write(relative, updated)


def remove_test_containing(relative: str, marker: str) -> None:
    path = ROOT / relative
    text = path.read_text(encoding="utf-8")
    marker_indexes = [match.start() for match in re.finditer(re.escape(marker), text)]
    require(
        len(marker_indexes) == 1,
        f"{relative}: expected marker {marker!r} once, found {len(marker_indexes)}",
    )
    marker_index = marker_indexes[0]
    start = text.rfind("#[test]", 0, marker_index)
    require(start >= 0, f"{relative}: no #[test] before {marker!r}")
    next_test = text.find("#[test]", start + 1)
    require(
        next_test < 0 or marker_index < next_test,
        f"{relative}: marker {marker!r} is not in the nearest test",
    )
    brace = text.find("{", start, marker_index)
    require(brace >= 0, f"{relative}: no function body before {marker!r}")
    depth = 0
    in_string = False
    escaped = False
    end = None
    for index in range(brace, len(text)):
        char = text[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                break
    require(end is not None, f"{relative}: unterminated test containing {marker!r}")
    while end < len(text) and text[end] == "\n":
        end += 1
    path.write_text(text[:start] + text[end:], encoding="utf-8")


NEW_CONTRACT = r'''#!/usr/bin/env python3
"""Recurrence contract for #10060: no ordinary-PR Codecov test telemetry."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
CI = WORKFLOWS / "ci.yml"
UX_GATE = WORKFLOWS / "ux-regression-gate.yml"
NIGHTLY = WORKFLOWS / "ci-nightly.yml"


def workflow_telemetry_findings(path: Path, text: str) -> list[str]:
    findings: list[str] = []
    if "report_type: test_results" in text:
        findings.append(f"{path}: report_type_test_results")
    if "codecov/test-results-action@" in text:
        findings.append(f"{path}: legacy_test_results_action")
    return findings


def operational_text_files() -> list[Path]:
    roots = [WORKFLOWS, ROOT / ".ci", ROOT / "policy", ROOT / "scripts", ROOT / "xtask"]
    excluded = {
        Path(__file__).resolve(),
        (ROOT / "scripts/ci/apply_retire_codecov_pr_telemetry.py").resolve(),
    }
    files: list[Path] = []
    for root in roots:
        for path in root.rglob("*"):
            if not path.is_file() or path.resolve() in excluded:
                continue
            if path.suffix.lower() not in {
                ".json",
                ".md",
                ".py",
                ".rs",
                ".sh",
                ".toml",
                ".yaml",
                ".yml",
            }:
                continue
            files.append(path)
    return sorted(files)


class NoPrCodecovTelemetryTests(unittest.TestCase):
    def test_pr_capable_workflows_have_no_test_results_transport(self) -> None:
        findings: list[str] = []
        for path in sorted(WORKFLOWS.glob("*.yml")):
            findings.extend(
                workflow_telemetry_findings(
                    path.relative_to(ROOT), path.read_text(encoding="utf-8")
                )
            )
        self.assertEqual([], findings)

    def test_receipt_to_junit_adapter_and_operational_references_are_gone(self) -> None:
        adapter = "receipts-to-" + "junit.py"
        adapter_test = "test_receipts_to_" + "junit.py"
        self.assertFalse((ROOT / "scripts/ci" / adapter).exists())
        self.assertFalse((ROOT / "scripts/ci" / adapter_test).exists())

        findings: list[str] = []
        for path in operational_text_files():
            text = path.read_text(encoding="utf-8")
            if adapter in text or adapter_test in text:
                findings.append(str(path.relative_to(ROOT)))
        self.assertEqual([], findings)

    def test_repository_owned_json_evidence_and_summaries_remain(self) -> None:
        ci = CI.read_text(encoding="utf-8")
        ux = UX_GATE.read_text(encoding="utf-8")
        for marker in (
            "- name: Upload PR-fast receipt",
            "target/receipts/shards",
            "- name: Upload UX regression evidence",
            "GITHUB_STEP_SUMMARY",
        ):
            self.assertIn(marker, ci)
        for marker in (
            "- name: Upload UX regression evidence",
            "target/receipts/ux-regression.json",
            "- name: Summarize UX evidence",
            "GITHUB_STEP_SUMMARY",
        ):
            self.assertIn(marker, ux)

    def test_scheduled_manual_coverage_lane_remains_outside_pr_scope(self) -> None:
        nightly = NIGHTLY.read_text(encoding="utf-8")
        match = re.search(
            r"(?ms)^  test-coverage:\n(?P<body>.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)",
            nightly,
        )
        self.assertIsNotNone(match)
        body = match.group("body")
        self.assertIn("name: Codecov / Patch 95", body)
        self.assertIn("github.event_name == 'schedule'", body)
        self.assertIn("github.event_name == 'workflow_dispatch'", body)
        self.assertNotIn("github.event_name == 'pull_request'", body)
        self.assertIn("codecov/codecov-action@", body)
        self.assertIn("files: target/lcov.info", body)

    def test_detector_rejects_reintroduced_test_results_forms(self) -> None:
        fixture = """
steps:
  - uses: codecov/test-results-action@deadbeef
  - uses: codecov/codecov-action@deadbeef
    with:
      report_type: test_results
"""
        findings = workflow_telemetry_findings(Path("fixture.yml"), fixture)
        self.assertEqual(
            [
                "fixture.yml: report_type_test_results",
                "fixture.yml: legacy_test_results_action",
            ],
            findings,
        )

    def test_detector_allows_ordinary_scheduled_coverage_upload(self) -> None:
        fixture = """
steps:
  - uses: codecov/codecov-action@deadbeef
    with:
      files: target/lcov.info
      fail_ci_if_error: false
"""
        self.assertEqual([], workflow_telemetry_findings(Path("fixture.yml"), fixture))


if __name__ == "__main__":
    unittest.main()
'''


def main() -> None:
    for step in (
        "Convert PR-fast receipt to JUnit",
        "Upload PR-fast test results to Codecov",
        "Convert gate shard receipts to JUnit",
        "Upload gate shard test results to Codecov",
        "Convert UX regression receipt to JUnit",
        "Upload UX regression test results to Codecov",
    ):
        remove_yaml_step(".github/workflows/ci.yml", step)

    for step in (
        "Convert UX regression receipt to JUnit",
        "Upload test results to Codecov without a repository secret",
    ):
        remove_yaml_step(".github/workflows/ux-regression-gate.yml", step)

    replace_exact(
        ".github/workflows/workflow-contracts-advisory.yml",
        "- name: Codecov test-results contract",
        "- name: No PR Codecov telemetry contract",
    )

    write("scripts/ci/test_codecov_test_results_workflows.py", NEW_CONTRACT)

    for relative in (
        "scripts/ci/receipts-to-junit.py",
        "scripts/ci/test_receipts_to_junit.py",
    ):
        path = ROOT / relative
        require(path.is_file(), f"expected current adapter file {relative}")
        path.unlink()

    remove_regex(
        "xtask/src/tasks/ci_route.rs",
        r'const RECEIPTS_JUNIT_PACK: ProofPack = ProofPack \{\n'
        r'    id: "receipts-junit-focused",\n'
        r'    commands: &\["python -m unittest scripts/ci/test_receipts_to_junit.py"\],\n'
        r'\};\n\n',
    )
    remove_regex(
        "xtask/src/tasks/ci_route.rs",
        r'    if file == "scripts/ci/receipts-to-junit.py" '
        r'\|\| file == "scripts/ci/test_receipts_to_junit.py" \{\n'
        r'        route\.add_surface\("receipts-junit"\);\n'
        r'        route\.add_pack\(RECEIPTS_JUNIT_PACK\);\n'
        r'        route\.add_coverage_pack\("patch-coverage-receipts-junit"\);\n'
        r'        return;\n'
        r'    \}\n\n',
    )

    remove_regex(
        ".ci/coverage-packs.toml",
        r'^\[\[pack\]\]\n'
        r'id = "patch-coverage-receipts-junit"\n'
        r'.*?(?=^\[\[pack\]\]\n|\Z)',
    )

    remove_test_containing(
        "xtask/tests/ci_route_cli.rs", "missing receipts-junit coverage pack"
    )
    remove_test_containing(
        "xtask/tests/ci_route_cli.rs",
        "receipt-to-JUnit changes must run the focused JUnit proof",
    )

    # Final migration-local checks. The durable contract runs again in CI after
    # this temporary runner is removed from the branch.
    for workflow in sorted((ROOT / ".github/workflows").glob("*.yml")):
        require(
            "report_type: test_results" not in workflow.read_text(encoding="utf-8"),
            f"test-results transport remains in {workflow.relative_to(ROOT)}",
        )
    require(
        "RECEIPTS_JUNIT_PACK" not in read("xtask/src/tasks/ci_route.rs"),
        "RECEIPTS_JUNIT_PACK remains",
    )
    require(
        "patch-coverage-receipts-junit" not in read(".ci/coverage-packs.toml"),
        "receipts-junit coverage pack remains",
    )


if __name__ == "__main__":
    main()
