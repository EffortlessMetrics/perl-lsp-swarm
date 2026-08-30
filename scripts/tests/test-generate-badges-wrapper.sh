#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The toolchain guard (#12593) probes `cargo --version` before delegating;
# fake cargo stubs answer with the workspace-required version and do not log it.
FAKE_CARGO_VERSION="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${REPO_ROOT}/Cargo.toml")"
export FAKE_CARGO_VERSION
GENERATE_BADGES_SCRIPT="${REPO_ROOT}/scripts/generate-badges.sh"

PASS=0
FAIL=0
TMPDIR_BASE=""

cleanup() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
}
trap cleanup EXIT

pass() {
  printf 'PASS %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf 'FAIL %s\n' "$1"
  FAIL=$((FAIL + 1))
}

assert_exit_zero() {
  local label="$1"
  local code="$2"
  if [[ "$code" -eq 0 ]]; then
    pass "$label"
  else
    fail "$label (expected exit 0, got ${code})"
  fi
}

assert_exit_nonzero() {
  local label="$1"
  local code="$2"
  if [[ "$code" -ne 0 ]]; then
    pass "$label (exit ${code} as expected)"
  else
    fail "$label (expected non-zero exit, got 0)"
  fi
}

write_fake_cargo() {
  local fake_bin="$1"
  local log_path="$2"

  mkdir -p "$fake_bin"
  cat > "${fake_bin}/cargo" <<FAKE
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then printf 'cargo %s (stub)\n' "\${FAKE_CARGO_VERSION:-1.95.0}"; exit 0; fi
printf '%s\n' "\$@" > "${log_path}"
exit "\${FAKE_CARGO_EXIT:-0}"
FAKE
  chmod +x "${fake_bin}/cargo"
}

assert_args_equal() {
  local label="$1"
  local expected="$2"
  local actual="$3"

  if cmp -s "$expected" "$actual"; then
    pass "$label"
  else
    fail "$label"
    printf 'expected:\n'
    cat "$expected"
    printf 'actual:\n'
    cat "$actual"
  fi
}

echo "=== generate-badges wrapper test suite ==="
echo ""

if [[ ! -f "$GENERATE_BADGES_SCRIPT" ]]; then
  echo "ERROR: generate-badges.sh not found at ${GENERATE_BADGES_SCRIPT}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
FAKE_LOG="${TMPDIR_BASE}/cargo-args.txt"
write_fake_cargo "$FAKE_BIN" "$FAKE_LOG"

PASS_DIR="${TMPDIR_BASE}/pass"
mkdir -p "$PASS_DIR"
EXPECTED_PASS_ARGS="${PASS_DIR}/expected-args.txt"
cat > "$EXPECTED_PASS_ARGS" <<'ARGS'
xtask
ci-hygiene
generate-badges
--check
ARGS

code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" bash "$GENERATE_BADGES_SCRIPT" --check
) > "${PASS_DIR}/out.txt" 2> "${PASS_DIR}/err.txt" || code=$?
assert_exit_zero "delegates to cargo xtask ci-hygiene generate-badges" "$code"
assert_args_equal "forwards badge generator arguments unchanged" "$EXPECTED_PASS_ARGS" "$FAKE_LOG"

FAIL_DIR="${TMPDIR_BASE}/fail"
mkdir -p "$FAIL_DIR"
code=0
(
  cd "$REPO_ROOT"
  PATH="${FAKE_BIN}:$PATH" FAKE_CARGO_EXIT=37 bash "$GENERATE_BADGES_SCRIPT" --check
) > "${FAIL_DIR}/out.txt" 2> "${FAIL_DIR}/err.txt" || code=$?
assert_exit_nonzero "propagates cargo failure from delegated command" "$code"

