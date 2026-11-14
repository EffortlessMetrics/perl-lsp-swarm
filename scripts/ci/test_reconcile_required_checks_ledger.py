#!/usr/bin/env python3
"""Falsifiers for scripts/ci/reconcile_required_checks_ledger.py (#17195).

The realistic wrong implementations are:

- treating singular ``[[check]]`` workflow-trigger rows as the merge gate;
- treating ``required = false`` inventory rows as required;
- treating an unreadable classic-protection surface as an empty surface;
- reporting MATCH when live GitHub requires a context the ledger does not.

Every test is offline. ``gh api`` is injected; no test performs a network call.
"""

from __future__ import annotations

import importlib.util
import io
import json
import sys
import tempfile
import textwrap
import tomllib
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

_HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "reconcile_required_checks_ledger",
    _HERE / "reconcile_required_checks_ledger.py",
)
assert SPEC is not None and SPEC.loader is not None
ledger = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ledger
SPEC.loader.exec_module(ledger)

ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / ".ci/policies/required-checks.toml"


def policy_text() -> str:
    return textwrap.dedent(
        """\
        version = 2
        source = "github-enforcement-union"

        [[check]]
        name = "CI Gate"
        required = true
        contract_role = "required-workflow-shape"

        [[checks]]
        name = "Perl LSP Rust Small Result"
        required = true
        policy_role = "required"
        enforcement = "github-ruleset"

        [[checks]]
        name = "ripr+ New Gap Gate"
        required = true
        policy_role = "required"
        enforcement = "github-ruleset"

        [[checks]]
        name = "Compile All Targets (bit-rot guard)"
        required = true
        policy_role = "required"
        enforcement = "github-ruleset"

        [[checks]]
        name = "Conflict marker check"
        required = true
        policy_role = "required"
        enforcement = "github-ruleset"

        [[checks]]
        name = "validate-title"
        required = true
        policy_role = "required"
        enforcement = "github-ruleset"

        [[checks]]
        name = "Public API Surface (facade PR)"
        required = false
        policy_role = "advisory"
        enforcement = "neither"
        """
    )


REQUIRED = (
    "Compile All Targets (bit-rot guard)",
    "Conflict marker check",
    "Perl LSP Rust Small Result",
    "ripr+ New Gap Gate",
    "validate-title",
)


def rules_for(names: tuple[str, ...] | list[str]) -> list[dict]:
    return [
        {
            "type": "pull_request",
            "parameters": {"required_approving_review_count": 0},
        },
        {
            "type": "required_status_checks",
            "parameters": {
                "strict_required_status_checks_policy": True,
                "required_status_checks": [
                    {"context": name, "integration_id": 15368} for name in names
                ],
            },
        },
    ]


def protection_for(names: tuple[str, ...] | list[str]) -> dict:
    return {
        "required_status_checks": {
            "strict": True,
            "contexts": list(names),
            "checks": [{"context": name, "app_id": 15368} for name in names],
        }
    }


def snapshot_for(
    names: tuple[str, ...] | list[str], *, protection: dict | None = None
) -> dict:
    return {"rules": rules_for(names), "protection": protection}


def write_json(path: Path, payload: object) -> Path:
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return path


def write_policy(path: Path, text: str | None = None) -> Path:
    path.write_text(text if text is not None else policy_text(), encoding="utf-8")
    return path


class WorkflowWiringTests(unittest.TestCase):
    def test_gate_enforcement_contract_runs_these_falsifiers(self) -> None:
        workflow = (
            ROOT / ".github/workflows/gate-enforcement-contract.yml"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "scripts/ci/test_reconcile_required_checks_ledger.py", workflow
        )


class PublicApiBaselineRoutingTests(unittest.TestCase):
    def test_public_api_baseline_routes_enforcement_to_issue_17089(self) -> None:
        # Defect 2 of #17195: REVIEW_SURFACES must not send this surface to
        # #11796 (AR03), which does not mention the public API ratchet.
        doc = tomllib.loads(
            (ROOT / "policy/review-surfaces.toml").read_text(encoding="utf-8")
        )
        row = doc["surface"]["public_api_baseline"]
        self.assertEqual(row["enforcement_successor"], "#17089")
        self.assertEqual(row["code_owner_route"]["resolution_owner"], "#17089")


class LedgerParseTests(unittest.TestCase):
    def test_required_true_plural_checks_are_the_gate_inventory(self) -> None:
        names = ledger.ledger_required_names(policy_text())
        self.assertEqual(names, frozenset(REQUIRED))

    def test_singular_check_rows_are_not_merge_authority(self) -> None:
        names = ledger.ledger_required_names(policy_text())
        self.assertNotIn("CI Gate", names)

    def test_advisory_inventory_rows_are_not_required(self) -> None:
        names = ledger.ledger_required_names(policy_text())
        self.assertNotIn("Public API Surface (facade PR)", names)

    def test_current_repo_ledger_parses_and_round_trips_a_matching_snapshot(
        self,
    ) -> None:
        names = ledger.ledger_required_names(POLICY_PATH.read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(names), 1)
        live = ledger.live_required_names(rules_for(sorted(names)), None)
        report = ledger.diff_contexts(names, live)
        self.assertEqual(report.status, "MATCH")
        self.assertEqual(report.missing, ())
        self.assertEqual(report.extra, ())


