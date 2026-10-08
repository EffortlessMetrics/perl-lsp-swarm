#!/usr/bin/env python3
"""Bounded Cargo admission. No artifact deletion or ownership inference."""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import socket
import stat
import subprocess
import sys


class Denied(Exception):
    pass


def native_path(value):
    # Python on Windows does not understand Git Bash /c/... environment paths.
    if value.startswith(("\\\\", "//")):
        raise Denied("network/foreign storage is unsupported")
    if os.name == "nt" and value.startswith("/"):
        if re.match(r"^/[a-zA-Z]/", value):
            value = value[1] + ":" + value[2:]
        else:
            raise Denied("use an absolute native Windows path, not a WSL/MSYS path")
    if os.name != "nt" and (re.match(r"^[a-zA-Z]:", value) or value.startswith("/mnt/")):
        raise Denied("foreign Windows storage is unsupported; use host-local storage")
    path = Path(value).expanduser()
    if "{" in str(path) or "}" in str(path):
        raise Denied("Cargo template braces are unsupported in resource paths")
    if not path.is_absolute():
        raise Denied("storage paths must be absolute")
    # Reject links/junctions at every existing level rather than silently adopting
    # another resource's identity. Never delete or break a link.
    for part in (path, *path.parents):
        reparse = False
        try:
            reparse = bool(getattr(part.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))
        except FileNotFoundError:
            pass
        if part.is_symlink() or reparse:
            raise Denied("linked storage path is unsupported: " + str(part))
    return path.resolve()


def validate_args(args):
    allowed = {"build", "check", "test", "run", "bench", "doc"}
    if not args or args[0] not in allowed:
        raise Denied("expected build/check/test/run/bench/doc; aliases, +toolchain, clean and external commands are unsupported")
    for arg in args[1:]:
        if arg == "--":
            break  # arguments to the test/program, not Cargo configuration
        # Cargo/Clap accepts clusters such as -vj8. Refuse all joined short
        # tokens except pure verbosity, rather than duplicating its value grammar.
        if (arg.startswith("-") and not arg.startswith("--") and len(arg) > 2
                and not re.fullmatch(r"-v{2,}", arg)):
            raise Denied("joined short options/values are unsupported; spell them separately: " + arg)
        if (arg.startswith(("--config", "--target-dir", "--manifest-path", "--jobs", "-j", "-Z", "-C", "--lockfile-path", "--out-dir", "--artifact-dir", "--build-dir"))
                or arg.startswith("+")):
            raise Denied("configuration/path/job override is unsupported: " + arg)


def check_capacity(paths, env):
    policy = env.get("CARGO_STORAGE_POLICY", "percent")
    if policy not in ("percent", "byte-budget"):
        raise Denied("unknown CARGO_STORAGE_POLICY")
    try:
        reserve_gib = float(env.get("MIN_FREE_GB", "40"))
        floor = reserve_gib * 1024 ** 3
        if not math.isfinite(floor) or floor <= 0:
            raise ValueError()
        report = {"policy": policy, "reserve_bytes": floor}
        if policy == "byte-budget":
            growth = float(env.get("CARGO_EXPECTED_GROWTH_GB", "")) * 1024 ** 3
            evidence = env.get("CARGO_STORAGE_BUDGET_EVIDENCE", "").strip()
            if reserve_gib < 40 or not math.isfinite(growth) or growth <= 0 or not evidence:
                raise ValueError()
            required = floor + growth
            if not math.isfinite(required):
                raise ValueError()
            report.update(expected_total_growth_bytes=growth, evidence_reference=evidence)
        else:
            ceiling = float(env.get("MAX_USED_PCT", "85"))
            if not math.isfinite(ceiling) or not 0 < ceiling <= 100:
                raise ValueError()
            required = floor
            report["max_used_pct"] = ceiling
    except ValueError:
        raise Denied("invalid capacity policy inputs; byte-budget requires reserve >=40 GiB, positive finite total growth and an evidence reference")
    observations = []
    for path in paths:
        ancestor = path
        while not ancestor.exists():
            ancestor = ancestor.parent
        usage = shutil.disk_usage(ancestor)
        # The same TOTAL growth bound is checked independently at every
        # destination. Never sum it for paths sharing a volume. Applying the
        # entire bound on different volumes is intentionally conservative.
        if usage.free < required or (policy == "percent" and
                (usage.total - usage.free) * 100 / usage.total >= ceiling):
            raise Denied("insufficient disk headroom at " + str(path))
        observations.append({"path": str(path), "observed_ancestor": str(ancestor),
                             "free_bytes": usage.free, "required_free_bytes": required,
                             "headroom_bytes": usage.free - required})
    report["destinations"] = observations
    return report


def git_path(*args, env=None):
    # Read bytes: text mode also translates CR/LF that may belong to a POSIX
    # filename. Git emits one terminal LF; preserve every preceding path byte.
    output = subprocess.check_output(["git", "rev-parse", *args], env=env)
    if not output.endswith(b"\n"):
        raise Denied("Git path output is missing its terminal newline")
    return native_path(os.fsdecode(output[:-1]))


