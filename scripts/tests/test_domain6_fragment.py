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

SEED_IDS = [
    "PR#10198",
    "PR#12086",
    "PR#12742",
    "PR#12808",
    "PR#14523",
    "PR#15103",
    "PR#15221",
    "PR#15260",
    "PR#15443",
    "PR#15447",
    "PR#15449",
    "PR#15450",
    "PR#15456",
    "PR#15726",
    "PR#15911",
    "PR#16207",
    "PR#16371",
    "PR#4788",
    "PR#5477",
    "PR#5481",
    "PR#6180",
    "PR#6186",
    "PR#6233",
    "PR#6371",
    "PR#7897",
    "PR#8899",
]
SEED_FRAGMENTS = {
    "PR#16371": "distribution",
    "PR#16207": "editor",
    "PR#12086": "editor",
    "PR#10198": "editor",
    "PR#14523": "editor",
    "PR#15443": "editor",
    "PR#15450": "editor",
    "PR#15103": "first_mile",
    "PR#12742": "distribution",
    "PR#12808": "editor",
    "PR#15260": "distribution",
    "PR#15447": "distribution",
    "PR#15449": "distribution",
    "PR#15726": "distribution",
    "PR#15221": "editor",
    "PR#15456": "editor",
    "PR#15911": "distribution",
    "PR#6180": "distribution",
    "PR#6186": "distribution",
    "PR#7897": "distribution",
    "PR#4788": "distribution",
    "PR#6371": "distribution",
    "PR#8899": "distribution",
    "PR#5481": "distribution",
    "PR#6233": "distribution",
    "PR#5477": "distribution",
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


def raw_history_fixture(commit: str) -> str:
    return (
        f"commit {commit}\0\n:100644 100644 {'a' * 40} {'b' * 40} M\0path\0"
    )


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

    def test_range_holder_cache_is_bounded_and_exactly_scoped(self) -> None:
        import importlib.util

        spec = importlib.util.spec_from_file_location("domain6_cache_generator", GENERATOR)
        self.assertIsNotNone(spec)
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        calls = []

        def query(*args, cwd):
            calls.append((args, cwd))
            return raw_history_fixture(f"{len(calls):040x}")

        root = REPO_ROOT.resolve()
        cache = generator.RangeHolderCache(root, "start-a", "head-a", query, 3)
        self.assertEqual(cache.holders("a" * 40, "path-a"), (f"{1:040x}",))
        self.assertEqual(cache.holders("a" * 40, "path-a"), (f"{1:040x}",))
        self.assertEqual(cache.holders("b" * 40, "path-a"), (f"{1:040x}",))
        self.assertEqual(cache.holders("c" * 40, "path-a"), ())
        self.assertEqual(len(calls), 1)
        self.assertEqual(cache.holders("a" * 40, "path-b"), (f"{2:040x}",))
        cache.start_sha = "start-b"
        self.assertEqual(cache.holders("a" * 40, "path-a"), (f"{3:040x}",))
        cache.start_sha = "start-a"
        cache.observed_head = "head-b"
        self.assertEqual(cache.holders("a" * 40, "path-a"), (f"{4:040x}",))
        cache.observed_head = "head-a"
        cache.repo_root = root.parent
        self.assertEqual(cache.holders("a" * 40, "path-a"), (f"{5:040x}",))
        cache.repo_root = root

        def alternate_query(*args, cwd):
            calls.append((args, cwd))
            return raw_history_fixture("f" * 40)

        cache.query = alternate_query
        self.assertEqual(cache.holders("a" * 40, "path-a"), ("f" * 40,))
        cache.query = query
        # The original entry was evicted after exceeding the three-entry cap.
        self.assertEqual(cache.holders("a" * 40, "path-a"), (f"{7:040x}",))
        fresh_render = generator.RangeHolderCache(root, "start-a", "head-a", query)
        self.assertEqual(fresh_render.holders("a" * 40, "path-a"), (f"{8:040x}",))
        self.assertLessEqual(len(cache._values), 3)
        self.assertEqual(calls[0][1], root)

    def test_range_holder_cache_retains_query_until_eviction(self) -> None:
        import gc
        import importlib.util
        import weakref

        spec = importlib.util.spec_from_file_location("domain6_query_lifetime", GENERATOR)
        self.assertIsNotNone(spec)
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)

        class Query:
            __hash__ = None

            def __init__(self, result):
                self.result = result

            def __call__(self, *args, cwd):
                return self.result

        first = Query(raw_history_fixture("1" * 40))
        first_ref = weakref.ref(first)
        cache = generator.RangeHolderCache(REPO_ROOT, "start", "head", first, 1)
        self.assertEqual(cache.holders("a" * 40, "path"), ("1" * 40,))
        cache.query = Query(raw_history_fixture("2" * 40))
        del first
        gc.collect()
        self.assertIsNotNone(first_ref())
        self.assertEqual(cache.holders("a" * 40, "path"), ("2" * 40,))
        gc.collect()
        self.assertIsNone(first_ref())
        self.assertEqual(len(cache._values), 1)

    def test_record_counts(self) -> None:
        counts = self.doc["record_counts"]
        self.assertEqual(counts["non_merge"], 682)
        self.assertEqual(counts["merge_units"], 20)
        self.assertEqual(counts["noref_watchlist_rows"], 3)
        self.assertEqual(counts["total_rows"], 705)
        self.assertEqual(counts["unique_commits"], 702)
        self.assertEqual(self.doc["work_unit_count"], 672)
        self.assertEqual(self.doc["reviewed_seed_count"], 26)
        self.assertEqual(self.doc["not_proven_unit_count"], 646)
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

    def test_twenty_six_seed_rows_reviewed(self) -> None:
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


