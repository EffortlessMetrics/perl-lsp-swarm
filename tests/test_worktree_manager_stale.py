"""Stale-slot aging and Cargo-retirement cleanup controls.

A slot whose lane died before ``release`` has ``last_released_at is None``.
``stale_slot`` must age such slots from ``last_used_at`` while cleanup must
preserve dirty worktrees, durably hand off managed Cargo assets before removal,
and retain enough state to retry when removal fails.
"""

from __future__ import annotations

import importlib.util
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "worktree_manager", ROOT / "scripts" / "worktree-manager.py"
)
WM = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WM)


def ts(days_ago: float) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days_ago)).isoformat()


def slot(**overrides):
    base = {
        "status": "idle",
        "branch": "HEAD",
        "last_released_at": None,
        "last_used_at": None,
    }
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
        self.assertTrue(
            WM.stale_slot(slot(last_released_at=ts(30), last_used_at=ts(31)), 7)
        )

    def test_released_recent_slot_is_not_stale_even_if_used_long_ago(self):
        self.assertFalse(
            WM.stale_slot(slot(last_released_at=ts(1), last_used_at=ts(90)), 7)
        )

    def test_malformed_timestamp_is_not_stale(self):
        self.assertFalse(WM.stale_slot(slot(last_used_at="not-a-date"), 7))

    def test_retired_slot_is_not_stale_via_aging(self):
        self.assertFalse(
            WM.stale_slot(slot(status="retired", last_used_at=ts(90)), 7)
        )

    def test_owned_active_slot_is_never_stale(self):
        self.assertFalse(
            WM.stale_slot(
                slot(status="active", owner="lane-a", last_used_at=ts(90)), 7
            )
        )

    def test_owned_released_slot_is_never_stale(self):
        self.assertFalse(
            WM.stale_slot(slot(owner="lane-a", last_released_at=ts(90)), 7)
        )

    def test_ownerless_active_old_slot_is_stale(self):
        self.assertTrue(
            WM.stale_slot(slot(status="active", last_used_at=ts(90)), 7)
        )