def resource_plan(env):
    # Cargo uses the invocation directory, not Git's repository-location
    # overrides. Refuse an ambiguous subject instead of silently retargeting it.
    for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"):
        if name in env:
            raise Denied(name + " repository-location override is unsupported; unset it and invoke from the intended worktree")
    worktree = git_path("--show-toplevel", env=env)
    common = git_path("--path-format=absolute", "--git-common-dir", env=env)
    identity = str(common).casefold() if os.name == "nt" else str(common)
    key = hashlib.sha256(os.fsencode(identity)).hexdigest()[:16]
    base = native_path(env.get("DEVPLANE", str(Path.home() / ".cache" / "devplane")))
    # Keep one admission/lease domain per common repository and OS/host.
    # Cargo fingerprints deliberately omit the workspace absolute path, so
    # serializing divergent worktrees does not make their build state compatible.
    slot = base / (socket.gethostname() + "-" + sys.platform + "-" + key)
    # native_path already resolves the actual root; do not invent Unicode/case
    # equivalence between distinct filesystem paths. fsencode is lossless on POSIX.
    worktree_key = hashlib.sha256(os.fsencode(worktree)).hexdigest()
    private = slot / "worktrees" / worktree_key
    target, build = private / "target", private / "build"
    for name, expected in (("CARGO_TARGET_DIR", target), ("CARGO_BUILD_BUILD_DIR", build)):
        if env.get(name) and native_path(env[name]) != expected:
            raise Denied(name + " override unsupported; use this worktree's private target/build paths")
    paths = {"target": target, "build": build,
             "cargo_home": native_path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))),
             "temp": native_path(env.get("TMPDIR", env.get("TEMP", str(slot / "tmp"))))}
    # Windows programs commonly use TEMP/TMP rather than TMPDIR: unify all three.
    for path in (slot, *paths.values()):
        native_path(str(path))
    return worktree, slot, paths


def main(args=None):
    args = sys.argv[1:] if args is None else args
    try:
        validate_args(args)
        env = os.environ.copy()
        env["RUSTUP_AUTO_INSTALL"] = "0"
        worktree, slot, paths = resource_plan(env)
        admission = check_capacity([slot, *paths.values()], env)  # before any mkdir/Cargo invocation
        if env.get("RUSTC_WRAPPER") or env.get("RUSTC_WORKSPACE_WRAPPER"):
            raise Denied("compiler wrappers have unverified storage; use a separately admitted route")
        jobs = int(env.get("CARGO_BUILD_JOBS", "2"))
        if not 1 <= jobs <= 4:
            raise Denied("CARGO_BUILD_JOBS must be between 1 and 4")
        slot.mkdir(parents=True, exist_ok=True)
        lock = slot / "cargo-active"
        try:
            lock.mkdir()
        except FileExistsError:
            raise Denied("slot is active or awaits owner verification: " + str(lock))
        # Parent interruption/failed spawn retain the directory lease. Recognized
        # child statuses govern release below; no PID-based stale-lock stealing.
        completed = False
        try:
            for path in paths.values():
                path.mkdir(parents=True, exist_ok=True)
            env.update(CARGO_TARGET_DIR=str(paths["target"]),
                       CARGO_BUILD_BUILD_DIR=str(paths["build"]),
                       CARGO_HOME=str(paths["cargo_home"]), CARGO_INCREMENTAL="0",
                       CARGO_BUILD_JOBS=str(jobs), TMPDIR=str(paths["temp"]),
                       TEMP=str(paths["temp"]), TMP=str(paths["temp"]))
            descriptor = {"worktree": str(worktree), "resources": {k: str(v) for k, v in paths.items()},
                          "lease": str(lock), "pid": os.getpid(), "admission": admission, "disposition": "retained with reason: reusable bounded slot"}
            print("cargo-admitted resources: " + json.dumps(descriptor), file=sys.stderr, flush=True)
            command = ["cargo", "--config", "unstable.unstable-options=false",
                       "--config", "build.build-dir=" + json.dumps(str(paths["build"])),
                       "--config", 'build.rustc-wrapper=""',
                       "--config", 'build.rustc-workspace-wrapper=""',
                       *args[:1], "--target-dir", str(paths["target"]), *args[1:]]
            # Cargo's [env] force=true can override process environment for
            # rustc/build scripts. Override those config entries as well.
            for name in ("TEMP", "TMP", "TMPDIR", "RUSTUP_AUTO_INSTALL"):
                child_value = "0" if name == "RUSTUP_AUTO_INSTALL" else str(paths["temp"])
                for field, value in (("value", json.dumps(child_value)), ("force", "true"), ("relative", "false")):
                    command[1:1] = ["--config", "env." + name + "." + field + "=" + value]
            result = subprocess.call(command, env=env)
            # Cargo uses 101 for ordinary failure and may also panic with 101.
            # Retain for signals/NT termination/unfamiliar exits, but no exit
            # status proves all descendants ended: root verifies other consumers.
            completed = result in (0, 101)
            return result
        finally:
            if completed:
                lock.rmdir()
            else:
                print("cargo-admitted: lease retained; root must verify all consumers before release", file=sys.stderr)
    except (Denied, OSError, ValueError, subprocess.CalledProcessError) as error:
        print("cargo-admitted: DENY: " + str(error), file=sys.stderr)
        return 75


if __name__ == "__main__":
    sys.exit(main())
