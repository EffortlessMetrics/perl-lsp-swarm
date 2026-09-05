#!/usr/bin/env python3
import io
import json
import importlib.util
import os
from pathlib import Path, PosixPath
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

REPO_ROOT = Path(__file__).parents[2]
SCRIPT = Path(__file__).parents[1] / "generate-badges.py"
WORKFLOW = Path(__file__).parents[2] / ".github/workflows/badge-endpoints.yml"
RUST_DELEGATE = Path(__file__).parents[2] / "xtask/src/tasks/badges.rs"
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
if os.environ.get("FAKE_RIPR_HANG") == "1":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(300)"])
    Path(os.environ["FAKE_RIPR_CHILD_PID"]).write_text(str(child.pid), encoding="utf-8")
    time.sleep(300)
print(os.environ["FAKE_RIPR_PAYLOAD"])
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
        "workflow_run",
        "github.event.workflow_run.conclusion == 'success'",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.repository.full_name == github.repository",
        "ref: ${{ env.SOURCE_SHA }}",
        "timeout-minutes: 20",
        "python3 scripts/generate-badges.py",
        "permissions: actions: read contents: read",
        "--ripr-receipt",
        "--producer-receipt",
        "receipts/quality/ripr-plus.json",
        "receipts/quality/ripr-badge-producer.json",
    ]
    for fragment in required:
        if fragment not in compact:
            raise ValueError(f"badge workflow contract is missing {fragment!r}")
    if "cargo xtask badges" in text:
        raise ValueError("the displaced Rust badge mapper remains in workflow guidance")
    writer_required = [
        "github.event_name == 'workflow_run'",
        "github.event.workflow_run.head_branch == github.event.repository.default_branch",
        "github.event.workflow_run.event == 'push'",
        "github.event.workflow_run.repository.full_name == github.repository",
        "contents: write",
        "pull-requests: write",
        'title: "chore(badges): refresh public endpoints (#13694)"',
        'commit-message: "chore(badges): refresh public endpoints"',
        "Source SHA: `${{ env.SOURCE_SHA }}`",
        "RIPR producer run: `${{ github.event.workflow_run.id }}`",
        "Badge payload: `badge-endpoints-${{ github.run_id }}`",
        "Refs #13694.",
    ]
    for fragment in writer_required:
        if fragment not in open_pr:
            raise ValueError(f"badge PR writer contract is missing {fragment!r}")
    if "#8820" in open_pr:
        raise ValueError("badge PR writer retains stale #8820 ownership")
    for closing in (
        "Closes #13694", "Close #13694",
        "Fixes #13694", "Fix #13694",
        "Resolves #13694", "Resolve #13694",
    ):
        if closing.lower() in open_pr.lower():
            raise ValueError("badge PR writer must not close the recovery umbrella")
    if "github.event_name == 'workflow_dispatch'" in open_pr:
        raise ValueError("manual candidate proof must not admit the write-capable PR job")


class TerminalProcess:
    """A Popen stand-in that has already exited with the given streams."""

    pid = 789

    def __init__(self, stdout: io.BytesIO, stderr: io.BytesIO, returncode: int = 0):
        self.stdout = stdout
        self.stderr = stderr
        self.stdin = io.BytesIO()
        self.returncode = returncode
        self.killed = False

    def poll(self):
        return self.returncode

    def kill(self):
        self.killed = True

    def wait(self, timeout):
        return self.returncode


class ReadFailureStream(io.BytesIO):
    def __init__(self, first: bytes, detail: str):
        super().__init__()
        self.first = first
        self.detail = detail
        self.delivered = False

    def read1(self, size=-1):
        if not self.delivered:
            self.delivered = True
            return self.first
        raise OSError(self.detail)


class NonOSErrorReadFailureStream(io.BytesIO):
    def read1(self, size=-1):
        raise ValueError("simulated non-os read crash")