class CleanupRetirementGuardTests(unittest.TestCase):
    """Cleanup hands off exact external Cargo state before Git removal."""

    def _run_cleanup(
        self,
        *,
        dirty: bool,
        removal_ok: bool = True,
        blockers: tuple[str, ...] = (),
        dry_run: bool = False,
        classification: str = "MANAGED_ADMITTED",
        selection_changes: bool = False,
    ):
        import tempfile

        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(__import__("shutil").rmtree, tmp, True)
        wt = tmp / "slot-a"
        wt.mkdir()
        (wt / "tracked.txt").write_text("dirty\n" if dirty else "clean\n")
        record = {
            "slot_id": "slot-a",
            "path": "slot-a",
            "branch": "HEAD",
            "status": "idle",
            "owner": None,
            "last_released_at": None,
            "last_used_at": ts(90),
        }
        state = {"slots": [record]}
        calls: list[tuple[str, object]] = []
        manifest_path = tmp / "cargo-retirements" / "manifest.json"
        plan = {
            "classification": classification,
            "manifest_path": str(manifest_path) if classification == "MANAGED_ADMITTED" else None,
            "cleanup_plan_fingerprint": "a" * 64,
            "manifest": {"created_at": "2026-11-09T00:00:00Z"},
            "evidence": {"created_at": "2026-11-09T00:00:00Z"},
            "created_at": "2026-11-09T00:00:00Z",
            "blockers": list(blockers),
            "retained_asset_observations": {},
        }

        class Result:
            def __init__(self, returncode: int):
                self.returncode = returncode

        def fake_git(args, check=False):
            calls.append(("git", tuple(args)))
            return Result(0 if removal_ok else 1)

        def fake_plan(*args, **kwargs):
            calls.append(("plan", kwargs.get("created_at")))
            return plan

        def fake_write(candidate, *, replan):
            calls.append(("write", candidate["cleanup_plan_fingerprint"]))
            # Exercise the callback contract: a writer must re-observe before
            # publishing, not trust the stale object passed by cleanup.
            replan()
            return {
                "path": str(manifest_path),
                "sha256": "b" * 64,
                "bytes": 1,
                "reused_existing": False,
            }

        def fake_revalidate(candidate, replan):
            calls.append(("revalidate", candidate["cleanup_plan_fingerprint"]))
            replan()
            return candidate

        def fake_verify(candidate):
            calls.append(("verify", candidate["cleanup_plan_fingerprint"]))
            return {"verdict": "RETAINED_IDENTITIES_MATCH", "reclaim_authorized": False}

        selection_count = 0

        def fake_reason(slot_record, branch, stale_days):
            nonlocal selection_count
            selection_count += 1
            if selection_changes and selection_count > 1:
                return False, "keep"
            return True, "stale"

        replacements = {
            "worktree_dirty": lambda path: dirty,
            "git": fake_git,
            "_plan_retirement": fake_plan,
            "_retirement_reason": fake_reason,
        }
        original_globals = {name: getattr(WM, name) for name in replacements}
        original_write = WM._retirement.write_retirement_manifest
        original_revalidate = WM._retirement.revalidate_plan
        original_verify = WM._retirement.verify_retained_assets
        try:
            for name, value in replacements.items():
                setattr(WM, name, value)
            WM._retirement.write_retirement_manifest = fake_write
            WM._retirement.revalidate_plan = fake_revalidate
            WM._retirement.verify_retained_assets = fake_verify
            WM.cleanup(
                SimpleNamespace(stale_days=7, dry_run=dry_run, run_low_level=False),
                state,
                tmp / "state.json",
                tmp,
            )
        finally:
            for name, value in original_globals.items():
                setattr(WM, name, value)
            WM._retirement.write_retirement_manifest = original_write
            WM._retirement.revalidate_plan = original_revalidate
            WM._retirement.verify_retained_assets = original_verify
        return state, calls

    def test_cleanup_keeps_dirty_stale_slot_without_planning(self):
        state, calls = self._run_cleanup(dirty=True)
        self.assertEqual([s["slot_id"] for s in state["slots"]], ["slot-a"])
        self.assertFalse(any(name == "plan" for name, _ in calls))

    def test_cleanup_prunes_only_after_manifest_and_identity_verification(self):
        state, calls = self._run_cleanup(dirty=False)
        self.assertEqual(state["slots"], [])
        names = [name for name, _ in calls]
        self.assertLess(names.index("write"), names.index("git"))
        self.assertLess(names.index("git"), names.index("verify"))

    def test_cleanup_keeps_blocked_retirement_without_manifest_or_git(self):
        state, calls = self._run_cleanup(
            dirty=False, blockers=("active_or_unsettled_cargo_lease",)
        )
        self.assertEqual([s["slot_id"] for s in state["slots"]], ["slot-a"])
        self.assertFalse(any(name in {"write", "git", "verify"} for name, _ in calls))

    def test_cleanup_keeps_retryable_slot_when_removal_fails(self):
        state, calls = self._run_cleanup(dirty=False, removal_ok=False)
        self.assertEqual([s["slot_id"] for s in state["slots"]], ["slot-a"])
        retained = state["slots"][0]
        self.assertEqual("idle", retained["status"])
        self.assertEqual(
            "manifest_written_worktree_retained", retained["cargo_retirement_state"]
        )
        self.assertIn("cargo_retirement_manifest", retained)
        self.assertFalse(any(name == "verify" for name, _ in calls))

    def test_cleanup_revalidates_selection_before_git_mutation(self):
        state, calls = self._run_cleanup(dirty=False, selection_changes=True)
        self.assertEqual([s["slot_id"] for s in state["slots"]], ["slot-a"])
        self.assertFalse(any(name == "git" for name, _ in calls))

    def test_cleanup_prunes_unmanaged_slot_without_manifest(self):
        state, calls = self._run_cleanup(dirty=False, classification="UNMANAGED")
        self.assertEqual(state["slots"], [])
        names = [name for name, _ in calls]
        self.assertNotIn("write", names)
        self.assertIn("git", names)
        self.assertNotIn("verify", names)

    def test_cleanup_dry_run_writes_nothing_and_removes_nothing(self):
        state, calls = self._run_cleanup(dirty=False, dry_run=True)
        self.assertEqual(state["slots"], [])
        self.assertFalse(any(name in {"write", "git", "verify"} for name, _ in calls))


if __name__ == "__main__":
    unittest.main()
