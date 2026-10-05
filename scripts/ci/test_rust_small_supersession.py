#!/usr/bin/env python3
"""Supersession-discriminator contract for the required Rust Small aggregate (#16187).

A second push to a pull request cancels this workflow's in-flight lanes through
the concurrency group's `cancel-in-progress`, and the cancelled lane used to be
reported as a red required check whose cause could not be stated — the gap
#5460 recorded for the routed gates, with the same mechanism #16087 recorded
for ci.yml's advisory aggregate. The discriminator #16186 built there is
ported into `em-ci-routed-rust.yml`, and this file owns its contract for
`Perl LSP Rust Small Result`:

- a resolve step binds this run's tested head, the pull request's live head,
  and a replacement run of this workflow for the pull request on the live head,
  keyed on `github.run_id` and the pull request number — never on
  candidate-controlled data (`github.head_ref` is fork-controlled, #5981; the
  event payload's head sha is candidate-controlled, #12911);
- the cancelled branch splits on that evidence: `superseded` requires both
  heads to be well-formed 40-hex object names that differ AND a bound
  replacement run id; an unresolved or malformed value reads as unknown, never
  as "different, therefore newer";
- a genuine lane failure alongside a cancellation is a failure;
- every branch stays red. #5460's rule that no proof never becomes green is
  unchanged, so no forgiven head carries a green a later force-push could
  resurrect — the #16187 blocking precondition.

Workflow-text inspection detects the listed wiring and single-clause text
mutations by name. Separate shell controls execute the actual resolver with
stubbed API responses and real jq, and the actual evaluator with supplied lane
and head evidence. They cover trusted-base filter provenance, mixed
preflight/cancellation results and executable candidate-filter/green/errexit
mutants. They cannot establish live API or hand-cancel acceptance observations.
"""

from __future__ import annotations

import base64
import json
import os
import re
import shlex
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"

RESULT_JOB = "rust-small-result"
RESOLVE_STEP = "Resolve the tested head, the live head, and the replacement run"
EVALUATE_STEP = "Evaluate routed result"
IDENTITY_FILTER = "scripts/ci/replacement_run_filter.jq"
PARTITION_START = "any_cancelled=false"
PARTITION_END = 'case "${ROUTER_TARGET:-}" in'
OBJECT_NAME = re.compile(r"^[0-9a-f]{40}$")
RUN_ID = re.compile(r"^[0-9]+$")


def _workflow_text() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def _job_body(text: str, job: str) -> str:
    """Lines of one top-level job, from its two-space key to the next key."""
    start = text.index(f"\n  {job}:\n") + 1
    rest = text[start:]
    lines = rest.splitlines(keepends=True)
    end = len(lines)
    for i, line in enumerate(lines[1:], start=1):
        if re.match(r"^  [a-zA-Z0-9_-]+:\s*(#|$)", line):
            end = i
            break
    return "".join(lines[:end])


def _step_body(job: str, name: str) -> str:
    """Lines of one named step, from its `      - name:` to the next step."""
    start = job.index(f"      - name: {name}")
    lines = job[start:].splitlines(keepends=True)
    end = len(lines)
    for i, line in enumerate(lines[1:], start=1):
        if line.startswith("      - name: ") or re.match(r"^  [a-zA-Z0-9_-]+:", line):
            end = i
            break
    return "".join(lines[:end])


def _partition_block(evaluate: str) -> str:
    """The cancelled-lane partition: from the lane scan to the route case."""
    start = evaluate.index(PARTITION_START)
    end = evaluate.index(PARTITION_END)
    return evaluate[start:end]


def _evaluate_script(text: str) -> str:
    """The actual production shell block, without YAML indentation."""
    step = _step_body(_job_body(text, RESULT_JOB), EVALUATE_STEP)
    return textwrap.dedent(step.split("        run: |\n", 1)[1])


def _resolve_script(text: str) -> str:
    step = _step_body(_job_body(text, RESULT_JOB), RESOLVE_STEP)
    return textwrap.dedent(step.split("        run: |\n", 1)[1])


