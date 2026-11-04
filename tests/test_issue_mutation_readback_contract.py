from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SKILL_PATHS = (
    ".agents/skills/prepare-issue/SKILL.md",
    ".claude/skills/prepare-issue/SKILL.md",
)
TEST_PATH = "tests/test_issue_mutation_readback_contract.py"
WORKFLOW_PATH = ".github/workflows/issue-mutation-readback-contract.yml"
TRIGGER_PATHS = (*SKILL_PATHS, TEST_PATH, WORKFLOW_PATH)
SECTION_HEADING = "### Canonical mutation readback"
REQUIRED_MARKERS = (
    "Reject outgoing text containing a control character other than ordinary tab or newline.",
    "Before creating an issue comment, read the target issue's existing comments and search for the intended content identity.",
    "Treat an existing match as `APPLIED_AND_VERIFIED` and do not write a duplicate.",
    "A successful mutation response alone is not proof that the intended canonical state exists.",
    "Re-read the exact GitHub target after the mutation",
    "compare the observed identity and field against the intended bytes or a deterministic normalized digest.",
    "APPLIED_AND_VERIFIED",
    "WRITE_REJECTED",
    "UNKNOWN_OUTCOME",
    "READBACK_MISMATCH",
    "do not retry until the exact target has been read back",
    "never post a duplicate comment",
    "Never report an issue, comment, or PR field as canonical from the local request alone.",
    "Repair a mismatch through a bounded new mutation and fresh readback",
    "return `NOT_PROVEN` with the observed mismatch",
)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def readback_section(text: str) -> str:
    match = re.search(
        rf"(?ms)^{re.escape(SECTION_HEADING)}\n\n(.*?)(?=^#{{2,6}}\s|\Z)",
        text,
    )
    if match is None:
        return ""
    return " ".join(match.group(1).split())


def contract_violations(text: str) -> tuple[str, ...]:
    section = readback_section(text)
    if not section:
        return ("missing canonical mutation readback section",)
    return tuple(marker for marker in REQUIRED_MARKERS if marker not in section)


def workflow_event_paths(text: str, event: str) -> tuple[str, ...]:
    lines = text.splitlines()
    event_line = f"  {event}:"
    try:
        start = lines.index(event_line) + 1
    except ValueError:
        return ()

    end = len(lines)
    for index in range(start, len(lines)):
        line = lines[index]
        if line.startswith("  ") and not line.startswith("    "):
            end = index
            break

    block = lines[start:end]
    try:
        paths_start = block.index("    paths:") + 1
    except ValueError:
        return ()

    paths: list[str] = []
    for line in block[paths_start:]:
        if not line.startswith("      - "):
            break
        paths.append(line.removeprefix("      - ").strip().strip("'\""))
    return tuple(paths)


class IssueMutationReadbackContractTests(unittest.TestCase):
    def test_active_skills_share_one_exact_readback_contract(self) -> None:
        sections = {path: readback_section(read(path)) for path in SKILL_PATHS}
        for path, section in sections.items():
            self.assertTrue(section, f"{path} is missing {SECTION_HEADING}")
        self.assertEqual(
            sections[SKILL_PATHS[0]],
            sections[SKILL_PATHS[1]],
            "active prepare-issue providers must share the same mutation-readback contract",
        )

    def test_contract_is_fail_closed_and_duplicate_safe(self) -> None:
        for path in SKILL_PATHS:
            text = read(path)
            self.assertEqual((), contract_violations(text), path)
            bad_controls = [
                character
                for character in text
                if ord(character) < 32 and character not in {"\t", "\n"}
            ]
            self.assertEqual([], bad_controls, f"{path} contains disallowed control characters")

    def test_workflow_executes_and_tracks_the_complete_contract(self) -> None:
        workflow = read(WORKFLOW_PATH)
        for event in ("pull_request", "push"):
            self.assertEqual(
                set(TRIGGER_PATHS),
                set(workflow_event_paths(workflow, event)),
                f"workflow {event} paths do not match the complete readback contract",
            )
        self.assertIn(
            f"run: python3 -m unittest {TEST_PATH}",
            workflow,
            "workflow must execute the focused readback contract test",
        )
        self.assertNotIn("cargo test", workflow)
        self.assertNotIn("cargo fmt", workflow)

    def test_negative_controls_reject_incomplete_contracts(self) -> None:
        source = read(SKILL_PATHS[0])
        for marker in REQUIRED_MARKERS:
            mutant = source.replace(marker, "")
            self.assertIn(
                marker,
                contract_violations(mutant),
                f"removing {marker!r} must make the contract incomplete",
            )

        without_section = source.replace(SECTION_HEADING, "### Mutation notes", 1)
        self.assertEqual(
            ("missing canonical mutation readback section",),
            contract_violations(without_section),
        )


if __name__ == "__main__":
    unittest.main()
