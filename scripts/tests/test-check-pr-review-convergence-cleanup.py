#!/usr/bin/env python3
"""Offline exit/cleanup controls for the real convergence shell self-test."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch


SUBJECT = Path(__file__).with_name("test-check-pr-review-convergence.sh").resolve()


class ConvergenceCleanup(unittest.TestCase):
    def check_exit(self, fail_copy_at, cleanup_fails=False, missing_jq=False):
        executables = {}
        for name in ("cp", "mktemp", "rm"):
            executable = shutil.which(name)
            self.assertIsNotNone(executable, f"ERROR: missing required dependency: {name}")
            executables[name] = executable
        with tempfile.TemporaryDirectory(prefix="convergence-cleanup-") as outer:
            root = Path(outer)
            temporary = root / "temporary parent with spaces"
            temporary.mkdir()
            sentinel = temporary / "unrelated-sentinel"
            sentinel.write_text("preserve unrelated files\n", encoding="utf-8")
            shim = root / "bin"
            shim.mkdir()
            counter = root / "copy-count"
            mktemp_counter = root / "mktemp-calls"
            suite_temp = root / "suite-temp"
            (shim / "cp").write_text(
                '#!/usr/bin/env bash\n'
                'count=0\n'
                '[[ ! -f "$COPY_COUNTER" ]] || read -r count < "$COPY_COUNTER"\n'
                'count=$((count + 1))\n'
                'printf "%s\\n" "$count" > "$COPY_COUNTER"\n'
                'if [[ "$count" -eq "$FAIL_COPY_AT" ]]; then\n'
                '  echo injected-copy-failure >&2\n'
                '  exit 73\n'
                'fi\n'
                'exec "$REAL_CP" "$@"\n',
                encoding="utf-8",
            )
            (shim / "mktemp").write_text(
                '#!/usr/bin/env bash\n'
                'echo call >> "$MKTEMP_COUNTER"\n'
                'created=$("$REAL_MKTEMP" "$@") || exit $?\n'
                '[[ -f "$SUITE_TEMP" ]] || printf "%s\\n" "$created" > "$SUITE_TEMP"\n'
                'printf "%s\\n" "$created"\n',
                encoding="utf-8",
            )
            if cleanup_fails:
                (shim / "rm").write_text(
                    '#!/usr/bin/env bash\n'
                    'if [[ "${1:-}" == -rf && -f "$SUITE_TEMP" ]] && '
                    '[[ "${2:-}" == "$(<"$SUITE_TEMP")" ]]; then\n'
                    '  echo injected-cleanup-failure >&2\n'
                    '  exit 74\n'
                    'fi\n'
                    'exec "$REAL_RM" "$@"\n',
                    encoding="utf-8",
                )
            for path in shim.iterdir():
                path.chmod(0o755)
            env = dict(
                os.environ,
                TMPDIR=str(temporary),
                PATH=str(shim) + os.pathsep + os.environ["PATH"],
                COPY_COUNTER=str(counter),
                FAIL_COPY_AT=str(fail_copy_at),
                REAL_CP=executables["cp"],
                MKTEMP_COUNTER=str(mktemp_counter),
                REAL_MKTEMP=executables["mktemp"],
                REAL_RM=executables["rm"],
                SUITE_TEMP=str(suite_temp),
            )
            start = time.monotonic()
            result = subprocess.run(
                ["bash", str(SUBJECT)], env=env, capture_output=True,
                text=True, timeout=30,
            )
            expected_exit = 73 if fail_copy_at else (74 if cleanup_fails else 0)
            if missing_jq:
                expected_exit = 1
            self.assertEqual(expected_exit, result.returncode,
                             result.stdout + result.stderr)
            remaining = [path for path in temporary.iterdir() if path != sentinel]
            print(json.dumps({
                "fail_copy_at": fail_copy_at,
                "exit": result.returncode,
                "wall_seconds": round(time.monotonic() - start, 3),
                "mktemp_calls": (len(mktemp_counter.read_text().splitlines())
                                 if mktemp_counter.is_file() else None),
                "remaining_roots": len(remaining),
                "remaining_files": sum(path.is_file() for entry in remaining
                                       for path in entry.rglob("*")),
                "remaining_bytes": sum(path.stat().st_size for entry in remaining
                                       for path in entry.rglob("*") if path.is_file()),
                "summary": result.stdout.splitlines()[-1:],
            }), flush=True)
            if missing_jq:
                self.assertIn("ERROR: jq required", result.stdout + result.stderr)
            elif fail_copy_at:
                self.assertIn("injected-copy-failure", result.stderr)
                self.assertEqual(str(fail_copy_at), counter.read_text().strip())
            else:
                self.assertIn("=== Results: 29 passed, 0 failed ===", result.stdout)
            self.assertEqual("preserve unrelated files\n", sentinel.read_text())
            if cleanup_fails:
                self.assertIn("injected-cleanup-failure", result.stderr)
                self.assertEqual([Path(suite_temp.read_text().strip())], remaining,
                                 "cleanup refusal must retain only the owned root")
            else:
                self.assertEqual([], remaining, "suite leaked its temporary fixtures")

    def test_normal_exit(self):
        self.check_exit(0)

    def test_first_fixture_copy_failure(self):
        self.check_exit(1)

    def test_late_fixture_copy_failure(self):
        self.check_exit(4)

    def test_missing_jq_reports_child_failure(self):
        missing_jq = ('() { if [[ "$*" == "-v jq" ]]; then return 1; fi; '
                      'builtin command "$@"; }')
        with patch.dict(os.environ, {"BASH_FUNC_command%%": missing_jq}):
            self.check_exit(0, missing_jq=True)

    def test_cleanup_failure_preserves_copy_failure(self):
        self.check_exit(1, cleanup_fails=True)

    def test_cleanup_failure_after_success_fails(self):
        self.check_exit(0, cleanup_fails=True)

    def test_missing_required_dependencies_report_name(self):
        which = shutil.which
        for missing in ("cp", "mktemp", "rm"):
            with self.subTest(dependency=missing):
                with patch.object(shutil, "which", side_effect=lambda name:
                                  None if name == missing else which(name)):
                    with self.assertRaisesRegex(
                        AssertionError, f"missing required dependency: {missing}"
                    ):
                        self.check_exit(0)


if __name__ == "__main__":
    unittest.main()
