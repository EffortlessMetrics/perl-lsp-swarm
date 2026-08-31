from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

# Surfaces this contract owns.
MAINTAINER_DOCTRINE = "docs/reference/MAINTAINER_AGENT_DOCTRINE.md"
CONTRIBUTING = "CONTRIBUTING.md"
COPILOT = ".github/copilot-instructions.md"
WORKTREE_PROTOCOL = "docs/reference/WORKTREE_PROTOCOL.md"
WORKFLOW = ".github/workflows/active-authority-contract.yml"
SELF_TEST = "tests/test_active_authority_contract.py"

# Surfaces this contract reads to check the entrypoints against real repository
# state instead of against themselves.
ROUTE_AUTHORITIES = ("AGENTS.md", "CLAUDE.md")
JUSTFILE = "justfile"
GITIGNORE = ".gitignore"
ORCHESTRATION_SKILL_SURFACES = (
    ".agents/skills/deliver-goal/SKILL.md",
    ".agents/skills/deliver-pr/SKILL.md",
    ".agents/skills/orchestrate-work/SKILL.md",
    ".claude/skills/deliver-goal/SKILL.md",
    ".claude/skills/deliver-pr/SKILL.md",
    ".claude/skills/orchestrate-work/SKILL.md",
)
ACTIVE_SKILL_ROOTS = (".agents/skills", ".claude/skills")
ACTIVE_SKILL_GLOBS = (".agents/skills/**", ".claude/skills/**")
SHARED_ORCHESTRATION_SURFACES = (
    "docs/agents/DEVELOPMENT_METHOD.md",
    "docs/agents/REVIEW_CURRENTNESS.md",
    "docs/agents/SKILL_CONTRACT.md",
)
CLAUDE_AGENT_ROSTER = ".claude/agents/README.md"
CLAUDE_AGENT_GLOB = ".claude/agents/**"
RETIRED_LANE_AGENT = ".claude/agents/lane-orchestrator.md"

PARITY_SKILLS = (
    "address-review-comments",
    "build-candidate",
    "deliver-goal",
    "deliver-pr",
    "final-challenge",
    "finish-pr",
    "improve-test-suite",
    "merge-reconcile",
    "orchestrate-work",
    "prepare-issue",
    "prepare-proof",
    "review-candidate",
    "review-pr",
    "review-tests",
    "simplify-candidate",
    "verify-live-ci",
)

KNOWN_SKILLS = frozenset(
    {
        "deliver-goal",
        "deliver-pr",
        "prepare-issue",
        "prepare-proof",
        "build-candidate",
        "finish-pr",
        "find-or-create-issue",
        "research-issue",
        "review-issue",
        "issue-to-plan",
        "research-plan",
        "review-plan",
        "compile-spec",
        "spec-to-test",
        "review-tests",
        "build-from-proof",
        "improve-test-suite",
        "simplify-candidate",
        "review-candidate",
        "publish-pr",
        "address-review-comments",
        "final-challenge",
        "review-pr",
        "verify-live-ci",
        "merge-reconcile",
        "orchestrate-work",
    }
)

