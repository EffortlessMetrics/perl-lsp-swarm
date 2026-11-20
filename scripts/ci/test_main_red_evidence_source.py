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

Three further contracts are pinned here.

The attempt is *verified*, not assumed. The run list is re-read before a
suite is fetched, because a re-run keeps the run id and moves the run to a
new check suite: the suite chosen from an earlier read can already be a
superseded attempt, and its rows name the same run id, so the run binding
alone would admit them as the current attempt's evidence.

A degraded subject is not a green subject. A run whose check suite carries
no check run at all never executed, and that stays a non-blocking evidence
gap. A run whose check suite carries check runs but none of them name the
run reported jobs whose own verdicts are unreadable: that is a
gate-affecting condition with its own exit status, so the probe fails the
required context instead of passing with warnings.

The call sites are checked in order. The reader's arguments are a contract,
not a set of tokens: a probe that invokes the reader with the main and
candidate events swapped satisfies "the event variable appears somewhere"
while binding every subject to the wrong run.
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
# The ordered arguments of the reader invocations in the probe. The
# validator checks the order, not the presence of a token: a reader
# invoked with the two events swapped still names both event variables,
# so a presence check accepts it while every subject is bound to the
# wrong run.
MAIN_INVOCATION_ARGS = (
    "$probe_dir/main-workflow-runs.json",
    "${MAIN_SHA_BEFORE}",
    "$probe_dir/main-runs.json",
    "main",
    "${MAIN_EVENT}",
)
CANDIDATE_INVOCATION_ARGS = (
    "$probe_dir/candidate-workflow-runs.json",
    "${CANDIDATE_SHA}",
    "$probe_dir/candidate-runs.json",
    "candidate",
    "${CANDIDATE_EVENT}",
)
# The poll loop's wall-clock budget. The block that declares it and
# bounds the loop with it is extracted and executed by the poll-bound
# proof.
POLL_BUDGET_START = "MAIN_RED_JOB_TIMEOUT_SECONDS="

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
# A suite read that did not reach the count the endpoint reported. The
# endpoint returns total_count beside its rows, so a suite that grows past
# one page would be truncated exactly this way, and the loss is invisible
# unless the reported count is reconciled against the rows actually read.
TRUNCATED_SUITE = 120361150443
TRUNCATED_RUN = 44464451999
TRUNCATED_REPORTED_COUNT = 24
TRUNCATED_PARSED_COUNT = 3
# Two canonical runs of one subject and event. A suite can hold rows that
# name the other run, and those rows are not evidence about the run whose
# suite it is.
OWN_RUN = 44464452000
OWN_SUITE = 120361150445
FOREIGN_RUN = 44464452100
FOREIGN_SUITE = 120361150446
# The stale-attempt shape. The run list read first names the superseded
# attempt's suite; the re-read names the attempt the run is on now. The
# superseded attempt's rows name the same run id, so only the re-read
# tells the two apart.
STALE_RUN = 44464453000
STALE_SUITE = 120361150460
CURRENT_SUITE = 120361150461
# A run whose check suite carries check runs that are not about it: the
# run reported jobs, and its own verdicts are unreadable.
DEGRADED_RUN = 44464454000
DEGRADED_SUITE = 120361150470
# The exit status the reader gives a degraded subject. It is distinct
# from 0 (evidence written) and from 1 (an evidence gap that stays
# non-blocking).
GATE_AFFECTING_EXIT = 3
# A run whose check suite carries nothing at all: the run never executed.
NEVER_RAN_RUN = 44464455000
NEVER_RAN_SUITE = 120361150480


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


def invocation_arguments(invocation: str) -> tuple[str, ...]:
    """Return one reader invocation's quoted arguments, in order.

    The program path is quoted before the matched text starts, so the
    arguments are the quoted tokens the match carries.
    """
    return tuple(re.findall(r'"([^"\n]*)"', invocation))


def probe_script(workflow: str) -> str:
    """Return the probe step's shell script, dedented.

    The proof executes the shipped shell rather than a restatement of it,
    so the script is taken from the step exactly as the runner sees it.
    """
    lines = probe_block(workflow).split("\n")
    start = None
    for index, line in enumerate(lines):
        if line.strip() == "run: |":
            start = index + 1
            break
    if start is None:
        raise AssertionError("the probe step must ship a run script")
    body: list[str] = []
    base: int | None = None
    for line in lines[start:]:
        if not line.strip():
            body.append("")
            continue
        indent = len(line) - len(line.lstrip())
        if base is None:
            if indent == 0:
                break
            base = indent
        if indent < base:
            break
        body.append(line[base:])
    return "\n".join(body)


