from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "reviews" / "claim_digest.py"

spec = importlib.util.spec_from_file_location("claim_digest_hidden_comments", MODULE_PATH)
assert spec is not None and spec.loader is not None
claim_digest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(claim_digest)


class HiddenCommentDigestTests(unittest.TestCase):
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

    def test_multiline_hidden_comment_shape_does_not_change_digest(self) -> None:
        one_line = self.body("Adds X\n<!-- one line -->")
        multiline = self.body(
            "Adds X\n<!--\ninternal note\nwith more lines\n-->"
        )
        self.assertEqual(
            claim_digest.claim_digest(one_line)["digest"],
            claim_digest.claim_digest(multiline)["digest"],
        )


if __name__ == "__main__":
    unittest.main()
