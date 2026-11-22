#!/usr/bin/env python3
"""Self-test for the ci-contract receipt envelope check (#15310).

Two halves gate each other, and both are asserted here:

  * `scripts/ci/ci_contract_receipt_status.py` refuses an envelope version it
    does not understand, and
  * `.github/workflows/ci.yml` actually routes the ci-contract receipt through
    that module instead of reading `.status` directly.

Without the second half the module could be correct while the workflow keeps
the silent consumer, so this suite asserts the workflow's own text as well.
The producer constant is asserted too, so a producer bump cannot leave the
consumer pinned to a version the producer no longer writes.
"""

from __future__ import annotations

import importlib.util
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("ci_contract_receipt_status.py")
SPEC = importlib.util.spec_from_file_location("ci_contract_receipt_status", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load the receipt status reader from {SCRIPT}")
reader = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = reader
SPEC.loader.exec_module(reader)

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
PRODUCER = ROOT / "xtask" / "src" / "tasks" / "ci_contract.rs"

# The read the workflow used before this change. It is the silent shape: a
# renamed field yields the empty string, the comparison falls through, and the
# step exits non-zero with no diagnostic naming the envelope mismatch.
SILENT_CONTRACT_READ = "jq -r '.status // empty' target/receipts/ci-contract.json"

CONSUMER_INVOCATION = "scripts/ci/ci_contract_receipt_status.py"


def write_receipt(directory: Path, name: str, payload: object) -> Path:
    path = directory / name
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


def run_reader(receipt: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(receipt)],
        capture_output=True,
        text=True,
        check=False,
    )


class EnvelopeVersionTests(unittest.TestCase):
    """The envelope check itself, exercised as the workflow invokes it."""

    def test_v1_receipt_reports_the_policy_finding_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = write_receipt(
                Path(tmp),
                "ci-contract.json",
                {"schema_version": "ci-contract.v1", "status": "POLICY_FINDING"},
            )
            result = run_reader(receipt)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "POLICY_FINDING")

    def test_v1_receipt_reports_every_other_status_unchanged(self) -> None:
        for status in ("SUCCESS", "NOT_PROVEN", "NOT_APPLICABLE", "STALE"):
            with self.subTest(status=status):
                with tempfile.TemporaryDirectory() as tmp:
                    receipt = write_receipt(
                        Path(tmp),
                        "ci-contract.json",
                        {"schema_version": "ci-contract.v1", "status": status},
                    )
                    result = run_reader(receipt)

                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), status)

    def test_v1_receipt_without_a_status_reads_as_empty(self) -> None:
        """The shell consumer read an absent field as empty; that is preserved."""
        with tempfile.TemporaryDirectory() as tmp:
            receipt = write_receipt(
                Path(tmp),
                "ci-contract.json",
                {"schema_version": "ci-contract.v1"},
            )
            result = run_reader(receipt)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "")

    def test_a_renamed_status_field_is_refused_loudly(self) -> None:
        """The discriminating case: the envelope this consumer cannot read.

        The receipt is a v2 envelope whose status field has been renamed. The
        pre-change consumer read the empty string here and fell through
        silently; this consumer refuses and names the mismatch.
        """
        with tempfile.TemporaryDirectory() as tmp:
            receipt = write_receipt(
                Path(tmp),
                "ci-contract.json",
                {
                    "schema_version": "ci-contract.v2",
                    "contract_outcome": "POLICY_FINDING",
                },
            )
            result = run_reader(receipt)

        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout.strip(), "")
        self.assertIn("schema_version", result.stderr)
        self.assertIn("ci-contract.v2", result.stderr)
        self.assertIn("ci-contract.v1", result.stderr)
        self.assertIn("::error::", result.stderr)

    def test_a_receipt_without_a_schema_version_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = write_receipt(
                Path(tmp), "ci-contract.json", {"status": "POLICY_FINDING"}
            )
            result = run_reader(receipt)

        self.assertEqual(result.returncode, 1)
        self.assertIn("schema_version", result.stderr)
        self.assertIn("::error::", result.stderr)

    def test_a_missing_receipt_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "absent.json"
            result = run_reader(missing)

        self.assertEqual(result.returncode, 1)
        self.assertIn("unreadable", result.stderr)
        self.assertIn("::error::", result.stderr)

    def test_a_malformed_receipt_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "ci-contract.json"
            receipt.write_text("{not json", encoding="utf-8")
            result = run_reader(receipt)

        self.assertEqual(result.returncode, 1)
        self.assertIn("not valid JSON", result.stderr)
        self.assertIn("::error::", result.stderr)

    def test_a_receipt_that_is_not_an_object_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            receipt = Path(tmp) / "ci-contract.json"
            receipt.write_text('["ci-contract.v1"]', encoding="utf-8")
            result = run_reader(receipt)

        self.assertEqual(result.returncode, 1)
        self.assertIn("not a JSON object", result.stderr)
        self.assertIn("::error::", result.stderr)


class ProducerConsumerAgreementTests(unittest.TestCase):
    """The consumer's expected version must be the version the producer writes."""

    def test_expected_version_matches_the_producer_constant(self) -> None:
        source = PRODUCER.read_text(encoding="utf-8")
        match = re.search(
            r'const\s+SCHEMA_VERSION\s*:\s*&str\s*=\s*"([^"]+)"\s*;',
            source,
        )
        self.assertIsNotNone(
            match,
            f"no SCHEMA_VERSION constant found in {PRODUCER}",
        )
        self.assertEqual(match.group(1), reader.EXPECTED_SCHEMA_VERSION)


class WorkflowWiringTests(unittest.TestCase):
    """The workflow must actually use the checked reader."""

    def test_workflow_routes_the_contract_receipt_through_the_reader(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(CONSUMER_INVOCATION, workflow)
        self.assertNotIn(
            SILENT_CONTRACT_READ,
            workflow,
            "ci.yml still reads the ci-contract receipt without checking its "
            "envelope version, so the silent fallback is back",
        )

    def test_workflow_keeps_the_policy_finding_advisory_branch(self) -> None:
        """The envelope check must not have removed the branch it protects."""
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn('"POLICY_FINDING"', workflow)


if __name__ == "__main__":
    unittest.main()
