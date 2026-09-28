#!/usr/bin/env python3
"""Focused tests for scripts/ci/pr_plan.py."""

from __future__ import annotations

import importlib.util
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("pr_plan.py")
SPEC = importlib.util.spec_from_file_location("pr_plan", SCRIPT_PATH)
assert SPEC is not None
pr_plan = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(pr_plan)


class PrPlanTests(unittest.TestCase):
    def test_path_matches_glob_handles_recursive_and_single_segment_patterns(self) -> None:
        self.assertTrue(pr_plan.path_matches_glob("docs/foo/bar.md", "docs/**"))
        self.assertTrue(
            pr_plan.path_matches_glob(
                "crates/perl-lsp-rs-core/src/providers/foo.rs",
                "crates/perl-lsp-rs-core/src/providers/**",
            )
        )
        self.assertTrue(pr_plan.path_matches_glob("docs/foo.md", "docs/*.md"))
        self.assertFalse(pr_plan.path_matches_glob("docs/nested/foo.md", "docs/*.md"))
        self.assertTrue(pr_plan.path_matches_glob("README.md", "README*"))

    def test_docs_only_requires_every_changed_file_to_be_docs(self) -> None:
        self.assertTrue(pr_plan.docs_only(["docs/a.md", "README.md"]))
        self.assertFalse(pr_plan.docs_only([]))
        self.assertFalse(pr_plan.docs_only(["docs/a.md", "crates/foo/src/lib.rs"]))

    def test_classify_areas_matches_path_and_keyword_risk_packs(self) -> None:
        risk_packs = {
            "parser": {"paths": ["crates/perl-parser/**"], "keywords": []},
            "coverage": {"paths": [], "keywords": ["coverage"]},
            "unmatched": {"paths": ["vscode-extension/**"], "keywords": ["release"]},
        }

        selected, areas = pr_plan.classify_areas(
            ["crates/perl-parser/src/lib.rs", "docs/coverage-plan.md"],
            risk_packs,
        )

        self.assertEqual(["parser", "coverage"], selected)
        self.assertEqual(["coverage", "parser"], areas)

    def test_select_lanes_keeps_docs_only_changes_on_docs_gate(self) -> None:
        lanes = {
            "docs_gate": {"base_lem": 2, "blocking": True},
            "rust_small": {"default_pr": True, "base_lem": 10, "blocking": True},
            "ripr_advisory": {
                "default_pr": True,
                "base_lem": 5,
                "paths": ["crates/**/*.rs"],
            },
        }

        selected, skipped = pr_plan.select_lanes(
            files=["docs/status.md"],
            labels=[],
            risk_pack_ids=[],
            risk_packs={},
            lanes=lanes,
        )

        self.assertEqual(["docs_gate"], [lane["id"] for lane in selected])
        self.assertEqual([], skipped)

    def test_full_ci_does_not_select_schedule_manual_coverage(self) -> None:
        lanes = {
            "coverage_alias": {
                "base_lem": 45,
                "blocking": False,
                "workflow": ".github/workflows/ci-nightly.yml",
                "job": "test-coverage",
                "labels": ["coverage-alias"],
            },
            "mutation": {"base_lem": 60, "blocking": False},
        }
        risk_packs = {
            "parser": {
                "lanes": ["coverage_alias"],
                "deep_lanes": ["coverage_alias", "mutation"],
            },
        }

        selected, skipped = pr_plan.select_lanes(
            files=["crates/perl-parser/src/parser.rs"],
            labels=["full-ci"],
            risk_pack_ids=["parser"],
            risk_packs=risk_packs,
            lanes=lanes,
        )

        self.assertEqual(["mutation"], [lane["id"] for lane in selected])
        self.assertEqual([], skipped)

    def test_main_loads_canonical_policy_without_routing_coverage_on_full_ci(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output = root / "ci-plan.json"
            policy_root = Path(__file__).resolve().parents[2] / "policy"

            old_argv = sys.argv
            old_discover = pr_plan.discover_changed_files
            try:
                pr_plan.discover_changed_files = lambda _base, _head: {
                    "status": "known",
                    "files": ["crates/perl-parser/src/parser.rs"],
                    "digest": "test-digest-nonempty",
                }
                sys.argv = [
                    "pr_plan.py",
                    "--base",
                    "origin/main",
                    "--head",
                    "HEAD",
                    "--labels-json",
                    '["full-ci"]',
                    "--budget",
                    str(policy_root / "ci-budget.toml"),
                    "--lanes",
                    str(policy_root / "ci-lanes.toml"),
                    "--risk-packs",
                    str(policy_root / "ci-risk-packs.toml"),
                    "--trust-lanes",
                    str(policy_root / "trust-lanes.toml"),
                    "--history",
                    str(root / "missing-history.json"),
                    "--json-out",
                    str(output),
                ]
                stdout = io.StringIO()
                with redirect_stdout(stdout):
                    status = pr_plan.main()
            finally:
                sys.argv = old_argv
                pr_plan.discover_changed_files = old_discover

            plan = json.loads(output.read_text(encoding="utf-8"))

        self.assertEqual(0, status)
        self.assertEqual(["parser"], plan["selection"]["risk_packs"])
        self.assertEqual(
            ["crates/perl-parser/src/parser.rs"], plan["changed"]["files"]
        )
        selected_ids = [lane["id"] for lane in plan["selection"]["lanes"]]
        skipped_ids = [lane["id"] for lane in plan["selection"]["skipped_lanes"]]
        self.assertIn("pr_smoke", selected_ids)
        self.assertIn("merge_gate_shards", selected_ids)
        self.assertNotIn("coverage", selected_ids)
        self.assertNotIn("coverage", skipped_ids)

    def test_select_lanes_reports_path_filtered_default_lane_when_it_does_not_match(self) -> None:
        lanes = {
            "rust_small": {"default_pr": True, "base_lem": 10, "blocking": True},
            "ripr_advisory": {
                "default_pr": True,
                "base_lem": 5,
                "paths": ["crates/**/*.rs"],
            },
        }

        selected, skipped = pr_plan.select_lanes(
            files=["scripts/ci/pr_plan.py"],
            labels=[],
            risk_pack_ids=[],
            risk_packs={},
            lanes=lanes,
        )

        self.assertEqual(["rust_small"], [lane["id"] for lane in selected])
        self.assertEqual(["ripr_advisory"], [lane["id"] for lane in skipped])
        self.assertEqual("paths-filter-no-match", skipped[0]["skipped_reason"])

    def test_apply_learned_estimates_uses_recent_p50_or_static_floor(self) -> None:
        lanes = [
            {"id": "rust_small", "base_lem": 10},
            {"id": "slow_lane", "base_lem": 100},
            {"id": "not_learned", "base_lem": 5},
        ]
        history = {
            "lanes": {
                "rust_small": {"learned": True, "p50": 20, "static_floor": 15},
                "slow_lane": {"learned": True, "p50": 20, "static_floor": 80},
                "not_learned": {"learned": False, "p50": 50, "static_floor": 5},
            }
        }

        delta, learned_count = pr_plan.apply_learned_estimates(lanes, history)

        self.assertEqual(-7.0, delta)
        self.assertEqual(2, learned_count)
        self.assertEqual(23.0, lanes[0]["base_lem"])
        self.assertEqual("learned (p50 * 1.15)", lanes[0]["learned_source"])
        self.assertEqual(80.0, lanes[1]["base_lem"])
        self.assertEqual("static_floor", lanes[1]["learned_source"])
        self.assertEqual(5, lanes[2]["base_lem"])

    def _write_history(self, root: Path, text: str, name: str = "ci-lane-history.json") -> Path:
        path = root / name
        path.write_text(text, encoding="utf-8")
        return path

    def test_load_learned_history_accepts_a_v1_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(
                Path(tmp),
                json.dumps(
                    {
                        "schema_version": 1,
                        "lanes": {
                            "rust_small": {"learned": True, "p50": 20.0},
                        },
                    }
                ),
            )
            payload, disposition = pr_plan.load_learned_history(path)

        self.assertEqual("accepted", disposition)
        self.assertEqual(1, payload["schema_version"])
        self.assertIn("rust_small", payload["lanes"])

    def test_load_learned_history_rejects_a_future_schema_even_when_lanes_survive(
        self,
    ) -> None:
        """The control that matters: the check is on the envelope version, not
        on whether a `lanes` key happens to still be present. A v2 producer that
        reshapes the per-record shape must not be consumed as v1."""
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(
                Path(tmp),
                json.dumps(
                    {
                        "schema_version": 2,
                        "lanes": {
                            "rust_small": {
                                "learned": True,
                                "p50": 1.0,
                                "static_floor": 999.0,
                            }
                        },
                    }
                ),
            )
            lanes = [{"id": "rust_small", "base_lem": 10}]
            payload, disposition = pr_plan.load_learned_history(path)
            delta, learned_count = pr_plan.apply_learned_estimates(lanes, payload)

        self.assertEqual("unsupported_schema", disposition)
        self.assertEqual({}, payload)
        self.assertEqual(0, learned_count)
        self.assertEqual(0.0, delta)
        self.assertEqual(10, lanes[0]["base_lem"], "static floor must survive")

    def test_load_learned_history_rejects_non_integer_schema_versions(self) -> None:
        """Python's bool is an int subclass and 1.0 == 1, so a JSON `true` or
        `1.0` envelope version would slip past bare equality and let its lane
        numbers rewrite the budget. Only an exact integer satisfies v1."""
        for forged_version in (True, 1.0):
            with self.subTest(forged_version=forged_version):
                with tempfile.TemporaryDirectory() as tmp:
                    path = self._write_history(
                        Path(tmp),
                        json.dumps(
                            {
                                "schema_version": forged_version,
                                "lanes": {
                                    "rust_small": {
                                        "learned": True,
                                        "p50": 868.0,
                                        "static_floor": 999.0,
                                    }
                                },
                            }
                        ),
                    )
                    lanes = [{"id": "rust_small", "base_lem": 10}]
                    payload, disposition = pr_plan.load_learned_history(path)
                    delta, learned_count = pr_plan.apply_learned_estimates(
                        lanes, payload
                    )

                self.assertEqual("unsupported_schema", disposition)
                self.assertEqual({}, payload)
                self.assertEqual(0, learned_count)
                self.assertEqual(0.0, delta)
                self.assertEqual(10, lanes[0]["base_lem"], "static floor must survive")

    def test_load_learned_history_rejects_a_renamed_lane_container(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(
                Path(tmp),
                json.dumps(
                    {
                        "schema_version": 1,
                        "lane_stats": {"rust_small": {"learned": True, "p50": 20.0}},
                    }
                ),
            )
            payload, disposition = pr_plan.load_learned_history(path)

        self.assertEqual("malformed", disposition)
        self.assertEqual({}, payload)

    def test_load_learned_history_rejects_a_payload_with_no_envelope(self) -> None:
        """An unenveloped payload is not v1. Accepting it would re-open exactly
        the drift this check exists to catch."""
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(
                Path(tmp),
                json.dumps({"lanes": {"rust_small": {"learned": True, "p50": 20.0}}}),
            )
            payload, disposition = pr_plan.load_learned_history(path)

        self.assertEqual("unsupported_schema", disposition)
        self.assertEqual({}, payload)

    def test_load_learned_history_rejects_non_object_payloads(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write_history(Path(tmp), json.dumps([{"lanes": {}}]))
            payload, disposition = pr_plan.load_learned_history(path)

        self.assertEqual("malformed", disposition)
        self.assertEqual({}, payload)

    def _run_plan_with_history(self, root: Path, history_text: str | None) -> dict:
        """Drive main() end to end against one history payload.

        Returns the plan receipt and the rendered summary so both the receipt
        disposition and the human-visible warning can be asserted together.
        """
        budget = root / "ci-budget.toml"
        budget.write_text(
            """
[budget]
default_limit_lem = 35
elevated_limit_lem = 75
hard_limit_lem = 125
linux_minute_rate_usd = 0.008
""",
            encoding="utf-8",
        )
        lanes = root / "ci-lanes.toml"
        lanes.write_text(
            """
[lane.rust_small]
default_pr = true
base_lem = 10
blocking = true
""",
            encoding="utf-8",
        )
        risk_packs = root / "ci-risk-packs.toml"
        risk_packs.write_text("", encoding="utf-8")
        trust_lanes = root / "trust-lanes.toml"
        trust_lanes.write_text("", encoding="utf-8")
        output = root / "ci-plan.json"
        summary = root / "summary.md"

        argv = [
            "pr_plan.py",
            "--base", "origin/main",
            "--head", "HEAD",
            "--labels-json", "[]",
            "--budget", str(budget),
            "--lanes", str(lanes),
            "--risk-packs", str(risk_packs),
            "--trust-lanes", str(trust_lanes),
            "--json-out", str(output),
            "--summary", str(summary),
        ]
        if history_text is None:
            # Name the absent path explicitly. Omitting --history falls back to
            # main()'s default, `.ci/metrics/ci-lane-history.json`, which is
            # relative to the *repository root*: the real file is checked in, so
            # a cwd-dependent read resolves to `accepted` on the CI runner and
            # to `absent` when the suite is run from scripts/ci.
            argv += ["--history", str(root / "no-such-history.json")]
        else:
            history = self._write_history(root, history_text)
            argv += ["--history", str(history)]

        old_argv = sys.argv
        old_discover = pr_plan.discover_changed_files
        try:
            pr_plan.discover_changed_files = lambda _base, _head: {
                "status": "known",
                "files": ["scripts/ci/pr_plan.py"],
                "digest": "test-digest-history",
            }
            sys.argv = argv
            with redirect_stdout(io.StringIO()):
                self.assertEqual(0, pr_plan.main())
        finally:
            sys.argv = old_argv
            pr_plan.discover_changed_files = old_discover

        return {
            "plan": json.loads(output.read_text(encoding="utf-8")),
            "summary": summary.read_text(encoding="utf-8"),
        }

    def test_accepted_v1_history_applies_estimates_without_a_warning(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            result = self._run_plan_with_history(
                Path(tmp),
                json.dumps(
                    {
                        "schema_version": 1,
                        "lanes": {
                            "rust_small": {
                                "learned": True,
                                "p50": 20.0,
                                "static_floor": 2.0,
                            }
                        },
                    }
                ),
            )

        learned = result["plan"]["learned"]
        self.assertEqual("accepted", learned["history_disposition"])
        self.assertTrue(learned["history_present"])
        self.assertEqual(1, learned["lanes_using_learned"])
        self.assertEqual(23.0, result["plan"]["budget"]["estimated_lem"])
        self.assertNotIn("Lane history", result["summary"])

    def test_rejected_history_is_named_in_the_summary_and_falls_back(self) -> None:
        """The control: the same lane, the same learned record, and only the
        envelope version differs. A rejection must be visible in the summary a
        reviewer reads, and must not move the estimate."""
        with tempfile.TemporaryDirectory() as tmp:
            result = self._run_plan_with_history(
                Path(tmp),
                json.dumps(
                    {
                        "schema_version": 2,
                        "lanes": {
                            "rust_small": {
                                "learned": True,
                                "p50": 20.0,
                                "static_floor": 999.0,
                            }
                        },
                    }
                ),
            )

        learned = result["plan"]["learned"]
        self.assertEqual("unsupported_schema", learned["history_disposition"])
        self.assertTrue(learned["history_present"], "the file does exist")
        self.assertEqual(0, learned["lanes_using_learned"])
        self.assertEqual(10.0, result["plan"]["budget"]["estimated_lem"], "static floor")
        self.assertIn("Lane history", result["summary"])
        self.assertIn("unsupported_schema", result["summary"])

    def test_absent_history_is_not_warned_about(self) -> None:
        """No file is a normal state, not a defect: the warning must not fire."""
        with tempfile.TemporaryDirectory() as tmp:
            result = self._run_plan_with_history(Path(tmp), None)

        self.assertEqual("absent", result["plan"]["learned"]["history_disposition"])
        self.assertFalse(result["plan"]["learned"]["history_present"])
        self.assertNotIn("Lane history", result["summary"])

    def test_load_learned_history_separates_corrupt_from_absent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            corrupt = self._write_history(root, "{not json")

            payload, disposition = pr_plan.load_learned_history(corrupt)
            self.assertEqual("unreadable", disposition)
            self.assertEqual({}, payload)

            missing_payload, missing_disposition = pr_plan.load_learned_history(
                root / "does-not-exist.json"
            )
            self.assertEqual("absent", missing_disposition)
            self.assertEqual({}, missing_payload)

    def test_every_disposition_the_loader_can_return_is_declared(self) -> None:
        """The receipt advertises a closed token vocabulary. Drive the loader
        through every reachable outcome and hold each one to that vocabulary,
        so a future branch cannot emit an undeclared disposition."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            cases: list[tuple[str, Path]] = [
                ("absent", root / "never-written.json"),
                ("unreadable", self._write_history(root, "{not json", "corrupt.json")),
                ("malformed", self._write_history(root, "[]", "list.json")),
                (
                    "unsupported_schema",
                    self._write_history(
                        root,
                        json.dumps({"schema_version": 7, "lanes": {}}),
                        "v7.json",
                    ),
                ),
                (
                    "accepted",
                    self._write_history(
                        root,
                        json.dumps({"schema_version": 1, "lanes": {}}),
                        "v1.json",
                    ),
                ),
            ]
            declared = set(pr_plan.HISTORY_DISPOSITIONS)
            for expected, path in cases:
                _, disposition = pr_plan.load_learned_history(path)
                self.assertEqual(expected, disposition, f"{path.name}")
                self.assertIn(disposition, declared, f"{path.name} emitted undeclared")

    def test_main_writes_plan_summary_and_trust_lane_for_pr_plan_helper(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            budget = root / "ci-budget.toml"
            budget.write_text(
                """
[budget]
default_limit_lem = 35
elevated_limit_lem = 75
hard_limit_lem = 125
linux_minute_rate_usd = 0.008
""",
                encoding="utf-8",
            )
            lanes = root / "ci-lanes.toml"
            lanes.write_text(
                """
[lane.docs_gate]
base_lem = 2
blocking = true

[lane.rust_small]
default_pr = true
base_lem = 10
blocking = true

[lane.ripr_advisory]
default_pr = true
base_lem = 5
paths = ["crates/**/*.rs"]
""",
                encoding="utf-8",
            )
            risk_packs = root / "ci-risk-packs.toml"
            risk_packs.write_text("", encoding="utf-8")
            trust_lanes = root / "trust-lanes.toml"
            trust_lanes.write_text(
                """
schema_version = 1
policy = "trust-lanes"
status = "advisory"

[class.docs_status_only]
risk_rank = 1
claim_boundary = "Docs, status, policy, and CI planning only."
required_checks = ["docs"]
""",
                encoding="utf-8",
            )
            output = root / "ci-plan.json"
            summary = root / "summary.md"

            old_argv = sys.argv
            old_discover = pr_plan.discover_changed_files
            try:
                pr_plan.discover_changed_files = lambda _base, _head: {
                    "status": "known",
                    "files": ["scripts/ci/pr_plan.py"],
                    "digest": "test-digest-pr-plan-helper",
                }
                sys.argv = [
                    "pr_plan.py",
                    "--base",
                    "origin/main",
                    "--head",
                    "HEAD",
                    "--labels-json",
                    "[]",
                    "--budget",
                    str(budget),
                    "--lanes",
                    str(lanes),
                    "--risk-packs",
                    str(risk_packs),
                    "--trust-lanes",
                    str(trust_lanes),
                    "--json-out",
                    str(output),
                    "--summary",
                    str(summary),
                ]
                stdout = io.StringIO()
                with redirect_stdout(stdout):
                    status = pr_plan.main()
            finally:
                sys.argv = old_argv
                pr_plan.discover_changed_files = old_discover

            plan = json.loads(output.read_text(encoding="utf-8"))
            printed = json.loads(stdout.getvalue())
            summary_text = summary.read_text(encoding="utf-8")

        self.assertEqual(0, status)
        self.assertEqual(["scripts/ci/pr_plan.py"], plan["changed"]["files"])
        self.assertEqual("docs_status_only", plan["trust_lanes"]["strongest_class"]["id"])
        self.assertEqual(["rust_small"], [lane["id"] for lane in plan["selection"]["lanes"]])
        self.assertEqual(["ripr_advisory"], [lane["id"] for lane in plan["selection"]["skipped_lanes"]])
        self.assertEqual({"estimated_lem": 10.0, "band": "default", "lanes": 1}, printed)
        self.assertIn("## Trust lane (advisory)", summary_text)
        self.assertIn("`docs_status_only`", summary_text)
        self.assertIn("`ripr_advisory` | paths-filter-no-match", summary_text)

    def test_discover_changed_files_keeps_failure_and_empty_opposite(self) -> None:
        """A failed diff and a genuine empty diff are opposite facts (#15347)."""

        class Failure:
            returncode = 128
            stdout = ""
            stderr = "fatal: bad revision 'origin/main...HEAD'"

        class OkFiles:
            returncode = 0
            stdout = "b.rs\na.rs\n"
            stderr = ""

        class OkEmpty:
            returncode = 0
            stdout = ""
            stderr = ""

        commands: list[list[str]] = []
        outcomes = iter([Failure(), OkFiles(), OkEmpty()])

        def fake_run(command, **_kwargs):
            commands.append(command)
            return next(outcomes)

        old_run = pr_plan.subprocess.run
        try:
            pr_plan.subprocess.run = fake_run
            failed = pr_plan.discover_changed_files("origin/main", "HEAD")
            known = pr_plan.discover_changed_files("origin/main", "HEAD")
            empty = pr_plan.discover_changed_files("origin/main", "HEAD")
        finally:
            pr_plan.subprocess.run = old_run

        self.assertEqual("unavailable", failed["status"])
        self.assertEqual("git-diff-exit-128", failed["code"])
        self.assertIn("bad revision", failed["detail"])
        self.assertEqual(
            ["git", "diff", "--name-only", "origin/main...HEAD"], failed["command"]
        )
        self.assertNotIn("files", failed)

        self.assertEqual("known", known["status"])
        self.assertEqual(["b.rs", "a.rs"], known["files"])
        self.assertEqual(64, len(known["digest"]))

        self.assertEqual("known_empty", empty["status"])
        self.assertEqual([], empty["files"])
        self.assertEqual(64, len(empty["digest"]))
        self.assertEqual(3, len(commands))
        reproduce = ["git", "diff", "--name-only", "origin/main...HEAD"]
        self.assertTrue(all(command == reproduce for command in commands))

    def test_discover_changed_files_reports_unspawnable_git_as_unavailable(self) -> None:
        def raising_run(_command, **_kwargs):
            raise OSError("git: executable not found")

        old_run = pr_plan.subprocess.run
        try:
            pr_plan.subprocess.run = raising_run
            changeset = pr_plan.discover_changed_files("origin/main", "HEAD")
        finally:
            pr_plan.subprocess.run = old_run

        self.assertEqual("unavailable", changeset["status"])
        self.assertEqual("git-unspawnable", changeset["code"])
        self.assertIn("not found", changeset["detail"])
        self.assertEqual(["git", "diff", "--name-only", "origin/main...HEAD"], changeset["command"])

    def test_discover_changed_files_timeout_refuses_without_planning_empty(self) -> None:
        command = ["git", "diff", "--name-only", "origin/main...HEAD"]

        def timed_out(actual_command, **kwargs):
            self.assertEqual(command, actual_command)
            self.assertEqual(pr_plan.GIT_DIFF_TIMEOUT_SECONDS, kwargs["timeout"])
            self.assertTrue(kwargs["capture_output"])
            raise pr_plan.subprocess.TimeoutExpired(actual_command, kwargs["timeout"])

        old_run = pr_plan.subprocess.run
        try:
            pr_plan.subprocess.run = timed_out
            changeset = pr_plan.discover_changed_files("origin/main", "HEAD")
        finally:
            pr_plan.subprocess.run = old_run

        self.assertEqual("unavailable", changeset["status"])
        self.assertEqual("git-diff-timeout", changeset["code"])
        self.assertIn("30 seconds", changeset["detail"])
        self.assertEqual(command, changeset["command"])
        self.assertNotIn("files", changeset)

    def test_discovery_annotation_escapes_git_stderr_but_keeps_receipt_detail(self) -> None:
        detail = "fatal: bad revision 100%\rmore detail\n::warning::injected"
        changeset = {
            "status": "unavailable",
            "code": "git-diff-exit-128",
            "detail": detail,
            "command": ["git", "diff", "--name-only", "origin/main...HEAD"],
        }
        plan = pr_plan.not_proven_plan(
            base="origin/main", head="HEAD", labels=[], changeset=changeset
        )

        self.assertEqual(detail, plan["refusal"]["detail"])
        self.assertEqual(detail, plan["changed_set"]["detail"])
        self.assertIn(detail, pr_plan.render_not_proven_summary(plan))
        annotation = plan["warnings"][0]
        self.assertEqual(1, len(annotation.splitlines()))
        self.assertIn("100%25%0Dmore detail%0A::warning::injected", annotation)
        self.assertTrue(annotation.startswith("::error::"))

    def test_main_writes_not_proven_receipt_and_fails_when_discovery_fails(self) -> None:
        """Negative control: a Rust change behind a failed diff cannot route
        as a zero-impact plan; the receipt must be refuseable without prose."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output = root / "ci-plan.json"
            summary = root / "summary.md"
            policy_root = Path(__file__).resolve().parents[2] / "policy"

            old_argv = sys.argv
            old_discover = pr_plan.discover_changed_files
            try:
                pr_plan.discover_changed_files = lambda _base, _head: {
                    "status": "unavailable",
                    "code": "git-diff-exit-128",
                    "detail": "fatal: bad revision 'origin/main...HEAD'",
                    "command": [
                        "git",
                        "diff",
                        "--name-only",
                        "origin/main...HEAD",
                    ],
                }
                sys.argv = [
                    "pr_plan.py",
                    "--base",
                    "origin/main",
                    "--head",
                    "HEAD",
                    "--labels-json",
                    "[]",
                    "--budget",
                    str(policy_root / "ci-budget.toml"),
                    "--lanes",
                    str(policy_root / "ci-lanes.toml"),
                    "--risk-packs",
                    str(policy_root / "ci-risk-packs.toml"),
                    "--trust-lanes",
                    str(policy_root / "trust-lanes.toml"),
                    "--history",
                    str(root / "missing-history.json"),
                    "--json-out",
                    str(output),
                    "--summary",
                    str(summary),
                ]
                stdout = io.StringIO()
                stderr = io.StringIO()
                with redirect_stdout(stdout), redirect_stderr(stderr):
                    status = pr_plan.main()
            finally:
                sys.argv = old_argv
                pr_plan.discover_changed_files = old_discover

            plan = json.loads(output.read_text(encoding="utf-8"))
            summary_text = summary.read_text(encoding="utf-8")

        self.assertEqual(3, status)
        self.assertEqual("NOT_PROVEN", plan["posture"])
        self.assertNotIn("changed", plan)
        self.assertEqual("unavailable", plan["changed_set"]["status"])
        self.assertEqual(
            "changed_file_discovery_unavailable", plan["refusal"]["reason"]
        )
        self.assertEqual("git-diff-exit-128", plan["refusal"]["code"])
        self.assertIn("bad revision", plan["refusal"]["detail"])
        self.assertEqual(
            "git diff --name-only origin/main...HEAD", plan["refusal"]["reproduce"]
        )
        self.assertTrue(plan["selection"]["refused"])
        self.assertEqual([], plan["selection"]["lanes"])
        self.assertEqual([], plan["selection"]["risk_packs"])
        self.assertEqual([], plan["selection"]["skipped_lanes"])
        self.assertTrue(plan["guard"]["failed"])
        self.assertIn("NOT_PROVEN", stdout.getvalue())
        # The ::error:: annotation is printed to the workflow log, not only
        # stored in the receipt (#15347).
        self.assertIn("::error::Changed-file discovery failed", stderr.getvalue())
        self.assertIn("git-diff-exit-128", summary_text)
        self.assertIn("NOT_PROVEN", summary_text)

    def test_main_keeps_genuinely_empty_diff_a_valid_plan(self) -> None:
        """An exit-0 empty diff is a valid zero-change plan, not a failure."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            budget = root / "ci-budget.toml"
            budget.write_text(
                """
[budget]
default_limit_lem = 35
elevated_limit_lem = 75
hard_limit_lem = 125
linux_minute_rate_usd = 0.008
""",
                encoding="utf-8",
            )
            lanes = root / "ci-lanes.toml"
            lanes.write_text(
                """
[lane.rust_small]
default_pr = true
base_lem = 10
blocking = true
""",
                encoding="utf-8",
            )
            risk_packs = root / "ci-risk-packs.toml"
            risk_packs.write_text("", encoding="utf-8")
            trust_lanes = root / "trust-lanes.toml"
            trust_lanes.write_text(
                """
schema_version = 1
policy = "trust-lanes"
status = "advisory"
""",
                encoding="utf-8",
            )
            output = root / "ci-plan.json"

            old_argv = sys.argv
            old_discover = pr_plan.discover_changed_files
            try:
                pr_plan.discover_changed_files = lambda _base, _head: {
                    "status": "known_empty",
                    "files": [],
                    "digest": "0" * 64,
                }
                sys.argv = [
                    "pr_plan.py",
                    "--base",
                    "origin/main",
                    "--head",
                    "HEAD",
                    "--labels-json",
                    "[]",
                    "--budget",
                    str(budget),
                    "--lanes",
                    str(lanes),
                    "--risk-packs",
                    str(risk_packs),
                    "--trust-lanes",
                    str(trust_lanes),
                    "--json-out",
                    str(output),
                ]
                stdout = io.StringIO()
                with redirect_stdout(stdout):
                    status = pr_plan.main()
            finally:
                sys.argv = old_argv
                pr_plan.discover_changed_files = old_discover

            plan = json.loads(output.read_text(encoding="utf-8"))
            printed = json.loads(stdout.getvalue())

        self.assertEqual(0, status)
        self.assertEqual("rust", plan["posture"])
        self.assertEqual("known_empty", plan["changed_set"]["status"])
        self.assertEqual([], plan["changed"]["files"])
        self.assertEqual([], plan["selection"]["risk_packs"])
        self.assertFalse(plan["guard"]["failed"])
        self.assertEqual(
            {"estimated_lem": 10.0, "band": "default", "lanes": 1}, printed
        )


if __name__ == "__main__":
    unittest.main()
