"""Exercise the warning hook directly and as a real Git post-checkout hook.

The freshness producer is controlled; no Cargo build or network is involved.
Run with: python -m unittest discover -s scripts/tests -p test_warn_stale_checkout.py
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
HOOK = ROOT / "scripts/githooks/warn-stale-checkout.sh"


def find_bash() -> str | None:
    if os.name == "nt":
        git = shutil.which("git")
        if git:
            candidate = Path(git).resolve().parents[1] / "bin/bash.exe"
            if candidate.is_file():
                return str(candidate)
    return shutil.which("bash")


class WarningHookTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.bash = find_bash()
        cls.git = shutil.which("git")
        if not cls.bash or not cls.git:
            raise unittest.SkipTest("Bash and Git are required")
        cls.temp = tempfile.TemporaryDirectory(prefix="warning-hook-")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.root = Path(cls.temp.name)
        cls.repo = cls.root / "repo"
        cls.repo.mkdir()
        cls.bin = cls.root / "bin"
        cls.bin.mkdir()
        cls.hooks = cls.root / "hooks"
        cls.hooks.mkdir()
        cls.shell_bin = cls.shell_path(cls.bin)
        cls.shell_env = cls.root / "shell-env"
        cls.shell_env.write_text('export PATH="$FIXTURE_PATH"\n', encoding="utf-8")
        cls.trace = cls.root / "parser-trace"
        cls.write_tool("bash", 'exec /bin/bash "$@"')
        cls.write_tool("cargo", 'printf "%s" "$FIXTURE_RECEIPT"\nexit "$FIXTURE_CARGO_EXIT"')
        cls.run_git("init", "-q", "-b", "first")
        cls.run_git("config", "core.hooksPath", str(cls.hooks))
        cls.run_git("config", "user.name", "Warning hook fixture")
        cls.run_git("config", "user.email", "fixture@example.invalid")
        cls.run_git("config", "commit.gpgsign", "false")
        cls.run_git("config", "core.autocrlf", "false")
        (cls.repo / "content").write_text("first", encoding="utf-8")
        cls.run_git("add", "content")
        cls.run_git("commit", "-qm", "first")
        cls.run_git("checkout", "-qb", "second")
        (cls.repo / "content").write_text("second", encoding="utf-8")
        cls.run_git("commit", "-qam", "second")
        shutil.copyfile(HOOK, cls.hooks / "post-checkout")
        (cls.hooks / "post-checkout").chmod(0o755)

    @classmethod
    def shell_path(cls, path: Path) -> str:
        if os.name != "nt":
            return str(path)
        return subprocess.check_output(
            [cls.bash, "-c", 'cygpath -u "$1"', "_", str(path)], text=True
        ).strip()

    @classmethod
    def write_tool(cls, name: str, body: str) -> None:
        path = cls.bin / name
        path.write_text("#!/bin/bash\n" + body + "\n", encoding="utf-8", newline="\n")
        path.chmod(0o755)

    @classmethod
    def run_git(cls, *args: str, **kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [cls.git, *args], cwd=cls.repo, text=True, capture_output=True,
            check=True, **kwargs
        )

    def select_parser(self, parser: str) -> None:
        for name in ("jq", "python3", "python"):
            (self.bin / name).unlink(missing_ok=True)
        if parser == "none":
            return
        actual = shutil.which("jq") if parser == "jq" else os.sys.executable
        if not actual:
            self.skipTest(f"{parser} is unavailable")
        executable = self.shell_path(Path(actual)).replace("'", "'\\''")
        self.write_tool(parser, f'printf "%s\\n" "{parser}" >> "$FIXTURE_TRACE"\n'
                        'if [[ "$FIXTURE_PARSER_EXIT" != 0 ]]; then exit "$FIXTURE_PARSER_EXIT"; fi\n'
                        f"exec '{executable}' \"$@\"")

    def test_receipts_never_fail_checkout(self) -> None:
        stale = {"safe_for_code_state_claims": False, "behind_by": 7, "base_ref": "origin/main"}
        fresh = dict(stale, safe_for_code_state_claims=True, behind_by=0)
        cases = [
            ("malformed", "{broken", 0, False),
            ("wrong-shape", '"unexpected"', 0, False),
            ("missing-fields", "{}", 0, False),
            ("missing-warning-fields", '{"safe_for_code_state_claims": false}', 0, False),
            ("wrong-field-type", json.dumps(dict(stale, behind_by="seven")), 0, False),
            ("stale", json.dumps(stale), 0, True),
            ("fresh", json.dumps(fresh), 0, False),
            ("producer-failed", json.dumps(stale), 37, False),
            ("parser-failed", json.dumps(stale), 0, False),
        ]
        for parser in ("jq", "python3", "python", "none"):
            with self.subTest(parser=parser):
                self.select_parser(parser)
                for name, receipt, code, warning in cases:
                    for mode in ("direct", "checkout"):
                        with self.subTest(parser=parser, receipt=name, mode=mode):
                            env = dict(os.environ, PATH=self.shell_bin,
                                       FIXTURE_PATH=self.shell_bin,
                                       BASH_ENV=self.shell_path(self.shell_env),
                                       FIXTURE_TRACE=self.shell_path(self.trace),
                                       FIXTURE_PARSER_EXIT="23" if name == "parser-failed" else "0",
                                       FIXTURE_RECEIPT=receipt, FIXTURE_CARGO_EXIT=str(code))
                            self.trace.unlink(missing_ok=True)
                            if mode == "direct":
                                args = [self.bash, self.shell_path(HOOK)]
                            else:
                                current = self.run_git("branch", "--show-current").stdout.strip()
                                destination = "first" if current == "second" else "second"
                                args = [self.git, "checkout", "-q", destination]
                            result = subprocess.run(args, cwd=self.repo, env=env,
                                                    text=True, capture_output=True)
                            observed = self.trace.read_text().splitlines() if self.trace.exists() else []
                            if parser != "none" and code == 0:
                                self.assertTrue(observed, "selected parser did not execute")
                                self.assertEqual(set(observed), {parser})
                                self.assertEqual(len(observed), 1, "receipt should be parsed once")
                            else:
                                self.assertEqual(observed, [])
                            self.assertEqual(result.returncode, 0, result.stderr)
                            expected_warning = warning and parser != "none"
                            self.assertEqual("stale checkout:" in result.stderr, expected_warning,
                                             result.stderr)
                            if expected_warning:
                                self.assertIn("7 commit(s) behind origin/main", result.stderr)
                                self.assertIn("run: git pull --rebase", result.stderr)
                            if mode == "checkout":
                                self.assertEqual(self.run_git("branch", "--show-current").stdout.strip(),
                                                 destination)
                                self.assertEqual((self.repo / "content").read_text(), destination)


if __name__ == "__main__":
    unittest.main()