class LiveSurfaceTests(unittest.TestCase):
    def test_rules_and_empty_classic_protection_union_to_ruleset_contexts(
        self,
    ) -> None:
        live = ledger.live_required_names(rules_for(REQUIRED), None)
        self.assertEqual(live, frozenset(REQUIRED))

    def test_classic_protection_contexts_union_with_rules(self) -> None:
        live = ledger.live_required_names(
            rules_for(REQUIRED[:3]),
            protection_for(REQUIRED[3:]),
        )
        self.assertEqual(live, frozenset(REQUIRED))

    def test_github_404_protection_body_is_observed_absence_not_a_context(
        self,
    ) -> None:
        live = ledger.live_required_names(
            rules_for(("Perl LSP Rust Small Result",)),
            {
                "message": "Branch not protected",
                "documentation_url": "https://docs.github.com",
                "status": "404",
            },
        )
        self.assertEqual(live, frozenset({"Perl LSP Rust Small Result"}))

    def test_non_status_check_rules_are_ignored(self) -> None:
        live = ledger.live_required_names(
            [{"type": "deletion", "parameters": {}}],
            None,
        )
        self.assertEqual(live, frozenset())


class DiffTests(unittest.TestCase):
    def test_match_is_silent_on_both_directions(self) -> None:
        report = ledger.diff_contexts(frozenset(REQUIRED), frozenset(REQUIRED))
        self.assertEqual(report.status, "MATCH")
        self.assertEqual(report.missing, ())
        self.assertEqual(report.extra, ())

    def test_ledger_context_absent_from_live_is_missing(self) -> None:
        live = frozenset(REQUIRED[:-1])
        report = ledger.diff_contexts(frozenset(REQUIRED), live)
        self.assertEqual(report.status, "DRIFT")
        self.assertEqual(report.missing, ("validate-title",))
        self.assertEqual(report.extra, ())

    def test_live_context_absent_from_required_ledger_is_extra(self) -> None:
        live = frozenset(REQUIRED + ("Public API Surface (facade PR)",))
        report = ledger.diff_contexts(frozenset(REQUIRED), live)
        self.assertEqual(report.status, "DRIFT")
        self.assertEqual(report.missing, ())
        self.assertEqual(report.extra, ("Public API Surface (facade PR)",))


class CliSnapshotTests(unittest.TestCase):
    def run_main(self, argv: list[str]) -> tuple[int, str, str]:
        stdout = io.StringIO()
        stderr = io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            code = ledger.main(argv)
        return code, stdout.getvalue(), stderr.getvalue()

    def test_snapshot_match_exits_zero_and_names_the_required_set(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root / "required-checks.toml")
            snapshot = write_json(
                root / "snapshot.json", snapshot_for(REQUIRED, protection=None)
            )
            code, out, err = self.run_main(
                ["--ledger", str(policy), "--snapshot", str(snapshot)]
            )
        self.assertEqual(code, 0, err)
        self.assertIn("MATCH", out)
        self.assertIn("Perl LSP Rust Small Result", out)
        self.assertEqual(err, "")

    def test_snapshot_missing_context_exits_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root / "required-checks.toml")
            snapshot = write_json(
                root / "snapshot.json",
                snapshot_for(REQUIRED[:-1], protection=None),
            )
            code, out, err = self.run_main(
                ["--ledger", str(policy), "--snapshot", str(snapshot)]
            )
        self.assertEqual(code, 1, err)
        self.assertIn("DRIFT", out)
        self.assertIn("missing from live: validate-title", out)

    def test_snapshot_extra_context_exits_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root / "required-checks.toml")
            snapshot = write_json(
                root / "snapshot.json",
                snapshot_for(
                    REQUIRED + ("unexpected-required",),
                    protection=None,
                ),
            )
            code, out, _ = self.run_main(
                ["--ledger", str(policy), "--snapshot", str(snapshot)]
            )
        self.assertEqual(code, 1)
        self.assertIn("extra on live: unexpected-required", out)

    def test_separate_rules_and_protection_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root / "required-checks.toml")
            rules = write_json(root / "rules.json", rules_for(REQUIRED[:3]))
            protection = write_json(
                root / "protection.json",
                protection_for(REQUIRED[3:]),
            )
            code, out, err = self.run_main(
                [
                    "--ledger",
                    str(policy),
                    "--rules",
                    str(rules),
                    "--protection",
                    str(protection),
                ]
            )
        self.assertEqual(code, 0, err)
        self.assertIn("MATCH", out)

    def test_malformed_snapshot_is_not_proven(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root / "required-checks.toml")
            snapshot = write_json(root / "snapshot.json", ["not", "an", "object"])
            code, out, err = self.run_main(
                ["--ledger", str(policy), "--snapshot", str(snapshot)]
            )
        self.assertEqual(code, 2)
        self.assertIn("NOT_PROVEN", out)
        self.assertIn("snapshot must be an object", err)


