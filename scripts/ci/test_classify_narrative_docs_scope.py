from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shlex
import shutil
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
FILE_DIGEST = "794d5f956c9b3140e585d22c2d57e2d858bf571128598e641b39ab72e17d23ad"
BASH = (
    Path(r"C:\Program Files\Git\bin\bash.exe")
    if Path(r"C:\Program Files\Git\bin\bash.exe").is_file()
    else (Path(shutil.which("bash")) if shutil.which("bash") else None)
)


def event(paths: list[str], *, event_name: str = "pull_request", fork: bool = False) -> dict:
    return {
        "event_name": event_name,
        "number": 12987,
        "repository": {"full_name": REPOSITORY},
        "pull_request": {
            "changed_files": len(paths),
            "base": {"sha": BASE_SHA, "repo": {"full_name": REPOSITORY}},
            "head": {
                "sha": HEAD_SHA,
                "repo": {"full_name": "someone/fork" if fork else REPOSITORY},
            },
        },
    }


def observation(paths: list[str], *, fork: bool = False) -> dict:
    return {
        "number": 12987,
        "base": {"sha": BASE_SHA, "repo": {"full_name": REPOSITORY}},
        "head": {
            "sha": HEAD_SHA,
            "repo": {"full_name": "someone/fork" if fork else REPOSITORY},
        },
        "changed_files": len(paths),
    }


def pages(paths: list[str]) -> list[list[dict[str, str]]]:
    return [[{"filename": path, "status": "modified"} for path in paths]]


def classify(paths: list[str], **event_options: object) -> dict:
    subject = event(paths, **event_options)
    observed = observation(paths, fork=bool(event_options.get("fork", False)))
    return CLASSIFIER.classify(subject, copy.deepcopy(POLICY), observed, pages(paths), observed)


def workflow_run_step(path: Path, name: str) -> str:
    lines = path.read_text(encoding="utf-8").replace("\r\n", "\n").splitlines()
    step_header = f"      - name: {name}"
    start = lines.index(step_header)
    run_line = next(index for index in range(start, len(lines)) if lines[index] == "        run: |")
    body: list[str] = []
    for line in lines[run_line + 1 :]:
        if not line:
            body.append("")
            continue
        if not line.startswith("          "):
            break
        body.append(line[10:])
    if not body:
        raise RuntimeError(f"workflow step {name!r} has no run body")
    return "\n".join(body) + "\n"


def execute_bash(script: str, environment: dict[str, str]) -> subprocess.CompletedProcess[bytes]:
    if BASH is None:
        raise RuntimeError("bash is unavailable")
    exports = "\n".join(
        f"export {key}={shlex.quote(value)}" for key, value in environment.items()
    )
    if "PATH_PREFIX" in environment:
        exports += '\nexport PATH="$PATH_PREFIX:$PATH"'
    return subprocess.run(
        [str(BASH), "-s"], input=f"{exports}\n{script}".encode(), capture_output=True
    )