def poll_budget_block(workflow: str) -> str | None:
    """Return the probe's poll-budget block, or ``None`` when it has none.

    The block declares the poll budget and bounds the poll loop with it.
    It is extracted verbatim so the proof can execute the shipped shell
    rather than a restatement of it.
    """
    script = probe_script(workflow)
    start = script.find(POLL_BUDGET_START)
    if start < 0:
        return None
    return script[start:]


def bash_script(script: str) -> str:
    """Run a shell script and return its standard output.

    ``bash`` is used where the host has it; a Windows host reaches its
    WSL bash the same way. The script arrives on stdin, so it needs no
    path translation, and it is sent as bytes: text mode would rewrite
    the line endings on the way out.
    """
    failures: list[str] = []
    ran = False
    # The shipped files carry the checkout's line endings; the shell
    # reads one command per line, so the script is normalised first.
    script = script.replace("\r\n", "\n").replace("\r", "\n")
    for command in (["bash", "-s"], ["wsl", "bash", "-s"]):
        try:
            completed = subprocess.run(
                command,
                input=script.encode("utf-8"),
                capture_output=True,
                check=False,
                timeout=300,
            )
        except OSError as error:  # pragma: no cover - host without bash
            failures.append(f"{command[0]}: {error}")
            continue
        except subprocess.TimeoutExpired:  # pragma: no cover - stuck shell
            failures.append(f"{command[0]}: timed out")
            continue
        ran = True
        stdout = completed.stdout.decode("utf-8", "replace")
        stderr = completed.stderr.decode("utf-8", "replace")
        if "PROBE_RESULT" in stdout:
            return stdout
        failures.append(
            f"{command[0]}: rc={completed.returncode} out={stdout!r} err={stderr!r}"
        )
    if ran:
        raise AssertionError(
            "the probe harness reported no result: " + "; ".join(failures)
        )
    raise unittest.SkipTest(f"no usable bash on this host: {'; '.join(failures)}")


def probe_result(stdout: str) -> dict[str, str]:
    """Return the probe harness's reported values."""
    for line in stdout.splitlines():
        if line.startswith("PROBE_RESULT"):
            fields = {}
            for item in line.split()[1:]:
                name, _, value = item.partition("=")
                fields[name] = value
            return fields
    raise AssertionError(f"the probe harness reported no result: {stdout!r}")


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
    # #19567: the arguments are a contract, not a set of tokens. The
    # ordered argument list of every reader invocation is checked, so a
    # probe that invokes the reader with the main and candidate events
    # swapped is rejected: both event variables are still named somewhere
    # in the probe, so a presence check cannot tell the two apart.
    arguments = [invocation_arguments(invocation) for invocation in invocations]
    if len(arguments) != 3:
        raise AssertionError(
            "the probe must invoke the evidence reader exactly three times: once "
            "for main, once for the candidate, and once for the candidate poll"
        )
    if arguments.count(MAIN_INVOCATION_ARGS) != 1:
        raise AssertionError(
            "the probe must invoke the evidence reader once for main with the main "
            "subject SHA, the main output path, the main label and the main event, "
            "in that order"
        )
    if arguments.count(CANDIDATE_INVOCATION_ARGS) != 2:
        raise AssertionError(
            "the probe must invoke the evidence reader twice for the candidate, "
            "for the initial read and the poll read, with the candidate subject "
            "SHA, the candidate output path, the candidate label and the "
            "candidate event, in that order"
        )
    # #19567: a suite read must be reconciled against the count the
    # endpoint reported, and every kept row must be bound to the suite it
    # was read from and to the run whose suite that is. A read that did not
    # reach the reported count was truncated, and a truncated suite is
    # unreadable rather than partially good.
    for needle in (
        "suite_count_reconciliation",
        "row_suite_id",
        "select_suite_runs",
    ):
        if needle not in program:
            raise AssertionError(f"the evidence reader must use {needle}")
    # #19567: the attempt is verified against a re-read of the run list,
    # and a degraded subject has a status of its own so the probe can fail
    # the required context instead of passing with warnings.
    for needle in (
        "fetch_workflow_runs",
        "current_suite_ids",
        "GATE_AFFECTING_EXIT",
    ):
        if needle not in program:
            raise AssertionError(f"the evidence reader must use {needle}")
    if "GATE_AFFECTING_EXIT = 3" not in program:
        raise AssertionError(
            "the evidence reader must give the gate-affecting status its own value"
        )


