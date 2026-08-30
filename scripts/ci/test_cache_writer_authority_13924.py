#!/usr/bin/env python3
"""Focused cache-writer authority contract for issue #13924."""

from __future__ import annotations

import os
import unittest
from pathlib import Path

REPO_ROOT = Path(os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2]))
WORKFLOW = REPO_ROOT / ".github/workflows/ci.yml"
TRUSTED_SAVE = (
    "save-if: ${{ github.ref == 'refs/heads/master' || "
    "github.ref == 'refs/heads/main' }}"
)


class CacheWriterAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        if not WORKFLOW.is_file():
            raise unittest.SkipTest("ci.yml not present in this checkout")
        cls.text = WORKFLOW.read_text(encoding="utf-8")

    def assert_restore_only_cache(
        self, start_marker: str, end_marker: str, key_marker: str
    ) -> None:
        start = self.text.index(start_marker)
        end = self.text.index(end_marker, start)
        segment = self.text[start:end]
        self.assertIn(key_marker, segment)
        key = segment.index(key_marker)
        cache_segment = segment[key : key + 400]
        self.assertIn(
            TRUSTED_SAVE,
            cache_segment,
            f"{start_marker.rstrip(':')} must restore on candidates but save only on canonical branch refs",
        )

    def test_platform_override_cache_is_restore_only_on_candidates(self) -> None:
        self.assert_restore_only_cache(
            "platform-overrides:",
            "windows-platform-smoke:",
            "shared-key: ci-platform-scope-${{ hashFiles('Cargo.lock') }}",
        )

    def test_repository_contract_cache_is_restore_only_on_candidates(self) -> None:
        self.assert_restore_only_cache(
            "repository-contract:",
            "conflict-markers:",
            "shared-key: ci-contract-${{ hashFiles('Cargo.lock') }}",
        )

    def test_public_api_cache_is_restore_only_on_candidates(self) -> None:
        self.assert_restore_only_cache(
            "public-api-pr:",
            "semver-pr:",
            "key: public-api-${{ hashFiles('Cargo.lock') }}",
        )

    def test_semver_cache_is_restore_only_on_candidates(self) -> None:
        self.assert_restore_only_cache(
            "semver-pr:",
            "# ── PR Smoke:",
            "key: semver-${{ hashFiles('Cargo.lock') }}",
        )

    def test_key_identity_is_unchanged(self) -> None:
        for marker in (
            "shared-key: ci-platform-scope-${{ hashFiles('Cargo.lock') }}",
            "shared-key: ci-contract-${{ hashFiles('Cargo.lock') }}",
            "key: public-api-${{ hashFiles('Cargo.lock') }}",
            "key: semver-${{ hashFiles('Cargo.lock') }}",
        ):
            with self.subTest(marker=marker):
                self.assertEqual(1, self.text.count(marker))


if __name__ == "__main__":
    unittest.main()