def output_map(path: Path) -> dict[str, str]:
    return dict(
        line.split("=", 1)
        for line in path.read_text(encoding="utf-8").splitlines()
        if "=" in line
    )


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
            FILE_DIGEST,
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

    def test_event_and_observation_repository_identity_must_be_complete_and_coherent(self) -> None:
        for fork in [False, True]:
            subject = event([ALLOWED], fork=fork)
            current = observation([ALLOWED], fork=fork)
            expected_head_repository = "someone/fork" if fork else REPOSITORY
            self.assertEqual(
                CLASSIFIER.classify(subject, POLICY, current, pages([ALLOWED]), current)["decision"],
                "scoped_noop",
            )

            for side in ["base", "head"]:
                with self.subTest(location="event", fork=fork, side=side):
                    missing = copy.deepcopy(subject)
                    del missing["pull_request"][side]["repo"]
                    result = CLASSIFIER.classify(missing, POLICY, current, pages([ALLOWED]), current)
                    self.assertEqual(result["decision"], "run")

            for position in ["before", "after"]:
                for side, mismatch in [
                    ("base", "other/base"),
                    ("head", "other/head" if expected_head_repository != "other/head" else "third/head"),
                ]:
                    with self.subTest(position=position, fork=fork, side=side):
                        incoherent = copy.deepcopy(current)
                        incoherent[side]["repo"]["full_name"] = mismatch
                        before = incoherent if position == "before" else current
                        after = incoherent if position == "after" else current
                        result = CLASSIFIER.classify(subject, POLICY, before, pages([ALLOWED]), after)
                        self.assertEqual(result["decision"], "run")
                        self.assertEqual(result["reason"], f"stale_pr_{position}_observation")

                with self.subTest(position=position, fork=fork, side="head-missing"):
                    incomplete = copy.deepcopy(current)
                    del incomplete["head"]["repo"]
                    before = incomplete if position == "before" else current
                    after = incomplete if position == "after" else current
                    result = CLASSIFIER.classify(subject, POLICY, before, pages([ALLOWED]), after)
                    self.assertEqual(result["decision"], "run")

                with self.subTest(position=position, fork=fork, field="boolean-changed-files"):
                    malformed_count = copy.deepcopy(current)
                    malformed_count["changed_files"] = True
                    before = malformed_count if position == "before" else current
                    after = malformed_count if position == "after" else current
                    result = CLASSIFIER.classify(subject, POLICY, before, pages([ALLOWED]), after)
                    self.assertEqual(
                        (result["decision"], result["reason"]),
                        ("run", f"stale_pr_{position}_observation"),
                    )

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

    @unittest.skipIf(BASH is None, "bash is required to execute workflow aggregates")
    def test_aggregates_reject_arbitrary_and_sentinel_digests(self) -> None:
        base_environment = {
            "GITHUB_STEP_SUMMARY": "/dev/null",
            "ROUTE_RESULT": "success",
            "ROUTER_TARGET": "scoped_noop",
            "ROUTER_REASON": "audited_narrative_docs_only",
            "ROUTER_ERROR": "false",
            "ROUTER_FALLBACK_ALLOWED": "false",
            "SCOPE_DECISION": "scoped_noop",
            "SCOPE_REASON": "audited_narrative_docs_only",
            "SCOPE_SUBJECT": "pull_request",
            "SCOPE_FILE_COUNT": "1",
            "SCOPE_FILE_DIGEST": FILE_DIGEST,
            "SCOPE_BASE_SHA": BASE_SHA,
            "SCOPE_HEAD_SHA": HEAD_SHA,
            "SCOPE_POLICY_IDENTITY": POLICY["policy_identity"],
            "SCOPE_CLASSIFIER_IDENTITY": POLICY["classifier_identity"],
            "SCOPE_POLICY_DIGEST": "a" * 64,
            "SCOPE_CLASSIFIER_DIGEST": "b" * 64,
            "EXPECTED_BASE_SHA": BASE_SHA,
            "EXPECTED_HEAD_SHA": HEAD_SHA,
            "EXPECTED_CHANGED_FILES": "1",
            "CX53_RESULT": "skipped",
            "CX43_RESULT": "skipped",
            "GITHUB_RESULT": "skipped",
            "FALLBACK_RESULT": "skipped",
        }
        scripts = [
            workflow_run_step(ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml", "Evaluate routed result"),
            workflow_run_step(ROOT / ".github" / "workflows" / "ripr.yml", "Evaluate routed result"),
        ]
        mutations = [
            {"SCOPE_FILE_DIGEST": "f" * 64},
            {"SCOPE_POLICY_DIGEST": "0" * 64},
            {"SCOPE_POLICY_DIGEST": CLASSIFIER.EMPTY_DIGEST},
            {"SCOPE_CLASSIFIER_DIGEST": "0" * 64},
            {"SCOPE_CLASSIFIER_DIGEST": CLASSIFIER.EMPTY_DIGEST},
        ]
        for script in scripts:
            valid = execute_bash(script, base_environment)
            self.assertEqual(valid.returncode, 0, valid.stderr.decode(errors="replace"))
            for mutation in mutations:
                with self.subTest(mutation=mutation):
                    mutated_environment = {**base_environment, **mutation}
                    rejected = execute_bash(script, mutated_environment)
                    self.assertNotEqual(rejected.returncode, 0)

    @unittest.skipIf(BASH is None, "bash is required to execute workflow routers")
    def test_routers_fail_closed_across_bootstrap_api_drift_and_output_failures(self) -> None:
        mock_gh = r"""gh() {
local request="$*"
if [[ "$request" == *"/contents/scripts/ci/classify-narrative-docs-scope.py"* ]]; then
  [ "${MOCK_FAILURE:-}" != "bootstrap" ] || return 1
  cat "$MOCK_CLASSIFIER_RESPONSE"
elif [[ "$request" == *"/contents/policy/ci-narrative-docs-scope.toml"* ]]; then
  [ "${MOCK_FAILURE:-}" != "bootstrap" ] || return 1
  cat "$MOCK_POLICY_RESPONSE"
elif [[ "$request" == *"/files?per_page=100"* ]]; then
  [ "${MOCK_FAILURE:-}" != "files" ] || return 1
  cat "$MOCK_FILES"
elif [[ "$request" == *"/pulls/12987"* ]]; then
  calls=0
  [ ! -f "$MOCK_COUNTER" ] || calls="$(cat "$MOCK_COUNTER")"
  calls=$((calls + 1))
  printf '%s' "$calls" > "$MOCK_COUNTER"
  if [ "$calls" -eq 1 ]; then
    [ "${MOCK_FAILURE:-}" != "before" ] || return 1
    cat "$MOCK_BEFORE"
  else
    [ "${MOCK_FAILURE:-}" != "after" ] || return 1
    cat "$MOCK_AFTER"
  fi
else
  return 64
fi
}
base64() {
  tr -d '\r' | command base64 "$@"
}
"""

        def content_response(contents: bytes) -> dict[str, str]:
            import base64

            return {
                "type": "file",
                "content": base64.b64encode(contents).decode("ascii"),
            }

        scripts = [
            workflow_run_step(
                ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml",
                "Decide target runner",
            ),
            workflow_run_step(
                ROOT / ".github" / "workflows" / "ripr.yml", "Decide target runner"
            ),
        ]
        cases = [
            ("", False, "scoped_noop", "audited_narrative_docs_only"),
            ("bootstrap", False, "run", "trusted_scope_artifacts_unavailable"),
            ("before", False, "run", "pr_before_api_error"),
            ("files", False, "run", "files_api_error"),
            ("after", False, "run", "pr_after_api_error"),
            ("", True, "run", "stale_pr_after_observation"),
            ("malformed-output", False, "run", "malformed_classifier_output"),
        ]

        with tempfile.TemporaryDirectory(dir=ROOT) as temp:
            root = Path(temp)
            event_path = root / "event.json"
            before_path = root / "before.json"
            after_path = root / "after.json"
            files_path = root / "files.json"
            policy_response = root / "policy-response.json"
            classifier_response = root / "classifier-response.json"
            malformed_classifier_response = root / "malformed-classifier-response.json"
            event_path.write_text(json.dumps(event([ALLOWED], fork=True)), encoding="utf-8")
            before_path.write_text(
                json.dumps(observation([ALLOWED], fork=True)), encoding="utf-8"
            )
            files_path.write_text(json.dumps(pages([ALLOWED])), encoding="utf-8")
            policy_response.write_text(
                json.dumps(content_response(POLICY_PATH.read_bytes())), encoding="utf-8"
            )
            classifier_response.write_text(
                json.dumps(content_response(CLASSIFIER_PATH.read_bytes())), encoding="utf-8"
            )
            malformed_classifier_response.write_text(
                json.dumps(content_response(b"print('{}')\n")), encoding="utf-8"
            )

            for script in scripts:
                for failure, drift, expected_decision, expected_reason in cases:
                    with self.subTest(failure=failure, drift=drift):
                        after = observation([ALLOWED], fork=True)
                        if drift:
                            after["head"]["sha"] = "3" * 40
                        after_path.write_text(json.dumps(after), encoding="utf-8")
                        output_path = root / "output.txt"
                        summary_path = root / "summary.txt"
                        counter_path = root / "counter.txt"
                        for path in [output_path, summary_path, counter_path]:
                            path.unlink(missing_ok=True)
                        environment = {
                            "GH_TOKEN": "fixture-token",
                            "RUNNER_TOKEN": "",
                            "ORG": "EffortlessMetrics",
                            "REPOSITORY": REPOSITORY,
                            "EVENT_NAME": "pull_request",
                            "PR_NUMBER": "12987",
                            "EVENT_BASE_SHA": BASE_SHA,
                            "EVENT_HEAD_SHA": HEAD_SHA,
                            "EVENT_CHANGED_FILES": "1",
                            "FORCE_TARGET": "auto",
                            "IS_PULL_REQUEST": "true",
                            "IS_FORK_PR": "true",
                            "PR_AUTHOR_LOGIN": "fixture-user",
                            "PR_AUTHOR_TYPE": "User",
                            "GITHUB_EVENT_PATH": event_path.as_posix(),
                            "GITHUB_OUTPUT": output_path.as_posix(),
                            "GITHUB_STEP_SUMMARY": summary_path.as_posix(),
                            "MOCK_FAILURE": failure if failure != "malformed-output" else "",
                            "MOCK_COUNTER": counter_path.as_posix(),
                            "MOCK_BEFORE": before_path.as_posix(),
                            "MOCK_AFTER": after_path.as_posix(),
                            "MOCK_FILES": files_path.as_posix(),
                            "MOCK_POLICY_RESPONSE": policy_response.as_posix(),
                            "MOCK_CLASSIFIER_RESPONSE": (
                                malformed_classifier_response.as_posix()
                                if failure == "malformed-output"
                                else classifier_response.as_posix()
                            ),
                        }
                        completed = execute_bash(f"{mock_gh}\n{script}", environment)
                        self.assertEqual(
                            completed.returncode,
                            0,
                            completed.stderr.decode(errors="replace"),
                        )
                        outputs = output_map(output_path)
                        diagnostic = completed.stderr.decode(errors="replace")
                        self.assertEqual(outputs["scope_decision"], expected_decision, diagnostic)
                        self.assertEqual(outputs["scope_reason"], expected_reason, diagnostic)
                        self.assertEqual(
                            outputs["target"],
                            "scoped_noop" if expected_decision == "scoped_noop" else "github",
                        )


if __name__ == "__main__":
    unittest.main()
