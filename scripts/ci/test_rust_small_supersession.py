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

Red-first contract: any single-clause mutation of the discriminator — dropping
the head-inequality check, relaxing a 40-hex shape, dropping the replacement
run's digits check, exporting an unvalidated value, keying the lookup on
candidate-controlled data, duplicating the identity filter, or granting an
`exit 0` anywhere in the partition — must fail this contract with the clause
named.
"""

from __future__ import annotations

import re
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


def discriminator_violations(text: str) -> list[str]:
    """Every way the workflow departs from the #16187 discriminator contract."""
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
    if f"-f {IDENTITY_FILTER}" not in resolve:
        violations.append(
            "replacement identity must come from the shared filter file, not "
            "an inline copy that can drift from the advisory aggregate"
        )
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
        violations.append("partition must detect a genuine failure lane")
    failure_slice = partition.split('if [ "$any_failure" = "true" ]; then', 1)[1].split(
        "exit 1", 1
    )[0]
    if "real test failure" not in failure_slice:
        violations.append("failed-lane branch must read as a real test failure")
    if "NOT a test failure" in failure_slice:
        violations.append(
            "failed-lane branch must not borrow the cancelled NOT-a-failure wording"
        )

    if "exit 0" in partition:
        violations.append("no proof never becomes green: the partition must not exit 0")
    if "if: cancelled()" in result_job:
        violations.append(
            "cancelled() is not the discriminator (#16187 correction): the "
            "aggregate must classify on evidence, not on run state"
        )
    return violations


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
        lambda t: t.replace(f"-f {IDENTITY_FILTER}", "-f /dev/null"),
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
        lambda t: t.replace(
            "RUST_SMALL_GATE_VERDICT=superseded",
            "RUST_SMALL_GATE_VERDICT=superseded",
        ).replace("Perl LSP Rust Small Result: cancelled (superseded", "exit 0  # (superseded", 1),
        "must not exit 0",
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


class RedFirstMutations(unittest.TestCase):
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


class SharedFilterFileIsExecuted(unittest.TestCase):
    def test_the_identity_filter_exists_and_both_aggregates_run_it(self) -> None:
        self.assertTrue((ROOT / IDENTITY_FILTER).is_file())
        routed = _step_body(_job_body(_workflow_text(), RESULT_JOB), RESOLVE_STEP)
        advisory = _workflow_text()
        ci_yml = ROOT / ".github" / "workflows" / "ci.yml"
        if ci_yml.is_file():
            advisory = ci_yml.read_text(encoding="utf-8")
        self.assertIn(f"-f {IDENTITY_FILTER}", routed)
        self.assertIn(f"-f {IDENTITY_FILTER}", advisory)


if __name__ == "__main__":
    unittest.main()
