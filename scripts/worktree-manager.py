#!/usr/bin/env python3
"""Manage local agent worktrees under the repository root."""

from __future__ import annotations

import argparse
import contextlib
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from typing import Any

WORKTREES_DIRNAME = ".agent-worktrees"
STATE_DIRNAME = ".worktree-manager"
STATE_FILENAME = "state.json"
LOCK_FILENAME = "state.lock"
STATE_VERSION = 1
ALLOWED_KINDS = {"pr", "issue", "task"}
SLOT_MIN = 1
SLOT_MAX = 9999

JsonObject = dict[str, Any]


class ManagerError(RuntimeError):
    """User-facing worktree manager failure."""


def run(
    args: list[str],
    *,
    cwd: Path,
    capture: bool = True,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        text=True,
        capture_output=capture,
        check=check,
    )


def discover_repo_root() -> Path:
    result = run(["git", "rev-parse", "--show-toplevel"], cwd=Path.cwd())
    return Path(result.stdout.strip()).resolve()


def current_branch(repo_root: Path) -> str:
    result = run(["git", "branch", "--show-current"], cwd=repo_root)
    return result.stdout.strip()


def normalize_repo_relative(repo_root: Path, path: Path) -> str:
    return path.resolve().relative_to(repo_root).as_posix()


def validate_kind(value: str) -> str:
    normalized = value.strip()
    if normalized not in ALLOWED_KINDS:
        allowed = ", ".join(sorted(ALLOWED_KINDS))
        raise argparse.ArgumentTypeError(f"kind must be one of: {allowed}")
    return normalized


def validate_slug(value: str) -> str:
    normalized = value.strip()
    if not normalized:
        raise argparse.ArgumentTypeError("slug must not be empty")
    if any(char not in "abcdefghijklmnopqrstuvwxyz0123456789-" for char in normalized):
        raise argparse.ArgumentTypeError(
            "slug must contain only lowercase ASCII letters, digits, and hyphens"
        )
    if normalized.startswith("-") or normalized.endswith("-") or "--" in normalized:
        raise argparse.ArgumentTypeError(
            "slug must not start/end with '-' or contain consecutive '-'"
        )
    return normalized


def validate_slot(value: str) -> int:
    try:
        slot = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("slot must be an integer") from error
    if not SLOT_MIN <= slot <= SLOT_MAX:
        raise argparse.ArgumentTypeError(f"slot must be between {SLOT_MIN} and {SLOT_MAX}")
    return slot


@contextlib.contextmanager
def locked_state(repo_root: Path):
    state_dir = repo_root / WORKTREES_DIRNAME / STATE_DIRNAME
    state_dir.mkdir(parents=True, exist_ok=True)
    lock_path = state_dir / LOCK_FILENAME
    with lock_path.open("a+", encoding="utf-8") as lock_file:
        fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)


def empty_state() -> JsonObject:
    return {"version": STATE_VERSION, "allocations": {}}


def state_path(repo_root: Path) -> Path:
    return repo_root / WORKTREES_DIRNAME / STATE_DIRNAME / STATE_FILENAME


def load_state(repo_root: Path) -> JsonObject:
    path = state_path(repo_root)
    if not path.exists():
        return empty_state()
    try:
        state = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ManagerError(f"failed to read state file {path}: {error}") from error
    if not isinstance(state, dict):
        raise ManagerError(f"state file {path} does not contain an object")
    if state.get("version") != STATE_VERSION:
        raise ManagerError(
            f"state file {path} has unsupported version {state.get('version')!r}"
        )
    allocations = state.get("allocations")
    if not isinstance(allocations, dict):
        raise ManagerError(f"state file {path} has invalid allocations")
    return state


def write_state(repo_root: Path, state: JsonObject) -> None:
    path = state_path(repo_root)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f"{STATE_FILENAME}.",
        suffix=".tmp",
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as file:
            json.dump(state, file, indent=2, sort_keys=True)
            file.write("\n")
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary_name, path)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary_name)


