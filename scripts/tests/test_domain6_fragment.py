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

# Canonical 16-field authority for disposition-state exclusivity: every
# field a reviewed row must carry and a not_proven row must not.
REVIEWED_ONLY_FIELDS = [
    "release_domains",
    "primary_disposition",
    "reachable_installed_effect",
    "public_claim_refs",
    "release_note_disposition",
    "migration_or_upgrade_refs",
    "api_schema_package_effects",
    "proof_owner_refs",
    "known_limitations",
    "open_pr_relationships",
    "controlling_issues",
    "invalidators",
    "platforms_and_targets",
    "artifact_or_route_effects",
    "editor_manifest_or_protocol_effects",
    "installed_evidence_stage",
]

# Valid representative value per reviewed-only field for injection probes.
REVIEWED_FIELD_VALUES = {
    "release_domains": ["distribution"],
    "primary_disposition": "packaging_install_or_editor",
    "reachable_installed_effect": "bounded",
    "public_claim_refs": ["claim"],
    "release_note_disposition": "required",
    "migration_or_upgrade_refs": ["mig"],
    "api_schema_package_effects": ["eff"],
    "proof_owner_refs": ["#16408"],
    "known_limitations": ["lim"],
    "open_pr_relationships": ["#16419"],
    "controlling_issues": ["#16421"],
    "invalidators": ["inv"],
    "platforms_and_targets": ["windows-x64"],
    "artifact_or_route_effects": ["art"],
    "editor_manifest_or_protocol_effects": ["man"],
    "installed_evidence_stage": "candidate",
}

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
        # Rejecting fixtures: a reviewed row stripped of EACH required
        # reviewed-only field, and a row carrying an unknown property.
        import copy

        reviewed = next(
            u
            for u in self.doc["work_units"]
            if u["disposition_state"] == "reviewed"
        )
        not_proven = next(
            u
            for u in self.doc["work_units"]
            if u["disposition_state"] == "not_proven"
        )
        for key in REVIEWED_ONLY_FIELDS:
            self.assertIn(
                key, reviewed, f"reviewed fixture lacks {key} to strip"
            )
            self.assertNotIn(
                key, not_proven, f"checked artifact already leaks {key}"
            )
            missing = copy.deepcopy(self.doc)
            bad_unit = next(
                u
                for u in missing["work_units"]
                if u["work_unit_id"] == reviewed["work_unit_id"]
            )
            del bad_unit[key]
            self.assertTrue(
                list(validator.iter_errors(missing)),
                f"schema accepts reviewed row without {key}",
            )
            injected = copy.deepcopy(self.doc)
            target = next(
                u
                for u in injected["work_units"]
                if u["work_unit_id"] == not_proven["work_unit_id"]
            )
            target[key] = REVIEWED_FIELD_VALUES[key]
            self.assertTrue(
                list(validator.iter_errors(injected)),
                f"schema accepts not_proven row carrying {key}",
            )
        unknown = copy.deepcopy(self.doc)
        unknown["work_units"][0] = dict(
            unknown["work_units"][0], bogus_field_xyz=1
        )
        self.assertTrue(list(validator.iter_errors(unknown)))

    def test_grouping_identity_rules(self) -> None:
        # Load by file path: direct execution (python
        # scripts/tests/test_domain6_fragment.py) puts scripts/tests first
        # on sys.path, so a scripts.* import only resolves when the
        # repository root is the entry point (python -m unittest ...).
        import importlib.util

        spec = importlib.util.spec_from_file_location(
            "domain6_generator", GENERATOR
        )
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        BODY_REF = generator.BODY_REF
        group_unit_id = generator.group_unit_id
        split_unit_id = generator.split_unit_id

        # Trailing pair keys the PR; lone refs key the issue; bare
        # subjects are NOREF.
        self.assertEqual(
            group_unit_id("fix(release): smoke (#16368) (#16371)"),
            "PR#16371",
        )
        self.assertEqual(
            group_unit_id("feat(vscode): retain candidates (#10083)"),
            "ISS#10083",
        )
        self.assertEqual(
            group_unit_id("perf(vscode): source census from ownership"),
            "NOREF",
        )
        # (#0000) is a placeholder, never an identity: the sentinel
        # splits per commit.
        record = {
            "sha": "e7c992321fdcd39af50b8d6b764366fa58cbc2cf",
            "subject": "release: prepare v0.15.1 hardening (#0000)",
            "body": "",
        }
        provisional = group_unit_id(record["subject"])
        self.assertEqual(provisional, "ISS#0000")
        self.assertEqual(
            split_unit_id(record, provisional), "COMMIT#e7c99232"
        )
        other = dict(
            record,
            sha="fecc9de71eebb87b6a34de12261e6378ab4bb530",
        )
        self.assertEqual(split_unit_id(other, provisional), "COMMIT#fecc9de7")
        self.assertNotEqual(
            split_unit_id(record, provisional),
            split_unit_id(other, provisional),
        )
        # Placeholder trailing a real pair does not steal identity.
        self.assertEqual(
            group_unit_id("sync: cut bdfde90d7 (#1234) (#0000)"), "ISS#1234"
        )
        # Non-placeholder identities pass through untouched.
        self.assertEqual(split_unit_id(record, "PR#16371"), "PR#16371")
        self.assertEqual(split_unit_id(record, "NOREF"), "NOREF")
        # Body mentions are candidates for notes, never regroups.
        self.assertEqual(
            sorted(set(BODY_REF.findall("Merge the bounded #4346 fix"))),
            ["4346"],
        )
        self.assertFalse(
            BODY_REF.search("no references here"),
        )

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

        # --check builds the document before it inspects the output file:
        # without the pinned range, resolve_range() exits 1 before the
        # CRLF branch, so skip cleanly instead of proving nothing.
        for sha in (START_SHA, OBSERVED_HEAD):
            probe = subprocess.run(
                ["git", "cat-file", "-t", sha],
                cwd=REPO_ROOT,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if probe.returncode != 0:
                self.skipTest(f"pinned history range unavailable ({sha})")

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
        # Exact code 2 plus the canonical-byte diagnostic pins the CRLF
        # branch: setup failures, missing history, and FragmentError paths
        # all exit 1, and plain drift carries a different message.
        self.assertEqual(proc.returncode, 2)
        self.assertIn(
            "contains CRLF; canonical bytes are LF-only",
            proc.stderr.decode("utf-8", errors="strict"),
        )

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
