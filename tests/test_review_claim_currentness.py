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
REVIEW_RUNNER = ROOT / "scripts" / "reviews" / "run"

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
    ) -> dict[str, object]:
        return {
            "id": 101,
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
            if emit_json:
                command.append("--json")
            return subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
            )

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

    def test_live_reader_normalizes_null_body_to_empty_text(self) -> None:
        completed = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr=""
        )
        with patch.object(claim_digest.subprocess, "run", return_value=completed) as run:
            body = claim_digest._read_live_pr_body("123", "owner/repo")

        self.assertEqual(body, "")
        command = run.call_args.args[0]
        self.assertIn('.body // ""', command)

    def test_currentness_checker_matches_trusted_head_and_claim(self) -> None:
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
        completed = self.run_checker(
            body,
            [self.receipt_comment(marker)],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        result = json.loads(completed.stdout)
        self.assertEqual(result["result"], "current")
        self.assertEqual(result["trusted_receipts"], 1)
        self.assertEqual(result["untrusted_receipts"], 0)

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
        completed = self.run_checker(new_body, [self.receipt_comment(marker)])

        self.assertEqual(completed.returncode, 1)
        self.assertIn("NOT_PROVEN", completed.stderr)

    def test_currentness_checker_rejects_untrusted_forged_receipt(self) -> None:
        body = self.body()
        marker = {
            "v": 1,
            "kind": "standard",
            "head": "abc123",
            "claim_digest": claim_digest.claim_digest(body)["digest"],
            "reviewer": "forged-reviewer",
            "status": "done",
        }
        completed = self.run_checker(
            body,
            [self.receipt_comment(marker, association="NONE", login="external-user")],
            emit_json=True,
        )

        self.assertEqual(completed.returncode, 1)
        result = json.loads(completed.stdout)
        self.assertEqual(result["matching_receipts"], 0)
        self.assertEqual(result["trusted_receipts"], 0)
        self.assertEqual(result["untrusted_receipts"], 1)

    def test_currentness_checker_is_not_applicable_without_review_receipts(self) -> None:
        completed = self.run_checker(self.body(), [], emit_json=True)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(json.loads(completed.stdout)["result"], "not_applicable")

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
                "--dry-run",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("requires the --claim-digest", completed.stderr)

    def test_review_done_updates_matching_running_receipt_in_place(self) -> None:
        bash = shutil.which("bash")
        jq = shutil.which("jq")
        if bash is None or jq is None:
            self.skipTest("bash and jq are required for the review receipt integration test")

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            state = root / "comments.json"
            state.write_text("[]\n", encoding="utf-8")
            gh = fake_bin / "gh"
            gh.write_text(
                """#!/usr/bin/env bash
set -euo pipefail
state="${FAKE_GH_STATE:?}"
if [[ "$1" == "pr" && "$2" == "comment" ]]; then
  shift 2
  body=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --body) body="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  tmp="${state}.tmp"
  jq --arg body "$body" '. + [{id: 101, body: $body, user: {login: "owner"}, author_association: "OWNER"}]' "$state" > "$tmp"
  mv "$tmp" "$state"
  exit 0
fi
if [[ "$1" == "api" && "$2" == "--paginate" ]]; then
  cat "$state"
  exit 0
fi
if [[ "$1" == "api" && "$2" == "-X" && "$3" == "PATCH" ]]; then
  endpoint="$4"
  shift 4
  body=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --raw-field) body="${2#body=}"; shift 2 ;;
      *) shift ;;
    esac
  done
  id="${endpoint##*/}"
  tmp="${state}.tmp"
  jq --argjson id "$id" --arg body "$body" 'map(if .id == $id then .body = $body else . end)' "$state" > "$tmp"
  mv "$tmp" "$state"
  exit 0
fi
echo "unexpected fake gh invocation: $*" >&2
exit 2
""",
                encoding="utf-8",
            )
            gh.chmod(0o755)

            env = os.environ.copy()
            env["FAKE_GH_STATE"] = str(state)
            env["PATH"] = f"{fake_bin}{os.pathsep}{env.get('PATH', '')}"
            common = [
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
            ]

            started = subprocess.run(
                [bash, str(REVIEW_RUNNER), "review-start", *common],
                env=env,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(started.returncode, 0, started.stderr)
            running = json.loads(state.read_text(encoding="utf-8"))
            self.assertEqual(len(running), 1)
            self.assertIn('"status":"running"', running[0]["body"])

            completed = subprocess.run(
                [bash, str(REVIEW_RUNNER), "review-done", *common],
                env=env,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            done = json.loads(state.read_text(encoding="utf-8"))
            self.assertEqual(len(done), 1, "review-done must update, not append")
            self.assertIn('"status":"done"', done[0]["body"])
            self.assertNotIn('"status":"running"', done[0]["body"])


if __name__ == "__main__":
    unittest.main()
