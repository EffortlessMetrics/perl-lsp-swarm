#!/usr/bin/env python3
"""Compiler-free queue-entry falsifiers for the existing workflow catalog.

Fixtures exercise committed Git source and the real CLI. They do not simulate
GitHub queue admission, authenticate group members, or prove live enforcement.
"""
from __future__ import annotations

from contextlib import contextmanager
from copy import deepcopy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("validate_gate_enforcement_contract.py")
SPEC = importlib.util.spec_from_file_location("queue_gate_contract", SCRIPT)
assert SPEC and SPEC.loader
contract = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = contract
SPEC.loader.exec_module(contract)

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ".github/workflows/ci.yml"
POLICY = ".ci/policies/required-checks.toml"
OWN_WORKFLOW = ".github/workflows/gate-enforcement-contract.yml"
QUEUE_ON = "on:\n  merge_group:\n    types: [checks_requested]\n"
JOBS = """jobs:
  compile:
    name: Compile All Targets
    runs-on: ubuntu-latest
    steps: []
"""
CHECK = """version = 2
source = "github-enforcement-union"
[[checks]]
name = "Compile All Targets"
producer = "repository-job"
workflow = ".github/workflows/ci.yml"
job = "compile"
workflow_result = "propagate"
events = ["merge_group"]
required = true
policy_role = "required"
applicability = "always-or-scoped-noop"
enforcement = "github-ruleset"
ruleset_integration_id = 15368
"""


def git(root: Path, *args: str) -> str:
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    # Fixture commits never depend on the contributor's hooks, signing, or identity.
    return subprocess.run(
        ["git", "-C", str(root), "-c", "core.hooksPath=", "-c", "commit.gpgsign=false",
         "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", *args],
        env=env, check=True, capture_output=True, text=True,
    ).stdout.strip()


def write(root: Path, path: str, content: str) -> None:
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")


@contextmanager
def repository(on: str = QUEUE_ON, *, jobs: str = JOBS, extra: dict[str, str] | None = None,
               policy: str = CHECK):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        write(root, SOURCE, "name: CI\n" + on + jobs)
        write(root, POLICY, policy)
        for path, content in (extra or {}).items():
            write(root, path, content)
        git(root, "init", "-q", "--template=")
        git(root, "add", ".")
        git(root, "commit", "-qm", "queue source fixture")
        yield root


def findings(root: Path, *, selected=(SOURCE,), entry_event="merge_group"):
    return contract.qualification_source_findings(
        contract.read_workflow_catalog(root), selected, entry_event=entry_event)


def codes(items) -> set[str]:
    return {item.code for item in items}


def validate(root: Path, *, selected=(SOURCE,), entry_event="merge_group"):
    return contract.validate(root, root / POLICY, qualification_sources=selected,
                             qualification_entry_event=entry_event)


def cli(root: Path, *args: str):
    return subprocess.run(
        [sys.executable, "-B", str(SCRIPT), "--root", str(root), "--json", *args],
        capture_output=True, text=True, check=False,
    )


