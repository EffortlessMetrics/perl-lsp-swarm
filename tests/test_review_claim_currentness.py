from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "reviews" / "claim_digest.py"
CHECKER = ROOT / "scripts" / "ci" / "check-pr-claim-currentness"
CONVERGENCE = ROOT / "scripts" / "ci" / "check-pr-review-convergence"
REVIEW_RUNNER = ROOT / "scripts" / "reviews" / "run"
CONVERGENCE_FIXTURES = ROOT / "scripts" / "ci" / "fixtures" / "convergence"

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
        self,
        claim: str = "Adds X",
        verification_checked: bool = False,
        risk_checked: bool = False,
    ) -> str:
        checked = "x" if verification_checked else " "
        risk = "x" if risk_checked else " "
        return f"""## Lane
- [x] substrate

## Claim Boundary
{claim}

## Non-goals
- Does not add Y.

## Behavior
- [x] live behavior change

## Risk Surfaces
- [{risk}] subprocess

## Changes
- Adds X through the existing owner.

## Verification
- [{checked}] focused proof

## Remaining Work
- Y remains separate.
"""

    @staticmethod
    def receipt_comment(
        marker: dict[str, object],
        *,
        association: str = "OWNER",
        login: str = "review-owner",
        comment_id: int = 101,
    ) -> dict[str, object]:
        return {
            "id": comment_id,
            "author": {"login": login},
            "authorAssociation": association,
            "body": (
                "Formal review complete.\n\n"
                f"<!-- review-run:v1 {json.dumps(marker, separators=(',', ':'))} -->"
            ),
        }

    def run_checker(
        self,
        body: str,
        comments: list[dict[str, object]],
        *,
        head: str = "abc123",
        expected_head: str | None = None,
        emit_json: bool = False,
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            body_path = root / "body.md"
            comments_path = root / "comments.json"
            body_path.write_text(body, encoding="utf-8")
            comments_path.write_text(json.dumps(comments), encoding="utf-8")
            command = [
                sys.executable,
                str(CHECKER),
                "--head",
                head,
                "--body-file",
                str(body_path),
                "--comments-file",
                str(comments_path),
            ]
            if expected_head is not None:
                command.extend(["--expected-head", expected_head])
            if emit_json:
                command.append("--json")
            return subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
            )

    def valid_marker(
        self,
        body: str,
        *,
        head: str = "abc123",
        status: str = "done",
    ) -> dict[str, object]:
        return {
            "v": 1,
            "kind": "standard",
            "head": head,
            "claim_digest": claim_digest.claim_digest(body)["digest"],
            "reviewer": "reviewer",
            "status": status,
            "comment_id": "101",
        }

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

    def test_risk_surfaces_change_is_material(self) -> None:
        first = claim_digest.claim_digest(self.legacy_body(risk_checked=False))["digest"]
        second = claim_digest.claim_digest(self.legacy_body(risk_checked=True))["digest"]
        self.assertNotEqual(first, second)

    def test_empty_body_has_no_reviewable_claim(self) -> None:
        with self.assertRaisesRegex(ValueError, "no material claim"):
            claim_digest.claim_digest("")

    def test_heading_inside_fenced_code_does_not_select_material_mode(self) -> None:
        body = """Visible context before the example.

```markdown
## Claim
This is example text, not the PR claim.
```

Visible context after the example.
"""
        canonical, mode = claim_digest.canonical_material_claim(body)
        self.assertEqual(mode, "full_body_fallback")
        self.assertIn("Visible context before", canonical)
        self.assertIn("Visible context after", canonical)

    def test_heading_inside_html_comment_does_not_select_material_mode(self) -> None:
        body = """Visible context before the comment.

<!--
## Claim
Hidden migration note.
-->

Visible context after the comment.
"""
        canonical, mode = claim_digest.canonical_material_claim(body)
        self.assertEqual(mode, "full_body_fallback")
        self.assertIn("Visible context before", canonical)
        self.assertIn("Visible context after", canonical)

    def test_hidden_comment_inside_material_section_does_not_change_digest(self) -> None:
        first = self.body(claim="Adds X <!-- internal note A -->")
        second = self.body(claim="Adds X <!-- internal note B -->")
        self.assertEqual(
            claim_digest.claim_digest(first)["digest"],
            claim_digest.claim_digest(second)["digest"],
        )

    def test_hidden_only_material_section_is_not_reviewable(self) -> None:
        with self.assertRaisesRegex(ValueError, "no material claim content"):
            claim_digest.claim_digest("## Claim\n<!-- template instruction only -->\n")

    def test_hidden_comment_in_full_body_fallback_does_not_change_digest(self) -> None:
        first = "Visible legacy claim.\n<!-- internal note A -->\n"
        second = "Visible legacy claim.\n<!-- internal note B -->\n"
        self.assertEqual(
            claim_digest.claim_digest(first)["digest"],
            claim_digest.claim_digest(second)["digest"],
        )

    def test_html_comment_inside_fenced_code_remains_material(self) -> None:
        first = self.body(claim="Adds X\n```html\n<!-- A -->\n```")
        second = self.body(claim="Adds X\n```html\n<!-- B -->\n```")
        self.assertNotEqual(
            claim_digest.claim_digest(first)["digest"],
            claim_digest.claim_digest(second)["digest"],
        )

    def test_currentness_checker_matches_trusted_head_and_claim(self) -> None:
        body = self.body()
        completed = self.run_checker(
            body,
            [self.receipt_comment(self.valid_marker(body))],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 3, completed.stderr)
        result = json.loads(completed.stdout)
        self.assertEqual(result["result"], "historical_match")
        self.assertEqual(result["trusted_receipts"], 1)
        self.assertEqual(result["untrusted_receipts"], 0)
        self.assertEqual(result["invalid_receipts"], 0)

    def test_currentness_checker_rejects_changed_claim(self) -> None:
        old_body = self.body(claim="Adds X")
        new_body = self.body(claim="Adds X and Y")
        completed = self.run_checker(
            new_body,
            [self.receipt_comment(self.valid_marker(old_body))],
        )

        self.assertEqual(completed.returncode, 1)
        self.assertIn("NOT_PROVEN", completed.stderr)

    def test_currentness_checker_rejects_untrusted_forged_receipt(self) -> None:
        body = self.body()
        completed = self.run_checker(
            body,
            [
                self.receipt_comment(
                    self.valid_marker(body),
                    association="NONE",
                    login="external-user",
                )
            ],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 1)
        result = json.loads(completed.stdout)
        self.assertEqual(result["matching_receipts"], 0)
        self.assertEqual(result["trusted_receipts"], 0)
        self.assertEqual(result["untrusted_receipts"], 1)

    def test_currentness_checker_rejects_copied_receipt_marker(self) -> None:
        body = self.body()
        marker = self.valid_marker(body)
        copied = self.receipt_comment(marker, comment_id=202)
        completed = self.run_checker(
            body,
            [copied],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 1)
        result = json.loads(completed.stdout)
        self.assertEqual(result["matching_receipts"], 0)
        self.assertEqual(result["trusted_receipts"], 1)

    def test_currentness_checker_rejects_malformed_trusted_receipt(self) -> None:
        body = self.body()
        marker = self.valid_marker(body)
        marker.pop("reviewer")
        completed = self.run_checker(
            body,
            [self.receipt_comment(marker)],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 1)
        result = json.loads(completed.stdout)
        self.assertEqual(result["result"], "not_proven")
        self.assertEqual(result["invalid_receipts"], 1)
        self.assertEqual(result["matching_receipts"], 0)

    def test_legacy_receipt_without_claim_digest_is_valid_but_not_current(self) -> None:
        body = self.body()
        marker = self.valid_marker(body)
        marker.pop("claim_digest")
        completed = self.run_checker(
            body,
            [self.receipt_comment(marker)],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 1)
        result = json.loads(completed.stdout)
        self.assertEqual(result["invalid_receipts"], 0)
        self.assertEqual(result["trusted_receipts"], 1)
        self.assertEqual(result["matching_receipts"], 0)

    def test_currentness_checker_is_not_applicable_without_review_receipts(self) -> None:
        completed = self.run_checker(self.body(), [], emit_json=True)
        self.assertEqual(completed.returncode, 3, completed.stderr)
        self.assertEqual(
            json.loads(completed.stdout)["result"], "historical_not_applicable"
        )

    def test_currentness_checker_rejects_moved_candidate_snapshot(self) -> None:
        completed = self.run_checker(
            self.body(),
            [],
            head="new-head",
            expected_head="old-head",
            emit_json=True,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("candidate moved", completed.stderr)

    def test_blocked_composite_still_emits_material_claim_fields(self) -> None:
        bash = shutil.which("bash")
        jq = shutil.which("jq")
        fixture = CONVERGENCE_FIXTURES / "outdated-unresolved-blocks"
        if bash is None or jq is None or not fixture.is_dir():
            self.skipTest("bash, jq, and convergence fixtures are required")

        env = os.environ.copy()
        env["CONVERGENCE_TEST_FIXTURE_DIR"] = str(fixture)
        completed = subprocess.run(
            [bash, str(CONVERGENCE), "9999", "test-owner/test-repo"],
            env=env,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 1, completed.stderr)
        json_text = completed.stdout[completed.stdout.find("{") :]
        result = json.loads(json_text)
        self.assertFalse(result["converged"])
        self.assertEqual(result["material_claim_review"], "not_applicable")
        self.assertTrue(result["material_claim_head_matches_candidate"])

    def test_review_done_requires_starting_claim_digest(self) -> None:
        bash = shutil.which("bash")
        jq = shutil.which("jq")
        if bash is None or jq is None:
            self.skipTest("bash and jq are required for the review receipt test")

        completed = subprocess.run(
            [
                bash,
                str(REVIEW_RUNNER),
                "review-done",
                "--pr",
                "7",
                "--kind",
                "standard",
                "--reviewer",
                "reviewer",
                "--repo",
                "owner/repo",
                "--head",
                "abc123",
                "--comment-id",
                "101",
                "--dry-run",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("requires the --claim-digest", completed.stderr)

    def test_review_done_requires_starting_comment_id(self) -> None:
        bash = shutil.which("bash")
        jq = shutil.which("jq")
        if bash is None or jq is None:
            self.skipTest("bash and jq are required for the review receipt test")

        completed = subprocess.run(
            [
                bash,
                str(REVIEW_RUNNER),
                "review-done",
                "--pr",
                "7",
                "--kind",
                "standard",
                "--reviewer",
                "reviewer",
                "--repo",
                "owner/repo",
                "--head",
                "abc123",
                "--claim-digest",
                "claim123",
                "--dry-run",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("requires the --comment-id", completed.stderr)

    def test_review_lifecycle_writer_fails_closed(self) -> None:
        bash = shutil.which("bash")
        if bash is None:
            self.skipTest("bash is required for the retired review command test")

        completed = subprocess.run(
            [bash, str(REVIEW_RUNNER), "review-done", "--dry-run"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 2, completed.stderr)
        self.assertIn("RETIRED", completed.stderr)


if __name__ == "__main__":
    unittest.main()