SEMANTIC_PARITY_MARKERS: dict[str, tuple[str, ...]] = {
    "address-review-comments": ("NOT_PROVEN", "final-challenge"),
    "build-candidate": ("CANDIDATE_READY", "NOT_PROVEN", "prepare-proof"),
    "deliver-goal": ("IN_FLIGHT", "deliver-pr", "NOT_PROVEN"),
    "deliver-pr": ("IN_FLIGHT", "NOT_PROVEN", "finish-pr"),
    "final-challenge": ("CANDIDATE_READY_FOR_REVIEW", "NOT_PROVEN", "review-pr"),
    "finish-pr": ("REVIEW_CURRENT", "CHANGES_REQUIRED", "INTEGRATION_READY", "NOT_PROVEN"),
    "improve-test-suite": ("NOT_PROVEN", "review-tests", "simplify-candidate"),
    "merge-reconcile": (
        "REVIEW_CURRENT",
        "INTEGRATION_READY",
        "PR_IN_FLIGHT",
        "NOT_PROVEN",
        "--match-head-commit",
        "autoMergeRequest",
        "Return `PR_IN_FLIGHT` only after a fresh GitHub read confirms",
    ),
    "orchestrate-work": ("NOT_PROVEN", "deliver-pr", "review-pr", "verify-live-ci"),
    "prepare-issue": ("PLAN_READY", "NOT_PROVEN", "prepare-proof"),
    "prepare-proof": ("PROOF_READY", "NOT_PROVEN", "build-candidate"),
    "review-candidate": ("CANDIDATE_READY", "NOT_PROVEN", "prepare-proof"),
    "review-pr": (
        "REVIEW_CURRENT",
        "CHANGES_REQUIRED",
        "NOT_PROVEN",
        "semantic-review:v1",
        "verify-live-ci",
        "Never include both `## Findings` and `## No material findings`",
    ),
    "review-tests": ("NOT_PROVEN", "build-candidate", "prepare-proof"),
    "simplify-candidate": ("ALREADY_MINIMAL", "NOT_PROVEN", "review-candidate"),
    "verify-live-ci": (
        "REVIEW_CURRENT",
        "INTEGRATION_READY",
        "PR_IN_FLIGHT",
        "MERGE_BLOCKED",
        "NOT_PROVEN",
        "a rerun of a merge-tree-evaluated check replays its original merge snapshot",
        "re-evaluates the current tree and is the honest action",
    ),
}

REQUIRED_ROUTE_EDGES: dict[str, frozenset[tuple[str, str]]] = {
    "review-pr": frozenset(
        {
            ("REVIEW_CURRENT", "verify-live-ci"),
            ("CHANGES_REQUIRED", "address-review-comments"),
        }
    ),
    "merge-reconcile": frozenset(
        {
            ("REVIEW_REQUIRED", "finish-pr"),
            ("REVIEW_REQUIRED", "review-pr"),
            ("CHANGES_REQUIRED", "address-review-comments"),
            ("PR_IN_FLIGHT", "deliver-goal"),
        }
    ),
}

# Every path that must re-run this contract, under both workflow events.
TRIGGER_PATHS = (
    CONTRIBUTING,
    COPILOT,
    MAINTAINER_DOCTRINE,
    WORKTREE_PROTOCOL,
    "AGENTS.md",
    "CLAUDE.md",
    *ACTIVE_SKILL_GLOBS,
    CLAUDE_AGENT_GLOB,
    *SHARED_ORCHESTRATION_SURFACES,
    JUSTFILE,
    GITIGNORE,
    SELF_TEST,
    WORKFLOW,
)

HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)
FENCED_BLOCK = re.compile(r"^```.*?^```", re.DOTALL | re.MULTILINE)
JUST_CALL = re.compile(r"\bjust ([a-z0-9][a-z0-9-]*)")
JUST_RECIPE_DEF = re.compile(r"^([a-z0-9_][a-z0-9_-]*)(?:\s+[^\n]*?)?:(?!=)", re.MULTILINE)
FLOW_ROSTER_HEADING = "Choose the narrowest applicable public flow:"
FLOW_BULLET = re.compile(r"^- `\$?([a-z][a-z-]*)`", re.MULTILINE)
BACKTICK_SKILL = re.compile(r"`\$?([a-z][a-z0-9-]*)`")
BACKTICK_RESULT = re.compile(r"`([A-Z][A-Z0-9_]+)`")
ROUTE_BULLET = re.compile(r"(?ms)^\s*-\s+(.*?)(?=^\s*-\s+|^##\s|\Z)")
RETIRED_ACTIVE_SKILL_AUTHORITY = (
    re.compile(r"\bcampaign root\b"),
    re.compile(r"\blane root\b"),
    re.compile(r"\blane-root\b"),
    re.compile(r"\blane owner\b"),
)
SUBORDINATE_AUTHORITY = re.compile(
    r"\b(?:the|a|one)\s+(?:claim|lane)(?:[- ](?:root|orchestrator|coordinator|owner))"
    r"\b[^.]{0,100}\b(?:owns?|retains?|decides?|joins?|posts?|selects?|routes?|runs?)\b",
    re.IGNORECASE,
)
CODEX_ROOT_AUTHORITY = re.compile(r"\b(?:accountable root|Codex root|main/root(?: Codex)? (?:thread|orchestrator))\b")
CLAUDE_ROOT_AUTHORITY = re.compile(r"\bmain Claude thread\b")


