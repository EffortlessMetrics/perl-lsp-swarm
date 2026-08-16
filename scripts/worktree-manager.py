#!/usr/bin/env python3
"""Manage reusable git worktree slots for swarm sessions."""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Protocol


# ---------------------------------------------------------------------------
# Primary-root discovery (defect 1)
# ---------------------------------------------------------------------------
# The original `REPO_ROOT = Path(__file__).resolve().parents[1]` follows the
# *script file*, so invoking from a linked worktree at
# `<pool>/slot-N/scripts/worktree-manager.py` yields `<pool>/slot-N/` as the
# root — a different root for every slot, each with its own local slot
# database and sibling pool name.  We must resolve the *shared* git
# repository root instead.


def _resolve_primary_repo_root() -> Path:
    """Return the shared Git repository root, independent of which linked worktree invokes this script.

    ``git worktree list --porcelain`` always reports the *main* working tree
    as its first ``worktree`` record, from any linked worktree and from any
    subdirectory.  Asking git directly for the main working tree is more
    robust than deriving it from ``--git-common-dir``, which yields a
    repository directory whose layout varies: it is not named ``.git`` for a
    bare repository or when ``GIT_DIR``/``GIT_COMMON_DIR`` override the
    default, and for a submodule it points into ``<super>/.git/modules/...``
    rather than anywhere near the working tree.

    Falls back to the script-location heuristic (the original behavior) when
    git is unavailable, the script is not inside a repository, or the
    repository has no working tree at all (bare).
    """
    script_dir = Path(__file__).resolve().parent
    try:
        proc = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            cwd=str(script_dir),
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode == 0:
            for line in proc.stdout.splitlines():
                # The first `worktree <path>` record is the main working tree.
                if line.startswith("worktree "):
                    raw = line[len("worktree ") :].strip()
                    if raw:
                        return Path(raw).resolve()
                    break
    except Exception:
        pass
    # Fallback: script-location heuristic (same as original code).
    return Path(__file__).resolve().parents[1]


# Module-level constants.  PRIMARY_REPO_ROOT is the shared repository root
# resolved at import time; REPO_ROOT is a backward-compatibility alias.
PRIMARY_REPO_ROOT: Path = _resolve_primary_repo_root()
REPO_ROOT: Path = PRIMARY_REPO_ROOT

DEFAULT_STATE_FILE = PRIMARY_REPO_ROOT / ".ops-perl-lsp" / "worktree-manager" / "state.json"
DEFAULT_MANAGED_ROOT = PRIMARY_REPO_ROOT.parent / f"{PRIMARY_REPO_ROOT.name}-worktrees"
DEFAULT_BRANCH_BASES = ("origin/master", "master", "origin/main", "main")
OWNER_ENV_VARS = (
    "WORKTREE_MANAGER_OWNER",
    "CLAUDE_SUBAGENT_NAME",
    "CLAUDE_AGENT_NAME",
    "CLAUDE_TEAMMATE_NAME",
    "AGENT_NAME",
    "SUBAGENT_NAME",
)

# State-lock wait bound.  `allocate` fetches from origin while holding the
# lock, so the default is generous enough for a slow network but still finite.
LOCK_TIMEOUT_ENV_VAR = "WORKTREE_MANAGER_LOCK_TIMEOUT"
DEFAULT_LOCK_TIMEOUT_SECONDS = 120.0
LOCK_POLL_SECONDS = 0.1

# Explicit opt-in to running without any file-locking backend.  Absent this,
# a platform with neither fcntl nor msvcrt fails closed rather than silently
# running unserialized (see the platform matrix above _StateLock).
ALLOW_UNLOCKED_ENV_VAR = "WORKTREE_MANAGER_ALLOW_UNLOCKED"


def lock_timeout_seconds() -> float:
    """Return the state-lock wait bound, overridable for tests and slow links.

    A non-numeric or negative override is ignored in favor of the default
    rather than failing the command outright.
    """
    raw = os.environ.get(LOCK_TIMEOUT_ENV_VAR)
    if raw is None:
        return DEFAULT_LOCK_TIMEOUT_SECONDS
    try:
        value = float(raw)
    except ValueError:
        return DEFAULT_LOCK_TIMEOUT_SECONDS
    return value if value >= 0 else DEFAULT_LOCK_TIMEOUT_SECONDS


