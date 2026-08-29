from __future__ import annotations

import re
import tomllib
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "docs" / "agents" / "authority_status.toml"
MIGRATOR = ROOT / "scripts" / "migrate-legacy-authority-banners.py"
WORKFLOW = ROOT / ".github" / "workflows" / "legacy-authority-banners.yml"
MARKER = "<!-- authority-status:v1 -->"
HISTORICAL_COMMIT = "f6d3f9919dca35095fa8a7b26923c3190008d040"
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
    "docs/development/RUST_1_95_ROLLOUT.md": (
        "superseded",
        "CLIPPY_POLICY.md",
    ),
    "docs/development/STRONG_CLIPPY_LINTS_ROLLOUT.md": (
        "superseded",
        "CLIPPY_POLICY.md",
    ),
    "docs/development/RUST_1_95_PROACTIVE_GUARDS.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
    "docs/ci/perl-lsp-rust-1.95-rollout.md": (
        "superseded",
        "CLIPPY_POLICY.md",
    ),
    "docs/project/RAILS_INDEX.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
}
ROLLOUT_REDIRECTS: dict[str, dict[str, str]] = {
    "docs/development/RUST_1_95_ROLLOUT.md": {
        "blob": "61284c6ceca9395c7802b0d151e9847bdbba63d4",
        "successor": "docs/CLIPPY_POLICY.md",
    },
    "docs/development/STRONG_CLIPPY_LINTS_ROLLOUT.md": {
        "blob": "1a676040c83475a9a2e7dd9a091da3bb732c4677",
        "successor": "docs/CLIPPY_POLICY.md",
    },
    "docs/development/RUST_1_95_PROACTIVE_GUARDS.md": {
        "blob": "05bfe9bf9344b4f4ea469dddbafed78eaa815131",
        "successor": "docs/agents/DEVELOPMENT_METHOD.md",
    },
    "docs/ci/perl-lsp-rust-1.95-rollout.md": {
        "blob": "b1667cef5512abb2700dd24080c2731b6e327733",
        "successor": "docs/CLIPPY_POLICY.md",
    },
    "docs/project/RAILS_INDEX.md": {
        "blob": "5d27670786f80b7274e7e1d9ff357d7ee65bfb3b",
        "successor": "docs/agents/DEVELOPMENT_METHOD.md",
    },
}
RETIRED_ROLLOUT_PATHS = tuple(ROLLOUT_REDIRECTS)
RETIRED_REFERENCE_METADATA = {
    *ROLLOUT_REDIRECTS,
    "docs/agents/AUTHORITY_STATUS.md",
    "docs/agents/authority_status.toml",
    "docs/policy/NON_RUST_INVENTORY.md",
}


def unqualified_retired_references(document: str, body: str) -> list[str]:
    """Return inbound rollout references without an explicit historical marker."""
    if document in RETIRED_REFERENCE_METADATA:
        return []

    findings: list[str] = []
    for line_number, line in enumerate(body.splitlines(), start=1):
        if any(path in line for path in RETIRED_ROLLOUT_PATHS) and not re.search(
            r"(?i)historical|superseded|immutable",
            line,
        ):
            findings.append(f"{document}:{line_number}: {line}")
    return findings


def registry_rows() -> dict[str, dict[str, Any]]:
    registry = tomllib.loads(REGISTRY.read_text(encoding="utf-8"))
    return {row["path"]: row for row in registry["documents"]}


def banner_link_targets(document: str, head: str) -> dict[str, Path]:
    """Resolve every local Markdown link in a banner to a repository path."""
    targets: dict[str, Path] = {}
    for text, href in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", head):
        if href.startswith(("http://", "https://", "#")):
            continue
        target = href.split("#", 1)[0]
        if not target:
            continue
        targets[text] = ((ROOT / document).parent / target).resolve()
    return targets


def historical_urls(document: str, head: str) -> list[str]:
    return [
        href
        for _text, href in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", head)
        if href.startswith("https://github.com/") and f"/blob/{HISTORICAL_COMMIT}/" in href
    ]


def workflow_event_paths(event: str) -> set[str]:
    lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
    event_indent = None
    paths_indent = None
    collected: set[str] = set()
    in_event = False
    in_paths = False

    for line in lines:
        stripped = line.strip()
        indent = len(line) - len(line.lstrip())
        if stripped == f"{event}:" and indent == 2:
            event_indent = indent
            in_event = True
            in_paths = False
            continue
        if in_event and event_indent is not None and stripped and indent <= event_indent:
            break
        if in_event and stripped == "paths:":
            paths_indent = indent
            in_paths = True
            continue
        if in_paths and paths_indent is not None:
            if stripped and indent <= paths_indent:
                in_paths = False
                continue
            match = re.match(r"- ['\"](.+)['\"]$", stripped)
            if match:
                collected.add(match.group(1))

    return collected