def result_route_edges(text: str) -> set[tuple[str, str]]:
    """Extract result-to-skill edges from reader-visible Markdown route bullets."""

    edges: set[tuple[str, str]] = set()
    for bullet in ROUTE_BULLET.findall(text):
        if "→" not in bullet:
            continue
        source, destination = bullet.split("→", 1)
        results = BACKTICK_RESULT.findall(source)
        skills = [token for token in BACKTICK_SKILL.findall(destination) if token in KNOWN_SKILLS]
        edges.update((result, skill) for result in results for skill in skills)
    return edges


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def strip_hidden(text: str) -> str:
    """Drop HTML comments.

    Text a reader never sees must never satisfy a currentness marker, so no
    surface derived here retains commented-out content.
    """

    return HTML_COMMENT.sub(" ", text)


def normalize(text: str) -> str:
    """Normalize Markdown line wrapping without weakening phrase checks."""

    return " ".join(text.split())


def active_text(path_or_text: str, *, is_text: bool = False) -> str:
    """Reader-visible text, including fenced command and diagram blocks."""

    raw = path_or_text if is_text else read(path_or_text)
    return normalize(strip_hidden(raw))


def visible_text(path_or_text: str, *, is_text: bool = False) -> str:
    """Reader-visible text with line structure preserved.

    Command extraction must not normalize newlines away: `cargo install just`
    followed by `just devex` would otherwise read as the call `just just`.
    """

    raw = path_or_text if is_text else read(path_or_text)
    return strip_hidden(raw)


def prose_text(path_or_text: str, *, is_text: bool = False) -> str:
    """Reader-visible narrative prose, with fenced blocks removed.

    A policy sentence that survives only inside a code fence, a quoted retired
    excerpt, or an HTML comment is not the document's active claim, so policy
    markers are asserted against this surface rather than the flattened file.
    """

    raw = path_or_text if is_text else read(path_or_text)
    return normalize(FENCED_BLOCK.sub(" ", strip_hidden(raw)))


def workflow_event_paths(text: str, event: str) -> tuple[str, ...]:
    """Extract `on.<event>.paths` for one event only.

    Scoping per event is deliberate: a whole-file substring search stays green
    when a path is dropped from `pull_request` but kept under `push`, which
    silently disables candidate-time or post-merge enforcement.
    """

    on_block = re.search(r"^on:\n((?:[ \t].*\n|\n)*)", text, re.MULTILINE)
    if on_block is None:
        return ()
    event_block = re.search(
        rf"^  {re.escape(event)}:\n((?:    .*\n|\n)*)", on_block.group(1), re.MULTILINE
    )
    if event_block is None:
        return ()
    paths_block = re.search(r"^    paths:\n((?:      - .*\n)*)", event_block.group(1), re.MULTILINE)
    if paths_block is None:
        return ()
    return tuple(
        line.strip().removeprefix("- ").strip().strip("'\"")
        for line in paths_block.group(1).splitlines()
        if line.strip()
    )


def declared_flows(text: str) -> tuple[str, ...]:
    """Public flow names from a route authority's narrowest-flow roster."""

    _, _, tail = text.partition(FLOW_ROSTER_HEADING)
    roster, _, _ = tail.partition("\n\n\n")
    section = roster.split("\n\n")[1] if len(roster.split("\n\n")) > 1 else roster
    return tuple(FLOW_BULLET.findall(section))


def skill_references(text: str) -> frozenset[str]:
    """Normalize provider-local `$skill`/`skill` references to one semantic set."""

    return frozenset(token for token in BACKTICK_SKILL.findall(text) if token in KNOWN_SKILLS)


