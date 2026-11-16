"""Negative controls for the Windows hygiene process-test smoke oracle."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

CHECKER = Path(__file__).with_name("check_hygiene_process_tests.py")


def transcript(name="process::tests::probe", status="ok", passed=1, ignored=0):
    return (
        "running 1 test\n"
        f"test {name} ... {status}\n\n"
        f"test result: ok. {passed} passed; 0 failed; {ignored} ignored; "
        "0 measured; 10 filtered out; finished in 0.01s\n"
    )


class HygieneProcessSelectionTests(unittest.TestCase):
    def check_output(self, output, accepted):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "output.log"
            log.write_text(output, encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(CHECKER), str(log)], capture_output=True, text=True
            )
        self.assertEqual(result.returncode, 0 if accepted else 1, result.stderr)

    def test_intended_pass_is_accepted(self):
        self.check_output(transcript(), True)

    def test_should_panic_pass_is_accepted(self):
        self.check_output(
            transcript().replace(
                "test process::tests::probe ... ok",
                "test process::tests::probe - should panic ... ok",
            ), True
        )

    def test_ignored_with_reason_is_recorded(self):
        self.check_output(
            "running 2 tests\n"
            "test process::tests::probe ... ok\n"
            "test process::tests::skipped ... ignored, needs Windows fixture\n\n"
            "test result: ok. 1 passed; 0 failed; 1 ignored; "
            "0 measured; 10 filtered out; finished in 0.01s\n", True
        )

    def test_windows_newlines_are_accepted(self):
        self.check_output(transcript().replace("\n", "\r\n"), True)

    def test_library_target_or_typo_zero_selection_is_rejected(self):
        self.check_output(
            "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; "
            "0 measured; 139 filtered out; finished in 0.00s\n", False
        )

    def test_unrelated_library_test_is_rejected(self):
        self.check_output(transcript(name="tests::valid_version"), False)

    def test_ignored_only_is_rejected(self):
        self.check_output(transcript(status="ignored", passed=0, ignored=1), False)

    def test_missing_summary_is_rejected(self):
        self.check_output("running 1 test\ntest process::tests::probe ... ok\n", False)

    def test_summary_without_execution_is_rejected(self):
        self.check_output(transcript().replace("test process::tests::probe ... ok\n", ""), False)

    def test_no_run_or_list_output_is_rejected(self):
        for output in ("", "Finished test profile\nExecutable main.rs\n", "process::tests::probe: test\n1 test, 0 benchmarks\n"):
            with self.subTest(output=output):
                self.check_output(output, False)

    def test_failed_test_is_rejected(self):
        self.check_output(transcript(status="FAILED").replace("result: ok", "result: FAILED"), False)

    def test_inconsistent_pass_count_is_rejected(self):
        self.check_output(transcript(passed=2), False)

    def test_duplicate_target_output_is_rejected(self):
        self.check_output(transcript() * 2, False)

    def test_partial_selection_output_is_rejected(self):
        self.check_output(transcript().replace("running 1 test", "running 2 tests"), False)

    def test_nested_fixture_harness_output_is_accepted(self):
        self.check_output(
            "running 2 tests\n"
            "test process::tests::fixture ... ok\n"
            + transcript(name="process::tests::fixture")
            + "running 1 test\n"
            "test process::tests::status_helper ... ok\n"
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; "
            "5 filtered out; finished in 0.01s\n", True
        )

    def test_nested_summary_cannot_complete_missing_outer_tests(self):
        self.check_output("running 54 tests\n" + transcript(), False)

    def test_missing_file_is_rejected(self):
        result = subprocess.run(
            [sys.executable, str(CHECKER), "/nonexistent/hygiene-process.log"],
            capture_output=True, text=True,
        )
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
