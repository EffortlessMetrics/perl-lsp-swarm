#!/usr/bin/env python3
"""Falsifiers for run_gate_shard.py."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("run_gate_shard.py")
SPEC = importlib.util.spec_from_file_location("run_gate_shard", SCRIPT)
assert SPEC and SPEC.loader
shard = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = shard
SPEC.loader.exec_module(shard)
SUBJECT = "a" * 40
REPO_ROOT = Path(
    os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2])
)


def write_policy(
    root: Path,
    gates: dict[str, list[str]],
    *,
    schema_version: int = 1,
    source: str = "gate-shard-execution",
) -> Path:
    path = root / "policy.json"
    path.write_text(
        json.dumps(
            {
                "schema_version": schema_version,
                "source": source,
                "owner_issue": 8073,
                "migration_owner_issue": 4787,
                "gates": {
                    gate: {
                        "requires": requires,
                        "on_dependency_failure": "blocked_not_proven",
                    }
                    for gate, requires in gates.items()
                },
            }
        ),
        encoding="utf-8",
    )
    return path


def write_gate_policy(root: Path, rows: str) -> Path:
    path = root / "gate-policy.yaml"
    path.write_text(f"schema_version: 1\ngates:\n{rows}", encoding="utf-8")
    return path


def write_receipt_schema(root: Path) -> Path:
    path = root / "receipt.schema.json"
    path.write_text(
        json.dumps(
            {
                "required": [
                    "schema_version",
                    "metadata",
                    "gates",
                    "summary",
                ],
                "$defs": {
                    "metadata": {
                        "required": [
                            "timestamp",
                            "git_sha",
                            "git_branch",
                            "toolchain",
                            "platform",
                            "environment",
                        ]
                    },
                    "gate_result": {
                        "required": [
                            "gate_name",
                            "tier",
                            "status",
                            "duration_ms",
                            "command",
                        ],
                        "properties": {
                            "status": {
                                "enum": [
                                    "pass",
                                    "fail",
                                    "skip",
                                    "timeout",
                                    "error",
                                ]
                            },
                            "tier": {
                                "enum": [
                                    "commit",
                                    "pr_fast",
                                    "merge_gate",
                                    "nightly",
                                    "release",
                                ]
                            },
                        },
                    },
                    "summary": {
                        "required": [
                            "total_gates",
                            "passed",
                            "failed",
                            "skipped",
                            "total_duration_ms",
                            "overall_status",
                        ],
                        "properties": {
                            "overall_status": {
                                "enum": ["pass", "fail", "partial"]
                            }
                        },
                    },
                },
            }
        ),
        encoding="utf-8",
    )
    return path


def receipt_payload(
    gate: str,
    status: str,
    *,
    sha: str = SUBJECT,
    omit_metadata: bool = False,
    omit_summary: bool = False,
    omit_gate_field: str | None = None,
) -> dict[str, object]:
    gate_row: dict[str, object] = {
        "gate_name": gate,
        "tier": "merge_gate",
        "status": status,
        "duration_ms": 1,
        "command": f"fake {gate}",
        "exit_code": 0 if status in {"pass", "skip"} else 1,
    }
    if omit_gate_field:
        gate_row.pop(omit_gate_field, None)
    counts = {
        "passed": int(status == "pass"),
        "failed": int(status == "fail"),
        "skipped": int(status == "skip"),
        "timeout": int(status == "timeout"),
        "error": int(status == "error"),
    }
    payload: dict[str, object] = {
        "schema_version": "1.0.0",
        "gates": [gate_row],
    }
    if not omit_metadata:
        payload["metadata"] = {
            "timestamp": "2026-08-15T00:00:00Z",
            "git_sha": sha,
            "git_branch": "fixture",
            "toolchain": {"rustc_version": "rustc 1.95.0"},
            "platform": {"os": "linux", "arch": "x86_64"},
            "environment": {"type": "ci"},
        }
    if not omit_summary:
        payload["summary"] = {
            "total_gates": 1,
            **counts,
            "total_duration_ms": 1,
            "overall_status": "pass" if status in {"pass", "skip"} else "fail",
        }
    return payload


class FakeProcess:
    _next_pid = 10000

    def __init__(
        self,
        command: list[str],
        spec: dict[str, object],
        popen_options: dict[str, object],
    ) -> None:
        self.command = command
        self.spec = spec
        self.popen_options = popen_options
        self.returncode = int(spec.get("exit", 0))
        self.pid = FakeProcess._next_pid
        FakeProcess._next_pid += 1
        self.signals: list[int] = []
        self.terminated = 0
        self.killed = 0
        gate = command[command.index("--gate") + 1]
        receipt = Path(command[command.index("--receipt-path") + 1])
        receipt.parent.mkdir(parents=True, exist_ok=True)
        if spec.get("receipt", True):
            if spec.get("malformed"):
                receipt.write_text("{", encoding="utf-8")
            else:
                payload = receipt_payload(
                    str(spec.get("gate_name", gate)),
                    str(spec.get("status", "pass")),
                    sha=str(spec.get("sha", SUBJECT)),
                    omit_metadata=bool(spec.get("omit_metadata")),
                    omit_summary=bool(spec.get("omit_summary")),
                    omit_gate_field=(
                        str(spec["omit_gate_field"])
                        if spec.get("omit_gate_field")
                        else None
                    ),
                )
                if spec.get("schema_version") is not None:
                    payload["schema_version"] = spec["schema_version"]
                receipt.write_text(json.dumps(payload), encoding="utf-8")

    def wait(self, timeout: float | None = None) -> int:
        del timeout
        return self.returncode

    def poll(self) -> int:
        return self.returncode

    def send_signal(self, signum: int) -> None:
        self.signals.append(signum)

    def terminate(self) -> None:
        self.terminated += 1

    def kill(self) -> None:
        self.killed += 1


def run_direct(
    root: Path,
    behavior: dict[str, dict[str, object]],
    gates: list[str],
    *,
    dependencies: dict[str, list[str]] | None = None,
) -> tuple[int, dict[str, object], list[str], list[dict[str, object]]]:
    policy = write_policy(root, dependencies or {gate: [] for gate in gates})
    receipt_schema = write_receipt_schema(root)
    rules = shard.load_execution_policy(policy, gates)
    contract = shard.load_receipt_contract(receipt_schema)
    runner = shard.ShardRunner(
        xtask=Path("target/debug/xtask"),
        gate_policy=policy,
        receipt_dir=root / "receipts",
        summary_path=root / "summary.json",
        subject_sha=SUBJECT,
        gates=gates,
        dependency_rules=rules,
        receipt_contract=contract,
    )
    invoked: list[str] = []
    options: list[dict[str, object]] = []

    def factory(command: list[str], **kwargs: object) -> FakeProcess:
        gate = command[command.index("--gate") + 1]
        invoked.append(gate)
        options.append(dict(kwargs))
        return FakeProcess(command, behavior[gate], dict(kwargs))

    with mock.patch.object(shard.subprocess, "Popen", side_effect=factory):
        status = runner.run()
    summary = json.loads((root / "summary.json").read_text(encoding="utf-8"))
    return status, summary, invoked, options


def fake_sleeping_xtask(root: Path, marker: Path) -> Path:
    path = root / "fake-xtask.py"
    path.write_text(
        textwrap.dedent(
            f"""\
            #!{sys.executable}
            import os
            import pathlib
            import time
            pathlib.Path({str(marker)!r}).write_text(str(os.getpid()), encoding="utf-8")
            time.sleep(60)
            """
        ),
        encoding="utf-8",
    )
    path.chmod(0o755)
    return path


class RunningFakeProcess:
    def __init__(self) -> None:
        self.pid = 4242
        self.signals: list[int] = []
        self.terminated = 0
        self.killed = 0

    def poll(self) -> None:
        return None

    def send_signal(self, signum: int) -> None:
        self.signals.append(signum)

    def terminate(self) -> None:
        self.terminated += 1

    def kill(self) -> None:
        self.killed += 1


class GateShardTests(unittest.TestCase):
    def test_first_failure_does_not_mask_later_gates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, summary, invoked, _ = run_direct(
                Path(tmp),
                {
                    "first": {"status": "fail", "exit": 1},
                    "second": {"status": "pass", "exit": 0},
                    "third": {"status": "fail", "exit": 1},
                },
                ["first", "second", "third"],
            )
        self.assertEqual(1, status)
        self.assertEqual(["first", "second", "third"], invoked)
        self.assertEqual(
            ["failure", "success", "failure"],
            [row["result"] for row in summary["gates"]],
        )
        self.assertEqual(
            ["first", "third"], summary["summary"]["non_success_gates"]
        )

    def test_dependency_failure_blocks_only_the_dependent_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, summary, invoked, _ = run_direct(
                Path(tmp),
                {
                    "a": {"status": "fail", "exit": 1},
                    # Deliberately no behavior for b: invoking it would fail.
                    "c": {"status": "pass", "exit": 0},
                },
                ["b", "c", "a"],
                dependencies={"a": [], "b": ["a"], "c": []},
            )
        self.assertEqual(1, status)
        self.assertEqual(["c", "a"], invoked)
        rows = {row["gate_name"]: row for row in summary["gates"]}
        self.assertEqual("failure", rows["a"]["result"])
        self.assertEqual("blocked_not_proven", rows["b"]["result"])
        self.assertEqual(["a"], rows["b"]["blocked_by"])
        self.assertEqual("success", rows["c"]["result"])

    def test_timeout_result_does_not_mask_later_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, summary, invoked, _ = run_direct(
                Path(tmp),
                {
                    "timed": {"status": "timeout", "exit": 1},
                    "later": {"status": "pass", "exit": 0},
                },
                ["timed", "later"],
            )
        self.assertEqual(1, status)
        self.assertEqual(["timed", "later"], invoked)
        self.assertEqual(
            ["timeout", "success"],
            [row["result"] for row in summary["gates"]],
        )

    def test_missing_receipt_is_instrument_failure_and_later_gate_runs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, summary, invoked, _ = run_direct(
                Path(tmp),
                {
                    "missing": {"receipt": False, "exit": 0},
                    "later": {"status": "pass", "exit": 0},
                },
                ["missing", "later"],
            )
        self.assertEqual(1, status)
        self.assertEqual(["missing", "later"], invoked)
        self.assertEqual("instrument_failure", summary["gates"][0]["result"])
        self.assertEqual("success", summary["gates"][1]["result"])

    def test_malformed_cross_subject_or_unbound_receipt_fails_closed(self) -> None:
        for spec in (
            {"malformed": True},
            {"sha": "b" * 40},
            {"omit_metadata": True},
        ):
            with self.subTest(spec=spec), tempfile.TemporaryDirectory() as tmp:
                status, summary, _, _ = run_direct(
                    Path(tmp), {"bad": {**spec, "exit": 0}}, ["bad"]
                )
            self.assertEqual(1, status)
            self.assertEqual("instrument_failure", summary["gates"][0]["result"])

    def test_incomplete_or_unsupported_receipt_fails_closed(self) -> None:
        for spec in (
            {"omit_summary": True},
            {"omit_gate_field": "command"},
            {"status": "success", "exit": 0},
            {"schema_version": "future"},
        ):
            with self.subTest(spec=spec), tempfile.TemporaryDirectory() as tmp:
                status, summary, _, _ = run_direct(
                    Path(tmp), {"bad": spec}, ["bad"]
                )
            self.assertEqual(1, status)
            self.assertEqual("instrument_failure", summary["gates"][0]["result"])

    def test_all_success_is_zero_and_deterministically_ordered(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, summary, invoked, options = run_direct(
                Path(tmp),
                {
                    "alpha": {"status": "pass", "exit": 0},
                    "beta": {"status": "pass", "exit": 0},
                },
                ["alpha", "beta"],
            )
        self.assertEqual(0, status)
        self.assertEqual(["alpha", "beta"], invoked)
        self.assertEqual("passed", summary["summary"]["overall_status"])
        self.assertEqual(["alpha", "beta"], summary["selected_gates"])
        self.assertEqual([shard._process_group_options(os.name)] * 2, options)

    def test_zero_exit_selected_skip_is_not_success(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            status, summary, _, _ = run_direct(
                Path(tmp),
                {"skipped": {"status": "skip", "exit": 0}},
                ["skipped"],
            )
        self.assertEqual(1, status)
        self.assertEqual("not_proven", summary["gates"][0]["result"])

    def test_receipt_summary_must_reconcile_with_gate_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            behavior = {"bad": {"status": "pass", "exit": 0}}
            policy = write_policy(root, {"bad": []})
            receipt_schema = write_receipt_schema(root)
            rules = shard.load_execution_policy(policy, ["bad"])
            contract = shard.load_receipt_contract(receipt_schema)
            runner = shard.ShardRunner(
                xtask=Path("target/debug/xtask"),
                gate_policy=policy,
                receipt_dir=root / "receipts",
                summary_path=root / "summary.json",
                subject_sha=SUBJECT,
                gates=["bad"],
                dependency_rules=rules,
                receipt_contract=contract,
            )

            def factory(command: list[str], **kwargs: object) -> FakeProcess:
                process = FakeProcess(command, behavior["bad"], dict(kwargs))
                receipt = root / "receipts/bad.json"
                payload = json.loads(receipt.read_text(encoding="utf-8"))
                payload["summary"]["passed"] = 0
                payload["summary"]["failed"] = 1
                receipt.write_text(json.dumps(payload), encoding="utf-8")
                return process

            with mock.patch.object(shard.subprocess, "Popen", side_effect=factory):
                status = runner.run()
            summary = json.loads((root / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(1, status)
        self.assertEqual("instrument_failure", summary["gates"][0]["result"])

    def test_process_group_options_are_cross_platform(self) -> None:
        self.assertEqual(
            {"start_new_session": True},
            shard._process_group_options("posix"),
        )
        with mock.patch.object(
            shard.subprocess, "CREATE_NEW_PROCESS_GROUP", 512, create=True
        ):
            self.assertEqual(
                {"creationflags": 512}, shard._process_group_options("nt")
            )

    def test_process_group_cleanup_is_cross_platform(self) -> None:
        posix_process = RunningFakeProcess()
        with mock.patch.object(shard.os, "killpg", create=True) as killpg:
            shard._terminate_process_group(
                posix_process, signal.SIGTERM, platform_name="posix"
            )
        self.assertEqual(2, killpg.call_count)

        windows_process = RunningFakeProcess()
        with mock.patch.object(
            shard.signal, "CTRL_BREAK_EVENT", 123, create=True
        ):
            shard._terminate_process_group(
                windows_process, signal.SIGTERM, platform_name="nt"
            )
        self.assertEqual([123], windows_process.signals)
        self.assertEqual(1, windows_process.terminated)
        self.assertEqual(1, windows_process.killed)

    def test_committed_execution_policy_matches_current_workflow_matrix(self) -> None:
        workflow = REPO_ROOT / ".github/workflows/ci.yml"
        policy = REPO_ROOT / ".ci/gate-shard-execution.json"
        text = workflow.read_text(encoding="utf-8")
        start = text.index("  merge-gate-shards:\n")
        end = text.index("    permissions:\n", start)
        matrix_gates: set[str] = set()
        for line in text[start:end].splitlines():
            stripped = line.strip()
            if stripped.startswith("gates: "):
                matrix_gates.update(stripped.removeprefix("gates: ").split())
        payload = json.loads(policy.read_text(encoding="utf-8"))
        self.assertEqual(matrix_gates, set(payload["gates"]))
        self.assertEqual(8073, payload["owner_issue"])
        self.assertEqual(4787, payload["migration_owner_issue"])

    def test_every_current_workflow_gate_preflights_against_exact_policy(self) -> None:
        workflow = REPO_ROOT / ".github/workflows/ci.yml"
        policy = REPO_ROOT / ".ci/gate-policy.yaml"
        text = workflow.read_text(encoding="utf-8")
        start = text.index("  merge-gate-shards:\n")
        end = text.index("    permissions:\n", start)
        selected: set[str] = set()
        for line in text[start:end].splitlines():
            stripped = line.strip()
            if stripped.startswith("gates: "):
                selected.update(stripped.removeprefix("gates: ").split())
        commands = shard.load_gate_commands(policy, sorted(selected), root=REPO_ROOT)
        self.assertEqual(selected, set(commands))
        self.assertTrue(all(commands.values()))

    def test_shard_command_propagates_exact_gate_policy_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_policy(root, {"alpha": []})
            receipt_schema = write_receipt_schema(root)
            runner = shard.ShardRunner(
                xtask=Path("target/debug/xtask"),
                gate_policy=policy,
                receipt_dir=root / "receipts",
                summary_path=root / "summary.json",
                subject_sha=SUBJECT,
                gates=["alpha"],
                dependency_rules=shard.load_execution_policy(policy, ["alpha"]),
                receipt_contract=shard.load_receipt_contract(receipt_schema),
            )
            command = runner._command("alpha")
        self.assertEqual(str(policy), command[command.index("--gate-policy") + 1])

    def test_current_tree_source_commit_api_gate_has_executable_policy_command(self) -> None:
        policy = REPO_ROOT / ".ci/gate-policy.yaml"
        commands = shard.load_gate_commands(
            policy,
            ["source_commit_api_check"],
            root=REPO_ROOT,
        )
        self.assertEqual(
            "python3 scripts/ci/check_source_commit_api.py",
            commands["source_commit_api_check"],
        )
        self.assertTrue((REPO_ROOT / "scripts/ci/check_source_commit_api.py").is_file())
        policy_text = policy.read_text(encoding="utf-8")
        policy_row = re.search(
            r"(?ms)^  - name: source_commit_api_check\n(?P<body>.*?)(?=^  - name:|\Z)",
            policy_text,
        )
        self.assertIsNotNone(policy_row)
        assert policy_row is not None
        self.assertIn("    tier: pr_fast\n", policy_row.group("body"))
        self.assertIn(
            "    command: python3 scripts/ci/check_source_commit_api.py\n",
            policy_row.group("body"),
        )
        execution = json.loads(
            (REPO_ROOT / ".ci/gate-shard-execution.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            {"requires": [], "on_dependency_failure": "blocked_not_proven"},
            execution["gates"]["source_commit_api_check"],
        )
        workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        start = workflow.index("  merge-gate-shards:\n")
        end = workflow.index("    permissions:\n", start)
        lane: str | None = None
        lanes: dict[str, set[str]] = {}
        for line in workflow[start:end].splitlines():
            stripped = line.strip()
            if stripped.startswith("- name: "):
                lane = stripped.removeprefix("- name: ")
            elif lane is not None and stripped.startswith("gates: "):
                lanes[lane] = set(stripped.removeprefix("gates: ").split())
        self.assertEqual(
            ["meta"],
            [name for name, gates in lanes.items() if "source_commit_api_check" in gates],
        )
        run_start = workflow.index("      - name: Run merge-gate shard with receipts")
        run_end = workflow.index("      - name:", run_start + 1)
        runner_step = workflow[run_start:run_end]
        invocation_start = runner_step.index("python3 scripts/ci/run_gate_shard.py")
        invocation_end = runner_step.index("          status=$?", invocation_start)
        runner_invocation = runner_step[invocation_start:invocation_end]
        self.assertIn("--gate-policy .ci/gate-policy.yaml", runner_invocation)

    def test_multiline_commands_match_actual_yaml_loader(self) -> None:
        yaml_spec = importlib.util.find_spec("yaml")
        if yaml_spec is None:
            self.skipTest("PyYAML is unavailable for the parser recurrence oracle")
        import yaml

        policy = REPO_ROOT / ".ci/gate-policy.yaml"
        document = yaml.safe_load(policy.read_text(encoding="utf-8"))
        expected = {
            row["name"]: row["command"]
            for row in document["gates"]
            if row["name"] in {"nested_lock_check", "determinism_check"}
        }
        actual = shard._read_gate_command_specs(policy)
        self.assertEqual(expected, {name: actual[name] for name in expected})

    def test_fallback_parser_matches_multiline_commands_and_rejects_yaml_extensions(self) -> None:
        yaml_spec = importlib.util.find_spec("yaml")
        if yaml_spec is None:
            self.skipTest("PyYAML is unavailable for the fallback parity oracle")
        import yaml

        policy = REPO_ROOT / ".ci/gate-policy.yaml"
        document = yaml.safe_load(policy.read_text(encoding="utf-8"))
        expected = {
            row["name"]: row["command"]
            for row in document["gates"]
            if row["name"] in {"nested_lock_check", "determinism_check"}
        }
        with mock.patch.object(shard, "yaml", None):
            fallback = shard._read_gate_command_specs(policy)
            self.assertEqual(expected, {name: fallback[name] for name in expected})
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                comment_policy = write_gate_policy(
                    root,
                    "  - name: comment\n    command: echo # inline comment\n",
                )
                self.assertEqual(
                    "echo",
                    shard._read_gate_command_specs(comment_policy)["comment"],
                )
                for scalar in ("!!str echo", "&command echo"):
                    tagged_policy = write_gate_policy(
                        root,
                        "  - name: tagged\n    command: " + scalar + "\n",
                    )
                    with self.subTest(scalar=scalar), self.assertRaisesRegex(
                        ValueError, "YAML string"
                    ):
                        shard._read_gate_command_specs(tagged_policy)
                adjacent_policy = write_gate_policy(
                    root,
                    "  - name: adjacent\n    command: 'echo' 'outside'\n",
                )
                with mock.patch.object(shard, "yaml", None), self.assertRaisesRegex(
                    ValueError, "adjacent quoted"
                ):
                    shard._read_gate_command_specs(adjacent_policy)

    def test_fallback_parser_rejects_adjacent_double_quoted_scalars_without_pyyaml(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_gate_policy(
                root,
                '  - name: adjacent_double\n    command: "echo" "outside"\n',
            )
            with mock.patch.object(shard, "yaml", None), self.assertRaisesRegex(
                ValueError, "adjacent quoted"
            ):
                shard._read_gate_command_specs(policy)

    def test_gate_policy_preflight_rejects_qualified_unsafe_command_basenames(self) -> None:
        commands = ("./source", "./eval", "./exec", "./cd", "./alias")
        for command in commands:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: source_commit_api_check\n"
                    f"    command: {command} scripts/ci/check.py\n",
                )
                with self.assertRaisesRegex(ValueError, "unsupported shell command"):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=root,
                    )

    def test_repository_root_derivation_matches_xtask_from_subdirectory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".ci").mkdir()
            (root / ".ci/gate-policy.yaml").write_text(
                "schema_version: 1\ngates:\n", encoding="utf-8"
            )
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (root / "target/debug").mkdir(parents=True)
            nested = root / "scripts/ci"
            nested.mkdir(parents=True)
            derived = shard._repository_root_for_execution(
                Path("target/debug/xtask"),
                Path(".ci/gate-policy.yaml"),
                cwd=nested,
            )
        self.assertEqual(root.resolve(), derived)
        self.assertEqual(
            root / "target/debug/xtask",
            shard._path_from_repository_root(Path("target/debug/xtask"), root),
        )
        self.assertEqual(
            root / "receipts",
            shard._path_from_repository_root(Path("receipts"), root),
        )

    def test_main_rebases_relative_launch_paths_from_subdirectory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".ci").mkdir()
            (root / ".ci/gate-policy.yaml").write_text(
                "schema_version: 1\ngates:\n"
                "  - name: alpha\n"
                "    command: echo alpha\n",
                encoding="utf-8",
            )
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            execution_policy = write_policy(root, {"alpha": []})
            receipt_schema = write_receipt_schema(root)
            nested = root / "scripts/ci"
            nested.mkdir(parents=True)
            runner_type = mock.Mock()
            runner_type.return_value.run.return_value = 0
            with (
                mock.patch.object(shard.Path, "cwd", return_value=nested),
                mock.patch.object(shard, "ShardRunner", runner_type),
                mock.patch.object(shard.signal, "signal"),
            ):
                status = shard.main(
                    [
                        "--xtask",
                        "target/debug/xtask",
                        "--receipt-dir",
                        "receipts",
                        "--summary",
                        "summary.json",
                        "--execution-policy",
                        str(execution_policy.relative_to(root)),
                        "--receipt-schema",
                        str(receipt_schema.relative_to(root)),
                        "--gate-policy",
                        ".ci/gate-policy.yaml",
                        "alpha",
                    ]
                )
            self.assertEqual(0, status)
            kwargs = runner_type.call_args.kwargs
        self.assertEqual(root / "target/debug/xtask", kwargs["xtask"])
        self.assertEqual(root / "receipts", kwargs["receipt_dir"])
        self.assertEqual(root / "summary.json", kwargs["summary_path"])
        self.assertEqual(root / ".ci/gate-policy.yaml", kwargs["gate_policy"])

    def test_gate_policy_preflight_rejects_missing_row(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            policy = write_gate_policy(
                Path(tmp),
                "  - name: other\n    command: echo\n",
            )
            with self.assertRaisesRegex(ValueError, "no gate-policy row"):
                shard.load_gate_commands(policy, ["source_commit_api_check"], root=Path(tmp))

    def test_gate_policy_preflight_rejects_missing_command(self) -> None:
        for rows, message in (
            ("  - name: source_commit_api_check\n", "no executable command"),
            ("  - name: source_commit_api_check\n    command:\n", "YAML string"),
        ):
            with self.subTest(rows=rows), tempfile.TemporaryDirectory() as tmp:
                policy = write_gate_policy(Path(tmp), rows)
                with self.assertRaisesRegex(ValueError, message):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=Path(tmp),
                    )

    def test_gate_policy_preflight_rejects_non_string_command_types(self) -> None:
        for scalar in (
            "true",
            "null",
            "42",
            "0x10",
            "0x10_00",
            "1_000",
            ".inf",
            "2026-08-21",
            "2026-08-21T12:34:56Z",
            "2026-08-21 12:34:56+00:00",
            "1:20",
            "1:20:30",
            "&command_anchor python3 scripts/check.py",
            "*command_anchor",
            "true # inline comment",
            "0x10 # inline comment",
            "[]",
            "{}",
        ):
            with self.subTest(scalar=scalar), tempfile.TemporaryDirectory() as tmp:
                policy = write_gate_policy(
                    Path(tmp),
                    "  - name: source_commit_api_check\n"
                    f"    command: {scalar}\n",
                )
                with self.assertRaisesRegex(ValueError, "YAML string"):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=Path(tmp),
                    )

    def test_gate_policy_preflight_rejects_yaml_block_scalar_indicators(self) -> None:
        for indicator in ("|2", ">-2", ">+4"):
            with self.subTest(indicator=indicator), tempfile.TemporaryDirectory() as tmp:
                policy = write_gate_policy(
                    Path(tmp),
                    "  - name: source_commit_api_check\n"
                    f"    command: {indicator}\n",
                )
                with self.assertRaisesRegex(ValueError, "YAML block scalar"):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=Path(tmp),
                    )

    def test_gate_policy_preflight_rejects_missing_referenced_command_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            policy = write_gate_policy(
                Path(tmp),
                "  - name: source_commit_api_check\n"
                "    command: python3 scripts/ci/missing.py\n",
            )
            with self.assertRaisesRegex(ValueError, "missing command path"):
                shard.load_gate_commands(
                    policy,
                    ["source_commit_api_check"],
                    root=Path(tmp),
                )

    def test_gate_policy_preflight_rejects_missing_path_after_interpreter_option(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            policy = write_gate_policy(
                Path(tmp),
                "  - name: source_commit_api_check\n"
                "    command: python3 -u scripts/ci/missing.py\n",
            )
            with self.assertRaisesRegex(ValueError, "missing command path"):
                shard.load_gate_commands(
                    policy,
                    ["source_commit_api_check"],
                    root=Path(tmp),
                )

    def test_gate_policy_preflight_rejects_missing_explicit_input_path(self) -> None:
        for command, message in (
            ("cargo run --manifest .ci/missing-manifest.txt", "missing command path"),
            ("cargo run --manifest", "missing its value"),
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                policy = write_gate_policy(
                    Path(tmp),
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, message):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=Path(tmp),
                    )

    def test_gate_policy_preflight_confines_output_option_paths(self) -> None:
        flags = (
            "--artifact-dir",
            "--json-out",
            "--log-path",
            "--output",
            "--receipt-dir",
            "--receipt-path",
            "--report",
            "--summary",
            "--output-path",
        )
        for flag in flags:
            for form in ("separate", "equals"):
                with self.subTest(flag=flag, form=form), tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    (root / "scripts/ci").mkdir(parents=True)
                    (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                    value = "../outside.log"
                    option = f"{flag} {value}" if form == "separate" else f"{flag}={value}"
                    policy = write_gate_policy(
                        root,
                        "  - name: output_path\n"
                        f"    command: python3 scripts/ci/check.py {option}\n",
                    )
                    with self.assertRaisesRegex(ValueError, "checked-out tree"):
                        shard.load_gate_commands(policy, ["output_path"], root=root)

    def test_gate_policy_preflight_confines_optional_receipt_paths(self) -> None:
        for command in (
            "python3 scripts/ci/check.py --receipt ../outside.json",
            "python3 scripts/ci/check.py --receipt=../outside.json",
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: receipt_path\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, "checked-out tree"):
                    shard.load_gate_commands(policy, ["receipt_path"], root=root)

    def test_gate_policy_preflight_rejects_windows_nested_cmd_wrappers(self) -> None:
        for command in (
            "cmd /C python3 ../outside.py",
            "cmd.exe /C python3 ../outside.py",
            "cmd.exe /c scripts/ci/missing.py",
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                policy = write_gate_policy(
                    Path(tmp),
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, "nested-shell wrapper"):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=Path(tmp),
                    )

    def test_gate_policy_preflight_rejects_extensionless_and_incomplete_paths(self) -> None:
        for command, message in (
            ("python3 missing_script", "missing command path"),
            ("python3", "missing its script path"),
            ("python3 -W", "missing its value"),
            ("cargo --output", "missing its value"),
            ("cargo --receipt-path", "missing its value"),
            ("cargo --output --manifest", "missing its value"),
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                policy = write_gate_policy(
                    Path(tmp),
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, message):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=Path(tmp),
                    )

    def test_gate_policy_preflight_supports_shell_wrappers_and_comments(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "scripts/ci/first.sh").write_text("#!/bin/sh\n", encoding="utf-8")
            (root / "scripts/ci/second.sh").write_text("#!/bin/sh\n", encoding="utf-8")
            policy = write_gate_policy(
                root,
                "  - name: first\n"
                "    command: bash -u scripts/ci/first.sh # inline comment\n"
                "# A top-level YAML comment must not end the gates sequence.\n"
                "  - name: second\n"
                "    command: sh ./scripts/ci/second.sh\n",
            )
            commands = shard.load_gate_commands(policy, ["first", "second"], root=root)
            self.assertEqual(
                "bash -u scripts/ci/first.sh",
                commands["first"],
            )
            self.assertEqual("sh ./scripts/ci/second.sh", commands["second"])

    def test_gate_policy_preflight_decodes_yaml_single_quote_escaping(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = write_gate_policy(
                root,
                "  - name: quoted\n"
                "    command: 'echo ''ok'''\n",
            )
            commands = shard.load_gate_commands(policy, ["quoted"], root=root)
            self.assertEqual("echo 'ok'", commands["quoted"])

    def test_gate_policy_preflight_rejects_path_outside_checked_out_tree(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            parent = Path(tmp)
            root = parent / "repo"
            root.mkdir()
            (parent / "outside.py").write_text("# outside\n", encoding="utf-8")
            policy = write_gate_policy(
                root,
                "  - name: source_commit_api_check\n"
                "    command: python3 ../outside.py\n",
            )
            with self.assertRaisesRegex(ValueError, "outside checked-out tree"):
                shard.load_gate_commands(
                    policy,
                    ["source_commit_api_check"],
                    root=root,
                )

    def test_gate_policy_preflight_rejects_unresolvable_shell_expansion(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            policy = write_gate_policy(
                Path(tmp),
                "  - name: source_commit_api_check\n"
                "    command: python3 $SCRIPT\n",
            )
            with self.assertRaisesRegex(ValueError, "shell expansion"):
                shard.load_gate_commands(
                    policy,
                    ["source_commit_api_check"],
                    root=Path(tmp),
                )

    def test_gate_policy_preflight_rejects_ambiguous_hash_and_windows_expansion(self) -> None:
        commands = (
            "python3 scripts/ci/check.py#comment; echo outside",
            "python3 scripts/ci/check.py;#comment; echo outside",
            "python3 %SCRIPT%",
            "%ComSpec% /C python3 ../outside.py",
        )
        for command in commands:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaises(ValueError):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=root,
                    )

    def test_gate_policy_preflight_only_allows_structured_determinism_log(self) -> None:
        for command in (
            "python3 scripts/ci/check.py --output run_${i}.log",
            "echo run_${i}.log",
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: dynamic_output\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, "expansion"):
                    shard.load_gate_commands(policy, ["dynamic_output"], root=root)

    def test_gate_policy_preflight_rejects_dynamic_separators_and_constructs(self) -> None:
        commands = (
            ("${GATE_SCRIPT}", "shell brace expansion"),
            ("echo ; ${GATE_SCRIPT}", "shell brace expansion"),
            ("echo; ${GATE_SCRIPT}", "shell brace expansion"),
            (
                "python3 scripts/ci/check.py ; ${GATE_SCRIPT}",
                "shell brace expansion",
            ),
            ("echo &&", "shell separator"),
            ("echo ||", "shell separator"),
            ("echo |", "shell separator"),
            (
                "while true; do python3 scripts/ci/check.py; done",
                "unsupported shell construct",
            ),
            (
                "python3 scripts/ci/check.py [[ -f scripts/ci/check.py ]]",
                "unsupported shell construct",
            ),
        )
        for command, message in commands:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, message):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=root,
                    )

    def test_gate_policy_preflight_rejects_assignments_grouping_globs_and_windows_paths(self) -> None:
        commands = (
            "FOO=bar python3 scripts/ci/check.py",
            "if FOO=bar; then python3 scripts/ci/check.py; fi",
            "then FOO=bar python3 scripts/ci/check.py",
            "for FOO=bar in one; do python3 scripts/ci/check.py; done",
            "else FOO=bar python3 scripts/ci/check.py",
            "echo ( python3 scripts/ci/check.py )",
            "python3 scripts/ci/check.py {one,two}",
            "python3 scripts/ci/check.py *.py",
            "python3 scripts/ci/check.py [abc]",
            "python3 scripts\\ci\\check.py",
            "python3 C:\\outside.py",
            "python3 scripts/ci/check.py --output-path ..\\outside.log",
            "!!str python3 scripts/ci/check.py",
        )
        for command in commands:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: unsafe\n"
                    f"    command: {command}\n",
                )
                with self.assertRaises(ValueError):
                    shard.load_gate_commands(policy, ["unsafe"], root=root)

    def test_gate_policy_preflight_rejects_command_wrapper_operands_with_existing_fixture(
        self,
    ) -> None:
        for command in (
            "command scripts/ci/check.py",
            "command python3 scripts/ci/check.py",
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: wrapper\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, "nested-shell wrapper"):
                    shard.load_gate_commands(policy, ["wrapper"], root=root)

    def test_gate_policy_preflight_rejects_exe_wrapper_aliases_with_existing_fixture(
        self,
    ) -> None:
        for command in (
            "command.exe scripts/ci/check.py",
            "env.exe -- scripts/ci/check.py",
            "timeout.exe 1 python3 scripts/ci/check.py",
            "xargs.exe python3 scripts/ci/check.py",
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: wrapper\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, "nested-shell wrapper"):
                    shard.load_gate_commands(policy, ["wrapper"], root=root)

    def test_gate_policy_preflight_rejects_missing_input_redirect_targets(self) -> None:
        for command, message in (
            ("python3 scripts/ci/check.py < scripts/ci/missing.txt", "missing input path"),
            ("python3 scripts/ci/check.py <../outside.txt", "checked-out tree"),
            ("python3 scripts/ci/check.py <", "missing its target"),
            ("python3 scripts/ci/check.py <~/outside.txt", "tilde expansion"),
            ("python3 scripts/ci/check.py >~/outside.log", "tilde expansion"),
        ):
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                parent = Path(tmp)
                root = parent / "repo"
                root.mkdir()
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                (parent / "outside.txt").write_text("outside\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, message):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=root,
                    )

    def test_gate_policy_preflight_tracks_env_assignment_wrappers(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
            policy = write_gate_policy(
                root,
                "  - name: normal\n"
                "    command: env NAME=foo.py python3 scripts/ci/check.py\n"
                "  - name: after_options\n"
                "    command: env -- NAME=foo.py python3 scripts/ci/check.py\n",
            )
            commands = shard.load_gate_commands(
                policy,
                ["normal", "after_options"],
                root=root,
            )
            self.assertEqual(
                "env NAME=foo.py python3 scripts/ci/check.py",
                commands["normal"],
            )
            self.assertEqual(
                "env -- NAME=foo.py python3 scripts/ci/check.py",
                commands["after_options"],
            )

    def test_gate_policy_preflight_rejects_nested_shell_and_unsafe_wrappers(self) -> None:
        commands = (
            ("bash -c 'python3 scripts/ci/check.py'", "nested interpreter command"),
            ("env -S 'python3 scripts/ci/check.py'", "env -S"),
            ("env NAME=foo.py python3 ../outside.py", "outside checked-out tree"),
            ("python3 -uscripts/ci/check.py", "attached interpreter option"),
            ("python3 scripts/ci/check.py > ../outside.log", "checked-out tree"),
            ("python3 scripts/ci/check.py >../outside.log", "checked-out tree"),
            ("python3 scripts/ci/check.py >>../outside.log", "checked-out tree"),
            ("python3 scripts/ci/check.py &> ../outside.log", "Bash redirection"),
            ("python3 scripts/ci/check.py &>> ../outside.log", "Bash redirection"),
            ("python3 scripts/ci/check.py >| ../outside.log", "Bash redirection"),
            ("python3 scripts/ci/check.py `pwd`", "backtick substitution"),
            ("python3 -m missing_module", "nested interpreter command"),
            ("cd scripts/ci && python3 scripts/ci/check.py", "unsupported shell command"),
            ("alias py=python3; py scripts/ci/check.py", "unsupported shell command"),
            ("pwsh -Command 'python3 scripts/ci/check.py'", "nested interpreter command"),
            ("pwsh -File ../outside.ps1", "checked-out tree"),
            ("powershell.exe -Command 'python3 scripts/ci/check.py'", "nested interpreter command"),
            ("powershell.exe -File ../outside.ps1", "checked-out tree"),
            ("call python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("start python3 scripts/ci/check.py", "nested-shell wrapper"),
            ('"%ComSpec% /C python3 scripts/ci/check.py"', "dynamic shell expansion"),
            ("python3 scripts/ci/check.py ;; echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py ;& echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py ;;& echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py &&& echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py ||| echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py ;;;; echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py &&&& echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py |||| echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py |& echo ok", "malformed shell punctuation"),
            ("python3 scripts/ci/check.py <>", "malformed shell punctuation"),
            ("xargs bash -c 'python3 scripts/ci/check.py'", "nested-shell wrapper"),
            ("nice python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("nohup python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("setsid python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("parallel python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("busybox sh scripts/ci/check.py", "nested-shell wrapper"),
            ("sudo python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("chroot ../outside python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("taskset -c 0 python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("stdbuf -oL python3 scripts/ci/check.py", "nested-shell wrapper"),
            ("time python3 scripts/ci/check.py", "nested-shell wrapper"),
            (
                "find . -exec bash -c 'python3 scripts/ci/check.py' {} +",
                "find -exec",
            ),
            ("command bash -c 'python3 scripts/ci/check.py'", "nested-shell wrapper"),
            ("timeout 10s bash -c 'python3 scripts/ci/check.py'", "nested-shell wrapper"),
        )
        for command, message in commands:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "scripts/ci").mkdir(parents=True)
                (root / "scripts/ci/check.py").write_text("# check\n", encoding="utf-8")
                policy = write_gate_policy(
                    root,
                    "  - name: source_commit_api_check\n"
                    f"    command: {command}\n",
                )
                with self.assertRaisesRegex(ValueError, message):
                    shard.load_gate_commands(
                        policy,
                        ["source_commit_api_check"],
                        root=root,
                    )

    def test_gate_policy_header_allows_trailing_whitespace(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            (root / "scripts/check.py").write_text("# check\n", encoding="utf-8")
            policy = root / "gate-policy.yaml"
            policy.write_text(
                "schema_version: 1\n"
                "gates:   \n"
                "  - name: source_commit_api_check\n"
                "    command: python3 scripts/check.py\n",
                encoding="utf-8",
            )
            commands = shard.load_gate_commands(
                policy,
                ["source_commit_api_check"],
                root=root,
            )
            self.assertEqual("python3 scripts/check.py", commands["source_commit_api_check"])

    def test_gate_policy_block_scalar_header_comment_is_ignored(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            (root / "scripts/check.py").write_text("# check\n", encoding="utf-8")
            policy = root / "gate-policy.yaml"
            policy.write_text(
                "schema_version: 1\n"
                "gates:\n"
                "  - name: source_commit_api_check\n"
                "    command: >- # header comment\n"
                "      python3 scripts/check.py\n",
                encoding="utf-8",
            )
            commands = shard.load_gate_commands(policy, ["source_commit_api_check"], root=root)
            self.assertEqual("python3 scripts/check.py", commands["source_commit_api_check"])

    def test_canonical_receipt_schema_is_loadable(self) -> None:
        contract = shard.load_receipt_contract(
            REPO_ROOT / ".ci/receipt.schema.json"
        )
        self.assertEqual(
            {"pass", "fail", "skip", "timeout", "error"},
            set(contract.gate_statuses),
        )

    def test_invalid_dependency_policy_is_rejected_before_execution(self) -> None:
        for gates, selected in (
            ({"a": ["missing"]}, ["a"]),
            ({"a": ["b"], "b": ["a"]}, ["a", "b"]),
            ({"a": []}, ["a", "missing-row"]),
        ):
            with (
                self.subTest(gates=gates, selected=selected),
                tempfile.TemporaryDirectory() as tmp,
            ):
                policy = write_policy(Path(tmp), gates)
                with self.assertRaises(ValueError):
                    shard.load_execution_policy(policy, selected)

    def test_duplicate_and_unsafe_gate_ids_are_rejected_before_execution(self) -> None:
        duplicate = argparse.Namespace(
            gates=["alpha", "alpha"], subject_sha=SUBJECT
        )
        unsafe = argparse.Namespace(gates=["../alpha"], subject_sha=SUBJECT)
        with self.assertRaises(ValueError):
            shard.validate_args(duplicate)
        with self.assertRaises(ValueError):
            shard.validate_args(unsafe)

    def test_stale_receipt_is_removed_before_invocation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = root / "receipts"
            receipts.mkdir()
            (receipts / "alpha.json").write_text(
                json.dumps(receipt_payload("alpha", "pass")),
                encoding="utf-8",
            )
            status, summary, _, _ = run_direct(
                root,
                {"alpha": {"receipt": False, "exit": 0}},
                ["alpha"],
            )
        self.assertEqual(1, status)
        self.assertEqual("instrument_failure", summary["gates"][0]["result"])

    @unittest.skipUnless(
        os.name != "nt" and hasattr(os, "killpg"),
        "POSIX process-group integration test",
    )
    def test_termination_kills_running_process_and_preserves_partial_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "slow.pid"
            xtask = fake_sleeping_xtask(root, marker)
            policy = write_policy(root, {"slow": [], "later": []})
            gate_policy = write_gate_policy(
                root,
                "  - name: slow\n"
                "    command: echo slow\n"
                "  - name: later\n"
                "    command: echo later\n",
            )
            receipt_schema = write_receipt_schema(root)
            process = subprocess.Popen(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--xtask",
                    str(xtask),
                    "--receipt-dir",
                    str(root / "receipts"),
                    "--summary",
                    str(root / "summary.json"),
                    "--execution-policy",
                    str(policy),
                    "--gate-policy",
                    str(gate_policy),
                    "--receipt-schema",
                    str(receipt_schema),
                    "--subject-sha",
                    SUBJECT,
                    "slow",
                    "later",
                ],
                cwd=root,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            deadline = time.monotonic() + 10
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertTrue(marker.exists(), "fake gate never started")
            child_pid = int(marker.read_text(encoding="utf-8"))
            process.terminate()
            returncode = process.wait(timeout=10)
            summary = json.loads(
                (root / "summary.json").read_text(encoding="utf-8")
            )
            child_deadline = time.monotonic() + 5
            while time.monotonic() < child_deadline:
                try:
                    os.kill(child_pid, 0)
                except OSError:
                    break
                time.sleep(0.05)
            else:
                self.fail("running gate process survived shard termination")
        self.assertEqual(143, returncode)
        self.assertEqual(
            ["cancelled", "not_proven"],
            [row["result"] for row in summary["gates"]],
        )

    def test_build_parser_registers_gate_policy_argument(self) -> None:
        """Regression guard: build_parser() must declare --gate-policy or workflow shards fail.

        The hosted CI gate-shard workflow invokes::

            python3 scripts/ci/run_gate_shard.py --gate-policy .ci/gate-policy.yaml ...

        If build_parser() ever loses that argument (for example, because an in-flight
        PR branch carries a stale copy of the script), every runner shard exits with
        "unrecognized arguments: --gate-policy <gate-name>" before executing any gate,
        making the CI evidence unavailable (see issue #11925).

        By asserting the parser's declared options here, the ci-gate-self-tests shard
        catches the incompatibility at the earliest shared authority — in the base
        branch, before any merge — rather than at hosted shard execution time.
        """
        parser = shard.build_parser()

        # Supplying --gate-policy must produce no unrecognised tokens.
        _, unknown = parser.parse_known_args(
            [
                "--xtask", "target/debug/xtask",
                "--receipt-dir", "receipts",
                "--summary", "summary.json",
                "--gate-policy", ".ci/gate-policy.yaml",
                "gate1",
            ]
        )
        self.assertEqual(
            [],
            unknown,
            "--gate-policy must be a registered argument so the workflow invocation is accepted",
        )

        # The argument must be resolved as a Path (not a plain string) to match
        # the path-rebasing logic in main().
        parsed, _ = parser.parse_known_args(
            [
                "--xtask", "target/debug/xtask",
                "--receipt-dir", "receipts",
                "--summary", "summary.json",
                "--gate-policy", "custom/policy.yaml",
                "gate1",
            ]
        )
        self.assertIsInstance(
            parsed.gate_policy,
            Path,
            "--gate-policy must be typed as Path",
        )
        self.assertEqual(
            Path("custom/policy.yaml"),
            parsed.gate_policy,
        )

    def test_build_parser_gate_policy_defaults_to_canonical_ci_path(self) -> None:
        """build_parser() default for --gate-policy must match the workflow's hard-coded path.

        The CI workflow passes ``--gate-policy .ci/gate-policy.yaml`` on every shard
        invocation.  This test asserts two things at once:

        1. The flag is optional (a default exists, so callers that omit it still work).
        2. The default is the canonical path ``Path(".ci/gate-policy.yaml")``, which is
           what the workflow would use when the flag is provided but the value matches
           the default — confirming that the two stay in sync even if the explicit flag
           is later dropped from the workflow or defaulted differently in the script.
        """
        parser = shard.build_parser()
        parsed, _ = parser.parse_known_args(
            [
                "--xtask", "target/debug/xtask",
                "--receipt-dir", "receipts",
                "--summary", "summary.json",
                "gate1",
            ]
        )
        self.assertEqual(
            Path(".ci/gate-policy.yaml"),
            parsed.gate_policy,
            "--gate-policy default must be Path('.ci/gate-policy.yaml') to match the workflow",
        )

    def test_workflow_and_parser_agree_on_gate_policy_flag(self) -> None:
        """The gate-policy flag passed by the CI workflow must be accepted by build_parser().

        This test reads the exact --gate-policy invocation from the workflow YAML and
        feeds it into the argument parser, confirming the two sources of truth stay
        compatible as the repository evolves.  It is the direct regression guard for
        issue #11925, where a stale PR branch had a parser that did not accept
        --gate-policy, breaking every hosted shard before any gate ran.
        """
        workflow = REPO_ROOT / ".github/workflows/ci.yml"
        if not workflow.is_file():
            self.skipTest("ci.yml not present in this checkout")

        text = workflow.read_text(encoding="utf-8")
        # Locate the runner step in the workflow.
        try:
            run_start = text.index("      - name: Run merge-gate shard with receipts")
            run_end = text.index("      - name:", run_start + 1)
        except ValueError:
            self.skipTest("runner step not found in ci.yml")

        runner_step = text[run_start:run_end]

        # Extract the --gate-policy value from the workflow invocation.
        match = re.search(r"--gate-policy\s+(\S+)", runner_step)
        self.assertIsNotNone(
            match,
            "ci.yml runner step must pass --gate-policy to run_gate_shard.py",
        )
        assert match is not None
        workflow_policy_path = match.group(1)

        # Feed the workflow's exact --gate-policy value into the parser.
        parser = shard.build_parser()
        _, unknown = parser.parse_known_args(
            [
                "--xtask", "target/debug/xtask",
                "--receipt-dir", "receipts",
                "--summary", "summary.json",
                "--gate-policy", workflow_policy_path,
                "gate1",
            ]
        )
        self.assertEqual(
            [],
            unknown,
            f"--gate-policy {workflow_policy_path!r} from ci.yml is not accepted by build_parser()",
        )


if __name__ == "__main__":
    unittest.main()
