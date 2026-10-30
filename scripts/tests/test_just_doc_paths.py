"""Exercise the real doc-path recipe without compiling repository tooling."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[2] / "justfile"


class DocPathRecipeTests(unittest.TestCase):
    def invoke(self, status=0):
        with tempfile.TemporaryDirectory(prefix="doc-path-recipe-") as directory:
            root = Path(directory)
            shutil.copyfile(SOURCE, root / "justfile")
            tools = root / "bin"
            tools.mkdir()
            cargo = tools / "cargo"
            cargo.write_text(
                f"#!{sys.executable}\n"
                "import json, os, sys\n"
                "with open(os.environ['ARGUMENT_LOG'], 'a', encoding='utf-8') as log:\n"
                "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
                "print('checker output')\n"
                "print('checker diagnostic', file=sys.stderr)\n"
                "sys.exit(int(os.environ['CHECKER_STATUS']))\n",
                encoding="utf-8",
            )
            cargo.chmod(0o755)
            log = root / "arguments.jsonl"
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                       ARGUMENT_LOG=str(log), CHECKER_STATUS=str(status))
            result = subprocess.run(
                ["just", "--justfile", str(root / "justfile"), "ci-doc-paths"],
                cwd=root, env=env, capture_output=True, text=True, timeout=20,
            )
            calls = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
            return result, calls

    def test_invokes_only_the_owning_package_with_the_complete_docs_scope(self):
        result, calls = self.invoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(calls, [[
            "run", "--quiet", "-p", "perl-ci-hygiene", "--bin", "perl-ci-hygiene",
            "--locked", "--", "check-doc-paths", "docs",
        ]])
        self.assertIn("checker output", result.stdout)
        self.assertIn("checker diagnostic", result.stderr)
        self.assertIn("Documentation paths check passed", result.stdout)

    def test_child_failure_stops_the_recipe_before_its_success_message(self):
        result, calls = self.invoke(status=37)
        self.assertEqual(len(calls), 1)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("37", result.stderr)
        self.assertIn("checker diagnostic", result.stderr)
        self.assertNotIn("Documentation paths check passed", result.stdout)


if __name__ == "__main__":
    unittest.main()
