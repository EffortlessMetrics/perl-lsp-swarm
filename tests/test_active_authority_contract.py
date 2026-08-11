from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def read_prose(path: str) -> str:
    """Normalize Markdown line wrapping without weakening phrase checks."""

    return " ".join(read(path).split())


def assert_contains_all(
    case: unittest.TestCase,
    surface: str,
    name: str,
    required: tuple[str, ...],
) -> None:
    for marker in required:
        case.assertIn(marker, surface, f"{name} is missing current marker {marker!r}")


def assert_contains_none(
    case: unittest.TestCase,
    surface: str,
    name: str,
    forbidden: tuple[str, ...],
) -> None:
    for marker in forbidden:
        case.assertNotIn(marker, surface, f"{name} restored retired marker {marker!r}")


class ActiveAuthorityContractTests(unittest.TestCase):
    def test_maintainer_contract_is_current(self) -> None:
        doctrine = read_prose("docs/reference/MAINTAINER_AGENT_DOCTRINE.md")

        assert_contains_all(
            self,
            doctrine,
            "maintainer doctrine",
            (
                "Status: current authority",
                "maintainer or system ruling",
                "evidence, current source, and external constraints",
                "one mutation owner",
                "Behind-only movement requires no action.",
                "There is no mechanical one-rebase limit.",
                "Labels are navigation.",
            ),
        )
        assert_contains_none(
            self,
            doctrine,
            "maintainer doctrine",
            (
                "prefer GitHub branch update or ordinary rebase",
                "the north star for *why* the conveyor",
                "The conveyor",
                "agents propose; the reconciler disposes",
            ),
        )

    def test_contributing_uses_current_review_model(self) -> None:
        contributing = read_prose("CONTRIBUTING.md")

        assert_contains_all(
            self,
            contributing,
            "CONTRIBUTING.md",
            (
                "agent contributing guide",
                "Rust channel `1.95.0`",
                "MSRV 1.95",
                "There is no fixed two-model review ladder",
                "Labels may help navigation. They are not proof or merge permission.",
                "Behind-only movement requires no action.",
                "At merge, the current head is used as compare-and-swap protection",
                "## Specialized contributor workflows",
                "just semver-check",
                "just public-api-update",
                "just publish-allowlist-check",
                "just bump-version X.Y.Z",
                "docs/assets/gifs/README.md",
                "Contributions are licensed under both [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).",
            ),
        )
        assert_contains_none(
            self,
            contributing,
            "CONTRIBUTING.md",
            (
                "haiku-tier",
                "sonnet-tier",
                "The CI merge gate only runs on `merge-ready` PRs",
                "`merge-ready` | Approved and ready for merge",
                "--base origin/master",
            ),
        )

    def test_copilot_is_a_current_route_map(self) -> None:
        copilot = read_prose(".github/copilot-instructions.md")

        assert_contains_all(
            self,
            copilot,
            "Copilot instructions",
            (
                "This file is a concise route map",
                "Historical articles, forensics, completed implementation specs",
                "one mutation owner",
                "There is no one-rebase quota.",
                "Required GitHub statuses remain attached to the commit they evaluated.",
            ),
        )
        assert_contains_none(
            self,
            copilot,
            "Copilot instructions",
            (
                "80+ crates",
                "CI is optional/opt-in",
                "`/crates/perl-lsp/`",
                "`perl-workspace-index`",
                "Gate 1",
                "Gate 7",
            ),
        )

    def test_worktree_mutation_has_a_concrete_reason(self) -> None:
        protocol = read_prose("docs/reference/WORKTREE_PROTOCOL.md")

        assert_contains_all(
            self,
            protocol,
            "worktree protocol",
            (
                "Status: current operational reference",
                "one mutation owner",
                "Behind-only movement requires no action.",
                "There is no mechanical one-rebase limit.",
                "The repository's `scripts/cargo-safe` and `just agent-*` commands are a deliberate",
                '--force-with-lease="refs/heads/<branch>:<expected-old-sha>"',
                "A squash merge does not preserve feature-branch ancestry on `main`.",
            ),
        )
        assert_contains_none(
            self,
            protocol,
            "worktree protocol",
            (
                "origin/master",
                "main checkout stays on `master`",
                "claim/lease protocol described in issue",
                "restrict each box to a disjoint set of issue numbers",
            ),
        )

    def test_workflow_tracks_current_entrypoints(self) -> None:
        workflow = read(".github/workflows/active-authority-contract.yml")

        for path in (
            "CONTRIBUTING.md",
            ".github/copilot-instructions.md",
            "docs/reference/MAINTAINER_AGENT_DOCTRINE.md",
            "docs/reference/WORKTREE_PROTOCOL.md",
            "tests/test_active_authority_contract.py",
            ".github/workflows/active-authority-contract.yml",
        ):
            self.assertIn(
                f"- '{path}'",
                workflow,
                f"active-authority workflow must trigger for {path}",
            )

        self.assertIn(
            "python3 -m unittest tests/test_active_authority_contract.py",
            workflow,
        )
        self.assertNotIn("cargo test", workflow)
        self.assertNotIn("cargo fmt", workflow)


if __name__ == "__main__":
    unittest.main()
