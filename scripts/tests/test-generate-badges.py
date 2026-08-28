#!/usr/bin/env python3
import json
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SCRIPT = Path(__file__).parents[1] / "generate-badges.py"
WORKFLOW = Path(__file__).parents[2] / ".github/workflows/badge-endpoints.yml"
RUST_DELEGATE = Path(__file__).parents[2] / "xtask/src/tasks/badges.rs"
BADGE_README = Path(__file__).parents[2] / "badges/README.md"
SPEC = importlib.util.spec_from_file_location("ripr_badge_generator", SCRIPT)
assert SPEC is not None
generator = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(generator)

VALID_COUNTS = {
    "unsuppressed_exposure_gaps": 0,
    "unsuppressed_test_efficiency_findings": 0,
}

FAKE_RIPR = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = str(Path(os.environ["FAKE_RIPR_EXPECTED_ROOT"]).resolve())
observed = {"argv": sys.argv[1:], "cwd": str(Path.cwd().resolve())}
Path(os.environ["FAKE_RIPR_RECORD"]).write_text(json.dumps(observed), encoding="utf-8")
expected = ["check", "--root", root, "--format", "repo-badge-json"]
if observed != {"argv": expected, "cwd": root}:
    print("unexpected fake RIPR invocation", file=sys.stderr)
    raise SystemExit(64)
if os.environ.get("FAKE_RIPR_SPAWN_CHILD") == "1":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(300)"])
    Path(os.environ["FAKE_RIPR_CHILD_PID"]).write_text(str(child.pid), encoding="utf-8")
    if os.environ.get("FAKE_RIPR_EXIT_AFTER_CHILD") == "1":
        raise SystemExit(0)
if os.environ.get("FAKE_RIPR_HANG") == "1":
    time.sleep(300)
stdout_bytes = int(os.environ.get("FAKE_RIPR_STDOUT_BYTES", "0"))
stderr_bytes = int(os.environ.get("FAKE_RIPR_STDERR_BYTES", "0"))
if stdout_bytes:
    sys.stdout.write("o" * stdout_bytes)
    sys.stdout.flush()
if stderr_bytes:
    sys.stderr.write("e" * stderr_bytes)
    sys.stderr.flush()
if os.environ.get("FAKE_RIPR_AFTER_OUTPUT_HANG") == "1":
    time.sleep(300)
print(os.environ["FAKE_RIPR_PAYLOAD"])
trailing_stdout_bytes = int(os.environ.get("FAKE_RIPR_TRAILING_STDOUT_BYTES", "0"))
if trailing_stdout_bytes:
    sys.stdout.write(" " * trailing_stdout_bytes)
    sys.stdout.flush()
