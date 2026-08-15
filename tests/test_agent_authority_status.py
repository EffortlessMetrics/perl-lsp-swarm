from __future__ import annotations

import copy
import re
import tomllib
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "docs" / "agents" / "authority_status.toml"
WORKFLOW = ROOT / ".github" / "workflows" / "agent-authority-status.yml"
ALLOWED_STATUSES = {"current", "transitional", "historical", "superseded"}
TOP_LEVEL_PATH_FIELDS = ("current_method", "review_currentness", "github_surfaces")
REQUIRED_CURRENT = {
    "AGENTS.md",
    "CLAUDE.md",
    "docs/agents/AUTHORITY_STATUS.md",
    "docs/agents/authority_status.toml",
    "docs/agents/README.md",
    "docs/agents/DEVELOPMENT_METHOD.md",
    "docs/agents/REVIEW_CURRENTNESS.md",
    "docs/agents/GITHUB_SURFACES.md",
    "docs/agents/SKILL_CONTRACT.md",
    "docs/how-to/SESSION_OPERATIONS.md",
    "docs/how-to/AGENT_CONTRIBUTING.md",
    # Amended by PR #6863 (e9a698285f) and current on `main` since.
    "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    # Rewritten by PR #6868 (709b4ca939) and current on `main` since.
    "docs/reference/MAINTAINER_AGENT_DOCTRINE.md",
    "docs/reference/WORKTREE_PROTOCOL.md",
    "CONTRIBUTING.md",
    ".github/copilot-instructions.md",
}
REQUIRED_LEGACY = {
    "docs/reference/ORCHESTRATION_DOCTRINE.md",
    "docs/reference/PIPELINE_GATES.md",
    "docs/reference/OCTOPUS_CLUSTER.md",
    "docs/reference/GLOSSARY.md",
    "docs/reference/LIVE_SIGNALS_VS_LABELS.md",
    "docs/adr/0044-octopus-cluster-orchestration.md",
    "docs/articles/PIPELINE_STATE_MACHINE.md",
    "docs/handoff/SWARM_DESIGN.md",
    ".spec/3988-merge-readiness/spec.md",
}
REQUIRED_TRANSITIONAL = {
    "scripts/ci/check-pr-review-convergence-core",
}

# A `transitional` row asserts a fact about the document's *present* content: the
# retired text is still live on `main`, and a named replacement is still pending.
# That assertion has two halves, and both need an oracle.
#
# The negative half -- "the document does not consider itself settled" -- is
# checked by self-declaration. The positive half -- "the retired text is still
# there" -- is checked by `stale_marker`: each transitional row names an exact
# substring that must still appear in the document. When the replacement lands
# and removes that text, the row fails.
#
# Only the negative half existed at first, and it let four rows go stale
# undetected when PR #6868 landed. `MAINTAINER_AGENT_DOCTRINE.md` and
# `WORKTREE_PROTOCOL.md` were caught, because the rewrite gave them a
# `Status: current` line. `CONTRIBUTING.md` and `.github/copilot-instructions.md`
# were not: their retired conveyor text was deleted, but neither file declares a
# status, so nothing contradicted the row. A vanished `stale_marker` catches
# exactly that case without needing the document to say anything about itself.
#
# A `current` self-claim catches the authority inversion this registry exists to
# prevent -- a document that declares itself the current contract must not be
# demoted to transitional. A `retired` self-claim catches the mirror error -- a
# document whose replacement already landed is historical, not pending.
CURRENT_SELF_CLAIMS = (
    r"\bis the current durable\b",
    r"\bis the current\b[^.]{0,80}\bcontract\b",
    r"\bStatus:\s*current\b",
)
RETIRED_SELF_CLAIMS = (
    r"\bRETIRED\b",
    r"\bno longer has\b",
    r"\bno longer\b[^.]{0,40}\bauthority\b",
)
# Retired self-claims are read from the header/docstring region only. Body prose
# legitimately discusses the retirement of *other* things -- WORKTREE_PROTOCOL.md
# says "is retired" about a branch -- which is not a self-declaration.
SELF_CLAIM_HEADER_LINES = 40
WORKFLOW_PATHS = {
    "AGENTS.md",
    "CLAUDE.md",
    "docs/agents/AUTHORITY_STATUS.md",
    "docs/agents/authority_status.toml",
    "docs/agents/README.md",
    "tests/test_agent_authority_status.py",
    ".github/workflows/agent-authority-status.yml",
}


def load_registry() -> dict[str, Any]:
    return tomllib.loads(REGISTRY.read_text(encoding="utf-8"))


