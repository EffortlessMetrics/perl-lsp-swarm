#!/usr/bin/env python3
"""Exercise release.yml's real primary-dispatch shell without publication.

POSIX, Python, PyYAML, Bash and grep only. A recording gh fake is the only
publisher command on PATH. The environment contains no credentials. This
executes one reviewed shell step, not GitHub job eligibility, credentials,
child workflows, or installed qualification. It is not a general sandbox.

The RC regression is deliberately red until #17628 is implemented in the
existing release-workflow carrier. Do not add it to a passing gate in that state.
"""

from __future__ import annotations

import copy
import itertools
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import yaml

WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/release.yml"
REPOSITORY = "fixture-owner/release-proof"
PUBLISHERS = ["publish-crates.yml", "publish-extension.yml", "docker-publish.yml"]
FAKE_GH = r'''
import json
import os
import re
import sys
from pathlib import Path

args = sys.argv[1:]
if os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN"):
    sys.exit("instrument refuses credentials")
if args[:3] != ["api", "-X", "POST"] or len(args) < 4:
    sys.exit("unexpected gh command: " + repr(args))
match = re.fullmatch(r"/repos/fixture-owner/release-proof/actions/workflows/([^/]+)/dispatches", args[3])
if not match:
    sys.exit("unexpected dispatch endpoint: " + repr(args))
fields = {}
for index in range(4, len(args), 2):
    if args[index] not in ("-f", "-F") or index + 1 >= len(args):
        sys.exit("unexpected gh field arguments")
    key, separator, value = args[index + 1].partition("=")
    if not separator or key in fields:
        sys.exit("malformed or duplicate gh field")
    fields[key] = value
with Path(os.environ["FAKE_GH_LOG"]).open("a", encoding="utf-8") as log:
    log.write(json.dumps({"workflow": match[1], "fields": fields, "argv": args}) + "\n")
'''


def primary_step(document: dict) -> dict:
    steps = document["jobs"]["dispatch-publishers"]["steps"]
    matches = [step for step in steps if step.get("name") == "Dispatch primary publishers"]
    if len(matches) != 1:
        raise ValueError("expected exactly one primary publisher step")
    step = matches[0]
    if not isinstance(step.get("run"), str) or not step["run"].strip():
        raise ValueError("primary publisher shell is absent or empty")
    if "${{" in step["run"]:
        raise ValueError("inline Actions expressions require an explicit instrument update")
    if not isinstance(step.get("env"), dict):
        raise ValueError("primary publisher environment is absent")
    return step


