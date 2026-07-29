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

    def test_live_reader_normalizes_null_body_to_empty_text(self) -> None:
        completed = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr=""
        )
        with patch.object(claim_digest.subprocess, "run", return_value=completed) as run:
            body = claim_digest._read_live_pr_body("123", "owner/repo")

        self.assertEqual(body, "")
        command = run.call_args.args[0]
        self.assertIn('.body // ""', command)

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
  jq --arg body "$body" '. + [{id: 101, body: $body}]' "$state" > "$tmp"
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
