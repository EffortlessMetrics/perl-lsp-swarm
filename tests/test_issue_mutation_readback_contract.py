from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SKILL_PATHS = (
    ".agents/skills/prepare-issue/SKILL.md",
    ".claude/skills/prepare-issue/SKILL.md",
)
SECTION_HEADING = "### Canonical mutation readback"
REQUIRED_MARKERS = (
    "Reject outgoing text containing a control character other than ordinary tab or newline.",
    "A successful mutation response alone is not proof that the intended canonical state exists.",
    "Re-read the exact GitHub target after the mutation",
    "APPLIED_AND_VERIFIED",
    "WRITE_REJECTED",
    "UNKNOWN_OUTCOME",
    "READBACK_MISMATCH",
    "do not retry until the exact target has been read back",
    "never post a duplicate comment",
    "Never report an issue, comment, or PR field as canonical from the local request alone.",
    "return `NOT_PROVEN` with the observed mismatch",
)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def readback_section(text: str) -> str:
    match = re.search(
        rf"(?ms)^{re.escape(SECTION_HEADING)}\n\n(.*?)(?=^##\s|\Z)",
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

    def test_negative_controls_reject_incomplete_contracts(self) -> None:
        source = read(SKILL_PATHS[0])
        for marker in REQUIRED_MARKERS:
            mutant = source.replace(marker, "", 1)
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