def just_recipes(text: str) -> frozenset[str]:
    return frozenset(JUST_RECIPE_DEF.findall(text))


def referenced_just_recipes(text: str) -> frozenset[str]:
    """`just <recipe>` calls a document tells a reader to run.

    Glob prose such as `just agent-*` is not a runnable recipe name and is
    excluded rather than asserted.
    """

    return frozenset(name for name in JUST_CALL.findall(text) if not name.endswith("-"))


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
        assert_contains_all(
            self,
            prose_text(MAINTAINER_DOCTRINE),
            "maintainer doctrine prose",
            (
                "Status: current authority",
                "Behind-only movement requires no action.",
                "There is no mechanical one-rebase limit.",
                "Labels are navigation.",
            ),
        )
        assert_contains_all(
            self,
            active_text(MAINTAINER_DOCTRINE),
            "maintainer doctrine",
            (
                "maintainer or system ruling",
                "evidence, current source, and external constraints",
                "one mutation owner",
            ),
        )
        assert_contains_none(
            self,
            active_text(MAINTAINER_DOCTRINE),
            "maintainer doctrine",
            (
                "prefer GitHub branch update or ordinary rebase",
                "the north star for *why* the conveyor",
                "The conveyor",
                "agents propose; the reconciler disposes",
            ),
        )

    def test_contributing_uses_current_review_model(self) -> None:
        assert_contains_all(
            self,
            prose_text(CONTRIBUTING),
            "CONTRIBUTING.md prose",
            (
                "agent contributing guide",
                "Rust channel `1.95.0`",
                "MSRV 1.95",
                "There is no fixed two-model review ladder",
                "Labels may help navigation. They are not proof or merge permission.",
                "Behind-only movement requires no action.",
                "At merge, the current head is used as compare-and-swap protection",
                "## Specialized contributor workflows",
                "Production code must not introduce `unwrap`, `expect`, `panic!`, `todo!`, "
                "`unimplemented!`, `abort`, or `dbg!` outside a documented narrow exception",
                "docs/assets/gifs/README.md",
                "Contributions are licensed under both [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).",
            ),
        )
        assert_contains_all(
            self,
            active_text(CONTRIBUTING),
            "CONTRIBUTING.md",
            (
                "just semver-check",
                "just public-api-update",
                "just publish-allowlist-check",
                "just bump-version X.Y.Z",
            ),
        )
        assert_contains_none(
            self,
            active_text(CONTRIBUTING),
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
        assert_contains_all(
            self,
            prose_text(COPILOT),
            "Copilot instructions prose",
            (
                "This file is a concise route map",
                "Historical articles, forensics, completed implementation specs",
                "There is no one-rebase quota.",
                "Required GitHub statuses remain attached to the commit they evaluated.",
                "`unwrap` or `expect` outside a documented narrow exception",
                "`panic!`, `todo!`, `unimplemented!`, or `abort`",
            ),
        )
        assert_contains_all(
            self,
            active_text(COPILOT),
            "Copilot instructions",
            ("one mutation owner",),
        )
        assert_contains_none(
            self,
            active_text(COPILOT),
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
        assert_contains_all(
            self,
            prose_text(WORKTREE_PROTOCOL),
            "worktree protocol prose",
            (
                "Status: current operational reference",
                "one mutation owner",
                "Behind-only movement requires no action.",
                "There is no mechanical one-rebase limit.",
                "The repository's `scripts/cargo-safe` and `just agent-*` commands are a deliberate",
                "A squash merge does not preserve feature-branch ancestry on `main`.",
            ),
        )
        assert_contains_all(
            self,
            active_text(WORKTREE_PROTOCOL),
            "worktree protocol",
            ('--force-with-lease="refs/heads/<branch>:<expected-old-sha>"',),
        )
        assert_contains_none(
            self,
            active_text(WORKTREE_PROTOCOL),
            "worktree protocol",
            (
                "origin/master",
                "main checkout stays on `master`",
                "claim/lease protocol described in issue",
                "restrict each box to a disjoint set of issue numbers",
            ),
        )


class CrossSurfaceInvariantTests(unittest.TestCase):
    """Check the entrypoints against real repository state, not against themselves."""

    def test_every_documented_just_recipe_exists(self) -> None:
        recipes = just_recipes(read(JUSTFILE))
        self.assertIn("pr-fast", recipes, "justfile recipe extraction produced no useful names")
        for surface in (CONTRIBUTING, COPILOT, MAINTAINER_DOCTRINE, WORKTREE_PROTOCOL):
            for name in sorted(referenced_just_recipes(visible_text(surface))):
                self.assertIn(
                    name,
                    recipes,
                    f"{surface} tells a reader to run `just {name}`, "
                    f"which is not defined in {JUSTFILE}",
                )

    def test_route_map_names_every_public_flow(self) -> None:
        rosters = {name: declared_flows(read(name)) for name in ROUTE_AUTHORITIES}
        for name, flows in rosters.items():
            self.assertIn("deliver-pr", flows, f"{name} route roster did not parse")
        self.assertEqual(
            *(set(flows) for flows in rosters.values()),
            "root route authorities disagree about the public flow roster",
        )
        copilot = active_text(COPILOT)
        for flow in sorted(set(next(iter(rosters.values())))):
            self.assertIn(
                f"`{flow}`",
                copilot,
                f"{COPILOT} omits public flow `{flow}` named by {' and '.join(ROUTE_AUTHORITIES)}",
            )

    def test_root_owns_claim_orchestration(self) -> None:
        for authority in ROUTE_AUTHORITIES:
            text = prose_text(authority)
            self.assertIn(
                "accountable orchestrator",
                text,
                f"{authority} no longer names the root as accountable orchestrator",
            )
            self.assertIn(
                "logical claim frames",
                text,
                f"{authority} no longer keeps logical claim state in the root",
            )

        for surface in ORCHESTRATION_SKILL_SURFACES + SHARED_ORCHESTRATION_SURFACES:
            lowered = active_text(surface).lower()
            for retired in ("campaign root", "lane root", "lane-orchestrator"):
                self.assertNotIn(
                    retired,
                    lowered,
                    f"{surface} restored retired nested orchestration marker {retired!r}",
                )
            for retired in RETIRED_ACTIVE_SKILL_AUTHORITY:
                self.assertIsNone(
                    retired.search(lowered),
                    f"{surface} restored retired claim-orchestration authority "
                    f"{retired.pattern!r}",
                )

        for skill_root in ACTIVE_SKILL_ROOTS:
            for skill_path in sorted((ROOT / skill_root).rglob("SKILL.md")):
                surface = active_text(str(skill_path), is_text=False)
                lowered = surface.lower()
                relative = skill_path.relative_to(ROOT).as_posix()
                for retired in RETIRED_ACTIVE_SKILL_AUTHORITY:
                    self.assertIsNone(
                        retired.search(lowered),
                        f"{relative} restored retired claim-orchestration authority "
                        f"{retired.pattern!r}",
                    )
                self.assertIsNone(
                    SUBORDINATE_AUTHORITY.search(surface),
                    f"{relative} assigned decision authority to a subordinate claim/lane actor",
                )

        self.assertFalse(
            (ROOT / RETIRED_LANE_AGENT).exists(),
            "the retired lane-orchestrator profile returned as an active Claude agent",
        )
        roster = prose_text(CLAUDE_AGENT_ROSTER)
        self.assertIn("main Claude thread owns orchestration", roster)
        for agent in ("`researcher`", "`builder`", "`reviewer`"):
            self.assertIn(agent, roster)

    def test_provider_skill_route_and_semantic_parity(self) -> None:
        for skill in PARITY_SKILLS:
            codex_path = f".agents/skills/{skill}/SKILL.md"
            claude_path = f".claude/skills/{skill}/SKILL.md"
            codex = active_text(codex_path)
            claude = active_text(claude_path)
            codex_visible = visible_text(codex_path)
            claude_visible = visible_text(claude_path)

            self.assertEqual(
                skill_references(codex),
                skill_references(claude),
                f"{skill} provider implementations disagree on callable skill references",
            )
            codex_edges = result_route_edges(codex_visible)
            claude_edges = result_route_edges(claude_visible)
            self.assertEqual(
                codex_edges,
                claude_edges,
                f"{skill} provider implementations disagree on result-to-route edges",
            )
            for required_edge in REQUIRED_ROUTE_EDGES.get(skill, frozenset()):
                self.assertIn(
                    required_edge,
                    codex_edges,
                    f"{codex_path} lost required result-to-route edge {required_edge!r}",
                )
                self.assertIn(
                    required_edge,
                    claude_edges,
                    f"{claude_path} lost required result-to-route edge {required_edge!r}",
                )
            self.assertRegex(
                codex,
                CODEX_ROOT_AUTHORITY,
                f"{codex_path} does not positively retain orchestration in the Codex root",
            )
            self.assertRegex(
                claude,
                CLAUDE_ROOT_AUTHORITY,
                f"{claude_path} does not positively retain orchestration in the main Claude thread",
            )
            for marker in SEMANTIC_PARITY_MARKERS[skill]:
                self.assertIn(marker, codex, f"{codex_path} is missing semantic invariant {marker!r}")
                self.assertIn(marker, claude, f"{claude_path} is missing semantic invariant {marker!r}")

    def test_documented_worktree_root_is_ignored(self) -> None:
        protocol = active_text(WORKTREE_PROTOCOL)
        self.assertIn(
            "`/.worktrees/` is ignored in `.gitignore`",
            prose_text(WORKTREE_PROTOCOL),
            "worktree protocol no longer states where linked checkouts may live",
        )
        self.assertIn("git worktree add", protocol)
        self.assertIn(
            "/.worktrees/",
            read(GITIGNORE).splitlines().__str__(),
            "the documented linked-worktree root is not ignored, so a linked checkout "
            "would appear as untracked content in the coordination checkout",
        )


class WorkflowContractTests(unittest.TestCase):
    def test_workflow_tracks_current_entrypoints_under_both_events(self) -> None:
        workflow = read(WORKFLOW)
        for event in ("pull_request", "push"):
            self.assertEqual(
                set(workflow_event_paths(workflow, event)),
                set(TRIGGER_PATHS),
                f"active-authority workflow `on.{event}.paths` does not match the "
                "surfaces this contract reads",
            )

    def test_workflow_runs_only_its_own_surface(self) -> None:
        workflow = read(WORKFLOW)
        self.assertIn(f"python3 -m unittest {SELF_TEST}", workflow)
        self.assertNotIn("cargo test", workflow)
        self.assertNotIn("cargo fmt", workflow)


class RatchetSelfTests(unittest.TestCase):
    """Negative controls: prove the helpers reject the bypasses they exist to catch."""

    def test_html_comment_does_not_satisfy_a_marker(self) -> None:
        hidden = "Active prose says nothing.\n<!-- Behind-only movement requires no action. -->\n"
        self.assertNotIn(
            "Behind-only movement requires no action.", active_text(hidden, is_text=True)
        )
        self.assertNotIn(
            "Behind-only movement requires no action.", prose_text(hidden, is_text=True)
        )

    def test_fenced_block_does_not_satisfy_a_prose_marker(self) -> None:
        fenced = "Intro.\n\n```text\nBehind-only movement requires no action.\n```\n\nOutro.\n"
        self.assertIn("Behind-only movement requires no action.", active_text(fenced, is_text=True))
        self.assertNotIn(
            "Behind-only movement requires no action.", prose_text(fenced, is_text=True)
        )

    def test_subordinate_authority_detector_rejects_renamed_actor(self) -> None:
        self.assertIsNotNone(SUBORDINATE_AUTHORITY.search("The claim coordinator owns review disposition."))
        self.assertIsNone(SUBORDINATE_AUTHORITY.search("A claim is a logical frame held by the root."))

    def test_result_route_edges_cover_real_bullets_and_reject_inversion(self) -> None:
        routes = (
            "## Routes\n\n"
            "- `REVIEW_CURRENT` → `$verify-live-ci`\n"
            "- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` →\n"
            "  `$address-review-comments`\n"
            "- `REVIEW_REQUIRED` → `$finish-pr` / `$review-pr`\n"
        )
        expected = {
            ("REVIEW_CURRENT", "verify-live-ci"),
            ("CHANGES_REQUIRED", "address-review-comments"),
            ("REVIEW_FINDINGS_OPEN", "address-review-comments"),
            ("REVIEW_REQUIRED", "finish-pr"),
            ("REVIEW_REQUIRED", "review-pr"),
        }
        self.assertEqual(result_route_edges(routes), expected)

        inverted = routes.replace("`REVIEW_CURRENT` → `$verify-live-ci`", "`REVIEW_CURRENT` → `$address-review-comments`")
        self.assertNotEqual(result_route_edges(inverted), expected)
        self.assertNotIn(("REVIEW_CURRENT", "verify-live-ci"), result_route_edges(inverted))

    def test_one_sided_path_removal_is_detected(self) -> None:
        both = (
            "on:\n"
            "  pull_request:\n"
            "    branches: [main]\n"
            "    paths:\n"
            "      - 'CONTRIBUTING.md'\n"
            "      - 'justfile'\n"
            "  push:\n"
            "    branches: [main]\n"
            "    paths:\n"
            "      - 'CONTRIBUTING.md'\n"
            "      - 'justfile'\n"
            "\njobs: {}\n"
        )
        self.assertEqual(
            workflow_event_paths(both, "pull_request"), ("CONTRIBUTING.md", "justfile")
        )
        self.assertEqual(workflow_event_paths(both, "push"), ("CONTRIBUTING.md", "justfile"))

        dropped_from_pr = both.replace("      - 'justfile'\n", "", 1)
        self.assertEqual(workflow_event_paths(dropped_from_pr, "pull_request"), ("CONTRIBUTING.md",))
        self.assertEqual(
            workflow_event_paths(dropped_from_pr, "push"), ("CONTRIBUTING.md", "justfile")
        )

        dropped_from_push = "".join(both.rsplit("      - 'justfile'\n", 1))
        self.assertEqual(
            workflow_event_paths(dropped_from_push, "pull_request"), ("CONTRIBUTING.md", "justfile")
        )
        self.assertEqual(workflow_event_paths(dropped_from_push, "push"), ("CONTRIBUTING.md",))

    def test_missing_event_block_yields_no_paths(self) -> None:
        pr_only = "on:\n  pull_request:\n    paths:\n      - 'CONTRIBUTING.md'\n\njobs: {}\n"
        self.assertEqual(workflow_event_paths(pr_only, "push"), ())

    def test_recipe_extraction_separates_definitions_from_calls(self) -> None:
        justfile = (
            "alias := 'x'\n\n"
            "# comment mentioning just status-update lsp\n"
            "ci-docs-check:\n    @echo hi\n\n"
            "docs-verify *args:\n    @echo hi\n\n"
            'status-update subsystem="":\n    @echo hi\n'
        )
        self.assertEqual(
            just_recipes(justfile),
            frozenset({"ci-docs-check", "docs-verify", "status-update"}),
        )
        self.assertNotIn("alias", just_recipes(justfile))
        self.assertEqual(
            referenced_just_recipes("run just ci-docs-check and just agent-* then just docs-verify"),
            frozenset({"ci-docs-check", "docs-verify"}),
        )
        self.assertEqual(
            referenced_just_recipes("cargo install just\njust devex\n"),
            frozenset({"devex"}),
        )

    def test_flow_roster_extraction_finds_the_public_flows(self) -> None:
        roster = (
            "## Select and run the route\n\n"
            f"{FLOW_ROSTER_HEADING}\n\n"
            "- `$deliver-goal` — durable multi-PR outcome;\n"
            "- `$deliver-pr` — one coherent claim;\n"
            "- `$finish-pr` — publication.\n"
        )
        self.assertEqual(declared_flows(roster), ("deliver-goal", "deliver-pr", "finish-pr"))


if __name__ == "__main__":
    unittest.main()
