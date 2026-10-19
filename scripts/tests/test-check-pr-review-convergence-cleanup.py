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


SUBJECT = Path(__file__).with_name("test-check-pr-review-convergence.sh").resolve()


class ConvergenceCleanup(unittest.TestCase):
    def check_exit(self, fail_copy_at):
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
                'exec "$REAL_MKTEMP" "$@"\n',
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
                REAL_CP=shutil.which("cp"),
                MKTEMP_COUNTER=str(mktemp_counter),
                REAL_MKTEMP=shutil.which("mktemp"),
            )
            start = time.monotonic()
            result = subprocess.run(
                ["bash", str(SUBJECT)], env=env, capture_output=True,
                text=True, timeout=30,
            )
            remaining = [path for path in temporary.iterdir() if path != sentinel]
            print(json.dumps({
                "fail_copy_at": fail_copy_at,
                "exit": result.returncode,
                "wall_seconds": round(time.monotonic() - start, 3),
                "mktemp_calls": len(mktemp_counter.read_text().splitlines()),
                "remaining_roots": len(remaining),
                "remaining_files": sum(path.is_file() for entry in remaining
                                       for path in entry.rglob("*")),
                "remaining_bytes": sum(path.stat().st_size for entry in remaining
                                       for path in entry.rglob("*") if path.is_file()),
                "summary": result.stdout.splitlines()[-1:],
            }), flush=True)
            self.assertEqual(73 if fail_copy_at else 0, result.returncode,
                             result.stdout + result.stderr)
            if fail_copy_at:
                self.assertIn("injected-copy-failure", result.stderr)
                self.assertEqual(str(fail_copy_at), counter.read_text().strip())
            else:
                self.assertIn("=== Results: 29 passed, 0 failed ===", result.stdout)
            self.assertEqual("preserve unrelated files\n", sentinel.read_text())
            self.assertEqual([], remaining, "suite leaked its temporary fixtures")

    def test_normal_exit(self):
        self.check_exit(0)

    def test_first_fixture_copy_failure(self):
        self.check_exit(1)

    def test_late_fixture_copy_failure(self):
        self.check_exit(4)


if __name__ == "__main__":
    unittest.main()
