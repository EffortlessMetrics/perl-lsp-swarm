from __future__ import annotations

from copy import deepcopy
from pathlib import Path
import sys
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from check_dist_shadow import validate_contract  # noqa: E402


RELEASE = """
strategy:
  matrix:
    include:
      - target: x86_64-unknown-linux-gnu
        os: ubuntu-22.04
      - target: aarch64-unknown-linux-gnu
        os: ubuntu-22.04
run: |
  $BUILD_CMD build --locked --release --target ${{ matrix.target }} -p perllsp --bin perllsp
  $BUILD_CMD build --locked --release --target ${{ matrix.target }} -p perl-dap --bin perl-dap
"""


def valid_dist() -> dict:
    return {
        "workspace": {"members": ["cargo:."]},
        "dist": {
            "include": ["perllsp", "perl-dap"],
            "targets": [
                "x86_64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
            ],
            "ci": "github",
            "installers": [],
            "cargo-dist-version": "0.29.0",
            "pr-run-mode": "skip",
            "checksum": "sha256",
            "github-releases": {"create": False},
        },
    }


class DistShadowTests(unittest.TestCase):
    def test_valid_contract(self) -> None:
        self.assertEqual([], validate_contract(valid_dist(), RELEASE))

    def test_missing_live_target_is_rejected(self) -> None:
        data = valid_dist()
        data["dist"]["targets"] = ["x86_64-unknown-linux-gnu"]
        errors = validate_contract(data, RELEASE)
        self.assertTrue(any("dist.targets" in error for error in errors))

    def test_stale_app_name_is_rejected(self) -> None:
        data = valid_dist()
        data["dist"]["include"] = ["perl-lsp", "perl-dap"]
        errors = validate_contract(data, RELEASE)
        self.assertTrue(any("stale dist contract token" in error for error in errors))

    def test_publish_authority_is_rejected(self) -> None:
        data = valid_dist()
        data["dist"]["github-releases"]["create"] = True
        errors = validate_contract(data, RELEASE)
        self.assertTrue(any("create must be false" in error for error in errors))

    def test_installer_drift_is_rejected(self) -> None:
        data = valid_dist()
        data["dist"]["installers"] = ["shell"]
        errors = validate_contract(data, RELEASE)
        self.assertTrue(any("installers must be empty" in error for error in errors))

    def test_release_package_binary_divergence_is_rejected(self) -> None:
        release = RELEASE.replace("-p perllsp --bin perllsp", "-p perl-lsp --bin perllsp")
        errors = validate_contract(valid_dist(), release)
        self.assertTrue(any("package/bin names diverge" in error for error in errors))

    def test_duplicate_target_is_rejected(self) -> None:
        data = deepcopy(valid_dist())
        data["dist"]["targets"].append("x86_64-unknown-linux-gnu")
        errors = validate_contract(data, RELEASE)
        self.assertTrue(any("must not contain duplicates" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
