#!/usr/bin/env python3
"""CLI entrypoint for managed worktrees and exact Cargo retirement handoff."""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path
from typing import Any


BUNDLE_FILES = ("worktree_manager.py", "cargo_retirement.py", "cargo_admitted.py")
BUNDLE_ROOT_ENV = "WORKTREE_MANAGER_MODULE_ROOT"


def _ensure_bundle() -> tuple[Path, bool]:
    """Keep the entrypoint importable when hermetic fixtures copy it alone.

    Normal repository use finds all modules beside this entrypoint. Existing
    hermetic shell fixtures intentionally copy only the entrypoint into a
    throwaway Git repository; those fixtures set BUNDLE_ROOT_ENV so the exact
    checked-in bundle is copied into that fixture before import. No network or
    package installation is involved.
    """

    destination = Path(__file__).resolve().parent
    missing = [name for name in BUNDLE_FILES if not (destination / name).is_file()]
    if not missing:
        return destination, True
    source_raw = os.environ.get(BUNDLE_ROOT_ENV)
    if not source_raw:
        # Preserve the historical standalone-import contract used by the bash
        # resolver test. Full CLI operation still fails closed in main().
        return destination, False
    source = Path(source_raw).resolve()
    for name in missing:
        candidate = source / name
        if not candidate.is_file() or candidate.is_symlink():
            raise RuntimeError("worktree-manager fixture bundle source is not a regular file: " + str(candidate))
        temporary = destination / (name + ".tmp")
        shutil.copyfile(candidate, temporary)
        os.replace(temporary, destination / name)
    return destination, True


MODULE_ROOT, BUNDLE_AVAILABLE = _ensure_bundle()
if str(MODULE_ROOT) not in sys.path:
    sys.path.insert(0, str(MODULE_ROOT))

if BUNDLE_AVAILABLE:
    import cargo_retirement as _retirement  # noqa: E402
    import worktree_manager as _manager  # noqa: E402

    # Preserve the import surface used by existing tests and local tooling while
    # moving the hyphenated executable into a small entrypoint module.
    for _name in dir(_manager):
        if not _name.startswith("__"):
            globals().setdefault(_name, getattr(_manager, _name))
else:
    _manager = None
    _retirement = None

    def _bash_spawn(script: Path) -> list[str]:
        return ["bash", Path(script).as_posix()]


def _retirement_reason(slot: dict[str, Any], branch: str, stale_days: int) -> tuple[bool, str]:
    if slot.get("status") == "retired":
        return True, "retired"
    if _manager.stale_slot(slot, stale_days):
        return True, "stale"
    if slot.get("status") == "idle" and branch != "HEAD":
        if _manager.branch_merged(branch) and _manager.open_pr_number(branch) is None:
            return True, "idle_merged_without_open_pr"
    return False, "keep"


def _plan_retirement(
    slot_path: Path,
    slot: dict[str, Any],
    state_path: Path,
    repo_root: Path,
    reason: str,
    *,
    created_at: str | None = None,
) -> dict[str, Any]:
    return _retirement.plan_retirement(
        slot_path,
        repo_root=repo_root,
        slot_record=slot,
        state_path=state_path,
        cleanup_reason=reason,
        env=os.environ,
        created_at=created_at,
    )