class Domain6GitQueryTest(unittest.TestCase):
    """Real Git remains the oracle; no historical fixture artifacts are trusted."""

    def setUp(self) -> None:
        import importlib.util
        import os
        import tempfile

        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        # Foreign-repository fixtures must ignore calling-hook selectors,
        # signing, hooks, templates and newline policies from the host.
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        empty = self.root / "empty"
        empty.mkdir()
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(empty / "config"),
                        GIT_CONFIG_SYSTEM=str(empty / "config"),
                        GIT_AUTHOR_NAME="Domain6 fixture", GIT_COMMITTER_NAME="Domain6 fixture",
                        GIT_AUTHOR_EMAIL="fixture@example.invalid",
                        GIT_COMMITTER_EMAIL="fixture@example.invalid")
        self.query("init", "-q", "--initial-branch=main", "--object-format=sha1",
                   f"--template={empty}")
        self.query("config", "core.hooksPath", str(empty))
        self.query("config", "commit.gpgsign", "false")
        self.query("config", "core.autocrlf", "false")
        spec = importlib.util.spec_from_file_location("domain6_git_fixture", GENERATOR)
        self.generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.generator)

    def query(self, *args, cwd=None) -> str:
        proc = subprocess.run(["git", *args], cwd=cwd or self.root, env=self.env,
                              capture_output=True, text=True, check=False)
        if proc.returncode:
            error = self.generator.FragmentError if hasattr(self, "generator") else RuntimeError
            raise error(proc.stderr)
        return proc.stdout

    def write(self, path: str, value: str) -> None:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(value, encoding="utf-8")

    def commit(self, subject: str) -> str:
        self.query("add", "-A")
        self.query("commit", "-qm", subject)
        return self.query("rev-parse", "HEAD").strip()

    def test_path_history_matches_find_object_and_shares_reads(self) -> None:
        self.write("file", "base\n")
        start = self.commit("base")
        self.write("file", "A\n")
        self.write("dir/one", "X\n")
        self.write("dir/two", "Y\n")
        self.commit("A and pair")
        blob_a = self.query("rev-parse", "HEAD:file").strip()
        blob_x = self.query("rev-parse", "HEAD:dir/one").strip()
        tree = self.query("rev-parse", "HEAD:dir").strip()
        self.write("file", "B\n")
        self.commit("B")
        self.write("file", "A\n")
        self.write("dir/one", "Y\n")
        self.write("dir/two", "X\n")
        swap = self.commit("revert and swap")
        self.query("mv", "file", "renamed")
        rename = self.commit("unchanged-content rename")
        self.query("update-index", "--chmod=+x", "renamed")
        self.query("commit", "-qm", "mode only")
        mode = self.query("rev-parse", "HEAD").strip()
        (self.root / "renamed").unlink()
        self.commit("delete")
        self.query("checkout", "-qb", "side")
        self.write("dir/one", "side\n")
        self.commit("side")
        self.query("checkout", "-q", "main")
        self.write("other", "main\n")
        self.commit("main")
        self.query("merge", "--no-ff", "-qm", "adopt side", "side")
        self.query("checkout", "-qb", "discarded")
        self.write("dir/one", "transient\n")
        transient_commit = self.commit("temporary side content")
        transient = self.query("rev-parse", "HEAD:dir/one").strip()
        self.write("dir/one", "side\n")
        self.commit("revert side before merging")
        self.query("checkout", "-q", "main")
        self.query("merge", "--no-ff", "-qm", "merge treesame side", "discarded")
        head = self.query("rev-parse", "HEAD").strip()
        calls = []

        def counted(*args, cwd):
            calls.append(args)
            return self.query(*args, cwd=cwd)

        cache = self.generator.RangeHolderCache(self.root, start, head, counted)
        for path in ("file", "renamed", "dir", "*"):
            for oid in (blob_a, blob_x, tree, transient, "f" * 40):
                with self.subTest(path=path, oid=oid):
                    oracle = self.query("log", "--format=%H", f"--find-object={oid}",
                                        f"{start}..{head}", "--", path)
                    self.assertEqual(cache.holders(oid, path), tuple(sorted(oracle.splitlines())))
        self.assertEqual(len(calls), 4, "one history read per path, shared by all object queries")
        self.assertIn(swap, cache.holders(blob_x, "dir"))
        self.assertIn(rename, cache.holders(blob_a, "*"))
        self.assertIn(mode, cache.holders(blob_a, "renamed"))
        self.assertIn(transient_commit, cache.holders(transient, "dir"))

    def test_tree_reads_keep_exact_paths_absence_and_errors(self) -> None:
        names = ["plain", "nested/leaf", "tab\tname", "line\nname", "special[a]"]
        # Newline/tab paths are valid Git paths on POSIX; Windows cannot create them.
        import os
        if os.name == "nt":
            names = ["plain", "nested/leaf", "special[a]"]
        for name in names:
            self.write(name, name)
        names.append("nested")
        head = self.commit("unusual paths")
        self.generator.git = self.query
        expected = {name: self.query("rev-parse", f"{head}:{name}").strip() for name in names}
        expected["absent"] = None
        self.assertEqual(self.generator.tree_blobs(head, names + ["absent"]), expected)
        with self.assertRaises(self.generator.FragmentError):
            self.generator.tree_blobs("f" * 40, ["plain"])
        self.generator.git = lambda *args: "100644 blob invalid\tplain\0"
        with self.assertRaisesRegex(self.generator.FragmentError, "malformed git ls-tree object"):
            self.generator.tree_blobs(head, ["plain"])

    def test_tree_reads_bound_native_many_and_long_path_arguments(self) -> None:
        import os
        from unittest.mock import patch

        self.write("present", "content")
        head = self.commit("argument transport")
        oid = self.query("rev-parse", "HEAD:present").strip()
        original = self.generator.git
        calls = []

        def counted(*args):
            calls.append(args)
            with patch.dict(os.environ, self.env, clear=True):
                return original(*args, cwd=self.root)

        self.generator.git = counted
        populations = [
            ["scripts/" + "x" * 110 + f"{i:04}.py" for i in range(400)],
            ["nested/" + "y" * 700 + f"{i:04}.py" for i in range(64)],
            ["unicode/" + "\U0001f680" * 180 + ' space"' + f"{i:04}" for i in range(100)],
        ]
        for absent in populations:
            with self.subTest(count=len(absent), length=len(absent[0])):
                paths = ["present", *absent, "present"]
                calls.clear()
                self.assertEqual(self.generator.tree_blobs(head, paths),
                                 {**dict.fromkeys(absent), "present": oid})
                self.assertGreater(len(calls), 1)
                self.assertLess(len(calls), len(paths) // 2, "retain batched reads")
                self.assertEqual([path for call in calls for path in call[6:]], paths)

        # Exercise the inclusive boundary through native Git, not only a mock.
        prefix = ["git", "ls-tree", "-r", "-t", "-z", head, "--"]
        fixed = len(subprocess.list2cmdline(prefix).encode("utf-16-le")) // 2 + 1
        exact = "z" * (30000 - fixed - 1)
        calls.clear()
        self.assertEqual(self.generator.tree_blobs(head, [exact]), {exact: None})
        self.assertEqual(len(calls), 1)

        # Hooks select their repository through the environment even with cwd
        # set. The native route must use the same isolated env as fixture setup.
        caller_env = dict(os.environ)
        poison = {"GIT_DIR": str(self.root / "absent.git"),
                  "GIT_WORK_TREE": str(self.root / "absent-worktree"),
                  "GIT_INDEX_FILE": str(self.root / "absent-index")}
        with patch.dict(os.environ, poison):
            with self.assertRaises(self.generator.FragmentError):
                original("ls-tree", head, "--", "present", cwd=self.root)
            paths = ["present", *populations[0]]
            self.assertEqual(self.generator.tree_blobs(head, paths),
                             {**dict.fromkeys(populations[0]), "present": oid})
            self.assertTrue(all(os.environ[key] == value for key, value in poison.items()))
        self.assertEqual(dict(os.environ), caller_env)

    def test_tree_read_batches_count_quoted_utf16_and_exact_limit(self) -> None:
        head = "a" * 40
        # CreateProcessW has a 32767-unit ceiling; the 30000-unit budget leaves launcher headroom.
        prefix = ["git", "ls-tree", "-r", "-t", "-z", head, "--"]
        fixed = len(subprocess.list2cmdline(prefix).encode("utf-16-le")) // 2 + 1
        exact = "x" * (30000 - fixed - 1)
        quoted = ('\U0001f680 "' + "\\" * 3) * 2100
        paths = [exact, quoted, quoted + "suffix", "tail", "tail"]
        calls = []
        self.generator.git = lambda *args: calls.append(args) or ""
        self.assertEqual(self.generator.tree_blobs(head, paths), dict.fromkeys(paths))
        sizes = [len(subprocess.list2cmdline(["git", *call]).encode("utf-16-le")) // 2 + 1
                 for call in calls]
        self.assertEqual(sizes[0], 30000)
        self.assertTrue(all(size <= 30000 for size in sizes), sizes)
        self.assertEqual([path for call in calls for path in call[6:]], paths)
        calls.clear()
        with self.assertRaisesRegex(self.generator.FragmentError, "argument.*limit"):
            self.generator.tree_blobs(head, [exact + "x"])
        self.assertEqual(calls, [], "oversized individual arguments fail before spawning")
        self.assertEqual(self.generator.tree_blobs(head, []), {})
        self.assertEqual(calls, [])

    def test_tree_read_later_batch_errors_do_not_become_absence(self) -> None:
        paths = ["x" * 20000, "y" * 20000]
        for failure in (self.generator.FragmentError("later batch failed"),
                        "malformed\0", "100644 blob invalid\tpath\0"):
            calls = []

            def query(*args):
                calls.append(args)
                if len(calls) == 1:
                    return "100644 blob " + "a" * 40 + "\t" + paths[0] + "\0"
                if isinstance(failure, Exception):
                    raise failure
                return failure

            with self.subTest(failure=str(failure)):
                self.generator.git = query
                with self.assertRaises(self.generator.FragmentError):
                    self.generator.tree_blobs("a" * 40, paths)
                self.assertEqual(len(calls), 2)

    def test_history_errors_and_malformed_output_are_not_cached_as_absence(self) -> None:
        def failed(*args, cwd):
            raise self.generator.FragmentError("unavailable history")

        cache = self.generator.RangeHolderCache(self.root, "start", "head", failed)
        with self.assertRaisesRegex(self.generator.FragmentError, "unavailable history"):
            cache.holders("a" * 40, "path")
        self.assertEqual(len(cache._values), 0)
        for malformed in ("not raw history\0", "commit " + "1" * 40 + "\0\n:bad\0"):
            with self.subTest(malformed=malformed):
                cache.query = lambda *args, cwd: malformed
                with self.assertRaises(self.generator.FragmentError):
                    cache.holders("a" * 40, "path")
                self.assertEqual(len(cache._values), 0)


if __name__ == "__main__":
    unittest.main()
