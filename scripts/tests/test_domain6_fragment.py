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
SCHEMA_PATH = (
    REPO_ROOT
    / "docs"
    / "releases"
    / "domain6-windows-editor-distribution-first-mile.v1.schema.json"
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
        self.assertEqual(counts["merge_units"], 20)
        self.assertEqual(counts["noref_watchlist_rows"], 3)
        self.assertEqual(counts["total_rows"], 705)
        self.assertEqual(counts["unique_commits"], 702)
        self.assertEqual(self.doc["work_unit_count"], 672)
        self.assertEqual(self.doc["reviewed_seed_count"], 5)
        self.assertEqual(self.doc["not_proven_unit_count"], 667)
        self.assertEqual(len(self.doc["work_units"]), 672)

    def test_exactly_once_coverage(self) -> None:
        covered: list[str] = []
        for unit in self.doc["work_units"]:
            covered.extend(unit["commits"])
        self.assertEqual(len(covered), 682)
        self.assertEqual(len(set(covered)), 682)
        merge_commits = [m["commits"][0] for m in self.doc["merge_units"]]
        self.assertEqual(len(merge_commits), 20)
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
        units = self.doc["merge_units"]
        self.assertEqual(len(units), 20)
        ids = sorted(m["work_unit_id"] for m in units)
        self.assertIn("MERGE#fb68dd94", ids)
        self.assertIn("PR#12670", ids)
        created = sorted(i for i in ids if i.startswith("MERGE#"))
        self.assertEqual(len(created), 19)
        self.assertNotIn("ISS#0000", ids)
        self.assertFalse(any(i == "PR#0000" for i in ids))
        for unit in units:
            self.assertEqual(len(unit["commits"]), 1)
            self.assertEqual(unit["disposition_state"], "not_proven")
            self.assertTrue(unit["paths_or_components"])
            self.assertTrue(unit["grouping_evidence"])

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

    def test_schema_validates_checked_artifact_draft_2020_12(self) -> None:
        jsonschema = __import__("importlib").util.find_spec("jsonschema")
        self.assertIsNotNone(
            jsonschema, "jsonschema package is required for schema proof"
        )
        from jsonschema import Draft202012Validator

        schema = json.loads(SCHEMA_PATH.read_bytes().decode("utf-8"))
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        errors = list(validator.iter_errors(self.doc))
        self.assertEqual(errors, [])
        # Rejecting fixtures: reviewed row without its required fields,
        # and a row carrying an unknown property.
        import copy

        reviewed = next(
            u
            for u in self.doc["work_units"]
            if u["disposition_state"] == "reviewed"
        )
        missing = copy.deepcopy(self.doc)
        bad_unit = next(
            u
            for u in missing["work_units"]
            if u["work_unit_id"] == reviewed["work_unit_id"]
        )
        del bad_unit["release_domains"]
        self.assertTrue(list(validator.iter_errors(missing)))
        unknown = copy.deepcopy(self.doc)
        unknown["work_units"][0] = dict(
            unknown["work_units"][0], bogus_field_xyz=1
        )
        self.assertTrue(list(validator.iter_errors(unknown)))

    def test_merge_terminal_arithmetic(self) -> None:
        enum = self.doc["merge_enumeration"]
        self.assertEqual(enum["range_merges_touching_prefilter"], 94)
        self.assertEqual(enum["merge_units_kept"], 2)
        self.assertEqual(enum["related_remerges_recorded"], 1)
        self.assertEqual(enum["content_free_merges_excluded"], 62)
        self.assertEqual(enum["blob_covered_merges_excluded"], 11)
        self.assertEqual(enum["merge_units_created"], 18)
        self.assertEqual(
            enum["merge_units_kept"]
            + enum["related_remerges_recorded"]
            + enum["content_free_merges_excluded"]
            + enum["blob_covered_merges_excluded"]
            + enum["merge_units_created"],
            enum["range_merges_touching_prefilter"],
        )

    def test_excluded_merges_ledger_complete(self) -> None:
        excluded = self.doc["excluded_merges"]
        self.assertEqual(len(excluded), 74)
        reasons: dict[str, int] = {}
        for row in excluded:
            reasons[row["reason"]] = reasons.get(row["reason"], 0) + 1
            self.assertTrue(row["sha"])
            self.assertTrue(row["subject"])
            self.assertTrue(row["detail"])
        self.assertEqual(reasons.get("content-free"), 62)
        self.assertEqual(reasons.get("blob-covered"), 11)
        self.assertEqual(reasons.get("related-record"), 1)
        related = next(
            r for r in excluded if r["reason"] == "related-record"
        )
        self.assertEqual(
            related["sha"], "984ff2c897c930e9cbeea331b65487730bda96bd"
        )
        self.assertIn("3dc8ade1", related["detail"])
        # Every enumerated merge occurs exactly once: unit or excluded.
        merge_commits = {m["commits"][0] for m in self.doc["merge_units"]}
        excluded_shas = {r["sha"] for r in excluded}
        self.assertFalse(merge_commits & excluded_shas)
        self.assertEqual(len(merge_commits) + len(excluded_shas), 94)
        covered = {c for u in self.doc["work_units"] for c in u["commits"]}
        self.assertFalse(merge_commits & covered)
        self.assertFalse(excluded_shas & covered)

    def test_no_placeholder_identity(self) -> None:
        ids = [u["work_unit_id"] for u in self.doc["work_units"]]
        self.assertNotIn("ISS#0000", ids)
        self.assertFalse(any(i == "PR#0000" for i in ids))
        split = sorted(i for i in ids if i.startswith("COMMIT#"))
        self.assertEqual(split, ["COMMIT#e7c99232", "COMMIT#fecc9de7"])
        for unit in self.doc["work_units"]:
            if unit["work_unit_id"] in split:
                self.assertEqual(len(unit["commits"]), 1)
                self.assertIn("placeholder (#0000)", unit["grouping_evidence"])

    def test_noref_body_evidence_emitted(self) -> None:
        unit = self.units["NOREF"]
        self.assertEqual(len(unit["commits"]), 5)
        self.assertIn(
            "6a4e0ea6->#14501,#14549,#14581,#14678,#14680,#14686,#7866",
            unit["grouping_evidence"],
        )
        self.assertIn("ca11589a->#4346", unit["grouping_evidence"])

    def test_checked_artifact_is_lf_only(self) -> None:
        raw = ARTIFACT.read_bytes()
        self.assertNotIn(b"\r", raw)
        self.assertTrue(raw.endswith(b"\n"))

    def test_check_rejects_crlf_mutation(self) -> None:
        import tempfile

        raw = ARTIFACT.read_bytes()
        with tempfile.NamedTemporaryFile(
            suffix=".json", delete=False
        ) as handle:
            handle.write(raw.replace(b"\n", b"\r\n"))
            tmp = handle.name
        try:
            proc = subprocess.run(
                [sys.executable, str(GENERATOR), "--check", "--out", tmp],
                cwd=REPO_ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
        finally:
            Path(tmp).unlink()
        self.assertNotEqual(proc.returncode, 0)

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
