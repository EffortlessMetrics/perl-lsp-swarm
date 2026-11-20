from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


PACKAGE_ROOT = Path(__file__).resolve().parents[1]
CLIENT_ROOT = PACKAGE_ROOT.parent
VALIDATOR_PATH = CLIENT_ROOT / "validate_sublime_host_receipt.py"
SCHEMA_PATH = CLIENT_ROOT / "sublime-host-receipt.v1.schema.json"
WORKFLOW_PATH = Path(__file__).resolve().parents[4] / ".github" / "workflows" / "sublime-real-host.yml"


def job_level_env_blocks(workflow_text: str):
    """Yield `(indent, lines)` for every `env:` mapping owned by a job.

    Stdlib-only and deliberately structural rather than a YAML parse: the
    package-contract runner installs a bare interpreter, so PyYAML is not
    available. A job-level `env:` is one nested exactly two levels under the
    top-level `jobs:` key (`jobs:` -> `<job_id>:` -> `env:`), which
    distinguishes it from a step-level `env:` nested inside a `steps:` list
    item, where the runner context *is* legal.
    """
    lines = workflow_text.splitlines()
    in_jobs = False
    job_indent = None
    blocks = []
    for index, line in enumerate(lines):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if indent == 0:
            in_jobs = line.startswith("jobs:")
            job_indent = None
            continue
        if not in_jobs:
            continue
        if job_indent is None:
            # First nested key under `jobs:` establishes the job-id indent.
            job_indent = indent
        if indent != job_indent + 2:
            continue
        if line.strip() != "env:":
            continue
        body = []
        for following in lines[index + 1 :]:
            if not following.strip() or following.lstrip().startswith("#"):
                continue
            if len(following) - len(following.lstrip()) <= indent:
                break
            body.append(following)
        blocks.append((indent, body))
    return blocks