class PrimaryDispatchTests(unittest.TestCase):
    def setUp(self) -> None:
        if os.name != "posix":
            raise RuntimeError("this shell proof requires a POSIX host")
        self.bash = shutil.which("bash")
        grep = shutil.which("grep")
        if self.bash is None or grep is None:
            raise RuntimeError("Bash and grep are required; missing tools are not passing proof")
        self.document = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        self.step = primary_step(self.document)
        self.temp = tempfile.TemporaryDirectory(prefix="perl-rc-dispatch-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.bin / "grep").symlink_to(grep)
        self.fake = self.root / "fake_gh.py"
        self.fake.write_text(FAKE_GH, encoding="utf-8")
        gh = self.bin / "gh"
        gh.write_text('#!/bin/sh\nexec "$FAKE_PYTHON" -S "$FAKE_GH_SCRIPT" "$@"\n', encoding="utf-8")
        gh.chmod(0o755)
        self.log = self.root / "gh.jsonl"

    def run_step(self, *, version: str = "0.18.0-rc.987", skips: tuple[str, str, str] = ("false",) * 3,
                 script: str | None = None, context_patch: dict[str, str] | None = None) -> tuple[subprocess.CompletedProcess, list[dict]]:
        context = {
            "github.token": "", "github.repository": REPOSITORY, "github.sha": "a" * 40,
            "needs.release-metadata.outputs.version": version,
            "needs.release-metadata.outputs.tag": f"v{version}",
            "needs.release-metadata.outputs.prerelease": "true" if "-" in version else "false",
            "needs.publisher-eligibility.outputs.candidate_run_id": "1001",
            "needs.publisher-eligibility.outputs.manifest_sha256": "b" * 64,
            "github.event.inputs.skip_crates": skips[0],
            "github.event.inputs.skip_extension": skips[1],
            "github.event.inputs.skip_docker": skips[2],
        }
        context.update(context_patch or {})
        # Only bind variables actually declared by the workflow. Supplying a
        # missing authorization/PRERELEASE variable here would conceal bad wiring.
        env = {"PATH": str(self.bin), "HOME": str(self.root), "LC_ALL": "C"}
        for name, raw in self.step["env"].items():
            value = str(raw)
            match = re.fullmatch(r"\$\{\{\s*(.*?)\s*\}\}", value)
            if match:
                env[name] = context[match[1]]
            elif "${{" in value:
                raise ValueError("unsupported compound Actions expression")
            else:
                env[name] = value
        env.update({"GITHUB_REPOSITORY": REPOSITORY, "FAKE_PYTHON": sys.executable,
                    "FAKE_GH_SCRIPT": str(self.fake), "FAKE_GH_LOG": str(self.log)})
        if env.get("GH_TOKEN") or env.get("GITHUB_TOKEN"):
            raise ValueError("instrument refuses credentials before spawning")
        self.log.unlink(missing_ok=True)
        shell = self.root / "step.sh"
        shell.write_text(self.step["run"] if script is None else script, encoding="utf-8")
        syntax = subprocess.run([self.bash, "--noprofile", "--norc", "-n", str(shell)],
                                env=env, capture_output=True, text=True, timeout=5)
        self.assertEqual(syntax.returncode, 0, syntax.stderr)
        result = subprocess.run([self.bash, "--noprofile", "--norc", "--restricted", str(shell)],
                                cwd=self.root, env=env, capture_output=True, text=True, timeout=5)
        calls = [json.loads(line) for line in self.log.read_text(encoding="utf-8").splitlines()] if self.log.exists() else []
        return result, calls

    def assert_completed(self, result: subprocess.CompletedProcess) -> None:
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def assert_no_rc_primary_dispatch(self, calls: list[dict]) -> None:
        # This fixture selects no optional Open VSX channel. A combined
        # Marketplace/Open VSX workflow is not authorized by skip_extension=false.
        self.assertEqual(calls, [], "RC fixture dispatched a forbidden primary publisher")

    def test_rc_skip_flags_cannot_grant_primary_publisher_authority(self) -> None:
        for skips in itertools.product(("false", "", "true"), repeat=3):
            with self.subTest(skips=skips):
                result, calls = self.run_step(skips=skips)
                self.assert_completed(result)
                self.assert_no_rc_primary_dispatch(calls)

    def test_existing_final_fanout_and_exact_identity_fields_remain_observable(self) -> None:
        # A positive instrument/current-behavior control, NOT proof of final
        # release authorization. That upstream authority is outside this step.
        for skips in itertools.product(("false", "true"), repeat=3):
            with self.subTest(skips=skips):
                result, calls = self.run_step(version="0.18.0", skips=skips)
                self.assert_completed(result)
                self.assertEqual([call["workflow"] for call in calls],
                                 [name for name, skip in zip(PUBLISHERS, skips) if skip != "true"])
                for call in calls:
                    fields = call["fields"]
                    self.assertEqual(fields["ref"], "v0.18.0")
                    self.assertEqual(fields["inputs[version]"], "0.18.0")
                    self.assertEqual(fields["inputs[expected_sha]"], "a" * 40)
                    self.assertEqual(fields["inputs[candidate_run_id]"], "1001")
                    self.assertEqual(fields["inputs[manifest_sha256]"], "b" * 64)

    def test_malformed_existing_identity_fields_refuse_before_dispatch(self) -> None:
        for key, value in [("github.sha", "a" * 39), ("github.sha", "g" * 40),
                           ("needs.publisher-eligibility.outputs.candidate_run_id", "not-a-run"),
                           ("needs.publisher-eligibility.outputs.manifest_sha256", "b" * 63)]:
            with self.subTest(key=key, value=value):
                result, calls = self.run_step(context_patch={key: value})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [])

    def test_recording_fake_retains_dispatch_when_shell_later_fails(self) -> None:
        result, calls = self.run_step(version="0.18.0", skips=("false", "true", "true"),
                                      script=self.step["run"] + "\nexit 73\n")
        self.assertEqual(result.returncode, 73)
        self.assertEqual([call["workflow"] for call in calls], ["publish-crates.yml"])

    def test_empty_duplicate_or_missing_step_is_instrument_failure(self) -> None:
        for mutation in ("empty", "duplicate", "missing"):
            with self.subTest(mutation=mutation):
                document = copy.deepcopy(self.document)
                steps = document["jobs"]["dispatch-publishers"]["steps"]
                step = primary_step(document)
                if mutation == "empty":
                    step["run"] = ""
                elif mutation == "duplicate":
                    steps.append(copy.deepcopy(step))
                else:
                    steps.remove(step)
                with self.assertRaises(ValueError):
                    primary_step(document)

    def test_fixture_guard_and_its_removal_discriminate_the_rc_failure(self) -> None:
        # Instrument control, independent of whether production has been fixed.
        # This deliberately wrong fixture is not a copied implementation oracle.
        unsafe = ('gh api -X POST '
                  '/repos/$REPOSITORY/actions/workflows/publish-crates.yml/dispatches '
                  '-f ref="$TAG"\n')
        guarded = 'if [[ "$VERSION" == *-rc.* ]]; then exit 0; fi\n' + unsafe
        result, calls = self.run_step(script=guarded)
        self.assert_completed(result)
        self.assert_no_rc_primary_dispatch(calls)
        result, calls = self.run_step(version="0.18.0", script=guarded)
        self.assert_completed(result)
        self.assertEqual([call["workflow"] for call in calls], ["publish-crates.yml"])
        result, calls = self.run_step(script=unsafe)
        self.assert_completed(result)
        with self.assertRaises(AssertionError):
            self.assert_no_rc_primary_dispatch(calls)

    def test_an_always_closed_fixture_is_not_successful_final_fanout(self) -> None:
        result, calls = self.run_step(version="0.18.0", script="exit 0\n")
        self.assert_completed(result)
        with self.assertRaises(AssertionError):
            self.assertEqual([call["workflow"] for call in calls], PUBLISHERS)


if __name__ == "__main__":
    unittest.main()