def discriminator_violations(text: str) -> list[str]:
    """Workflow-text wiring and wording violations of the discriminator contract."""
    violations: list[str] = []
    try:
        result_job = _job_body(text, RESULT_JOB)
        resolve = _step_body(result_job, RESOLVE_STEP)
        evaluate = _step_body(result_job, EVALUATE_STEP)
        partition = _partition_block(evaluate)
    except (ValueError, IndexError) as exc:
        return [f"discriminator surfaces missing from the workflow: {exc}"]

    if result_job.index(f"      - name: {RESOLVE_STEP}") > result_job.index(
        f"      - name: {EVALUATE_STEP}"
    ):
        violations.append("resolve step must run before the evaluate step")

    gate = "if: always() && github.event_name == 'pull_request'"
    if gate not in resolve:
        violations.append(
            "resolve step must be gated on always() && pull_request so the "
            "API lookups run on the cancelled aggregate, not on merge_group"
        )
    # Comments may name the dangerous patterns; code may not use them. The
    # evaluate step's pre-existing event-payload reads are other claims'
    # contracts — the discriminator must simply not add new ones.
    resolve_code = "\n".join(
        line for line in resolve.splitlines() if not line.lstrip().startswith("#")
    )
    if "${{ github.token }}" not in resolve_code:
        violations.append("resolve step must authenticate with github.token")
    if "${{ secrets." in resolve_code:
        violations.append("resolve step must not reference an untrusted secret")
    if "RUN_ID: ${{ github.run_id }}" not in resolve:
        violations.append("resolve step must key the run read on github.run_id")
    if "PR_NUMBER: ${{ github.event.pull_request.number }}" not in resolve:
        violations.append(
            "resolve step must key the pull request on its number, not a "
            "candidate-controlled branch name (#5981)"
        )
    for pattern, label in (
        ("github.head_ref", "fork-controlled branch name (#5981)"),
        ("github.event.pull_request.head.sha", "candidate-controlled event head sha (#12911)"),
    ):
        if pattern in resolve_code or pattern in partition:
            violations.append(
                f"discriminator must not read the {label}; both heads come "
                "from the API keyed on run id and pull request number"
            )
    if ".head_sha" not in resolve or ".workflow_id" not in resolve:
        violations.append(
            "resolve step must bind tested head and workflow id from this "
            "run's own object"
        )
    if "head_sha=${latest}" not in resolve:
        violations.append("replacement lookup must query the live head")
    if "${workflow_id}" not in resolve:
        violations.append("replacement lookup must be bound to this workflow")
    if (f"contents/{IDENTITY_FILTER}?ref=${{BASE_SHA}}" not in resolve
            or '-f "$trusted_filter"' not in resolve
            or f"-f {IDENTITY_FILTER}" in resolve):
        violations.append(
            "replacement identity must come from the shared filter file at the "
            "trusted base, never candidate content or an inline copy"
        )
    if "BASE_SHA: ${{ github.event.pull_request.base.sha }}" not in resolve:
        violations.append("resolve step must bind the immutable trusted base")
    for clause, label in (
        ('[[ "${latest}" =~ ^[0-9a-f]{40}$ ]]', "live head 40-hex shape"),
        ('[[ "${tested}" =~ ^[0-9a-f]{40}$ ]]', "tested head 40-hex shape"),
        ('[[ "${latest}" != "${tested}" ]]', "heads must differ"),
    ):
        if clause not in resolve:
            violations.append(f"replacement lookup precondition lost: {label}")
    if '[[ "${pair#*=}" =~ ^[0-9a-f]{40}$ ]]' not in resolve:
        violations.append(
            "head exports must be gated on the 40-hex shape — an unvalidated "
            "value would read as a supersession that did not happen"
        )
    if "REPLACEMENT_RUN_ID=${replacement}" not in resolve or "^[0-9]+$" not in resolve:
        violations.append("replacement run id must be exported only well-formed")

    for clause, label in (
        ('[[ "$run_head" =~ ^[0-9a-f]{40}$ ]]', "tested head 40-hex shape"),
        ('[[ "$latest_head" =~ ^[0-9a-f]{40}$ ]]', "live head 40-hex shape"),
        ('[ "$run_head" != "$latest_head" ]', "heads must differ"),
        ('[[ "$replacement" =~ ^[0-9]+$ ]]', "replacement run digits shape"),
    ):
        if clause not in partition:
            violations.append(f"superseded predicate clause lost: {label}")

    if "RUST_SMALL_GATE_VERDICT=superseded" not in partition:
        violations.append("superseded verdict marker missing")
    superseded_to_no_verdict = partition.split("RUST_SMALL_GATE_VERDICT=superseded", 1)[1]
    superseded_slice = superseded_to_no_verdict.split(
        "RUST_SMALL_GATE_VERDICT=cancelled-no-verdict", 1
    )[0]
    if "exit 1" not in superseded_slice:
        violations.append(
            "superseded branch must exit 1 before falling through to "
            "cancelled-no-verdict"
        )
    if "This is NOT a test failure" not in superseded_slice:
        violations.append("superseded message must say it is NOT a test failure")
    if "owns the verdict" not in superseded_slice:
        violations.append("superseded message must hand the verdict to the replacement run")
    if "${replacement}" not in superseded_slice or "${latest_head}" not in superseded_slice:
        violations.append("superseded message must name the replacement run and live head")

    no_verdict_slice = partition.split("RUST_SMALL_GATE_VERDICT=cancelled-no-verdict", 1)[1]
    if "no newer run of this workflow was identified" not in no_verdict_slice:
        violations.append(
            "cancelled-no-verdict must name only what the inputs support: no "
            "newer run identified"
        )
    if "${replacement}" in no_verdict_slice.split("exit 1", 1)[0]:
        violations.append(
            "cancelled-no-verdict must not name a replacement run it did not bind"
        )

    if "any_failure=true" not in partition:
        violations.append("partition must detect failures not classified as primary preflight")
    failure_slice = partition.split('if [ "$any_failure" = "true" ]; then', 1)[1].split(
        "exit 1", 1
    )[0]
    if "lane failed" not in failure_slice or "real test failure" in failure_slice:
        violations.append("failed-lane branch must name the failure without guessing its cause")
    if "NOT a test failure" in failure_slice:
        violations.append(
            "failed-lane branch must not borrow the cancelled NOT-a-failure wording"
        )

    if "exit 0" in partition:
        violations.append("no proof never becomes green: the partition must not exit 0")
    if "set -euo pipefail" not in evaluate:
        violations.append("evaluate must retain errexit for failed route assertions")
    for primary in ("cx53", "cx43"):
        binding = f"{primary.upper()}_PREFLIGHT_OK: ${{{{ needs.rust-small-{primary}.outputs.preflight_ok }}}}"
        if binding not in evaluate:
            violations.append(f"evaluate must bind {primary} preflight output")
    if "if: cancelled()" in result_job:
        violations.append(
            "cancelled() is not the discriminator (#16187 correction): the "
            "aggregate must classify on evidence, not on run state"
        )
    return violations