class QueueEntrySourceTests(unittest.TestCase):
    def test_explicit_inline_and_block_activities(self):
        for event in (
            "  merge_group:\n    types: [checks_requested]\n",
            "  'merge_group':\n    types: ['checks_requested'] # deliberate activity\n",
            '  merge_group:\n    "types": ["checks_requested"]\n',
            "  merge_group:\n    # queue requests only\n    types:\n      - checks_requested\n",
            "  merge_group:\n    types:\n      - 'checks_requested' # one activity\n",
        ):
            with self.subTest(event=event), repository("on:\n" + event) as root:
                self.assertEqual(findings(root), [])
                self.assertEqual(validate(root)["status"], "SUCCESS")

    def test_non_queue_events_are_rejected_even_with_false_job_guard(self):
        guarded = JOBS.replace("    runs-on:", "    if: false\n    runs-on:")
        for event in ("pull_request", "pull_request_target", "push", "workflow_dispatch",
                      "schedule", "repository_dispatch", "workflow_call", "workflow_run"):
            with self.subTest(event=event), repository(QUEUE_ON + f"  {event}: {{}}\n", jobs=guarded) as root:
                self.assertIn("qualification_non_queue_entry", codes(findings(root)))

    def test_ready_draft_update_and_label_filters_are_not_queue_authority(self):
        for action in ("opened", "reopened", "synchronize", "ready_for_review",
                       "converted_to_draft", "labeled", "unlabeled", "edited"):
            on = QUEUE_ON + f"  pull_request:\n    types: [{action}]\n"
            with self.subTest(action=action), repository(on) as root:
                self.assertIn("qualification_non_queue_entry", codes(findings(root)))

    def test_queue_activity_missing_malformed_or_ambiguous_refuses(self):
        events = (
            "  merge_group: {}\n",
            "  merge_group:\n",
            "  merge_group:\n    types: [destroyed]\n",
            "  merge_group:\n    types: [checks_requested, destroyed]\n",
            "  merge_group:\n    types: [checks_requested, checks_requested]\n",
            "  merge_group:\n    types: []\n",
            "  merge_group:\n    types: [checks_requested,]\n",
            "  merge_group:\n    types: checks_requested\n",
            "  merge_group:\n    types: *activities\n",
            "  merge_group: {types: [checks_requested]}\n",
            "  merge_group: &queue\n    types: [checks_requested]\n",
            "  merge_group:\n    types: >-\n      checks_requested\n",
            "  merge_group:\n    types: [checks_requested]\n    types: [destroyed]\n",
            "  merge_group:\n    types: [checks_requested]\n  merge_group: {}\n",
            "  merge_group:\n     types: [checks_requested]\n",
            "  merge_group:\n    \ttypes: [checks_requested]\n",
            "  merge_group:\n    types:\n     - checks_requested\n",
            "  merge_group:\n    types: [checks_requested]\n      - extra\n",
            "  merge_group:\n    types: [checks_requested]\n    unknown: true\n",
            "  merge_group:\n    types: [checks_requested]\n    <<: *events\n",
            # Bounded syntax refuses filters it cannot independently validate.
            "  merge_group:\n    branches: [main]\n    types: [checks_requested]\n",
            "  merge_group:\n    branches:\n      - main\n       malformed: true\n    types: [checks_requested]\n",
            "  merge_group:\n    types: [checks_requested]\n\tpush: {}\n",
        )
        for event in events:
            with self.subTest(event=event), repository("on:\n" + event) as root:
                self.assertIn("qualification_merge_group_types", codes(findings(root)))

    def test_flow_event_map_and_duplicate_on_keys_refuse(self):
        for on in (
            "on: {merge_group: {types: [checks_requested]}}\n",
            QUEUE_ON + "on:\n  push:\n",
            QUEUE_ON + "'on':\n  push:\n",
            QUEUE_ON + '"on":\n  push:\n',
            QUEUE_ON + '"\\x6f\\x6e":\n  push:\n',
            QUEUE_ON + '? on\n: {push: {}}\n',

        ):
            with self.subTest(on=on), repository(on) as root:
                self.assertTrue(findings(root))
                self.assertIn("qualification_merge_group_types", codes(findings(root)))

    def test_missing_selection_or_invalid_profile_is_not_vacuous_success(self):
        with repository() as root:
            catalog = contract.read_workflow_catalog(root)
            with self.assertRaisesRegex(ValueError, "at least one"):
                contract.qualification_source_findings(catalog, [], entry_event="merge_group")
            with self.assertRaisesRegex(ValueError, "unsupported qualification"):
                contract.qualification_source_findings(catalog, [SOURCE], entry_event="pull_request")
            self.assertEqual(contract.qualification_source_findings(catalog, []), [])

    def test_missing_selected_source_is_visible(self):
        with repository() as root:
            self.assertIn("qualification_source_missing",
                          codes(findings(root, selected=(".github/workflows/absent.yml",))))

    def test_each_selected_root_is_checked(self):
        second = ".github/workflows/second.yml"
        with repository(extra={second: "name: Second\non:\n  pull_request:\n" + JOBS}) as root:
            result = findings(root, selected=(SOURCE, second))
            self.assertTrue(any(item.subject == second and item.code == "qualification_non_queue_entry"
                                for item in result))

    def test_absent_duplicate_or_flow_jobs_remain_unsupported(self):
        for jobs in ("jobs:\n", JOBS + JOBS.split("jobs:\n")[1],
                     "jobs:\n  compile: {runs-on: ubuntu-latest, steps: []}\n"):
            with self.subTest(jobs=jobs), repository(jobs=jobs) as root:
                self.assertIn("qualification_source_unsupported", codes(findings(root)))

    def test_selected_reusable_is_not_automatically_trusted(self):
        jobs = "jobs:\n  compile:\n    uses: ./.github/workflows/leaf.yml\n"
        with repository(jobs=jobs) as root:
            self.assertIn("qualification_unresolved_reusable", codes(findings(root)))

    def test_transitive_callers_and_cycles_do_not_hide_incoming_paths(self):
        caller = ".github/workflows/caller.yml"
        outer = ".github/workflows/outer.yml"
        extra = {
            caller: "name: Caller\non:\n  workflow_call:\njobs:\n  nested:\n"
                    f"    uses: ./{SOURCE}\n  cycle:\n    uses: ./{outer}\n",
            outer: "name: Outer\non:\n  pull_request:\njobs:\n  invoke:\n"
                   f"    uses: ./{caller}\n",
        }
        with repository(extra=extra) as root:
            result = findings(root)
            self.assertEqual({item.subject for item in result
                              if item.code == "qualification_unresolved_caller"}, {caller, outer})

    def test_unresolved_remote_and_missing_reusables_remain_visible(self):
        for target in ("owner/repo/.github/workflows/build.yml@main",
                       "./.github/workflows/missing.yml", "${{ inputs.workflow }}"):
            extra = {".github/workflows/caller.yml":
                     "name: Caller\non:\n  workflow_dispatch:\njobs:\n  invoke:\n"
                     f"    uses: {target}\n"}
            with self.subTest(target=target), repository(extra=extra) as root:
                self.assertIn("qualification_catalog_unresolved_reusable", codes(findings(root)))

    def test_unselected_unsupported_workflow_cannot_disappear(self):
        extra = {".github/workflows/bad.yml":
                 "name: Bad\non: {pull_request: {}}\njobs:\n  bad: {}\n"}
        with repository(extra=extra) as root:
            self.assertIn("qualification_catalog_unsupported", codes(findings(root)))

    def test_result_paths_remain_unresolved_not_blanket_allowed(self):
        extra = {".github/workflows/result.yml":
                 "name: Result\non:\n  workflow_run:\n    workflows: [CI]\n" + JOBS}
        with repository(extra=extra) as root:
            self.assertIn("qualification_unresolved_result_path", codes(findings(root)))

    def test_dispatch_hints_remain_visible_behind_false_guard(self):
        for command in ("gh workflow run ci.yml", "gh workflow \\\n          run ci.yml",
                        "github.rest.actions.createWorkflowDispatch({})",
                        "github.rest.repos.createDispatchEvent({})",
                        "curl repos/owner/repo/dispatches"):
            jobs = JOBS.replace("    steps: []", "    if: false\n    steps:\n"
                                f"      - run: |\n          {command}")
            with self.subTest(command=command), repository(jobs=jobs) as root:
                self.assertIn("qualification_unresolved_dispatch", codes(findings(root)))

    def test_policy_declared_events_still_have_to_exist(self):
        with repository(policy=CHECK.replace('["merge_group"]', '["pull_request"]')) as root:
            result = validate(root)
            self.assertEqual(result["status"], "BLOCKED")
            self.assertIn("workflow_event_missing", {item["code"] for item in result["findings"]})