def validate_state_repo_root(repo_root: Path, state: JsonObject) -> None:
    recorded = state.get("repo_root")
    if recorded is not None and recorded != str(repo_root):
        raise ManagerError(
            f"state repo_root {recorded!r} does not match repository root {str(repo_root)!r}"
        )


def ensure_relative_in_repo(path_text: str) -> Path:
    path = Path(path_text)
    if path.is_absolute():
        raise ManagerError(f"refusing absolute managed path: {path_text}")
    if any(part == ".." for part in path.parts):
        raise ManagerError(f"refusing parent traversal in managed path: {path_text}")
    return path


def ensure_contained_path(repo_root: Path, path_text: str) -> Path:
    path = ensure_relative_in_repo(path_text)
    managed_root = (repo_root / WORKTREES_DIRNAME).resolve()
    target = (repo_root / path).resolve()
    if target == managed_root or managed_root not in target.parents:
        raise ManagerError(
            f"managed path escapes {WORKTREES_DIRNAME}: {path_text}"
        )
    return target


def ensure_git_repository(repo_root: Path, path: Path) -> None:
    try:
        result = run(["git", "rev-parse", "--show-toplevel"], cwd=path)
    except subprocess.CalledProcessError as error:
        message = error.stderr.strip() or error.stdout.strip() or str(error)
        raise ManagerError(f"{path} is not a Git worktree: {message}") from error
    actual_root = Path(result.stdout.strip()).resolve()
    if actual_root != path.resolve():
        raise ManagerError(
            f"managed path {path} reports unexpected Git root {actual_root}"
        )
    admin_dir = run(
        ["git", "rev-parse", "--absolute-git-dir"],
        cwd=path,
    ).stdout.strip()
    common_dir = run(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=path,
    ).stdout.strip()
    admin_path = Path(admin_dir).resolve()
    common_path = Path(common_dir)
    if not common_path.is_absolute():
        common_path = (path / common_path).resolve()
    else:
        common_path = common_path.resolve()
    if admin_path == common_path:
        raise ManagerError(
            f"managed path {path} is an independent repository, not a linked worktree"
        )


def worktree_is_clean(path: Path) -> bool:
    result = run(
        ["git", "status", "--porcelain", "--untracked-files=all"],
        cwd=path,
    )
    return result.stdout == ""


def branch_exists(repo_root: Path, branch: str) -> bool:
    result = run(
        ["git", "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"],
        cwd=repo_root,
        check=False,
    )
    return result.returncode == 0


def branch_checked_out_at(repo_root: Path, branch: str) -> Path | None:
    result = run(["git", "worktree", "list", "--porcelain"], cwd=repo_root)
    worktree: Path | None = None
    for line in result.stdout.splitlines():
        if line.startswith("worktree "):
            worktree = Path(line.removeprefix("worktree ")).resolve()
        elif line == f"branch refs/heads/{branch}" and worktree is not None:
            return worktree
    return None


def prune_stale_allocations(repo_root: Path, state: JsonObject) -> bool:
    removed = False
    allocations = state["allocations"]
    for slot, entry in list(allocations.items()):
        try:
            path = ensure_contained_path(repo_root, entry["path"])
            path_exists = path.exists() or path.is_symlink()
            # Only validate a path that is still present. Probing a removed
            # directory raises OSError from the subprocess cwd, which would
            # abort list/allocate/release instead of pruning the stale entry.
            if path_exists:
                ensure_git_repository(repo_root, path)
        except (KeyError, TypeError, OSError, ManagerError):
            path_exists = False
        if not path_exists:
            allocations.pop(slot, None)
            removed = True
    return removed


def allocation_payload(entry: JsonObject) -> JsonObject:
    return {
        "slot": entry["slot"],
        "kind": entry["kind"],
        "id": entry["id"],
        "slug": entry["slug"],
        "branch": entry["branch"],
        "path": entry["path"],
        "base_ref": entry["base_ref"],
        "owner": entry.get("owner"),
    }