def _mutate_evaluate(text: str, old: str, new: str) -> str:
    step = _step_body(_job_body(text, RESULT_JOB), EVALUATE_STEP)
    return text.replace(step, step.replace(old, new, 1), 1)


def _green_partition_mutation(text: str) -> str:
    marker = 'echo "RUST_SMALL_GATE_VERDICT=superseded"'
    return _mutate_evaluate(text, marker, marker + "\n          exit 0")


MUTATIONS: tuple[tuple[str, str, Callable[[str], str], str], ...] = (
    (
        "head inequality dropped",
        "superseded fires on an unchanged head",
        lambda t: t.replace('[ "$run_head" != "$latest_head" ]', '[ "$run_head" = "$run_head" ]'),
        "heads must differ",
    ),
    (
        "tested head shape relaxed",
        "a malformed tested head reads as supersession",
        lambda t: t.replace('[[ "$run_head" =~ ^[0-9a-f]{40}$ ]] \\', '[[ -n "$run_head" ]] \\', 1),
        "tested head 40-hex shape",
    ),
    (
        "replacement digits check dropped",
        "a garbage replacement id is claimed as proof",
        lambda t: t.replace('&& [[ "$replacement" =~ ^[0-9]+$ ]]; then', '&& [ -n "$replacement" ]; then'),
        "replacement run digits shape",
    ),
    (
        "unvalidated head export",
        "an unresolved head would be believed",
        lambda t: t.replace('if [[ "${pair#*=}" =~ ^[0-9a-f]{40}$ ]]; then', 'if :; then'),
        "40-hex shape",
    ),
    (
        "identity filter duplicated inline",
        "the shared rule can drift from the advisory aggregate",
        lambda t: t.replace('-f "$trusted_filter"', "-f /dev/null"),
        "shared filter file",
    ),
    (
        "pull request keyed on a literal",
        "the lookup no longer keys on the pull request number",
        lambda t: t.replace("PR_NUMBER: ${{ github.event.pull_request.number }}", "PR_NUMBER: 1"),
        "pull request on its number",
    ),
    (
        "superseded branch grants green",
        "no proof becomes green",
        _green_partition_mutation,
        "must not exit 0",
    ),
    (
        "evaluator errexit removed",
        "failed route assertions fall through to success",
        lambda t: _mutate_evaluate(t, "set -euo pipefail", "set -uo pipefail"),
        "retain errexit",
    ),
    (
        "cancelled() taken as the discriminator",
        "run state replaces evidence",
        lambda t: t.replace(
            "if: always() && github.event_name == 'pull_request'", "if: cancelled()", 1
        ),
        "cancelled()",
    ),
)


