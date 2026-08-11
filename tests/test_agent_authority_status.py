from __future__ import annotations

import copy
import tomllib
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "docs" / "agents" / "authority_status.toml"
ALLOWED_STATUSES = {"current", "transitional", "historical", "superseded"}
REQUIRED_CURRENT = {
    "AGENTS.md",
    "CLAUDE.md",
    "docs/agents/DEVELOPMENT_METHOD.md",
    "docs/agents/REVIEW_CURRENTNESS.md",
    "docs/agents/GITHUB_SURFACES.md",
    "docs/agents/SKILL_CONTRACT.md",
    "docs/how-to/SESSION_OPERATIONS.md",
    "docs/how-to/AGENT_CONTRIBUTING.md",
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
    "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    "docs/reference/MAINTAINER_AGENT_DOCTRINE.md",
    "docs/reference/WORKTREE_PROTOCOL.md",
    "CONTRIBUTING.md",
    ".github/copilot-instructions.md",
    "scripts/ci/check-pr-claim-currentness",
    "scripts/reviews/claim-digest",
    "scripts/ci/check-pr-review-convergence-core",
}


def load_registry() -> dict[str, Any]:
    return tomllib.loads(REGISTRY.read_text(encoding="utf-8"))


def prose(path: Path) -> str:
    return " ".join(path.read_text(encoding="utf-8").split())


def validate_registry(document: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if document.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if document.get("tracking_issue") != 4555:
        errors.append("tracking_issue must remain #4555")

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
            if row["path"] != "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md"
        ]

        errors = validate_registry(document)
        self.assertTrue(any("missing transitional paths" in error for error in errors), errors)

    def test_duplicate_path_is_rejected(self) -> None:
        document = copy.deepcopy(load_registry())
        document["documents"].append(copy.deepcopy(document["documents"][0]))

        errors = validate_registry(document)
        self.assertTrue(any("duplicate path" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