def prose(path: Path) -> str:
    return " ".join(path.read_text(encoding="utf-8").split())


def _normalize(text: str) -> str:
    return " ".join(text.split())


def stale_marker_contradiction(path: str, marker: str, text: str) -> str | None:
    """Report a `transitional` row whose retired text is no longer in the document.

    This is the half of the transitional claim that no self-declaration can
    carry. A replacement PR that simply deletes the stale passage leaves a
    document that says nothing about its own status, so only the absence of the
    text it was classified for can reveal that the row is now false.
    """
    if _normalize(marker) in _normalize(text):
        return None
    return (
        f"{path}: classified transitional, but its stale_marker {marker!r} is no "
        f"longer present; the text this row was classified for is gone, so the "
        f"replacement has landed and the row is stale"
    )


def self_claim_contradictions(path: str, text: str) -> list[str]:
    """Report ways a document's own content contradicts a `transitional` status.

    `transitional` is the only status that makes a checkable claim about the
    document as it stands today: the stale text is still live and a replacement
    is still pending. This reads the document rather than the registry, so a row
    cannot stay green merely because its status string is spelled correctly.
    """
    problems: list[str] = []
    body = _normalize(text)
    header = _normalize("\n".join(text.splitlines()[:SELF_CLAIM_HEADER_LINES]))

    for pattern in CURRENT_SELF_CLAIMS:
        if re.search(pattern, body, re.IGNORECASE):
            problems.append(
                f"{path}: classified transitional, but the document declares itself "
                f"current (matched {pattern!r}); classifying it transitional would "
                f"demote a current authority"
            )
            break
    for pattern in RETIRED_SELF_CLAIMS:
        if re.search(pattern, header):
            problems.append(
                f"{path}: classified transitional, but the document declares itself "
                f"already retired (matched {pattern!r}); its replacement has landed, "
                f"so it is historical rather than pending"
            )
            break
    return problems


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def workflow_event_paths(event: str) -> set[str]:
    lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
    marker = f"  {event}:"
    try:
        start = lines.index(marker)
    except ValueError as error:
        raise AssertionError(f"missing on.{event} event block") from error

    end = len(lines)
    for index in range(start + 1, len(lines)):
        stripped = lines[index].strip()
        if not stripped or stripped.startswith("#"):
            continue
        if _indent(lines[index]) <= 2 and stripped.endswith(":"):
            end = index
            break

    block = lines[start:end]
    try:
        paths_index = block.index("    paths:")
    except ValueError as error:
        raise AssertionError(f"missing on.{event}.paths") from error

    paths: set[str] = set()
    for line in block[paths_index + 1 :]:
        stripped = line.strip()
        if not stripped:
            continue
        if _indent(line) <= 4:
            break
        if stripped.startswith("- '") and stripped.endswith("'"):
            paths.add(stripped[3:-1])
    return paths