class DiscriminatorHolds(unittest.TestCase):
    def test_current_workflow_carries_no_violation(self) -> None:
        self.assertEqual(discriminator_violations(_workflow_text()), [])


class WorkflowTextMutations(unittest.TestCase):
    def test_each_single_clause_mutation_is_caught_by_name(self) -> None:
        text = _workflow_text()
        for name, why, mutate, expected_keyword in MUTATIONS:
            with self.subTest(mutation=name):
                mutated = mutate(text)
                self.assertNotEqual(mutated, text, f"mutation did not apply: {name}")
                violations = discriminator_violations(mutated)
                self.assertTrue(
                    any(expected_keyword in v for v in violations),
                    f"{name} ({why}) was not caught by any violation naming "
                    f"'{expected_keyword}'; got: {violations}",
                )


class SharedFilterFileWiring(unittest.TestCase):
    def test_the_identity_filter_exists_and_both_aggregates_run_it(self) -> None:
        self.assertTrue((ROOT / IDENTITY_FILTER).is_file())
        routed = _step_body(_job_body(_workflow_text(), RESULT_JOB), RESOLVE_STEP)
        advisory = _workflow_text()
        ci_yml = ROOT / ".github" / "workflows" / "ci.yml"
        if ci_yml.is_file():
            advisory = ci_yml.read_text(encoding="utf-8")
        self.assertIn(f"contents/{IDENTITY_FILTER}?ref=${{BASE_SHA}}", routed)
        self.assertIn('-f "$trusted_filter"', routed)
        self.assertIn(f"-f {IDENTITY_FILTER}", advisory)