print(os.environ.get("FAKE_RIPR_STDERR", ""), file=sys.stderr)
raise SystemExit(int(os.environ.get("FAKE_RIPR_EXIT", "0")))
'''


def validate_workflow_contract(text: str) -> None:
    try:
        generate, open_pr = text.split("\n  open-pr:\n", 1)
    except ValueError as error:
        raise ValueError("badge workflow is missing the separated PR writer") from error
    compact = " ".join(generate.split())
    required = [
        "source_sha: description:",
        "required: true type: string",
        "github.event_name == 'workflow_dispatch' && inputs.source_sha == github.sha",
        "ref: ${{ github.sha }}",
        "timeout-minutes: 20",
        "run: python3 scripts/generate-badges.py",
        "permissions: contents: read",
    ]
    for fragment in required:
        if fragment not in compact:
            raise ValueError(f"badge workflow contract is missing {fragment!r}")
    if "cargo xtask badges" in text:
        raise ValueError("the displaced Rust badge mapper remains in workflow guidance")
    writer_required = [
        "github.event_name == 'push'",
        "github.ref == 'refs/heads/main'",
        "contents: write",
        "pull-requests: write",
    ]
    for fragment in writer_required:
        if fragment not in open_pr:
            raise ValueError(f"badge PR writer contract is missing {fragment!r}")
    if "github.event_name == 'workflow_dispatch'" in open_pr:
        raise ValueError("manual candidate proof must not admit the write-capable PR job")


class GenerateBadgesTests(unittest.TestCase):
    def test_rust_compatibility_entrypoint_only_delegates_to_python_owner(self):
        source = RUST_DELEGATE.read_text(encoding="utf-8")
        self.assertIn("scripts/generate-badges.py", source)
        for displaced_semantic in [
            "unsuppressed_exposure_gaps",
            "unsuppressed_test_efficiency_findings",
            "brightgreen",
            "repo-badge-json",
            "ShieldsEndpointBadge",
        ]:
            with self.subTest(displaced_semantic=displaced_semantic):
                self.assertNotIn(displaced_semantic, source)

    def test_badge_owner_guide_names_only_the_python_generator(self):
        source = BADGE_README.read_text(encoding="utf-8")
        self.assertIn("python3 scripts/generate-badges.py", source)
        self.assertIn("python3 scripts/generate-badges.py --check", source)
        self.assertNotIn("cargo xtask badges", source)

    def test_exact_source_manual_proof_is_read_only_and_writer_separated(self):
        validate_workflow_contract(WORKFLOW.read_text(encoding="utf-8"))

    def test_wrong_or_unbound_source_and_manual_writer_are_rejected(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        mutations = [
            text.replace("inputs.source_sha == github.sha", "inputs.source_sha != github.sha", 1),
            text.replace("ref: ${{ github.sha }}", "ref: ${{ github.ref }}", 1),
            text.replace(
                "github.event_name == 'push' &&\n      (github.ref == 'refs/heads/main'",
                "github.event_name == 'workflow_dispatch' &&\n      (github.ref == 'refs/heads/main'",
                1,
            ),
        ]
        for mutation in mutations:
            with self.subTest():
                with self.assertRaises(ValueError):
                    validate_workflow_contract(mutation)

    def make_fixture(self, directory: str):
        root = Path(directory).resolve()
        (root / "badges").mkdir()
        (root / "badges/ripr-plus.json").write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "label": "ripr+",
                    "message": "0",
                    "color": "brightgreen",
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        local_script = root / "scripts/generate-badges.py"
        local_script.parent.mkdir()
        local_script.write_bytes(SCRIPT.read_bytes())
        fake_source = root / "fake-ripr.py"
        fake_source.write_text(FAKE_RIPR, encoding="utf-8")
        fake_source.chmod(0o755)
        if os.name == "nt":
            fake = root / "ripr.cmd"
            fake.write_text(
                f'@"{sys.executable}" "{fake_source}" %*\n', encoding="utf-8"
            )
        else:
            fake = root / "ripr"
            fake.write_text(FAKE_RIPR, encoding="utf-8")
            fake.chmod(0o755)
        return root, local_script, fake, fake_source

    def fake_env(self, root: Path, fake: Path, payload: object) -> dict[str, str]:
        return {
            **os.environ,
            "RIPR_BIN": str(fake),
            "FAKE_RIPR_EXPECTED_ROOT": str(root),
            "FAKE_RIPR_RECORD": str(root / "ripr-invocation.json"),
            "FAKE_RIPR_PAYLOAD": json.dumps(payload),
            "FAKE_RIPR_CHILD_PID": str(root / "ripr-child.pid"),
        }

    def run_generator(self, payload, *, check=False, exit_code=0, stderr=""):
        with tempfile.TemporaryDirectory() as directory:
            root, local_script, fake, _ = self.make_fixture(directory)
            env = self.fake_env(root, fake, payload)
            env["FAKE_RIPR_EXIT"] = str(exit_code)
            env["FAKE_RIPR_STDERR"] = stderr
            command = [sys.executable, str(local_script)] + (["--check"] if check else [])
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            output = (root / "badges/ripr-plus.json").read_text(encoding="utf-8")
            invocation = json.loads((root / "ripr-invocation.json").read_text(encoding="utf-8"))
            self.assertEqual(
                invocation,
                {
                    "argv": [
                        "check",
                        "--root",
                        str(root),
                        "--format",
                        "repo-badge-json",
                    ],
                    "cwd": str(root),
                },
            )
            return result, output

    def test_nonzero_counts_are_yellow(self):
        result, output = self.run_generator(
            {
                "counts": {
                    "unsuppressed_exposure_gaps": 3,
                    "unsuppressed_test_efficiency_findings": 2,
                }
            }
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        badge = json.loads(output)
        self.assertEqual((badge["message"], badge["color"]), ("5", "yellow"))

    def test_zero_counts_are_brightgreen(self):
        result, output = self.run_generator({"counts": VALID_COUNTS})
        self.assertEqual(result.returncode, 0, result.stderr)
        badge = json.loads(output)
        self.assertEqual((badge["message"], badge["color"]), ("0", "brightgreen"))

    def test_malformed_or_incomplete_counts_fail_closed(self):
        invalid_payloads = [
            [],
            {},
            {"counts": []},
            {"counts": {}},
            {"counts": {"unexpected": 7}},
            {"counts": {**VALID_COUNTS, "unsuppressed_exposure_gaps": True}},
            {"counts": {**VALID_COUNTS, "unsuppressed_exposure_gaps": -1}},
            {"counts": {**VALID_COUNTS, "unsuppressed_test_efficiency_findings": 1.5}},
        ]
        for payload in invalid_payloads:
            with self.subTest(payload=payload):
                self.assertNotEqual(self.run_generator(payload)[0].returncode, 0)

    def test_process_failure_has_bounded_stderr(self):
        result, _ = self.run_generator(
            {"counts": VALID_COUNTS}, exit_code=7, stderr="x" * 10_000
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("[truncated]", result.stderr)
        self.assertLess(len(result.stderr), generator.STDERR_DIAGNOSTIC_LIMIT + 300)

    def test_check_detects_drift(self):
        self.assertNotEqual(
            self.run_generator(
                {
                    "counts": {
                        "unsuppressed_exposure_gaps": 1,
                        "unsuppressed_test_efficiency_findings": 0,
                    }
                },
                check=True,
            )[0].returncode,
            0,
        )

    def test_fake_rejects_command_and_scope_drift(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, fake, fake_source = self.make_fixture(directory)
            env = self.fake_env(root, fake, {"counts": VALID_COUNTS})
            valid = ["check", "--root", str(root), "--format", "repo-badge-json"]
            cases = [
                (valid[1:], root),
                (["scan", *valid[1:]], root),
                (["check", "--root", str(root / "subdir"), *valid[3:]], root),
                ([*valid[:-1], "json"], root),
                (valid, root / "scripts"),
            ]
            for argv, cwd in cases:
                with self.subTest(argv=argv, cwd=cwd):
                    cwd.mkdir(parents=True, exist_ok=True)
                    result = subprocess.run(
                        [sys.executable, str(fake_source), *argv],
                        cwd=cwd,
                        env=env,
                        capture_output=True,
                        text=True,
                    )
                    self.assertNotEqual(result.returncode, 0)

    def test_timeout_terminates_fake_ripr_process_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, fake, _ = self.make_fixture(directory)
            env = self.fake_env(root, fake, {"counts": VALID_COUNTS})
            env["FAKE_RIPR_SPAWN_CHILD"] = "1"
            env["FAKE_RIPR_HANG"] = "1"
            previous = os.environ.copy()
            os.environ.update(env)
            try:
                with self.assertRaisesRegex(RuntimeError, "process tree was terminated"):
                    generator.generate(root, check=False, ripr_timeout_seconds=0.5)
            finally:
                os.environ.clear()
                os.environ.update(previous)
            self.assert_fake_child_stopped(root, "timed-out")

    def assert_fake_child_stopped(self, root: Path, reason: str):
        child_pid = int((root / "ripr-child.pid").read_text(encoding="utf-8"))
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            try:
                os.kill(child_pid, 0)
            except OSError:
                return
            time.sleep(0.05)
        self.fail(f"{reason} fake RIPR child {child_pid} remained alive")

    def assert_output_overflow_terminates_tree(self, stream_name: str):
        with tempfile.TemporaryDirectory() as directory:
            root, _, fake, _ = self.make_fixture(directory)
            env = self.fake_env(root, fake, {"counts": VALID_COUNTS})
            env["FAKE_RIPR_SPAWN_CHILD"] = "1"
            env["FAKE_RIPR_AFTER_OUTPUT_HANG"] = "1"
            limit = getattr(generator, f"PRODUCER_{stream_name.upper()}_LIMIT")
            env[f"FAKE_RIPR_{stream_name.upper()}_BYTES"] = str(limit * 4)
            previous = os.environ.copy()
            os.environ.update(env)
            try:
                with self.assertRaisesRegex(
                    generator.RiprOutputLimitExceeded,
                    rf"{stream_name} exceeded {limit} bytes",
                ) as raised:
                    generator.generate(root, check=False, ripr_timeout_seconds=15)
            finally:
                os.environ.clear()
                os.environ.update(previous)
            self.assertLessEqual(
                raised.exception.retained_stdout_bytes,
                generator.PRODUCER_STDOUT_LIMIT,
            )
            self.assertLessEqual(
                raised.exception.retained_stderr_bytes,
                generator.PRODUCER_STDERR_LIMIT,
            )
            self.assert_fake_child_stopped(root, f"oversized {stream_name}")

    def test_oversized_stdout_is_bounded_and_terminates_process_tree(self):
        self.assert_output_overflow_terminates_tree("stdout")

    def test_oversized_stderr_is_bounded_and_terminates_process_tree(self):
        self.assert_output_overflow_terminates_tree("stderr")

    def assert_prompt_exit_overflow_is_rejected(self, stream_name: str):
        with tempfile.TemporaryDirectory() as directory:
            root, _, fake, _ = self.make_fixture(directory)
            payload = json.dumps({"counts": VALID_COUNTS})
            env = self.fake_env(root, fake, {"counts": VALID_COUNTS})
            limit = getattr(generator, f"PRODUCER_{stream_name.upper()}_LIMIT")
            if stream_name == "stdout":
                env["FAKE_RIPR_TRAILING_STDOUT_BYTES"] = str(
                    limit + 1 - len((payload + "\n").encode("utf-8"))
                )
            else:
                env["FAKE_RIPR_STDERR_BYTES"] = str(limit + 1)
            previous = os.environ.copy()
            os.environ.update(env)
            try:
                with self.assertRaisesRegex(
                    generator.RiprOutputLimitExceeded,
                    rf"{stream_name} exceeded {limit} bytes",
                ) as raised:
                    generator.generate(root, check=False, ripr_timeout_seconds=15)
            finally:
                os.environ.clear()
                os.environ.update(previous)
            self.assertLessEqual(
                raised.exception.retained_stdout_bytes,
                generator.PRODUCER_STDOUT_LIMIT,
            )
            self.assertLessEqual(
                raised.exception.retained_stderr_bytes,
                generator.PRODUCER_STDERR_LIMIT,
            )

    def test_prompt_exit_oversized_stdout_is_rejected(self):
        self.assert_prompt_exit_overflow_is_rejected("stdout")

    def test_prompt_exit_oversized_stderr_is_rejected(self):
        self.assert_prompt_exit_overflow_is_rejected("stderr")

    @unittest.skipUnless(os.name == "nt", "Windows Job Object behavior")
    def test_windows_dead_leader_live_child_is_terminated(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, fake, _ = self.make_fixture(directory)
            env = self.fake_env(root, fake, {"counts": VALID_COUNTS})
            env["FAKE_RIPR_SPAWN_CHILD"] = "1"
            env["FAKE_RIPR_EXIT_AFTER_CHILD"] = "1"
            previous = os.environ.copy()
            os.environ.update(env)
            try:
                with self.assertRaisesRegex(RuntimeError, "process tree was terminated"):
                    generator.generate(root, check=False, ripr_timeout_seconds=3)
            finally:
                os.environ.clear()
                os.environ.update(previous)
            self.assert_fake_child_stopped(root, "dead-leader")

    def test_cleanup_failure_is_preserved_through_generate_on_overflow(self):
        with tempfile.TemporaryDirectory() as directory:
            root, _, fake, _ = self.make_fixture(directory)
            env = self.fake_env(root, fake, {"counts": VALID_COUNTS})
            env["FAKE_RIPR_AFTER_OUTPUT_HANG"] = "1"
            env["FAKE_RIPR_STDOUT_BYTES"] = str(generator.PRODUCER_STDOUT_LIMIT * 4)
            previous = os.environ.copy()
            os.environ.update(env)
            original_cleanup = generator.terminate_process_tree

            def cleanup_with_failure(process, **kwargs):
                original_cleanup(process, **kwargs)
                return ["simulated cleanup failure"]

            try:
                with mock.patch.object(
                    generator,
                    "terminate_process_tree",
                    side_effect=cleanup_with_failure,
                ):
                    with self.assertRaisesRegex(
                        generator.RiprOutputLimitExceeded,
                        "cleanup incomplete: simulated cleanup failure",
                    ):
                        generator.generate(root, check=False, ripr_timeout_seconds=15)
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_windows_taskkill_timeout_is_reported_without_masking_trigger(self):
        class FakeProcess:
            pid = 123

            def __init__(self):
                self.killed = False

            def poll(self):
                return None

            def kill(self):
                self.killed = True

            def wait(self, timeout):
                return 1

        process = FakeProcess()
        with mock.patch.object(
            generator.subprocess,
            "run",
            side_effect=subprocess.TimeoutExpired("taskkill", 1),
        ) as taskkill:
            failures = generator.terminate_process_tree(process, windows=True)
        taskkill.assert_called_once_with(
            ["taskkill", "/PID", "123", "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=generator.TERMINATION_GRACE_SECONDS,
            check=False,
        )
        self.assertTrue(process.killed)
        self.assertIn("taskkill timed out", failures)

    def test_final_process_wait_timeout_is_reported_and_retried(self):
        class FakeProcess:
            pid = 456

            def __init__(self):
                self.killed = False
                self.waits = 0

            def poll(self):
                return None

            def kill(self):
                self.killed = True

            def wait(self, timeout):
                self.waits += 1
                if self.waits == 1:
                    raise subprocess.TimeoutExpired("ripr", timeout)
                return 1

        process = FakeProcess()
        completed = subprocess.CompletedProcess(["taskkill"], 0)
        with mock.patch.object(generator.subprocess, "run", return_value=completed):
            failures = generator.terminate_process_tree(process, windows=True)
        self.assertTrue(process.killed)
        self.assertEqual(process.waits, 2)
        self.assertIn("direct process wait timed out", failures)

    def test_reader_pipes_close_only_after_all_readers_are_terminal(self):
        class FakeReader:
            def __init__(self, name: str, alive: bool):
                self.name = name
                self.alive = alive

            def join(self, timeout):
                return None

            def is_alive(self):
                return self.alive

        class FakeStream:
            def __init__(self):
                self.closed = False

            def close(self):
                self.closed = True

        blocked_streams = [FakeStream(), FakeStream()]
        failure = generator.finish_readers(
            [FakeReader("blocked", True)], blocked_streams
        )
        self.assertIn("blocked", failure)
        self.assertTrue(all(not stream.closed for stream in blocked_streams))

        terminal_streams = [FakeStream(), FakeStream()]
        self.assertIsNone(
            generator.finish_readers([FakeReader("done", False)], terminal_streams)
        )
        self.assertTrue(all(stream.closed for stream in terminal_streams))


if __name__ == "__main__":
    unittest.main()
