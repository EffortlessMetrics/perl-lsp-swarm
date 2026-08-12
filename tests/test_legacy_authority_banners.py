from __future__ import annotations

import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "docs" / "agents" / "authority_status.toml"
MIGRATOR = ROOT / "scripts" / "migrate-legacy-authority-banners.py"
MARKER = "<!-- authority-status:v1 -->"
EXPECTED = {
    "docs/reference/ORCHESTRATION_DOCTRINE.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
    "docs/reference/PIPELINE_GATES.md": ("superseded", "DEVELOPMENT_METHOD.md"),
    "docs/reference/OCTOPUS_CLUSTER.md": ("historical", "DEVELOPMENT_METHOD.md"),
    "docs/reference/GLOSSARY.md": ("superseded", "AUTHORITY_STATUS.md"),
    "docs/reference/LIVE_SIGNALS_VS_LABELS.md": (
        "historical",
        "GITHUB_SURFACES.md",
    ),
    "docs/adr/0044-octopus-cluster-orchestration.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
    "docs/articles/PIPELINE_STATE_MACHINE.md": (
        "historical",
        "GITHUB_SURFACES.md",
    ),
    "docs/handoff/SWARM_DESIGN.md": ("historical", "DEVELOPMENT_METHOD.md"),
    ".spec/3988-merge-readiness/spec.md": (
        "historical",
        "REVIEW_CURRENTNESS.md",
    ),
}


def registry_rows() -> dict[str, dict[str, object]]:
    registry = tomllib.loads(REGISTRY.read_text(encoding="utf-8"))
    return {row["path"]: row for row in registry["documents"]}


class LegacyAuthorityBannerTests(unittest.TestCase):
    def test_local_banners_match_registry(self) -> None:
        by_path = registry_rows()

        for path, (status, successor) in EXPECTED.items():
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                self.assertIn(MARKER, head)
                self.assertIn(f"Status: {status}.", head)
                self.assertIn("AUTHORITY_STATUS.md", head)
                self.assertIn(successor, head)
                self.assertEqual(by_path[path]["status"], status)

    def test_explicit_old_statuses_are_not_still_current(self) -> None:
        pipeline = "\n".join(
            (ROOT / "docs/reference/PIPELINE_GATES.md")
            .read_text(encoding="utf-8")
            .splitlines()[:24]
        )
        adr = "\n".join(
            (ROOT / "docs/adr/0044-octopus-cluster-orchestration.md")
            .read_text(encoding="utf-8")
            .splitlines()[:24]
        )

        self.assertNotIn("**Status**: Active doctrine", pipeline)
        self.assertIn("**Status**: Superseded", pipeline)
        self.assertNotIn("**Status**: Accepted", adr)
        self.assertIn("**Status**: Superseded", adr)

    def test_banner_set_matches_every_legacy_registry_row(self) -> None:
        legacy_paths = {
            path
            for path, row in registry_rows().items()
            if row["status"] in {"historical", "superseded"}
        }

        self.assertEqual(set(EXPECTED), legacy_paths)
        self.assertEqual(len(EXPECTED), 9)

    def test_one_shot_migrator_is_inert(self) -> None:
        source = MIGRATOR.read_text(encoding="utf-8")

        self.assertIn("RETIRED", source)
        self.assertIn("tests/test_legacy_authority_banners.py", source)
        self.assertNotIn("write_text", source)
        self.assertNotIn("def migrate", source)
        self.assertIn("raise SystemExit(2)", source)


if __name__ == "__main__":
    unittest.main()