# ---------------------------------------------------------------------------
# State-transaction lock (defect 3)
# ---------------------------------------------------------------------------
# The original load_state / save_state used a bare read-modify-write with no
# lock and wrote directly to the final file path.  Concurrent allocations or
# releases could lose each other's updates; an interrupted write could leave
# partial JSON.
#
# Fix: one serialization boundary using a lock file adjacent to the state
# file, and an atomic temp-file + rename write.  Both locking backends (fcntl
# on POSIX, msvcrt on Windows) are imported *lazily inside the function* so
# this module can be imported on any platform without error.
#
# Serialization strength is NOT uniform across platforms, and this module does
# not pretend otherwise:
#
#   POSIX (fcntl.flock)   — PROVEN.  Exercised by Case 6, which holds the
#                           manager's own lock file from an external process
#                           and asserts the manager blocks, times out citing
#                           the lock, and proceeds once released.
#   Windows (msvcrt)      — IMPLEMENTED, NOT_PROVEN.  No Windows CI lane runs
#                           the competing-mutation control yet (issue #5444).
#                           A defect here surfaces as a raised exception, not
#                           as silent non-serialization.
#   Neither available     — FAILS CLOSED.  There is no lock at all, so a
#                           mutation would be silently unserialized; that is
#                           the one case that cannot be allowed to proceed by
#                           default.  See ``ALLOW_UNLOCKED_ENV_VAR``.


class _StateLock(Protocol):
    """Exclusive-lock contract shared by every platform backend.

    ``try_acquire`` must not block: it returns ``True`` when the lock is held
    and ``False`` when another process holds it, so the caller can bound its
    own wait (see ``_state_transaction``).
    """

    def try_acquire(self) -> bool: ...

    def release(self) -> None: ...


class _UnlockedFallback:
    """Explicitly opted-in, unserialized operation when no backend exists.

    This is *not* a lock.  It exists only so a single-process operator on a
    Python build without ``fcntl`` or ``msvcrt`` is not hard-blocked, and it
    is reachable only via ``ALLOW_UNLOCKED_ENV_VAR``.  Without that opt-in,
    ``_state_transaction`` refuses to run rather than silently dropping the
    serialization guarantee the caller is entitled to assume.
    """

    def try_acquire(self) -> bool:
        print(
            f"WARNING: {ALLOW_UNLOCKED_ENV_VAR} is set and no file-locking "
            "backend (fcntl/msvcrt) is available; state mutations are NOT "
            "serialized. Concurrent manager processes can lose each other's "
            "updates. Run one manager process at a time.",
            file=sys.stderr,
        )
        return True

    def release(self) -> None:
        pass


class _FcntlLock:
    """POSIX exclusive file lock (fcntl.flock)."""

    def __init__(self, fh: Any) -> None:
        self._fh = fh

    def try_acquire(self) -> bool:
        import fcntl as _fcntl  # noqa: PLC0415

        try:
            _fcntl.flock(self._fh, _fcntl.LOCK_EX | _fcntl.LOCK_NB)
        except OSError:
            return False
        return True

    def release(self) -> None:
        import fcntl as _fcntl  # noqa: PLC0415

        _fcntl.flock(self._fh, _fcntl.LOCK_UN)


class _MsvcrtLock:
    """Windows exclusive file lock (msvcrt.locking).

    ``msvcrt.locking`` locks a byte range rather than the whole file, so the
    lock file carries one sentinel byte and that single-byte range is locked.
    The sentinel is written only when the file is empty: the lock file is
    opened in read/write (not append) mode, but writing unconditionally would
    still rewrite the same byte on every acquisition for no benefit.
    """

    _NBYTES = 1

    def __init__(self, fh: Any) -> None:
        self._fh = fh

    def try_acquire(self) -> bool:
        import msvcrt as _msvcrt  # noqa: PLC0415

        # The sentinel write is inside the guarded block, not just the lock call.
        # Windows byte-range locks are mandatory rather than advisory, so writing
        # to a range another process already holds raises `PermissionError`. Two
        # processes racing on a not-yet-created lock file both observe size 0, and
        # whichever writes second hits that error. Letting it escape would crash
        # the manager on the first concurrent run instead of reporting contention.
        try:
            if os.fstat(self._fh.fileno()).st_size == 0:
                self._fh.seek(0)
                self._fh.write(b"L")
                self._fh.flush()
            self._fh.seek(0)
            # LK_NBLCK fails immediately instead of retrying for ~10s.
            _msvcrt.locking(self._fh.fileno(), _msvcrt.LK_NBLCK, self._NBYTES)
        except OSError:
            return False
        return True

    def release(self) -> None:
        import msvcrt as _msvcrt  # noqa: PLC0415

        self._fh.seek(0)
        _msvcrt.locking(self._fh.fileno(), _msvcrt.LK_UNLCK, self._NBYTES)