def load_validator():
    spec = importlib.util.spec_from_file_location("sublime_host_receipt_validator", VALIDATOR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load Sublime host receipt validator")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sample_receipt() -> dict:
    assertions = {name: True for name in load_validator().REQUIRED_ASSERTIONS}
    return {
        "schema_version": 1,
        "stage": "exact_source_local",
        "source_sha": "a" * 40,
        "recorded_at": "2026-08-13T10:30:00+00:00",
        "host": {
            "name": "Sublime Text",
            "version": "4200",
            "platform": "linux",
            "arch": "x64",
        },
        "lsp_package": {
            "repository": "sublimelsp/LSP",
            "ref": "cc9f5201d9f053d9ab67aa0ea575b494fd133803",
        },
        "helper_package": {
            "name": "LSP-perllsp",
            "source": "clients/sublime/LSP-perllsp",
        },
        "binary": {
            "path": "/tmp/perllsp",
            "sha256": "b" * 64,
            "command": ["/tmp/perllsp", "--stdio"],
        },
        "fixtures": {
            "pl": "app.pl",
            "pm": "customlib/Greeting.pm",
            "t": "t/greeting.t",
        },
        "assertions": assertions,
    }


class SublimeHostContractTests(unittest.TestCase):
    def test_validator_accepts_complete_exact_source_receipt(self) -> None:
        load_validator().validate(sample_receipt())

    def test_validator_rejects_public_stage_overclaim(self) -> None:
        payload = sample_receipt()
        payload["stage"] = "package_control_public"
        with self.assertRaisesRegex(ValueError, "exact_source_local"):
            load_validator().validate(payload)

    def test_schema_and_unittesting_configuration_are_valid_json(self) -> None:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        config = json.loads((PACKAGE_ROOT / "unittesting.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["stage"]["const"], "exact_source_local")
        self.assertEqual(config["tests_dir"], "host_tests")
        self.assertTrue(config["deferred"])
        self.assertGreaterEqual(config["condition_timeout"], 120_000)

    def test_workflow_pins_lsp_2_13_source_and_all_three_host_os_families(self) -> None:
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertIn("cc9f5201d9f053d9ab67aa0ea575b494fd133803", workflow)
        self.assertIn("ubuntu-latest", workflow)
        self.assertIn("macos-latest", workflow)
        self.assertIn("windows-latest", workflow)
        self.assertNotIn("Package Control: Install Package", workflow)

    def test_workflow_uses_pinned_setup_without_the_latest_installer(self) -> None:
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertIn("run: python scripts/sublime/setup_pinned_host.py", workflow)
        self.assertNotIn("SublimeText/UnitTesting/actions/setup", workflow)

    def test_pinned_setup_with_latest_unavailable_and_wrong_digest(self) -> None:
        repo = WORKFLOW_PATH.parents[2]
        spec = importlib.util.spec_from_file_location(
            "setup_pinned_host", repo / "scripts/sublime/setup_pinned_host.py"
        )
        setup = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(setup)
        run = subprocess.run

        def without_network(command, **kwargs):
            if command[0] == "curl":
                self.assertEqual(
                    command[command.index("-o") - 1],
                    "https://download.sublimetext.com/sublime_text_build_4200_x64.tar.xz",
                    "only the exact artifact is available in this fixture",
                )
                shutil.copyfile(archive, command[-1])
            elif command[0] == "git" and "fetch" in command:
                self.assertIn(command[-2], package_commits)
                self.assertEqual(command[-1], commit)
                return run([*command[:-2], str(package_source), commit], **kwargs)
            else:
                return run(command, **kwargs)

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / setup.ARTIFACTS["Linux"][0]
            with tarfile.open(archive, "w:xz") as fixture:
                for name in ("sublime_text", "plugin_host-3.8"):
                    data = b"reviewed fixture bytes"
                    info = tarfile.TarInfo("sublime_text/" + name)
                    info.size = len(data)
                    fixture.addfile(info, io.BytesIO(data))

            package_source = root / "package"
            run(["git", "init", "-q", package_source], check=True)
            (package_source / "package.py").touch()
            run(["git", "-C", package_source, "add", "package.py"], check=True)
            run(
                ["git", "-C", package_source, "-c", "user.name=Fixture",
                 "-c", "user.email=fixture@example.com", "commit", "-qm", "fixture"],
                check=True,
            )
            commit = subprocess.check_output(
                ["git", "-C", package_source, "rev-parse", "HEAD"], text=True
            ).strip()

            # Route the three reviewed commit requests to one local Git fixture.
            package_commits = {
                "https://github.com/SublimeText/UnitTesting.git":
                    "b8abedefd5fbc0f64a8c6082506ebd1e24203164",
                "https://github.com/sublimelsp/LSP.git": "cc9f5201d9f053d9ab67aa0ea575b494fd133803",
                "https://github.com/packagecontrol/sublime_aio.git":
                    "b3d1d559ec223f276c9693e17476768257951266",
            }
            install_package = setup.install_package

            def install_fixture(destination, repository, expected):
                url = f"https://github.com/{repository}.git"
                self.assertEqual(expected, package_commits[url])
                install_package(destination, repository, commit)

            config_home = root / "config"
            github_env = root / "github.env"
            with mock.patch.dict(os.environ, {
                "RUNNER_OS": "Linux",
                "RUNNER_TEMP": temp,
                "XDG_CONFIG_HOME": str(config_home),
                "GITHUB_ENV": str(github_env),
                "PERLLSP_LSP_REF": package_commits["https://github.com/sublimelsp/LSP.git"],
            }), mock.patch.object(setup.subprocess, "run", side_effect=without_network):
                with self.assertRaisesRegex(SystemExit, "SHA-256 mismatch"):
                    setup.main()
                self.assertFalse(github_env.exists())
                self.assertFalse(config_home.exists())
                self.assertEqual(list(root.glob("sublime-4200-*")), [])

                digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                with mock.patch.dict(setup.ARTIFACTS, {"Linux": (archive.name, digest)}), \
                        mock.patch.object(setup, "install_package", side_effect=install_fixture):
                    setup.main()

            values = dict(line.split("=", 1) for line in github_env.read_text().splitlines())
            self.assertEqual(
                Path(values["PERLLSP_SUBLIME_LAUNCHER"]).read_bytes(), b"reviewed fixture bytes"
            )
            packages = Path(values["SUBLIME_TEXT_PACKAGES"])
            for name in ("LSP", "UnitTesting", "sublime_aio"):
                self.assertTrue((packages / name / "package.py").is_file())
                self.assertFalse((packages / name / ".git").exists())
            self.assertTrue((packages / "LSP-perllsp/plugin.py").is_file())

    def test_job_level_env_uses_no_runner_context(self) -> None:
        """The host lane must survive GitHub's workflow validation.

        `runner` is not an available context for `jobs.<id>.env`. A
        `${{ runner.* }}` reference there is rejected while the run is being
        created, so the run is marked failed with zero jobs and no logs — the
        three-OS journey never executes and no receipt is ever produced, while
        the red X looks like an ordinary test failure. Every receipt path must
        therefore be bound in a step, where `$RUNNER_OS` is real.
        """
        for indent, block in job_level_env_blocks(WORKFLOW_PATH.read_text(encoding="utf-8")):
            for line in block:
                self.assertNotIn(
                    "runner.",
                    line,
                    msg=(
                        "job-level env (indent {}) references the runner context, which "
                        "fails workflow validation before any job starts: {!r}".format(
                            indent, line.strip()
                        )
                    ),
                )

    def test_host_receipt_path_is_bound_per_runner_in_a_step(self) -> None:
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertIn('PERLLSP_SUBLIME_RECEIPT=%s\\n', workflow)
        self.assertIn('"$GITHUB_WORKSPACE/target/sublime-host/$RUNNER_OS.json"', workflow)


if __name__ == "__main__":
    unittest.main()