DIRECT_DIR="${TMPDIR_BASE}/direct"
mkdir -p "$DIRECT_DIR"
direct_code=0
(
  cd "$REPO_ROOT"
  python3 - "$REPO_ROOT" <<'PY'
from __future__ import annotations

import importlib.util
import io
import json
from pathlib import Path, PosixPath
import sys
import tempfile
import unittest
from unittest import mock

root = Path(sys.argv[1]).resolve()
sys.argv = [sys.argv[0]]
script = root / "scripts/generate-badges.py"
spec = importlib.util.spec_from_file_location("ripr_badge_generator_direct_proof", script)
if spec is None or spec.loader is None:
    raise RuntimeError("could not load scripts/generate-badges.py")
generator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generator)

COUNTS = {
    "unsuppressed_exposure_gaps": 0,
    "unsuppressed_test_efficiency_findings": 0,
}


class TerminalProcess:
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


class DirectRiprContainmentProof(unittest.TestCase):
    def run_terminal(self, stdout: bytes, stderr: bytes = b"", returncode: int = 0):
        process = TerminalProcess(io.BytesIO(stdout), io.BytesIO(stderr), returncode)
        terminate = mock.Mock(return_value=[])
        with mock.patch.object(
            generator.subprocess, "Popen", return_value=process
        ), mock.patch.object(
            generator, "terminate_process_tree", terminate
        ):
            generator.run_ripr(root, timeout_seconds=1)
        return terminate

    def assert_process_tree_terminated(self, terminate) -> None:
        terminate.assert_called_once()
        _, kwargs = terminate.call_args
        self.assertIsNone(kwargs.get("windows_job"))

    def test_prompt_exit_oversized_stdout_is_rejected_at_the_cap(self):
        with self.assertRaises(generator.RiprOutputLimitExceeded) as raised:
            terminate = self.run_terminal(b"o" * (generator.PRODUCER_STDOUT_LIMIT + 1))
        self.assertEqual(raised.exception.stream_name, "stdout")
        self.assertEqual(
            raised.exception.retained_stdout_bytes,
            generator.PRODUCER_STDOUT_LIMIT,
        )
        self.assert_process_tree_terminated(terminate)

    def test_prompt_exit_oversized_stderr_is_rejected_at_the_cap(self):
        payload = json.dumps({"counts": COUNTS}).encode() + b"\n"
        with self.assertRaises(generator.RiprOutputLimitExceeded) as raised:
            terminate = self.run_terminal(
                payload,
                b"e" * (generator.PRODUCER_STDERR_LIMIT + 1),
            )
        self.assertEqual(raised.exception.stream_name, "stderr")
        self.assertEqual(
            raised.exception.retained_stderr_bytes,
            generator.PRODUCER_STDERR_LIMIT,
        )
        self.assert_process_tree_terminated(terminate)

    def test_pipe_read_failure_rejects_otherwise_valid_output(self):
        payload = json.dumps({"counts": COUNTS}).encode() + b"\n"
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
                generator.run_ripr(root, timeout_seconds=1)
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
                generator.run_ripr(root, timeout_seconds=1)
        self.assert_process_tree_terminated(terminate)

    def test_failed_windows_launch_closes_the_job(self):
        job = FakeWindowsJob()
        with mock.patch.object(generator.os, "name", "nt"), mock.patch.object(
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
                generator.run_ripr(root, timeout_seconds=1)
        self.assertTrue(job.closed)

    def test_exact_receipt_mode_never_launches_direct_ripr(self):
        source_sha = "a" * 40
        receipt = {
            "schema_version": 2,
            "kind": "ripr_plus_baseline",
            "head": source_sha,
            "root": ".",
            "source_format": "ripr check --format repo-badge-json (counts)",
            "counts": COUNTS,
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
                generator.run_ripr(root, timeout_seconds=1)
        self.assertTrue(process.killed)
        self.assertTrue(job.terminated)


if __name__ == "__main__":
    unittest.main(verbosity=2)
PY
) > "${DIRECT_DIR}/out.txt" 2> "${DIRECT_DIR}/err.txt" || direct_code=$?
if [[ "$direct_code" -ne 0 ]]; then
  cat "${DIRECT_DIR}/out.txt"
  cat "${DIRECT_DIR}/err.txt" >&2
fi
assert_exit_zero "bounds direct Python RIPR capture and preserves exact-receipt mode" "$direct_code"

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
