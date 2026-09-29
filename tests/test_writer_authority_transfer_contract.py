from __future__ import annotations

import re
import unittest
from pathlib import Path

from tests.test_active_authority_contract import prose_text, workflow_event_paths


ROOT = Path(__file__).resolve().parents[1]
ORCHESTRATION_SKILLS = (
    ("codex", ".agents/skills/orchestrate-work/SKILL.md", "## A quiet worker is not a result"),
    ("claude", ".claude/skills/orchestrate-work/SKILL.md", "## A quiet agent is not a result"),
)
BUILD_SKILLS = (
    ("codex", ".agents/skills/build-candidate/SKILL.md"),
    ("claude", ".claude/skills/build-candidate/SKILL.md"),
)
WORKFLOW = ".github/workflows/active-authority-contract.yml"
SELF_TEST = "tests/test_writer_authority_transfer_contract.py"
HEADING = re.compile(r"^(#{1,6})\s+")

TRANSFER_REQUIREMENTS = (
    ("typed return", "typed return"),
    ("explicit uncertainty", "`not_proven`"),
    ("silence is not transfer", "silence does not transfer ownership"),
    ("silence does not free the candidate", "does not make a claim available to a second writer"),
    ("artifact and mutation-state inspection", "inspect the durable artifact and local mutation state"),
    ("prior-writer resumability", "prior writer can resume"),
    ("process stop state", "process has stopped"),
    ("unique local work", "uncommitted/unpushed work exists"),
    ("salvage before replacement", "salvage useful work before replacing a writer"),
    (
        "inspection is not authority transfer",
        "establishing those facts alone does not transfer mutation authority",
    ),
    ("revoke before reassign", "reassign only after"),
    ("non-resumable path", "provably unable to resume"),
    ("acknowledged stopped-or-revoked path", "acknowledged handoff has stopped or revoked it"),
    ("one retained writer", "two writers never mutate the same candidate concurrently"),
)
TRANSFER_ORDER = (
    "prior writer can resume",
    "salvage useful work before replacing a writer",
    "establishing those facts alone does not transfer mutation authority",
    "reassign only after",
    "two writers never mutate the same candidate concurrently",
)
MUTATION_OWNER_REQUIREMENTS = (
    (
        "one candidate writer",
        ("one writer mutates the candidate branch/worktree at a time",),
    ),
    (
        "explicit reviewer reassignment",
        (
            "a reviewer may become the writer only through an explicit reassignment",
            "a reviewer may become writer only through an explicit reassignment",
        ),
    ),
    (
        "affected proof and review",
        (
            "the resulting mutation still returns through affected proof and review",
            "resulting mutation still returns through affected proof/review",
        ),
    ),
)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def normalize(text: str) -> str:
    return " ".join(text.split()).lower()


def section(text: str, heading: str) -> tuple[str | None, str | None]:
    lines = text.splitlines()

    def fenced_regions(line: str, state: list[bool]) -> bool:
        """Toggle fence state on ``` lines and report whether `line` sits in a fence.

        A fenced line such as `# gh pr merge ...` is example text, not a
        heading, and must never terminate the scanned section.
        """
        if line.lstrip().startswith("```"):
            state[0] = not state[0]
            return True
        return state[0]

    fence = [False]
    matches = [
        index
        for index, line in enumerate(lines)
        if not fenced_regions(line, fence) and line.strip().casefold() == heading.casefold()
    ]
    if not matches:
        return None, f"missing section {heading!r}"
    if len(matches) != 1:
        return None, f"section {heading!r} occurs {len(matches)} times"

    target_level = len(heading) - len(heading.lstrip("#"))
    body: list[str] = []
    fence = [False]
    for line in lines[matches[0] + 1 :]:
        if fenced_regions(line, fence):
            body.append(line)
            continue
        found = HEADING.match(line.lstrip())
        if found is not None and len(found.group(1)) <= target_level:
            break
        body.append(line)
    return "\n".join(body), None