def _make_lock(fh: Any) -> _StateLock | None:
    """Return the exclusive-lock backend for *fh*, or ``None`` if there is none.

    ``None`` means this platform offers no file locking at all.  The caller
    decides what to do about that; this function does not substitute a no-op
    that would satisfy the type while dropping the guarantee.
    """
    try:
        import fcntl  # noqa: F401, PLC0415

        return _FcntlLock(fh)
    except ImportError:
        pass
    try:
        import msvcrt  # noqa: F401, PLC0415

        return _MsvcrtLock(fh)
    except ImportError:
        pass
    return None


@contextlib.contextmanager
def _state_transaction(state_path: Path):
    """Serialize state read-modify-write across concurrent manager processes.

    Opens (or creates) a lock file adjacent to ``state_path``, acquires an
    exclusive lock, and yields.  The caller is responsible for loading,
    mutating, and saving state inside the context.

    The wait is *bounded*.  ``main`` holds this lock for the whole command,
    and ``allocate`` runs ``git ls-remote`` and ``git fetch`` inside it, so a
    holder that stalls on an unreachable origin or a credential prompt would
    otherwise wedge every other manager invocation — including read-only
    ``query`` — with no output at all.  Instead each backend polls
    non-blockingly until ``LOCK_TIMEOUT_SECONDS``, reports that it is waiting,
    and then fails with an actionable error naming the lock file.

    Lock acquisition uses the platform-native mechanism (``fcntl.flock`` on
    POSIX, ``msvcrt.locking`` on Windows).  Both modules are imported lazily
    so this file can be imported cleanly on any platform — importing the
    module never requires a locking backend, only *mutating state* does.  When
    neither backend exists this fails closed with an actionable error naming
    ``ALLOW_UNLOCKED_ENV_VAR``, rather than proceeding unserialized behind a
    stderr warning a caller may never read.
    """
    lock_path = state_path.with_suffix(".lock")
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    timeout = lock_timeout_seconds()
    # Read/write, not append: append mode ignores seek(0), so the Windows
    # sentinel write would extend the lock file on every acquisition.
    lock_path.touch(exist_ok=True)
    with open(lock_path, "r+b") as lock_fh:
        lock = _make_lock(lock_fh)
        if lock is None:
            if os.environ.get(ALLOW_UNLOCKED_ENV_VAR, "") not in {"", "0"}:
                lock = _UnlockedFallback()
            else:
                raise RuntimeError(
                    "no file-locking backend is available on this Python "
                    "runtime (neither fcntl nor msvcrt could be imported), so "
                    "concurrent worktree-manager processes cannot be "
                    "serialized and would lose each other's state updates. "
                    f"Refusing to continue. Set {ALLOW_UNLOCKED_ENV_VAR}=1 to "
                    "proceed anyway if you are certain only one manager "
                    "process runs at a time."
                )
        deadline = time.monotonic() + timeout
        announced = False
        while not lock.try_acquire():
            if time.monotonic() >= deadline:
                raise RuntimeError(
                    f"timed out after {timeout:g}s waiting for the worktree-manager "
                    f"state lock at {lock_path}. Another manager process is still "
                    "holding it (an allocate that is fetching from origin can hold it "
                    "for a while). Wait for it to finish, or if no manager is running, "
                    f"remove {lock_path} and retry. Set "
                    f"{LOCK_TIMEOUT_ENV_VAR} to change the timeout."
                )
            if not announced:
                announced = True
                print(
                    f"waiting for the state lock at {lock_path} "
                    f"(held by another manager process; timeout {timeout:g}s)",
                    file=sys.stderr,
                )
            time.sleep(LOCK_POLL_SECONDS)
        try:
            yield
        finally:
            lock.release()


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def slugify(value: str) -> str:
    pieces: list[str] = []
    dash = False
    for char in value.lower():
        if char.isalnum():
            pieces.append(char)
            dash = False
        elif not dash:
            pieces.append("-")
            dash = True
    slug = "".join(pieces).strip("-")
    return slug or "slot"


def render_state_path(path: Path, repo_root: Path | None = None) -> str:
    """Return a path string relative to *repo_root*, or absolute if outside it.

    All root-sensitive callers pass the resolved manager root explicitly
    (defect 4) so a linked-worktree invocation cannot silently mix roots.
    Falls back to the module-level ``REPO_ROOT`` when *repo_root* is omitted
    (e.g. in ``default_state`` where the runtime root is not yet known).
    """
    root = repo_root if repo_root is not None else REPO_ROOT
    try:
        return str(path.relative_to(root))
    except ValueError:
        return str(path)


def run(
    cmd: list[str], *, cwd: Path | None = None, check: bool = True
) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        cmd,
        cwd=str(cwd) if cwd is not None else None,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc


