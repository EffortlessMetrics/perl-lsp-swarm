#!/usr/bin/env python3
"""Contract tests for the main-red refusal's evidence source (#19565, #19567).

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

#19567 binds the subject to the event that produced its verdicts. A
``pull_request`` subject's evidence comes from the ``pull_request`` run and
``main``'s from the ``push`` run. Without that binding, a
``workflow_dispatch`` run on the same SHA stands in for the authoritative
run, so an authoritative run that never executed reads as another event's
green verdicts -- the same "no evidence read as success" defect one level
down. The attempt is bound with it: a re-run keeps the run id, increments
``run_attempt`` and reports the current attempt's own ``check_suite_id``, so
only the current attempt's suite is authoritative.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest import mock

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

# The live counterexample, measured on the GitHub REST API: PR #19392's head
# carries two ci.yml runs on one SHA, and the authoritative event's suite is
# empty because no job ever ran.
LIVE_SHA = "89ceda719e2e8c8d0744f6f53f62dada9d7df5b0"
LIVE_PULL_REQUEST_RUN = 44464451758
LIVE_PULL_REQUEST_SUITE = 120361150442
LIVE_DISPATCH_RUN = 44464451342
LIVE_DISPATCH_SUITE = 120361149362
# The live re-run shape: one run id, attempt 1's suite and attempt 2's
# suite. The list endpoint reports attempt 2 (the current attempt) and
# attempt 1's suite is reachable only through the run's previous attempt.
LIVE_RERUN_RUN = 43985707605
LIVE_RERUN_SUPERSEDED_SUITE = 119075331084
LIVE_RERUN_CURRENT_SUITE = 119075337920


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
    return load_program_text(evidence_program(workflow))


def load_program_text(program: str) -> dict:
    """Execute an evidence reader program and return its namespace."""
    namespace: dict = {
        "__name__": "probe_evidence_reader",
        "__file__": PROGRAM_NAME,
    }
    exec(compile(program, PROGRAM_NAME, "exec"), namespace)
    return namespace


def program_invocations(workflow: str) -> list[str]:
    return [match.group(0) for match in INVOCATION_PATTERN.finditer(probe_block(workflow))]


def select_run_ids(namespace: dict, payload: dict, subject_sha: str, event: str):
    """Return the run ids the reader admits for one subject and event.

    A reader that takes the authoritative event binds it. A reader that
    does not take one at all admits every run on the SHA, which is the
    defect these contracts pin. The fallback keeps the assertion about
    the reader's behaviour rather than about its argument list, so the
    pre-repair reader is measured on what it selects instead of failing
    on an arity error.
    """
    try:
        return namespace["canonical_run_ids"](payload, subject_sha, event)
    except TypeError:
        return namespace["canonical_run_ids"](payload, subject_sha)


def select_suites(namespace: dict, payload: dict, subject_sha: str, event: str):
    """Return the check suite ids the reader admits for one subject and event.

    See :func:`select_run_ids` for the fallback's rationale.
    """
    try:
        return namespace["canonical_suites"](payload, subject_sha, event)
    except TypeError:
        return namespace["canonical_suites"](payload, subject_sha)


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
    # #19567: the reader must bind the subject to the event that produced
    # its verdicts, and the call sites must name that event. Without the
    # binding, a workflow_dispatch run on the subject SHA stands in for the
    # authoritative run.
    if 'run.get("event") != event' not in program:
        raise AssertionError(
            "the evidence reader must bind the subject to its authoritative event"
        )
    if 'run.get("run_attempt")' not in program:
        raise AssertionError(
            "the evidence reader must bind the subject to the current attempt"
        )
    if not any('"${MAIN_EVENT}"' in invocation for invocation in invocations):
        raise AssertionError(
            "the probe must bind main's evidence to the event that produced main's verdicts"
        )
    if not any('"${CANDIDATE_EVENT}"' in invocation for invocation in invocations):
        raise AssertionError(
            "the probe must bind the candidate's evidence to the event that produced it"
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
                {
                    "id": 43982865194,
                    "event": "push",
                    "head_sha": "a" * 40,
                    "check_suite_id": 119067894139,
                },
                {
                    "id": 44578100891,
                    "event": "push",
                    "head_sha": "b" * 40,
                    "check_suite_id": 120669076434,
                },
            ]
        }
        self.assertEqual(
            namespace["canonical_suites"](payload, "a" * 40, "push"),
            [119067894139],
        )
        self.assertEqual(
            namespace["canonical_run_ids"](payload, "b" * 40, "push"),
            {44578100891},
        )
        self.assertEqual(namespace["canonical_suites"](payload, "c" * 40, "push"), [])

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

    # -- #19567: the subject is bound to its authoritative event and attempt.

    @staticmethod
    def _counterexample_runs() -> dict:
        """Return the live counterexample's workflow-run payload.

        PR #19392's head carries two ci.yml runs: the ``pull_request`` run
        whose suite is empty because no job ever ran, and a
        ``workflow_dispatch`` run whose suite holds eight green shards.
        """
        return {
            "workflow_runs": [
                {
                    "id": LIVE_PULL_REQUEST_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": LIVE_PULL_REQUEST_SUITE,
                    "run_attempt": 1,
                },
                {
                    "id": LIVE_DISPATCH_RUN,
                    "event": "workflow_dispatch",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": LIVE_DISPATCH_SUITE,
                    "run_attempt": 1,
                },
            ]
        }

    @staticmethod
    def _counterexample_suites() -> dict:
        """Return the live counterexample's per-suite check-run payloads."""
        return {
            LIVE_PULL_REQUEST_SUITE: {"check_runs": []},
            LIVE_DISPATCH_SUITE: {
                "check_runs": [
                    shard_run(
                        "CI Gate shard (meta)",
                        LIVE_DISPATCH_RUN,
                        head_sha=LIVE_SHA,
                    ),
                    shard_run(
                        "CI Gate shard (policy)",
                        LIVE_DISPATCH_RUN,
                        head_sha=LIVE_SHA,
                    ),
                ]
            },
        }

    def _run_reader_program(
        self,
        program: str,
        runs_payload: dict,
        suites: dict,
        event: str,
        label: str = "candidate",
    ):
        """Run a reader program against fixture suites.

        Returns ``(status, stdout, payload text, fetched suite ids)``. The
        suite fetch is replaced so the test observes which suites the
        reader chose without touching the network.
        """
        namespace = load_program_text(program)
        fetched: list[int] = []

        def fake_fetch(repository: str, suite_id: int):
            fetched.append(suite_id)
            return suites[suite_id]

        namespace["fetch_suite_check_runs"] = fake_fetch
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runs_path = root / "runs.json"
            runs_path.write_text(json.dumps(runs_payload), encoding="utf-8")
            out_path = root / "out.json"
            argv = [
                PROGRAM_NAME,
                str(runs_path),
                LIVE_SHA,
                str(out_path),
                label,
                event,
            ]
            stdout = io.StringIO()
            with mock.patch.dict(
                os.environ,
                {"REPOSITORY": "EffortlessMetrics/perl-lsp-swarm"},
            ):
                with contextlib.redirect_stdout(stdout):
                    status = namespace["main"](argv)
                    if status == 2:
                        # A reader that does not take the authoritative
                        # event at all. Call it the way it wants to be
                        # called, so the test measures what it selects
                        # rather than failing on an arity error.
                        status = namespace["main"](argv[:5])
            written = out_path.read_text(encoding="utf-8") if out_path.exists() else None
        return status, stdout.getvalue(), written, fetched

    def test_evidence_reader_binds_the_subject_to_its_authoritative_event(self) -> None:
        """The subject's evidence comes from the authoritative event's run.

        Discriminating assertion. It fails unless the reader filters the
        subject's runs by the authoritative event: without that filter the
        pre-repair reader admitted both runs on the SHA, and the
        ``workflow_dispatch`` run stood in for the ``pull_request`` run
        that never executed.
        """
        namespace = load_program(self.workflow)
        payload = self._counterexample_runs()
        self.assertEqual(
            select_run_ids(namespace, payload, LIVE_SHA, "pull_request"),
            {LIVE_PULL_REQUEST_RUN},
        )
        self.assertEqual(
            select_suites(namespace, payload, LIVE_SHA, "pull_request"),
            [LIVE_PULL_REQUEST_SUITE],
        )
        # The other event's run is still readable when it is the
        # authoritative one; it is only excluded as a substitute.
        self.assertEqual(
            select_run_ids(namespace, payload, LIVE_SHA, "workflow_dispatch"),
            {LIVE_DISPATCH_RUN},
        )
        self.assertEqual(
            select_suites(namespace, payload, LIVE_SHA, "workflow_dispatch"),
            [LIVE_DISPATCH_SUITE],
        )

    def test_evidence_reader_does_not_substitute_another_event_suite(self) -> None:
        """An empty authoritative suite stays an evidence gap.

        This is the live counterexample end to end. The ``pull_request``
        run's suite is empty because no job ever ran, and the
        ``workflow_dispatch`` run's suite holds green shards. The reader
        must never read the dispatch suite as the candidate's evidence:
        that is the "no evidence read as success" defect one level down.

        Discriminating assertion. It fails unless the reader binds the
        subject to its authoritative event: with the binding removed the
        dispatch suite is fetched and its green shards become the
        candidate's payload, so the payload is a valid observation of
        eight green shards instead of an evidence gap.
        """
        program = evidence_program(self.workflow)
        status, stdout, written, fetched = self._run_reader_program(
            program,
            self._counterexample_runs(),
            self._counterexample_suites(),
            "pull_request",
        )
        # The other event's suite was never even read.
        self.assertNotIn(LIVE_DISPATCH_SUITE, fetched)
        self.assertIn(LIVE_PULL_REQUEST_SUITE, fetched)
        # The authoritative run never executed: that is an evidence gap,
        # not another event's verdicts. The payload is the empty file the
        # trusted classifier reads as missing evidence.
        self.assertEqual(written, "")
        self.assertNotEqual(status, 0)
        self.assertIn("::warning::", stdout)
        # The warning names the authoritative event, so the gap is
        # distinguishable from "no run at all".
        self.assertIn("pull_request", stdout)
        self.assertNotIn("CI Gate shard", stdout)

    def test_evidence_reader_event_binding_discriminates_the_substitution(self) -> None:
        """Removing the event binding reproduces the defect.

        The mutation is the pre-repair selection rule: it drops the event
        filter, so every run on the SHA is admitted. The reader then
        fetches the ``workflow_dispatch`` suite and writes its green
        shards as the candidate's payload. This proves the assertion
        above discriminates the binding rather than the fixture.
        """
        program = evidence_program(self.workflow)
        mutated = program.replace('if run.get("event") != event:', "if False:")
        if mutated == program:
            self.fail("the event-binding mutation did not apply; update the mutation anchor")
        status, _, written, fetched = self._run_reader_program(
            mutated,
            self._counterexample_runs(),
            self._counterexample_suites(),
            "pull_request",
        )
        self.assertIn(LIVE_DISPATCH_SUITE, fetched)
        self.assertEqual(status, 0)
        self.assertIsNotNone(written)
        self.assertIn("CI Gate shard (meta)", written)

    def test_evidence_reader_binds_to_the_current_attempt(self) -> None:
        """A superseded attempt's suite is not the current attempt's evidence.

        A re-run keeps the run id and increments ``run_attempt``; the run
        object reports the current attempt's own ``check_suite_id``. The
        live list endpoint reports only the current attempt (run 43985707605
        appears with ``run_attempt`` 2 and suite 119075337920, and attempt
        1's suite 119075331084 is reachable only through the run's previous
        attempt), so reading the suite off the run object is the binding
        itself and no second attempt endpoint is needed.

        Discriminating assertion. It fails unless the reader keeps the
        highest attempt per run id: the pre-repair reader admitted every
        suite the payload carried for the run.
        """
        namespace = load_program(self.workflow)
        payload = {
            "workflow_runs": [
                {
                    "id": LIVE_RERUN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": LIVE_RERUN_SUPERSEDED_SUITE,
                    "run_attempt": 1,
                },
                {
                    "id": LIVE_RERUN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": LIVE_RERUN_CURRENT_SUITE,
                    "run_attempt": 2,
                },
            ]
        }
        self.assertEqual(
            select_suites(namespace, payload, LIVE_SHA, "pull_request"),
            [LIVE_RERUN_CURRENT_SUITE],
        )
        self.assertEqual(
            select_run_ids(namespace, payload, LIVE_SHA, "pull_request"),
            {LIVE_RERUN_RUN},
        )

    def test_evidence_reader_attempt_binding_discriminates_the_superseded_suite(self) -> None:
        """Removing the attempt binding admits the superseded attempt's suite.

        The mutation keeps the first attempt the payload carries instead of
        the current one, so the superseded attempt's suite is read.
        """
        program = evidence_program(self.workflow)
        mutated = program.replace(
            "if previous is None or run_attempt(run) > run_attempt(previous):",
            "if previous is None:",
        )
        if mutated == program:
            self.fail("the attempt-binding mutation did not apply; update the mutation anchor")
        namespace = load_program_text(mutated)
        payload = {
            "workflow_runs": [
                {
                    "id": LIVE_RERUN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": LIVE_RERUN_SUPERSEDED_SUITE,
                    "run_attempt": 1,
                },
                {
                    "id": LIVE_RERUN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": LIVE_RERUN_CURRENT_SUITE,
                    "run_attempt": 2,
                },
            ]
        }
        self.assertEqual(
            namespace["canonical_suites"](payload, LIVE_SHA, "pull_request"),
            [LIVE_RERUN_SUPERSEDED_SUITE],
        )

    def _run_reader(self, directory: Path, subject_sha: str, environment=None):
        """Run the shipped reader and return (completed, output payload text)."""
        root = Path(directory)
        program_path = root / PROGRAM_NAME
        program_path.write_text(evidence_program(self.workflow), encoding="utf-8")
        runs_path = root / "runs.json"
        runs_path.write_text(
            json.dumps(
                {
                    "workflow_runs": [
                        {
                            "id": 1,
                            "event": "push",
                            "head_sha": "c" * 40,
                            "check_suite_id": 2,
                        }
                    ]
                }
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
                "push",
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
            env=run_environment,
        )
        if completed.returncode == 2:
            # A reader that does not take the authoritative event at all.
            # Call it the way it wants to be called, so the test measures
            # what it does rather than failing on an arity error.
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
        # The branch under test must be the one the test names, and the
        # warning must name the authoritative event so the gap is
        # distinguishable from "a run exists but its suite is empty".
        self.assertIn(f"no canonical push ci.yml run for {'d' * 40}", completed.stdout)
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
