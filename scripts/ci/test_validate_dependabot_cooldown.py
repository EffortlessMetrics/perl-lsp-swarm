#!/usr/bin/env python3
"""Falsifiers for the 14-day Dependabot cooldown contract."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from validate_dependabot_contract import CONFIG_PATH, MANAGEMENT_GUIDE, QUICK_REFERENCE  # noqa: E402
from validate_dependabot_cooldown import validate  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
SURFACES = (CONFIG_PATH, MANAGEMENT_GUIDE, QUICK_REFERENCE)


def _clone(tmp: Path) -> None:
    for rel in SURFACES:
        src = ROOT / rel
        dest = tmp / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(src.read_bytes())


def _rewrite(tmp: Path, rel: Path, old: str, new: str, *, count: int = 1) -> None:
    path = tmp / rel
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise AssertionError(f"fixture marker missing from {rel}: {old!r}")
    path.write_text(text.replace(old, new, count), encoding="utf-8")


class DependabotCooldownTests(unittest.TestCase):
    def test_current_surfaces_pass(self) -> None:
        self.assertEqual(validate(ROOT), [])

    def test_default_days_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone(tmp)
            _rewrite(tmp, CONFIG_PATH, "      default-days: 14\n", "      default-days: 13\n")
            self.assertTrue(any(item.startswith("cooldown-drift:") for item in validate(tmp)))

    def test_missing_cooldown_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone(tmp)
            _rewrite(
                tmp,
                CONFIG_PATH,
                "    cooldown:\n      default-days: 14\n",
                "",
            )
            self.assertTrue(any(item.startswith("cooldown-missing:") for item in validate(tmp)))

    def test_management_guide_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone(tmp)
            _rewrite(tmp, MANAGEMENT_GUIDE, "14-day", "two-week")
            self.assertTrue(any(item.startswith("cooldown-guide-drift:") for item in validate(tmp)))

    def test_quick_reference_scalar_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            tmp = Path(temp)
            _clone(tmp)
            _rewrite(tmp, QUICK_REFERENCE, "default-days: 14", "default-days: 7")
            self.assertTrue(any(item.startswith("cooldown-guide-drift:") for item in validate(tmp)))


if __name__ == "__main__":
    unittest.main()