def cleanup(
    args: Any,
    state: dict[str, Any],
    state_path: Path,
    repo_root: Path,
) -> None:
    """Prune only after an exact, durable external-Cargo-state handoff."""

    slots = state.get("slots", [])
    kept: list[dict[str, Any]] = []
    removed = 0
    pruned = 0

    if args.run_low_level:
        script = repo_root / "scripts" / "cleanup-completed-worktrees.sh"
        cmd = _manager._bash_spawn(script)
        if args.dry_run:
            cmd.append("--dry-run")
        proc = _manager.run(cmd, check=False)
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        if proc.returncode != 0:
            raise RuntimeError("low-level cleanup script failed")

    for original_slot in slots:
        slot = dict(original_slot)
        recorded = slot.get("path")
        if not recorded or not isinstance(recorded, str):
            removed += 1
            continue
        slot_path = repo_root / recorded
        if not slot_path.exists():
            # Absence is not a Cargo retirement handoff. Preserve the source
            # identity for #11671 rather than erasing the only remaining clue.
            slot["status"] = "missing_no_retirement_manifest"
            print(
                f"kept missing slot={slot.get('slot_id')} path={slot.get('path')}: "
                "external Cargo retirement not proven"
            )
            kept.append(slot)
            continue

        branch = _manager.normalize_branch(slot.get("branch", "HEAD"))
        should_prune, reason = _retirement_reason(slot, branch, args.stale_days)
        if not should_prune:
            kept.append(slot)
            continue

        dirty_check = globals().get("worktree_dirty", _manager.worktree_dirty)
        if dirty_check(slot_path):
            print(f"kept dirty slot={slot.get('slot_id')} path={slot.get('path')} branch={branch}")
            kept.append(slot)
            continue

        try:
            plan = _plan_retirement(slot_path, slot, state_path, repo_root, reason)
        except _retirement.RetirementRefused as error:
            print(
                f"kept slot={slot.get('slot_id')} path={slot.get('path')} branch={branch}: "
                f"cargo_storage=NOT_PROVEN reason={error}"
            )
            kept.append(slot)
            continue

        classification = plan["classification"]
        if plan["blockers"]:
            print(
                f"kept slot={slot.get('slot_id')} path={slot.get('path')} branch={branch}: "
                f"cargo_storage={classification} retirement=REVIEW blockers="
                + ",".join(plan["blockers"])
            )
            kept.append(slot)
            continue

        if classification not in {"MANAGED_ADMITTED", "UNMANAGED"}:
            print(
                f"kept slot={slot.get('slot_id')} path={slot.get('path')} branch={branch}: "
                f"cargo_storage={classification} retirement=NOT_PROVEN"
            )
            kept.append(slot)
            continue

        if args.dry_run:
            manifest = plan["manifest_path"] or "none"
            print(
                f"would prune slot={slot.get('slot_id')} path={slot.get('path')} branch={branch} "
                f"cargo_storage={classification} manifest={manifest}"
            )
            pruned += 1
            continue

        created_at = plan["created_at"]

        def replan() -> dict[str, Any]:
            selected, fresh_reason = _retirement_reason(slot, branch, args.stale_days)
            if not selected or fresh_reason != reason:
                raise _retirement.RetirementRefused(
                    "worktree cleanup selection changed before mutation"
                )
            return _plan_retirement(
                slot_path,
                slot,
                state_path,
                repo_root,
                reason,
                created_at=created_at,
            )

        receipt: dict[str, Any] | None = None
        try:
            if classification == "MANAGED_ADMITTED":
                receipt = _retirement.write_retirement_manifest(plan, replan=replan)
            current = _retirement.revalidate_plan(plan, replan)
        except (OSError, _retirement.RetirementRefused) as error:
            print(
                f"kept slot={slot.get('slot_id')} path={slot.get('path')} branch={branch}: "
                f"cargo_storage={classification} retirement handoff failed: {error}"
            )
            kept.append(slot)
            continue

        git_command = globals().get("git", _manager.git)
        dirty_check = globals().get("worktree_dirty", _manager.worktree_dirty)
        removal = git_command(["worktree", "remove", str(slot_path)], check=False)
        if removal.returncode != 0 and not dirty_check(slot_path):
            removal = git_command(["worktree", "remove", "--force", str(slot_path)], check=False)
        if removal.returncode != 0:
            if receipt is not None:
                slot["cargo_retirement_manifest"] = receipt
                slot["cargo_retirement_state"] = "manifest_written_worktree_retained"
            else:
                slot["cargo_retirement_state"] = "unmanaged_worktree_removal_failed"
            # Preserve the original lifecycle status so the same retired/stale
            # candidate remains retryable. Manifest state is orthogonal to the
            # worktree-manager status; collapsing them would strand a failed
            # removal because stale_slot() would stop selecting it.
            manifest = receipt["path"] if receipt is not None else "none"
            print(
                f"kept slot={slot.get('slot_id')} path={slot.get('path')}: "
                f"worktree removal failed manifest={manifest}"
            )
            kept.append(slot)
            continue

        if classification == "MANAGED_ADMITTED":
            try:
                verification = _retirement.verify_retained_assets(current)
            except _retirement.RetirementRefused as error:
                slot["cargo_retirement_manifest"] = receipt
                slot["status"] = "retirement_review_after_worktree_removal"
                slot["retirement_review_reason"] = str(error)
                print(
                    f"review slot={slot.get('slot_id')} path={slot.get('path')}: "
                    f"worktree removed but retained Cargo identity verification failed: {error}"
                )
                kept.append(slot)
                continue
        else:
            verification = {
                "verdict": "NO_EXTERNAL_MANAGED_ASSETS_IDENTIFIED",
                "reclaim_authorized": False,
            }

        if branch != "HEAD":
            git_command(["branch", "-D", branch], check=False)
        manifest = receipt["path"] if receipt is not None else "none"
        print(
            f"pruned slot={slot.get('slot_id')} path={slot.get('path')} branch={branch} "
            f"cargo_storage={classification} manifest={manifest} retained={verification['verdict']}"
        )
        pruned += 1

    state["slots"] = kept
    if not args.dry_run:
        _manager.save_state(state_path, state)
    print(f"cleanup complete: kept={len(kept)} removed_state={removed} pruned={pruned}")


def main() -> int:
    if not BUNDLE_AVAILABLE:
        raise RuntimeError(
            "worktree-manager module bundle is incomplete. Copy this entrypoint together with "
            + ", ".join(BUNDLE_FILES)
            + f", or set {BUNDLE_ROOT_ENV} for a hermetic fixture."
        )
    parser = _manager.build_parser()
    args = parser.parse_args()
    repo_root = _manager._resolve_primary_repo_root()
    state_path = _manager.state_path_from_args(args, repo_root)
    managed_root = _manager.managed_root_from_args(args, repo_root)

    with _manager._state_transaction(state_path):
        state = _manager.load_state(state_path)
        state["managed_root"] = _manager.render_state_path(managed_root, repo_root)
        _manager.sync_state(state, managed_root, repo_root)

        if args.command == "query":
            if args.json:
                _manager.json_query(state)
            else:
                _manager.print_query(state)
            _manager.save_state(state_path, state)
            return 0
        if args.command == "allocate":
            _manager.allocate(args, state, state_path, managed_root, repo_root)
            return 0
        if args.command == "release":
            _manager.release(args, state, state_path, repo_root)
            return 0
        if args.command == "cleanup":
            cleanup(args, state, state_path, repo_root)
            return 0

    parser.error(f"unknown command: {args.command}")
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1)
