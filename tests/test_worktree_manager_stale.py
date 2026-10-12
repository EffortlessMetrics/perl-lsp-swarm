"""Stale-slot aging for never-released worktree-manager slots (#17673 C5, #11606).

A slot whose lane died before `release` has `last_released_at is None`.
`stale_slot` must age such slots from `last_used_at` (set at allocate)
instead of treating them as never stale, while released slots keep
aging from `last_released_at`.
"""

from __future__ import annotations

import importlib.util
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "worktree_manager", ROOT / "scripts" / "worktree-manager.py"
)
WM = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WM)


def ts(days_ago: float) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days_ago)).isoformat()


def slot(**overrides):
    base = {"status": "idle", "branch": "HEAD", "last_released_at": None,
            "last_used_at": None}
    base.update(overrides)
    return base


class StaleSlotTests(unittest.TestCase):
    def test_never_released_old_slot_is_stale(self):
        self.assertTrue(WM.stale_slot(slot(last_used_at=ts(30)), 7))

    def test_never_released_recent_slot_is_not_stale(self):
        self.assertFalse(WM.stale_slot(slot(last_used_at=ts(1)), 7))

    def test_never_released_without_timestamps_is_not_stale(self):
        self.assertFalse(WM.stale_slot(slot(), 7))

    def test_released_old_slot_is_stale(self):
        self.assertTrue(WM.stale_slot(
            slot(last_released_at=ts(30), last_used_at=ts(31)), 7))

    def test_released_recent_slot_is_not_stale_even_if_used_long_ago(self):
        self.assertFalse(WM.stale_slot(
            slot(last_released_at=ts(1), last_used_at=ts(90)), 7))

    def test_malformed_timestamp_is_not_stale(self):
        self.assertFalse(WM.stale_slot(slot(last_used_at="not-a-date"), 7))

    def test_retired_slot_is_not_stale_via_aging(self):
        self.assertFalse(WM.stale_slot(
            slot(status="retired", last_used_at=ts(90)), 7))


if __name__ == "__main__":
    unittest.main()
