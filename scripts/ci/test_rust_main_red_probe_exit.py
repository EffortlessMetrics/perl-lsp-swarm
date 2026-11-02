"""Execute the complete extracted probe with hermetic read/status fixtures.

This tests the shell consumer, including trusted-source transport and decoding.
Classifier policy and network responses are fixtures, not live qualification.
"""
from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / ".ci/rust-main-red-probe.sh"

FIXTURE = r'''
set -u
read -r -a statuses <<< "$FIXTURE_STATUSES"
call=0
fail() { echo "HARNESS_ERROR:$*" >> "$TRACE"; return 97; }
python3() {
  if [ "$1" = scripts/ci/github_read.py ]; then
    shift
    if [ "$1" = --paginate ]; then
      [ "$#" = 3 ] && [ "$2" = --slurp ] || { fail read-arguments; return 97; }
      path="$3"
      case "$path" in
        "repos/$REPOSITORY/commits/$MAIN/check-runs?per_page=100"|"repos/$REPOSITORY/commits/$HEAD/check-runs?per_page=100") printf '{"check_runs":[]}\n' ;;
        "repos/$REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$MAIN&per_page=100"|"repos/$REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$HEAD&per_page=100") printf '{"workflow_runs":[]}\n' ;;
        *) fail read-endpoint; return 97 ;;
      esac
    else
      case "$1" in
        "repos/$REPOSITORY/git/ref/heads/main")
          [ "$#" = 3 ] && [ "$2" = --field ] && [ "$3" = .object.sha ] || { fail main-read; return 97; }
          printf '%s\n' "$MAIN" ;;
        "repos/$REPOSITORY/contents/.github/workflows/ci.yml?ref=$MAIN"|"repos/$REPOSITORY/contents/.github/workflows/ci.yml?ref=$TREE")
          [ "$#" = 3 ] && [ "$2" = --field ] && [ "$3" = .sha ] || { fail workflow-read; return 97; }
          printf '%s\n' "$BLOB" ;;
        "repos/$REPOSITORY/contents/scripts/ci/main_red_refusal.py?ref=$MAIN")
          [ "$#" = 1 ] || { fail trusted-read; return 97; }
          if [ "$FIXTURE_UNAVAILABLE" = true ]; then return 1; fi
          printf '{"content":"IyBmaXh0dXJlCg=="}\n' ;;
        *) fail initial-endpoint; return 97 ;;
      esac
    fi
    return 0
  fi
  [ "$1" = "$TRUSTED_SCRIPT" ] && [ "$(< "$1")" = '# fixture' ] || { fail trusted-bytes; return 97; }
  shift
  for name in main-runs candidate-runs main-workflow-runs candidate-workflow-runs; do
    [ "$1" = "--$name" ] && [ "$2" = "$probe_dir/$name.json" ] && [ -s "$2" ] || { fail input-file; return 97; }
    shift 2
  done
  for pair in "main-sha-before:$MAIN" "main-sha-after:$MAIN" "candidate-sha:$HEAD" "main-workflow-sha:$BLOB" "candidate-workflow-sha:$BLOB"; do
    [ "$1" = "--${pair%%:*}" ] && [ "$2" = "${pair#*:}" ] || { fail identity; return 97; }
    shift 2
  done
  phase=normal
  if [ "$#" != 0 ]; then
    [ "$#" = 1 ] && [ "$1" = --final ] || { fail classifier-arguments; return 97; }
    phase=final
  fi
  [ "$call" -lt "${#statuses[@]}" ] || { fail extra-call; return 97; }
  status="${statuses[$call]}"
  call=$((call + 1))
  echo "CALL:$phase:$status" >> "$TRACE"
  case "$status" in
    missing) "$FIXTURE_DIR/missing-python"; return $? ;;
    signal) "$BASH" --noprofile --norc -c 'kill -TERM "$$"'; return $? ;;
    *) return "$status" ;;
  esac
}
sleep() {
  [ "$#" = 1 ] && [ "$1" = 30 ] || { fail sleep-arguments; return 97; }
  echo SLEEP >> "$TRACE"
}
source "$PROBE"
'''


class ExtractedProbeExitTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.tools = {}
        for tool in ("bash", "mktemp", "rm", "jq", "tr", "base64"):
            executable = shutil.which(tool)
            if executable is None:
                raise RuntimeError(f"Required probe test tool unavailable: {tool}")
            cls.tools[tool] = str(Path(executable).resolve())

    def execute(self, statuses, *, expected, polls=0, final=False, unavailable=False):
        with tempfile.TemporaryDirectory(prefix="probe-exit-") as temporary:
            root = Path(temporary)
            binary = root / "bin"
            binary.mkdir()
            for name, executable in self.tools.items():
                (binary / name).symlink_to(executable)
            trace = root / "trace"
            trace.touch()
            env = {
                "PATH": str(binary), "HOME": temporary, "TMPDIR": temporary,
                "LC_ALL": "C", "PROBE": str(PROBE), "TRACE": str(trace),
                "FIXTURE_DIR": temporary, "FIXTURE_STATUSES": " ".join(map(str, statuses)),
                "FIXTURE_UNAVAILABLE": str(unavailable).lower(),
                "REPOSITORY": "fixture/probe", "EVENT_NAME": "merge_group",
                "MAIN": "a" * 40, "HEAD": "b" * 40, "TREE": "c" * 40,
                "BLOB": "d" * 40, "CANDIDATE_SHA": "b" * 40,
                "EFFECTIVE_WORKFLOW_TREE": "c" * 40,
            }
            result = subprocess.run(
                [self.tools["bash"], "--noprofile", "--norc", "-eo", "pipefail", "-c", FIXTURE],
                cwd=temporary, env=env, text=True, capture_output=True, timeout=10,
            )
            events = trace.read_text().splitlines()
            diagnostic = result.stdout + result.stderr + "\n".join(events)
            self.assertNotIn("HARNESS_ERROR", diagnostic)
            self.assertNotIn("command not found", diagnostic)
            self.assertEqual(result.returncode, expected, diagnostic)
            calls = [event for event in events if event.startswith("CALL:")]
            self.assertEqual(len(calls), 0 if unavailable else len(statuses), diagnostic)
            self.assertEqual(events.count("SLEEP"), polls, diagnostic)
            if calls:
                self.assertEqual([":final:" in call for call in calls], [False] * (len(calls) - 1) + [final])
            # The production EXIT trap must clean its own probe directory.
            self.assertEqual(sorted(path.name for path in root.iterdir()), ["bin", "trace"])
            return result

    def test_success_and_explicit_refusal(self):
        for status in (0, 1):
            with self.subTest(status=status):
                self.execute([status], expected=status)

    def test_all_unexpected_initial_statuses_block(self):
        for status in range(3, 256):
            with self.subTest(status=status):
                result = self.execute([status], expected=1)
                self.assertIn(f"exit {status}", result.stdout)

    def test_pending_can_recover_or_refuse(self):
        for status in (0, 1, 3, 127, 137, 255):
            with self.subTest(status=status):
                self.execute([2, status], expected=0 if status == 0 else 1, polls=1)

    def test_final_decisions_after_exact_poll_bound(self):
        for status in (0, 1, 2, 3, 127, 137, 255):
            with self.subTest(status=status):
                self.execute([2] * 51 + [status], expected=0 if status == 0 else 1, polls=50, final=True)

    def test_real_process_failures_at_each_phase(self):
        for failure, code in (("missing", 127), ("signal", 143)):
            for prefix, polls, final in (([], 0, False), ([2], 1, False), ([2] * 51, 50, True)):
                with self.subTest(failure=failure, polls=polls):
                    result = self.execute(prefix + [failure], expected=1, polls=polls, final=final)
                    self.assertIn(f"exit {code}", result.stdout)

    def test_existing_unavailable_classifier_policy_is_unchanged(self):
        result = self.execute([0], expected=0, unavailable=True)
        self.assertIn("non-blocking during bootstrap", result.stdout)

    def test_regression_executes_in_the_retained_result_command(self):
        script = (ROOT / ".ci/rust-standard-result.sh").read_text()
        self.assertLess(script.index("scripts/ci/test_rust_main_red_probe_exit.py"),
                        script.index("python3 scripts/ci/rust_standard_result.py"))
        self.assertIn("scripts/ci/test_rust_standard_result.py", script)
        self.assertIn("scripts/ci/test_main_red_refusal.py", script)


if __name__ == "__main__":
    unittest.main()