def transfer_errors(text: str, heading: str) -> list[str]:
    body, error = section(text, heading)
    if error is not None:
        return [error]

    visible = prose_text(body or "", is_text=True).lower()
    failures = [
        f"missing {label}"
        for label, phrase in TRANSFER_REQUIREMENTS
        if phrase not in visible
    ]
    positions = [visible.find(phrase) for phrase in TRANSFER_ORDER]
    if all(position >= 0 for position in positions) and positions != sorted(positions):
        failures.append("transfer obligations are out of order")
    return failures


def mutation_owner_errors(text: str) -> list[str]:
    body, error = section(text, "### Mutation owner")
    if error is not None:
        return [error]

    visible = prose_text(body or "", is_text=True).lower()
    return [
        f"missing {label}"
        for label, alternatives in MUTATION_OWNER_REQUIREMENTS
        if not any(phrase in visible for phrase in alternatives)
    ]


def workflow_run_commands(text: str) -> tuple[str, ...]:
    lines = text.splitlines()
    commands: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        active = line.strip().removeprefix("- ").lstrip()
        if line.lstrip().startswith("#") or not active.startswith("run:"):
            index += 1
            continue

        indent = len(line) - len(line.lstrip(" "))
        value = active.removeprefix("run:").strip()
        if value not in {"|", "|-", ">", ">-"}:
            commands.append(normalize(value))
            index += 1
            continue

        block: list[str] = []
        index += 1
        while index < len(lines):
            candidate = lines[index]
            stripped = candidate.strip()
            if stripped and len(candidate) - len(candidate.lstrip(" ")) <= indent:
                break
            if stripped and not stripped.startswith("#"):
                block.append(stripped)
            index += 1
        commands.append(normalize(" ".join(block)))
    return tuple(commands)


def transfer_fixture() -> str:
    return """\
## A quiet worker is not a result

Every dispatched programme owes a typed return or leaves its dimension visibly
`NOT_PROVEN`. Silence does not transfer ownership and does not make a claim available to
a second writer.

When a worker goes quiet, inspect the durable artifact and local mutation state.

Before reassignment, establish whether the prior writer can resume, whether its process
has stopped, and whether uncommitted/unpushed work exists. Salvage useful work before
replacing a writer. Establishing those facts alone does not transfer mutation authority:
reassign only after the prior writer is provably unable to resume, or after an
acknowledged handoff has stopped or revoked it, so two writers never mutate the same
candidate concurrently.

## Root-held claim frames
"""


