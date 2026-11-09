#!/usr/bin/env python3
"""Exact, evidence-only retirement handoff for admitted Cargo worktree state.

This module never deletes Cargo state. It binds a worktree-manager cleanup
selection to the existing cargo_admitted resource planner, writes one durable
immutable manifest before Git removes the worktree, and verifies that the
external assets retained their native identities afterward.
"""

from __future__ import annotations

import hashlib
import importlib
import json
import os
import socket
import stat
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Mapping

SCHEMA = "cargo_asset_retirement.v1"
MAX_GIT_OUTPUT = 1024 * 1024
MAX_MANIFEST_BYTES = 256 * 1024
MAX_LEASE_ENTRIES = 32
GIT_TIMEOUT_SECONDS = 10


class RetirementRefused(RuntimeError):
    """The exact retirement handoff could not be proven or persisted."""


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def canonical_json(payload: Mapping[str, Any]) -> bytes:
    return (json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("utf-8")


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _load_cargo_admitted():
    return importlib.import_module("cargo_admitted")


def _run_git_bytes(
    worktree: Path,
    args: list[str],
    *,
    env: Mapping[str, str],
    allowed: tuple[int, ...] = (0,),
) -> subprocess.CompletedProcess[bytes]:
    try:
        result = subprocess.run(
            ["git", "-C", str(worktree), *args],
            env=dict(env),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=GIT_TIMEOUT_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RetirementRefused("bounded Git observation failed: " + str(error)) from error
    size = len(result.stdout) + len(result.stderr)
    if size > MAX_GIT_OUTPUT:
        raise RetirementRefused("bounded Git observation exceeded 1 MiB")
    if result.returncode not in allowed:
        diagnostic = result.stderr.decode("utf-8", "replace").strip()[-4096:]
        raise RetirementRefused(
            "Git observation exited " + str(result.returncode) + (": " + diagnostic if diagnostic else "")
        )
    return result


def _planner_reader(worktree: Path) -> Callable[[list[str], Mapping[str, str]], bytes]:
    def read(command: list[str], env: Mapping[str, str]) -> bytes:
        if not command or command[0] != "git":
            raise RetirementRefused("Cargo planner requested a non-Git observer")
        return _run_git_bytes(worktree, command[1:], env=env).stdout

    return read


def resource_plan_for(
    source_worktree: Path | str,
    env: Mapping[str, str] | None = None,
    *,
    cargo: Any | None = None,
) -> tuple[Any, Path, Path, dict[str, Path], dict[str, str], Callable[[list[str], Mapping[str, str]], bytes]]:
    """Resolve the existing admitted resource plan for one exact worktree.

    The path/hash formula remains owned by ``cargo_admitted.resource_plan``.
    This seam only supplies a worktree-bound read-only Git observer so cleanup
    never reimplements or guesses that formula.
    """

    cargo = cargo or _load_cargo_admitted()
    source = cargo.native_path(str(source_worktree))
    observed_env = cargo.read_only_git_environment(dict(os.environ if env is None else env))
    reader = _planner_reader(source)
    try:
        worktree, slot, paths = cargo.resource_plan(observed_env, git_output_reader=reader)
    except Exception as error:  # cargo owns its typed denial hierarchy.
        raise RetirementRefused("admitted Cargo resource plan is not exact: " + str(error)) from error
    if worktree != source:
        raise RetirementRefused(
            "admitted Cargo planner resolved a different worktree: " + str(worktree) + " != " + str(source)
        )
    return cargo, worktree, slot, paths, observed_env, reader


def _path_kind(mode: int) -> str:
    if stat.S_ISDIR(mode):
        return "directory"
    if stat.S_ISREG(mode):
        return "regular_file"
    if stat.S_ISLNK(mode):
        return "symlink"
    if stat.S_ISFIFO(mode):
        return "fifo"
    if stat.S_ISSOCK(mode):
        return "socket"
    if stat.S_ISCHR(mode):
        return "character_device"
    if stat.S_ISBLK(mode):
        return "block_device"
    return "other"


def observe_path(path: Path | str) -> dict[str, Any]:
    path = Path(path)
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return {
            "path": str(path),
            "exists": False,
            "type": "absent",
            "identity": None,
            "mode": None,
            "link_count": None,
            "logical_bytes": 0,
            "allocated_bytes": "NOT_PROVEN",
        }
    except OSError as error:
        raise RetirementRefused("cannot lstat retained Cargo asset " + str(path) + ": " + str(error)) from error
    kind = _path_kind(metadata.st_mode)
    return {
        "path": str(path),
        "exists": True,
        "type": kind,
        "identity": [metadata.st_dev, metadata.st_ino],
        "mode": stat.S_IMODE(metadata.st_mode),
        "link_count": metadata.st_nlink,
        "logical_bytes": metadata.st_size if kind == "regular_file" else "NOT_PROVEN",
        "allocated_bytes": "NOT_PROVEN",
    }



def _is_within(path: Path, parent: Path) -> bool:
    try:
        return os.path.commonpath((str(path), str(parent))) == str(parent)
    except ValueError:
        return False


def _require_directory_observation(name: str, observation: Mapping[str, Any], blockers: list[str]) -> None:
    if observation["exists"] and observation["type"] != "directory":
        blockers.append(name + "_unexpected_" + str(observation["type"]))


def _git_path(cargo: Any, reader: Callable, env: Mapping[str, str], *args: str) -> Path:
    try:
        return cargo.git_path(*args, env=env, output_reader=reader)
    except Exception as error:
        raise RetirementRefused("cannot resolve exact Git path: " + str(error)) from error


def _git_text(
    worktree: Path,
    args: list[str],
    env: Mapping[str, str],
    *,
    allowed: tuple[int, ...] = (0,),
) -> tuple[int, str]:
    result = _run_git_bytes(worktree, args, env=env, allowed=allowed)
    try:
        text = result.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise RetirementRefused("Git observation was not UTF-8") from error
    return result.returncode, text.rstrip("\n")


def _source_state(worktree: Path, git_dir: Path, env: Mapping[str, str]) -> tuple[dict[str, Any], list[str]]:
    blockers: list[str] = []
    _, head = _git_text(worktree, ["rev-parse", "HEAD"], env)
    if len(head) != 40 or any(character not in "0123456789abcdef" for character in head.lower()):
        raise RetirementRefused("source HEAD is not a full object id")

    branch_code, branch = _git_text(worktree, ["symbolic-ref", "--short", "-q", "HEAD"], env, allowed=(0, 1))
    if branch_code == 0 and branch:
        branch_state: dict[str, Any] = {"state": "branch", "name": branch}
    else:
        branch_state = {"state": "detached", "name": None}
        blockers.append("source_detached")

    status = _run_git_bytes(
        worktree,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        env=env,
    ).stdout
    entries = [entry for entry in status.split(b"\0") if entry]
    untracked = sum(entry.startswith(b"??") for entry in entries)
    dirty = len(entries)
    if dirty:
        blockers.append("source_dirty")
    if untracked:
        blockers.append("source_untracked")

    locked_observation = observe_path(git_dir / "locked")
    locked = locked_observation["exists"]
    if locked:
        blockers.append("source_worktree_locked")

    unpushed: dict[str, Any]
    if branch_state["state"] != "branch":
        unpushed = {"state": "NOT_PROVEN", "reason": "detached_head"}
    else:
        upstream_code, upstream = _git_text(
            worktree,
            ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"],
            env,
            allowed=(0, 1, 128),
        )
        if upstream_code != 0 or not upstream:
            unpushed = {"state": "NOT_PROVEN", "reason": "no_upstream"}
            blockers.append("source_unpushed_not_proven")
        else:
            _, ahead_raw = _git_text(worktree, ["rev-list", "--count", "@{upstream}..HEAD"], env)
            try:
                ahead = int(ahead_raw)
            except ValueError as error:
                raise RetirementRefused("Git ahead count is not an integer") from error
            unpushed = {"state": "PROVEN", "upstream": upstream, "ahead_commits": ahead}
            if ahead:
                blockers.append("source_unpushed")

    return {
        "head": head,
        "branch_or_detached": branch_state,
        "dirty_entry_count": dirty,
        "untracked_entry_count": untracked,
        "status_sha256": sha256_bytes(status),
        "locked": locked,
        "lock_observation": locked_observation,
        "unpushed": unpushed,
    }, blockers


def _lease_state(slot: Path) -> tuple[dict[str, Any], list[str]]:
    lease = slot / "cargo-active"
    observation = observe_path(lease)
    blockers: list[str] = []
    markers: list[dict[str, Any]] = []
    if not observation["exists"]:
        state = "MISSING_NOT_PROOF"
    elif observation["type"] != "directory":
        state = "REVIEW_UNEXPECTED_TYPE"
        blockers.append("lease_unexpected_type")
    else:
        state = "ACTIVE_OR_UNSETTLED"
        blockers.append("active_or_unsettled_cargo_lease")
        try:
            entries = sorted(lease.iterdir(), key=lambda item: os.fsencode(item.name))
        except OSError as error:
            raise RetirementRefused("cannot inspect bounded Cargo lease: " + str(error)) from error
        if len(entries) > MAX_LEASE_ENTRIES:
            blockers.append("lease_entry_bound_exceeded")
            entries = entries[:MAX_LEASE_ENTRIES]
        for entry in entries:
            markers.append({"name": entry.name, "observation": observe_path(entry)})
        if any(not marker["name"].startswith("owner-") for marker in markers):
            blockers.append("lease_contains_unexpected_entry")
    return {
        "path": str(lease),
        "state": state,
        "identity": observation["identity"],
        "observation": observation,
        "markers": markers,
        "missing_is_positive_proof": False,
    }, blockers


def _legacy_cargo_observations(
    cargo: Any, worktree: Path, env: Mapping[str, str], admitted_slot: Path
) -> dict[str, Any]:
    """Observe the legacy cargo-safe root without treating it as admitted state.

    cargo-safe used DEVPLANE directly, or a basename-derived default. The
    admitted route instead treats DEVPLANE as a parent and appends a
    host/platform/repository slot. Merely seeing the parent directory is not
    legacy evidence; direct cargo-safe child roles are.
    """

    if env.get("DEVPLANE"):
        root = cargo.native_path(env["DEVPLANE"])
    else:
        cache = env.get("XDG_CACHE_HOME")
        if cache:
            cache_root = cargo.native_path(cache)
        else:
            home = env.get("HOME")
            cache_root = cargo.native_path(str(Path(home) / ".cache" if home else Path.home() / ".cache"))
        root = cache_root / "devplane" / worktree.name
    roles = {
        "root": root,
        "cargo_home": root / "cargo-home",
        "target": root / "target",
        "build": root / "build",
        "sccache": root / "sccache",
        "tmp": root / "tmp",
        "locks": root / "locks",
    }
    observations = {name: observe_path(value) for name, value in roles.items()}
    direct_present = any(
        observation["exists"]
        for name, observation in observations.items()
        # tmp alone can be the admitted repository-domain temporary root if a
        # pathological worktree basename equals the admitted slot name. A real
        # cargo-safe initialization creates the other direct roles together.
        if name not in {"root", "tmp"}
    )
    return {
        "root": str(root),
        "same_as_admitted_slot": root == admitted_slot,
        "direct_role_present": direct_present,
        "roles": observations,
        "reclaim_eligibility": "NOT_PROVEN",
    }


def _stable_slot_record(slot_record: Mapping[str, Any]) -> dict[str, Any]:
    fields = (
        "slot_id",
        "path",
        "branch",
        "owner",
        "owner_set_at",
        "status",
        "last_used_at",
        "last_released_at",
    )
    return {field: slot_record.get(field) for field in fields}


def _fingerprint_subject(manifest: Mapping[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in manifest.items() if key not in {"cleanup_plan_fingerprint", "created_at"}}


def _manifest_path(manifest_root: Path, source_id: str, head: str, fingerprint: str) -> Path:
    return manifest_root / source_id / (head + "-" + fingerprint + ".json")


def plan_retirement(
    source_worktree: Path | str,
    *,
    repo_root: Path | str,
    slot_record: Mapping[str, Any],
    state_path: Path | str,
    cleanup_reason: str,
    env: Mapping[str, str] | None = None,
    created_at: str | None = None,
    cargo: Any | None = None,
) -> dict[str, Any]:
    """Produce a zero-mutation, exact retirement plan for one managed worktree."""

    cargo, worktree, slot, paths, observed_env, reader = resource_plan_for(source_worktree, env, cargo=cargo)
    repo_root = cargo.native_path(str(repo_root))
    state_path = Path(state_path)
    common = _git_path(cargo, reader, observed_env, "--path-format=absolute", "--git-common-dir")
    git_dir = _git_path(cargo, reader, observed_env, "--absolute-git-dir")
    private = cargo.private_worktree_root(slot, worktree)
    if paths["target"] != private / "target" or paths["build"] != private / "build":
        raise RetirementRefused("Cargo private target/build roles differ from their authoritative member")

    source_state, blockers = _source_state(worktree, git_dir, observed_env)
    lease, lease_blockers = _lease_state(slot)
    blockers.extend(lease_blockers)

    observations = {
        "manager_repository_root": observe_path(repo_root),
        "repository_common_dir": observe_path(common),
        "repository_git_dir": observe_path(git_dir),
        "source_worktree": observe_path(worktree),
        "devplane_domain": observe_path(slot),
        "private_member": observe_path(private),
        "private_target": observe_path(paths["target"]),
        "private_build": observe_path(paths["build"]),
        "private_tmp": observe_path(paths["temp"]),
    }
    for name in ("manager_repository_root", "repository_common_dir", "repository_git_dir", "source_worktree"):
        _require_directory_observation(name, observations[name], blockers)
    for name in ("devplane_domain", "private_member", "private_target", "private_build", "private_tmp"):
        _require_directory_observation(name, observations[name], blockers)

    try:
        allocation = cargo.PrivateAllocation(worktree, slot, paths, observed_env)
    except Exception as error:
        raise RetirementRefused("private allocation provenance is invalid: " + str(error)) from error
    allocation_receipt = dict(allocation.receipt)
    allocation_record = observe_path(getattr(allocation, "path", private / "allocation.json"))
    observations["private_allocation_record"] = allocation_record
    if allocation_record["exists"] and allocation_record["type"] != "regular_file":
        blockers.append("allocation_record_unexpected_type")

    legacy = _legacy_cargo_observations(cargo, worktree, observed_env, slot)
    private_evidence_present = any(
        observations[name]["exists"]
        for name in ("private_member", "private_target", "private_build", "private_allocation_record")
    )
    private_shape_ambiguous = any(
        blocker.startswith(("private_member_unexpected_", "private_target_unexpected_",
                            "private_build_unexpected_", "allocation_record_unexpected_"))
        for blocker in blockers
    )
    if private_shape_ambiguous:
        classification = "NOT_PROVEN"
        blockers.append("private_storage_shape_not_proven")
    elif private_evidence_present:
        # The authoritative planner proves which external member belongs to
        # this worktree even when an older/non-Linux allocation has no creation
        # record. The manifest may preserve that exact handoff, but its
        # allocation provenance and reclaim eligibility remain explicitly
        # unknown. Only #19235 matching records can strengthen later reclaim.
        classification = "MANAGED_ADMITTED"
    elif legacy["direct_role_present"]:
        classification = "LEGACY_SHARED"
        blockers.append("legacy_shared_state_requires_separate_owner")
    else:
        classification = "UNMANAGED"

    common_identity = str(common).casefold() if os.name == "nt" else str(common)
    common_hash = hashlib.sha256(os.fsencode(common_identity)).hexdigest()
    source_id = private.name
    if source_id != hashlib.sha256(os.fsencode(worktree)).hexdigest():
        raise RetirementRefused("private member does not match the authoritative worktree identity")

    try:
        owner_revision = cargo.file_subject(Path(cargo.__file__))
    except Exception as error:
        raise RetirementRefused("cannot bind cargo_admitted owner revision: " + str(error)) from error

    observed_at = created_at or utc_now()
    common_evidence: dict[str, Any] = {
        "schema_version": "cargo_storage_retirement_plan.v1",
        "storage_classification": classification,
        "manager_repository_root_canonical_identity": observations["manager_repository_root"],
        "repository_common_dir_canonical_identity": observations["repository_common_dir"],
        "repository_common_dir_hash": common_hash,
        "host": socket.gethostname(),
        "platform": sys.platform,
        "source_worktree_canonical_path": str(worktree),
        "source_worktree_id": source_id,
        "source_branch_or_detached_state": source_state["branch_or_detached"],
        "source_head": source_state["head"],
        "cleanup_reason": cleanup_reason,
        "cleanup_slot_record": _stable_slot_record(slot_record),
        "cargo_admitted_owner_revision": owner_revision,
        "devplane_domain": str(slot),
        "private_member_path": str(private),
        "private_target_path": str(paths["target"]),
        "private_build_path": str(paths["build"]),
        "private_tmp_path": str(paths["temp"]),
        "path_roles": {
            "private_member": "worktree_private",
            "private_target": "worktree_private",
            "private_build": "worktree_private",
            "private_tmp": "repository_domain_shared",
        },
        "path_lstat_type_and_identity": observations,
        "legacy_shared_cargo_observation": legacy,
        "lease_state": lease,
        "worktree_locked_dirty_untracked_unpushed_state": {
            "locked": source_state["locked"],
            "dirty_entry_count": source_state["dirty_entry_count"],
            "untracked_entry_count": source_state["untracked_entry_count"],
            "status_sha256": source_state["status_sha256"],
            "unpushed": source_state["unpushed"],
        },
        "private_allocation_provenance": allocation_receipt,
        "blockers": sorted(set(blockers)),
        "created_at": observed_at,
    }

    manifest_root = (state_path.parent / "cargo-retirements").absolute()
    source_absolute = worktree.absolute()
    for retained_name in ("devplane_domain", "private_member", "private_target", "private_build", "private_tmp"):
        retained = Path(observations[retained_name]["path"]).absolute()
        if _is_within(retained, source_absolute):
            blockers.append(retained_name + "_inside_retiring_worktree")
    common_evidence["blockers"] = sorted(set(blockers))

    if classification != "MANAGED_ADMITTED":
        fingerprint_subject = {
            key: value for key, value in common_evidence.items() if key != "created_at"
        }
        fingerprint = sha256_bytes(canonical_json(fingerprint_subject))
        return {
            "classification": classification,
            "manifest_path": None,
            "cleanup_plan_fingerprint": fingerprint,
            "manifest": None,
            "evidence": common_evidence,
            "retained_asset_observations": {},
            "blockers": common_evidence["blockers"],
            "created_at": observed_at,
        }

    manifest: dict[str, Any] = {
        **common_evidence,
        "schema_version": SCHEMA,
        "cleanup_plan_fingerprint": None,
        "logical_bytes": {
            name: observation["logical_bytes"]
            for name, observation in observations.items()
            if name.startswith("private_")
        },
        "allocated_bytes_or_not_proven": "NOT_PROVEN",
        "lease_path": lease["path"],
        "lease_identity": lease["identity"],
        "lease_marker": [marker["name"] for marker in lease["markers"]],
        "marker_identity": [marker["observation"]["identity"] for marker in lease["markers"]],
        "live_consumer_state": "ACTIVE_OR_REVIEW" if lease["state"] != "MISSING_NOT_PROOF" else "NOT_PROVEN",
        "retirement_disposition": "PREPARED_EXTERNAL_ASSETS_RETAINED_RECLAIM_NOT_AUTHORIZED",
        "reclaim_eligibility": "NOT_PROVEN",
    }
    fingerprint = sha256_bytes(canonical_json(_fingerprint_subject(manifest)))
    manifest["cleanup_plan_fingerprint"] = fingerprint
    path = _manifest_path(manifest_root, source_id, source_state["head"], fingerprint)
    if _is_within(path.absolute(), source_absolute):
        raise RetirementRefused("retirement manifest destination is inside the retiring worktree")
    retained_keys = (
        "devplane_domain",
        "private_member",
        "private_target",
        "private_build",
        "private_tmp",
        "private_allocation_record",
    )
    return {
        "classification": classification,
        "manifest_path": str(path),
        "cleanup_plan_fingerprint": fingerprint,
        "manifest": manifest,
        "evidence": common_evidence,
        "retained_asset_observations": {key: observations[key] for key in retained_keys},
        "blockers": manifest["blockers"],
        "created_at": observed_at,
    }


def assert_applicable(plan: Mapping[str, Any]) -> None:
    blockers = list(plan.get("blockers", []))
    if blockers:
        raise RetirementRefused("retirement handoff blocked: " + ", ".join(blockers))


def _plan_identity(plan: Mapping[str, Any]) -> tuple[Any, ...]:
    evidence = plan["manifest"] if plan.get("manifest") is not None else plan["evidence"]
    return (
        plan["classification"],
        plan.get("manifest_path"),
        plan["cleanup_plan_fingerprint"],
        evidence["source_worktree_canonical_path"],
        evidence["source_head"],
        evidence["devplane_domain"],
        evidence["private_member_path"],
        evidence["private_target_path"],
        evidence["private_build_path"],
        evidence["private_tmp_path"],
        canonical_json(evidence),
    )


def revalidate_plan(plan: Mapping[str, Any], replan: Callable[[], Mapping[str, Any]]) -> Mapping[str, Any]:
    fresh = replan()
    if _plan_identity(plan) != _plan_identity(fresh):
        raise RetirementRefused("retirement plan changed before mutation")
    assert_applicable(fresh)
    return fresh


def _fsync_parent(path: Path) -> None:
    """Persist a POSIX directory entry after publication.

    Windows publication uses MOVEFILE_WRITE_THROUGH in _publish_manifest, the
    native durability primitive for the rename itself.
    """

    if os.name == "nt":
        return
    descriptor = os.open(path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _publish_manifest(temporary: Path, path: Path) -> None:
    if os.name != "nt":
        os.replace(temporary, path)
        _fsync_parent(path)
        return

    # os.replace does not expose MOVEFILE_WRITE_THROUGH. Use the native
    # no-overwrite publication primitive so a racing destination is refused
    # rather than silently replacing immutable evidence.
    import ctypes

    movefile_write_through = 0x00000008
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    move = kernel32.MoveFileExW
    move.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32]
    move.restype = ctypes.c_int
    if not move(str(temporary), str(path), movefile_write_through):
        error = ctypes.get_last_error()
        raise OSError(error, "MoveFileExW(MOVEFILE_WRITE_THROUGH) failed", str(path))


def _read_bounded(path: Path) -> bytes:
    try:
        descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0))
    except OSError as error:
        raise RetirementRefused("cannot open retirement manifest: " + str(error)) from error
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or metadata.st_size > MAX_MANIFEST_BYTES:
            raise RetirementRefused("retirement manifest must be a bounded singly linked regular file")
        raw = bytearray()
        while len(raw) <= MAX_MANIFEST_BYTES:
            chunk = os.read(descriptor, min(8192, MAX_MANIFEST_BYTES + 1 - len(raw)))
            if not chunk:
                break
            raw.extend(chunk)
        if len(raw) != metadata.st_size or len(raw) > MAX_MANIFEST_BYTES:
            raise RetirementRefused("retirement manifest changed or exceeded its byte bound")
        return bytes(raw)
    finally:
        os.close(descriptor)


def write_retirement_manifest(
    plan: Mapping[str, Any],
    *,
    replan: Callable[[], Mapping[str, Any]],
) -> dict[str, Any]:
    """Revalidate and durably write one immutable exact manifest."""

    fresh = revalidate_plan(plan, replan)
    if fresh["classification"] != "MANAGED_ADMITTED" or fresh.get("manifest") is None:
        raise RetirementRefused("only MANAGED_ADMITTED storage may produce a retirement manifest")
    path = Path(fresh["manifest_path"])
    raw = canonical_json(fresh["manifest"])
    if len(raw) > MAX_MANIFEST_BYTES:
        raise RetirementRefused("retirement manifest exceeds 256 KiB")
    path.parent.mkdir(parents=True, exist_ok=True)
    if os.path.lexists(path):
        existing = _read_bounded(path)
        if existing != raw:
            raise RetirementRefused("an immutable retirement manifest already exists with different evidence")
        # A prior attempt may have published the file and then failed while
        # flushing the parent. Complete that durability step before reuse.
        _fsync_parent(path)
        return {
            "path": str(path),
            "sha256": sha256_bytes(existing),
            "bytes": len(existing),
            "reused_existing": True,
        }

    descriptor, temporary_name = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=str(path.parent))
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, 0o600)
        if os.path.lexists(path):
            raise RetirementRefused("retirement manifest destination appeared during write")
        _publish_manifest(temporary, path)
    except BaseException:
        try:
            temporary.unlink()
        except OSError:
            pass
        raise
    observed = _read_bounded(path)
    if observed != raw:
        raise RetirementRefused("retirement manifest readback differs from written evidence")
    return {
        "path": str(path),
        "sha256": sha256_bytes(observed),
        "bytes": len(observed),
        "reused_existing": False,
    }


def verify_retained_assets(plan: Mapping[str, Any]) -> dict[str, Any]:
    """Prove Git worktree removal did not rename/delete/adopt external assets."""

    verified: dict[str, Any] = {}
    for name, before in plan["retained_asset_observations"].items():
        after = observe_path(before["path"])
        identity_fields = ("exists", "type", "identity")
        if any(before[field] != after[field] for field in identity_fields):
            raise RetirementRefused(
                "retained Cargo asset identity changed during worktree removal: " + name
            )
        verified[name] = after
    return {
        "verdict": "RETAINED_IDENTITIES_MATCH",
        "assets": verified,
        "reclaim_authorized": False,
    }