def allocate(args: argparse.Namespace, repo_root: Path) -> JsonObject:
    slot = args.slot
    owner = args.owner.strip() if args.owner else None
    branch = f"agent/{args.kind}-{args.id}-{args.slug}"
    path_relative = (
        Path(WORKTREES_DIRNAME)
        / f"{slot:04d}-{args.kind}-{args.id}-{args.slug}"
    ).as_posix()
    path = ensure_contained_path(repo_root, path_relative)

    git_args = ["git", "worktree", "add"]
    if args.create_branch:
        git_args.extend(["-b", branch, str(path), args.base])
    else:
        existing = branch_checked_out_at(repo_root, branch)
        if existing is not None:
            raise ManagerError(f"branch {branch} is already checked out at {existing}")
        git_args.extend([str(path), branch])

    with locked_state(repo_root):
        state = load_state(repo_root)
        validate_state_repo_root(repo_root, state)
        state["repo_root"] = str(repo_root)
        changed = prune_stale_allocations(repo_root, state)
        key = str(slot)
        if key in state["allocations"]:
            entry = state["allocations"][key]
            raise ManagerError(
                f"slot {slot} is already allocated to "
                f"{entry.get('branch', 'an unknown branch')}"
            )
        if path.exists() or path.is_symlink():
            raise ManagerError(f"managed path already exists: {path}")
        if changed:
            write_state(repo_root, state)

        # Captured before the mutation so rollback only ever removes Git state
        # that this invocation is responsible for creating. A pre-existing
        # branch (always the case under --use-existing-branch) must survive a
        # failed allocation.
        branch_preexisting = branch_exists(repo_root, branch)

        try:
            run(git_args, cwd=repo_root)
            entry = {
                "slot": slot,
                "kind": args.kind,
                "id": str(args.id),
                "slug": args.slug,
                "branch": branch,
                "path": path_relative,
                "base_ref": args.base,
                "owner": owner,
            }
            state["allocations"][key] = entry
            write_state(repo_root, state)
        except Exception:
            # The managed path was proven absent above, so anything now at it
            # was created here and is safe to remove.
            with contextlib.suppress(subprocess.CalledProcessError):
                run(["git", "worktree", "remove", "--force", str(path)], cwd=repo_root)
            # Never force-delete a branch this invocation did not create;
            # doing so destroys another work item's unmerged commits.
            if not branch_preexisting:
                with contextlib.suppress(subprocess.CalledProcessError):
                    run(["git", "branch", "-D", branch], cwd=repo_root)
            raise

    return allocation_payload(entry)


def list_allocations(repo_root: Path) -> list[JsonObject]:
    with locked_state(repo_root):
        state = load_state(repo_root)
        validate_state_repo_root(repo_root, state)
        state["repo_root"] = str(repo_root)
        changed = prune_stale_allocations(repo_root, state)
        if changed:
            write_state(repo_root, state)
        return [
            allocation_payload(entry)
            for _, entry in sorted(
                state["allocations"].items(),
                key=lambda pair: int(pair[0]),
            )
        ]


def release(args: argparse.Namespace, repo_root: Path) -> JsonObject:
    slot = args.slot
    with locked_state(repo_root):
        state = load_state(repo_root)
        validate_state_repo_root(repo_root, state)
        state["repo_root"] = str(repo_root)
        changed = prune_stale_allocations(repo_root, state)
        key = str(slot)
        entry = state["allocations"].get(key)
        if entry is None:
            if changed:
                write_state(repo_root, state)
            raise ManagerError(f"slot {slot} is not allocated")

        recorded_owner = entry.get("owner")
        owner = args.owner.strip() if args.owner else None
        if recorded_owner and owner != recorded_owner and not args.force:
            raise ManagerError(
                f"slot {slot} is owned by {recorded_owner!r}; "
                "release as that owner or use --force"
            )

        path = ensure_contained_path(repo_root, entry["path"])
        ensure_git_repository(repo_root, path)
        if not args.force and not worktree_is_clean(path):
            raise ManagerError(
                f"managed worktree {path} is dirty; clean it or release with --force"
            )
        if normalize_repo_relative(repo_root, path) != entry["path"]:
            raise ManagerError(
                f"managed path {path} does not match recorded path {entry['path']}"
            )
        run(
            ["git", "worktree", "remove", *(["--force"] if args.force else []), str(path)],
            cwd=repo_root,
        )
        state["allocations"].pop(key)
        write_state(repo_root, state)

    return allocation_payload(entry)