def validate_registry(document: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if document.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if document.get("tracking_issue") != 4555:
        errors.append("tracking_issue must remain #4555")
    owner = document.get("owner")
    if not isinstance(owner, str) or not owner.strip():
        errors.append("owner must be a non-empty string")

    rows = document.get("documents")
    if not isinstance(rows, list):
        return [*errors, "documents must be an array"]

    by_path: dict[str, dict[str, Any]] = {}
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            errors.append(f"documents[{index}] must be a table")
            continue
        path = row.get("path")
        status = row.get("status")
        successor = row.get("successor")
        notes = row.get("notes")
        if not isinstance(path, str) or not path:
            errors.append(f"documents[{index}].path must be non-empty")
            continue
        if path in by_path:
            errors.append(f"duplicate path {path}")
        by_path[path] = row
        if status not in ALLOWED_STATUSES:
            errors.append(f"{path}: invalid status {status!r}")
        if not isinstance(successor, str):
            errors.append(f"{path}: successor must be a string")
        elif status == "current" and successor:
            errors.append(f"{path}: current documents must not name a successor")
        elif status != "current" and not successor:
            errors.append(f"{path}: non-current documents must name a successor")
        if not isinstance(notes, str) or not notes.strip():
            errors.append(f"{path}: notes must be non-empty")
        if not (ROOT / path).exists():
            errors.append(f"{path}: registry path does not exist")
        elif status == "transitional":
            text = (ROOT / path).read_text(encoding="utf-8", errors="replace")
            errors.extend(self_claim_contradictions(path, text))
            marker = row.get("stale_marker")
            if not isinstance(marker, str) or not marker.strip():
                errors.append(
                    f"{path}: transitional rows must name a non-empty stale_marker "
                    f"so the row fails when its retired text is removed"
                )
            else:
                problem = stale_marker_contradiction(path, marker, text)
                if problem:
                    errors.append(problem)
        elif "stale_marker" in row:
            errors.append(
                f"{path}: stale_marker is only meaningful for transitional rows"
            )

    for field in TOP_LEVEL_PATH_FIELDS:
        path = document.get(field)
        if not isinstance(path, str) or not path:
            errors.append(f"{field} must be a non-empty path")
            continue
        if not (ROOT / path).exists():
            errors.append(f"{field}: path does not exist: {path}")
        if by_path.get(path, {}).get("status") != "current":
            errors.append(f"{field}: referenced document is not current: {path}")

    paths = set(by_path)
    missing_current = REQUIRED_CURRENT - paths
    missing_legacy = REQUIRED_LEGACY - paths
    missing_transitional = REQUIRED_TRANSITIONAL - paths
    if missing_current:
        errors.append(f"missing current paths: {sorted(missing_current)!r}")
    if missing_legacy:
        errors.append(f"missing legacy paths: {sorted(missing_legacy)!r}")
    if missing_transitional:
        errors.append(f"missing transitional paths: {sorted(missing_transitional)!r}")

    for path in REQUIRED_CURRENT:
        if by_path.get(path, {}).get("status") != "current":
            errors.append(f"{path}: required current authority is not current")
    for path in REQUIRED_LEGACY:
        if by_path.get(path, {}).get("status") not in {"historical", "superseded"}:
            errors.append(f"{path}: legacy authority must be historical or superseded")
    for path in REQUIRED_TRANSITIONAL:
        if by_path.get(path, {}).get("status") != "transitional":
            errors.append(f"{path}: current-main replacement must remain transitional")

    return errors


class AgentAuthorityStatusTests(unittest.TestCase):
    def test_registry_is_complete_and_paths_exist(self) -> None:
        self.assertEqual(validate_registry(load_registry()), [])

    def test_human_index_points_to_registry_and_current_method(self) -> None:
        index = prose(ROOT / "docs" / "agents" / "AUTHORITY_STATUS.md")
        readme = prose(ROOT / "docs" / "agents" / "README.md")

        self.assertIn("authority_status.toml", index)
        self.assertIn("Internal words", index)
        self.assertIn("do not override this index", index)
        self.assertIn("Historical and superseded design graph", index)
        self.assertIn("AUTHORITY_STATUS.md", readme)
        self.assertIn("authority_status.toml", readme)
        self.assertIn("DEVELOPMENT_METHOD.md", readme)
        self.assertIn("REVIEW_CURRENTNESS.md", readme)

    def test_root_routes_delegate_document_status(self) -> None:
        for path in ("AGENTS.md", "CLAUDE.md"):
            contract = prose(ROOT / path)
            self.assertIn("docs/agents/AUTHORITY_STATUS.md", contract)
            self.assertIn("docs/agents/authority_status.toml", contract)
            self.assertIn("does not re-enter the hierarchy", contract)
            self.assertIn("accepted", contract)
            self.assertIn("active doctrine", contract)
            self.assertIn("north star", contract)

    def test_workflow_covers_root_delegation_for_both_events(self) -> None:
        for event in ("pull_request", "push"):
            paths = workflow_event_paths(event)
            self.assertEqual(
                paths,
                WORKFLOW_PATHS,
                f"on.{event}.paths must exactly cover the authority contract",
            )

    def test_registry_cannot_remove_its_own_current_status(self) -> None:
        document = copy.deepcopy(load_registry())
        row = next(
            item
            for item in document["documents"]
            if item["path"] == "docs/agents/authority_status.toml"
        )
        row["status"] = "historical"
        row["successor"] = "docs/agents/DEVELOPMENT_METHOD.md"

        errors = validate_registry(document)
        self.assertTrue(any("required current authority" in error for error in errors), errors)

    def test_legacy_document_cannot_silently_become_current(self) -> None:
        document = copy.deepcopy(load_registry())
        row = next(
            item
            for item in document["documents"]
            if item["path"] == "docs/reference/ORCHESTRATION_DOCTRINE.md"
        )
        row["status"] = "current"
        row["successor"] = ""

        errors = validate_registry(document)
        self.assertTrue(any("legacy authority" in error for error in errors), errors)

    def test_transitional_replacement_cannot_silently_disappear(self) -> None:
        document = copy.deepcopy(load_registry())
        document["documents"] = [
            row
            for row in document["documents"]
            if row["path"] != "scripts/ci/check-pr-review-convergence-core"
        ]

        errors = validate_registry(document)
        self.assertTrue(any("missing transitional paths" in error for error in errors), errors)

    def test_document_declaring_itself_current_cannot_be_transitional(self) -> None:
        """The exact row this registry shipped with, against current `main`.

        Before PR #6863 landed, classifying PLSP-SPEC-0006 `transitional` was
        true. After it landed, the amended specification retired the
        mandatory-rebase contract itself and declares that it *is* the current
        durable disposition contract. The status enum and the path both stayed
        valid, so every pre-existing check stayed green while the row asserted
        the opposite of the document -- and landing it would have demoted a
        still-authoritative document.
        """
        document = copy.deepcopy(load_registry())
        row = next(
            item
            for item in document["documents"]
            if item["path"] == "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md"
        )
        row["status"] = "transitional"
        row["successor"] = "issue #4560 / PR #6863"

        errors = validate_registry(document)
        self.assertTrue(
            any("would demote a current authority" in error for error in errors),
            errors,
        )

    def test_document_declaring_itself_retired_cannot_be_transitional(self) -> None:
        """The mirror error, from the same `main` movement.

        PR #6871 retired both review-receipt commands. A `transitional` row for
        either one asserts a replacement is still pending when it has already
        landed.
        """
        markers = {
            "scripts/ci/check-pr-claim-currentness": "review-convergence authority",
            "scripts/reviews/claim-digest": "claim-digest command-line entry point",
        }
        for path, marker in markers.items():
            with self.subTest(path=path):
                document = copy.deepcopy(load_registry())
                row = next(
                    item for item in document["documents"] if item["path"] == path
                )
                row["status"] = "transitional"
                row["successor"] = "issue #5778 / PR #6871"
                # A present marker, so the row fails on the self-declaration
                # rather than incidentally on a missing or vanished marker.
                row["stale_marker"] = marker

                errors = validate_registry(document)
                self.assertTrue(
                    any("already retired" in error for error in errors), errors
                )
                self.assertFalse(
                    any("stale_marker" in error for error in errors), errors
                )

    def test_transitional_checks_do_not_fire_on_genuine_transitional_rows(self) -> None:
        """Negative control over every row still genuinely transitional.

        Without this, the checks above could be satisfied by rules broad enough
        to condemn every transitional row, which would make the status useless.

        This control is weaker than when it was written: four of the five rows
        it covered were reclassified `current` after PR #6868 landed, leaving
        one. It narrows as the migration succeeds, which is the intended
        direction, but a single subject cannot separate a targeted rule from an
        accidentally-passing one. The mutation tests carry that weight instead.
        """
        rows = {row["path"]: row for row in load_registry()["documents"]}
        for path in sorted(REQUIRED_TRANSITIONAL):
            with self.subTest(path=path):
                text = (ROOT / path).read_text(encoding="utf-8", errors="replace")
                self.assertEqual(self_claim_contradictions(path, text), [])
                self.assertIsNone(
                    stale_marker_contradiction(path, rows[path]["stale_marker"], text)
                )

    def test_transitional_row_fails_when_its_stale_text_is_removed(self) -> None:
        """The defect no self-declaration can catch.

        `CONTRIBUTING.md` and `.github/copilot-instructions.md` went stale
        exactly this way: PR #6868 deleted the retired conveyor text those rows
        were classified for, and neither file declares a status, so every
        self-declaration check stayed green while both rows asserted a
        replacement was still pending.
        """
        document = copy.deepcopy(load_registry())
        row = next(
            item
            for item in document["documents"]
            if item["path"] == "scripts/ci/check-pr-review-convergence-core"
        )
        row["stale_marker"] = "text a successor PR has already deleted"

        errors = validate_registry(document)
        self.assertTrue(
            any("is no longer present" in error for error in errors), errors
        )

    def test_transitional_row_must_name_a_stale_marker(self) -> None:
        """A row cannot opt out of the content oracle by omitting the field."""
        document = copy.deepcopy(load_registry())
        row = next(
            item
            for item in document["documents"]
            if item["path"] == "scripts/ci/check-pr-review-convergence-core"
        )
        del row["stale_marker"]

        errors = validate_registry(document)
        self.assertTrue(
            any("must name a non-empty stale_marker" in error for error in errors),
            errors,
        )

    def test_duplicate_path_is_rejected(self) -> None:
        document = copy.deepcopy(load_registry())
        document["documents"].append(copy.deepcopy(document["documents"][0]))

        errors = validate_registry(document)
        self.assertTrue(any("duplicate path" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