class QueueReceiptAndCliTests(unittest.TestCase):
    def test_queue_static_success_is_never_native_or_product_proof(self):
        with repository() as root:
            result = validate(root)
            self.assertEqual(result["status"], "SUCCESS")
            self.assertEqual(result["qualification_status"], "NOT_PROVEN")
            self.assertEqual(result["live_enforcement_status"], "NOT_PROVEN")
            self.assertEqual(result["qualification_entry_event"], "merge_group")
            self.assertEqual(result["subjects"]["repository_sha"], git(root, "rev-parse", "HEAD"))

    def test_entry_mode_is_bound_into_both_fingerprints(self):
        with repository() as root:
            queue = validate(root)
            legacy = validate(root, entry_event="workflow_dispatch")
        self.assertNotEqual(queue["subject_sha256"], legacy["subject_sha256"])
        self.assertNotEqual(queue["exact_source_sha256"], legacy["exact_source_sha256"])
        self.assertEqual(queue["semantic_subject"]["qualification_entry_event"], "merge_group")
        self.assertEqual(queue["subjects"]["qualification_entry_event"], "merge_group")
        # Recalculate each hash independently from the serialized subject.
        def digest(value):
            return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                              ensure_ascii=False).encode("utf-8")).hexdigest()
        self.assertEqual(queue["subject_sha256"], digest(queue["semantic_subject"]))
        exact = deepcopy(queue["subjects"])
        exact.pop("contexts")
        self.assertEqual(queue["exact_source_sha256"], digest(exact))

    def test_default_dispatch_receipt_is_identical_to_explicit_legacy_mode(self):
        with repository("on:\n  workflow_dispatch:\n",
                        policy=CHECK.replace('["merge_group"]', '["workflow_dispatch"]')) as root:
            implicit = contract.validate(root, root / POLICY, qualification_sources=[SOURCE])
            explicit = validate(root, entry_event="workflow_dispatch")
            self.assertEqual(implicit, explicit)
            self.assertEqual(explicit["status"], "SUCCESS")
            self.assertNotIn("qualification_entry_event", explicit)
            self.assertNotIn("qualification_entry_event", explicit["semantic_subject"])
            self.assertNotIn("qualification_entry_event", explicit["subjects"])

    def test_legacy_mode_still_rejects_queue_roots(self):
        with repository() as root:
            result = validate(root, entry_event="workflow_dispatch")
            self.assertEqual(result["status"], "BLOCKED")
            finding = next(item for item in result["findings"]
                           if item["code"] == "qualification_automatic_entry")
            self.assertEqual(finding["message"],
                             "expected only explicit workflow_dispatch; observed ['merge_group']")

    def test_duplicate_selection_is_deterministic(self):
        with repository() as root:
            self.assertEqual(validate(root), validate(root, selected=(SOURCE, SOURCE)))

    def test_actual_cli_success_and_written_receipt(self):
        with repository() as root:
            receipt = root / "result.json"
            process = cli(root, "--qualification-entry-event", "merge_group",
                          "--qualification-source", SOURCE, "--receipt", str(receipt))
            self.assertEqual(process.returncode, 0, process.stderr)
            result = json.loads(process.stdout)
            self.assertEqual(result, json.loads(receipt.read_text(encoding="utf-8")))
            self.assertEqual(result["status"], "SUCCESS")
            self.assertEqual(result["qualification_status"], "NOT_PROVEN")
            self.assertEqual(result["live_enforcement_status"], "NOT_PROVEN")

    def test_cli_no_selection_fails_with_explicit_not_proven(self):
        with repository() as root:
            process = cli(root, "--qualification-entry-event", "merge_group")
        self.assertEqual(process.returncode, 1, process.stderr)
        result = json.loads(process.stdout)
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertEqual(result["qualification_status"], "NOT_PROVEN")
        self.assertEqual(result["qualification_entry_event"], "merge_group")
        self.assertIn("at least one", result["findings"][0]["message"])

    def test_cli_lifecycle_root_and_malformed_queue_are_non_success(self):
        for on in (QUEUE_ON + "  pull_request:\n", "on:\n  merge_group: {}\n"):
            with self.subTest(on=on), repository(on) as root:
                process = cli(root, "--qualification-entry-event", "merge_group",
                              "--qualification-source", SOURCE)
                self.assertEqual(process.returncode, 1, process.stderr)
                self.assertEqual(json.loads(process.stdout)["status"], "BLOCKED")

    def test_dirty_exact_source_stays_not_proven(self):
        with repository() as root:
            write(root, SOURCE, (root / SOURCE).read_text(encoding="utf-8") + "# dirty\n")
            process = cli(root, "--qualification-entry-event", "merge_group",
                          "--qualification-source", SOURCE)
        self.assertEqual(process.returncode, 1)
        result = json.loads(process.stdout)
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIsNone(result["subjects"])

    def test_invalid_cli_profile_is_not_accepted(self):
        with repository() as root:
            process = cli(root, "--qualification-entry-event", "pull_request")
        self.assertEqual(process.returncode, 2)
        self.assertIn("invalid choice", process.stderr)

    def test_validator_invokes_only_read_only_local_git_commands(self):
        with repository() as root:
            original = subprocess.run
            calls = []
            def spy(argv, *args, **kwargs):
                calls.append(argv)
                self.assertEqual(argv[0], "git")
                self.assertIn(argv[3], {"rev-parse", "status", "ls-files"})
                return original(argv, *args, **kwargs)
            with patch.object(contract.subprocess, "run", side_effect=spy):
                self.assertEqual(validate(root)["status"], "SUCCESS")
            self.assertTrue(calls)
            self.assertEqual(git(root, "status", "--porcelain"), "")


