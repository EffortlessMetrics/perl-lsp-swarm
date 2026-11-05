"""On-disk unreleased Changie fragments must declare `project:` (#18411).

Full checker re-validation of every on-disk fragment remains #18410
(`xtask/src/tasks/changelog.rs`); this scan is the cheap fleet guard.
"""

from __future__ import annotations

import shutil
import subprocess
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
UNRELEASED = REPO_ROOT / ".changes" / "unreleased"
CHANGIE_CONFIG = REPO_ROOT / ".changie.yaml"

# The three fragments that lacked `project:` on main (issue #18411).
REPAIRED_FRAGMENTS = (
    "product-12581-Fixed-165152.yaml",
    "product-16602-Fixed-config-error-kind.yaml",
    "product-16603-Fixed-054627.yaml",
)


def _fragment_declares_project(path: Path) -> bool:
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("project:"):
            stripped = line.removeprefix("project:").strip()
            return bool(stripped)
    return False


class UnreleasedChangieProjectTests(unittest.TestCase):
    def test_every_unreleased_yaml_declares_project(self) -> None:
        missing: list[str] = []
        for path in sorted(UNRELEASED.glob("*.yaml")):
            if not _fragment_declares_project(path):
                missing.append(path.name)
        self.assertEqual(
            missing,
            [],
            "every `.changes/unreleased/*.yaml` must declare a non-empty `project:`",
        )

    def test_repaired_fragments_name_product_project(self) -> None:
        for name in REPAIRED_FRAGMENTS:
            path = UNRELEASED / name
            self.assertTrue(path.is_file(), f"missing fixture {name}")
            text = path.read_text(encoding="utf-8")
            self.assertRegex(
                text,
                r"(?m)^project:\s*product\s*$",
                f"{name} must declare `project: product`",
            )


class ChangieBatchRenderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.changie = shutil.which("changie")

    def test_product_batch_dry_run_includes_repaired_fragments(self) -> None:
        if self.changie is None:
            self.skipTest("changie not on PATH")
        if not CHANGIE_CONFIG.is_file():
            self.skipTest(".changie.yaml missing")

        proc = subprocess.run(
            [
                self.changie,
                "batch",
                "v0.0.0-unreleased-project-scan",
                "--project",
                "product",
                "--dry-run",
                "--keep",
            ],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        combined = proc.stdout + proc.stderr
        self.assertEqual(
            proc.returncode,
            0,
            f"changie batch --project product --dry-run failed: {combined}",
        )
        for name in REPAIRED_FRAGMENTS:
            fragment = (UNRELEASED / name).read_text(encoding="utf-8")
            marker = fragment.split('"', 2)[1][:40]
            self.assertIn(
                marker,
                combined,
                f"batch output must include body from {name}",
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