class WriterAuthorityTransferContractTests(unittest.TestCase):
    def test_live_provider_contracts(self) -> None:
        for provider, path, heading in ORCHESTRATION_SKILLS:
            with self.subTest(provider=provider, surface="orchestration"):
                self.assertEqual(transfer_errors(read(path), heading), [])
            with self.subTest(provider=provider, surface="candidate-local"):
                self.assertIn(
                    "separate specified claims may use separate writers/worktrees; "
                    "one candidate still has exactly one writer",
                    prose_text(read(path), is_text=True).lower(),
                )

        for provider, path in BUILD_SKILLS:
            with self.subTest(provider=provider, surface="reviewer-reassignment"):
                self.assertEqual(mutation_owner_errors(read(path)), [])

    def test_workflow_wiring(self) -> None:
        workflow = read(WORKFLOW)
        for event in ("pull_request", "push"):
            with self.subTest(event=event):
                self.assertIn(SELF_TEST, workflow_event_paths(workflow, event))

        expected = normalize(
            "python3 -m unittest "
            "tests/test_active_authority_contract.py "
            "tests/test_writer_authority_transfer_contract.py"
        )
        self.assertIn(expected, workflow_run_commands(workflow))

    def test_transfer_mutations_fail_closed(self) -> None:
        source = transfer_fixture()
        active_body = source.split("## A quiet worker is not a result\n\n", 1)[1].split(
            "\n## Root-held claim frames",
            1,
        )[0]
        cases = (
            (
                "silence transfers",
                source.replace(
                    "Silence does not transfer ownership and does not make a claim available to\n"
                    "a second writer.",
                    "Silence transfers ownership and frees the claim for a second writer.",
                ),
                "missing silence is not transfer",
            ),
            (
                "unique work skipped",
                source.replace(
                    "whether uncommitted/unpushed work exists",
                    "whether the worktree exists",
                ),
                "missing unique local work",
            ),
            (
                "salvage skipped",
                source.replace(
                    "Salvage useful work before\nreplacing a writer.",
                    "Replace the writer.",
                ),
                "missing salvage before replacement",
            ),
            (
                "inspection becomes transfer",
                source.replace(
                    "Establishing those facts alone does not transfer mutation authority:",
                    "Those facts transfer mutation authority:",
                ),
                "missing inspection is not authority transfer",
            ),
            (
                "grant before revoke",
                source.replace("reassign only after", "reassign before"),
                "missing revoke before reassign",
            ),
            (
                "idle substitutes for revocation",
                source.replace(
                    "the prior writer is provably unable to resume, or after an\n"
                    "acknowledged handoff has stopped or revoked it",
                    "the prior writer appears idle",
                ),
                "missing non-resumable path",
            ),
            (
                "requirements only in fence",
                "## A quiet worker is not a result\n\n```text\n"
                + active_body
                + "\n```\n\nSilence does not transfer ownership.\n## Root-held claim frames\n",
                "missing prior-writer resumability",
            ),
            (
                "requirements only in comment",
                "## A quiet worker is not a result\n\n<!--\n"
                + active_body
                + "\n-->\n\nSilence does not transfer ownership.\n## Root-held claim frames\n",
                "missing prior-writer resumability",
            ),
            (
                "requirements outside active section",
                source.replace("## A quiet worker is not a result", "## Retired example", 1)
                + "\n## A quiet worker is not a result\nSilence does not transfer ownership.\n",
                "missing prior-writer resumability",
            ),
            (
                "duplicate transfer section",
                source + "\n" + source,
                "section '## A quiet worker is not a result' occurs 2 times",
            ),
        )
        for name, text, expected in cases:
            with self.subTest(name=name):
                self.assertIn(
                    expected,
                    transfer_errors(text, "## A quiet worker is not a result"),
                )

    def test_transfer_order_is_load_bearing(self) -> None:
        source = transfer_fixture()
        inspection = (
            "Before reassignment, establish whether the prior writer can resume, whether its process\n"
            "has stopped, and whether uncommitted/unpushed work exists. "
        )
        reassign = (
            "reassign only after the prior writer is provably unable to resume, or after an\n"
            "acknowledged handoff has stopped or revoked it, "
        )
        text = source.replace(inspection, "").replace(reassign, "")
        text = text.replace(
            "When a worker goes quiet, inspect the durable artifact and local mutation state.",
            "When a worker goes quiet, inspect the durable artifact and local mutation state.\n\n"
            + reassign
            + inspection,
        )
        self.assertIn(
            "transfer obligations are out of order",
            transfer_errors(text, "## A quiet worker is not a result"),
        )

    def test_duplicate_reviewer_transfer_section_fails_closed(self) -> None:
        section_text = """\
### Mutation owner

One writer mutates the candidate branch/worktree at a time. A reviewer may become
writer only through an explicit reassignment; resulting mutation still returns through
affected proof/review.
"""
        self.assertEqual(
            mutation_owner_errors(section_text + "\n" + section_text),
            ["section '### Mutation owner' occurs 2 times"],
        )

    def test_commented_workflow_command_does_not_count(self) -> None:
        expected = normalize(
            "python3 -m unittest "
            "tests/test_active_authority_contract.py "
            "tests/test_writer_authority_transfer_contract.py"
        )
        workflow = (
            f"# run: {expected}\n"
            "jobs:\n"
            "  check:\n"
            "    steps:\n"
            "      - run: echo not-the-contract\n"
        )
        self.assertNotIn(expected, workflow_run_commands(workflow))


if __name__ == "__main__":
    unittest.main()
