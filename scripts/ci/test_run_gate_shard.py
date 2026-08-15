#!/usr/bin/env python3
"""Falsifiers for run_gate_shard.py."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
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
                    "--receipt-schema",
                    str(receipt_schema),
                    "--subject-sha",
                    SUBJECT,
                    "slow",
                    "later",
                ],
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


if __name__ == "__main__":
    unittest.main()
