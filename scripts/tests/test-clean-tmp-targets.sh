#!/usr/bin/env bash
# Behavioral safety proof: fixtures never expose real /tmp caches to deletion.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 - "$SCRIPT_DIR/../clean-tmp-targets.sh" <<'PY'
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

IMPL = Path(sys.argv.pop()).resolve()
BASH = shutil.which("bash")


class TemporaryTargetInspection(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="tmp-target-proof-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.targets = self.root / "targets"
        self.targets.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "calls.log"
        self.porcelain = self.root / "worktrees.txt"
        self.porcelain.write_text(
            "worktree /repo/.worktrees/agent-live\nHEAD abc\n"
            "branch refs/heads/topic/renamed\n\n"
        )
        self.env = os.environ.copy()
        self.env.update(
            PATH=str(self.bin) + os.pathsep + self.env["PATH"],
            APPLY="0", GRACE_MINUTES="5", SPY_LOG=str(self.log),
            GIT_PORCELAIN=str(self.porcelain), GIT_CASE="good",
        )
        source = IMPL.read_text()
        # Preserve the actual script except its two literal scan roots. Refuse
        # fixture setup if the production entrypoint changes those constants.
        for pattern in ('"/tmp/agent-*-target"', '"/tmp/wt-*-target"'):
            self.assertEqual(source.count(pattern), 1)
            source = source.replace(pattern, '"' + str(self.targets) + pattern[5:])
        self.fixture_impl = self.root / "inspect.sh"
        self.fixture_impl.write_text(source)
        self.old = self.make_target("agent-unowned-target", old=True)
        self.named = self.make_target("agent-live-target", old=True)
        self.renamed = self.make_target("agent-worktree-agent-live-target", old=True)
        self.recent = self.make_target("wt-recent-target", old=False)
        victim = self.root / "outside"
        victim.mkdir()
        (victim / "unique-source.pl").write_text("retained source\n")
        (self.targets / "agent-linked-target").symlink_to(victim, target_is_directory=True)
        self.before = self.snapshot()
        self.write_tool("git", """
case "$*" in
  *rev-parse*)
    [ "$GIT_CASE" != root-failure ] || exit 9
    printf '%s\\n' /repo
    ;;
  *"worktree list --porcelain"*)
    [ "$GIT_CASE" != empty ] || exit 0
    cat "$GIT_PORCELAIN"
    [ "$GIT_CASE" != partial-failure ] || exit 9
    ;;
  *) exit 99 ;;
esac
""")

    def make_target(self, name, old):
        p = self.targets / name
        p.mkdir()
        for name, content in [
            ("source.pl", "unique work\n"),
            (".ignored", "scratch\n"),
            ("receipt.json", '{"proof": "keep"}\n'),
            ("owner-marker", "retained lease evidence\n"),
        ]:
            f = p / name
            f.write_text(content)
            if old:
                os.utime(f, (946684800, 946684800))
        if old:
            os.utime(p, (946684800, 946684800))
        return p

    def snapshot(self):
        result = {}
        for base in (self.targets, self.root / "outside"):
            if not base.exists():
                continue
            for p in [base, *base.rglob("*")]:
                st = p.lstat()
                value = os.readlink(p) if p.is_symlink() else (
                    p.read_bytes() if p.is_file() else None
                )
                result[str(p.relative_to(self.root))] = (
                    st.st_ino, st.st_mode, st.st_size, st.st_mtime_ns, value
                )
        return result

    def write_tool(self, name, body):
        p = self.bin / name
        p.write_text(
            "#!/bin/sh\n"
            + "printf '%s\\n' " + shlex.quote(name) + " >> \"$SPY_LOG\"\n"
            + body
        )
        p.chmod(0o755)

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def run_script(self, args=(), apply="0", impl=None, git_case="good"):
        env = dict(self.env, APPLY=apply, GIT_CASE=git_case)
        result = subprocess.run(
            [BASH, str(impl or self.fixture_impl), *args],
            env=env, cwd=self.root, text=True, capture_output=True, timeout=15,
        )
        self.assertEqual(self.snapshot(), self.before, result.stdout + result.stderr)
        return result

    def test_retired_forms_refuse_before_external_discovery(self):
        # The original production entrypoint is safe to exercise even on RED:
        # every discovery/deletion command is a refusing spy.
        for name in ("git", "mktemp", "find", "du", "rm", "sort",
                     "basename", "head", "awk", "grep", "sed"):
            self.write_tool(name, "exit 99\n")
        cases = [
            (("--prune",), "0"), ((), "1"),
            (("--prune", "--dry-run"), "0"),
            (("--dry-run", "--prune"), "0"),
            (("--dry-run",), "1"), (("--prune",), "1"),
            (("--help", "--prune"), "0"),
            (("--prune", "--help"), "0"), (("--help",), "1"),
        ]
        for args, apply in cases:
            with self.subTest(args=args, apply=apply):
                self.log.unlink(missing_ok=True)
                result = self.run_script(args, apply, impl=IMPL)
                self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
                self.assertIn("retired", result.stderr.lower())
                self.assertEqual(self.calls(), [])

    def test_advisory_observes_fixture_without_ownership_clearance(self):
        # Deterministic size oracle plus argument evidence: a no-op size
        # observer must not pass merely because it prints the fixture path.
        self.write_tool("du", """
printf 'DU_ARG:%s|%s\\n' "$1" "$2" >> "$SPY_LOG"
case "$1" in
  -sk) printf '8192\\t%s\\n' "$2" ;;
  -sh) printf '8.0M\\t%s\\n' "$2" ;;
  *) exit 99 ;;
esac
""")
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("UNVERIFIED  " + str(self.old), result.stdout)
        self.assertIn("NAMED   " + str(self.named), result.stdout)
        self.assertIn("NAMED   " + str(self.renamed), result.stdout)
        self.assertIn("RECENT  " + str(self.recent), result.stdout)
        self.assertIn("ownership and reclaimability unverified", result.stdout)
        self.assertIn("8.0M observed; no ownership clearance", result.stdout)
        self.assertIn("8M observed", result.stdout)
        self.assertIn("DU_ARG:-sk|" + str(self.old), self.calls())
        self.assertIn("DU_ARG:-sh|" + str(self.old), self.calls())
        for misleading in ("reclaimable", "Re-run with --prune", "safe to delete", "ORPHAN"):
            self.assertNotIn(misleading, result.stdout)

    def test_git_observation_failure_refuses_without_traversal(self):
        for name in ("mktemp", "find", "du", "rm"):
            self.write_tool(name, "exit 99\n")
        for case in ("root-failure", "partial-failure", "empty"):
            with self.subTest(case=case):
                self.log.unlink(missing_ok=True)
                result = self.run_script(git_case=case)
                self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
                self.assertIn("REFUSING", result.stderr)
                self.assertTrue(self.calls())
                self.assertTrue(all(name == "git" for name in self.calls()))
                self.assertNotIn("UNVERIFIED", result.stdout)

    def test_help_and_unknown_flags_remain_non_destructive(self):
        result = self.run_script(("--help",), impl=IMPL)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("advisory", result.stdout.lower())
        self.assertIn("retired", result.stdout.lower())
        self.assertEqual(self.calls(), [])
        result = self.run_script(("--unknown",), impl=IMPL)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])


unittest.main(verbosity=2)
PY
