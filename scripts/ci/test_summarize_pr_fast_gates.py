#!/usr/bin/env python3
"""Focused tests for scripts/ci/summarize_pr_fast_gates.py (#15492).

Each test pins one branch of the annotation path that the PR body could only
demonstrate synthetically: the ``skip`` exclusion, workflow-command property
escaping, the ``::stop-commands::`` fence around the stdout mirror, and the
fold that keeps the annotation list inside GitHub's per-check-run cap without
losing a gate name.

These are logic-layer controls. Whether the hosted API then returns the
annotations is a separate claim, provable only on a real red run.
"""

from __future__ import annotations

import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("summarize_pr_fast_gates.py")
SPEC = importlib.util.spec_from_file_location("summarize_pr_fast_gates", SCRIPT)
assert SPEC and SPEC.loader
summarizer = importlib.util.module_from_spec(SPEC)
sys.modules["summarize_pr_fast_gates"] = summarizer
SPEC.loader.exec_module(summarizer)


def write_receipt(directory: str, gates: list[dict]) -> Path:
    path = Path(directory) / "receipt.json"
    path.write_text(json.dumps({"gates": gates}), encoding="utf-8")
    return path


def annotations_of(output: str) -> list[str]:
    return [line for line in output.splitlines() if line.startswith("::error ")]


def run(receipt: Path, token: str = "TOKEN") -> str:
    rendered, failing, problem = summarizer.summarize(receipt)
    stream = io.StringIO()
    summarizer.emit(rendered, failing, problem, token, stream=stream)
    return stream.getvalue()


class SkipExclusionTests(unittest.TestCase):
    """A short-circuited gate never ran, so it is listed but not annotated."""

    def test_skip_is_tabled_but_not_annotated(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [
                    {"gate_name": "fmt", "status": "pass", "exit_code": 0},
                    {"gate_name": "clippy", "status": "fail", "exit_code": 101},
                    {
                        "gate_name": "tests",
                        "status": "skip",
                        "exit_code": "unknown",
                    },
                ],
            )
            rendered, failing, problem = summarizer.summarize(receipt)

        self.assertIsNone(problem)
        # Both non-passing gates reach the human-readable table.
        self.assertIn("| `clippy` | `fail` | `101` |", rendered)
        self.assertIn("| `tests` | `skip` |", rendered)
        # Only the one that actually ran is annotated.
        self.assertEqual(failing, [("clippy", "status fail, exit 101")])

    def test_timeout_and_error_are_annotated(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [
                    {"gate_name": "slow", "status": "timeout", "exit_code": 124},
                    {"gate_name": "broken", "status": "error", "exit_code": 2},
                ],
            )
            _, failing, _ = summarizer.summarize(receipt)

        self.assertEqual(
            [name for name, _ in failing], ["slow", "broken"]
        )

    def test_all_passing_produces_no_annotations(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [
                    {"gate_name": "fmt", "status": "pass", "exit_code": 0},
                    {"gate_name": "clippy", "status": "passed", "exit_code": 0},
                    {"gate_name": "tests", "status": "success", "exit_code": 0},
                ],
            )
            output = run(receipt)

        self.assertEqual(annotations_of(output), [])
        self.assertIn("Non-success gates: none recorded.", output)