class ExecutableResolverControls(unittest.TestCase):
    """Execute the resolver with real jq, isolated candidate files and stubbed API data."""

    def _run_resolve(
        self, *, content: str = "trusted", base: str = "c" * 40,
        qualifying: bool = True, api_status: int = 0, script: str | None = None,
    ) -> tuple[dict[str, str], list[str]]:
        bash = os.environ.get("RUST_SMALL_TEST_BASH") or shutil.which("bash")
        self.assertIsNotNone(bash, "bash is required to execute the production resolver")
        self.assertIsNotNone(shutil.which("jq"), "jq is required to execute the production filter")
        trusted = (ROOT / IDENTITY_FILTER).read_bytes()
        if content == "trusted":
            content = base64.b64encode(trusted).decode("ascii")
        run = json.dumps({"head_sha": "a" * 40, "workflow_id": 7,
                          "created_at": "2026-09-20T12:00:00Z"})
        runs = [{"id": 666, "event": "push", "pull_requests": [{"number": 16239}],
                 "created_at": "2026-09-20T12:02:00Z"},
                {"id": 777, "event": "pull_request", "pull_requests": [{"number": 1}],
                 "created_at": "2026-09-20T12:02:00Z"},
                {"id": 555, "event": "pull_request", "pull_requests": [{"number": 16239}],
                 "created_at": "2026-09-20T11:59:00Z"}]
        if qualifying:
            runs.append({"id": 888, "event": "pull_request", "pull_requests": [{"number": 16239}],
                         "created_at": "2026-09-20T12:01:00Z"})
        responses = {
            "repos/owner/repo/actions/runs/11": (run, 0),
            "repos/owner/repo/pulls/16239": ("b" * 40, 0),
            f"repos/owner/repo/contents/{IDENTITY_FILTER}?ref={'c' * 40}": (content, api_status),
            f"repos/owner/repo/actions/workflows/7/runs?head_sha={'b' * 40}&event=pull_request&per_page=30":
                (json.dumps({"workflow_runs": runs}), 0),
        }
        cases = "\n".join(
            f"{shlex.quote(endpoint)}) printf '%s' {shlex.quote(body)}; return {status} ;;"
            for endpoint, (body, status) in responses.items()
        )
        stub = 'gh() { printf "%s\\n" "$2" >> "$API_CALLS"; case "$2" in\n' + cases
        stub += '\n*) echo "unexpected API endpoint" >&2; return 97 ;; esac; };\n'
        env = {key: os.environ[key] for key in ("PATH", "SystemRoot") if key in os.environ}
        with tempfile.TemporaryDirectory(prefix="rust-small-resolver-") as scratch:
            directory = Path(scratch)
            candidate_filter = directory / IDENTITY_FILTER
            candidate_filter.parent.mkdir(parents=True)
            candidate_filter.write_text("999\n", encoding="utf-8")
            env_file, calls_file = directory / "env", directory / "calls"
            env_file.touch()
            calls_file.touch()
            env.update(BASE_SHA=base, RUN_ID="11", PR_NUMBER="16239", REPOSITORY="owner/repo",
                       GITHUB_ENV=env_file.as_posix(), API_CALLS=calls_file.as_posix(),
                       TMPDIR=directory.as_posix(), RUNNER_TEMP=directory.as_posix())
            result = subprocess.run(
                [bash, "--noprofile", "--norc"],
                input=stub + (script if script is not None else _resolve_script(_workflow_text())),
                cwd=directory, env=env, capture_output=True, text=True, encoding="utf-8", timeout=10,
            )
            exports = dict(line.split("=", 1) for line in env_file.read_text().splitlines())
            calls = calls_file.read_text().splitlines()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("unexpected API endpoint", result.stderr)
        self.assertEqual(exports.get("RUN_HEAD_SHA"), "a" * 40)
        self.assertEqual(exports.get("LATEST_HEAD_SHA"), "b" * 40)
        return exports, calls

    def test_candidate_filter_cannot_invent_a_replacement(self) -> None:
        exports, calls = self._run_resolve(qualifying=False)
        self.assertNotIn("REPLACEMENT_RUN_ID", exports)
        self.assertIn(f"repos/owner/repo/contents/{IDENTITY_FILTER}?ref={'c' * 40}", calls)

    def test_trusted_filter_retains_event_pr_and_chronology_guards(self) -> None:
        exports, calls = self._run_resolve()
        self.assertEqual(exports.get("REPLACEMENT_RUN_ID"), "888")
        self.assertEqual(sum("/contents/" in call for call in calls), 1)

    def test_unavailable_or_malformed_base_filter_stays_unbound(self) -> None:
        for options in ({"base": "not-a-sha"}, {"content": ""}, {"content": "!invalid-base64!"},
                        {"content": base64.b64encode(b"invalid jq syntax !").decode("ascii")},
                        {"api_status": 1}):
            with self.subTest(options=options):
                exports, calls = self._run_resolve(**options)
                self.assertNotIn("REPLACEMENT_RUN_ID", exports)
                if "base" in options:
                    self.assertFalse(any("/contents/" in call for call in calls))

    def test_candidate_filter_fallback_mutant_is_rejected(self) -> None:
        script = _resolve_script(_workflow_text())
        mutant = script.replace('-f "$trusted_filter"', f"-f {IDENTITY_FILTER}")
        self.assertNotEqual(mutant, script)
        exports, _ = self._run_resolve(qualifying=False, script=mutant)
        self.assertEqual(exports.get("REPLACEMENT_RUN_ID"), "999")
        with self.assertRaises(AssertionError):
            self.assertNotIn("REPLACEMENT_RUN_ID", exports)


