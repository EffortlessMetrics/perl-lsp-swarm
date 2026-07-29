from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "reviews" / "claim_digest.py"
CHECKER = ROOT / "scripts" / "ci" / "check-pr-claim-currentness"

spec = importlib.util.spec_from_file_location("claim_digest", MODULE_PATH)
assert spec is not None and spec.loader is not None
claim_digest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(claim_digest)


class ClaimDigestTests(unittest.TestCase):
    def body(self, claim: str = "Adds X", verification: str = "one") -> str:
        return f"""## Claim
{claim}

## What this establishes
X is available.

## What this does not establish
Y remains unsupported.

## Risk and rollback
Revert the squash.

## Review index
- src/x.rs

## Verification
{verification}
"""

    def legacy_body(
        self, claim: str = "Adds X", verification_checked: bool = False
    ) -> str:
        checked = "x" if verification_checked else " "
        return f"""## Lane
- [x] substrate

## Claim Boundary
{claim}

## Non-goals
- Does not add Y.

## Behavior
- [x] live behavior change

## Changes
- Adds X through the existing owner.

## Verification
- [{checked}] focused proof

## Remaining Work
- Y remains separate.
"""

    def test_non_material_section_does_not_change_digest(self) -> None:
        first = claim_digest.claim_digest(self.body(verification="one"))["digest"]
        second = claim_digest.claim_digest(self.body(verification="two"))["digest"]
        self.assertEqual(first, second)

    def test_material_claim_change_changes_digest(self) -> None:
        first = claim_digest.claim_digest(self.body(claim="Adds X"))["digest"]
        second = claim_digest.claim_digest(self.body(claim="Adds X and Y"))["digest"]
        self.assertNotEqual(first, second)

    def test_legacy_template_verification_checkbox_does_not_change_digest(self) -> None:
        first = claim_digest.claim_digest(
            self.legacy_body(verification_checked=False)
        )["digest"]
        second = claim_digest.claim_digest(
            self.legacy_body(verification_checked=True)
        )["digest"]
        self.assertEqual(first, second)

    def test_legacy_claim_boundary_change_changes_digest(self) -> None:
        first = claim_digest.claim_digest(self.legacy_body(claim="Adds X"))["digest"]
        second = claim_digest.claim_digest(
            self.legacy_body(claim="Adds X and Y")
        )["digest"]
        self.assertNotEqual(first, second)

    def test_currentness_checker_matches_head_and_claim(self) -> None:
        body = self.body()
        digest = claim_digest.claim_digest(body)["digest"]
        marker = {
            "v": 1,
            "kind": "standard",
            "head": "abc123",
            "claim_digest": digest,
            "reviewer": "reviewer",
            "status": "done",
        }
        comments = f"<!-- review-run:v1 {json.dumps(marker)} -->\n"

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            body_path = root / "body.md"
            comments_path = root / "comments.txt"
            body_path.write_text(body, encoding="utf-8")
            comments_path.write_text(comments, encoding="utf-8")

            completed = subprocess.run(
                [
                    sys.executable,
                    str(CHECKER),
                    "--head",
                    "abc123",
                    "--body-file",
                    str(body_path),
                    "--comments-file",
                    str(comments_path),
                    "--json",
                ],
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(json.loads(completed.stdout)["result"], "current")

    def test_currentness_checker_rejects_changed_claim(self) -> None:
        old_body = self.body(claim="Adds X")
        new_body = self.body(claim="Adds X and Y")
        marker = {
            "v": 1,
            "kind": "standard",
            "head": "abc123",
            "claim_digest": claim_digest.claim_digest(old_body)["digest"],
            "reviewer": "reviewer",
            "status": "done",
        }

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            body_path = root / "body.md"
            comments_path = root / "comments.txt"
            body_path.write_text(new_body, encoding="utf-8")
            comments_path.write_text(
                f"<!-- review-run:v1 {json.dumps(marker)} -->\n", encoding="utf-8"
            )

            completed = subprocess.run(
                [
                    sys.executable,
                    str(CHECKER),
                    "--head",
                    "abc123",
                    "--body-file",
                    str(body_path),
                    "--comments-file",
                    str(comments_path),
                ],
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(completed.returncode, 2)
        self.assertIn("NOT_PROVEN", completed.stderr)


if __name__ == "__main__":
    unittest.main()
