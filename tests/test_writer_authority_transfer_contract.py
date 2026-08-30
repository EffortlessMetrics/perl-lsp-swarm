from __future__ import annotations

import unittest
from pathlib import Path


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

TRANSFER_REQUIREMENTS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("typed return", ("typed return",)),
    ("explicit uncertainty", ("`not_proven`",)),
    ("silence is not transfer", ("silence does not transfer ownership",)),
    (
        "silence does not free the candidate",
        ("does not make a claim available to a second writer",),
    ),
    (
        "artifact and mutation-state inspection",
        ("inspect the durable artifact and local mutation state",),
    ),
    ("prior-writer resumability", ("prior writer can resume",)),
    ("process stop state", ("process has stopped",)),
    ("unique local work", ("uncommitted/unpushed work exists",)),
    ("salvage before replacement", ("salvage useful work before replacing a writer",)),
    (
        "inspection is not authority transfer",
        ("establishing those facts alone does not transfer mutation authority",),
    ),
    ("revoke before reassign", ("reassign only after",)),
    ("non-resumable path", ("provably unable to resume",)),
    (
        "acknowledged stopped-or-revoked path",
        ("acknowledged handoff has stopped or revoked it",),
    ),
    (
        "one retained writer",
        ("two writers never mutate the same candidate concurrently",),
    ),
)

TRANSFER_ORDER = (
    "prior writer can resume",
    "salvage useful work before replacing a writer",
    "establishing those facts alone does not transfer mutation authority",
    "reassign only after",
    "two writers never mutate the same candidate concurrently",
)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def normalize(text: str) -> str:
    return " ".join(text.split()).lower()


def h2_section(text: str, heading: str) -> str | None:
    in_section = False
    lines: list[str] = []

    for line in text.splitlines():
        if line.strip().casefold() == heading.casefold():
            in_section = True
            continue
        if in_section and line.lstrip().startswith("## ") and not line.lstrip().startswith("### "):
            break
        if in_section:
            lines.append(line)

    return "\n".join(lines) if in_section else None


def h3_section(text: str, heading: str) -> str | None:
    in_section = False
    lines: list[str] = []

    for line in text.splitlines():
        if line.strip().casefold() == heading.casefold():
            in_section = True
            continue
        if in_section and line.lstrip().startswith("#"):
            break
        if in_section:
            lines.append(line)

    return "\n".join(lines) if in_section else None


def validate_transfer(text: str, heading: str) -> list[str]:
    section = h2_section(text, heading)
    if section is None:
        return [f"missing section {heading!r}"]

    normalized = normalize(section)
    failures: list[str] = []

    for label, alternatives in TRANSFER_REQUIREMENTS:
        if not any(term in normalized for term in alternatives):
            failures.append(f"missing {label}")

    positions = [normalized.find(marker) for marker in TRANSFER_ORDER]
    if all(position >= 0 for position in positions) and positions != sorted(positions):
        failures.append("transfer obligations are out of order")

    return failures