def shard_run(
    name: str,
    run_id: int,
    conclusion: str = "success",
    head_sha: str = "a" * 40,
    suite_id: int | None = None,
) -> dict:
    """Return one observed check run in the API's own shape.

    The endpoint stamps every row with the check suite it came from, so a
    row the reader would keep from a suite read must carry that suite.
    """
    row = {
        "name": name,
        "status": "completed",
        "conclusion": conclusion,
        "details_url": f"https://github.com/o/r/actions/runs/{run_id}/job/1",
        "head_sha": head_sha,
        "app": {"id": 15368, "slug": "github-actions"},
    }
    if suite_id is not None:
        row["check_suite"] = {"id": suite_id}
    return row


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
                        suite_id=LIVE_DISPATCH_SUITE,
                    ),
                    shard_run(
                        "CI Gate shard (policy)",
                        LIVE_DISPATCH_RUN,
                        head_sha=LIVE_SHA,
                        suite_id=LIVE_DISPATCH_SUITE,
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
        fresh_runs_payload: dict | None = None,
    ):
        """Run a reader program against fixture suites.

        Returns ``(status, stdout, payload text, fetched suite ids)``. The
        suite fetch is replaced so the test observes which suites the
        reader chose without touching the network, and the run-list re-read
        is replaced too: the re-read is how the reader verifies that the
        suite it fetched is the run's current attempt, so a reader that
        does not re-read at all is measured on what it admits.

        ``fresh_runs_payload`` is the re-read's answer. It defaults to the
        payload the reader was given, which is the state in which nothing
        has been superseded.
        """
        namespace = load_program_text(program)
        fetched: list[int] = []

        def fake_fetch(repository: str, suite_id: int):
            fetched.append(suite_id)
            if suite_id not in suites:
                raise RuntimeError(f"check suite {suite_id} could not be read")
            return suites[suite_id]

        def fake_fetch_workflow_runs(repository: str, subject_sha: str):
            if fresh_runs_payload is None:
                return runs_payload
            return fresh_runs_payload

        namespace["fetch_suite_check_runs"] = fake_fetch
        if "fetch_workflow_runs" in namespace:
            namespace["fetch_workflow_runs"] = fake_fetch_workflow_runs
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

    # -- #19567: a suite read is reconciled against the count it reported.

    @staticmethod
    def _truncated_runs() -> dict:
        """Return the workflow-run payload of a subject with one suite."""
        return {
            "workflow_runs": [
                {
                    "id": TRUNCATED_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": TRUNCATED_SUITE,
                    "run_attempt": 1,
                }
            ]
        }

    @staticmethod
    def _truncated_suites() -> dict:
        """Return a suite response that reports more runs than it carries."""
        return {
            TRUNCATED_SUITE: {
                "check_runs": [
                    shard_run(
                        f"CI Gate shard (shard{index})",
                        TRUNCATED_RUN,
                        head_sha=LIVE_SHA,
                        suite_id=TRUNCATED_SUITE,
                    )
                    for index in range(TRUNCATED_PARSED_COUNT)
                ],
                "total_count": TRUNCATED_REPORTED_COUNT,
            }
        }

    def test_evidence_reader_rejects_a_truncated_suite_read(self) -> None:
        """A read that did not reach the reported count is unreadable.

        The endpoint reports ``total_count`` beside its rows. A read that
        parsed fewer rows than the endpoint reported was truncated, and a
        truncated suite is unreadable rather than partially good: it must
        contribute nothing and must not be written as valid evidence.

        Discriminating assertion. It fails unless the reader reconciles the
        reported count against the rows it parsed: without the
        reconciliation the surviving rows are written as the candidate's
        evidence, so the truncation reads as a complete verdict.
        """
        status, stdout, written, fetched = self._run_reader_program(
            evidence_program(self.workflow),
            self._truncated_runs(),
            self._truncated_suites(),
            "pull_request",
        )
        self.assertIn(TRUNCATED_SUITE, fetched)
        # The truncated suite contributes nothing: the reader degrades to
        # the missing-evidence payload rather than writing a short
        # population as a valid observation.
        self.assertEqual(written, "")
        self.assertNotEqual(status, 0)
        self.assertIn("::warning::", stdout)
        # The warning names the count mismatch, so the gap is
        # distinguishable from a suite that was simply empty.
        self.assertIn("count mismatch", stdout)
        self.assertIn(f"reported {TRUNCATED_REPORTED_COUNT} check runs", stdout)
        self.assertIn(f"only {TRUNCATED_PARSED_COUNT} were read", stdout)

    def test_evidence_reader_count_reconciliation_discriminates_the_truncation(self) -> None:
        """Removing the reconciliation writes the truncated read as evidence.

        The mutation drops the count check, so a short read is written as
        the candidate's evidence. This proves the assertion above
        discriminates the reconciliation rather than the fixture.
        """
        program = evidence_program(self.workflow)
        mutated = program.replace("if parsed < reported:", "if False:")
        if mutated == program:
            self.fail(
                "the count-reconciliation mutation did not apply; update the mutation anchor"
            )
        status, _, written, _ = self._run_reader_program(
            mutated,
            self._truncated_runs(),
            self._truncated_suites(),
            "pull_request",
        )
        self.assertEqual(status, 0)
        self.assertIsNotNone(written)
        self.assertIn("CI Gate shard", written)

    # -- #19567: a suite's rows are evidence about its own run.

    @staticmethod
    def _foreign_rows_runs() -> dict:
        """Return the workflow-run payload of a subject with two runs."""
        return {
            "workflow_runs": [
                {
                    "id": OWN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": OWN_SUITE,
                    "run_attempt": 1,
                },
                {
                    "id": FOREIGN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": FOREIGN_SUITE,
                    "run_attempt": 1,
                },
            ]
        }

    @staticmethod
    def _foreign_rows_suites() -> dict:
        """Return a suite holding rows that name the other run."""
        return {
            OWN_SUITE: {
                "check_runs": [
                    shard_run(
                        "CI Gate shard (meta)",
                        FOREIGN_RUN,
                        head_sha=LIVE_SHA,
                        suite_id=OWN_SUITE,
                    )
                ]
            },
            FOREIGN_SUITE: {"check_runs": []},
        }

    def test_evidence_reader_drops_rows_that_name_another_run(self) -> None:
        """A suite's rows are evidence about the run whose suite it is.

        The suite read for the canonical run holds a row that names the
        other admitted run. That row is not evidence about the canonical
        run, so it must be dropped, and the reader must degrade to the
        missing-evidence path rather than write it as the canonical run's
        verdicts.

        Discriminating assertion. It fails unless the reader verifies each
        fetched suite against its own run: without the verification the
        foreign row is written as the canonical run's evidence, because the
        foreign run is itself admitted.
        """
        status, stdout, written, _ = self._run_reader_program(
            evidence_program(self.workflow),
            self._foreign_rows_runs(),
            self._foreign_rows_suites(),
            "pull_request",
        )
        self.assertEqual(written, "")
        self.assertNotEqual(status, 0)
        self.assertIn("::warning::", stdout)
        # The foreign row is not in the payload: the payload is the
        # missing-evidence file, not a short observation.
        self.assertNotIn("CI Gate shard", written)

    def test_evidence_reader_run_binding_discriminates_the_foreign_rows(self) -> None:
        """Removing the run binding writes the foreign rows as evidence.

        The mutation keeps the suite binding and drops the run binding, so
        a row that names another admitted run is written as the canonical
        run's evidence. This proves the assertion above discriminates the
        per-suite run binding rather than the fixture.
        """
        program = evidence_program(self.workflow)
        mutated = program.replace(
            "if match is None or int(match.group(1)) != run_id:",
            "if match is None:",
        )
        if mutated == program:
            self.fail(
                "the per-suite run-binding mutation did not apply; update the mutation anchor"
            )
        status, _, written, _ = self._run_reader_program(
            mutated,
            self._foreign_rows_runs(),
            self._foreign_rows_suites(),
            "pull_request",
        )
        self.assertEqual(status, 0)
        self.assertIsNotNone(written)
        self.assertIn("CI Gate shard (meta)", written)

    # -- #19567: the attempt is verified against a re-read of the run list.

    @staticmethod
    def _stale_runs() -> dict:
        """Return the run list read first: the superseded attempt's suite."""
        return {
            "workflow_runs": [
                {
                    "id": STALE_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": STALE_SUITE,
                    "run_attempt": 1,
                }
            ]
        }

    @staticmethod
    def _stale_fresh_runs() -> dict:
        """Return the re-read: the attempt the run is on now."""
        return {
            "workflow_runs": [
                {
                    "id": STALE_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": CURRENT_SUITE,
                    "run_attempt": 2,
                }
            ]
        }

    @staticmethod
    def _stale_suites() -> dict:
        """Return the superseded attempt's suite and the current one."""
        return {
            STALE_SUITE: {
                "check_runs": [
                    shard_run(
                        "CI Gate shard (superseded)",
                        STALE_RUN,
                        head_sha=LIVE_SHA,
                        suite_id=STALE_SUITE,
                    )
                ]
            },
            CURRENT_SUITE: {
                "check_runs": [
                    shard_run(
                        "CI Gate shard (current)",
                        STALE_RUN,
                        head_sha=LIVE_SHA,
                        suite_id=CURRENT_SUITE,
                    )
                ]
            },
        }

    def test_evidence_reader_rejects_a_stale_suite(self) -> None:
        """A superseded attempt's suite is not the current attempt's evidence.

        A re-run keeps the run id and moves the run to a new check suite.
        The superseded attempt's rows name the same run id, so only a
        re-read of the run list can tell the two attempts apart.

        Discriminating assertion. It fails unless the reader re-reads the
        run list and compares the run's current suite with the suite it
        fetched: without that comparison the superseded attempt's rows are
        written as the current attempt's evidence.
        """
        status, stdout, written, _ = self._run_reader_program(
            evidence_program(self.workflow),
            self._stale_runs(),
            self._stale_suites(),
            "pull_request",
            fresh_runs_payload=self._stale_fresh_runs(),
        )
        self.assertEqual(status, 0)
        self.assertIsNotNone(written)
        # The superseded attempt's rows are not the evidence.
        self.assertNotIn("superseded", written)
        # The current attempt's rows are.
        self.assertIn("current", written)
        # The stale condition is named, so it is distinguishable from a
        # suite that was simply empty.
        self.assertIn("stale", stdout)

    def test_evidence_reader_stale_attempt_discriminates_the_superseded_suite(self) -> None:
        """Removing the stale-suite check writes the superseded rows.

        The mutation drops the comparison between the run's current suite
        and the suite that was chosen, so the superseded attempt's suite
        is read and its rows become the candidate's evidence. This proves
        the assertion above discriminates the re-read rather than the
        fixture.
        """
        program = evidence_program(self.workflow)
        mutated = program.replace("if current_suite_id != suite_id:", "if False:")
        if mutated == program:
            self.fail("the stale-suite mutation did not apply; update the mutation anchor")
        status, _, written, _ = self._run_reader_program(
            mutated,
            self._stale_runs(),
            self._stale_suites(),
            "pull_request",
            fresh_runs_payload=self._stale_fresh_runs(),
        )
        self.assertEqual(status, 0)
        self.assertIsNotNone(written)
        self.assertIn("superseded", written)

    def test_evidence_reader_contributes_nothing_when_the_current_suite_cannot_be_read(self) -> None:
        """A stale suite whose replacement cannot be read contributes nothing.

        The superseded attempt's suite is not the run's evidence, and the
        run's current suite cannot be read, so the pair contributes
        nothing at all: the reader degrades to the missing-evidence
        payload rather than writing a stale or partial population.
        """
        status, stdout, written, _ = self._run_reader_program(
            evidence_program(self.workflow),
            self._stale_runs(),
            {STALE_SUITE: self._stale_suites()[STALE_SUITE]},
            "pull_request",
            fresh_runs_payload=self._stale_fresh_runs(),
        )
        self.assertEqual(written, "")
        self.assertNotEqual(status, 0)
        self.assertIn("could not be read", stdout)

    # -- #19567: a degraded subject is not a green subject.

    @staticmethod
    def _degraded_runs() -> dict:
        """Return the run list of a run whose own rows are absent."""
        return {
            "workflow_runs": [
                {
                    "id": DEGRADED_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": DEGRADED_SUITE,
                    "run_attempt": 1,
                }
            ]
        }

    @staticmethod
    def _degraded_suites() -> dict:
        """Return a suite that carries check runs that are not about the run."""
        return {
            DEGRADED_SUITE: {
                "check_runs": [
                    shard_run(
                        "CI Gate shard (foreign)",
                        FOREIGN_RUN,
                        head_sha=LIVE_SHA,
                        suite_id=DEGRADED_SUITE,
                    )
                ]
            }
        }

    @staticmethod
    def _never_ran_runs() -> dict:
        """Return the run list of a run whose check suite is empty."""
        return {
            "workflow_runs": [
                {
                    "id": NEVER_RAN_RUN,
                    "event": "pull_request",
                    "head_sha": LIVE_SHA,
                    "check_suite_id": NEVER_RAN_SUITE,
                    "run_attempt": 1,
                }
            ]
        }

    @staticmethod
    def _never_ran_suites() -> dict:
        """Return an empty check suite: the run never executed."""
        return {NEVER_RAN_SUITE: {"check_runs": []}}

    def test_evidence_reader_reports_a_degraded_subject_as_gate_affecting(self) -> None:
        """A run whose own check rows are absent is degraded, not green.

        The run's check suite carries check runs, so the run reported jobs,
        but none of them name the run: its own verdicts are unreadable.
        The reader gives that condition its own status, so the probe can
        fail the required context instead of passing with warnings, and it
        writes the missing-evidence payload so the classifier still reads
        "no evidence" rather than a valid payload that merely lacks the
        shard.

        Discriminating assertion. It fails unless the reader tells the two
        gaps apart: the pre-repair reader degrades every gap the same way,
        so a degraded subject read as a green subject.
        """
        program = evidence_program(self.workflow)
        self.assertIn(
            f"GATE_AFFECTING_EXIT = {GATE_AFFECTING_EXIT}",
            program,
            "the reader must give the gate-affecting status its own value",
        )
        status, stdout, written, _ = self._run_reader_program(
            program,
            self._degraded_runs(),
            self._degraded_suites(),
            "pull_request",
        )
        self.assertEqual(status, GATE_AFFECTING_EXIT)
        self.assertNotEqual(status, 1)
        self.assertEqual(written, "")
        self.assertIn("::warning::", stdout)
        # The warning names the condition, so it is distinguishable from
        # a run that never executed.
        self.assertIn("none of them named that run", stdout)

    def test_evidence_reader_keeps_a_never_executed_run_non_blocking(self) -> None:
        """A run whose check suite carries nothing never executed.

        That is an evidence gap and it stays a non-blocking one: the run
        never executed, which is #19036's subject (the advisory aggregate
        publishing success when shards never executed) and is not this
        probe's to change. The reader keeps the ordinary degraded status
        for it, so the probe does not fail the required context.

        This is the control for the gate-affecting assertion above: the
        two gaps must not be conflated in either direction.
        """
        status, stdout, written, _ = self._run_reader_program(
            evidence_program(self.workflow),
            self._never_ran_runs(),
            self._never_ran_suites(),
            "pull_request",
        )
        # The ordinary degraded status, not the gate-affecting one.
        self.assertEqual(status, 1)
        self.assertEqual(written, "")
        self.assertIn("::warning::", stdout)
        self.assertNotIn("none of them named that run", stdout)

    # -- #19567: the reader's arguments are a contract, not a set of tokens.

    def test_event_binding_validator_rejects_swapped_reader_arguments(self) -> None:
        """The validator rejects a reader invoked with the events swapped.

        The weak check was "the event variable appears somewhere in the
        probe", which a swap satisfies: both variables are still named.
        The validator now checks the ordered argument list of each
        invocation, so the swap is rejected while the shipped wiring is
        accepted.
        """
        validate_evidence_source(self.workflow)
        swapped = (
            self.workflow.replace('"${MAIN_EVENT}"', '"@@SWAPPED_EVENT@@"')
            .replace('"${CANDIDATE_EVENT}"', '"${MAIN_EVENT}"')
            .replace('"@@SWAPPED_EVENT@@"', '"${CANDIDATE_EVENT}"')
        )
        self.assertNotEqual(swapped, self.workflow)
        # The weak property the swap preserves: both event variables are
        # still named somewhere in the probe.
        self.assertIn('"${MAIN_EVENT}"', swapped)
        self.assertIn('"${CANDIDATE_EVENT}"', swapped)
        with self.assertRaisesRegex(AssertionError, "in that order"):
            validate_evidence_source(swapped)

    def _run_poll_block(self, block: str, sleep_seconds: int) -> dict[str, str]:
        """Execute the probe's poll block and return its reported values.

        The shipped shell is executed with the network and the classifier
        replaced by stubs, so the bound is observed rather than asserted:
        the stubbed sleep advances the shell's own clock, which is what
        the budget is measured against.
        """
        template = """set -u
REPOSITORY="EffortlessMetrics/perl-lsp-swarm"
GITHUB_RUN_ID="1"
CANDIDATE_SHA="__CANDIDATE_SHA__"
CANDIDATE_EVENT="pull_request"
EFFECTIVE_WORKFLOW_TREE=""
MAIN_SHA_AFTER=""
refusal_status=2
final_calls=0
exit_code=0
probe_dir="$(mktemp -d)"
job_started_at="$(date -u -d '-300 seconds' +%Y-%m-%dT%H:%M:%SZ)"
exit() {
  echo "PROBE_EXIT $1"
  exit_code=$1
}
gh() {
  case "$*" in
    *"/jobs?"*) printf '%s\\n' "$job_started_at" ;;
    *) printf '%s\\n' '{"workflow_runs":[]}' ;;
  esac
}
sleep() {
  SECONDS=$((SECONDS + __SLEEP_SECONDS__))
}
read_main_sha() {
  printf '%s\\n' "__MAIN_SHA__"
}
read_workflow_sha() {
  printf '%s\\n' "__WORKFLOW_SHA__"
}
python3() {
  :
}
run_refusal() {
  if [ "${1:-}" = "--final" ]; then
    final_calls=$((final_calls + 1))
    return 1
  fi
  return 2
}
__POLL_BLOCK__
echo "PROBE_RESULT polls=$poll_attempt final=$final_calls status=$refusal_status exit=$exit_code budget=$MAIN_RED_POLL_BUDGET_SECONDS"
"""
        script = (
            template.replace("__POLL_BLOCK__", block)
            .replace("__SLEEP_SECONDS__", str(sleep_seconds))
            .replace("__CANDIDATE_SHA__", "a" * 40)
            .replace("__MAIN_SHA__", "d" * 40)
            .replace("__WORKFLOW_SHA__", "e" * 40)
        )
        return probe_result(bash_script(script))

    def test_probe_poll_loop_is_bounded_by_wall_clock(self) -> None:
        """The poll loop stops inside the job's budget and still states a verdict.

        The loop was bounded by iteration count only, so a slow API ran it
        past the job's timeout and the job was killed mid-poll: the
        required context ended with no verdict at all.

        The shipped block is executed with a slow stubbed clock. The
        budget must stop the loop before the iteration cap, and the final
        classifier call must still run and still fail the step.
        """
        block = poll_budget_block(self.workflow)
        if block is None:
            self.fail(
                "the probe must bound the poll loop by elapsed wall clock; "
                "no poll budget is declared"
            )
        result = self._run_poll_block(block, sleep_seconds=600)
        polls = int(result["polls"])
        budget = int(result["budget"])
        # The wall-clock bound stopped the loop, not the iteration cap.
        self.assertLess(polls, 50)
        self.assertGreater(polls, 0)
        # The loop stayed inside the budget, allowing the one iteration
        # that was already running when the deadline was reached.
        self.assertLessEqual(polls * 600, budget + 600)
        # The budget leaves the job's timeout for the final call.
        self.assertLessEqual(budget, 2100 - 180)
        # The final classifier call still ran, and its verdict still
        # failed the step.
        self.assertEqual(result["final"], "1")
        self.assertEqual(result["exit"], "1")

    def test_probe_poll_loop_preserves_the_intended_wait_budget(self) -> None:
        """A fast API still gets the intended wait.

        The wall-clock bound must not shorten the wait: the candidate's
        shard lanes have been observed taking 18-20 minutes, and a
        shorter wait would fail-close legitimate pull requests whose CI is
        still running. With a fast clock the iteration cap governs and
        the loop still polls 50 times.
        """
        block = poll_budget_block(self.workflow)
        if block is None:
            self.fail(
                "the probe must bound the poll loop by elapsed wall clock; "
                "no poll budget is declared"
            )
        result = self._run_poll_block(block, sleep_seconds=30)
        self.assertEqual(result["polls"], "50")
        self.assertEqual(result["final"], "1")
        self.assertEqual(result["exit"], "1")

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
