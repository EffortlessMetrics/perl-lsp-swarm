"""Keep must-only test consumers independent of the parser test-support graph (#8605)."""

import os
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]


def resolved_test_dependencies(package):
    try:
        result = subprocess.run(
            [
                "cargo", "tree", "--manifest-path", str(ROOT / "Cargo.toml"),
                "-p", package, "--locked", "--offline", "--edges", "normal,build,dev",
                "--prefix", "none", "--format", "{p}",
            ],
            cwd=ROOT,
            env=dict(os.environ, RUSTUP_AUTO_INSTALL="0"),
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=True,
        )
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f"cargo tree failed for {package}:\n{error.stderr}") from error
    return {line.split()[0] for line in result.stdout.splitlines() if line.strip()}


class MinimalHelperDependencies(unittest.TestCase):
    def assert_minimal_helpers(self, package):
        dependencies = resolved_test_dependencies(package)
        self.assertIn("perl-test-must", dependencies)
        for forbidden in ("perl-tdd-support", "perl-parser-core"):
            self.assertNotIn(forbidden, dependencies, f"{package} regained {forbidden}")

    def test_uri_uses_minimal_helpers(self):
        self.assert_minimal_helpers("perl-uri")

    def test_core_harness_uses_minimal_helpers(self):
        self.assert_minimal_helpers("perl-core-harness")


if __name__ == "__main__":
    unittest.main()
