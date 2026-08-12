#!/usr/bin/env python3
"""Focused falsifiers for check-pr-semantic-review-currentness.py."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-pr-semantic-review-currentness.py")
SPEC = importlib.util.spec_from_file_location("semantic_currentness", SCRIPT)
assert SPEC and SPEC.loader
module = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = module
SPEC.loader.exec_module(module)


def git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def commit(root: Path, message: str) -> str:
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", message)
    return git(root, "rev-parse", "HEAD")


def setup_repo() -> tuple[tempfile.TemporaryDirectory[str], Path, str, str]:
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    git(root, "init", "-q")
    git(root, "config", "user.name", "Test")
    git(root, "config", "user.email", "test@example.invalid")
    (root / "docs").mkdir()
    (root / "docs/route.md").write_text("route = stable\n", encoding="utf-8")
    base = commit(root, "base")
    (root / "docs/route.md").write_text("route = candidate\n", encoding="utf-8")
    reviewed = commit(root, "candidate")
    return tmp, root, base, reviewed


def body(
    pr: int,
    root: Path,
    base: str,
    head: str,
    *,
    digest: str | None = None,
) -> str:
    digest = digest or module.subject_digest(root, base, head)
    marker = {
        "head": head,
        "merge_base": base,
        "pr": pr,
        "result": "REVIEW_CURRENT",
        "subject_sha256": digest,
    }
    encoded = json.dumps(marker, sort_keys=True, separators=(",", ":"))
    return f"""## Review scope
- cumulative candidate

## Evidence and falsifiers
- focused proof

## No material findings

## What this establishes
- claim supported

## Residual risk / not proved
- external state

## Substantive review result
- REVIEW_CURRENT

<!-- semantic-review:v1 {encoded} -->
"""


def review(pr: int, root: Path, base: str, head: str, **kwargs):
    return module.Review(
        login="reviewer",
        user_type=kwargs.get("user_type", "User"),
        state=kwargs.get("state", "COMMENTED"),
        body=body(pr, root, base, head, digest=kwargs.get("digest")),
        commit_oid=head,
        submitted_at=kwargs.get("submitted_at", "2026-08-12T00:00:00Z"),
    )


class SemanticReviewCurrentnessTests(unittest.TestCase):
    def test_exact_subject_bound_review_is_current(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(
            root,
            pr=42,
            current_head=head,
            reviews=[review(42, root, base, head)],
        )
        self.assertEqual("REVIEW_CURRENT", result["classification"])
        self.assertFalse(result["carried_forward"])

    def test_no_substantive_review_is_not_proven(self) -> None:
        tmp, root, _base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(root, pr=42, current_head=head, reviews=[])
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "no_substantive_review_currentness_marker",
            result["reason"],
        )

    def test_generic_human_comment_is_not_substantive(self) -> None:
        tmp, root, _base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        generic = module.Review(
            "reviewer",
            "User",
            "COMMENTED",
            "LGTM",
            head,
            "2026-08-12T00:00:00Z",
        )
        result = module.evaluate(root, pr=42, current_head=head, reviews=[generic])
        self.assertEqual("NOT_PROVEN", result["classification"])

    def test_prose_whitespace_only_followup_carries_review_forward(self) -> None:
        tmp, root, base, reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/route.md").write_text(
            "route    =    candidate\n\n",
            encoding="utf-8",
        )
        current = commit(root, "format prose")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review(42, root, base, reviewed)],
        )
        self.assertEqual("REVIEW_CURRENT", result["classification"])
        self.assertTrue(result["carried_forward"])

    def test_material_route_change_requires_focused_review(self) -> None:
        tmp, root, base, reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/route.md").write_text(
            "route = different-production-path\n",
            encoding="utf-8",
        )
        current = commit(root, "route change")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review(42, root, base, reviewed)],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual("material_content_change_after_review", result["reason"])

    def test_added_path_is_not_neutral(self) -> None:
        tmp, root, base, reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/new.md").write_text("new route\n", encoding="utf-8")
        current = commit(root, "add path")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review(42, root, base, reviewed)],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "path,_file-kind,_or_structural_change_after_review",
            result["reason"],
        )

    def test_code_indentation_change_is_not_neutral(self) -> None:
        tmp, root, base, _reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "src").mkdir()
        (root / "src/logic.py").write_text(
            "if True:\n    value = 1\n",
            encoding="utf-8",
        )
        reviewed = commit(root, "add python")
        review_row = review(42, root, base, reviewed)
        (root / "src/logic.py").write_text(
            "if True:\nvalue = 1\n",
            encoding="utf-8",
        )
        current = commit(root, "indentation change")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review_row],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "post-review_change_is_not_in_a_whitespace-insensitive_prose_file",
            result["reason"],
        )

    def test_wrong_subject_digest_is_not_proven(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(
            root,
            pr=42,
            current_head=head,
            reviews=[review(42, root, base, head, digest="0" * 64)],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual("review_subject_digest_mismatch", result["reason"])

    def test_bot_marker_is_not_substantive(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(
            root,
            pr=42,
            current_head=head,
            reviews=[review(42, root, base, head, user_type="Bot")],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])

    def test_marker_head_must_equal_review_commit(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        other = module.Review(
            "reviewer",
            "User",
            "COMMENTED",
            body(42, root, base, head),
            base,
            "2026-08-12T00:00:00Z",
        )
        result = module.evaluate(root, pr=42, current_head=head, reviews=[other])
        self.assertEqual("NOT_PROVEN", result["classification"])


if __name__ == "__main__":
    unittest.main()