def validate_reviewer_reassignment(text: str) -> list[str]:
    section = h3_section(text, "### Mutation owner")
    if section is None:
        return ["missing section '### Mutation owner'"]

    normalized = normalize(section)
    requirements = (
        ("one candidate writer", ("one writer mutates the candidate branch/worktree at a time",)),
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

    return [
        f"missing {label}"
        for label, alternatives in requirements
        if not any(term in normalized for term in alternatives)
    ]


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
    def test_provider_orchestration_skills_require_revoke_before_reassign(self) -> None:
        for provider, path, heading in ORCHESTRATION_SKILLS:
            failures = validate_transfer(read(path), heading)
            self.assertFalse(
                failures,
                f"{provider} ({path}) violates writer transfer contract: {failures}",
            )

    def test_provider_builders_require_explicit_reviewer_reassignment(self) -> None:
        for provider, path in BUILD_SKILLS:
            failures = validate_reviewer_reassignment(read(path))
            self.assertFalse(
                failures,
                f"{provider} ({path}) violates reviewer-to-writer transfer: {failures}",
            )

    def test_transfer_is_candidate_local_not_global(self) -> None:
        for provider, path, _ in ORCHESTRATION_SKILLS:
            text = normalize(read(path))
            self.assertIn(
                "separate specified claims may use separate writers/worktrees; "
                "one candidate still has exactly one writer",
                text,
                f"{provider} ({path}) no longer keeps writer authority candidate-local",
            )

    def test_workflow_runs_and_triggers_this_contract(self) -> None:
        workflow = read(WORKFLOW)
        self.assertEqual(
            workflow.count(f"- '{SELF_TEST}'"),
            2,
            "writer-transfer contract must trigger for both pull_request and push",
        )
        self.assertIn(
            "python3 -m unittest "
            "tests/test_active_authority_contract.py "
            "tests/test_writer_authority_transfer_contract.py",
            normalize(workflow),
        )

    def test_silence_cannot_become_transfer_authority(self) -> None:
        text = transfer_fixture().replace(
            "Silence does not transfer ownership and does not make a claim available to\n"
            "a second writer.",
            "Silence transfers ownership and makes the claim available to a second writer.",
        )
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("missing silence is not transfer", failures)
        self.assertIn("missing silence does not free the candidate", failures)

    def test_unique_work_cannot_be_skipped(self) -> None:
        text = (
            transfer_fixture()
            .replace(
                "whether uncommitted/unpushed work exists",
                "whether the worktree exists",
            )
            .replace(
                "Salvage useful work before\nreplacing a writer.",
                "Replace the writer.",
            )
        )
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("missing unique local work", failures)
        self.assertIn("missing salvage before replacement", failures)

    def test_grant_before_revocation_fails_closed(self) -> None:
        text = transfer_fixture().replace("reassign only after", "reassign before")
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("missing revoke before reassign", failures)

    def test_inspection_alone_cannot_transfer_authority(self) -> None:
        text = transfer_fixture().replace(
            "Establishing those facts alone does not transfer mutation authority:",
            "Those facts transfer mutation authority:",
        )
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("missing inspection is not authority transfer", failures)

    def test_unknown_non_resumability_cannot_be_treated_as_revoked(self) -> None:
        text = transfer_fixture().replace(
            "the prior writer is provably unable to resume, or after an\n"
            "acknowledged handoff has stopped or revoked it",
            "the prior writer appears idle",
        )
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("missing non-resumable path", failures)
        self.assertIn("missing acknowledged stopped-or-revoked path", failures)

    def test_transfer_obligations_cannot_be_reordered(self) -> None:
        text = transfer_fixture()
        inspection = (
            "Before reassignment, establish whether the prior writer can resume, whether its process\n"
            "has stopped, and whether uncommitted/unpushed work exists. "
        )
        reassign = (
            "reassign only after the prior writer is provably unable to resume, or after an\n"
            "acknowledged handoff has stopped or revoked it, "
        )
        text = text.replace(inspection, "").replace(reassign, "")
        text = text.replace(
            "When a worker goes quiet, inspect the durable artifact and local mutation state.",
            "When a worker goes quiet, inspect the durable artifact and local mutation state.\n\n"
            + reassign
            + inspection,
        )
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("transfer obligations are out of order", failures)

    def test_matching_words_outside_the_transfer_section_do_not_count(self) -> None:
        text = transfer_fixture().replace(
            "## A quiet worker is not a result",
            "## Retired example",
            1,
        )
        text += "\n## A quiet worker is not a result\nSilence does not transfer ownership.\n"
        failures = validate_transfer(text, "## A quiet worker is not a result")
        self.assertIn("missing prior-writer resumability", failures)
        self.assertIn("missing revoke before reassign", failures)


if __name__ == "__main__":
    unittest.main()
