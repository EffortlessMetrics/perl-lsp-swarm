from __future__ import annotations

import re
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


def banner_link_targets(document: str, head: str) -> dict[str, Path]:
    """Resolve every Markdown link in a banner to a real filesystem path.

    A banner is a redirection: its whole job is to send a reader somewhere that
    still has authority. Asserting the successor's *name* appears in the header
    cannot tell a working link from a broken one -- the name matches just as well
    inside a typo'd path, a link to a moved file, or bare prose with no link at
    all. Resolving the target relative to the document catches all three.
    """
    targets: dict[str, Path] = {}
    for text, href in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", head):
        if href.startswith(("http://", "https://", "#")):
            continue
        target = href.split("#", 1)[0]
        if not target:
            continue
        targets[text] = ((ROOT / document).parent / target).resolve()
    return targets


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

    def test_banner_links_resolve_to_real_files(self) -> None:
        """Every banner link must point at a file that exists.

        This is the half `assertIn(successor, head)` cannot prove: a substring
        match is satisfied by a link whose href is wrong, so a banner could send
        every reader of a superseded document to a 404 and still pass.
        """
        for path, (_status, successor) in EXPECTED.items():
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                targets = banner_link_targets(path, head)
                self.assertTrue(
                    targets, f"{path}: banner has no resolvable Markdown link"
                )
                for text, target in targets.items():
                    self.assertTrue(
                        target.is_file(),
                        f"{path}: banner link [{text}] points at missing {target}",
                    )

                # The authority index and the named successor must each be a
                # real link target, not merely words somewhere in the header --
                # and must be *the* canonical file, not merely something with a
                # matching basename. Comparing names would accept a link to any
                # other AUTHORITY_STATUS.md in the tree, which is the same class
                # of near-miss as the plain-text mention this check replaced.
                linked = set(targets.values())
                registry_successor = registry_rows()[path]["successor"]
                self.assertIn(
                    (ROOT / "docs/agents/AUTHORITY_STATUS.md").resolve(),
                    linked,
                    f"{path}: banner does not link the canonical authority index",
                )
                self.assertIn(
                    (ROOT / registry_successor).resolve(),
                    linked,
                    f"{path}: banner does not link the registry successor "
                    f"{registry_successor}",
                )
                # Guard the hardcoded expectation against the registry, so a
                # successor change cannot leave this test asserting the old one.
                self.assertEqual(Path(registry_successor).name, Path(successor).name)

    def test_each_document_carries_exactly_one_banner(self) -> None:
        """A second marker means two competing local statuses.

        The migration appends a banner; running it twice, or hand-adding one to
        an already-migrated document, yields a file whose first banner says one
        thing and whose second says another. Every other check reads only the
        first 24 lines, so the duplicate is invisible to them.
        """
        for path in EXPECTED:
            with self.subTest(path=path):
                body = (ROOT / path).read_text(encoding="utf-8")
                self.assertEqual(
                    body.count(MARKER), 1, f"{path}: expected exactly one banner marker"
                )

    def test_banner_links_stay_inside_the_repository(self) -> None:
        """Reject a relative href that escapes the repository root.

        `../../..`-style traversal resolves to a path outside the tree. On a
        developer machine that can still be a real file, so `is_file()` alone
        does not catch it.
        """
        for path in EXPECTED:
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                for text, target in banner_link_targets(path, head).items():
                    self.assertTrue(
                        target.is_relative_to(ROOT),
                        f"{path}: banner link [{text}] escapes the repository: {target}",
                    )

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

    def test_banner_set_matches_every_legacy_markdown_row(self) -> None:
        """Every legacy *document* must carry a banner.

        Scoped to Markdown because a banner is an HTML comment plus Markdown
        prose. Executable files cannot carry one -- `<!-- ... -->` is not a
        Python comment -- so a retired script declares its status in its own
        docstring instead, which `test_retired_legacy_scripts_say_so` checks.
        Without this split, reclassifying a retired script in the registry would
        demand a Markdown banner in a Python file.
        """
        legacy_markdown = {
            path
            for path, row in registry_rows().items()
            if row["status"] in {"historical", "superseded"} and path.endswith(".md")
        }

        self.assertEqual(set(EXPECTED), legacy_markdown)
        self.assertEqual(len(EXPECTED), 9)

    def test_retired_legacy_scripts_say_so(self) -> None:
        """The non-Markdown half of the same inventory.

        These rows are legacy too, so they still need a machine-checked local
        disposition -- just one their file format can carry.
        """
        legacy_scripts = {
            path
            for path, row in registry_rows().items()
            if row["status"] in {"historical", "superseded"}
            and not path.endswith(".md")
        }
        self.assertTrue(legacy_scripts, "expected retired commands in the registry")

        for path in sorted(legacy_scripts):
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:40]
                )
                self.assertRegex(
                    head,
                    r"RETIRED|[Rr]etired|no longer has",
                    f"{path}: retired command does not declare its own status",
                )

    def test_one_shot_migrator_is_inert(self) -> None:
        source = MIGRATOR.read_text(encoding="utf-8")

        self.assertIn("RETIRED", source)
        self.assertIn("tests/test_legacy_authority_banners.py", source)
        self.assertNotIn("write_text", source)
        self.assertNotIn("def migrate", source)
        self.assertIn("raise SystemExit(2)", source)


if __name__ == "__main__":
    unittest.main()
