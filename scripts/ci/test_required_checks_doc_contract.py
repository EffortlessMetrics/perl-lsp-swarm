#!/usr/bin/env python3
"""Falsifiers for scripts/ci/required_checks_doc_contract.py (#16127).

The first four tests cover the policy parsing and the line-level extraction
helpers. The remaining tests plant mutated fixture trees and assert that the
contract reports the corresponding drift finding.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

SPEC = importlib.util.spec_from_file_location(
    "required_checks_doc_contract", _HERE / "required_checks_doc_contract.py"
)
assert SPEC is not None and SPEC.loader is not None
contract = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = contract  # dataclass introspection needs sys.modules
SPEC.loader.exec_module(contract)

ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / ".ci/policies" / contract.POLICY_FILENAME


def write_tree(root: Path, files: dict[str, str]) -> None:
    for rel, content in files.items():
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")


def five_required_policy() -> str:
    return textwrap.dedent(
        """\
        version = 2

        [[checks]]
        name = "Compile All Targets (bit-rot guard)"
        required = true

        [[checks]]
        name = "Conflict marker check"
        required = true

        [[checks]]
        name = "validate-title"
        required = true

        [[checks]]
        name = "Perl LSP Rust Small Result"
        required = true

        [[checks]]
        name = "ripr+ New Gap Gate"
        required = true

        [[checks]]
        name = "Advisory Receipts"
        required = false
        """
    )


class ExtractRequiredChecksTests(unittest.TestCase):
    def test_returns_count_and_sorted_names_for_required_rows(self) -> None:
        count, names = contract.extract_required_checks(five_required_policy())
        self.assertEqual(count, 5)
        self.assertEqual(
            names,
            (
                "Compile All Targets (bit-rot guard)",
                "Conflict marker check",
                "Perl LSP Rust Small Result",
                "ripr+ New Gap Gate",
                "validate-title",
            ),
        )

    def test_zero_required_when_no_required_rows(self) -> None:
        toml = textwrap.dedent(
            """\
            [[checks]]
            name = "Advisory Receipts"
            required = false
            """
        )
        count, names = contract.extract_required_checks(toml)
        self.assertEqual(count, 0)
        self.assertEqual(names, ())

    def test_missing_checks_table_is_zero(self) -> None:
        count, names = contract.extract_required_checks("version = 2\n")
        self.assertEqual(count, 0)
        self.assertEqual(names, ())


class FindCountMentionsTests(unittest.TestCase):
    def test_digit_count_is_extracted(self) -> None:
        self.assertEqual(
            contract.find_count_mentions("the five required checks green"),
            [(5, "five required checks")],
        )

    def test_word_count_is_extracted(self) -> None:
        self.assertEqual(
            contract.find_count_mentions("all three required status checks"),
            [(3, "three required status checks")],
        )

    def test_no_match_for_unqualified_phrase(self) -> None:
        self.assertEqual(
            contract.find_count_mentions("the required checks are green"),
            [],
        )

    def test_no_match_for_inline_code_form(self) -> None:
        # Inline code spans with backtick fences are detected by the line
        # guard (and by the explicit fence-marker test below); we just assert
        # the line guard rejects a pure backtick-wrapped count.
        self.assertEqual(
            contract.find_count_mentions("`five required checks`"),
            [],
        )

    def test_no_match_inside_fence_marker(self) -> None:
        self.assertEqual(
            contract.find_count_mentions("```five required checks"),
            [],
        )

    def test_historical_line_marker_is_skipped(self) -> None:
        self.assertEqual(
            contract.find_count_mentions(
                "before #16125 the previous ruleset required two checks"
            ),
            [],
        )

    def test_fraction_pattern_is_skipped(self) -> None:
        # "at least 2 of 3 required checks are green" is a quota statement,
        # not a count claim. The contract is about the total.
        self.assertEqual(
            contract.find_count_mentions(
                "at least 2 of 3 required checks are green"
            ),
            [],
        )

    def test_fraction_pattern_out_of_is_skipped(self) -> None:
        self.assertEqual(
            contract.find_count_mentions(
                "2 out of 3 required checks pass before merging"
            ),
            [],
        )


class CheckDocTests(unittest.TestCase):
    def setUp(self) -> None:
        self.count = 5
        self.names = (
            "Compile All Targets (bit-rot guard)",
            "Conflict marker check",
            "validate-title",
            "Perl LSP Rust Small Result",
            "ripr+ New Gap Gate",
        )

    def test_compliant_prose_is_clean(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            doc = Path(tmp) / "ok.md"
            doc.write_text(
                "The five required checks must be green.\n",
                encoding="utf-8",
            )
            findings, _ = contract.check_doc(doc, self.count, self.names)
            self.assertEqual(findings, [], findings)

    def test_wrong_count_is_flagged(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            doc = Path(tmp) / "drift.md"
            doc.write_text(
                "The two required checks must be green.\n",
                encoding="utf-8",
            )
            findings, _ = contract.check_doc(doc, self.count, self.names)
            self.assertEqual(len(findings), 1, findings)
            self.assertIn("2", findings[0].detail)
            self.assertIn("5", findings[0].detail)
            self.assertEqual(findings[0].line_number, 1)

    def test_word_form_wrong_count_is_flagged(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            doc = Path(tmp) / "drift.md"
            doc.write_text(
                "all three required checks must be green.\n",
                encoding="utf-8",
            )
            findings, _ = contract.check_doc(doc, self.count, self.names)
            self.assertEqual(len(findings), 1, findings)

    def test_inside_code_block_is_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            doc = Path(tmp) / "code.md"
            doc.write_text(
                textwrap.dedent(
                    """\
                    ```text
                    the two required checks must be green
                    ```
                    And on the outside: the five required checks are green.
                    """
                ),
                encoding="utf-8",
            )
            findings, _ = contract.check_doc(doc, self.count, self.names)
            self.assertEqual(findings, [], findings)


class RunTests(unittest.TestCase):
    def test_archive_subtree_is_excluded(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_tree(
                root,
                {
                    "policy/required-checks.toml": five_required_policy(),
                    "docs/reference/active.md": (
                        "the five required checks must be green.\n"
                    ),
                    "docs/reference/archive/old.md": (
                        "the two required checks must be green.\n"
                    ),
                },
            )
            findings = contract.run(
                root,
                root / "policy" / "required-checks.toml",
                ("docs/reference",),
            )
            self.assertEqual(findings, [], findings)

    def test_real_main_tree_is_clean_after_16125(self) -> None:
        """The current main branch prose should match the current policy.

        This is a guard against future drift: if a doc re-introduces the
        wrong count, this test fails with a line-numbered message that names
        the file.
        """
        if not POLICY_PATH.exists():
            self.skipTest(f"policy file missing at {POLICY_PATH}")
        findings = contract.run(ROOT, POLICY_PATH, contract.DEFAULT_DOC_ROOTS)
        self.assertEqual(findings, [], findings)

    def test_real_main_policy_lists_five(self) -> None:
        if not POLICY_PATH.exists():
            self.skipTest(f"policy file missing at {POLICY_PATH}")
        count, _ = contract.extract_required_checks(
            POLICY_PATH.read_text(encoding="utf-8")
        )
        self.assertEqual(count, 5)

    def test_missing_policy_reports_inventory_finding(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            missing = root / "policy" / "required-checks.toml"
            findings = contract.run(root, missing, ())
            self.assertEqual(len(findings), 1, findings)
            self.assertIn("policy file not found", findings[0].detail)


if __name__ == "__main__":
    unittest.main()