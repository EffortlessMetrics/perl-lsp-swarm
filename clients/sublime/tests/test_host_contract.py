from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path


SUBLIME_ROOT = Path(__file__).resolve().parents[1]
PACKAGE_ROOT = SUBLIME_ROOT / "LSP-perllsp"
CLIENT_ROOT = SUBLIME_ROOT
VALIDATOR_PATH = CLIENT_ROOT / "validate_sublime_host_receipt.py"
SCHEMA_PATH = CLIENT_ROOT / "sublime-host-receipt.v1.schema.json"
WORKFLOW_PATH = SUBLIME_ROOT.parents[1] / ".github" / "workflows" / "sublime-real-host.yml"


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

    def test_lsp_package_is_installed_by_tag_and_asserted_against_the_commit(self) -> None:
        """`extra-packages` cannot pin a commit, so the tag carries a drift guard.

        The UnitTesting setup action resolves an `extra-packages` ref through
        `gitResolvePrefixToTag`, which matches only tags and branches and then
        runs `git clone --branch`. Passing a raw commit SHA there resolves to
        nothing and aborts the install with "No ref found". Installing by tag is
        the only mechanism the action offers, so the exact-source claim is held
        by asserting that the tag still points at the reviewed commit rather
        than by the clone ref itself.
        """
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertIn("PERLLSP_LSP_TAG: 4070-2.13.0", workflow)
        self.assertIn("LSP:sublimelsp/LSP@${{ env.PERLLSP_LSP_TAG }}", workflow)
        # A commit SHA must never be used as the install ref again.
        self.assertNotIn(
            "LSP:sublimelsp/LSP@cc9f5201d9f053d9ab67aa0ea575b494fd133803", workflow
        )
        self.assertIn('"refs/tags/$PERLLSP_LSP_TAG^{}"', workflow)
        self.assertIn('if [[ "$resolved" != "$PERLLSP_LSP_REF" ]]; then', workflow)

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
