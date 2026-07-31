from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
CHECKER_PATH = ROOT / "scripts" / "ci" / "check-pr-claim-currentness"
DIGEST_PATH = ROOT / "scripts" / "reviews" / "claim_digest.py"


def load_module(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


checker = load_module("review_claim_currentness_checker", CHECKER_PATH)
claim_digest = load_module("review_claim_digest_fail_closed", DIGEST_PATH)


class ReviewClaimFailClosedTests(unittest.TestCase):
    @staticmethod
    def body(claim: str) -> str:
        return f"""## Claim
{claim}

## What this establishes
X is available.

## What this does not establish
Y remains unsupported.

## Risk and rollback
Revert.

## Review index
- src/x.rs
"""

    def test_truncated_trusted_marker_is_invalid_not_absent(self) -> None:
        records, invalid_trusted, invalid_untrusted = checker._markers(
            [
                {
                    "author": {"login": "owner"},
                    "authorAssociation": "OWNER",
                    "body": '<!-- review-run:v1 {"v":1',
                }
            ]
        )
        self.assertEqual(records, [])
        self.assertEqual(invalid_trusted, 1)
        self.assertEqual(invalid_untrusted, 0)

    def test_live_snapshot_rejects_material_body_movement(self) -> None:
        first = json.dumps({"headRefOid": "abc123", "body": self.body("Adds X")})
        second = json.dumps(
            {"headRefOid": "abc123", "body": self.body("Adds X and Y")}
        )
        with patch.object(checker, "_run", side_effect=[first, "[]", second]):
            with self.assertRaisesRegex(RuntimeError, "head or body changed"):
                checker._live_pr("7", "owner/repo")

    def test_inline_code_html_delimiter_is_visible_material(self) -> None:
        first = claim_digest.claim_digest(self.body("Adds X `<!-- A -->`"))["digest"]
        second = claim_digest.claim_digest(self.body("Adds X `<!-- B -->`"))["digest"]
        self.assertNotEqual(first, second)

    def test_indented_code_html_delimiter_is_visible_material(self) -> None:
        first = claim_digest.claim_digest(self.body("Adds X\n    <!-- A -->"))["digest"]
        second = claim_digest.claim_digest(self.body("Adds X\n    <!-- B -->"))["digest"]
        self.assertNotEqual(first, second)


if __name__ == "__main__":
    unittest.main()
