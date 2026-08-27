from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CLASSIFIER_PATH = ROOT / "scripts" / "ci" / "classify-narrative-docs-scope.py"
POLICY_PATH = ROOT / "policy" / "ci-narrative-docs-scope.toml"
SPEC = importlib.util.spec_from_file_location("narrative_docs_scope", CLASSIFIER_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("unable to load narrative docs scope classifier")
CLASSIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLASSIFIER)


BASE_SHA = "1" * 40
HEAD_SHA = "2" * 40
REPOSITORY = "EffortlessMetrics/perl-lsp-swarm"
ALLOWED = "docs/project/status/release.md"
POLICY = {
    "schema_version": 1,
    "policy_identity": "ci-narrative-docs-scope:v1",
    "classifier_identity": "classify-narrative-docs-scope.py:v1",
    "allowed_paths": [ALLOWED],
}


def event(paths: list[str], *, event_name: str = "pull_request", fork: bool = False) -> dict:
    return {
        "event_name": event_name,
        "number": 12987,
        "repository": {"full_name": REPOSITORY},
        "pull_request": {
            "changed_files": len(paths),
            "base": {"sha": BASE_SHA},
            "head": {
                "sha": HEAD_SHA,
                "repo": {"full_name": "someone/fork" if fork else REPOSITORY},
            },
        },
    }


def observation(paths: list[str]) -> dict:
    return {
        "number": 12987,
        "base": {"sha": BASE_SHA},
        "head": {"sha": HEAD_SHA},
        "changed_files": len(paths),
    }


def pages(paths: list[str]) -> list[list[dict[str, str]]]:
    return [[{"filename": path, "status": "modified"} for path in paths]]


def classify(paths: list[str], **event_options: object) -> dict:
    subject = event(paths, **event_options)
    observed = observation(paths)
    return CLASSIFIER.classify(subject, copy.deepcopy(POLICY), observed, pages(paths), observed)


