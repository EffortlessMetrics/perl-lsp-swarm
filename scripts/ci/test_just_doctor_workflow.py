"""Keep the seven real-entrypoint doctor regressions reachable in hosted CI.

This is a bounded contract for the existing self-test job, not a general YAML
validator or proof of all doctor checks. No third-party Python packages needed.
"""

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci-gate-self-tests.yml"
JOB = "agent-preflight-self-test"
PATHS = (
    "justfile",
    "scripts/tests/test_just_doctor.py",
    "scripts/ci/test_just_doctor_workflow.py",
    ".github/workflows/ci-gate-self-tests.yml",
)
COMMAND = (
    "python3 -B -m unittest scripts.tests.test_just_doctor "
    "scripts.ci.test_just_doctor_workflow -v"
)
INSTALL = """      - name: Install just for doctor regressions
        uses: taiki-e/install-action@d438492cf8a250514fa2d34b30bc3c0dc37c65ff
        with:
          tool: just@1.46.0
          fallback: none"""
RUN = f"""      - name: Run doctor entrypoint and CI route regressions
        shell: bash
        run: |
          set -euo pipefail
          just --version
          git --version
          bash --version
          {COMMAND}"""


def block(text, header, boundary):
    match = re.search(rf"^{re.escape(header)}\n(.*?)(?={boundary}|\Z)", text, re.M | re.S)
    if not match:
        raise AssertionError(f"missing workflow block: {header.strip()}")
    return match.group(0).rstrip()


def check_route(text):
    # Deliberately validate this workflow's small, literal YAML shape. Comments
    # cannot satisfy a trigger, installation, or executable-command obligation.
    text = "\n".join(line.split(" #", 1)[0].rstrip() for line in text.splitlines()
                     if not line.lstrip().startswith("#")) + "\n"
    event = block(text, "  pull_request:", r"^  \S")
    if set(re.findall(r"^    ([\w-]+):", event, re.M)) != {"branches", "paths"}:
        raise AssertionError("doctor PR trigger must keep the default update activity types")
    if "    branches: [main, master]" not in event.splitlines():
        raise AssertionError("doctor regressions must run for PRs to main and master")
    paths = block(event, "    paths:", r"^    \S")
    if re.search(r"^      - ['\"]?!", paths, re.M):
        raise AssertionError("negative PR paths can exclude doctor changes")
    for path in PATHS:
        if not re.search(rf"^      - '{re.escape(path)}'$", paths, re.M):
            raise AssertionError(f"missing pull_request path: {path}")
    job = block(text, f"  {JOB}:", r"^  \S")
    if re.search(r"^    (if|needs|continue-on-error):", job, re.M):
        raise AssertionError("doctor job must run unconditionally and fail honestly")
    for setting in ("    runs-on: ubuntu-24.04", "    timeout-minutes: 10"):
        if setting not in job.splitlines():
            raise AssertionError(f"missing bounded hosted setting: {setting.strip()}")
    for expected in (INSTALL, RUN):
        header = expected.splitlines()[0]
        if block(job, header, r"^      - ") != expected:
            raise AssertionError(f"changed doctor step: {header.strip()}")
    if not job.index("uses: actions/checkout@") < job.index(INSTALL) < job.index(RUN):
        raise AssertionError("doctor requires checkout then just installation before tests")
    return block(job, RUN.splitlines()[0], r"^      - ").split("        run: |\n", 1)[1]


class JustDoctorWorkflowTests(unittest.TestCase):
    def test_hosted_route(self):
        check_route(WORKFLOW.read_text())

    def test_missing_triggers_are_rejected(self):
        text = WORKFLOW.read_text()
        for path in PATHS:
            with self.subTest(path=path), self.assertRaisesRegex(AssertionError, "path"):
                check_route(text.replace(f"      - '{path}'", f"      # - '{path}'"))
            with self.subTest(excluded=path), self.assertRaisesRegex(AssertionError, "negative"):
                check_route(text.replace(f"      - '{path}'", f"      - '{path}'\n      - '!{path}'"))

    def test_skipped_or_masked_execution_is_rejected(self):
        text = WORKFLOW.read_text()
        for before, after in (
            ("  pull_request:\n", "  pull_request:\n    types: [closed]\n"),
            ("    branches: [main, master]", "    branches: [unrelated]"),
            ("    paths:", "    paths-ignore:"),
            (f"  {JOB}:\n", f"  {JOB}:\n    if: false\n"),
            (f"  {JOB}:\n", f"  {JOB}:\n    continue-on-error: true\n"),
            (INSTALL.splitlines()[0], INSTALL.splitlines()[0] + "\n        if: false"),
            ("tool: just@1.46.0", "tool: unrelated-tool"),
            (RUN.splitlines()[0], RUN.splitlines()[0] + "\n        continue-on-error: true"),
            (COMMAND, "echo " + COMMAND),
            (COMMAND, "# " + COMMAND),
            (COMMAND, COMMAND + " || true"),
            (COMMAND, "python3 -m unittest discover -s scripts/tests -p absent.py"),
        ):
            with self.subTest(after=after), self.assertRaises(AssertionError):
                check_route(text.replace(before, after, 1))

    def test_step_propagates_tool_and_test_failures(self):
        script = check_route(WORKFLOW.read_text())
        script = "\n".join(line[10:] for line in script.splitlines())
        bash = shutil.which("bash")
        self.assertIsNotNone(bash, "Bash is required to prove the hosted run body")
        with tempfile.TemporaryDirectory(prefix="doctor-ci-route-") as temp:
            root = Path(temp)
            log = root / "python.args"
            for failing in (None, "just", "git", "bash", "python3"):
                with self.subTest(failing=failing):
                    log.unlink(missing_ok=True)
                    for tool in ("just", "git", "bash", "python3"):
                        body = '#!/bin/sh\n'
                        if tool == "python3":
                            body += 'printf "%s\\n" "$@" > "$DOCTOR_ROUTE_LOG"\n'
                        body += f'exit {23 if tool == failing else 0}\n'
                        executable = root / tool
                        executable.write_text(body)
                        executable.chmod(0o755)
                    result = subprocess.run(
                        [bash, "-c", script], cwd=ROOT,
                        env={**os.environ, "PATH": temp, "DOCTOR_ROUTE_LOG": str(log)},
                        capture_output=True, text=True, timeout=10,
                    )
                    self.assertEqual(result.returncode, 23 if failing else 0, result.stderr)
                    if failing in (None, "python3"):
                        self.assertEqual(log.read_text().splitlines(), COMMAND.split()[1:])
                    else:
                        self.assertFalse(log.exists(), "broken instruments must stop before tests")


if __name__ == "__main__":
    unittest.main()