class ReceiptProblemTests(unittest.TestCase):
    """An absent or corrupt receipt is itself reportable, and is a warning."""

    def test_missing_receipt_warns(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = run(Path(directory) / "absent.json")

        self.assertEqual(annotations_of(output), [])
        self.assertIn(
            "::warning title=pr-fast receipt::the PR-fast runner did not produce "
            "a final receipt",
            output,
        )

    def test_unreadable_receipt_warns(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "receipt.json"
            receipt.write_text("{not json", encoding="utf-8")
            output = run(receipt)

        self.assertEqual(annotations_of(output), [])
        self.assertIn("::warning title=pr-fast receipt::receipt unreadable", output)


class EscapingTests(unittest.TestCase):
    """Nothing validates the receipt before this module reads it.

    ``.ci/receipt.schema.json`` constrains ``gate_name`` to
    ``^[a-z][a-z0-9_-]*$``, so a conformant producer cannot reach these
    branches. They are defence in depth for a receipt that never met the
    schema.
    """

    def test_property_delimiters_are_escaped_in_the_title(self) -> None:
        raw = "weird:gate,name"
        self.assertEqual(
            summarizer.escape_property(raw), "weird%3Agate%2Cname"
        )
        # Message data leaves ':' and ',' alone; only properties delimit on them.
        self.assertEqual(summarizer.escape_data(raw), raw)

    def test_percent_and_newlines_are_escaped_in_both(self) -> None:
        raw = "a%b\rc\nd"
        self.assertEqual(summarizer.escape_data(raw), "a%25b%0Dc%0Ad")
        self.assertEqual(summarizer.escape_property(raw), "a%25b%0Dc%0Ad")

    def test_percent_is_escaped_before_the_others(self) -> None:
        # Escaping '%' last would re-escape the '%' of '%0A' into '%250A'.
        self.assertEqual(summarizer.escape_data("\n"), "%0A")

    def test_a_crafted_gate_name_cannot_inject_a_property(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [
                    {
                        "gate_name": "x::error title=spoof",
                        "status": "fail",
                        "exit_code": 1,
                    }
                ],
            )
            output = run(receipt)

        published = annotations_of(output)
        self.assertEqual(len(published), 1)
        self.assertIn("x%3A%3Aerror title=spoof", published[0])


class StopCommandsFenceTests(unittest.TestCase):
    """The mirror is data, so command parsing is suspended across it."""

    def test_mirror_is_fenced_by_the_token(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [{"gate_name": "clippy", "status": "fail", "exit_code": 101}],
            )
            output = run(receipt, token="FENCE")

        lines = output.splitlines()
        open_at = lines.index("::stop-commands::FENCE")
        close_at = lines.index("::FENCE::")
        self.assertLess(open_at, close_at)
        # The table sits inside the fence; the annotations sit after it.
        body = lines[open_at + 1 : close_at]
        self.assertIn("### PR Smoke gate summary", body)
        self.assertTrue(
            all(not line.startswith("::error ") for line in body),
            msg="annotations must be emitted outside the fence to be parsed",
        )
        self.assertTrue(
            all(index > close_at for index, line in enumerate(lines)
                if line.startswith("::error ")),
        )

    def test_a_directive_inside_the_receipt_stays_inside_the_fence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [
                    {
                        "gate_name": "evil\n::error::injected",
                        "status": "fail",
                        "exit_code": 1,
                    }
                ],
            )
            output = run(receipt, token="FENCE")

        lines = output.splitlines()
        close_at = lines.index("::FENCE::")
        # The crafted newline splits the table row, so the directive lands on a
        # line of its own -- inside the fence, where the runner will not parse it.
        injected = [
            index
            for index, line in enumerate(lines)
            if line.startswith("::error::injected")
        ]
        self.assertEqual(len(injected), 1)
        self.assertLess(injected[0], close_at, "injection escaped the fence")

    def test_token_is_random_per_run(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(directory, [])
            summary = Path(directory) / "summary.md"
            seen = set()
            for _ in range(3):
                stream = io.StringIO()
                stdout, sys.stdout = sys.stdout, stream
                try:
                    summarizer.main(
                        ["--receipt", str(receipt), "--summary", str(summary)]
                    )
                finally:
                    sys.stdout = stdout
                first = stream.getvalue().splitlines()[0]
                self.assertTrue(first.startswith("::stop-commands::pr-fast-summary-"))
                seen.add(first)
            self.assertEqual(len(seen), 3, "a fixed token could be reproduced")


class AnnotationCapTests(unittest.TestCase):
    """GitHub publishes at most ten annotations of a level per check run."""

    @staticmethod
    def _failing(count: int) -> list[tuple[str, str]]:
        return [(f"gate-{i:02d}", f"status fail, exit {i}") for i in range(count)]

    def test_at_the_cap_nothing_is_folded(self) -> None:
        failing = self._failing(10)
        self.assertEqual(summarizer.fold_annotations(failing), failing)

    def test_above_the_cap_folds_into_exactly_ten(self) -> None:
        failing = self._failing(23)
        folded = summarizer.fold_annotations(failing)

        self.assertEqual(len(folded), summarizer.MAX_ANNOTATIONS)
        self.assertEqual(folded[:9], failing[:9])
        title, detail = folded[9]
        self.assertEqual(title, "14 further failing gates")
        # Every gate name still reaches the annotations API.
        for name, _ in failing[9:]:
            self.assertIn(name, detail)

    def test_the_fold_survives_emission(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [
                    {"gate_name": f"gate-{i:02d}", "status": "fail", "exit_code": i}
                    for i in range(12)
                ],
            )
            output = run(receipt)

        published = annotations_of(output)
        self.assertEqual(len(published), summarizer.MAX_ANNOTATIONS)
        joined = "\n".join(published)
        for i in range(12):
            self.assertIn(f"gate-{i:02d}", joined)


class SummaryFileTests(unittest.TestCase):
    def test_main_writes_the_summary_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = write_receipt(
                directory,
                [{"gate_name": "clippy", "status": "fail", "exit_code": 101}],
            )
            summary = Path(directory) / "summary.md"
            stream = io.StringIO()
            stdout, sys.stdout = sys.stdout, stream
            try:
                code = summarizer.main(
                    ["--receipt", str(receipt), "--summary", str(summary)]
                )
            finally:
                sys.stdout = stdout
            written = summary.read_text(encoding="utf-8")

        # Advisory: the reporter never changes the gate verdict.
        self.assertEqual(code, 0)
        self.assertIn("| `clippy` | `fail` | `101` |", written)


if __name__ == "__main__":
    unittest.main()