class NarrativeDocsScopeTests(unittest.TestCase):
    def test_exact_allowlisted_path_is_scoped_noop_with_bound_identity(self) -> None:
        result = classify([ALLOWED])
        self.assertEqual(result["decision"], "scoped_noop")
        self.assertEqual(result["reason"], "audited_narrative_docs_only")
        self.assertEqual(result["subject"], "pull_request")
        self.assertEqual(result["file_count"], 1)
        self.assertRegex(result["file_digest"], r"^[0-9a-f]{64}$")
        self.assertEqual(
            result["file_digest"],
            "794d5f956c9b3140e585d22c2d57e2d858bf571128598e641b39ab72e17d23ad",
        )
        self.assertEqual(result["base_sha"], BASE_SHA)
        self.assertEqual(result["head_sha"], HEAD_SHA)
        self.assertEqual(result["policy_identity"], POLICY["policy_identity"])
        self.assertEqual(result["classifier_identity"], POLICY["classifier_identity"])
        self.assertRegex(result["policy_digest"], r"^[0-9a-f]{64}$")
        self.assertRegex(result["classifier_digest"], r"^[0-9a-f]{64}$")

    def test_all_required_path_falsifiers_run_full_proof(self) -> None:
        for path in [
            ".github/workflows/example.md",
            ".ci/example.md",
            "scripts/ci/readme.md",
            "hooks/readme.md",
            "policy/example.toml",
            "xtask/readme.md",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "crates/perl-lsp-rs-core/README.md",
            "docs/reference/CONFIG.md",
            "docs/reference/CONFIGURATION.md",
            "docs/reference/CONFIGURATION_SCHEMA.md",
            "docs/reference/PERL_KWALITEE_MIGRATION.md",
            "crates/perllsp/src/main.rs",
            "tests/fixture.rs",
            "unknown/new/path.md",
        ]:
            with self.subTest(path=path):
                self.assertEqual(classify([path])["decision"], "run")

    def test_mixed_allowlisted_and_code_path_runs_full_proof(self) -> None:
        result = classify([ALLOWED, "crates/perllsp/src/main.rs"])
        self.assertEqual((result["decision"], result["file_count"]), ("run", 2))

    def test_non_pr_subjects_run_full_proof_but_complete_fork_evidence_can_noop(self) -> None:
        for event_name in ["merge_group", "workflow_dispatch", "schedule", "push", "unknown"]:
            with self.subTest(event_name=event_name):
                result = classify([ALLOWED], event_name=event_name)
                self.assertEqual(result["decision"], "run")
                self.assertEqual(result["reason"], "non_pull_request_subject")
        fork = classify([ALLOWED], fork=True)
        self.assertEqual(
            (fork["decision"], fork["reason"]),
            ("scoped_noop", "audited_narrative_docs_only"),
        )

    def test_pre_and_post_subject_drift_runs_full_proof(self) -> None:
        subject = event([ALLOWED])
        current = observation([ALLOWED])
        for position in ["before", "after"]:
            for field, replacement in [("base", "3" * 40), ("head", "4" * 40)]:
                with self.subTest(position=position, field=field):
                    stale = copy.deepcopy(current)
                    stale[field]["sha"] = replacement
                    before = stale if position == "before" else current
                    after = stale if position == "after" else current
                    result = CLASSIFIER.classify(subject, POLICY, before, pages([ALLOWED]), after)
                    self.assertEqual(result["decision"], "run")
                    self.assertEqual(result["reason"], f"stale_pr_{position}_observation")

    def test_count_pagination_empty_duplicate_and_malformed_paths_fail_closed(self) -> None:
        subject = event([ALLOWED])
        current = observation([ALLOWED])
        cases = [
            ([], "invalid_files_pages"),
            ([[]], "invalid_files_page"),
            (pages([ALLOWED]) + pages([ALLOWED]), "changed_file_count_mismatch"),
            ([[{"filename": "", "status": "modified"}]], "malformed_changed_path"),
            ([[{"filename": " docs/project/status/release.md", "status": "modified"}]], "malformed_changed_path"),
            ([[{"filename": "/docs/project/status/release.md", "status": "modified"}]], "malformed_changed_path"),
            ([[{"filename": "docs/../docs/project/status/release.md", "status": "modified"}]], "malformed_changed_path"),
            ([[{"filename": "docs\\project\\status\\release.md", "status": "modified"}]], "malformed_changed_path"),
        ]
        for file_pages, reason in cases:
            with self.subTest(reason=reason, file_pages=file_pages):
                result = CLASSIFIER.classify(subject, POLICY, current, file_pages, current)
                self.assertEqual((result["decision"], result["reason"]), ("run", reason))

        two_file_subject = event([ALLOWED, ALLOWED])
        two_file_observation = observation([ALLOWED, ALLOWED])
        duplicate = CLASSIFIER.classify(
            two_file_subject,
            POLICY,
            two_file_observation,
            pages([ALLOWED, ALLOWED]),
            two_file_observation,
        )
        self.assertEqual((duplicate["decision"], duplicate["reason"]), ("run", "duplicate_changed_path"))

        two_paths = [ALLOWED, "crates/perllsp/src/main.rs"]
        two_subject = event(two_paths)
        two_observation = observation(two_paths)
        paginated = CLASSIFIER.classify(
            two_subject,
            POLICY,
            two_observation,
            [pages([two_paths[0]])[0], pages([two_paths[1]])[0]],
            two_observation,
        )
        self.assertEqual((paginated["decision"], paginated["file_count"]), ("run", 2))
        short = CLASSIFIER.classify(
            two_subject,
            POLICY,
            two_observation,
            pages([ALLOWED]),
            two_observation,
        )
        self.assertEqual((short["decision"], short["reason"]), ("run", "changed_file_count_mismatch"))

    def test_rename_previous_filename_and_unknown_status_run_full_proof(self) -> None:
        subject = event([ALLOWED])
        current = observation([ALLOWED])
        for entry in [
            {"filename": ALLOWED, "status": "renamed", "previous_filename": "src/lib.rs"},
            {"filename": ALLOWED, "status": "modified", "previous_filename": "src/lib.rs"},
            {"filename": ALLOWED, "status": "unknown"},
            {"filename": ALLOWED},
        ]:
            with self.subTest(entry=entry):
                result = CLASSIFIER.classify(subject, POLICY, current, [[entry]], current)
                self.assertEqual(
                    (result["decision"], result["reason"]),
                    ("run", "rename_or_unknown_file_status"),
                )

    def test_empty_event_file_set_and_invalid_sha_fail_closed(self) -> None:
        empty = event([])
        current = observation([])
        self.assertEqual(
            CLASSIFIER.classify(empty, POLICY, current, [], current)["reason"],
            "invalid_event_identity",
        )
        malformed = event([ALLOWED])
        malformed["pull_request"]["head"]["sha"] = "not-a-sha"
        self.assertEqual(
            CLASSIFIER.classify(malformed, POLICY, current, pages([ALLOWED]), current)["decision"],
            "run",
        )

    def test_malformed_or_unknown_policy_never_authorizes_noop(self) -> None:
        subject = event([ALLOWED])
        current = observation([ALLOWED])
        policies = [
            {},
            {**POLICY, "unknown": True},
            {**POLICY, "schema_version": 2},
            {**POLICY, "classifier_identity": "other"},
            {**POLICY, "allowed_paths": []},
            {**POLICY, "allowed_paths": [ALLOWED, ALLOWED]},
            {**POLICY, "allowed_paths": [ALLOWED, "docs/other.md"]},
            {**POLICY, "allowed_paths": ["docs/../release.md"]},
        ]
        for policy in policies:
            with self.subTest(policy=policy):
                result = CLASSIFIER.classify(subject, policy, current, pages([ALLOWED]), current)
                self.assertEqual(result["decision"], "run")

    def test_cli_malformed_json_and_toml_emit_typed_run(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            valid_event = root / "event.json"
            valid_observation = root / "observation.json"
            valid_pages = root / "files.json"
            valid_event.write_text(json.dumps(event([ALLOWED])), encoding="utf-8")
            valid_observation.write_text(json.dumps(observation([ALLOWED])), encoding="utf-8")
            valid_pages.write_text(json.dumps(pages([ALLOWED])), encoding="utf-8")
            for label, policy_text, event_text in [
                ("malformed_policy", "schema_version = [", valid_event.read_text(encoding="utf-8")),
                ("malformed_event", POLICY_PATH.read_text(encoding="utf-8"), "{"),
            ]:
                with self.subTest(label=label):
                    policy = root / f"{label}.toml"
                    event_path = root / f"{label}.json"
                    policy.write_text(policy_text, encoding="utf-8")
                    event_path.write_text(event_text, encoding="utf-8")
                    completed = subprocess.run(
                        [
                            sys.executable,
                            str(CLASSIFIER_PATH),
                            "--policy",
                            str(policy),
                            "--event",
                            str(event_path),
                            "--pr-before",
                            str(valid_observation),
                            "--files",
                            str(valid_pages),
                            "--pr-after",
                            str(valid_observation),
                        ],
                        check=True,
                        capture_output=True,
                        text=True,
                    )
                    result = json.loads(completed.stdout)
                    self.assertEqual(result["decision"], "run")

    def test_cli_positive_binds_real_policy_and_classifier_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixtures = {
                "event.json": event([ALLOWED]),
                "observation.json": observation([ALLOWED]),
                "files.json": pages([ALLOWED]),
            }
            for name, value in fixtures.items():
                (root / name).write_text(json.dumps(value), encoding="utf-8")
            completed = subprocess.run(
                [
                    sys.executable,
                    str(CLASSIFIER_PATH),
                    "--policy",
                    str(POLICY_PATH),
                    "--event",
                    str(root / "event.json"),
                    "--pr-before",
                    str(root / "observation.json"),
                    "--files",
                    str(root / "files.json"),
                    "--pr-after",
                    str(root / "observation.json"),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            result = json.loads(completed.stdout)
            self.assertEqual(result["decision"], "scoped_noop")
            self.assertEqual(
                result["policy_digest"], hashlib.sha256(POLICY_PATH.read_bytes()).hexdigest()
            )
            self.assertEqual(
                result["classifier_digest"],
                hashlib.sha256(CLASSIFIER_PATH.read_bytes()).hexdigest(),
            )
            self.assertNotEqual(result["policy_digest"], CLASSIFIER.EMPTY_DIGEST)
            self.assertNotEqual(result["classifier_digest"], CLASSIFIER.EMPTY_DIGEST)


if __name__ == "__main__":
    unittest.main()
