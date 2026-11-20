"""Exercise the local inventory command against a disposable tracked tree."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


class InventoryTest(unittest.TestCase):
    def test_inventory_and_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = Path("scripts/ci/python_self_tests.py")
            (root / script.parent).mkdir(parents=True)
            shutil.copyfile(Path(__file__).with_name(script.name), root / script)
            tests = root / "scripts/tests"
            tests.mkdir()
            (root / ".ci").mkdir()
            manifest = root / ".ci/python-self-tests.tsv"
            manifest.write_text("test_a.py\tlocal\ntest-a.py\tlocal\n")
            (tests / "test_a.py").write_text("raise SystemExit(0)\n")
            (tests / "test-a.py").write_text("raise SystemExit(37)\n")
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            subprocess.run(["git", "add", "."], cwd=root, check=True)

            def invoke(*args):
                return subprocess.run([sys.executable, script, *args], cwd=root,
                                      capture_output=True, text=True)

            listed = invoke()
            self.assertEqual(listed.returncode, 0, listed.stderr)
            self.assertEqual(listed.stdout, "test-a.py\tlocal\ntest_a.py\tlocal\n")
            self.assertEqual(invoke("--run", "test_a.py").returncode, 0)
            self.assertEqual(invoke("--run", "test-a.py").returncode, 37)
            manifest.write_text("test_a.py\ttool\ntest-a.py\tlocal\n")
            invalid = invoke()
            self.assertNotEqual(invalid.returncode, 0)
            self.assertIn("Invalid disposition", invalid.stderr)
            manifest.write_text("test_a.py\tlocal\ntest-a.py\tlocal\n")
            hook = "test_pre_push_validation_child.py"
            (tests / hook).write_text("raise AssertionError('requires the Rust fixture')\n")
            subprocess.run(["git", "add", str(tests / hook)], cwd=root, check=True)
            with manifest.open("a") as stream:
                stream.write(f"{hook}\trust-generated-hook-fixture\n")
            self.assertEqual(invoke().returncode, 0)
            blocked = invoke("--run", hook)
            self.assertNotEqual(blocked.returncode, 0)
            self.assertIn("unavailable for direct execution: rust-generated-hook-fixture", blocked.stderr)
            (tests / "new.py").touch()
            subprocess.run(["git", "add", "scripts/tests/new.py"], cwd=root, check=True)
            missing = invoke("--run", "test_a.py")
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn("Missing execution disposition: new.py", missing.stderr)


if __name__ == "__main__":
    unittest.main()