def git(
    args: list[str], *, cwd: Path | None = None, check: bool = True
) -> subprocess.CompletedProcess[str]:
    return run(["git", *args], cwd=cwd, check=check)


def gh_available() -> bool:
    return shutil.which("gh") is not None


def load_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def save_json(path: Path, payload: dict[str, Any]) -> None:
    """Write *payload* to *path* atomically.

    Writes to a sibling temporary file first, flushes, fsyncs, then renames.
    An interrupted write (crash, OOM, power loss) leaves the previous
    *path* content intact.

    The destination's existing permission bits are carried onto the temporary
    file before the rename.  ``mkstemp`` creates files 0600, and ``replace``
    keeps the *source* mode, so without this the first atomic write would
    silently tighten a shared state file to owner-only.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        prior_mode: int | None = path.stat().st_mode & 0o777
    except OSError:
        prior_mode = None
    fd, tmp_path_str = tempfile.mkstemp(dir=str(path.parent), suffix=".tmp")
    tmp_path = Path(tmp_path_str)
    try:
        # newline="\n" keeps the state file LF-terminated on every platform.
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(json.dumps(payload, indent=2, sort_keys=True) + "\n")
            fh.flush()
            os.fsync(fh.fileno())
        if prior_mode is not None:
            os.chmod(tmp_path, prior_mode)
        tmp_path.replace(path)  # atomic on POSIX; best-effort on Windows
        # fsync the directory so the rename itself survives a crash.
        with contextlib.suppress(OSError, AttributeError):
            dir_fd = os.open(str(path.parent), os.O_RDONLY)
            try:
                os.fsync(dir_fd)
            finally:
                os.close(dir_fd)
    except Exception:
        with contextlib.suppress(OSError):
            tmp_path.unlink()
        raise


def default_state() -> dict[str, Any]:
    return {
        "version": 1,
        "managed_root": render_state_path(DEFAULT_MANAGED_ROOT),
        "updated_at": None,
        "slots": [],
    }


def normalize_owner(value: str | None) -> str | None:
    if value is None:
        return None
    owner = value.strip()
    return owner or None


def owner_from_env() -> str | None:
    for key in OWNER_ENV_VARS:
        owner = normalize_owner(os.environ.get(key))
        if owner:
            return owner
    return None


def resolve_owner(explicit: str | None) -> str | None:
    owner = normalize_owner(explicit)
    if owner:
        return owner
    return owner_from_env()


def set_owner(slot: dict[str, Any], owner: str | None, owner_source: str | None) -> None:
    slot["owner"] = owner
    slot["owner_set_at"] = utc_now() if owner else None
    if owner and owner_source:
        slot["owner_source"] = owner_source
    else:
        slot.pop("owner_source", None)


def state_path_from_args(args: argparse.Namespace, repo_root: Path | None = None) -> Path:
    if args.state_file:
        return Path(args.state_file)
    override = os.environ.get("WORKTREE_MANAGER_STATE_FILE")
    if override:
        return Path(override)
    root = repo_root if repo_root is not None else PRIMARY_REPO_ROOT
    return root / ".ops-perl-lsp" / "worktree-manager" / "state.json"


def managed_root_from_args(args: argparse.Namespace, repo_root: Path | None = None) -> Path:
    root = repo_root if repo_root is not None else PRIMARY_REPO_ROOT
    if args.managed_root:
        candidate = Path(args.managed_root)
        return candidate if candidate.is_absolute() else root / candidate
    return root.parent / f"{root.name}-worktrees"


def normalize_branch(branch: str) -> str:
    if branch.startswith("refs/heads/"):
        return branch.removeprefix("refs/heads/")
    return branch


def parse_worktree_list() -> list[dict[str, str]]:
    proc = git(["worktree", "list", "--porcelain"])
    entries: list[dict[str, str]] = []
    current: dict[str, str] = {}
    for line in proc.stdout.splitlines():
        if not line.strip():
            if current:
                entries.append(current)
                current = {}
            continue
        key, _, value = line.partition(" ")
        current[key] = value
    if current:
        entries.append(current)
    return entries


def fetch_origin(refspec: str | None = None) -> subprocess.CompletedProcess[str]:
    """Fetch from ``origin`` so ref resolution reflects current remote state.

    Best-effort: fetch failures (offline, no ``origin`` remote, etc.) do not
    raise — callers fall back to whatever local refs are already available.
    """
    cmd = ["fetch", "origin"]
    if refspec:
        cmd.append(refspec)
    return git(cmd, check=False)


def base_ref() -> str:
    for candidate in DEFAULT_BRANCH_BASES:
        if git(["rev-parse", "--verify", f"{candidate}^{{commit}}"], check=False).returncode == 0:
            return candidate
    return "HEAD"


def origin_branch_ref(branch: str) -> str | None:
    """Return ``origin/<branch>`` if *branch* exists on ``origin``, having
    verified the local tracking ref was actually fetched fresh.

    Two failure modes are deliberately handled very differently (issue
    #3749 review follow-up):

    - ``git ls-remote --exit-code`` reports "no matching refs" (exit code 2,
      per git's own documented ``--exit-code`` contract): origin was
      reached successfully and the branch genuinely does not exist there.
      Returning ``None`` here is safe -- the caller's genuinely-new-branch
      path applies.
    - Anything else going wrong -- origin unreachable, credentials
      rejected, or the refspec fetch not actually landing the branch's
      objects/ref locally -- means we cannot prove the branch either
      doesn't exist or what its real content is. Silently falling back to
      "treat as new" here would recreate the exact #3749 footgun (cutting a
      fresh branch off local main that shares a name with an
      already-pushed branch). Fail closed with a ``RuntimeError`` instead
      of guessing.
    """
    wanted = normalize_branch(branch)
    if not wanted or wanted == "HEAD":
        return None

    ls_remote = git(["ls-remote", "--exit-code", "--heads", "origin", wanted], check=False)
    if ls_remote.returncode == 2:
        return None
    if ls_remote.returncode != 0:
        raise RuntimeError(
            f"could not reach origin to check whether branch {wanted!r} already exists "
            f"(git ls-remote exited {ls_remote.returncode}); refusing to guess -- fix "
            "connectivity/credentials and retry (issue #3749: treating this as a new "
            "branch would risk re-branching an already-pushed branch off local main)"
        )

    remote_sha = ls_remote.stdout.split()[0] if ls_remote.stdout.strip() else None
    if not remote_sha:
        raise RuntimeError(
            f"origin reported branch {wanted!r} exists but returned no SHA "
            f"(ls-remote output: {ls_remote.stdout!r}); refusing to guess"
        )

    fetch_proc = fetch_origin(f"+refs/heads/{wanted}:refs/remotes/origin/{wanted}")
    local_ref = f"origin/{wanted}"
    verify = git(["rev-parse", "--verify", f"{local_ref}^{{commit}}"], check=False)
    local_sha = verify.stdout.strip()
    if fetch_proc.returncode != 0 or verify.returncode != 0 or local_sha != remote_sha:
        raise RuntimeError(
            f"fetched {local_ref} but could not verify it matches origin's current tip "
            f"for {wanted!r} (remote={remote_sha}, local={local_sha or 'missing'}, "
            f"fetch_exit={fetch_proc.returncode}); refusing to check out a possibly-stale "
            "ref (issue #3749)"
        )
    return local_ref


def branch_exists_elsewhere(
    branch: str, entries: list[dict[str, str]], slot_id: str | None = None
) -> str | None:
    wanted = normalize_branch(branch)
    for entry in entries:
        current_branch = normalize_branch(entry.get("branch", ""))
        if current_branch == wanted:
            path = entry.get("worktree", "")
            if not slot_id or Path(path).name != slot_id:
                return path
    return None


def worktree_dirty(path: Path) -> bool:
    if not path.exists():
        return False
    return bool(git(["-C", str(path), "status", "--porcelain"], check=False).stdout.strip())


def branch_merged(branch: str) -> bool:
    wanted = normalize_branch(branch)
    if not wanted or wanted == "HEAD":
        return False
    base = base_ref()
    if base == "HEAD":
        return False
    return git(["merge-base", "--is-ancestor", wanted, base], check=False).returncode == 0


def open_pr_number(branch: str) -> str | None:
    if not gh_available():
        return None
    wanted = normalize_branch(branch)
    if not wanted or wanted == "HEAD":
        return None
    proc = run(
        ["gh", "pr", "list", "--head", wanted, "--state", "open", "--json", "number", "--jq", ".[0].number"],
        check=False,
    )
    number = proc.stdout.strip()
    return number or None


def load_state(path: Path) -> dict[str, Any]:
    state = default_state()
    state.update(load_json(path))
    state.setdefault("slots", [])
    return state


def save_state(path: Path, state: dict[str, Any]) -> None:
    state["updated_at"] = utc_now()
    save_json(path, state)


def slot_lookup(state: dict[str, Any]) -> dict[str, dict[str, Any]]:
    slots: dict[str, dict[str, Any]] = {}
    for slot in state.get("slots", []):
        slot_id = slot.get("slot_id")
        if slot_id:
            slots[slot_id] = slot
    return slots


def sync_state(state: dict[str, Any], managed_root: Path, repo_root: Path) -> None:
    """Reconcile recorded slot state against live git worktree state.

    All path reconstruction uses the explicitly supplied *repo_root* so that
    a linked-worktree invocation never silently resolves slot paths against
    the wrong checkout (defect 4).
    """
    entries = parse_worktree_list()
    slots = slot_lookup(state)
    known_paths: set[Path] = set()

    for entry in entries:
        path = Path(entry["worktree"])
        if managed_root not in path.parents and path != managed_root:
            continue

        slot_id = path.name
        branch = normalize_branch(entry.get("branch", "HEAD"))
        slot = slots.get(slot_id)
        if slot is None:
            slot = {
                "slot_id": slot_id,
                "path": render_state_path(path, repo_root),
                "branch": branch,
                "owner": None,
                "owner_set_at": None,
                "status": "active" if branch != "HEAD" else "detached",
                "reuse_count": 0,
                "last_used_at": utc_now(),
                "last_released_at": None,
                "notes": [],
            }
            state["slots"].append(slot)
            slots[slot_id] = slot
        else:
            slot["path"] = render_state_path(path, repo_root)
            slot["branch"] = branch
            slot.setdefault("owner", None)
            slot.setdefault("owner_set_at", None)
            if path.exists():
                if worktree_dirty(path):
                    slot["status"] = "dirty"
                elif slot.get("status") not in {"idle", "retired"}:
                    slot["status"] = "active" if branch != "HEAD" else "detached"
            else:
                slot["status"] = "missing"
            slot["last_seen_at"] = utc_now()
        known_paths.add(path)

    for slot in state.get("slots", []):
        slot_path = slot.get("path")
        if not slot_path:
            continue
        abs_path = repo_root / slot_path  # was REPO_ROOT (defect 4)
        if abs_path not in known_paths and not abs_path.exists() and slot.get("status") not in {"retired", "missing"}:
            slot["status"] = "missing"


def printable_status(slot: dict[str, Any]) -> str:
    status = slot.get("status", "unknown")
    branch = slot.get("branch", "HEAD")
    if status == "idle":
        return "idle/reusable"
    if status == "active" and branch_merged(branch):
        return "active/merged"
    if status == "dirty":
        return "dirty"
    if status == "missing":
        return "missing"
    if status == "detached":
        return "detached"
    return status


def print_query(state: dict[str, Any]) -> None:
    slots = state.get("slots", [])
    if not slots:
        print("No managed worktree slots recorded.")
        return

    print(f"{'SLOT':20} {'STATUS':15} {'OWNER':20} {'BRANCH':28} {'REUSE':5} PATH")
    print(f"{'-' * 20} {'-' * 15} {'-' * 20} {'-' * 28} {'-' * 5} {'-' * 30}")
    for slot in sorted(slots, key=lambda item: item.get("slot_id", "")):
        owner = normalize_owner(slot.get("owner")) or "(unowned)"
        print(
            f"{slot.get('slot_id', '')[:20]:20} "
            f"{printable_status(slot)[:15]:15} "
            f"{owner[:20]:20} "
            f"{normalize_branch(slot.get('branch', 'HEAD'))[:28]:28} "
            f"{str(slot.get('reuse_count', 0))[:5]:5} "
            f"{slot.get('path', '')}"
        )


def json_query(state: dict[str, Any]) -> None:
    print(json.dumps(state, indent=2, sort_keys=True))


def ensure_branch_not_in_use(branch: str, entries: list[dict[str, str]], slot_id: str) -> None:
    conflict = branch_exists_elsewhere(branch, entries, slot_id=slot_id)
    if conflict:
        raise RuntimeError(f"branch {branch!r} is already checked out at {conflict}")


def allocate(
    args: argparse.Namespace,
    state: dict[str, Any],
    state_path: Path,
    managed_root: Path,
    repo_root: Path,
) -> None:
    entries = parse_worktree_list()
    ensure_branch_not_in_use(args.branch, entries, args.slot)

    slots = slot_lookup(state)
    slot = slots.get(args.slot)
    slot_path = managed_root / args.slot
    slot_rel = render_state_path(slot_path, repo_root)  # defect 4: explicit repo_root
    owner = resolve_owner(args.owner)
    owner_source = "flag" if normalize_owner(args.owner) else "env" if owner else None

    if slot is None:
        slot = {
            "slot_id": args.slot,
            "path": slot_rel,
            "branch": normalize_branch(args.branch),
            "owner": owner,
            "owner_set_at": utc_now() if owner else None,
            "status": "idle",
            "reuse_count": 0,
            "last_used_at": None,
            "last_released_at": None,
            "notes": [],
        }
        state["slots"].append(slot)
        slots[args.slot] = slot

    if slot.get("status") not in {"idle", "missing", "retired"} and not args.force:
        raise RuntimeError(f"slot {args.slot!r} is currently {slot.get('status')!r}; use --force to reallocate")

    if args.ref:
        # Explicit override: honor it as-is, no origin-branch inference.
        base = args.ref
    else:
        # Issue #3749: never silently branch off local/base main when the
        # requested branch is already pushed to origin — check out its real
        # content instead. Only cut from base_ref() (freshly fetched) when
        # the branch is genuinely new.
        base = origin_branch_ref(args.branch)
        if base is None:
            fetch_origin()
            base = base_ref()
    action = "reuse" if slot_path.exists() else "create"

    if args.dry_run:
        print(f"would {action} slot={args.slot} path={slot_rel} branch={args.branch} ref={base}")
        return

    slot_path.parent.mkdir(parents=True, exist_ok=True)
    if slot_path.exists():
        if worktree_dirty(slot_path) and not args.force:
            raise RuntimeError(f"slot {args.slot!r} is dirty; clean it before allocate or use --force")
        git(["-C", str(slot_path), "checkout", "-B", args.branch, base])
    else:
        git(["worktree", "add", "-B", args.branch, str(slot_path), base])

    slot["branch"] = normalize_branch(args.branch)
    slot["path"] = slot_rel
    slot["status"] = "active"
    slot["reuse_count"] = int(slot.get("reuse_count", 0)) + 1
    slot["last_used_at"] = utc_now()
    set_owner(slot, owner, owner_source)
    save_state(state_path, state)
    print(f"allocated slot={args.slot} path={slot_rel} branch={args.branch} ref={base}")


def release(
    args: argparse.Namespace,
    state: dict[str, Any],
    state_path: Path,
    repo_root: Path,
) -> None:
    slots = slot_lookup(state)
    slot = slots.get(args.slot)
    if slot is None:
        raise RuntimeError(f"unknown slot {args.slot!r}")

    slot_path = repo_root / slot.get("path", "")  # defect 4: was REPO_ROOT
    if slot_path.exists() and worktree_dirty(slot_path) and not args.force:
        raise RuntimeError(f"slot {args.slot!r} is dirty; clean it before release or use --force")

    owner = resolve_owner(args.owner)
    recorded_owner = normalize_owner(slot.get("owner"))

    # Defect 2: the original guard was
    #   `if owner and recorded_owner and owner != recorded_owner and not args.force`
    # which allowed an ownerless caller (owner is None) to release a slot with
    # a recorded owner.  The correct invariant: when a slot carries a recorded
    # owner, release requires the *same* owner unless --force is explicit.
    #
    # This runs *before* the --dry-run return so the prediction matches the
    # real command.  Callers use --dry-run to decide whether to release; a
    # dry run that printed "would release" for another owner's slot would be
    # wrong in exactly the case this guard exists to protect.  The
    # dirty-worktree check above is ordered the same way for the same reason.
    if recorded_owner and (owner is None or owner != recorded_owner) and not args.force:
        raise RuntimeError(
            f"slot {args.slot!r} is owned by {recorded_owner!r}; "
            "supply --owner with the same owner or use --force to bypass"
        )

    if args.dry_run:
        print(f"would release slot={args.slot} path={slot.get('path', '')}")
        return

    slot["status"] = "retired" if args.retire else "idle"
    slot["last_released_at"] = utc_now()
    set_owner(slot, None, None)
    if args.note:
        slot.setdefault("notes", []).append(args.note)
    save_state(state_path, state)
    print(f"released slot={args.slot} status={slot['status']}")


def stale_slot(slot: dict[str, Any], stale_days: int) -> bool:
    if slot.get("status") not in {"idle", "active", "dirty"}:
        return False
    last_released = slot.get("last_released_at")
    if not last_released:
        return False
    try:
        released = datetime.fromisoformat(last_released.replace("Z", "+00:00"))
    except ValueError:
        return False
    age = datetime.now(timezone.utc) - released.astimezone(timezone.utc)
    if age.days < stale_days:
        return False
    branch = normalize_branch(slot.get("branch", "HEAD"))
    return branch == "HEAD" or branch_merged(branch)


def cleanup(
    args: argparse.Namespace,
    state: dict[str, Any],
    state_path: Path,
    repo_root: Path,
) -> None:
    slots = state.get("slots", [])
    kept: list[dict[str, Any]] = []
    removed = 0
    pruned = 0

    if args.run_low_level:
        script = repo_root / "scripts" / "cleanup-completed-worktrees.sh"  # defect 4: was REPO_ROOT
        cmd = ["bash", str(script)]
        if args.dry_run:
            cmd.append("--dry-run")
        proc = run(cmd, check=False)
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        if proc.returncode != 0:
            raise RuntimeError("low-level cleanup script failed")

    for slot in slots:
        slot_path = repo_root / slot.get("path", "")  # defect 4: was REPO_ROOT
        if not slot.get("path") or not slot_path.exists():
            removed += 1
            continue

        branch = normalize_branch(slot.get("branch", "HEAD"))
        should_prune = slot.get("status") == "retired" or stale_slot(slot, args.stale_days)
        if not should_prune and slot.get("status") == "idle" and branch != "HEAD":
            should_prune = branch_merged(branch) and open_pr_number(branch) is None

        if should_prune:
            if args.dry_run:
                print(f"would prune slot={slot.get('slot_id')} path={slot.get('path')} branch={branch}")
            else:
                git(["worktree", "remove", "--force", str(slot_path)], check=False)
                if branch != "HEAD":
                    git(["branch", "-D", branch], check=False)
            pruned += 1
            continue

        kept.append(slot)

    state["slots"] = kept
    if not args.dry_run:
        save_state(state_path, state)

    print(f"cleanup complete: kept={len(kept)} removed_state={removed} pruned={pruned}")


def build_parser() -> argparse.ArgumentParser:
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--state-file", help="Override the runtime state file path")
    common.add_argument("--managed-root", help="Override the managed worktree root")

    parser = argparse.ArgumentParser(description="Manage reusable worktree slots.", parents=[common])

    sub = parser.add_subparsers(dest="command", required=True)

    query = sub.add_parser("query", help="Show the current pool state", parents=[common])
    query.add_argument("--json", action="store_true", help="Emit JSON instead of a table")

    allocate_cmd = sub.add_parser("allocate", help="Allocate or reuse a worktree slot", parents=[common])
    allocate_cmd.add_argument("--slot", required=True, help="Named slot to allocate")
    allocate_cmd.add_argument("--branch", required=True, help="Branch to check out")
    allocate_cmd.add_argument("--ref", help="Base ref to check out from")
    allocate_cmd.add_argument(
        "--owner",
        help="Owner label for the slot (defaults to WORKTREE_MANAGER_OWNER / CLAUDE_* env)",
    )
    allocate_cmd.add_argument("--dry-run", action="store_true", help="Show the action without mutating anything")
    allocate_cmd.add_argument("--force", action="store_true", help="Reallocate even if the slot is active")

    release_cmd = sub.add_parser("release", help="Mark a slot reusable", parents=[common])
    release_cmd.add_argument("--slot", required=True, help="Named slot to release")
    release_cmd.add_argument(
        "--owner",
        help="Owner label to record on release (defaults to WORKTREE_MANAGER_OWNER / CLAUDE_* env)",
    )
    release_cmd.add_argument("--note", help="Optional note to append to the slot history")
    release_cmd.add_argument("--retire", action="store_true", help="Retire the slot instead of leaving it idle")
    release_cmd.add_argument("--dry-run", action="store_true", help="Show the action without mutating anything")
    release_cmd.add_argument("--force", action="store_true", help="Release even if the worktree is dirty")

    cleanup_cmd = sub.add_parser("cleanup", help="Prune stale managed slots", parents=[common])
    cleanup_cmd.add_argument("--dry-run", action="store_true", help="Show the action without mutating anything")
    cleanup_cmd.add_argument("--stale-days", type=int, default=7, help="Idle age threshold before pruning")
    cleanup_cmd.add_argument(
        "--run-low-level",
        action="store_true",
        help="Also run scripts/cleanup-completed-worktrees.sh before state reconciliation",
    )

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    repo_root = _resolve_primary_repo_root()  # defect 1: runtime resolution
    state_path = state_path_from_args(args, repo_root)
    managed_root = managed_root_from_args(args, repo_root)

    # Defect 3: wrap the entire read-modify-write in an exclusive file lock so
    # concurrent allocations and releases cannot overwrite each other's updates.
    with _state_transaction(state_path):
        state = load_state(state_path)
        state["managed_root"] = render_state_path(managed_root, repo_root)
        sync_state(state, managed_root, repo_root)

        if args.command == "query":
            if args.json:
                json_query(state)
            else:
                print_query(state)
            save_state(state_path, state)
            return 0
        if args.command == "allocate":
            allocate(args, state, state_path, managed_root, repo_root)
            return 0
        if args.command == "release":
            release(args, state, state_path, repo_root)
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