class QueueWorkflowWiringTests(unittest.TestCase):
    """Read actual migrated source, not a parallel invented workflow."""
    def test_advisory_contract_workflow_is_queue_only(self):
        workflow = contract.read_workflow(ROOT, OWN_WORKFLOW)
        self.assertEqual(workflow.events, frozenset({"merge_group"}))
        self.assertEqual(workflow.merge_group_types, ("checks_requested",))
        self.assertEqual(contract.qualification_source_findings(
            {OWN_WORKFLOW: workflow}, [OWN_WORKFLOW], entry_event="merge_group"), [])
        self.assertEqual(set(workflow.jobs), {"contract"})
        self.assertEqual(workflow.jobs["contract"].name, "Gate Enforcement Contract")
        self.assertFalse(workflow.jobs["contract"].continue_on_error)

    def test_policy_matches_actual_advisory_producer(self):
        policy = tomllib.loads((ROOT / POLICY).read_text(encoding="utf-8"))
        rows = [row for row in policy["checks"] if row["workflow"] == OWN_WORKFLOW]
        self.assertEqual(len(rows), 1)
        row = rows[0]
        self.assertEqual(row["events"], ["merge_group"])
        self.assertFalse(row["required"])
        self.assertEqual(row["policy_role"], "advisory")
        self.assertEqual(row["enforcement"], "neither")
        workflows = {OWN_WORKFLOW: contract.read_workflow(ROOT, OWN_WORKFLOW)}
        result, mapped = contract.validate_context(row, workflows,
                                                   contract.build_producer_index(workflows))
        self.assertEqual(result, [])
        self.assertTrue(mapped)

    def test_new_falsifiers_are_in_existing_executed_test_list(self):
        workflow = (ROOT / OWN_WORKFLOW).read_text(encoding="utf-8")
        command = workflow.split("python3 -m unittest \\\n", 1)[1].split("test_status=$?", 1)[0]
        self.assertIn("scripts/ci/test_queue_qualification_source.py", command)
        self.assertIn("scripts/ci/test_validate_gate_enforcement_contract.py", command)
        self.assertIn("scripts/ci/test_gate_enforcement_app_bindings.py", command)
        self.assertIn("group: ${{ github.workflow }}-${{ github.sha }}", workflow)
        self.assertIn("cancel-in-progress: false", workflow)
        self.assertIn("permissions:\n  contents: read\n", workflow)
        self.assertNotIn("checks: write", workflow)
        self.assertNotIn("pull-requests: write", workflow)


if __name__ == "__main__":
    unittest.main()
