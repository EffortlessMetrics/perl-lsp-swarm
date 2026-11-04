"""Exercise telemetry's real shell functions without running workspace probes."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "forensics" / "telemetry-runner.sh"


def shell_function(source: str, name: str) -> str:
    match = re.search(rf"^{name}\(\) \{{\n.*?^\}}$", source, re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError(f"missing telemetry function {name}")
    return match.group(0)


@unittest.skipUnless(shutil.which("bash"), "requires Bash")
class TelemetryTimeoutReportingTests(unittest.TestCase):
    def run_probe(self, exit_code: int, output: str, *, fallback: bool = False):
        source = SCRIPT.read_text(encoding="utf-8")
        definitions = "\n".join(
            shell_function(source, name) for name in ("run_with_timeout", "run_tests")
        )
        # Define only the selected functions. Sourcing the executable would run
        # repository probes and install its worktree-removal EXIT trap.
        driver = """set -euo pipefail
log_info() { :; }
TIMEOUT_TEST=600
timeout() {
    printf '%s\n' "$@" > "$FIXTURE_ARGS"
    printf '%s' "$FIXTURE_OUTPUT"
    return "$FIXTURE_EXIT"
}
perl() { timeout "$@"; }
"""
        if fallback:
            driver += """command() {
    if [[ "$1" == -v && "$2" == timeout ]]; then return 1; fi
    builtin command "$@"
}
"""
        driver += definitions + '\nrun_tests "$FIXTURE_ROOT"\n'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            arguments = root / "backend-arguments"
            environment = dict(
                os.environ,
                FIXTURE_ROOT=directory,
                FIXTURE_ARGS=str(arguments),
                FIXTURE_EXIT=str(exit_code),
                FIXTURE_OUTPUT=output,
            )
            result = subprocess.run(
                [shutil.which("bash"), "-c", driver],
                env=environment,
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            invoked = arguments.read_text(encoding="utf-8").splitlines()
        command = (
            "RUST_TEST_THREADS=2 cargo test --workspace --lib --no-fail-fast "
            "-- --test-threads=2 2>&1"
        )
        expected = ["-e", "alarm 600; exec @ARGV", "--", "bash", "-c", command]
        self.assertEqual(invoked, expected if fallback else ["600", "bash", "-c", command])
        return result.stdout.strip()

    def test_silent_timeout_is_not_a_passing_test_run(self):
        self.assertEqual(self.run_probe(124, ""), "fail|0|0|0")

    def test_timeout_retains_partial_success_counts_without_passing(self):
        partial = "test result: ok. 3 passed; 0 failed; 2 ignored\n"
        self.assertEqual(self.run_probe(124, partial), "fail|3|0|2")

    def test_perl_alarm_status_is_not_a_passing_test_run(self):
        self.assertEqual(self.run_probe(142, "", fallback=True), "fail|0|0|0")

    def test_ordinary_nonzero_exit_is_not_a_passing_test_run(self):
        self.assertEqual(self.run_probe(101, "compile error\n"), "fail|0|0|0")

    def test_completed_success_preserves_counts(self):
        output = "test result: ok. 3 passed; 0 failed; 2 ignored\n"
        self.assertEqual(self.run_probe(0, output), "pass|3|0|2")

    def test_multi_digit_counts_are_retained_exactly(self):
        output = "test result: ok. 123 passed; 0 failed; 45 ignored\n"
        self.assertEqual(self.run_probe(0, output), "pass|123|0|45")

    def test_multi_digit_failure_count_fails_even_with_zero_exit(self):
        output = "test result: FAILED. 123 passed; 10 failed; 45 ignored\n"
        self.assertEqual(self.run_probe(0, output), "fail|123|10|45")

    def test_timeout_retains_multi_digit_partial_counts(self):
        partial = "test result: ok. 123 passed; 10 failed; 45 ignored\n"
        self.assertEqual(self.run_probe(124, partial), "fail|123|10|45")

    def test_reported_test_failure_still_fails_even_with_zero_exit(self):
        output = "test result: FAILED. 3 passed; 1 failed; 2 ignored\n"
        self.assertEqual(self.run_probe(0, output), "fail|3|1|2")


if __name__ == "__main__":
    unittest.main()
