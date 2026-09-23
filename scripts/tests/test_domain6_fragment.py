"""Coverage and deterministic-render tests for the Domain-6 fragment (#16418).

Static checks need only the checked-in artifact. The deterministic
re-render check shells to scripts/generate_domain6_fragment.py --check and
skips when the pinned history range is unavailable.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ARTIFACT = (
    REPO_ROOT
    / "docs"
    / "releases"
    / "domain6-windows-editor-distribution-first-mile.v1.json"
)
GENERATOR = REPO_ROOT / "scripts" / "generate_domain6_fragment.py"

START_SHA = "f6b7b2c6626fbefbf01c9c9934cac5789186f8b2"
OBSERVED_HEAD = "102974155487bc955e01d5d5222053c4c449136c"

SEED_IDS = ["PR#10198", "PR#12086", "PR#14523", "PR#16207", "PR#16371"]
SEED_FRAGMENTS = {
    "PR#16371": "distribution",
    "PR#16207": "editor",
    "PR#12086": "editor",
    "PR#10198": "editor",
    "PR#14523": "editor",
}
REVIEWED_ONLY_KEYS = [
    "release_domains",
    "primary_disposition",
    "reachable_installed_effect",
    "release_note_disposition",
    "installed_evidence_stage",
]


def load_artifact() -> dict:
    return json.loads(ARTIFACT.read_text(encoding="utf-8"))


class Domain6FragmentTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.doc = load_artifact()
        cls.units = {u["work_unit_id"]: u for u in cls.doc["work_units"]}

    def test_header_pins_exact_range_and_prefilter(self) -> None:
        self.assertEqual(
            self.doc["parent_denominator"]["start_sha"], START_SHA
        )
        self.assertEqual(
            self.doc["parent_denominator"]["observed_head"], OBSERVED_HEAD
        )
        self.assertEqual(
            self.doc["prefilter_paths"],
            [
                "vscode-extension",
                "distribution",
                "clients",
                "crates/perl-install",
                "docs/install",
                "docs/tutorials",
                "book",
                "CONTRIBUTING.md",
                "scripts",
                "Formula",
                "man",
            ],
        )

    def test_record_counts(self) -> None:
        counts = self.doc["record_counts"]
        self.assertEqual(counts["non_merge"], 682)
        self.assertEqual(counts["merge_units"], 2)
        self.assertEqual(counts["noref_watchlist_rows"], 3)
        self.assertEqual(counts["total_rows"], 687)
        self.assertEqual(counts["unique_commits"], 684)
        self.assertEqual(self.doc["work_unit_count"], 671)
        self.assertEqual(self.doc["reviewed_seed_count"], 5)
        self.assertEqual(self.doc["not_proven_unit_count"], 666)
        self.assertEqual(len(self.doc["work_units"]), 671)

    def test_exactly_once_coverage(self) -> None:
        covered: list[str] = []
        for unit in self.doc["work_units"]:
            covered.extend(unit["commits"])
        self.assertEqual(len(covered), 682)
        self.assertEqual(len(set(covered)), 682)
        merge_commits = [m["commits"][0] for m in self.doc["merge_units"]]
        self.assertEqual(len(merge_commits), 2)
        self.assertFalse(set(merge_commits) & set(covered))
        watch_commits = {w["commit"] for w in self.doc["noref_watchlist"]}
        self.assertEqual(len(watch_commits), 3)
        # Watchlist rows are intentional duplicates of mapped commits.
        self.assertTrue(watch_commits <= set(covered))

    def test_five_seed_rows_reviewed(self) -> None:
        reviewed = [
            u
            for u in self.doc["work_units"]
            if u["disposition_state"] == "reviewed"
        ]
        self.assertEqual(
            sorted(u["work_unit_id"] for u in reviewed), sorted(SEED_IDS)
        )
        for unit in reviewed:
            for key in REVIEWED_ONLY_KEYS:
                self.assertIn(key, unit, f"{unit['work_unit_id']} lacks {key}")
            self.assertEqual(
                unit["primary_fragment"], SEED_FRAGMENTS[unit["work_unit_id"]]
            )
            self.assertEqual(unit["primary_fragment_basis"], "seed_review")
            self.assertTrue(unit["paths_or_components"])

    def test_all_other_units_explicit_not_proven(self) -> None:
        for unit in self.doc["work_units"]:
            if unit["work_unit_id"] in SEED_IDS:
                continue
            self.assertEqual(unit["disposition_state"], "not_proven")
            for key in REVIEWED_ONLY_KEYS:
                self.assertNotIn(
                    key,
                    unit,
                    f"{unit['work_unit_id']} carries semantic {key} "
                    "without review",
                )
            self.assertTrue(unit["paths_or_components"])
            self.assertTrue(unit["grouping_evidence"])

    def test_merge_units_named(self) -> None:
        ids = sorted(m["work_unit_id"] for m in self.doc["merge_units"])
        self.assertEqual(ids, ["MERGE#fb68dd94", "PR#12670"])

    def test_no_private_temp_path_references(self) -> None:
        text = ARTIFACT.read_text(encoding="utf-8")
        for marker in ("domain6-fragment.v1.yaml", "AppData", "/tmp/domain6"):
            self.assertNotIn(marker, text)

    def test_digest_valid(self) -> None:
        payload = dict(self.doc)
        digest = payload.pop("coverage")["digest"]
        canonical = json.dumps(
            payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False
        )
        expected = "sha256:" + hashlib.sha256(canonical.encode()).hexdigest()
        self.assertEqual(digest, expected)

    def test_deterministic_rerender_byte_identical(self) -> None:
        for sha in (START_SHA, OBSERVED_HEAD):
            proc = subprocess.run(
                ["git", "cat-file", "-t", sha],
                cwd=REPO_ROOT,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if proc.returncode != 0:
                self.skipTest(f"pinned history range unavailable ({sha})")
        proc = subprocess.run(
            [sys.executable, str(GENERATOR), "--check"],
            cwd=REPO_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        self.assertEqual(
            proc.returncode,
            0,
            f"regeneration is not byte-identical:\n{proc.stdout}\n{proc.stderr}",
        )


if __name__ == "__main__":
    unittest.main()
