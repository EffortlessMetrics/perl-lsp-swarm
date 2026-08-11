from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKER = ROOT / "scripts" / "ci" / "check-pr-claim-currentness"
CLAIM_DIGEST = ROOT / "scripts" / "reviews" / "claim-digest"


class RetiredReviewReceiptCommandTests(unittest.TestCase):
    def test_live_claim_currentness_command_fails_closed(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(CHECKER), "123", "--repo", "owner/repo", "--json"],
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(completed.returncode, 2)
        self.assertIn("RETIRED", completed.stderr)
        self.assertIn("scripts/ci/check-pr-review-convergence", completed.stderr)
        self.assertNotIn("NOT_PROVEN: command failed", completed.stderr)

    def test_claim_digest_cli_fails_closed(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(CLAIM_DIGEST)],
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(completed.returncode, 2)
        self.assertIn("RETIRED", completed.stderr)
        self.assertIn("claim_digest.py", completed.stderr)
        self.assertEqual(completed.stdout, "")

    def test_fixture_parser_remains_readable_but_not_authoritative(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            body = root / "body.md"
            comments = root / "comments.json"
            body.write_text("## Claim\nHistorical fixture only.\n", encoding="utf-8")
            comments.write_text("[]\n", encoding="utf-8")

            completed = subprocess.run(
                [
                    sys.executable,
                    str(CHECKER),
                    "--head",
                    "abc123",
                    "--body-file",
                    str(body),
                    "--comments-file",
                    str(comments),
                    "--json",
                ],
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        result = json.loads(completed.stdout)
        self.assertEqual(result["result"], "not_applicable")
        self.assertEqual(result["authority"], "historical_fixture_only")

    def test_executable_entry_point_cannot_call_live_reader(self) -> None:
        source = CHECKER.read_text(encoding="utf-8")
        main_body = source.split("def main(", maxsplit=1)[1]

        self.assertNotIn("_live_pr(args.pr", main_body)
        self.assertIn("if not fixture_mode:", main_body)
        self.assertIn("return 2", main_body)


if __name__ == "__main__":
    unittest.main()
