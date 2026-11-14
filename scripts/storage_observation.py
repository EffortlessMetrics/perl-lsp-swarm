#!/usr/bin/env python3
"""Bounded descriptor inventory and current-slot census, never admission or cleanup.

The descriptor can identify retained resources. It cannot bind a sample taken
before its emission, prove lease settlement, or establish a build's peak demand.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import stat
import subprocess
import sys
import threading
import time

from cargo_admitted import (Denied, native_path, private_worktree_root,
                            read_only_git_environment, resource_plan)

DESCRIPTOR_LIMIT = 65536
RESOURCE_NAMES = ("target", "build", "cargo_home", "temp")
MAX_ENTRIES = 10000
MAX_DEPTH = 12
MAX_SECONDS = 2.0
MAX_FINGERPRINTS = 64
MAX_FINGERPRINT_BYTES = 65536
HASH_FIELDS = ("rustc", "target", "profile", "path", "config", "compile_kind")
CENSUS_MEMBER_LIMIT = 256
GIT_BYTE_LIMIT = 65536


class Unknown(Exception):
    """Fixed public reason, never a native path or arbitrary input payload."""


class ObservationBudget:
    """One traversal/deadline/output budget, shared across every census member."""

    def __init__(self, max_entries=MAX_ENTRIES, max_seconds=MAX_SECONDS,
                 max_git_bytes=GIT_BYTE_LIMIT):
        self.max_entries = max_entries
        self.entries = 0
        self.deadline = time.monotonic() + max_seconds
        self.git_bytes_left = max_git_bytes

    def exhausted(self):
        return self.entries >= self.max_entries or time.monotonic() >= self.deadline

    def take(self):
        if self.exhausted():
            raise Unknown("traversal_clipped")
        self.entries += 1

    def remaining(self):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise Unknown("observation_timeout")
        return remaining


def digest(value):
    return hashlib.sha256(value).hexdigest()


def pairs(items):
    result = {}
    for key, value in items:
        if key in result:
            raise Unknown("duplicate_json_key")
        result[key] = value
    return result


def invalid_constant(value):
    raise Unknown("nonfinite_json")


def decode(raw):
    try:
        return json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid_constant)
    except (ValueError, UnicodeError, RecursionError) as error:
        raise Unknown("invalid_json") from error


def facts(metadata):
    return (metadata.st_dev, metadata.st_ino, metadata.st_size,
            metadata.st_mtime_ns, metadata.st_ctime_ns)


def linked(metadata):
    return stat.S_ISLNK(metadata.st_mode) or bool(
        getattr(metadata, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))


def read_file(path, limit, *, descriptor=False, charge_bytes=None):
    # Reuse admission's native path rules, including every link/junction ancestor.
    path = native_path(str(path))
    before = path.lstat()
    if linked(before) or not stat.S_ISREG(before.st_mode):
        raise Unknown("not_regular_file")
    if descriptor and before.st_nlink != 1:
        raise Unknown("linked_descriptor")
    if before.st_size > limit:
        raise Unknown("input_clipped")
    flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
    with os.fdopen(os.open(path, flags), "rb", buffering=0) as stream:
        opened = os.fstat(stream.fileno())
        # Windows path stat and handle stat can expose different change-time
        # fields. Compare their common identity/size/mtime, then compare each
        # API's full facts against itself before and after the read.
        if facts(opened)[:4] != facts(before)[:4]:
            raise Unknown("file_changed")
        raw = None
        try:
            raw = stream.read(limit + 1)
        finally:
            # Charge before stability/overflow validation. A failed read with
            # an unknown partial count consumes its full request conservatively.
            if charge_bytes is not None:
                charge_bytes(limit + 1 if raw is None else len(raw))
        if facts(os.fstat(stream.fileno())) != facts(opened):
            raise Unknown("file_changed")
        if len(raw) != opened.st_size:
            raise Unknown("incomplete_file_read")
    if len(raw) > limit or facts(path.lstat()) != facts(before):
        raise Unknown("file_changed_or_clipped")
    return raw


def _validate_file_subject(value):
    if not isinstance(value, dict) or set(value) != {"path", "file_identity", "sha256"}:
        raise Unknown("descriptor_scope_mismatch")
    if not isinstance(value["path"], str) or not value["path"] or len(value["path"]) > 4096:
        raise Unknown("descriptor_scope_mismatch")
    if any(part in (".", "..") for part in re.split(r"[/\\]", value["path"])):
        raise Unknown("descriptor_scope_mismatch")
    identity = value["file_identity"]
    if (not isinstance(identity, list) or len(identity) != 5
            or any(type(item) is not int or item < 0 for item in identity)):
        raise Unknown("descriptor_scope_mismatch")
    if not isinstance(value["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", value["sha256"]):
        raise Unknown("descriptor_scope_mismatch")


def _validate_clippy_setup(scope):
    if scope.get("operation") != "clippy":
        if "clippy_setup" in scope:
            raise Unknown("descriptor_scope_mismatch")
        return
    setup = scope.get("clippy_setup")
    if not isinstance(setup, dict) or set(setup) != {"toolchain", "configuration", "renderer"}:
        raise Unknown("descriptor_scope_mismatch")
    _validate_file_subject(setup["renderer"])
    toolchain = setup["toolchain"]
    if not isinstance(toolchain, dict) or set(toolchain) != {"pin", "host", "root", "subjects", "pin_subject"}:
        raise Unknown("descriptor_scope_mismatch")
    if not isinstance(toolchain["pin"], str) or not re.fullmatch(r"\d+\.\d+\.\d+", toolchain["pin"]):
        raise Unknown("descriptor_scope_mismatch")
    for key in ("host", "root"):
        if not isinstance(toolchain[key], str) or not toolchain[key] or len(toolchain[key]) > 4096:
            raise Unknown("descriptor_scope_mismatch")
    _validate_file_subject(toolchain["pin_subject"])
    subjects = toolchain["subjects"]
    if not isinstance(subjects, dict) or not 1 <= len(subjects) <= 64:
        raise Unknown("descriptor_scope_mismatch")
    for name, subject in subjects.items():
        if not isinstance(name, str) or not name or len(name) > 256:
            raise Unknown("descriptor_scope_mismatch")
        _validate_file_subject(subject)
    configuration = setup["configuration"]
    if not isinstance(configuration, list) or len(configuration) > 64:
        raise Unknown("descriptor_scope_mismatch")
    for subject in configuration:
        _validate_file_subject(subject)


def validate_descriptor(data):
    if not isinstance(data, dict):
        raise Unknown("descriptor_not_object")
    scope = data.get("scope")
    resources = data.get("resources")
    scope_fields = {"hostname", "platform", "working_directory", "worktree", "operation",
                    "cargo_argv_sha256", "resources", "jobs", "build_environment_sha256"}
    if isinstance(scope, dict) and scope.get("operation") == "clippy":
        scope_fields = scope_fields | {"clippy_setup"}
    if (not isinstance(scope, dict) or set(scope) != scope_fields
            or not isinstance(resources, dict) or set(resources) != set(RESOURCE_NAMES)
            or scope["resources"] != resources):
        raise Unknown("descriptor_scope_mismatch")
    _validate_clippy_setup(scope)
    if (scope["hostname"] != socket.gethostname() or scope["platform"] != sys.platform
            or type(scope["jobs"]) is not int or not 1 <= scope["jobs"] <= 4
            or type(data.get("pid")) is not int or data["pid"] <= 0
            or scope["operation"] not in ("build", "check", "test", "run", "bench", "doc", "clippy")):
        raise Unknown("descriptor_host_or_scope_invalid")
    for key in ("cargo_argv_sha256", "build_environment_sha256"):
        if not isinstance(scope[key], str) or not re.fullmatch(r"[0-9a-f]{64}", scope[key]):
            raise Unknown("descriptor_hash_invalid")
    for value in [data.get("worktree"), scope["worktree"], scope["working_directory"],
                  data.get("lease"), *resources.values()]:
        if not isinstance(value, str) or not value or len(value) > 4096:
            raise Unknown("descriptor_path_invalid")
        if any(part in (".", "..") for part in re.split(r"[/\\]", value)):
            raise Unknown("descriptor_path_not_canonical")
    paths = {key: native_path(value) for key, value in resources.items()}
    # Recover only the supplied base, then let the existing owner check the
    # host/common-dir/worktree slot. A consistent but sibling descriptor is not
    # authority for this checkout. No basename fallback or resource allocation.
    env = dict(os.environ)
    for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"):
        env.pop(name, None)
    env.update(DEVPLANE=str(paths["target"].parents[3]),
               CARGO_HOME=str(paths["cargo_home"]), TMPDIR=str(paths["temp"]))
    # The owner uses Git subprocesses whose failure diagnostics can expose native
    # paths. This single-threaded observer emits only its fixed public reasons.
    saved_stderr = os.dup(2)
    try:
        with open(os.devnull, "wb") as sink:
            os.dup2(sink.fileno(), 2)
            worktree, slot, expected = resource_plan(env)
    finally:
        os.dup2(saved_stderr, 2)
        os.close(saved_stderr)
    if (native_path(data["worktree"]) != worktree or native_path(scope["worktree"]) != worktree
            or native_path(scope["working_directory"]) != Path.cwd().resolve()
            or paths != expected or native_path(data["lease"]) != slot / "cargo-active"):
        raise Unknown("descriptor_native_identity_mismatch")
    return paths


def empty_record(reason):
    return {"schema_version": "cargo_storage_inventory.v1", "status": "NOT_PROVEN",
            "observation_kind": "historical_only", "descriptor_valid": False,
            "inventory_complete": False, "reasons": [reason], "resources": {},
            "combined": {"apparent_bytes": None, "allocated_bytes": None},
            "fingerprints": [], "fingerprints_complete": False,
            "admitted_before_sample": False, "same_lease_settlement": False,
            "build_delta_bytes": None, "peak_bytes": None, "open_unlinked_bytes": None,
            "wsl_guest_free_bytes": None, "vhdx_allocated_bytes": None,
            "host_backing_free_bytes": None}


def cargo_fingerprint_path(relative):
    """Recognize known Cargo layouts, not arbitrary JSON named fingerprint.

    Both layouts may coexist after an upgrade. Paths are relative to the
    observed target/build root; a target triple may precede the profile.
    This only selects observation inputs and grants no cleanup authority.
    """
    parts = relative.parts
    if relative.suffix != ".json":
        return False
    if len(parts) in (4, 5) and parts[-3] == ".fingerprint":
        return True
    return (len(parts) in (6, 7) and parts[-5] == "build"
            and parts[-2] == "fingerprint"
            and re.fullmatch(r"[0-9a-f]{16}", parts[-3]) is not None)


def inventory_roots(paths, *, max_entries=MAX_ENTRIES, max_depth=MAX_DEPTH,
                    max_seconds=MAX_SECONDS, max_fingerprints=MAX_FINGERPRINTS,
                    max_fingerprint_bytes=MAX_FINGERPRINT_BYTES,
                    collect_fingerprints=True, budget=None):
    """Measure named roots once, without asserting their origin or ownership."""
    record = {"resources": {}, "combined": {}, "fingerprints": []}
    record["byte_scope"] = "retained snapshot; apparent regular-file content; allocated includes directories"
    record["allocation_method"] = "st_blocks_times_512" if os.name == "posix" else "unavailable"
    record["combined"]["deduplication"] = "native_device_inode_across_all_resources"
    combined = {}
    budget = budget if budget is not None else ObservationBudget(max_entries, max_seconds)
    fingerprint_bytes = 0

    def charge_fingerprint_bytes(count):
        nonlocal fingerprint_bytes
        fingerprint_bytes += count

    fingerprint_reasons = set()
    for name, root in paths.items():
        errors = set()
        seen = {}
        directories = []
        stack = [(root, 0)]
        while stack:
            if budget.exhausted():
                errors.add("traversal_clipped")
                break
            path, depth = stack.pop()
            try:
                budget.take()
            except Unknown:
                errors.add("traversal_clipped")
                break
            try:
                # Check each ancestry again before opening: no traversal through
                # links or junctions. This is observation, not a hostile-FS sandbox.
                native_path(str(path))
                metadata = path.lstat()
                if linked(metadata):
                    raise Unknown("linked_entry")
                identity = (metadata.st_dev, metadata.st_ino)
                if not metadata.st_ino:
                    raise Unknown("file_identity_unavailable")
                if not (stat.S_ISREG(metadata.st_mode) or stat.S_ISDIR(metadata.st_mode)):
                    raise Unknown("unsupported_entry")
                size = metadata.st_size if stat.S_ISREG(metadata.st_mode) else 0
                allocated = getattr(metadata, "st_blocks", None)
                allocated = allocated * 512 if allocated is not None else None
                observation = (size, allocated, facts(metadata))
                if identity in seen:
                    # Deduplication cannot hide a changing inode. Keep the first
                    # partial observation, but invalidate totals on any mismatch.
                    if seen[identity] != observation:
                        errors.add("entry_changed")
                    continue
                seen[identity] = observation
                if identity in combined:
                    if combined[identity] != observation:
                        errors.add("entry_changed")
                else:
                    combined[identity] = observation
                if stat.S_ISDIR(metadata.st_mode):
                    directories.append((path, observation[2]))
                    if depth >= max_depth:
                        errors.add("depth_clipped")
                        continue
                    # Bound enumeration itself, not just the eventual stack walk.
                    with os.scandir(path) as children:
                        for child in children:
                            if (budget.entries + len(stack) >= budget.max_entries
                                    or time.monotonic() >= budget.deadline):
                                errors.add("traversal_clipped")
                                break
                            stack.append((Path(child.path), depth + 1))
                    if facts(path.lstat()) != facts(metadata):
                        errors.add("entry_changed")
                elif (collect_fingerprints and name in ("target", "build")
                      and cargo_fingerprint_path(path.relative_to(root))):
                    if (len(record["fingerprints"]) >= max_fingerprints
                            or fingerprint_bytes >= max_fingerprint_bytes):
                        fingerprint_reasons.add("fingerprints_clipped")
                        continue
                    try:
                        raw = read_file(path, max_fingerprint_bytes - fingerprint_bytes,
                                        charge_bytes=charge_fingerprint_bytes)
                        data = decode(raw)
                        if not isinstance(data, dict):
                            raise Unknown("fingerprint_not_object")
                        if any(key in data and (type(data[key]) is not int or not 0 <= data[key] < 2**64)
                               for key in HASH_FIELDS):
                            raise Unknown("fingerprint_scalar_invalid")
                        hashes = {key: str(data[key]) for key in HASH_FIELDS
                                  if type(data.get(key)) is int and 0 <= data[key] < 2**64}
                        record["fingerprints"].append({"resource": name,
                            "relative_path_sha256": digest(os.fsencode(path.relative_to(root))),
                            "sha256": digest(raw), "hashes": hashes})
                    except (OSError, Denied, Unknown, ValueError):
                        fingerprint_reasons.add("fingerprint_unavailable_or_clipped")
                    finally:
                        if facts(path.lstat()) != facts(metadata):
                            errors.add("entry_changed")
            except FileNotFoundError:
                errors.add("missing_or_changed_entry")
            except (OSError, Denied, Unknown, ValueError):
                errors.add("unreadable_linked_or_unsupported_entry")
        # A directory can change after its own listing without changing the
        # named root. Recheck every traversed directory under the same finite
        # budget before treating retained byte totals as complete.
        for directory, recorded in directories:
            try:
                budget.take()
            except Unknown:
                errors.add("traversal_clipped")
                break
            try:
                native_path(str(directory))
                current = directory.lstat()
                if (linked(current) or not stat.S_ISDIR(current.st_mode)
                        or not current.st_ino):
                    raise Unknown("directory_recheck_invalid")
                if facts(current) != recorded:
                    errors.add("entry_changed")
            except FileNotFoundError:
                errors.add("missing_or_changed_entry")
            except (OSError, Denied, Unknown, ValueError):
                errors.add("unreadable_linked_or_unsupported_entry")
        allocated_known = all(value[1] is not None for value in seen.values())
        complete = not errors
        record["resources"][name] = {
            "path_sha256": digest(os.fsencode(root)), "inventory_complete": complete,
            "reasons": sorted(errors), "entries_observed": len(seen),
            "observed_apparent_bytes": sum(value[0] for value in seen.values()),
            "observed_allocated_bytes": sum(value[1] for value in seen.values()) if allocated_known else None,
            "apparent_bytes": sum(value[0] for value in seen.values()) if complete else None,
            "allocated_bytes": sum(value[1] for value in seen.values()) if complete and allocated_known else None}
    complete = all(value["inventory_complete"] for value in record["resources"].values())
    allocated_known = all(value[1] is not None for value in combined.values())
    record["inventory_complete"] = complete
    record["combined"].update(
        observed_apparent_bytes=sum(value[0] for value in combined.values()),
        observed_allocated_bytes=sum(value[1] for value in combined.values()) if allocated_known else None,
        apparent_bytes=sum(value[0] for value in combined.values()) if complete else None,
        allocated_bytes=sum(value[1] for value in combined.values()) if complete and allocated_known else None)
    record["fingerprints_complete"] = complete and not fingerprint_reasons
    record["fingerprint_reasons"] = sorted(fingerprint_reasons)
    return record


def inventory(paths, **limits):
    record = empty_record("historical_inventory_does_not_qualify_build")
    record.update(inventory_roots({name: paths[name] for name in RESOURCE_NAMES}, **limits))
    return record


def read_git(command, env, budget):
    """Read one owned Git query without unbounded stdout, stderr, or waiting."""
    budget.remaining()
    limit = budget.git_bytes_left
    if limit <= 0:
        raise Unknown("git_output_clipped")
    process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0)
    output = bytearray()
    finished = threading.Event()
    failed = []

    def read():
        try:
            while len(output) <= limit:
                chunk = process.stdout.read(min(4096, limit + 1 - len(output)))
                if not chunk:
                    break
                output.extend(chunk)
        except OSError:
            failed.append(True)
        finally:
            finished.set()

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    try:
        if not finished.wait(budget.remaining()):
            raise Unknown("git_observation_timeout")
        budget.git_bytes_left -= min(len(output), limit)
        if len(output) > limit:
            raise Unknown("git_output_clipped")
        if failed:
            raise Unknown("git_output_unavailable")
        if process.wait(timeout=budget.remaining()) != 0:
            raise Unknown("git_observation_failed")
        return bytes(output)
    except subprocess.TimeoutExpired as error:
        raise Unknown("git_observation_timeout") from error
    finally:
        # These are our finite Git queries, not a process census or a lease owner.
        if process.poll() is None:
            process.kill()
        try:
            process.wait(timeout=0.25)
        except subprocess.TimeoutExpired:
            pass
        reader.join(timeout=0.25)
        if not reader.is_alive():
            process.stdout.close()


def registered_private_roots(raw, worktree, slot, budget):
    """Parse complete NUL-delimited Git records without lossy path conversion."""
    if not raw or not raw.endswith(b"\0\0"):
        raise Unknown("worktree_registration_invalid")
    roots = {}
    records = raw[:-2].split(b"\0\0")
    if len(records) > CENSUS_MEMBER_LIMIT:
        raise Unknown("worktree_registration_clipped")
    for record in records:
        budget.take()
        fields = record.split(b"\0")
        if not fields[0].startswith(b"worktree ") or len(fields[0]) <= 9:
            raise Unknown("worktree_registration_invalid")
        path = native_path(os.fsdecode(fields[0][9:]))
        keys = [field.split(b" ", 1)[0] for field in fields[1:]]
        if len(set(keys)) != len(keys) or not set(keys) <= {
                b"HEAD", b"branch", b"detached", b"bare", b"locked", b"prunable"}:
            raise Unknown("worktree_registration_invalid")
        for field in fields[1:]:
            key = field.split(b" ", 1)[0]
            if key in (b"detached", b"bare") and field != key:
                raise Unknown("worktree_registration_invalid")
            if key == b"branch" and (not field.startswith(b"branch refs/heads/")
                                     or len(field) == len(b"branch refs/heads/")):
                raise Unknown("worktree_registration_invalid")
        if b"bare" in keys:
            if b"HEAD" in keys or b"branch" in keys or b"detached" in keys:
                raise Unknown("worktree_registration_invalid")
            continue
        heads = [field[5:] for field in fields[1:] if field.startswith(b"HEAD ")]
        if len(heads) != 1 or not re.fullmatch(rb"[0-9a-f]{40}|[0-9a-f]{64}", heads[0]):
            raise Unknown("worktree_registration_invalid")
        if (b"branch" in keys) == (b"detached" in keys):
            raise Unknown("worktree_registration_invalid")
        root = private_worktree_root(slot, path)
        if root in roots:
            raise Unknown("worktree_registration_ambiguous")
        roots[root] = path
    if private_worktree_root(slot, worktree) not in roots:
        raise Unknown("current_worktree_not_registered")
    return roots


def census_row(path, presence):
    missing = presence == "missing"
    key = path.name if presence != "invalid" and re.fullmatch(r"[0-9a-f]{64}", path.name) else None
    return {"path_sha256": digest(os.fsencode(path)), "member_key": key, "registration": "unknown",
            "presence": presence, "allocation_ownership": "unknown",
            "reclaim_eligibility": "unknown", "inventory_complete": missing,
            "reasons": [], "entries_observed": 0, "observed_apparent_bytes": 0,
            "observed_allocated_bytes": 0, "apparent_bytes": 0 if missing else None,
            "allocated_bytes": 0 if missing else None}


def directory_snapshot(path):
    native_path(str(path))
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return None
    if linked(metadata) or not stat.S_ISDIR(metadata.st_mode) or not metadata.st_ino:
        raise Unknown("private_root_invalid")
    return facts(metadata)


def observe_current_slot(env=None, *, max_entries=MAX_ENTRIES, max_depth=MAX_DEPTH,
                         max_seconds=MAX_SECONDS, max_git_bytes=GIT_BYTE_LIMIT):
    """Census current-slot private members; registration never proves ownership."""
    budget = ObservationBudget(max_entries, max_seconds, max_git_bytes)
    record = {"schema_version": "cargo_private_storage_census.v1", "status": "NOT_PROVEN",
              "observation_kind": "read_only_snapshot", "registration_complete": False,
              "discovery_complete": False, "inventory_complete": False,
              "byte_scope": "private member contents and directories; excludes slot containers and shared state",
              "rows": [], "reasons": [], "entries_examined": 0,
              "combined": {"apparent_bytes": None, "allocated_bytes": None,
                           "observed_apparent_bytes": 0, "observed_allocated_bytes": 0,
                           "deduplication": "native_device_inode_across_all_members"}}
    env = read_only_git_environment(dict(os.environ) if env is None else env)
    reader = lambda command, environment: read_git(command, environment, budget)
    reasons = set()
    rows, roots, snapshots, registrations = {}, {}, {}, {}
    try:
        worktree, slot, _ = resource_plan(env, git_output_reader=reader)
        record["slot_path_sha256"] = digest(os.fsencode(slot))
        parent = slot / "worktrees"
        command = ["git", "-C", str(worktree), "worktree", "list", "--porcelain", "-z"]
        try:
            registration_raw = read_git(command, env, budget)
            registrations = registered_private_roots(registration_raw, worktree, slot, budget)
            record["registration_complete"] = True
        except (Unknown, Denied, OSError, subprocess.SubprocessError):
            reasons.add("worktree_registration_unavailable_or_invalid")

        parent_before = directory_snapshot(parent)
        record["discovery_complete"] = True
        if parent_before is not None:
            with os.scandir(parent) as children:
                for child in children:
                    if len(rows) >= CENSUS_MEMBER_LIMIT or budget.exhausted():
                        record["discovery_complete"] = False
                        reasons.add("member_discovery_clipped")
                        break
                    budget.take()
                    path = Path(child.path)
                    row = rows[path] = census_row(path, "invalid")
                    try:
                        if not re.fullmatch(r"[0-9a-f]{64}", child.name):
                            raise Unknown("private_member_name_invalid")
                        before = directory_snapshot(path)
                        if before is None:
                            raise Unknown("private_member_changed")
                        row["presence"] = "present"
                        row["member_key"] = path.name
                        roots[row["path_sha256"]] = path
                        snapshots[path] = before
                    except (Unknown, Denied, OSError):
                        row["reasons"].append("private_member_invalid_or_changed")
        for path in registrations:
            if path not in rows:
                if len(rows) >= CENSUS_MEMBER_LIMIT:
                    record["discovery_complete"] = False
                    reasons.add("member_discovery_clipped")
                    break
                rows[path] = census_row(path, "missing" if record["discovery_complete"] else "unknown")

        # One invocation shares all limits and one inode map across every member.
        measured = inventory_roots(roots, max_depth=max_depth,
                                   collect_fingerprints=False, budget=budget)
        record["allocation_method"] = measured["allocation_method"]
        record["combined"].update(measured["combined"])
        record["combined"]["deduplication"] = "native_device_inode_across_all_members"
        for path, row in rows.items():
            if row["path_sha256"] in measured["resources"]:
                row.update(measured["resources"][row["path_sha256"]])
        for path, before in snapshots.items():
            try:
                if directory_snapshot(path) != before:
                    raise Unknown("private_member_changed")
            except (Unknown, Denied, OSError):
                rows[path]["inventory_complete"] = False
                rows[path]["apparent_bytes"] = rows[path]["allocated_bytes"] = None
                rows[path]["reasons"].append("private_member_changed")
        if directory_snapshot(parent) != parent_before:
            record["discovery_complete"] = False
            reasons.add("member_discovery_changed")

        if record["registration_complete"]:
            try:
                after = read_git(command, env, budget)
                registered_private_roots(after, worktree, slot, budget)
                if after != registration_raw:
                    raise Unknown("worktree_registration_changed")
                final_worktree, final_slot, _ = resource_plan(env, git_output_reader=reader)
                if (final_worktree, final_slot) != (worktree, slot):
                    raise Unknown("repository_subject_changed")
            except (Unknown, Denied, OSError, subprocess.SubprocessError):
                record["registration_complete"] = False
                reasons.add("worktree_registration_changed_or_unavailable")
        if record["registration_complete"]:
            for path, row in rows.items():
                row["registration"] = "registered" if path in registrations else "unregistered"
    except (Unknown, Denied, OSError, ValueError, subprocess.SubprocessError):
        reasons.add("census_subject_or_observation_unavailable")
        record["discovery_complete"] = False
        record["registration_complete"] = False
    if not record["discovery_complete"]:
        for row in rows.values():
            if row["presence"] == "missing":
                row["presence"] = "unknown"
                row["inventory_complete"] = False
                row["apparent_bytes"] = row["allocated_bytes"] = None
                row["reasons"].append("member_absence_unproven")
    record["rows"] = sorted(rows.values(), key=lambda row: row["path_sha256"])
    complete = (record["registration_complete"] and record["discovery_complete"]
                and all(row["inventory_complete"] for row in rows.values()))
    record["inventory_complete"] = complete
    if complete:
        record["status"] = "OBSERVED"
    else:
        record["combined"]["apparent_bytes"] = record["combined"]["allocated_bytes"] = None
    record["entries_examined"] = budget.entries
    record["reasons"] = sorted(reasons)
    return record


def observe(filename):
    try:
        filename = Path(filename)
        if not filename.is_absolute():
            filename = Path.cwd() / filename
        raw = read_file(filename, DESCRIPTOR_LIMIT, descriptor=True)
        prefix = b"cargo-admitted resources: "
        data = decode(raw[len(prefix):] if raw.startswith(prefix) else raw)
        result = inventory(validate_descriptor(data))
        result["descriptor_valid"] = True
        result["descriptor_sha256"] = digest(raw)
        return result
    except Unknown as error:
        return empty_record(str(error))
    except (OSError, Denied, ValueError, TypeError, IndexError, subprocess.SubprocessError):
        return empty_record("descriptor_unavailable_or_invalid")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--resources-descriptor", "--descriptor", action="append")
    mode.add_argument("--current-slot", action="store_true", help="census this repository's private storage")
    parser.add_argument("--json", action="store_true", help="JSON is always emitted")
    args = parser.parse_args()
    if args.current_slot:
        result = observe_current_slot()
        print(json.dumps(result, sort_keys=True))
        return 0 if result["inventory_complete"] else 1
    result = (observe(args.resources_descriptor[0]) if len(args.resources_descriptor) == 1
              else empty_record("multiple_descriptor_inputs"))
    print(json.dumps(result, sort_keys=True))
    # A complete historical inventory is useful, but never a qualification pass.
    return 0 if result["descriptor_valid"] and result["inventory_complete"] else 1


if __name__ == "__main__":
    sys.exit(main())