def cleanup(repo_root: Path, *, force: bool) -> list[JsonObject]:
    branch = current_branch(repo_root)
    removed: list[JsonObject] = []
    with locked_state(repo_root):
        state = load_state(repo_root)
        validate_state_repo_root(repo_root, state)
        state["repo_root"] = str(repo_root)
        changed = prune_stale_allocations(repo_root, state)
        if not branch:
            if changed:
                write_state(repo_root, state)
            return removed

        for key, entry in list(state["allocations"].items()):
            if entry.get("branch") != branch:
                continue
            path = ensure_contained_path(repo_root, entry["path"])
            ensure_git_repository(repo_root, path)
            if not force and not worktree_is_clean(path):
                raise ManagerError(
                    f"managed worktree {path} is dirty; clean it or use --force"
                )
            run(
                [
                    "git",
                    "worktree",
                    "remove",
                    *(["--force"] if force else []),
                    str(path),
                ],
                cwd=repo_root,
            )
            state["allocations"].pop(key)
            removed.append(allocation_payload(entry))
            changed = True

        if changed:
            write_state(repo_root, state)
    return removed


def print_json(value: Any) -> None:
    json.dump(value, sys.stdout, sort_keys=True)
    sys.stdout.write("\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        help=argparse.SUPPRESS,
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    allocate_parser = subparsers.add_parser(
        "allocate",
        help="create a numbered repository-root worktree",
    )
    allocate_parser.add_argument("--slot", required=True, type=validate_slot)
    allocate_parser.add_argument("--kind", required=True, type=validate_kind)
    allocate_parser.add_argument("--id", required=True)
    allocate_parser.add_argument("--slug", required=True, type=validate_slug)
    allocate_parser.add_argument("--base", default="origin/main")
    allocate_parser.add_argument("--owner")
    allocation_mode = allocate_parser.add_mutually_exclusive_group()
    allocation_mode.add_argument(
        "--create-branch",
        dest="create_branch",
        action="store_true",
        default=True,
        help="create the canonical branch (default)",
    )
    allocation_mode.add_argument(
        "--use-existing-branch",
        dest="create_branch",
        action="store_false",
        help="check out an existing canonical branch",
    )

    list_parser = subparsers.add_parser(
        "list",
        help="list current manager allocations",
    )
    list_parser.set_defaults()

    release_parser = subparsers.add_parser(
        "release",
        help="remove one managed worktree",
    )
    release_parser.add_argument("--slot", required=True, type=validate_slot)
    release_parser.add_argument("--owner")
    release_parser.add_argument("--force", action="store_true")

    cleanup_parser = subparsers.add_parser(
        "cleanup-current",
        help="remove the managed worktree for the current branch",
    )
    cleanup_parser.add_argument("--force", action="store_true")

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        repo_root = (
            args.repo_root.resolve()
            if args.repo_root is not None
            else discover_repo_root()
        )
        if args.command == "allocate":
            print_json(allocate(args, repo_root))
        elif args.command == "list":
            print_json(list_allocations(repo_root))
        elif args.command == "release":
            print_json(release(args, repo_root))
        elif args.command == "cleanup-current":
            print_json(cleanup(repo_root, force=args.force))
        else:
            parser.error(f"unsupported command: {args.command}")
    except (ManagerError, subprocess.CalledProcessError, OSError) as error:
        print(f"worktree-manager: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
