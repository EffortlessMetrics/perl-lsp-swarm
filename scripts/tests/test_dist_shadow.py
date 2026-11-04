from __future__ import annotations

from copy import deepcopy
from pathlib import Path
import re
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
    def test_live_macos_release_rows_use_latest_native_runners(self) -> None:
        # Official standard-runner mappings, verified 2026-10-11:
        # https://github.com/actions/runner-images#available-images
        # macos-latest is ARM64; the latest standard Intel image needs its
        # explicit label. This checks routing, not macOS build compatibility.
        workflow = (SCRIPTS.parent / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        build = workflow.split("\n  build:\n", 1)[1].split("\n    steps:\n", 1)[0]
        self.assertIn("runs-on: ${{ matrix.os }}", build)
        include = build.split("\n        include:\n", 1)[1]
        rows = []
        for block in re.split(r"(?m)^[ \t]*-[ \t]+", include)[1:]:
            row = {}
            for field in ("target", "os", "use_cross"):
                values = re.findall(
                    rf"(?m)^[ \t]*{field}:[ \t]*(\S+)[ \t]*(?:#.*)?$", block
                )
                self.assertEqual(len(values), 1, f"ambiguous {field}: {block}")
                row[field] = values[0]
            if row["target"].endswith("apple-darwin") or row["os"].startswith("macos-"):
                rows.append((row["target"], row["os"], row["use_cross"]))
        self.assertCountEqual(
            rows,
            [
                ("x86_64-apple-darwin", "macos-26-intel", "false"),
                ("aarch64-apple-darwin", "macos-latest", "false"),
            ],
        )

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