class ExecutableCancellationControls(unittest.TestCase):
    """Execute the production evaluator; these controls make no API calls."""

    def _run_evaluate(
        self, values: dict[str, str], *, script: str | None = None
    ) -> subprocess.CompletedProcess[str]:
        bash = os.environ.get("RUST_SMALL_TEST_BASH") or shutil.which("bash")
        self.assertIsNotNone(bash, "bash is required to execute the production evaluator")
        # Do not inherit tokens or shell startup hooks. Stub gh as an additional
        # guard: the routed controls must never enter the unrelated draft API path.
        env = {key: os.environ[key] for key in ("PATH", "SystemRoot") if key in os.environ}
        env.update(
            ROUTE_RESULT="success",
            ROUTER_TARGET="github",
            CX53_RESULT="skipped",
            CX43_RESULT="skipped",
            GITHUB_RESULT="success",
            FALLBACK_RESULT="skipped",
        )
        env.update(values)
        with tempfile.TemporaryDirectory(prefix="rust-small-control-") as scratch:
            env["GITHUB_STEP_SUMMARY"] = (Path(scratch) / "summary").as_posix()
            result = subprocess.run(
                [bash, "--noprofile", "--norc"],
                input='gh() { echo "unexpected API call" >&2; return 97; };\n'
                + (script if script is not None else _evaluate_script(_workflow_text())),
                cwd=ROOT, env=env, capture_output=True, text=True, encoding="utf-8", timeout=10,
            )
        self.assertNotIn("unexpected API call", result.stderr)
        self.assertIn("route_result=success\n", result.stdout, "production evaluator must execute")
        return result

    def _mixed(self, primary: str, preflight: str | None, moved: bool) -> dict[str, str]:
        values = {
            "ROUTER_TARGET": primary.lower(),
            f"{primary}_RESULT": "failure",
            "GITHUB_RESULT": "skipped",
            "FALLBACK_RESULT": "cancelled",
        }
        if preflight is not None:
            values[f"{primary}_PREFLIGHT_OK"] = preflight
        if moved:
            values.update(RUN_HEAD_SHA="a" * 40, LATEST_HEAD_SHA="b" * 40,
                          REPLACEMENT_RUN_ID="123")
        return values

    def _assert_red(self, result: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(result.returncode, 1, "no proof must stay red: " + result.stdout)
        self.assertNotIn("Perl LSP Rust Small Result: success", result.stdout)

    def test_selected_primary_preflight_failure_and_cancelled_fallback(self) -> None:
        for primary in ("CX53", "CX43"):
            for moved in (False, True):
                with self.subTest(primary=primary, moved=moved):
                    result = self._run_evaluate(self._mixed(primary, "false", moved))
                    self._assert_red(result)
                    self.assertIn(f"{primary} preflight failed", result.stdout)
                    self.assertNotIn("real test failure", result.stdout)
                    self.assertNotIn("a lane failed alongside", result.stdout)
                    verdict = "superseded" if moved else "cancelled-no-verdict"
                    self.assertIn(f"RUST_SMALL_GATE_VERDICT={verdict}\n", result.stdout)

    def test_preflight_true_failure_keeps_failure_precedence(self) -> None:
        for primary in ("CX53", "CX43"):
            for moved in (False, True):
                with self.subTest(primary=primary, moved=moved):
                    result = self._run_evaluate(self._mixed(primary, "true", moved))
                    self._assert_red(result)
                    self.assertIn("a lane failed alongside a cancelled lane", result.stdout)
                    self.assertNotIn("RUST_SMALL_GATE_VERDICT=superseded", result.stdout)
                    self.assertNotIn("This is NOT a test failure", result.stdout)

    def test_missing_or_malformed_preflight_is_unknown(self) -> None:
        for primary in ("CX53", "CX43"):
            for preflight in (None, "", "False", "0", "garbage", "false "):
                with self.subTest(primary=primary, preflight=preflight):
                    result = self._run_evaluate(self._mixed(primary, preflight, True))
                    self._assert_red(result)
                    self.assertIn("a lane failed alongside a cancelled lane", result.stdout)
                    self.assertNotIn("preflight failed", result.stdout)
                    self.assertNotIn("real test failure", result.stdout)
                    self.assertNotIn("RUST_SMALL_GATE_VERDICT=superseded", result.stdout)

    def test_false_preflight_on_unselected_primary_is_not_inferred(self) -> None:
        for primary, other in (("CX53", "CX43"), ("CX43", "CX53")):
            with self.subTest(primary=primary):
                values = self._mixed(primary, "false", True)
                values.update({f"{primary}_RESULT": "skipped", f"{other}_RESULT": "failure",
                               f"{other}_PREFLIGHT_OK": "false"})
                result = self._run_evaluate(values)
                self._assert_red(result)
                self.assertIn("a lane failed alongside a cancelled lane", result.stdout)
                self.assertNotIn("preflight failed", result.stdout)
                self.assertNotIn("RUST_SMALL_GATE_VERDICT=superseded", result.stdout)

    def test_other_failed_lane_keeps_precedence_over_preflight_failure(self) -> None:
        for primary in ("CX53", "CX43"):
            for failed, cancelled in (("GITHUB", "FALLBACK"), ("FALLBACK", "GITHUB")):
                with self.subTest(primary=primary, failed=failed):
                    values = self._mixed(primary, "false", True)
                    values.update({f"{failed}_RESULT": "failure", f"{cancelled}_RESULT": "cancelled"})
                    result = self._run_evaluate(values)
                    self._assert_red(result)
                    self.assertIn("a lane failed alongside a cancelled lane", result.stdout)
                    self.assertNotIn("RUST_SMALL_GATE_VERDICT=superseded", result.stdout)

    def test_supersession_requires_well_formed_moved_heads_and_run(self) -> None:
        for overrides in ({"RUN_HEAD_SHA": "bad"}, {"LATEST_HEAD_SHA": ""},
                          {"LATEST_HEAD_SHA": "a" * 40}, {"REPLACEMENT_RUN_ID": "bad"},
                          {"REPLACEMENT_RUN_ID": ""}):
            with self.subTest(overrides=overrides):
                values = self._mixed("CX53", "false", True)
                values.update(overrides)
                result = self._run_evaluate(values)
                self._assert_red(result)
                self.assertIn("RUST_SMALL_GATE_VERDICT=cancelled-no-verdict\n", result.stdout)

    def test_cancelled_lanes_stay_red_and_success_control_passes(self) -> None:
        for lane in ("CX53", "CX43", "GITHUB", "FALLBACK"):
            with self.subTest(lane=lane):
                self._assert_red(self._run_evaluate({f"{lane}_RESULT": "cancelled"}))
        self.assertEqual(self._run_evaluate({}).returncode, 0)
        for result in ("skipped", "failure"):
            with self.subTest(result=result):
                self._assert_red(self._run_evaluate({"GITHUB_RESULT": result}))

    def test_executable_green_and_errexit_mutants_are_rejected(self) -> None:
        script = _evaluate_script(_workflow_text())
        green = _evaluate_script(_green_partition_mutation(_workflow_text()))
        self.assertNotEqual(green, script)
        with self.assertRaisesRegex(AssertionError, "no proof must stay red"):
            self._assert_red(self._run_evaluate(self._mixed("CX53", None, True)
                                               | {"CX53_RESULT": "skipped"}, script=green))
        without_errexit = _evaluate_script(
            _mutate_evaluate(_workflow_text(), "set -euo pipefail", "set -uo pipefail")
        )
        self.assertNotEqual(without_errexit, script)
        for result in ("skipped", "failure"):
            with self.subTest(result=result):
                with self.assertRaisesRegex(AssertionError, "no proof must stay red"):
                    self._assert_red(self._run_evaluate({"GITHUB_RESULT": result},
                                                       script=without_errexit))


if __name__ == "__main__":
    unittest.main()