class CliFetchTests(unittest.TestCase):
    def run_main(
        self, argv: list[str], runner: ledger.GhRunner
    ) -> tuple[int, str, str]:
        stdout = io.StringIO()
        stderr = io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            code = ledger.main(argv, gh_runner=runner)
        return code, stdout.getvalue(), stderr.getvalue()

    def test_fetch_uses_the_two_documented_gh_api_paths(self) -> None:
        calls: list[list[str]] = []

        def runner(args: list[str]) -> ledger.GhResult:
            calls.append(list(args))
            path = args[-1]
            if path.endswith("/rules/branches/main"):
                return ledger.GhResult(0, json.dumps(rules_for(REQUIRED)), "")
            if path.endswith("/branches/main/protection"):
                return ledger.GhResult(
                    1,
                    json.dumps(
                        {
                            "message": "Branch not protected",
                            "status": "404",
                        }
                    ),
                    "gh: HTTP 404",
                )
            return ledger.GhResult(1, "", f"unexpected path {path}")

        with tempfile.TemporaryDirectory() as tmp:
            policy = write_policy(Path(tmp) / "required-checks.toml")
            code, out, err = self.run_main(
                [
                    "--ledger",
                    str(policy),
                    "--fetch",
                    "--repository",
                    "EffortlessMetrics/perl-lsp-swarm",
                    "--branch",
                    "main",
                ],
                runner,
            )
        self.assertEqual(code, 0, err)
        self.assertIn("MATCH", out)
        self.assertEqual(
            [call[-1] for call in calls],
            [
                "repos/EffortlessMetrics/perl-lsp-swarm/rules/branches/main",
                "repos/EffortlessMetrics/perl-lsp-swarm/branches/main/protection",
            ],
        )

    def test_fetch_forbidden_protection_is_not_an_empty_surface(self) -> None:
        def runner(args: list[str]) -> ledger.GhResult:
            path = args[-1]
            if "rules/branches" in path:
                return ledger.GhResult(0, json.dumps(rules_for(REQUIRED)), "")
            return ledger.GhResult(
                1,
                json.dumps({"message": "Must have admin rights", "status": "403"}),
                "gh: HTTP 403",
            )

        with tempfile.TemporaryDirectory() as tmp:
            policy = write_policy(Path(tmp) / "required-checks.toml")
            code, out, err = self.run_main(
                [
                    "--ledger",
                    str(policy),
                    "--fetch",
                    "--repository",
                    "EffortlessMetrics/perl-lsp-swarm",
                ],
                runner,
            )
        self.assertEqual(code, 2)
        self.assertIn("NOT_PROVEN", out)
        self.assertIn("classic branch protection unreadable", err)
        self.assertNotIn("MATCH", out)

    def test_fetch_encodes_branch_names_as_one_path_segment(self) -> None:
        seen: list[str] = []

        def runner(args: list[str]) -> ledger.GhResult:
            seen.append(args[-1])
            if "rules/branches" in args[-1]:
                return ledger.GhResult(0, json.dumps(rules_for(REQUIRED)), "")
            return ledger.GhResult(
                1,
                json.dumps({"message": "Branch not protected", "status": "404"}),
                "gh: HTTP 404",
            )

        with tempfile.TemporaryDirectory() as tmp:
            policy = write_policy(Path(tmp) / "required-checks.toml")
            code, _, err = self.run_main(
                [
                    "--ledger",
                    str(policy),
                    "--fetch",
                    "--repository",
                    "EffortlessMetrics/perl-lsp-swarm",
                    "--branch",
                    "release/#123",
                ],
                runner,
            )
        self.assertEqual(code, 0, err)
        self.assertTrue(
            seen[0].endswith("/rules/branches/release%2F%23123"),
            seen,
        )
        self.assertTrue(
            seen[1].endswith("/branches/release%2F%23123/protection"),
            seen,
        )

    def test_snapshot_mode_never_invokes_gh(self) -> None:
        def runner(args: list[str]) -> ledger.GhResult:
            raise AssertionError(f"gh must not run in snapshot mode: {args}")

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root / "required-checks.toml")
            snapshot = write_json(
                root / "snapshot.json", snapshot_for(REQUIRED, protection=None)
            )
            stdout = io.StringIO()
            stderr = io.StringIO()
            with redirect_stdout(stdout), redirect_stderr(stderr):
                code = ledger.main(
                    ["--ledger", str(policy), "--snapshot", str(snapshot)],
                    gh_runner=runner,
                )
        self.assertEqual(code, 0, stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
