#!/usr/bin/env python3
"""Contract tests for the main-red refusal's evidence source (#19565).

The commit-level check-run census (``GET /repos/{repo}/commits/{sha}/check-runs``)
returns only the most recent 1000 check suites for a commit and silently drops
the rest, so a long-lived ``main`` HEAD loses the ``ci.yml`` push suite that
holds its ``CI Gate shard (...)`` verdicts. The refusal then reads "no
evidence" as non-blocking and reports itself non-blocking.

These tests pin the complete route. The probe must read the canonical
``ci.yml`` run's own check suite (``GET /repos/{repo}/check-suites/{id}/check-runs``),
which is complete and uncapped, and it must keep the check-run schema the
trusted classifier already consumes -- observed fields only, never a derived
or synthesised verdict. A missing suite or a failed fetch must degrade to a
non-blocking evidence gap, never to a block and never to a silent green.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"
PROBE_STEP = "Probe main-red refusal"
STRUCTURAL_STEP = "Prove rustfmt prevention contract"
CONTRACT_TEST = "scripts/ci/test_main_red_evidence_source.py"
PROGRAM_MARKER = "<<'MAIN_RED_EVIDENCE_PY'"
PROGRAM_TERMINATOR = "MAIN_RED_EVIDENCE_PY"
PROGRAM_NAME = "fetch_main_red_evidence.py"
# The census is the defective source: it must not survive anywhere in the
# probe. The suite route keeps "/check-runs" but never under "/commits/".
# The endpoint must appear as a request target, so a trailing quote, query
# or end of line is required; the probe's own comment may name the endpoint
# it no longer calls.
CENSUS_PATTERN = re.compile(r"commits/[^\s\")]*check-runs(?=$|[\"'?&])")
# One invocation of the inline evidence reader and its quoted arguments.
INVOCATION_PATTERN = re.compile(
    r"fetch_main_red_evidence\.py\"?[ \t]*(?:\\\r?\n[ \t]*\"[^\"]*\"[ \t]*)+"
)


def step_block(workflow: str, step_name: str) -> str:
    pattern = re.compile(
        rf"(?ms)^(?P<indent>[ \t]*)- name: {re.escape(step_name)}[ \t]*\n"
        rf"(?P<body>.*?)(?=^(?P=indent)- name: |\Z)"
    )
    match = pattern.search(workflow)
    if match is None:
        raise AssertionError(f"workflow step not found: {step_name}")
    return match.group(0)


def step_offset(workflow: str, step_name: str) -> int:
    pattern = re.compile(
        rf"(?m)^(?P<indent>[ \t]*)- name: {re.escape(step_name)}[ \t]*$"
    )
    match = pattern.search(workflow)
    if match is None:
        raise AssertionError(f"workflow step not found: {step_name}")
    return match.start()


def probe_block(workflow: str) -> str:
    return step_block(workflow, PROBE_STEP)


def evidence_program(workflow: str) -> str:
    """Return the Python program the probe writes and runs inline."""
    block = probe_block(workflow)
    if PROGRAM_MARKER not in block:
        raise AssertionError(
            "the probe must ship its check-suite evidence reader inline"
        )
    _, _, rest = block.partition(PROGRAM_MARKER)
    body: list[str] = []
    for line in rest.split("\n")[1:]:
        if line.strip() == PROGRAM_TERMINATOR:
            break
        body.append(line)
    else:
        raise AssertionError("the probe evidence reader heredoc has no terminator")
    program = textwrap.dedent("\n".join(body))
    if not program.strip():
        raise AssertionError("the probe evidence reader is empty")
    compile(program, "probe-evidence-reader", "exec")
    return program


def load_program(workflow: str) -> dict:
    """Execute the shipped evidence reader and return its namespace."""
    program = evidence_program(workflow)
    namespace: dict = {
        "__name__": "probe_evidence_reader",
        "__file__": PROGRAM_NAME,
    }
    exec(compile(program, PROGRAM_NAME, "exec"), namespace)
    return namespace


def program_invocations(workflow: str) -> list[str]:
    return [match.group(0) for match in INVOCATION_PATTERN.finditer(probe_block(workflow))]


def executable_text(block: str) -> str:
    """Return the block without whole-line comments.

    The probe documents the census endpoint it no longer calls, so a comment
    must not be read as a request target.
    """
    return "\n".join(
        line for line in block.split("\n") if not line.lstrip().startswith("#")
    )


def validate_evidence_source(workflow: str) -> None:
    """Raise when the probe does not read main evidence from the check suite."""
    block = probe_block(workflow)
    if CENSUS_PATTERN.search(executable_text(block)) is not None:
        raise AssertionError(
            "the probe must not read evidence from the commit-level check-run census"
        )
    if "check-suites/" not in block:
        raise AssertionError(
            "the probe must read evidence from the canonical ci.yml run's check suite"
        )
    program = evidence_program(workflow)
    for needle in (
        "check_suite_id",
        "check-suites/{suite_id}/check-runs?per_page=100",
    ):
        if needle not in program:
            raise AssertionError(f"the evidence reader must use {needle}")
    for forbidden in ('"conclusion":', '"status":', "conclusion=", "status="):
        if forbidden in program:
            raise AssertionError(
                "the evidence reader must pass observed check-run fields through unchanged"
            )
    invocations = program_invocations(workflow)
    if not any('"${MAIN_SHA_BEFORE}"' in invocation for invocation in invocations):
        raise AssertionError("the probe must read main evidence through the evidence reader")
    if not any('"${CANDIDATE_SHA}"' in invocation for invocation in invocations):
        raise AssertionError(
            "the probe must read candidate evidence through the evidence reader"
        )


def shard_run(
    name: str,
    run_id: int,
    conclusion: str = "success",
    head_sha: str = "a" * 40,
) -> dict:
    """Return one observed check run in the API's own shape."""
    return {
        "name": name,
        "status": "completed",
        "conclusion": conclusion,
        "details_url": f"https://github.com/o/r/actions/runs/{run_id}/job/1",
        "head_sha": head_sha,
        "app": {"id": 15368, "slug": "github-actions"},
    }


class MainRedEvidenceSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

    def test_checked_in_probe_reads_the_canonical_check_suite(self) -> None:
        validate_evidence_source(self.workflow)

    def test_evidence_reader_selects_the_canonical_suite(self) -> None:
        namespace = load_program(self.workflow)
        payload = {
            "workflow_runs": [
                {"id": 43982865194, "head_sha": "a" * 40, "check_suite_id": 119067894139},
                {"id": 44578100891, "head_sha": "b" * 40, "check_suite_id": 120669076434},
            ]
        }
        self.assertEqual(
            namespace["canonical_suites"](payload, "a" * 40),
            [119067894139],
        )
        self.assertEqual(
            namespace["canonical_run_ids"](payload, "b" * 40),
            {44578100891},
        )
        self.assertEqual(namespace["canonical_suites"](payload, "c" * 40), [])

    def test_evidence_reader_keeps_only_canonical_run_check_runs(self) -> None:
        namespace = load_program(self.workflow)
        suite_payload = {
            "check_runs": [
                shard_run("CI Gate shard (meta)", 43982865194),
                shard_run("CI Gate shard (policy)", 99999999999),
            ]
        }
        selected = namespace["select_canonical_runs"](suite_payload, {43982865194})
        self.assertEqual([run["name"] for run in selected], ["CI Gate shard (meta)"])

    def test_evidence_reader_treats_a_suite_without_canonical_runs_as_a_gap(self) -> None:
        namespace = load_program(self.workflow)
        foreign_payload = {"check_runs": [shard_run("CI Gate shard (meta)", 99999999999)]}
        self.assertIsNone(
            namespace["collect_canonical_runs"]([foreign_payload], {43982865194})
        )
        canonical_payload = {"check_runs": [shard_run("CI Gate shard (meta)", 43982865194)]}
        self.assertEqual(
            namespace["collect_canonical_runs"]([canonical_payload], {43982865194}),
            canonical_payload["check_runs"],
        )

    def test_evidence_reader_keeps_evidence_beside_an_empty_suite(self) -> None:
        """A superseded run leaves an empty suite beside a current one.

        The empty suite must not discard the evidence of the run beside it,
        or a green candidate would lose its evidence and a red main would
        then block it for the wrong reason.
        """
        namespace = load_program(self.workflow)
        empty_payload = {"check_runs": []}
        canonical_payload = {"check_runs": [shard_run("CI Gate shard (meta)", 43982865194)]}
        selected = namespace["collect_canonical_runs"](
            [empty_payload, canonical_payload], {43982865194}
        )
        self.assertEqual(selected, canonical_payload["check_runs"])
        self.assertEqual(
            namespace["collect_canonical_runs"](
                [canonical_payload, empty_payload], {43982865194}
            ),
            canonical_payload["check_runs"],
        )
        self.assertIsNone(
            namespace["collect_canonical_runs"]([empty_payload, empty_payload], {43982865194})
        )

    def test_evidence_reader_emits_observed_check_run_fields(self) -> None:
        namespace = load_program(self.workflow)
        observed = shard_run("CI Gate shard (meta)", 43982865194, conclusion="failure")
        with tempfile.TemporaryDirectory() as directory:
            out_path = Path(directory) / "main-runs.json"
            namespace["write_payload"](out_path, [observed])
            written = json.loads(out_path.read_text(encoding="utf-8"))
        self.assertEqual(written, {"check_runs": [observed]})

    def _run_reader(self, directory: Path, subject_sha: str, environment=None):
        """Run the shipped reader and return (completed, output payload text)."""
        root = Path(directory)
        program_path = root / PROGRAM_NAME
        program_path.write_text(evidence_program(self.workflow), encoding="utf-8")
        runs_path = root / "runs.json"
        runs_path.write_text(
            json.dumps(
                {"workflow_runs": [{"id": 1, "head_sha": "c" * 40, "check_suite_id": 2}]}
            ),
            encoding="utf-8",
        )
        out_path = root / "main-runs.json"
        run_environment = dict(os.environ)
        # The reader needs a repository to read from. Pin it so the branch
        # under test is the one the test names, not whatever the ambient
        # environment happens to carry.
        run_environment["REPOSITORY"] = "EffortlessMetrics/perl-lsp-swarm"
        if environment is not None:
            run_environment.update(environment)
        completed = subprocess.run(
            [
                sys.executable,
                str(program_path),
                str(runs_path),
                subject_sha,
                str(out_path),
                "main",
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
            env=run_environment,
        )
        return completed, out_path.read_text(encoding="utf-8")

    def test_evidence_reader_degrades_to_non_blocking_without_a_canonical_suite(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            completed, written = self._run_reader(directory, "d" * 40)
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("::warning::", completed.stdout)
        # The branch under test must be the one the test names.
        self.assertIn(f"no canonical ci.yml run for {'d' * 40}", completed.stdout)
        # The failure payload must be the empty file the retired census error
        # path wrote, so the trusted classifier reads it as missing evidence
        # (a warning, hence non-blocking) instead of as a valid observation
        # that merely lacks a shard.
        self.assertEqual(written, "")
        with self.assertRaises(ValueError):
            json.loads(written)

    def test_evidence_reader_degrades_when_the_repository_is_unknown(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            completed, written = self._run_reader(
                directory, "c" * 40, {"REPOSITORY": ""}
            )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("::warning::", completed.stdout)
        self.assertIn("the repository to read from is unknown", completed.stdout)
        self.assertEqual(written, "")

    def test_census_reintroduction_is_rejected(self) -> None:
        mutated = self.workflow.replace(
            f'            python3 "$probe_dir/{PROGRAM_NAME}" \\\n',
            "            gh api --paginate --slurp \\\n"
            '              "repos/${REPOSITORY}/commits/${MAIN_SHA_BEFORE}/check-runs?per_page=100" \\\n'
            '              >"$probe_dir/main-runs.json" || api_ok=false\n'
            f'            python3 "$probe_dir/{PROGRAM_NAME}" \\\n',
        )
        self.assertNotEqual(mutated, self.workflow)
        with self.assertRaises(AssertionError):
            validate_evidence_source(mutated)

    def test_suite_route_removal_is_rejected(self) -> None:
        mutated = self.workflow.replace(
            "/check-suites/{suite_id}/check-runs?per_page=100",
            "/check-runs?per_page=100",
        )
        self.assertNotEqual(mutated, self.workflow)
        with self.assertRaises(AssertionError):
            validate_evidence_source(mutated)

    def test_contract_test_is_registered_in_the_structural_lane(self) -> None:
        structural = step_block(self.workflow, STRUCTURAL_STEP)
        self.assertIn(CONTRACT_TEST, structural)
        self.assertLess(
            step_offset(self.workflow, STRUCTURAL_STEP),
            step_offset(self.workflow, PROBE_STEP),
        )


if __name__ == "__main__":
    unittest.main()
