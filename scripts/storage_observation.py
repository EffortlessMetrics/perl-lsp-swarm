#!/usr/bin/env python3
"""Bounded historical inventory of an admitted resource descriptor, never admission.

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
import time

from cargo_admitted import Denied, native_path, resource_plan

DESCRIPTOR_LIMIT = 65536
RESOURCE_NAMES = ("target", "build", "cargo_home", "temp")
MAX_ENTRIES = 10000
MAX_DEPTH = 12
MAX_SECONDS = 2.0
MAX_FINGERPRINTS = 64
MAX_FINGERPRINT_BYTES = 65536
HASH_FIELDS = ("rustc", "target", "profile", "path", "config", "compile_kind")


class Unknown(Exception):
    """Fixed public reason, never a native path or arbitrary input payload."""


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


def inventory(paths, *, max_entries=MAX_ENTRIES, max_depth=MAX_DEPTH,
              max_seconds=MAX_SECONDS, max_fingerprints=MAX_FINGERPRINTS,
              max_fingerprint_bytes=MAX_FINGERPRINT_BYTES):
    record = empty_record("historical_inventory_does_not_qualify_build")
    record["descriptor_valid"] = True
    record["byte_scope"] = "retained snapshot; apparent regular-file content; allocated includes directories"
    record["allocation_method"] = "st_blocks_times_512" if os.name == "posix" else "unavailable"
    record["combined"]["deduplication"] = "native_device_inode_across_all_resources"
    combined = {}
    entries = 0
    fingerprint_bytes = 0

    def charge_fingerprint_bytes(count):
        nonlocal fingerprint_bytes
        fingerprint_bytes += count

    deadline = time.monotonic() + max_seconds
    fingerprint_reasons = set()
    for name in RESOURCE_NAMES:
        root = paths[name]
        errors = set()
        seen = {}
        stack = [(root, 0)]
        while stack:
            if entries >= max_entries or time.monotonic() >= deadline:
                errors.add("traversal_clipped")
                break
            path, depth = stack.pop()
            entries += 1
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
                if identity in combined and combined[identity] != observation:
                    errors.add("entry_changed")
                combined[identity] = observation
                if stat.S_ISDIR(metadata.st_mode):
                    if depth >= max_depth:
                        errors.add("depth_clipped")
                        continue
                    # Bound enumeration itself, not just the eventual stack walk.
                    with os.scandir(path) as children:
                        for child in children:
                            if entries + len(stack) >= max_entries or time.monotonic() >= deadline:
                                errors.add("traversal_clipped")
                                break
                            stack.append((Path(child.path), depth + 1))
                    if facts(path.lstat()) != facts(metadata):
                        errors.add("entry_changed")
                elif (name in ("target", "build") and ".fingerprint" in path.relative_to(root).parts
                      and path.suffix == ".json"):
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


def observe(filename):
    try:
        filename = Path(filename)
        if not filename.is_absolute():
            filename = Path.cwd() / filename
        raw = read_file(filename, DESCRIPTOR_LIMIT, descriptor=True)
        prefix = b"cargo-admitted resources: "
        data = decode(raw[len(prefix):] if raw.startswith(prefix) else raw)
        result = inventory(validate_descriptor(data))
        result["descriptor_sha256"] = digest(raw)
        return result
    except Unknown as error:
        return empty_record(str(error))
    except (OSError, Denied, ValueError, TypeError, IndexError, subprocess.SubprocessError):
        return empty_record("descriptor_unavailable_or_invalid")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resources-descriptor", "--descriptor", required=True, action="append")
    parser.add_argument("--json", action="store_true", help="JSON is always emitted")
    args = parser.parse_args()
    result = (observe(args.resources_descriptor[0]) if len(args.resources_descriptor) == 1
              else empty_record("multiple_descriptor_inputs"))
    print(json.dumps(result, sort_keys=True))
    # A complete historical inventory is useful, but never a qualification pass.
    return 0 if result["descriptor_valid"] and result["inventory_complete"] else 1


if __name__ == "__main__":
    sys.exit(main())