class LegacyAuthorityBannerTests(unittest.TestCase):
    def test_current_docs_do_not_depend_on_retired_rollout_paths(self) -> None:
        findings: list[str] = []
        for document in (ROOT / "docs").rglob("*.md"):
            relative = document.relative_to(ROOT).as_posix()
            findings.extend(
                unqualified_retired_references(
                    relative, document.read_text(encoding="utf-8")
                )
            )

        self.assertEqual(
            findings,
            [],
            "current docs must point at maintained authorities; retained mentions "
            "must say historical, superseded, or immutable",
        )

    def test_unqualified_retired_reference_is_rejected(self) -> None:
        self.assertEqual(
            unqualified_retired_references(
                "docs/current.md",
                "See docs/development/RUST_1_95_ROLLOUT.md for the current plan.",
            ),
            [
                "docs/current.md:1: See docs/development/RUST_1_95_ROLLOUT.md for the current plan."
            ],
        )
        self.assertEqual(
            unqualified_retired_references(
                "docs/forensics.md",
                "Historical reference: docs/development/RUST_1_95_ROLLOUT.md",
            ),
            [],
        )

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
        """Every local banner link must resolve inside the repository."""
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
                        target.is_relative_to(ROOT),
                        f"{path}: banner link [{text}] escapes the repository: {target}",
                    )
                    self.assertTrue(
                        target.is_file(),
                        f"{path}: banner link [{text}] points at missing {target}",
                    )

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
                self.assertEqual(Path(registry_successor).name, Path(successor).name)

    def test_each_document_carries_exactly_one_banner(self) -> None:
        for path in EXPECTED:
            with self.subTest(path=path):
                body = (ROOT / path).read_text(encoding="utf-8")
                self.assertEqual(
                    body.count(MARKER), 1, f"{path}: expected exactly one banner marker"
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

    def test_banner_set_matches_every_non_archive_legacy_markdown_row(self) -> None:
        legacy_markdown = {
            path
            for path, row in registry_rows().items()
            if row["status"] in {"historical", "superseded"}
            and path.endswith(".md")
            and not path.startswith("archive/")
        }

        self.assertEqual(set(EXPECTED), legacy_markdown)
        self.assertEqual(len(EXPECTED), 14)

    def test_retired_legacy_scripts_say_so(self) -> None:
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

    def test_rollout_redirects_bind_exact_historical_subject(self) -> None:
        rows = registry_rows()
        for path, expected in ROLLOUT_REDIRECTS.items():
            with self.subTest(path=path):
                row = rows[path]
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                expected_url = (
                    "https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/"
                    f"{HISTORICAL_COMMIT}/{path}"
                )

                self.assertEqual(row["status"], "superseded")
                self.assertEqual(row["successor"], expected["successor"])
                self.assertEqual(row["historical_source_commit"], HISTORICAL_COMMIT)
                self.assertEqual(row["historical_source_blob_sha1"], expected["blob"])
                self.assertEqual(historical_urls(path, head), [expected_url])
                self.assertIn("stable redirect", head)
                self.assertIn("non-executable", head)

    def test_rollout_redirects_do_not_retain_executable_queue_prose(self) -> None:
        forbidden = (
            "Remaining implementation ladder",
            "Each row is one PR",
            "Coworker agents",
            "pick from rails",
            "Umbrella: **#8590**",
            "Canonical post-landing source of truth",
        )
        for path in ROLLOUT_REDIRECTS:
            with self.subTest(path=path):
                body = (ROOT / path).read_text(encoding="utf-8")
                self.assertLessEqual(len(body.splitlines()), 24)
                for phrase in forbidden:
                    self.assertNotIn(phrase, body)

    def test_strong_clippy_redirect_rejects_the_8590_collision(self) -> None:
        body = (
            ROOT / "docs/development/STRONG_CLIPPY_LINTS_ROLLOUT.md"
        ).read_text(encoding="utf-8")

        self.assertIn("Current #8590 owns CPANTS/kwalitee oracle work", body)
        for issue in ("#9850", "#11335", "#11337", "#11404"):
            self.assertIn(issue, body)
        self.assertNotIn("Umbrella: **#8590**", body)

    def test_historical_provenance_is_unique_to_rollout_redirects(self) -> None:
        rows = registry_rows()
        marked = {
            path
            for path, row in rows.items()
            if "historical_source_commit" in row or "historical_source_blob_sha1" in row
        }
        self.assertEqual(marked, set(ROLLOUT_REDIRECTS))

    def test_legacy_workflow_filters_cover_the_banner_set_exactly(self) -> None:
        required = set(EXPECTED) | {
            "docs/agents/authority_status.toml",
            "scripts/migrate-legacy-authority-banners.py",
            "tests/test_legacy_authority_banners.py",
            ".github/workflows/legacy-authority-banners.yml",
        }
        for event in ("pull_request", "push"):
            with self.subTest(event=event):
                self.assertEqual(workflow_event_paths(event), required)

    def test_one_shot_migrator_is_inert(self) -> None:
        source = MIGRATOR.read_text(encoding="utf-8")

        self.assertIn("RETIRED", source)
        self.assertIn("tests/test_legacy_authority_banners.py", source)
        self.assertNotIn("write_text", source)
        self.assertNotIn("def migrate", source)
        self.assertIn("raise SystemExit(2)", source)


if __name__ == "__main__":
    unittest.main()