class FakeWindowsJob:
    def __init__(self):
        self.terminated = False
        self.closed = False

    def assign(self, process):
        return None

    def terminate(self):
        self.terminated = True
        return []

    def close(self):
        self.closed = True
        return []


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

    def test_exact_source_manual_proof_is_read_only_and_writer_separated(self):
        validate_workflow_contract(WORKFLOW.read_text(encoding="utf-8"))

    def test_wrong_or_unbound_source_and_manual_writer_are_rejected(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        mutations = [
            text.replace("inputs.source_sha == github.sha", "inputs.source_sha != github.sha", 1),
            text.replace("ref: ${{ env.SOURCE_SHA }}", "ref: ${{ github.ref }}", 1),
            text.replace(
                "github.event.workflow_run.conclusion == 'success'",
                "github.event.workflow_run.conclusion != 'success'",
                1,
            ),
        ]
        for mutation in mutations:
            with self.subTest():
                with self.assertRaises(ValueError):
                    validate_workflow_contract(mutation)

    def test_ownership_metadata_drifts_are_rejected(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        mutations = [
            text.replace("Refs #13694.", "Closes #13694.", 1),
            text.replace(
                "Source SHA: `${{ env.SOURCE_SHA }}`",
                "Source SHA: `unknown`",
                1,
            ),
            text.replace(
                "RIPR producer run: `${{ github.event.workflow_run.id }}`",
                "RIPR producer run: `hardcoded`",
                1,
            ),
            text.replace(
                "Badge payload: `badge-endpoints-${{ github.run_id }}`",
                "Badge payload: `stale-artifact-name`",
                1,
            ),
            text.replace(
                'title: "chore(badges): refresh public endpoints (#13694)"',
                'title: "chore(badges): refresh public endpoints (#8820)"',
                1,
            ),
            text.replace(
                'title: "chore(badges): refresh public endpoints (#13694)"',
                'title: "chore(badges): refresh public endpoints"',
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

    def test_exact_receipt_reuse_is_bound_to_source_and_reviewed_producer(self):
        source_sha = "a" * 40
        receipt = {
            "schema_version": 2,
            "kind": "ripr_plus_baseline",
            "head": source_sha,
            "root": ".",
            "source_format": "ripr check --format repo-badge-json (counts)",
            "counts": VALID_COUNTS,
        }
        producer = {
            "schema_version": 1,
            "kind": "ripr_badge_producer",
            "head": source_sha,
            "root": ".",
            "source_format": "ripr-plus repo-badge-json",
            "ripr_version": generator.EXPECTED_RIPR_VERSION,
        }
        with tempfile.TemporaryDirectory() as directory:
            receipt_path = Path(directory) / "ripr-plus.json"
            producer_path = Path(directory) / "producer.json"
            receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
            producer_path.write_text(json.dumps(producer), encoding="utf-8")
            badge = generator.badge_from_receipt(receipt_path, producer_path, source_sha)
            self.assertEqual((badge["message"], badge["color"]), ("0", "brightgreen"))

            for mutation in (
                {**receipt, "head": "b" * 40},
                {**producer, "ripr_version": "0.0.0"},
                {**receipt, "counts": {"unsuppressed_exposure_gaps": True}},
            ):
                with self.subTest(mutation=mutation):
                    receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
                    producer_path.write_text(json.dumps(producer), encoding="utf-8")
                    if "kind" in mutation and mutation["kind"] == "ripr_badge_producer":
                        producer_path.write_text(json.dumps(mutation), encoding="utf-8")
                    else:
                        receipt_path.write_text(json.dumps(mutation), encoding="utf-8")
                    with self.assertRaises(ValueError):
                        generator.badge_from_receipt(receipt_path, producer_path, source_sha)

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
            env["FAKE_RIPR_HANG"] = "1"
            previous = os.environ.copy()
            os.environ.update(env)
            try:
                with self.assertRaisesRegex(RuntimeError, "process tree was terminated"):
                    generator.generate(root, check=False, ripr_timeout_seconds=0.5)
            finally:
                os.environ.clear()
                os.environ.update(previous)
            child_pid = int((root / "ripr-child.pid").read_text(encoding="utf-8"))
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                try:
                    os.kill(child_pid, 0)
                except OSError:
                    break
                time.sleep(0.05)
            else:
                self.fail(f"timed-out fake RIPR child {child_pid} remained alive")


class DirectRiprContainmentProof(unittest.TestCase):
    """Containment proof for the direct RIPR capture inside generate-badges.py.

    This suite owns the bounded-capture, reader-failure, and Windows
    job-lifecycle regressions from #14030. It lives beside the generator it
    exercises so that a change to `scripts/generate-badges.py` selects it:
    while it lived in the `generate-badges-wrapper` shell pack (#14184), a
    generator-only edit selected `ripr-badge-endpoints` and never ran it.
    """

    def assert_process_tree_terminated(self, terminate) -> None:
        terminate.assert_called_once()
        _, kwargs = terminate.call_args
        self.assertIsNone(kwargs.get("windows_job"))

    def test_prompt_exit_oversized_stdout_is_rejected_at_the_cap(self):
        process = TerminalProcess(
            io.BytesIO(b"o" * (generator.PRODUCER_STDOUT_LIMIT + 1)),
            io.BytesIO(),
        )
        terminate = mock.Mock(return_value=[])
        with mock.patch.object(
            generator.subprocess, "Popen", return_value=process
        ), mock.patch.object(
            generator, "terminate_process_tree", terminate
        ):
            with self.assertRaises(generator.RiprOutputLimitExceeded) as raised:
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assertEqual(raised.exception.stream_name, "stdout")
        self.assertEqual(
            raised.exception.retained_stdout_bytes,
            generator.PRODUCER_STDOUT_LIMIT,
        )
        self.assert_process_tree_terminated(terminate)

    def test_prompt_exit_oversized_stderr_is_rejected_at_the_cap(self):
        payload = json.dumps({"counts": VALID_COUNTS}).encode() + b"\n"
        process = TerminalProcess(
            io.BytesIO(payload),
            io.BytesIO(b"e" * (generator.PRODUCER_STDERR_LIMIT + 1)),
        )
        terminate = mock.Mock(return_value=[])
        with mock.patch.object(
            generator.subprocess, "Popen", return_value=process
        ), mock.patch.object(
            generator, "terminate_process_tree", terminate
        ):
            with self.assertRaises(generator.RiprOutputLimitExceeded) as raised:
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assertEqual(raised.exception.stream_name, "stderr")
        self.assertEqual(
            raised.exception.retained_stderr_bytes,
            generator.PRODUCER_STDERR_LIMIT,
        )
        self.assert_process_tree_terminated(terminate)

    def test_pipe_read_failure_rejects_otherwise_valid_output(self):
        payload = json.dumps({"counts": VALID_COUNTS}).encode() + b"\n"
        process = TerminalProcess(
            ReadFailureStream(payload, "simulated stdout read failure"),
            io.BytesIO(),
        )
        terminate = mock.Mock(return_value=[])
        with mock.patch.object(
            generator.subprocess, "Popen", return_value=process
        ), mock.patch.object(
            generator, "terminate_process_tree", terminate
        ):
            with self.assertRaisesRegex(
                RuntimeError,
                "stdout read failed: simulated stdout read failure",
            ):
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assert_process_tree_terminated(terminate)

    def test_non_oserror_reader_failure_still_fails_closed(self):
        process = TerminalProcess(
            NonOSErrorReadFailureStream(),
            io.BytesIO(),
        )
        terminate = mock.Mock(return_value=[])
        with mock.patch.object(
            generator.subprocess, "Popen", return_value=process
        ), mock.patch.object(
            generator, "terminate_process_tree", terminate
        ):
            with self.assertRaisesRegex(
                RuntimeError,
                "stdout read failed: simulated non-os read crash",
            ):
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assert_process_tree_terminated(terminate)

    def test_failed_windows_launch_closes_the_job(self):
        job = FakeWindowsJob()
        # Resolve the launcher path before flipping os.name: a POSIX host
        # cannot instantiate WindowsPath, and run_ripr builds the Windows
        # launcher command from Path(__file__).
        launcher = Path(__file__).resolve()
        with mock.patch.object(generator.os, "name", "nt"), mock.patch.object(
            generator, "Path", mock.Mock(return_value=launcher)
        ), mock.patch.object(
            generator, "WindowsJob", mock.Mock(return_value=job)
        ), mock.patch.object(
            generator.subprocess,
            "Popen",
            side_effect=OSError("launch refused"),
        ):
            with self.assertRaisesRegex(
                RuntimeError,
                "could not launch ripr badge producer: launch refused",
            ):
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assertTrue(job.closed)

    def test_windows_launch_interrupt_propagates_and_still_closes_the_job(self):
        job = FakeWindowsJob()
        launcher = Path(__file__).resolve()
        with mock.patch.object(generator.os, "name", "nt"), mock.patch.object(
            generator, "Path", mock.Mock(return_value=launcher)
        ), mock.patch.object(
            generator, "WindowsJob", mock.Mock(return_value=job)
        ), mock.patch.object(
            generator.subprocess,
            "Popen",
            side_effect=KeyboardInterrupt,
        ):
            with self.assertRaises(KeyboardInterrupt):
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assertTrue(job.closed)

    def test_exact_receipt_mode_never_launches_direct_ripr(self):
        source_sha = "a" * 40
        receipt = {
            "schema_version": 2,
            "kind": "ripr_plus_baseline",
            "head": source_sha,
            "root": ".",
            "source_format": "ripr check --format repo-badge-json (counts)",
            "counts": VALID_COUNTS,
        }
        producer = {
            "schema_version": 1,
            "kind": "ripr_badge_producer",
            "head": source_sha,
            "root": ".",
            "source_format": "ripr-plus repo-badge-json",
            "ripr_version": generator.EXPECTED_RIPR_VERSION,
        }
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            (fixture / "badges").mkdir()
            receipt_path = fixture / "ripr-plus.json"
            producer_path = fixture / "producer.json"
            receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
            producer_path.write_text(json.dumps(producer), encoding="utf-8")
            with mock.patch.object(
                generator,
                "run_ripr",
                side_effect=AssertionError("direct RIPR must not run"),
            ):
                generator.generate(
                    fixture,
                    check=False,
                    receipt_path=receipt_path,
                    producer_path=producer_path,
                    source_sha=source_sha,
                )
            badge = json.loads(
                (fixture / "badges/ripr-plus.json").read_text(encoding="utf-8")
            )
            self.assertEqual((badge["message"], badge["color"]), ("0", "brightgreen"))

    def test_windows_job_assignment_failure_is_fail_closed(self):
        class RejectingJob(FakeWindowsJob):
            def assign(self, process):
                raise OSError("simulated assignment failure")

        process = TerminalProcess(io.BytesIO(), io.BytesIO())
        job = RejectingJob()
        with mock.patch.object(generator.os, "name", "nt"), mock.patch.object(
            generator, "Path", PosixPath
        ), mock.patch.object(
            generator.subprocess, "Popen", return_value=process
        ), mock.patch.object(generator, "WindowsJob", return_value=job):
            with self.assertRaisesRegex(
                RuntimeError,
                "could not establish Windows process-tree ownership",
            ):
                generator.run_ripr(REPO_ROOT, timeout_seconds=1)
        self.assertTrue(process.killed)
        self.assertTrue(job.terminated)


if __name__ == "__main__":
    unittest.main()
